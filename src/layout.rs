// SPDX-License-Identifier: GPL-3.0-only

//! Layout engine mapping [`SlotId`]s to screen rectangles.
//!
//! [`LayoutPreset`] defines the available multi-panel arrangements (sidebar,
//! classic, dashboard, grid). [`LayoutPreset::compute`] takes the terminal
//! area, optional slot overrides, and component height hints, and returns a
//! [`SlotMap`] of `SlotId → (ComponentId, Rect)` pairs.

use ratatui::layout::{Constraint, Layout, Rect};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use strum::{Display, EnumString};

use crate::components::ComponentId;

/// Per-slot height hints supplied by the app so the layout engine can size
/// panels tightly to their content rather than using fixed percentages.
#[derive(Debug, Default, Clone)]
pub struct LayoutHints {
    /// Preferred height for the top-left slot (e.g. CPU in Sidebar).
    pub left_top: Option<u16>,
    /// Preferred height for the top-right slot (e.g. CPU in Grid).
    pub right_top: Option<u16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SlotId {
    // sidebar preset
    LeftTop,
    LeftBot,
    LeftExtra,
    Right,
    // classic preset
    TopLeft,
    TopRightTop,
    TopRightBot,
    Bottom,
    // dashboard preset
    Top,
    MidLeft,
    MidRight,
    // grid preset: left col = [Disk, Net], right col = [Cpu, Process]
    GridLeftMid,
    GridLeftBot,
    GridRightTop,
    GridRightBot,
}

#[derive(Debug, Clone, Default)]
pub struct SlotOverrides(pub HashMap<SlotId, ComponentId>);

pub type SlotMap = HashMap<SlotId, (ComponentId, Rect)>;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Display, EnumString, Serialize, Deserialize,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum LayoutPreset {
    #[default]
    Sidebar,
    Classic,
    Dashboard,
    Grid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, strum::EnumString, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum StatusBarPosition {
    #[default]
    Top,
    Bottom,
    Hidden,
}

pub fn split_status_bar(area: Rect, pos: StatusBarPosition, height: u16) -> (Rect, Rect) {
    match pos {
        StatusBarPosition::Hidden => (Rect::default(), area),
        StatusBarPosition::Top => {
            let chunks =
                Layout::vertical([Constraint::Length(height), Constraint::Fill(1)]).split(area);
            (chunks[0], chunks[1])
        }
        StatusBarPosition::Bottom => {
            let chunks =
                Layout::vertical([Constraint::Fill(1), Constraint::Length(height)]).split(area);
            (chunks[1], chunks[0])
        }
    }
}

/// Compute an adaptive layout for 0–3 visible components, filling all available space
/// in order. For 4+ components, callers should use [`LayoutPreset::compute`] instead.
///
/// - 0 components → empty
/// - 1 component  → fills the entire area
/// - 2 components → side by side (equal columns)
/// - 3 components → two stacked on the left, one filling the right
pub fn compute_adaptive(area: Rect, components: &[ComponentId]) -> Vec<(ComponentId, Rect)> {
    match components {
        [] => vec![],
        [c0] => vec![(*c0, area)],
        [c0, c1] => {
            let cols = Layout::horizontal([Constraint::Fill(1), Constraint::Fill(1)]).split(area);
            vec![(*c0, cols[0]), (*c1, cols[1])]
        }
        [c0, c1, c2] => {
            let cols = Layout::horizontal([Constraint::Fill(1), Constraint::Fill(1)]).split(area);
            let left = Layout::vertical([Constraint::Fill(1), Constraint::Fill(1)]).split(cols[0]);
            vec![(*c0, left[0]), (*c1, left[1]), (*c2, cols[1])]
        }
        _ => unreachable!("compute_adaptive called with 4+ components; use LayoutPreset::compute"),
    }
}

/// Components in the order they are dropped when the terminal is too small
/// (first entry is dropped first).
pub const DROP_ORDER: [ComponentId; 4] = [
    ComponentId::Disk,
    ComponentId::Net,
    ComponentId::Cpu,
    ComponentId::Process,
];

/// Result of [`fit_layout`]: the components that fit and those that were dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FitResult {
    pub placed: Vec<(ComponentId, Rect)>,
    pub dropped: Vec<ComponentId>,
}

fn meets_min_size(placed: &[(ComponentId, Rect)]) -> bool {
    placed.iter().all(|(id, rect)| {
        let (w, h) = id.min_size();
        rect.width >= w && rect.height >= h
    })
}

/// Stack `components` vertically, each spanning the full width of `area`.
///
/// CPU gets its preferred height from `hints` (or its minimum), capped so the
/// others can still reach their minimum heights; Process takes any excess.
fn compute_stacked(
    area: Rect,
    components: &[ComponentId],
    hints: &LayoutHints,
) -> Vec<(ComponentId, Rect)> {
    let others_min: u16 = components
        .iter()
        .filter(|id| **id != ComponentId::Cpu)
        .map(|id| id.min_size().1)
        .sum();
    let has_process = components.contains(&ComponentId::Process);
    let constraints: Vec<Constraint> = components
        .iter()
        .map(|id| {
            let min_h = id.min_size().1;
            if *id == ComponentId::Cpu {
                let preferred = hints.left_top.unwrap_or(min_h);
                let cap = area.height.saturating_sub(others_min);
                Constraint::Length(preferred.min(cap).max(min_h))
            } else if *id == ComponentId::Process {
                Constraint::Min(min_h)
            } else if has_process {
                // Process benefits most from extra rows, so it takes the excess.
                Constraint::Length(min_h)
            } else {
                Constraint::Min(min_h)
            }
        })
        .collect();
    let rows = Layout::vertical(constraints).split(area);
    components
        .iter()
        .copied()
        .zip(rows.iter().copied())
        .collect()
}

/// Place `visible` components in `area`, dropping components (per [`DROP_ORDER`],
/// never the `focused` one) until every placed rect meets its minimum size.
///
/// With all four components visible the preset is tried first; after any drop, or
/// with fewer than four visible, [`compute_adaptive`] is used. Before each drop a
/// vertically stacked arrangement ([`compute_stacked`]) of the remaining candidates
/// is tried. A single remaining component fills `area` even if it is below its
/// minimum.
pub fn fit_layout(
    preset: LayoutPreset,
    area: Rect,
    overrides: &SlotOverrides,
    hints: &LayoutHints,
    visible: &[ComponentId],
    focused: ComponentId,
) -> FitResult {
    let mut candidates: Vec<ComponentId> = visible.to_vec();
    let mut dropped = Vec::new();

    let mut placed = if candidates.len() == 4 {
        let slots = preset.compute(area, overrides, hints);
        let placed: Vec<(ComponentId, Rect)> = candidates
            .iter()
            .filter_map(|id| {
                slots
                    .values()
                    .find(|(c, _)| c == id)
                    .map(|(_, rect)| (*id, *rect))
            })
            .collect();
        // A slot override can leave a component without a slot; keep it hidden
        // even if drops later switch to the adaptive layout.
        candidates.retain(|id| placed.iter().any(|(p, _)| p == id));
        placed
    } else {
        compute_adaptive(area, &candidates)
    };

    while candidates.len() > 1 && !meets_min_size(&placed) {
        // Before dropping anything, see whether stacking the candidates
        // vertically (each full width) satisfies every minimum.
        let stacked = compute_stacked(area, &candidates, hints);
        if meets_min_size(&stacked) {
            placed = stacked;
            break;
        }
        let Some(victim) = DROP_ORDER
            .iter()
            .copied()
            .find(|id| *id != focused && candidates.contains(id))
        else {
            break;
        };
        candidates.retain(|id| *id != victim);
        dropped.push(victim);
        placed = compute_adaptive(area, &candidates);
    }

    FitResult { placed, dropped }
}

impl LayoutPreset {
    pub fn compute(&self, area: Rect, overrides: &SlotOverrides, hints: &LayoutHints) -> SlotMap {
        let defaults = self.default_slots();
        let mut map = SlotMap::new();
        for (slot_id, rect) in self.split_area(area, hints) {
            let component = overrides
                .0
                .get(&slot_id)
                .copied()
                .unwrap_or_else(|| *defaults.get(&slot_id).expect("every slot has a default"));
            map.insert(slot_id, (component, rect));
        }
        map
    }

    fn default_slots(&self) -> HashMap<SlotId, ComponentId> {
        use ComponentId::*;
        use SlotId::*;
        match self {
            Self::Sidebar => HashMap::from([
                (LeftTop, Cpu),
                (LeftBot, Net),
                (LeftExtra, Disk),
                (Right, Process),
            ]),
            Self::Classic => HashMap::from([
                (TopLeft, Cpu),
                (TopRightTop, Disk),
                (TopRightBot, Net),
                (Bottom, Process),
            ]),
            Self::Dashboard => HashMap::from([
                (Top, Cpu),
                (MidLeft, Disk),
                (MidRight, Net),
                (Bottom, Process),
            ]),
            Self::Grid => HashMap::from([
                (GridLeftMid, Disk),
                (GridLeftBot, Net),
                (GridRightTop, Cpu),
                (GridRightBot, Process),
            ]),
        }
    }

    fn split_area(&self, area: Rect, hints: &LayoutHints) -> Vec<(SlotId, Rect)> {
        use SlotId::*;
        match self {
            Self::Sidebar => {
                let cols = Layout::horizontal([Constraint::Percentage(35), Constraint::Fill(1)])
                    .split(area);
                // Use preferred heights from components when available so the
                // panels are tight to their content rather than percentage-based.
                let top_constraint = hints
                    .left_top
                    .map(Constraint::Length)
                    .unwrap_or(Constraint::Percentage(30));
                let left =
                    Layout::vertical([top_constraint, Constraint::Fill(1), Constraint::Fill(1)])
                        .split(cols[0]);
                vec![
                    (LeftTop, left[0]),
                    (LeftBot, left[1]),
                    (LeftExtra, left[2]),
                    (Right, cols[1]),
                ]
            }
            Self::Classic => {
                let rows =
                    Layout::vertical([Constraint::Percentage(45), Constraint::Fill(1)]).split(area);
                let top = Layout::horizontal([Constraint::Percentage(60), Constraint::Fill(1)])
                    .split(rows[0]);
                let top_right = Layout::vertical([Constraint::Percentage(50), Constraint::Fill(1)])
                    .split(top[1]);
                vec![
                    (TopLeft, top[0]),
                    (TopRightTop, top_right[0]),
                    (TopRightBot, top_right[1]),
                    (Bottom, rows[1]),
                ]
            }
            Self::Dashboard => {
                // CPU height from hint (capped at 8 cores inside CpuComponent);
                // fall back to a minimal height until the first snapshot arrives.
                let cpu_constraint = hints
                    .left_top
                    .map(Constraint::Length)
                    .unwrap_or(Constraint::Length(5));
                let rows =
                    Layout::vertical([cpu_constraint, Constraint::Length(8), Constraint::Fill(1)])
                        .split(area);
                let mid = Layout::horizontal([Constraint::Percentage(50), Constraint::Fill(1)])
                    .split(rows[1]);
                vec![
                    (Top, rows[0]),
                    (MidLeft, mid[0]),
                    (MidRight, mid[1]),
                    (Bottom, rows[2]),
                ]
            }
            Self::Grid => {
                // Two columns: left 40% has [Disk, Net], right 60% has [Cpu, Process].
                // CPU height comes from right_top hint; Process fills the rest.
                let cols = Layout::horizontal([Constraint::Percentage(40), Constraint::Fill(1)])
                    .split(area);
                let left =
                    Layout::vertical([Constraint::Fill(1), Constraint::Fill(1)]).split(cols[0]);
                let cpu_constraint = hints
                    .right_top
                    .map(Constraint::Length)
                    .unwrap_or(Constraint::Percentage(30));
                let right = Layout::vertical([cpu_constraint, Constraint::Fill(1)]).split(cols[1]);
                vec![
                    (GridLeftMid, left[0]),
                    (GridLeftBot, left[1]),
                    (GridRightTop, right[0]),
                    (GridRightBot, right[1]),
                ]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;

    #[test]
    fn adaptive_single_component_fills_area() {
        let area = Rect::new(0, 0, 200, 50);
        let pairs = compute_adaptive(area, &[ComponentId::Net]);
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].0, ComponentId::Net);
        assert_eq!(pairs[0].1, area);
    }

    #[test]
    fn adaptive_two_components_split_horizontally() {
        let area = Rect::new(0, 0, 200, 50);
        let pairs = compute_adaptive(area, &[ComponentId::Cpu, ComponentId::Net]);
        assert_eq!(pairs.len(), 2);
        // Both occupy full height
        assert_eq!(pairs[0].1.height, area.height);
        assert_eq!(pairs[1].1.height, area.height);
        // Together they span the full width
        assert_eq!(pairs[0].1.width + pairs[1].1.width, area.width);
        // First is on the left, second is to its right
        assert_eq!(pairs[0].1.x, area.x);
        assert_eq!(pairs[1].1.x, pairs[0].1.x + pairs[0].1.width);
    }

    #[test]
    fn adaptive_three_components_two_left_one_right() {
        let area = Rect::new(0, 0, 200, 50);
        let pairs = compute_adaptive(
            area,
            &[ComponentId::Cpu, ComponentId::Net, ComponentId::Disk],
        );
        assert_eq!(pairs.len(), 3);
        let (cpu_rect, net_rect, disk_rect) = (pairs[0].1, pairs[1].1, pairs[2].1);
        // cpu and net share the left column (same x, same width)
        assert_eq!(cpu_rect.x, area.x);
        assert_eq!(net_rect.x, area.x);
        assert_eq!(cpu_rect.width, net_rect.width);
        // disk fills the right column (full height)
        assert_eq!(disk_rect.height, area.height);
        assert!(disk_rect.x > cpu_rect.x);
        // Together the two columns span the full width
        assert_eq!(cpu_rect.width + disk_rect.width, area.width);
    }

    #[test]
    fn sidebar_preset_has_slot_for_every_main_component() {
        let area = Rect::new(0, 0, 200, 50);
        let map =
            LayoutPreset::Sidebar.compute(area, &SlotOverrides::default(), &LayoutHints::default());
        let ids: std::collections::HashSet<ComponentId> = map.values().map(|(id, _)| *id).collect();
        use ComponentId::*;
        assert!(ids.contains(&Cpu));
        assert!(ids.contains(&Net));
        assert!(ids.contains(&Disk));
        assert!(ids.contains(&Process));
    }

    #[test]
    fn layout_hints_shrink_cpu_slot() {
        let area = Rect::new(0, 0, 200, 50);
        let hints = LayoutHints {
            left_top: Some(8),
            right_top: None,
        };
        let map = LayoutPreset::Sidebar.compute(area, &SlotOverrides::default(), &hints);
        let cpu_rect = map
            .values()
            .find(|(id, _)| *id == ComponentId::Cpu)
            .map(|(_, r)| r)
            .unwrap();
        assert_eq!(cpu_rect.height, 8);
    }

    #[test]
    fn status_bar_reduces_available_area() {
        let area = Rect::new(0, 0, 200, 50);
        let (bar, rest) = split_status_bar(area, StatusBarPosition::Top, 6);
        assert_eq!(bar.height, 6);
        assert_eq!(rest.height, 44);
    }

    #[test]
    fn grid_preset_has_slot_for_every_main_component() {
        let area = Rect::new(0, 0, 200, 50);
        let map =
            LayoutPreset::Grid.compute(area, &SlotOverrides::default(), &LayoutHints::default());
        let ids: std::collections::HashSet<ComponentId> = map.values().map(|(id, _)| *id).collect();
        use ComponentId::*;
        assert!(ids.contains(&Cpu));
        assert!(ids.contains(&Net));
        assert!(ids.contains(&Disk));
        assert!(ids.contains(&Process));
    }

    #[test]
    fn grid_preset_cpu_height_follows_hint() {
        let area = Rect::new(0, 0, 200, 50);
        let hints = LayoutHints {
            left_top: None,
            right_top: Some(11),
        };
        let map = LayoutPreset::Grid.compute(area, &SlotOverrides::default(), &hints);
        let cpu_rect = map
            .values()
            .find(|(id, _)| *id == ComponentId::Cpu)
            .map(|(_, r)| r)
            .unwrap();
        assert_eq!(cpu_rect.height, 11);
    }

    #[test]
    fn dashboard_preset_cpu_height_follows_hint() {
        let area = Rect::new(0, 0, 200, 50);
        let hints = LayoutHints {
            left_top: Some(11),
            right_top: None,
        };
        let map = LayoutPreset::Dashboard.compute(area, &SlotOverrides::default(), &hints);
        let cpu_rect = map
            .values()
            .find(|(id, _)| *id == ComponentId::Cpu)
            .map(|(_, r)| r)
            .unwrap();
        assert_eq!(cpu_rect.height, 11);
    }

    #[test]
    fn dashboard_preset_cpu_height_fallback_without_hint() {
        let area = Rect::new(0, 0, 200, 50);
        let map = LayoutPreset::Dashboard.compute(
            area,
            &SlotOverrides::default(),
            &LayoutHints::default(),
        );
        let cpu_rect = map
            .values()
            .find(|(id, _)| *id == ComponentId::Cpu)
            .map(|(_, r)| r)
            .unwrap();
        assert_eq!(cpu_rect.height, 5);
    }

    fn all_four() -> Vec<ComponentId> {
        use ComponentId::*;
        vec![Cpu, Net, Disk, Process]
    }

    fn fit(preset: LayoutPreset, w: u16, h: u16, focused: ComponentId) -> FitResult {
        fit_layout(
            preset,
            Rect::new(0, 0, w, h),
            &SlotOverrides::default(),
            &LayoutHints::default(),
            &all_four(),
            focused,
        )
    }

    #[test]
    fn fit_keeps_all_four_at_200x50() {
        let r = fit(LayoutPreset::Sidebar, 200, 50, ComponentId::Process);
        assert!(r.dropped.is_empty());
        assert_eq!(r.placed.len(), 4);
    }

    #[test]
    fn fit_drops_disk_first() {
        // Sidebar at 80x16: the left column is ~28 wide (< 31 for Net), so the
        // preset fails, and stacking all four needs 19 rows. The adaptive 3-panel
        // [Cpu, Net, Process] layout puts Net in a 40-wide column with 8 rows
        // (>= 31x5) and Process in 40x16, so it fits.
        let r = fit(LayoutPreset::Sidebar, 80, 16, ComponentId::Process);
        assert_eq!(r.dropped, vec![ComponentId::Disk]);
        assert_eq!(r.placed.len(), 3);
    }

    #[test]
    fn fit_never_drops_focused() {
        let r = fit(LayoutPreset::Sidebar, 60, 20, ComponentId::Disk);
        assert!(r.placed.iter().any(|(id, _)| *id == ComponentId::Disk));
    }

    #[test]
    fn fit_falls_back_to_focused_only() {
        let area = Rect::new(0, 0, 40, 8);
        let r = fit(LayoutPreset::Sidebar, 40, 8, ComponentId::Process);
        assert_eq!(r.placed, vec![(ComponentId::Process, area)]);
        assert_eq!(r.dropped.len(), 3);
    }

    #[test]
    fn fit_respects_minimums_when_not_single() {
        let sizes = [(200, 50), (120, 40), (80, 24), (60, 20), (40, 12), (30, 8)];
        for preset in [
            LayoutPreset::Sidebar,
            LayoutPreset::Classic,
            LayoutPreset::Dashboard,
            LayoutPreset::Grid,
        ] {
            for (w, h) in sizes {
                let r = fit(preset, w, h, ComponentId::Process);
                if r.placed.len() > 1 {
                    for (id, rect) in &r.placed {
                        let (mw, mh) = id.min_size();
                        assert!(
                            rect.width >= mw && rect.height >= mh,
                            "{preset:?} {w}x{h}: {id:?} got {}x{}",
                            rect.width,
                            rect.height
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn fit_stacks_cpu_and_process_at_60x20() {
        // Side-by-side columns are 30 wide (< Process min 40); stacking full-width
        // rows must keep at least CPU and Process instead of falling to Process only.
        let r = fit(LayoutPreset::Sidebar, 60, 20, ComponentId::Process);
        let cpu = r.placed.iter().find(|(id, _)| *id == ComponentId::Cpu);
        let process = r.placed.iter().find(|(id, _)| *id == ComponentId::Process);
        let (Some((_, cpu)), Some((_, process))) = (cpu, process) else {
            panic!("expected CPU and Process placed, got {:?}", r.placed);
        };
        assert_eq!(cpu.width, 60);
        assert_eq!(process.width, 60);
        assert!(cpu.bottom() <= process.top(), "CPU stacked above Process");
    }

    #[test]
    fn fit_adaptive_when_fewer_than_four_visible() {
        let area = Rect::new(0, 0, 200, 50);
        let visible = [ComponentId::Cpu, ComponentId::Net];
        let r = fit_layout(
            LayoutPreset::Sidebar,
            area,
            &SlotOverrides::default(),
            &LayoutHints::default(),
            &visible,
            ComponentId::Cpu,
        );
        assert_eq!(r.placed, compute_adaptive(area, &visible));
        assert!(r.dropped.is_empty());
    }

    #[test]
    fn fit_keeps_override_hidden_component_hidden_after_drops() {
        // Override LeftTop to Net so Cpu has no slot in the preset. When drops force
        // the adaptive layout, Cpu must not reappear (nor be reported as dropped).
        // At 60x14 stacking [Net, Disk, Process] needs 16 rows, so Disk is dropped.
        let overrides = SlotOverrides(HashMap::from([(SlotId::LeftTop, ComponentId::Net)]));
        let r = fit_layout(
            LayoutPreset::Sidebar,
            Rect::new(0, 0, 60, 14),
            &overrides,
            &LayoutHints::default(),
            &all_four(),
            ComponentId::Process,
        );
        assert!(!r.dropped.is_empty());
        assert!(!r.placed.iter().any(|(id, _)| *id == ComponentId::Cpu));
        assert!(!r.dropped.contains(&ComponentId::Cpu));
    }
}
