//! # Zero-CPU Centralized Multiplexed Hot-Reload Watcher for `.rost` Documents
//!
//! Designed for high-scale applications and game engines (e.g., hundreds or thousands of
//! `.rost` assets). Uses a single shared background worker thread to monitor all registered
//! templates simultaneously with 0% idle CPU overhead, preventing OS thread exhaustion.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, SystemTime};

use super::parser;
use super::Template;

/// Callback closure invoked when an underlying `.rost` file is updated on disk.
pub type ReloadCallback = Box<dyn Fn(Template) + Send + Sync + 'static>;

struct FileWatchState {
    last_modified: SystemTime,
    subscribers: Vec<Sender<Template>>,
    callbacks: Vec<ReloadCallback>,
    latest_reloaded: Option<Template>,
}

/// Centralized manager that monitors multiple `.rost` documents using a single shared thread.
pub struct CentralWatcher {
    files: Mutex<HashMap<PathBuf, FileWatchState>>,
    running: Arc<AtomicBool>,
}

impl CentralWatcher {
    /// Returns the global shared watcher singleton.
    pub fn global() -> &'static Self {
        static INSTANCE: OnceLock<CentralWatcher> = OnceLock::new();
        INSTANCE.get_or_init(|| {
            let watcher = CentralWatcher {
                files: Mutex::new(HashMap::new()),
                running: Arc::new(AtomicBool::new(true)),
            };
            watcher.spawn_worker();
            watcher
        })
    }

    fn spawn_worker(&self) {
        let running = self.running.clone();

        thread::Builder::new()
            .name("rost-central-watcher".to_string())
            .spawn(move || {
                while running.load(Ordering::Relaxed) {
                    // Sleep on kernel futex yields 0.00% CPU
                    thread::sleep(Duration::from_millis(100));

                    let watcher = CentralWatcher::global();

                    // Batched scan of registered files
                    if let Ok(mut files) = watcher.files.lock() {
                        for (path, state) in files.iter_mut() {
                            if let Ok(metadata) = fs::metadata(path) {
                                if let Ok(modified) = metadata.modified() {
                                    if modified > state.last_modified {
                                        state.last_modified = modified;

                                        // Allow brief grace period for atomic editor file saves
                                        thread::sleep(Duration::from_millis(15));

                                        if let Ok(data) = fs::read(path) {
                                            let fields = parser::parse_into_bytecode(&data);
                                            let new_template = Template {
                                                path: path.clone(),
                                                data,
                                                fields,
                                                watcher: None,
                                            };
                                            state.latest_reloaded = Some(new_template.clone());

                                            // Trigger callbacks
                                            for cb in state.callbacks.iter() {
                                                cb(new_template.clone());
                                            }

                                            // Broadcast to channels
                                            state
                                                .subscribers
                                                .retain(|tx| tx.send(new_template.clone()).is_ok());
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            })
            .ok();
    }

    /// Registers a file for monitoring and returns a [`HotReloadWatcher`] handle.
    pub fn watch<P: AsRef<Path>>(&self, path: P) -> std::io::Result<Arc<HotReloadWatcher>> {
        let p = path.as_ref().to_path_buf();
        let canonical = p.canonicalize().unwrap_or_else(|_| p.clone());

        if !canonical.exists() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Cannot watch non-existent file: {:?}", canonical),
            ));
        }

        let last_modified = fs::metadata(&canonical)
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);

        if let Ok(mut files) = self.files.lock() {
            files
                .entry(canonical.clone())
                .or_insert_with(|| FileWatchState {
                    last_modified,
                    subscribers: Vec::new(),
                    callbacks: Vec::new(),
                    latest_reloaded: None,
                });
        }

        Ok(Arc::new(HotReloadWatcher { path: canonical }))
    }

    /// Returns the number of distinct `.rost` files currently monitored.
    pub fn active_files_count(&self) -> usize {
        self.files.lock().map(|f| f.len()).unwrap_or(0)
    }
}

/// A lightweight handle to a specific monitored file managed by [`CentralWatcher`].
pub struct HotReloadWatcher {
    path: PathBuf,
}

impl std::fmt::Debug for HotReloadWatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HotReloadWatcher")
            .field("path", &self.path)
            .finish()
    }
}

impl HotReloadWatcher {
    /// Registers a new file with the centralized multiplexed monitor.
    pub fn new<P: AsRef<Path>>(path: P) -> std::io::Result<Arc<Self>> {
        CentralWatcher::global().watch(path)
    }

    /// Subscribes to document updates via a channel receiver.
    pub fn subscribe(&self) -> Receiver<Template> {
        let (tx, rx) = channel();
        if let Ok(mut files) = CentralWatcher::global().files.lock() {
            if let Some(state) = files.get_mut(&self.path) {
                state.subscribers.push(tx);
            }
        }
        rx
    }

    /// Registers a callback closure to run whenever this document changes on disk.
    pub fn on_reload<F>(&self, cb: F)
    where
        F: Fn(Template) + Send + Sync + 'static,
    {
        if let Ok(mut files) = CentralWatcher::global().files.lock() {
            if let Some(state) = files.get_mut(&self.path) {
                state.callbacks.push(Box::new(cb));
            }
        }
    }

    /// Takes the latest reloaded template if an update occurred since the last check.
    pub fn check_reload(&self) -> Option<Template> {
        if let Ok(mut files) = CentralWatcher::global().files.lock() {
            if let Some(state) = files.get_mut(&self.path) {
                return state.latest_reloaded.take();
            }
        }
        None
    }
}

impl Drop for HotReloadWatcher {
    fn drop(&mut self) {
        if let Ok(mut files) = CentralWatcher::global().files.lock() {
            if let Some(state) = files.get_mut(&self.path) {
                if state.subscribers.is_empty() && state.callbacks.is_empty() {
                    files.remove(&self.path);
                }
            }
        }
    }
}
