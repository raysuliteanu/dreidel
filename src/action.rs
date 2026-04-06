// SPDX-License-Identifier: GPL-3.0-only

use crate::stats::snapshots::{
    CpuSnapshot, DiskSnapshot, MemSnapshot, NetSnapshot, ProcSnapshot, SysSnapshot,
};
use strum::Display;

#[derive(Debug, Clone, Display)]
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
    FocusComponent(crate::components::ComponentId),
    ToggleFullScreen,
    ToggleHelp,
    // Metric updates from the stats collector
    SysUpdate(SysSnapshot),
    CpuUpdate(CpuSnapshot),
    MemUpdate(MemSnapshot),
    NetUpdate(NetSnapshot),
    DiskUpdate(DiskSnapshot),
    ProcUpdate(ProcSnapshot),
}
