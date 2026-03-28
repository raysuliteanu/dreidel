// SPDX-License-Identifier: GPL-3.0-only

use insta::assert_snapshot;
use ratatui::{Terminal, backend::TestBackend};
use toppers::{
    action::Action,
    components::{Component, cpu::CpuComponent, status_bar::StatusBarComponent},
    stats::snapshots::{CpuSnapshot, MemSnapshot, SysSnapshot},
    theme::ColorPalette,
};

#[test]
fn cpu_component_snapshot() {
    let mut comp = CpuComponent::new(ColorPalette::dark(), 'c');
    comp.update(Action::CpuUpdate(CpuSnapshot::stub())).unwrap();
    let mut t = Terminal::new(TestBackend::new(80, 20)).unwrap();
    t.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
    assert_snapshot!(t.backend());
}

#[test]
fn status_bar_snapshot() {
    use chrono::TimeZone;
    let sys = SysSnapshot {
        hostname: "dev-box".into(),
        uptime: 273_600,
        load_avg: [1.24, 0.98, 0.87],
        timestamp: chrono::Local
            .with_ymd_and_hms(2026, 3, 25, 12, 0, 0)
            .unwrap(),
    };
    let mut comp = StatusBarComponent::new(ColorPalette::dark());
    comp.update(Action::SysUpdate(sys)).unwrap();
    comp.update(Action::MemUpdate(MemSnapshot::stub())).unwrap();
    let mut t = Terminal::new(TestBackend::new(120, 4)).unwrap();
    t.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
    assert_snapshot!(t.backend());
}
