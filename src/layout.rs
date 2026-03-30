// SPDX-License-Identifier: GPL-3.0-only

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StatusBarPosition {
    Top,
    Bottom,
    Hidden,
}

pub fn split_status_bar(area: Rect, pos: StatusBarPosition) -> (Rect, Rect) {
    match pos {
        StatusBarPosition::Hidden => (Rect::default(), area),
        StatusBarPosition::Top => {
            let chunks = Layout::vertical([Constraint::Length(4), Constraint::Fill(1)]).split(area);
            (chunks[0], chunks[1])
        }
        StatusBarPosition::Bottom => {
            let chunks = Layout::vertical([Constraint::Fill(1), Constraint::Length(4)]).split(area);
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
        let (bar, rest) = split_status_bar(area, StatusBarPosition::Top);
        assert_eq!(bar.height, 4);
        assert_eq!(rest.height, 46);
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
}
