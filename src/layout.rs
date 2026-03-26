use ratatui::layout::{Constraint, Layout, Rect};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use strum::{Display, EnumString};

use crate::components::ComponentId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SlotId {
    // sidebar preset
    LeftTop,
    LeftMid,
    LeftBot,
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
            let chunks = Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).split(area);
            (chunks[0], chunks[1])
        }
        StatusBarPosition::Bottom => {
            let chunks = Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).split(area);
            (chunks[1], chunks[0])
        }
    }
}

impl LayoutPreset {
    pub fn compute(&self, area: Rect, overrides: &SlotOverrides) -> SlotMap {
        let defaults = self.default_slots();
        let mut map = SlotMap::new();
        for (slot_id, rect) in self.split_area(area) {
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
                (LeftMid, Mem),
                (LeftBot, Net),
                (Right, Process),
            ]),
            Self::Classic => HashMap::from([
                (TopLeft, Cpu),
                (TopRightTop, Mem),
                (TopRightBot, Net),
                (Bottom, Process),
            ]),
            Self::Dashboard => HashMap::from([
                (Top, Cpu),
                (MidLeft, Mem),
                (MidRight, Net),
                (Bottom, Process),
            ]),
        }
    }

    fn split_area(&self, area: Rect) -> Vec<(SlotId, Rect)> {
        use SlotId::*;
        match self {
            Self::Sidebar => {
                let cols = Layout::horizontal([Constraint::Percentage(35), Constraint::Fill(1)])
                    .split(area);
                let left = Layout::vertical([
                    Constraint::Percentage(40),
                    Constraint::Percentage(30),
                    Constraint::Fill(1),
                ])
                .split(cols[0]);
                vec![
                    (LeftTop, left[0]),
                    (LeftMid, left[1]),
                    (LeftBot, left[2]),
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
                let rows = Layout::vertical([
                    Constraint::Length(5),
                    Constraint::Length(8),
                    Constraint::Fill(1),
                ])
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;

    #[test]
    fn sidebar_preset_allocates_right_column_to_proc() {
        let area = Rect::new(0, 0, 200, 50);
        let map = LayoutPreset::Sidebar.compute(area, &SlotOverrides::default());
        assert!(map.contains_key(&SlotId::Right));
    }

    #[test]
    fn status_bar_reduces_available_area() {
        let area = Rect::new(0, 0, 200, 50);
        let (bar, rest) = split_status_bar(area, StatusBarPosition::Top);
        assert_eq!(bar.height, 1);
        assert_eq!(rest.height, 49);
    }
}
