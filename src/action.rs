// SPDX-License-Identifier: GPL-3.0-only

use crate::stats::snapshots::{
    CpuSnapshot, DiskSnapshot, MemSnapshot, NetSnapshot, ProcSnapshot, SysSnapshot,
};
use serde::{Deserialize, Serialize};
use strum::Display;

#[derive(Debug, Clone, Display, Serialize, Deserialize)]
pub enum Action {
    // Infrastructure
    Render,
    Quit,
    #[allow(dead_code)] // reserved for platforms that support suspend (SIGTSTP)
    Suspend,
    #[allow(dead_code)] // reserved for platforms that support suspend (SIGTSTP)
    Resume,
    #[allow(dead_code)] // reserved for terminal clear-screen event handling
    ClearScreen,
    Resize(u16, u16),
    #[allow(dead_code)] // reserved for future error reporting to the UI layer
    Error(String),
    // Focus
    #[serde(skip)]
    FocusComponent(crate::components::ComponentId),
    ToggleFullScreen,
    ToggleHelp,
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
}
