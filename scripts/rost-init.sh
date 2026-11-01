#!/bin/sh
# Creates a small terminal app that shows what rost does:
#
#   * three themes in separate .rost files, each loaded into typed Rust values;
#   * edit any file while the app runs and it changes on screen, with no restart;
#   * a broken edit is rejected: the app keeps the last good value and shows the
#     error with its line and column;
#   * `t` switches theme by writing `theme: …` into app.rost (a patch that keeps
#     comments and layout), and the hot reload picks it up like any other edit.
#
#   scripts/rost-init.sh [DIR] [--run]
#
# DIR defaults to ./rost-demo. With --run the app starts right after it is built.
# Inside a rost checkout the demo uses that checkout; anywhere else it uses the
# released tag from git. Set ROST_PATH=/path/to/rost to force a local copy.
set -eu

TAG="v0.2.0-alpha.2"
REPO="https://github.com/rost-rs/rost.git"
dir="rost-demo"
run=0
for arg in "$@"; do
    case "$arg" in
        --run) run=1 ;;
        -h|--help) sed -n '2,15p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) dir="$arg" ;;
    esac
done

# ---- dependencies, declared first ------------------------------------------------
# Tools:   cargo + rustc >= 1.85 (rost uses the 2024 edition), git (only for the tag)
# Crates:  rost (features: derive, watch)   the format, typed loading, hot reload
#          ratatui 0.30                     terminal UI (brings crossterm)
echo "rost demo: checking dependencies"
command -v cargo >/dev/null || { echo "  missing: cargo (install Rust: https://rustup.rs)" >&2; exit 1; }
minor=$(rustc --version | sed 's/rustc 1\.\([0-9]*\).*/\1/')
[ "$minor" -ge 85 ] || { echo "  rustc 1.$minor is too old, rost needs 1.85 or newer: rustup update" >&2; exit 1; }

here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
if [ -n "${ROST_PATH:-}" ]; then
    dep="rost = { path = \"$ROST_PATH\", features = [\"derive\", \"watch\"] }"
elif [ -f "$here/../Cargo.toml" ] && grep -q '^name = "rost"' "$here/../Cargo.toml"; then
    dep="rost = { path = \"$(CDPATH= cd -- "$here/.." && pwd)\", features = [\"derive\", \"watch\"] }"
else
    command -v git >/dev/null || { echo "  missing: git (needed to fetch rost $TAG)" >&2; exit 1; }
    dep="rost = { git = \"$REPO\", tag = \"$TAG\", features = [\"derive\", \"watch\"] }"
fi
echo "  cargo $(cargo --version | cut -d' ' -f2), rustc 1.$minor"
echo "  $dep"

[ ! -e "$dir" ] || [ -z "$(ls -A "$dir" 2>/dev/null)" ] || { echo "$dir exists and is not empty" >&2; exit 1; }
mkdir -p "$dir/src" "$dir/themes"

# ---- project --------------------------------------------------------------------
cat >"$dir/Cargo.toml" <<EOF
[package]
name = "rost-demo"
version = "0.1.0"
edition = "2024"

[dependencies]
$dep
ratatui = "0.30"
EOF

cat >"$dir/app.rost" <<'EOF'
// The app's own settings. Edit this file while the demo runs.
title: "rost live demo"
theme: dark
// How often the screen is redrawn. Try 50ms, 1s, or something invalid like "fast".
refresh: 100ms
items [
    "Edit themes/dark.rost and save: colors change at once",
    "Break a file on purpose: the last good value stays and the error shows here",
    "Press t to switch theme (it patches app.rost, comments included)",
    "Set theme to something that does not exist: the edit is rejected",
    "Add --set title=hello style overrides on the command line"
]
EOF

cat >"$dir/themes/dark.rost" <<'EOF'
name: "Dark"
bg: #1e1e2e
fg: #cdd6f4
accent: #89b4fa
muted: #6c7086
border: #45475a
EOF

cat >"$dir/themes/light.rost" <<'EOF'
name: "Light"
bg: #eff1f5
fg: #4c4f69
accent: #1e66f5
muted: #8c8fa1
border: #acb0be
EOF

cat >"$dir/themes/solarized.rost" <<'EOF'
name: "Solarized"
bg: #002b36
fg: #93a1a1
accent: #b58900
muted: #586e75
border: #073642
EOF

cat >"$dir/src/main.rs" <<'EOF'
//! A terminal app driven by .rost files.
//!
//! Every setting below comes from a file that is watched while the app runs.

use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::time::Duration as Wait;

use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color as Paint, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use ratatui::{DefaultTerminal, Frame};
use rost::types::{Color, Duration};
use rost::{ReloadError, Reloader, Rost};

/// One colour scheme (themes/*.rost). `Color` reads `#rgb` or `#rrggbb`.
#[derive(Rost)]
struct Theme {
    name: String,
    bg: Color,
    fg: Color,
    accent: Color,
    muted: Color,
    border: Color,
}

/// app.rost
#[derive(Rost)]
struct AppConfig {
    title: String,
    theme: String,
    /// `Duration` reads `250ms`, `1.5s`, `2m`…
    refresh: Duration,
    #[rost(serde)]
    items: Vec<String>,
}

fn paint(c: Color) -> Paint {
    Paint::Rgb(c.r, c.g, c.b)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // `--set path=value` and APP_PATH__TO=value override app.rost without editing it.
    let overrides = rost::Overrides::from_env("DEMO").merge(rost::Overrides::from_args(std::env::args()));

    // Themes: one watcher per file in themes/.
    let mut themes: Vec<(String, Reloader<Theme>)> = Vec::new();
    for entry in fs::read_dir("themes")? {
        let path = entry?.path();
        if path.extension().is_some_and(|e| e == "rost") {
            let name = path.file_stem().unwrap().to_string_lossy().into_owned();
            themes.push((name, Reloader::watch_load(&path)?));
        }
    }
    themes.sort_by(|a, b| a.0.cmp(&b.0));
    let known: HashSet<String> = themes.iter().map(|(n, _)| n.clone()).collect();

    // app.rost: loaded, validated against the theme list, and watched.
    // A theme name that does not exist is rejected like a syntax error.
    let app = Reloader::<AppConfig>::builder("app.rost", move |text| {
        rost::load_with(text, &overrides).map_err(ReloadError::from_display)
    })
    .validate(move |c: &AppConfig| {
        if known.contains(&c.theme) {
            Ok(())
        } else {
            let mut names: Vec<_> = known.iter().cloned().collect();
            names.sort();
            Err(format!("field `theme`: no theme named `{}` (have: {})", c.theme, names.join(", ")))
        }
    })
    .build()?;

    if std::env::args().any(|a| a == "--check") {
        let c = app.get();
        println!("ok: \"{}\", theme {}, {} themes, refresh {}", c.title, c.theme, themes.len(), c.refresh.get().as_millis());
        return Ok(());
    }

    let mut terminal = ratatui::init();
    let result = run(&mut terminal, &app, &themes);
    ratatui::restore();
    result.map_err(Into::into)
}

fn run(terminal: &mut DefaultTerminal, app: &Reloader<AppConfig>, themes: &[(String, Reloader<Theme>)]) -> std::io::Result<()> {
    loop {
        let config = app.get();
        terminal.draw(|f| draw(f, app, &config, themes))?;
        if event::poll(config.refresh.get().max(Wait::from_millis(16)))?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                KeyCode::Char('t') => next_theme(&config.theme, themes),
                _ => {}
            }
        }
    }
}

/// Writes the next theme name into app.rost. Only the value changes; comments and the
/// rest of the file stay as they are, and the watcher reloads it like any other edit.
fn next_theme(current: &str, themes: &[(String, Reloader<Theme>)]) {
    let at = themes.iter().position(|(n, _)| n == current).unwrap_or(0);
    let next = &themes[(at + 1) % themes.len()].0;
    let path = Path::new("app.rost");
    if let Ok(text) = fs::read_to_string(path)
        && let Ok(Some(patched)) = rost::Overrides::new().set("theme", next).patch(&text)
    {
        let _ = fs::write(path, patched);
    }
}

fn draw(f: &mut Frame, app: &Reloader<AppConfig>, config: &AppConfig, themes: &[(String, Reloader<Theme>)]) {
    let live = themes.iter().find(|(n, _)| *n == config.theme).map(|(_, r)| r);
    let Some(live) = live else { return };
    let theme = live.get();
    let (bg, fg, accent, muted, border) = (paint(theme.bg), paint(theme.fg), paint(theme.accent), paint(theme.muted), paint(theme.border));
    let base = Style::new().bg(bg).fg(fg);
    f.render_widget(Block::new().style(base), f.area());

    let rows = Layout::vertical([Constraint::Length(3), Constraint::Min(5), Constraint::Length(5), Constraint::Length(1)]).split(f.area());
    let framed = |title: String| {
        Block::new()
            .borders(Borders::ALL)
            .border_style(Style::new().fg(border))
            .title(Span::styled(title, Style::new().fg(accent).bold()))
            .style(base)
    };

    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(config.title.clone(), Style::new().fg(accent).bold()),
            Span::styled(format!("   theme: {} ({})", theme.name, config.theme), Style::new().fg(muted)),
        ]))
        .block(framed("rost".into())),
        rows[0],
    );

    let body = Layout::horizontal([Constraint::Percentage(60), Constraint::Percentage(40)]).split(rows[1]);
    let items: Vec<ListItem> = config.items.iter().map(|i| ListItem::new(format!("• {i}"))).collect();
    f.render_widget(List::new(items).block(framed("app.rost items".into())).style(base), body[0]);

    let swatch = |label: &str, c: Color| {
        Line::from(vec![
            Span::styled("   ", Style::new().bg(paint(c))),
            Span::styled(format!(" {label:<7}{c}"), Style::new().fg(muted)),
        ])
    };
    f.render_widget(
        Paragraph::new(vec![swatch("bg", theme.bg), swatch("fg", theme.fg), swatch("accent", theme.accent), swatch("muted", theme.muted), swatch("border", theme.border)])
            .block(framed(format!("themes/{}.rost", config.theme))),
        body[1],
    );

    let status = |name: &str, version: u64, err: Option<ReloadError>| match err {
        Some(e) => {
            let at = e.line.map(|l| format!(" at {l}:{}", e.column.unwrap_or(1))).unwrap_or_default();
            Line::from(Span::styled(format!("✗ {name} v{version} kept; rejected{at}: {}", e.message), Style::new().fg(Paint::Rgb(243, 139, 168))))
        }
        None => Line::from(Span::styled(format!("✓ {name} v{version} ok"), Style::new().fg(muted))),
    };
    f.render_widget(
        Paragraph::new(vec![status("app.rost", app.version(), app.last_error()), status(&format!("themes/{}.rost", config.theme), live.version(), live.last_error())])
            .block(framed("hot reload".into())),
        rows[2],
    );

    f.render_widget(Paragraph::new("t switch theme   q quit   edit the .rost files in another terminal").style(Style::new().bg(bg).fg(muted)), rows[3]);
}
EOF

echo "rost demo: building $dir (first build downloads the crates)"
(cd "$dir" && cargo build --quiet)
echo
echo "Ready. Start it with:   cd $dir && cargo run"
echo "Then edit themes/*.rost or app.rost in another terminal and watch the screen."
[ "$run" -eq 0 ] || { cd "$dir" && exec cargo run --quiet; }
