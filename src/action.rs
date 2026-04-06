// SPDX-License-Identifier: GPL-3.0-only

//! The [`Action`] enum — the single message type on the application’s action bus.
//!
//! Every piece of app logic (stats updates, UI state changes, infrastructure
//! events) is expressed as an `Action` variant and dispatched through a bounded
//! `tokio::mpsc` channel from the stats collector to `App` (in `app.rs`),
//! which fans it out to each [`Component`](crate::components::Component).

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
