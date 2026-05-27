// SPDX-License-Identifier: GPL-3.0-only
// Doc screenshots are generated from Linux-specific stub data (MEM_LABEL_WIDTH,
// buffer/cache fields). Gate the whole suite so macOS runs never overwrite
// USER_GUIDE.md with platform-different output.
#![cfg(target_os = "linux")]

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
use std::sync::Mutex;

static GUIDE_LOCK: Mutex<()> = Mutex::new(());

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
    patch_user_guide(name, text);
}

fn patch_user_guide(name: &str, text: &str) {
    let _guard = GUIDE_LOCK.lock().expect("guide lock poisoned");
    let guide_path = Path::new("USER_GUIDE.md");
    let content =
        fs::read_to_string(guide_path).unwrap_or_else(|e| panic!("reading USER_GUIDE.md: {e}"));

    let begin_marker = format!("<!-- screenshot:{name}:begin -->");
    let end_marker = format!("<!-- screenshot:{name}:end -->");

    let begin = content.find(&begin_marker).unwrap_or_else(|| {
        panic!("begin marker for screenshot '{name}' not found in USER_GUIDE.md")
    });
    let end = content
        .find(&end_marker)
        .unwrap_or_else(|| panic!("end marker for screenshot '{name}' not found in USER_GUIDE.md"));

    assert!(begin < end, "markers out of order for '{name}'");

    let after_begin = begin + begin_marker.len();
    let new_content = format!(
        "{}\n\n```\n{text}```\n\n{}",
        &content[..after_begin],
        &content[end..]
    );

    fs::write(guide_path, new_content).unwrap_or_else(|e| panic!("writing USER_GUIDE.md: {e}"));
}

/// Verify that every screenshot block in USER_GUIDE.md matches its .txt file.
/// Run `cargo test --test doc_screenshots screenshot_` to regenerate if this fails.
#[test]
fn verify_user_guide_screenshots() {
    let guide = fs::read_to_string("USER_GUIDE.md").expect("reading USER_GUIDE.md");

    let names = [
        "cpu_compact",
        "cpu_fullscreen",
        "disk_list",
        "net_list",
        "process_detail",
        "process_list",
        "process_tree",
        "status_bar",
    ];

    for name in names {
        let txt_path = Path::new(OUT_DIR).join(format!("{name}.txt"));
        let expected = fs::read_to_string(&txt_path)
            .unwrap_or_else(|e| panic!("reading {}: {e}", txt_path.display()));

        let begin_marker = format!("<!-- screenshot:{name}:begin -->");
        let end_marker = format!("<!-- screenshot:{name}:end -->");

        let begin = guide
            .find(&begin_marker)
            .unwrap_or_else(|| panic!("begin marker for '{name}' not found in USER_GUIDE.md"));
        let end = guide
            .find(&end_marker)
            .unwrap_or_else(|| panic!("end marker for '{name}' not found in USER_GUIDE.md"));

        let block = &guide[begin + begin_marker.len()..end];

        let fence_start = block
            .find("```\n")
            .unwrap_or_else(|| panic!("no opening ``` fence in USER_GUIDE.md block for '{name}'"));
        let after_fence = fence_start + 4;
        let fence_end = block[after_fence..]
            .find("```")
            .unwrap_or_else(|| panic!("no closing ``` fence in USER_GUIDE.md block for '{name}'"));

        let actual = &block[after_fence..after_fence + fence_end];
        assert_eq!(
            actual, expected,
            "USER_GUIDE.md screenshot '{name}' is out of sync with {name}.txt\n\
             Regenerate with: cargo test --test doc_screenshots screenshot_"
        );
    }
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
