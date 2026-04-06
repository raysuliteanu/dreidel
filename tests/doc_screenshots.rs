// SPDX-License-Identifier: GPL-3.0-only

//! Render key component views to text files for use in documentation.
//!
//! Each test renders a component with stub data onto a `TestBackend`, strips
//! the per-line quoting that `TestBackend`'s `Display` impl adds, and writes
//! the result to `docs/screenshots/<name>.txt`.
//!
//! Run with:
//!   cargo test --test doc_screenshots
//!
//! After running, review the generated files and update USER_GUIDE.md to
//! reference them via `{% include ... %}` or inline the contents.

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};

use dreidel::{
    action::Action,
    components::{
        Component, cpu::CpuComponent, disk::DiskComponent, net::NetComponent,
        process::ProcessComponent, status_bar::StatusBarComponent,
    },
    config::ProcessConfig,
    stats::snapshots::{
        CpuSnapshot, DiskSnapshot, MemSnapshot, NetSnapshot, ProcSnapshot, SysSnapshot,
    },
    theme::ColorPalette,
};

/// Directory where screenshots are written.
const OUT_DIR: &str = "docs/screenshots";

/// Strip the leading/trailing `"` that TestBackend's Display adds per line,
/// then trim trailing whitespace from each line and drop trailing blank lines.
fn backend_to_text(backend: &TestBackend) -> String {
    let raw = format!("{}", backend);
    let mut lines: Vec<&str> = raw
        .lines()
        .map(|l| {
            let l = l.strip_prefix('"').unwrap_or(l);
            let l = l.strip_suffix('"').unwrap_or(l);
            l.trim_end()
        })
        .collect();

    // Drop trailing blank lines
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }

    let mut out = String::new();
    for line in &lines {
        writeln!(out, "{line}").expect("write to String cannot fail");
    }
    out
}

fn write_screenshot(name: &str, text: &str) {
    let dir = Path::new(OUT_DIR);
    fs::create_dir_all(dir).expect("create screenshots dir");
    let path = dir.join(format!("{name}.txt"));
    fs::write(&path, text).unwrap_or_else(|e| panic!("writing {}: {e}", path.display()));
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn fixed_sys_snapshot() -> SysSnapshot {
    use chrono::TimeZone;
    SysSnapshot {
        hostname: "dev-box".into(),
        uptime: 273_600,
        load_avg: [1.24, 0.98, 0.87],
        timestamp: chrono::Local
            .with_ymd_and_hms(2026, 4, 6, 14, 52, 7)
            .single()
            .expect("fixed timestamp must be valid"),
    }
}

// ── Status bar ──────────────────────────────────────────────────────────

#[test]
fn screenshot_status_bar() {
    let mut comp = StatusBarComponent::new(ColorPalette::dark());
    comp.update(&Action::SysUpdate(fixed_sys_snapshot()))
        .unwrap();
    comp.update(&Action::MemUpdate(MemSnapshot::stub()))
        .unwrap();

    let mut t = Terminal::new(TestBackend::new(100, 4)).unwrap();
    t.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
    write_screenshot("status_bar", &backend_to_text(t.backend()));
}

// ── CPU ─────────────────────────────────────────────────────────────────

#[test]
fn screenshot_cpu_compact() {
    let mut comp = CpuComponent::new(ColorPalette::dark(), 'c');
    comp.update(&Action::CpuUpdate(CpuSnapshot::stub()))
        .unwrap();
    comp.set_focused(true);

    let mut t = Terminal::new(TestBackend::new(60, 10)).unwrap();
    t.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
    write_screenshot("cpu_compact", &backend_to_text(t.backend()));
}

#[test]
fn screenshot_cpu_fullscreen() {
    let mut comp = CpuComponent::new(ColorPalette::dark(), 'c');
    comp.update(&Action::CpuUpdate(CpuSnapshot::stub()))
        .unwrap();
    comp.set_focused(true);
    comp.update(&Action::ToggleFullScreen).unwrap();
    comp.begin_overlay_render();

    let mut t = Terminal::new(TestBackend::new(80, 20)).unwrap();
    t.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
    write_screenshot("cpu_fullscreen", &backend_to_text(t.backend()));
}

// ── Network ─────────────────────────────────────────────────────────────

#[test]
fn screenshot_net_list() {
    let mut comp = NetComponent::new(ColorPalette::dark(), 'n');
    comp.update(&Action::NetUpdate(NetSnapshot::stub()))
        .unwrap();
    comp.set_focused(true);

    let mut t = Terminal::new(TestBackend::new(70, 8)).unwrap();
    t.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
    write_screenshot("net_list", &backend_to_text(t.backend()));
}

// ── Disk ────────────────────────────────────────────────────────────────

#[test]
fn screenshot_disk_list() {
    let mut comp = DiskComponent::new(ColorPalette::dark(), 'd');
    comp.update(&Action::DiskUpdate(DiskSnapshot::stub()))
        .unwrap();
    comp.set_focused(true);

    let mut t = Terminal::new(TestBackend::new(70, 8)).unwrap();
    t.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
    write_screenshot("disk_list", &backend_to_text(t.backend()));
}

// ── Process ─────────────────────────────────────────────────────────────

#[test]
fn screenshot_process_list() {
    let mut comp = ProcessComponent::new(ColorPalette::dark(), 'p', &ProcessConfig::default());
    comp.update(&Action::ProcUpdate(ProcSnapshot::stub()))
        .unwrap();
    comp.set_focused(true);

    let mut t = Terminal::new(TestBackend::new(100, 12)).unwrap();
    t.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
    write_screenshot("process_list", &backend_to_text(t.backend()));
}

#[test]
fn screenshot_process_detail() {
    let mut comp = ProcessComponent::new(ColorPalette::dark(), 'p', &ProcessConfig::default());
    comp.update(&Action::ProcUpdate(ProcSnapshot::stub()))
        .unwrap();
    comp.set_focused(true);
    // Row 0 is auto-selected; press Enter to open detail view.
    comp.handle_key_event(key(KeyCode::Enter)).unwrap();

    let mut t = Terminal::new(TestBackend::new(100, 30)).unwrap();
    t.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
    write_screenshot("process_detail", &backend_to_text(t.backend()));
}

#[test]
fn screenshot_process_tree() {
    let mut comp = ProcessComponent::new(
        ColorPalette::dark(),
        'p',
        &ProcessConfig {
            show_tree: true,
            ..ProcessConfig::default()
        },
    );
    comp.update(&Action::ProcUpdate(ProcSnapshot::stub()))
        .unwrap();
    comp.set_focused(true);

    let mut t = Terminal::new(TestBackend::new(100, 12)).unwrap();
    t.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
    write_screenshot("process_tree", &backend_to_text(t.backend()));
}
