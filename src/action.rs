#![allow(unused)]

use crate::stats::snapshots::{
    CpuSnapshot, DiskSnapshot, MemSnapshot, NetSnapshot, ProcSnapshot, SysSnapshot,
};
use serde::{Deserialize, Serialize};
use strum::Display;

#[derive(Debug, Clone, Display, Serialize, Deserialize)]
pub enum Action {
    // Infrastructure
    Tick,
    Render,
    Quit,
    Suspend,
    Resume,
    ClearScreen,
    Resize(u16, u16),
    Error(String),
    // Focus
    #[serde(skip)]
    FocusComponent(crate::components::ComponentId),
    ToggleFullScreen,
    ToggleDebug,
    // Metric updates — payloads are not serializable so skipped in serde
    #[serde(skip)]
    SysUpdate(SysSnapshot),
    #[serde(skip)]
    CpuUpdate(CpuSnapshot),
    #[serde(skip)]
    MemUpdate(MemSnapshot),
    #[serde(skip)]
    NetUpdate(NetSnapshot),
    #[serde(skip)]
    DiskUpdate(DiskSnapshot),
    #[serde(skip)]
    ProcUpdate(ProcSnapshot),
    // Debug
    DebugSnapshot(String),
}

impl Action {
    /// Returns true if two Actions are the same variant, ignoring payload.
    /// Use this for filtering Tick/Render from debug logs, not for equality checks.
    pub fn same_variant(&self, other: &Self) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
}
