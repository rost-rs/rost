//! # Live reloading: [`Reloader<T>`]
//!
//! A `Reloader<T>` owns a typed value loaded from a file and keeps it current while the
//! program runs:
//!
//! - **Readers never block.** [`Reloader::get`] is a lock-free snapshot (`arc-swap`); a reader
//!   sees the old value or the new one, never a half-written one.
//! - **A broken edit changes nothing.** The new text is parsed and validated off to the side;
//!   only a valid value is published. The error (with line and column) goes to
//!   [`Reloader::last_error`] and the [`Reloader::on_error`] callbacks.
//! - **Bursts collapse.** Editors that save by temp file plus rename, or by truncate plus
//!   write, produce several events; they are debounced and a file whose content did not
//!   change is not reloaded.
//! - **One thread for everything.** All reloaders share one dispatcher thread fed by the
//!   operating system's file events (`notify`), with a slow stat-based poll as a safety net
//!   (and as the only mechanism if events are unavailable).
//!
//! Callbacks run on the dispatcher thread: keep them short, and do not move a clone of the
//! `Reloader` into its own callback (that keeps it alive forever).

use std::collections::HashSet;
use std::fmt;
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, Weak};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use arc_swap::ArcSwap;
use notify::{RecursiveMode, Watcher};

/// Quiet time after the last event before a file is read.
const DEBOUNCE: Duration = Duration::from_millis(20);
/// A burst of events is cut off after this long, so a busy file still reloads.
const MAX_BURST: Duration = Duration::from_millis(250);
/// Stat poll period when file events work.
const SAFETY_POLL: Duration = Duration::from_secs(1);
/// Stat poll period when they do not.
const FALLBACK_POLL: Duration = Duration::from_millis(100);

/// Why a reload was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReloadError {
    /// The file that failed (for I/O errors) or the watched main file.
    pub path: PathBuf,
    /// Human readable reason, without the position prefix.
    pub message: String,
    /// 1-based line of the problem, when known.
    pub line: Option<u32>,
    /// 1-based column of the problem, when known.
    pub column: Option<u32>,
}

impl ReloadError {
    /// An error with no position.
    pub fn new(message: impl Into<String>) -> Self {
        Self { path: PathBuf::new(), message: message.into(), line: None, column: None }
    }

    /// Wraps any error, recovering a leading `line:column: ` position from its text
    /// (what [`ParseError`](crate::ParseError) and the typed loaders print).
    pub fn from_display(err: impl fmt::Display) -> Self {
        let text = err.to_string();
        let mut parts = text.splitn(3, ':');
        if let (Some(l), Some(c), Some(rest)) = (parts.next(), parts.next(), parts.next())
            && let (Ok(line), Ok(column)) = (l.trim().parse(), c.trim().parse())
        {
            return Self {
                path: PathBuf::new(),
                message: rest.trim_start().to_string(),
                line: Some(line),
                column: Some(column),
            };
        }
        Self::new(text)
    }

    fn at(mut self, path: &Path) -> Self {
        if self.path.as_os_str().is_empty() {
            self.path = path.to_path_buf();
        }
        self
    }
}

impl fmt::Display for ReloadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.path.display())?;
        match (self.line, self.column) {
            (Some(l), Some(c)) => write!(f, ":{l}:{c}")?,
            (Some(l), None) => write!(f, ":{l}")?,
            _ => {}
        }
        write!(f, ": {}", self.message)
    }
}

impl std::error::Error for ReloadError {}

type Loader<T> = Box<dyn Fn(&str) -> Result<T, ReloadError> + Send + Sync>;
type ChangeCb<T> = Arc<dyn Fn(&Arc<T>) + Send + Sync>;
type ErrorCb = Arc<dyn Fn(&ReloadError) + Send + Sync>;

#[cfg(feature = "serde")]
type PathCb = Arc<dyn Fn(&crate::diff::Change) + Send + Sync>;

#[derive(Default)]
struct State {
    /// `(modified, len)` per watched file at the last look.
    stat: Vec<Option<(SystemTime, u64)>>,
    /// Hash of the content of every watched file at the last load attempt.
    fingerprint: u64,
    last_error: Option<ReloadError>,
    /// The main file's text at the last successful load (for path diffs).
    #[cfg(feature = "serde")]
    text: Option<Arc<str>>,
    /// The document at the last successful load, kept only while path observers exist.
    #[cfg(feature = "serde")]
    doc: Option<crate::value::Value>,
}

struct Inner<T> {
    /// Main file first, then the files it depends on.
    paths: Vec<PathBuf>,
    loader: Loader<T>,
    current: ArcSwap<T>,
    version: AtomicU64,
    errors: AtomicU64,
    state: Mutex<State>,
    published: Condvar,
    on_change: Mutex<Vec<ChangeCb<T>>>,
    on_error: Mutex<Vec<ErrorCb>>,
    #[cfg(feature = "serde")]
    on_path: Mutex<Vec<(String, PathCb)>>,
}

fn lock<M>(m: &Mutex<M>) -> MutexGuard<'_, M> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn stat_of(path: &Path) -> Option<(SystemTime, u64)> {
    let meta = fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

/// Reads every watched file; `Ok(None)` when one is momentarily missing (a rename save).
fn read_all(paths: &[PathBuf]) -> Result<Option<(String, u64)>, ReloadError> {
    let mut hasher = DefaultHasher::new();
    let mut main = String::new();
    for (i, path) in paths.iter().enumerate() {
        let bytes = match fs::read(path) {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(ReloadError::new(e.to_string()).at(path)),
        };
        bytes.len().hash(&mut hasher);
        bytes.hash(&mut hasher);
        if i == 0 {
            main = String::from_utf8(bytes)
                .map_err(|_| ReloadError::new("file is not valid UTF-8").at(path))?;
        }
    }
    Ok(Some((main, hasher.finish())))
}

fn guarded<R>(what: &str, f: impl FnOnce() -> R) -> Result<R, ReloadError> {
    catch_unwind(AssertUnwindSafe(f)).map_err(|_| ReloadError::new(format!("{what} panicked")))
}

impl<T: Send + Sync + 'static> Inner<T> {
    /// Looks at the files and, if they changed, loads and publishes the new value.
    ///
    /// `force` skips the stat check (a file event already said something happened); an
    /// unchanged content hash still skips the load.
    fn refresh(&self, force: bool) {
        let stats: Vec<_> = self.paths.iter().map(|p| stat_of(p)).collect();
        if !force && lock(&self.state).stat == stats {
            return;
        }
        let (text, fingerprint) = match read_all(&self.paths) {
            Ok(Some(read)) => read,
            // A file is missing for an instant while an editor swaps it in: wait for the
            // next event or poll instead of calling it an error.
            Ok(None) => return,
            Err(err) => return self.reject(stats, None, err),
        };
        {
            let mut state = lock(&self.state);
            state.stat = stats.clone();
            if state.fingerprint == fingerprint {
                return;
            }
        }
        match guarded("the loader", || (self.loader)(&text)) {
            Ok(Ok(value)) => self.publish(stats, fingerprint, text, value),
            Ok(Err(err)) | Err(err) => self.reject(stats, Some(fingerprint), err.at(&self.paths[0])),
        }
    }

    fn reject(&self, stats: Vec<Option<(SystemTime, u64)>>, fingerprint: Option<u64>, err: ReloadError) {
        {
            let mut state = lock(&self.state);
            state.stat = stats;
            if let Some(fp) = fingerprint {
                state.fingerprint = fp;
            }
            state.last_error = Some(err.clone());
        }
        self.errors.fetch_add(1, Ordering::AcqRel);
        let callbacks = lock(&self.on_error).clone();
        for cb in callbacks {
            let _ = guarded("an error callback", || cb(&err));
        }
        self.published.notify_all();
    }

    fn publish(&self, stats: Vec<Option<(SystemTime, u64)>>, fingerprint: u64, text: String, value: T) {
        let value = Arc::new(value);
        #[cfg(feature = "serde")]
        let changes = self.diff_for_observers(&text);
        {
            let mut state = lock(&self.state);
            state.stat = stats;
            state.fingerprint = fingerprint;
            state.last_error = None;
            #[cfg(feature = "serde")]
            {
                state.text = Some(text.into());
            }
            #[cfg(not(feature = "serde"))]
            drop(text);
        }
        self.current.store(value.clone());
        self.version.fetch_add(1, Ordering::AcqRel);
        self.published.notify_all();

        let callbacks = lock(&self.on_change).clone();
        for cb in callbacks {
            let _ = guarded("a change callback", || cb(&value));
        }
        #[cfg(feature = "serde")]
        {
            let observers = lock(&self.on_path).clone();
            for change in &changes {
                for (watched, cb) in &observers {
                    if crate::diff::covers(watched, &change.path) {
                        let _ = guarded("a path callback", || cb(change));
                    }
                }
            }
        }
    }

    /// Parses the new text generically and diffs it with the previous document, only
    /// while somebody listens to paths.
    #[cfg(feature = "serde")]
    fn diff_for_observers(&self, text: &str) -> Vec<crate::diff::Change> {
        if lock(&self.on_path).is_empty() {
            return Vec::new();
        }
        let Ok(new) = crate::from_str::<crate::value::Value>(text) else {
            return Vec::new();
        };
        let mut state = lock(&self.state);
        let changes = match &state.doc {
            Some(old) => crate::diff::diff(old, &new),
            None => Vec::new(),
        };
        state.doc = Some(new);
        changes
    }
}

/// What the dispatcher thread needs from a reloader, independent of its value type.
trait Target: Send + Sync {
    fn paths(&self) -> &[PathBuf];
    fn refresh(&self, force: bool);
}

impl<T: Send + Sync + 'static> Target for Inner<T> {
    fn paths(&self) -> &[PathBuf] {
        &self.paths
    }
    fn refresh(&self, force: bool) {
        Inner::refresh(self, force)
    }
}

enum Msg {
    Event(Vec<PathBuf>),
}

struct Hub {
    targets: Mutex<Vec<Weak<dyn Target>>>,
    watcher: Mutex<Option<notify::RecommendedWatcher>>,
    dirs: Mutex<HashSet<PathBuf>>,
}

impl Hub {
    fn global() -> &'static Hub {
        static HUB: OnceLock<Hub> = OnceLock::new();
        HUB.get_or_init(|| {
            let (tx, rx) = channel::<Msg>();
            let watcher = Self::start_watcher(tx);
            let events = watcher.is_some();
            let hub = Hub {
                targets: Mutex::new(Vec::new()),
                watcher: Mutex::new(watcher),
                dirs: Mutex::new(HashSet::new()),
            };
            thread::Builder::new()
                .name("rost-reload".into())
                .spawn(move || Hub::run(rx, events))
                .expect("spawn the rost-reload thread");
            hub
        })
    }

    fn start_watcher(tx: Sender<Msg>) -> Option<notify::RecommendedWatcher> {
        notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(event) = res
                && !matches!(event.kind, notify::EventKind::Access(_))
            {
                let _ = tx.send(Msg::Event(event.paths));
            }
        })
        .ok()
    }

    fn register(&self, target: &Arc<dyn Target>) {
        if let Some(watcher) = lock(&self.watcher).as_mut() {
            let mut dirs = lock(&self.dirs);
            for path in target.paths() {
                let dir = path.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
                if !dirs.contains(&dir) && watcher.watch(&dir, RecursiveMode::NonRecursive).is_ok() {
                    dirs.insert(dir);
                }
            }
        }
        lock(&self.targets).push(Arc::downgrade(target));
    }

    fn live(&self) -> Vec<Arc<dyn Target>> {
        let mut targets = lock(&self.targets);
        targets.retain(|w| w.strong_count() > 0);
        targets.iter().filter_map(Weak::upgrade).collect()
    }

    fn run(rx: std::sync::mpsc::Receiver<Msg>, events: bool) {
        let poll = if events { SAFETY_POLL } else { FALLBACK_POLL };
        let mut last_poll = Instant::now();
        loop {
            match rx.recv_timeout(poll) {
                Ok(Msg::Event(first)) => {
                    let mut changed: HashSet<PathBuf> = first.into_iter().collect();
                    let started = Instant::now();
                    while started.elapsed() < MAX_BURST {
                        match rx.recv_timeout(DEBOUNCE) {
                            Ok(Msg::Event(more)) => changed.extend(more),
                            Err(RecvTimeoutError::Timeout) => break,
                            Err(RecvTimeoutError::Disconnected) => return,
                        }
                    }
                    for target in Hub::global().live() {
                        if target.paths().iter().any(|p| changed.contains(p)) {
                            target.refresh(true);
                        }
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            if last_poll.elapsed() >= poll {
                last_poll = Instant::now();
                for target in Hub::global().live() {
                    target.refresh(false);
                }
            }
        }
    }
}

/// Builder for a [`Reloader`] that also depends on other files.
pub struct ReloaderBuilder<T> {
    path: PathBuf,
    deps: Vec<PathBuf>,
    loader: Loader<T>,
}

impl<T: Send + Sync + 'static> ReloaderBuilder<T> {
    /// Also reloads when `path` changes (an imported file, a shared palette). The loader
    /// still receives only the main file's text.
    pub fn depends_on(mut self, path: impl AsRef<Path>) -> Self {
        self.deps.push(path.as_ref().to_path_buf());
        self
    }

    /// Loads the file once (failing if it is unreadable or invalid) and starts watching it.
    pub fn build(self) -> Result<Reloader<T>, ReloadError> {
        let canon = |p: &Path| p.canonicalize().map_err(|e| ReloadError::new(e.to_string()).at(p));
        let mut paths = vec![canon(&self.path)?];
        for dep in &self.deps {
            paths.push(canon(dep)?);
        }
        let stats: Vec<_> = paths.iter().map(|p| stat_of(p)).collect();
        let (text, fingerprint) = read_all(&paths)?
            .ok_or_else(|| ReloadError::new("file disappeared while loading").at(&paths[0]))?;
        let first = guarded("the loader", || (self.loader)(&text))
            .and_then(|r| r)
            .map_err(|e| e.at(&paths[0]))?;
        let inner = Arc::new(Inner {
            paths,
            loader: self.loader,
            current: ArcSwap::from_pointee(first),
            version: AtomicU64::new(1),
            errors: AtomicU64::new(0),
            state: Mutex::new(State {
                stat: stats,
                fingerprint,
                #[cfg(feature = "serde")]
                text: Some(text.into()),
                ..State::default()
            }),
            published: Condvar::new(),
            on_change: Mutex::new(Vec::new()),
            on_error: Mutex::new(Vec::new()),
            #[cfg(feature = "serde")]
            on_path: Mutex::new(Vec::new()),
        });
        let target: Arc<dyn Target> = inner.clone();
        Hub::global().register(&target);
        // Anything saved between the first read and the registration is caught here.
        inner.refresh(false);
        Ok(Reloader { inner })
    }
}

/// A typed value that follows its file. Cloning shares the same live value.
pub struct Reloader<T> {
    inner: Arc<Inner<T>>,
}

impl<T> Clone for Reloader<T> {
    fn clone(&self) -> Self {
        Self { inner: self.inner.clone() }
    }
}

impl<T: fmt::Debug> fmt::Debug for Reloader<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Reloader")
            .field("path", &self.inner.paths[0])
            .field("version", &self.inner.version.load(Ordering::Acquire))
            .field("value", &self.inner.current.load())
            .finish()
    }
}

impl<T: Send + Sync + 'static> Reloader<T> {
    /// Starts building a reloader that turns the file's text into a `T` with `loader`.
    pub fn builder<F>(path: impl AsRef<Path>, loader: F) -> ReloaderBuilder<T>
    where
        F: Fn(&str) -> Result<T, ReloadError> + Send + Sync + 'static,
    {
        ReloaderBuilder { path: path.as_ref().to_path_buf(), deps: Vec::new(), loader: Box::new(loader) }
    }

    /// Loads `path` with `loader` and keeps it current.
    pub fn watch<F>(path: impl AsRef<Path>, loader: F) -> Result<Self, ReloadError>
    where
        F: Fn(&str) -> Result<T, ReloadError> + Send + Sync + 'static,
    {
        Self::builder(path, loader).build()
    }

    /// The current value. Lock-free; never blocks a writer or sees a partial value.
    pub fn get(&self) -> Arc<T> {
        self.inner.current.load_full()
    }

    /// Number of values published so far (starts at 1 for the initial load).
    pub fn version(&self) -> u64 {
        self.inner.version.load(Ordering::Acquire)
    }

    /// Number of edits rejected so far.
    pub fn error_count(&self) -> u64 {
        self.inner.errors.load(Ordering::Acquire)
    }

    /// The reason the latest edit was rejected, or `None` if the file is currently valid.
    pub fn last_error(&self) -> Option<ReloadError> {
        lock(&self.inner.state).last_error.clone()
    }

    /// The watched file.
    pub fn path(&self) -> &Path {
        &self.inner.paths[0]
    }

    /// Re-reads the file now instead of waiting for an event.
    pub fn reload_now(&self) {
        self.inner.refresh(true);
    }

    /// Runs `cb` with the new value after every successful reload.
    pub fn on_change(&self, cb: impl Fn(&Arc<T>) + Send + Sync + 'static) {
        lock(&self.inner.on_change).push(Arc::new(cb));
    }

    /// Runs `cb` whenever an edit is rejected.
    pub fn on_error(&self, cb: impl Fn(&ReloadError) + Send + Sync + 'static) {
        lock(&self.inner.on_error).push(Arc::new(cb));
    }

    /// Blocks until [`version`](Self::version) is at least `version` or the timeout passes.
    pub fn wait_for_version(&self, version: u64, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut state = lock(&self.inner.state);
        loop {
            if self.version() >= version {
                return true;
            }
            let Some(left) = deadline.checked_duration_since(Instant::now()) else {
                return false;
            };
            state = self.inner.published.wait_timeout(state, left).unwrap_or_else(|e| e.into_inner()).0;
        }
    }

    /// Blocks until the number of rejected edits is at least `errors` or the timeout passes.
    pub fn wait_for_errors(&self, errors: u64, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut state = lock(&self.inner.state);
        loop {
            if self.error_count() >= errors {
                return true;
            }
            let Some(left) = deadline.checked_duration_since(Instant::now()) else {
                return false;
            };
            state = self.inner.published.wait_timeout(state, left).unwrap_or_else(|e| e.into_inner()).0;
        }
    }
}

#[cfg(feature = "serde")]
impl<T: Send + Sync + 'static> Reloader<T> {
    /// Runs `cb` for every value under `path` (`window.width`, `items[2]`, or a parent such
    /// as `window`) that differs after a reload, so an app can update only what changed.
    pub fn on_change_path(&self, path: &str, cb: impl Fn(&crate::diff::Change) + Send + Sync + 'static) {
        let mut observers = lock(&self.inner.on_path);
        if observers.is_empty() {
            let mut state = lock(&self.inner.state);
            state.doc = state.text.as_deref().and_then(|t| crate::from_str(t).ok());
        }
        observers.push((path.to_string(), Arc::new(cb)));
    }
}

#[cfg(feature = "serde")]
impl<T: serde::de::DeserializeOwned + Send + Sync + 'static> Reloader<T> {
    /// Loads `path` into a `T` through serde and keeps it current.
    pub fn watch_serde(path: impl AsRef<Path>) -> Result<Self, ReloadError> {
        Self::watch(path, |text| crate::from_str(text).map_err(ReloadError::from_display))
    }
}

#[cfg(feature = "derive")]
impl<T: for<'a> crate::Load<'a> + Send + Sync + 'static> Reloader<T> {
    /// Loads `path` into a `T` through its generated loader (`#[derive(Rost)]`) and keeps
    /// it current.
    pub fn watch_load(path: impl AsRef<Path>) -> Result<Self, ReloadError> {
        Self::watch(path, |text| crate::load(text).map_err(ReloadError::from_display))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stat_poll_alone_finds_a_change_and_skips_an_unchanged_file() {
        let dir = std::env::temp_dir().join(format!("rost-reload-unit-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.rost");
        fs::write(&path, "one").unwrap();
        let loads = Arc::new(AtomicU64::new(0));
        let l = loads.clone();
        let r = Reloader::watch(&path, move |t| {
            l.fetch_add(1, Ordering::SeqCst);
            Ok(t.len())
        })
        .unwrap();
        let before = loads.load(Ordering::SeqCst);
        r.inner.refresh(false);
        assert_eq!(loads.load(Ordering::SeqCst), before, "unchanged file: no load");
        fs::write(&path, "three").unwrap();
        r.inner.refresh(false);
        assert_eq!(*r.get(), 5);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn from_display_recovers_the_position() {
        let e = ReloadError::from_display("12:7: expected a number");
        assert_eq!((e.line, e.column), (Some(12), Some(7)));
        assert_eq!(e.message, "expected a number");
        let e = ReloadError::from_display("no position here: really");
        assert_eq!((e.line, e.column), (None, None));
        assert_eq!(e.message, "no position here: really");
    }
}
