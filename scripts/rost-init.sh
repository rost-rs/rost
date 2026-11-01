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
serde = { version = "1", features = ["derive"] }
EOF

cat >"$dir/app.rost" <<'EOF'
// The whole interface lives in this file. Edit it while the demo runs: the screen
// follows every save, and a bad edit is rejected with its line while the last good UI stays.
title: "rost live demo"
theme: dark
refresh: 100ms

// Themes available to this app (each one is themes/<name>.rost).
themes: ["dark", "light", "solarized"]

// Key bindings. Actions: quit, next_theme, prev_theme, theme:<name>, toggle:<id>.
keys [
    { key: "q", action: "quit", label: "quit" }
    { key: "t", action: "next_theme", label: "next theme" }
    { key: "1", action: "theme:dark", label: "dark" }
    { key: "2", action: "theme:light", label: "light" }
    { key: "3", action: "theme:solarized", label: "solarized" }
    { key: "h", action: "toggle:help", label: "help" }
]

// The widget tree. Kinds: column, row, block (framed container), text, list, gauge,
// palette (the theme's colors), status (hot reload state), keys (the bindings above).
// Sizes: 3 (rows or columns), 40%, min:5, fill, fill:2.
// Colors: bg fg accent muted border error, or #rrggbb.
// Text can use {title} {theme} {theme_name} {themes} {uptime} {size} {version} {refresh}.
layout {
    kind: column
    children [
        {
            kind: block
            size: 3
            border: rounded
            title: "rost"
            flow: row
            children [
                { kind: text, text: "{title}", fg: accent, bold: true }
                { kind: text, text: "theme {theme_name}  ·  up {uptime}", fg: muted, align: right, size: 34 }
            ]
        }
        {
            kind: row
            size: fill
            gap: 1
            children [
                {
                    kind: block
                    size: 60%
                    title: "try this in app.rost  ({size})"
                    border_color: accent
                    children [
                        {
                            kind: list
                            bullet: "▸ "
                            items [
                                "Change `size: 60%` of this panel to 30%",
                                "Set border: double (plain rounded double thick none)",
                                "Add a text node anywhere in the tree",
                                "Turn `hidden: true` on the help panel, or press h",
                                "Try `kind: widget`: the edit is rejected at its line",
                                "Press 1, 2 or 3, or t: the app patches `theme:` here"
                            ]
                        }
                    ]
                }
                {
                    kind: column
                    size: fill
                    children [
                        { kind: block, size: min:7, title: "themes/{theme}.rost", children [ { kind: palette } ] }
                        {
                            kind: block
                            size: 3
                            title: "activity"
                            children [ { kind: gauge, source: sweep, fg: accent } ]
                        }
                    ]
                }
            ]
        }
        {
            kind: block
            id: help
            size: 5
            title: "help"
            border: double
            border_color: muted
            children [
                {
                    kind: text
                    text: "{themes}: {title} reloaded {version} time(s). Layout, sizes, colors, texts, keys and themes all come from files."
                    fg: muted
                }
            ]
        }
        { kind: block, size: 4, title: "hot reload", children [ { kind: status } ] }
        { kind: keys, size: 1, fg: muted }
    ]
}
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
//! A terminal app whose whole interface is described in `app.rost`.
//!
//! The program only knows how to draw a few widget kinds. Which ones, where, how big,
//! in which colors, with which text and keys: all of that is read from watched files.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs;
use std::io;
use std::path::Path;
use std::str::FromStr;
use std::time::{Duration as Wait, Instant};

use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color as Paint, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Gauge, List, ListItem, Paragraph, Wrap};
use ratatui::{DefaultTerminal, Frame};
use rost::types::{Color, Duration};
use rost::{ReloadError, Reloader, Rost};
use serde::{Deserialize, Deserializer};

// ---- files ---------------------------------------------------------------------------

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
    themes: Vec<String>,
    #[rost(serde)]
    keys: Vec<Binding>,
    #[rost(serde)]
    layout: Node,
}

// ---- the widget tree -----------------------------------------------------------------

/// One node of the tree. Unknown keys are errors, so a typo is reported at its line.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Node {
    kind: Kind,
    id: Option<String>,
    size: Option<Size>,
    title: Option<String>,
    text: Option<String>,
    fg: Option<Tone>,
    bg: Option<Tone>,
    #[serde(default)]
    bold: bool,
    align: Option<Align>,
    wrap: Option<bool>,
    border: Option<Border>,
    border_color: Option<Tone>,
    flow: Option<Flow>,
    #[serde(default)]
    margin: u16,
    #[serde(default)]
    gap: u16,
    #[serde(default)]
    hidden: bool,
    bullet: Option<String>,
    #[serde(default)]
    items: Vec<String>,
    value: Option<f64>,
    source: Option<Source>,
    #[serde(default)]
    children: Vec<Node>,
}

#[derive(Deserialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "lowercase")]
enum Kind { Column, Row, Block, Text, List, Gauge, Palette, Status, Keys }

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum Flow { Column, Row }

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum Align { Left, Center, Right }

#[derive(Deserialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "lowercase")]
enum Border { None, Plain, Rounded, Double, Thick }

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum Source { Sweep }

/// `3`, `40%`, `min:5`, `fill`, `fill:2`.
#[derive(Clone, Copy)]
enum Size { Length(u16), Percent(u16), Min(u16), Fill(u16) }

#[derive(Clone, Copy)]
enum Slot { Bg, Fg, Accent, Muted, Border, Error }

/// A theme slot (`accent`) or a literal color (`#ff8800`).
#[derive(Clone, Copy)]
enum Tone { Slot(Slot), Rgb(u8, u8, u8) }

/// A key and what it does.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    key: String,
    action: Action,
    label: Option<String>,
}

/// `quit`, `next_theme`, `prev_theme`, `theme:<name>`, `toggle:<id>`.
enum Action { Quit, NextTheme, PrevTheme, Theme(String), Toggle(String) }

macro_rules! from_text {
    ($($t:ty),*) => {$(
        impl<'de> Deserialize<'de> for $t {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                String::deserialize(d)?.parse().map_err(serde::de::Error::custom)
            }
        }
    )*};
}
from_text!(Size, Tone, Action);

impl FromStr for Size {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        let bad = || format!("`{s}` is not a size: use 3, 40%, min:5, fill or fill:2");
        let num = |t: &str| t.parse::<u16>().map_err(|_| bad());
        if s == "fill" {
            Ok(Size::Fill(1))
        } else if let Some(n) = s.strip_prefix("fill:") {
            num(n).map(Size::Fill)
        } else if let Some(n) = s.strip_prefix("min:") {
            num(n).map(Size::Min)
        } else if let Some(n) = s.strip_suffix('%') {
            num(n).map(Size::Percent)
        } else {
            num(s).map(Size::Length)
        }
    }
}

impl Size {
    fn constraint(self) -> Constraint {
        match self {
            Size::Length(n) => Constraint::Length(n),
            Size::Percent(n) => Constraint::Percentage(n),
            Size::Min(n) => Constraint::Min(n),
            Size::Fill(n) => Constraint::Fill(n),
        }
    }
}

impl FromStr for Tone {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        Ok(match s {
            "bg" => Tone::Slot(Slot::Bg),
            "fg" => Tone::Slot(Slot::Fg),
            "accent" => Tone::Slot(Slot::Accent),
            "muted" => Tone::Slot(Slot::Muted),
            "border" => Tone::Slot(Slot::Border),
            "error" => Tone::Slot(Slot::Error),
            _ => s.parse::<Color>().map(|c| Tone::Rgb(c.r, c.g, c.b)).map_err(|_| {
                format!("`{s}` is not a color: use bg, fg, accent, muted, border, error or #rrggbb")
            })?,
        })
    }
}

impl FromStr for Action {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "quit" => Ok(Action::Quit),
            "next_theme" => Ok(Action::NextTheme),
            "prev_theme" => Ok(Action::PrevTheme),
            _ => match (s.strip_prefix("theme:"), s.strip_prefix("toggle:")) {
                (Some(name), _) => Ok(Action::Theme(name.into())),
                (_, Some(id)) => Ok(Action::Toggle(id.into())),
                _ => Err(format!("`{s}` is not an action: quit, next_theme, prev_theme, theme:<name>, toggle:<id>")),
            },
        }
    }
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Action::Quit => f.write_str("quit"),
            Action::NextTheme => f.write_str("next theme"),
            Action::PrevTheme => f.write_str("previous theme"),
            Action::Theme(n) => write!(f, "theme {n}"),
            Action::Toggle(i) => write!(f, "toggle {i}"),
        }
    }
}

impl Node {
    fn flow(&self) -> Flow {
        match self.kind {
            Kind::Row => Flow::Row,
            Kind::Column => Flow::Column,
            _ => self.flow.unwrap_or(Flow::Column),
        }
    }
}

// ---- validation: rules between fields, checked before a reload is accepted -------------

const VARS: [&str; 8] = ["title", "theme", "theme_name", "themes", "uptime", "size", "version", "refresh"];

fn validate(c: &AppConfig) -> Result<(), String> {
    let theme_file = |n: &str| Path::new("themes").join(format!("{n}.rost"));
    if c.themes.is_empty() {
        return Err("field `themes`: list at least one theme".into());
    }
    for name in &c.themes {
        if !theme_file(name).is_file() {
            return Err(format!("field `themes`: there is no file {}", theme_file(name).display()));
        }
    }
    if !c.themes.contains(&c.theme) {
        return Err(format!("field `theme`: `{}` is not in `themes` ({})", c.theme, c.themes.join(", ")));
    }
    let mut ids = HashSet::new();
    collect_ids(&c.layout, "layout", &mut ids)?;
    for (i, b) in c.keys.iter().enumerate() {
        match &b.action {
            Action::Theme(n) if !c.themes.contains(n) => {
                return Err(format!("field `keys[{i}].action`: `{n}` is not in `themes`"));
            }
            Action::Toggle(id) if !ids.contains(id.as_str()) => {
                return Err(format!("field `keys[{i}].action`: no node has id `{id}`"));
            }
            _ => {}
        }
    }
    check_node(&c.layout, "layout", 0)
}

fn collect_ids<'a>(n: &'a Node, path: &str, ids: &mut HashSet<&'a str>) -> Result<(), String> {
    if let Some(id) = &n.id
        && !ids.insert(id)
    {
        return Err(format!("field `{path}.id`: `{id}` is used twice"));
    }
    for (i, c) in n.children.iter().enumerate() {
        collect_ids(c, &format!("{path}.children[{i}]"), ids)?;
    }
    Ok(())
}

fn check_node(n: &Node, path: &str, depth: usize) -> Result<(), String> {
    let err = |field: &str, msg: &str| Err(format!("field `{path}.{field}`: {msg}"));
    if depth > 12 {
        return err("children", "nested deeper than 12 levels");
    }
    match n.kind {
        Kind::Text if n.text.is_none() => return err("text", "a text node needs `text`"),
        Kind::List if n.items.is_empty() => return err("items", "a list needs `items`"),
        Kind::Gauge if n.value.is_none() && n.source.is_none() => {
            return err("value", "a gauge needs `value` (0 to 100) or `source: sweep`");
        }
        _ => {}
    }
    if let Some(v) = n.value
        && !(0.0..=100.0).contains(&v)
    {
        return err("value", &format!("{v} is out of range: expected 0 to 100"));
    }
    let texts = n.text.iter().chain(&n.title).chain(&n.items);
    for t in texts {
        let mut rest = t.as_str();
        while let Some(open) = rest.find('{') {
            let Some(len) = rest[open..].find('}') else { break };
            let var = &rest[open + 1..open + len];
            if !VARS.contains(&var) {
                return err("text", &format!("unknown variable `{{{var}}}` (have: {})", VARS.join(", ")));
            }
            rest = &rest[open + len + 1..];
        }
    }
    for (i, c) in n.children.iter().enumerate() {
        check_node(c, &format!("{path}.children[{i}]"), depth + 1)?;
    }
    Ok(())
}

// ---- drawing ---------------------------------------------------------------------------

struct Cx<'a> {
    app: &'a Reloader<AppConfig>,
    config: &'a AppConfig,
    live: &'a Reloader<Theme>,
    theme: &'a Theme,
    toggled: &'a HashSet<String>,
    started: Instant,
}

fn paint(c: Color) -> Paint {
    Paint::Rgb(c.r, c.g, c.b)
}

impl Cx<'_> {
    fn tone(&self, t: Tone) -> Paint {
        match t {
            Tone::Rgb(r, g, b) => Paint::Rgb(r, g, b),
            Tone::Slot(Slot::Error) => Paint::Rgb(243, 139, 168),
            Tone::Slot(s) => paint(match s {
                Slot::Bg => self.theme.bg,
                Slot::Fg | Slot::Error => self.theme.fg,
                Slot::Accent => self.theme.accent,
                Slot::Muted => self.theme.muted,
                Slot::Border => self.theme.border,
            }),
        }
    }

    fn slot(&self, s: Slot) -> Paint {
        self.tone(Tone::Slot(s))
    }

    /// `hidden` in the file, flipped by the `toggle:<id>` key.
    fn hidden(&self, n: &Node) -> bool {
        n.hidden != n.id.as_ref().is_some_and(|id| self.toggled.contains(id))
    }

    fn style(&self, n: &Node) -> Style {
        let mut s = Style::new().fg(n.fg.map_or(self.slot(Slot::Fg), |t| self.tone(t)));
        if n.bold {
            s = s.add_modifier(Modifier::BOLD);
        }
        s
    }

    fn fill(&self, text: &str, area: Rect) -> String {
        let secs = self.started.elapsed().as_secs();
        text.replace("{title}", &self.config.title)
            .replace("{theme}", &self.config.theme)
            .replace("{theme_name}", &self.theme.name)
            .replace("{themes}", &self.config.themes.join(", "))
            .replace("{uptime}", &format!("{:02}:{:02}", secs / 60, secs % 60))
            .replace("{size}", &format!("{}x{}", area.width, area.height))
            .replace("{version}", &self.app.version().to_string())
            .replace("{refresh}", &format!("{}ms", self.config.refresh.get().as_millis()))
    }
}

fn render(f: &mut Frame, n: &Node, area: Rect, cx: &Cx) {
    if cx.hidden(n) {
        return;
    }
    let area = Rect {
        x: area.x + n.margin.min(area.width / 2),
        y: area.y + n.margin.min(area.height / 2),
        width: area.width.saturating_sub(2 * n.margin),
        height: area.height.saturating_sub(2 * n.margin),
    };
    if let Some(bg) = n.bg {
        f.render_widget(Block::new().style(Style::new().bg(cx.tone(bg))), area);
    }

    let mut inner = area;
    let border = n.border.unwrap_or(if n.kind == Kind::Block { Border::Plain } else { Border::None });
    if border != Border::None {
        let mut frame = Block::bordered()
            .border_type(match border {
                Border::Rounded => BorderType::Rounded,
                Border::Double => BorderType::Double,
                Border::Thick => BorderType::Thick,
                _ => BorderType::Plain,
            })
            .border_style(Style::new().fg(n.border_color.map_or(cx.slot(Slot::Border), |t| cx.tone(t))));
        if let Some(t) = &n.title {
            frame = frame.title(Span::styled(cx.fill(t, area), Style::new().fg(cx.slot(Slot::Accent)).bold()));
        }
        inner = frame.inner(area);
        f.render_widget(frame, area);
    }

    let style = cx.style(n);
    match n.kind {
        Kind::Column | Kind::Row | Kind::Block => {
            let kids: Vec<&Node> = n.children.iter().filter(|c| !cx.hidden(c)).collect();
            let direction = match n.flow() {
                Flow::Column => Direction::Vertical,
                Flow::Row => Direction::Horizontal,
            };
            let sizes = kids.iter().map(|c| c.size.map_or(Constraint::Fill(1), Size::constraint));
            let areas = Layout::new(direction, sizes).spacing(n.gap).split(inner);
            for (child, a) in kids.iter().zip(areas.iter()) {
                render(f, child, *a, cx);
            }
        }
        Kind::Text => {
            let mut p = Paragraph::new(cx.fill(n.text.as_deref().unwrap_or(""), inner)).style(style).alignment(match n.align {
                Some(Align::Center) => Alignment::Center,
                Some(Align::Right) => Alignment::Right,
                _ => Alignment::Left,
            });
            if n.wrap.unwrap_or(true) {
                p = p.wrap(Wrap { trim: true });
            }
            f.render_widget(p, inner);
        }
        Kind::List => {
            let bullet = n.bullet.as_deref().unwrap_or("• ");
            let items: Vec<ListItem> = n.items.iter().map(|i| ListItem::new(format!("{bullet}{}", cx.fill(i, inner)))).collect();
            f.render_widget(List::new(items).style(style), inner);
        }
        Kind::Gauge => {
            let pct = match (n.source, n.value) {
                (Some(Source::Sweep), _) => {
                    let t = cx.started.elapsed().as_millis() % 4000;
                    (if t < 2000 { t } else { 4000 - t }) as f64 / 20.0
                }
                (None, v) => v.unwrap_or(0.0),
            };
            let label = n.text.as_ref().map_or(format!("{pct:.0}%"), |t| cx.fill(t, inner));
            let accent = n.fg.map_or(cx.slot(Slot::Accent), |t| cx.tone(t));
            f.render_widget(
                Gauge::default().ratio(pct / 100.0).label(label).gauge_style(Style::new().fg(accent).bg(cx.slot(Slot::Border))),
                inner,
            );
        }
        Kind::Palette => {
            let t = cx.theme;
            let rows = [("bg", t.bg), ("fg", t.fg), ("accent", t.accent), ("muted", t.muted), ("border", t.border)];
            let lines: Vec<Line> = rows
                .iter()
                .map(|(label, c)| {
                    Line::from(vec![
                        Span::styled("   ", Style::new().bg(paint(*c))),
                        Span::styled(format!(" {label:<7}{c}"), Style::new().fg(cx.slot(Slot::Muted))),
                    ])
                })
                .collect();
            f.render_widget(Paragraph::new(lines), inner);
        }
        Kind::Status => {
            let lines = vec![
                status("app.rost", cx.app.version(), cx.app.last_error(), cx),
                status(&format!("themes/{}.rost", cx.config.theme), cx.live.version(), cx.live.last_error(), cx),
            ];
            f.render_widget(Paragraph::new(lines), inner);
        }
        Kind::Keys => {
            let legend: Vec<String> = cx.config.keys.iter().map(|b| format!("{} {}", b.key, b.label.clone().unwrap_or_else(|| b.action.to_string()))).collect();
            f.render_widget(Paragraph::new(legend.join("   ")).style(style), inner);
        }
    }
}

fn status<'a>(name: &str, version: u64, err: Option<ReloadError>, cx: &Cx) -> Line<'a> {
    match err {
        Some(e) => {
            let at = e.line.map(|l| format!(" at {l}:{}", e.column.unwrap_or(1))).unwrap_or_default();
            Line::from(Span::styled(format!("✗ {name} v{version} kept; rejected{at}: {}", e.message), Style::new().fg(cx.tone(Tone::Slot(Slot::Error)))))
        }
        None => Line::from(Span::styled(format!("✓ {name} v{version} ok"), Style::new().fg(cx.slot(Slot::Muted)))),
    }
}

// ---- running ---------------------------------------------------------------------------

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // `--set path=value` and DEMO_PATH__TO=value override app.rost without editing it.
    let overrides = rost::Overrides::from_env("DEMO").merge(rost::Overrides::from_args(std::env::args()));

    // app.rost: loaded, checked by `validate`, and watched. A bad edit never replaces the live one.
    let app = Reloader::<AppConfig>::builder("app.rost", move |text| rost::load_with(text, &overrides).map_err(ReloadError::from_display))
        .validate(validate)
        .build()?;

    let mut themes = HashMap::new();
    ensure_themes(&app.get(), &mut themes);
    if !themes.contains_key(&app.get().theme) {
        return Err(format!("cannot load themes/{}.rost", app.get().theme).into());
    }

    if std::env::args().any(|a| a == "--check") {
        let c = app.get();
        println!("ok: \"{}\", theme {}, {} themes, {} keys, refresh {}ms", c.title, c.theme, themes.len(), c.keys.len(), c.refresh.get().as_millis());
        return Ok(());
    }

    let mut terminal = ratatui::init();
    let result = run(&mut terminal, &app, &mut themes);
    ratatui::restore();
    result.map_err(Into::into)
}

/// Starts a watcher for every theme the config lists (also the ones added while running).
fn ensure_themes(config: &AppConfig, themes: &mut HashMap<String, Reloader<Theme>>) {
    for name in config.themes.iter().chain([&config.theme]) {
        if !themes.contains_key(name)
            && let Ok(r) = Reloader::watch_load(Path::new("themes").join(format!("{name}.rost")))
        {
            themes.insert(name.clone(), r);
        }
    }
}

fn run(terminal: &mut DefaultTerminal, app: &Reloader<AppConfig>, themes: &mut HashMap<String, Reloader<Theme>>) -> io::Result<()> {
    let started = Instant::now();
    let mut toggled = HashSet::new();
    loop {
        let config = app.get();
        ensure_themes(&config, themes);
        // A theme that failed to load is skipped: fall back to any loaded one.
        let Some(live) = themes.get(&config.theme).or_else(|| themes.values().next()) else { return Ok(()) };
        let theme = live.get();
        let cx = Cx { app, config: &config, live, theme: &theme, toggled: &toggled, started };
        terminal.draw(|f| {
            f.render_widget(Block::new().style(Style::new().bg(cx.slot(Slot::Bg)).fg(cx.slot(Slot::Fg))), f.area());
            render(f, &config.layout, f.area(), &cx);
        })?;

        if event::poll(config.refresh.get().max(Wait::from_millis(16)))?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            // Ctrl-C always quits, even if the keys in app.rost are wrong.
            if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                return Ok(());
            }
            let Some(binding) = config.keys.iter().find(|b| key_matches(&b.key, key.code)) else { continue };
            match &binding.action {
                Action::Quit => return Ok(()),
                Action::NextTheme => cycle(&config, 1),
                Action::PrevTheme => cycle(&config, -1),
                Action::Theme(name) => set_theme(name),
                Action::Toggle(id) => {
                    if !toggled.remove(id) {
                        toggled.insert(id.clone());
                    }
                }
            }
        }
    }
}

fn key_matches(spec: &str, code: KeyCode) -> bool {
    match (spec, code) {
        ("esc", KeyCode::Esc) | ("tab", KeyCode::Tab) | ("enter", KeyCode::Enter) | ("space", KeyCode::Char(' ')) => true,
        ("left", KeyCode::Left) | ("right", KeyCode::Right) | ("up", KeyCode::Up) | ("down", KeyCode::Down) => true,
        (s, KeyCode::Char(c)) => s.chars().eq([c]),
        _ => false,
    }
}

fn cycle(config: &AppConfig, step: isize) {
    let at = config.themes.iter().position(|n| *n == config.theme).unwrap_or(0) as isize;
    let next = (at + step).rem_euclid(config.themes.len() as isize) as usize;
    set_theme(&config.themes[next]);
}

/// Writes the theme name into app.rost. Only the value changes; comments and the rest of
/// the file stay as they are, and the watcher reloads it like any other edit.
fn set_theme(name: &str) {
    let path = Path::new("app.rost");
    if let Ok(text) = fs::read_to_string(path)
        && let Ok(Some(patched)) = rost::Overrides::new().set("theme", name).patch(&text)
    {
        let _ = fs::write(path, patched);
    }
}
EOF

echo "rost demo: building $dir (first build downloads the crates)"
(cd "$dir" && cargo build --quiet)
echo
echo "Ready. Start it with:   cd $dir && cargo run"
echo "Then edit themes/*.rost or app.rost in another terminal and watch the screen."
[ "$run" -eq 0 ] || { cd "$dir" && exec cargo run --quiet; }
