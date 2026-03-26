use insta::assert_snapshot;
use ratatui::{Terminal, backend::TestBackend};
use toppers::{
    action::Action,
    components::{Component, cpu::CpuComponent, mem::MemComponent},
    stats::snapshots::{CpuSnapshot, MemSnapshot},
    theme::ColorPalette,
};

#[test]
fn cpu_component_snapshot() {
    let mut comp = CpuComponent::new(ColorPalette::dark());
    comp.update(Action::CpuUpdate(CpuSnapshot::stub())).unwrap();
    let mut t = Terminal::new(TestBackend::new(80, 20)).unwrap();
    t.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
    assert_snapshot!(t.backend());
}

#[test]
fn mem_component_snapshot() {
    let mut comp = MemComponent::new(ColorPalette::dark());
    comp.update(Action::MemUpdate(MemSnapshot::stub())).unwrap();
    let mut t = Terminal::new(TestBackend::new(60, 10)).unwrap();
    t.draw(|f| comp.draw(f, f.area()).unwrap()).unwrap();
    assert_snapshot!(t.backend());
}
