// SPDX-License-Identifier: GPL-3.0-only

//! macOS-specific stats collection.
//!
//! All functions are called from `stats/mod.rs` behind `#[cfg(target_os = "macos")]`
//! guards. Failures are logged at `tracing::debug!` level and return zero/None so
//! the rest of the snapshot is unaffected.

use crate::stats::snapshots::{CpuModes, CpuSnapshot, ProcessEntry};
use sysinfo::System;

/// Per-CPU tick counts from `host_processor_info(PROCESSOR_CPU_LOAD_INFO)`.
/// Stored across ticks to compute mode-percentage deltas.
#[derive(Debug, Clone)]
#[allow(dead_code)] // fields used in Task 4: CPU modes implementation
pub struct MacosCpuTicks {
    pub user: Vec<u64>,
    pub system: Vec<u64>,
    pub idle: Vec<u64>,
    pub nice: Vec<u64>,
}

pub fn read_physical_core_count() -> Option<u32> {
    sysinfo::System::physical_core_count().map(|n| n as u32)
}

/// Returns `None` on the first call (no prior snapshot for delta) and on error.
pub fn read_cpu_modes(prev: Option<&MacosCpuTicks>) -> (Option<CpuModes>, Option<MacosCpuTicks>) {
    let _ = prev;
    (None, None)
}

pub fn build_cpu_macos(sys: &System, prev_ticks: &mut Option<MacosCpuTicks>) -> CpuSnapshot {
    let cpus = sys.cpus();
    let (cpu_modes, new_ticks) = read_cpu_modes(prev_ticks.as_ref());
    *prev_ticks = new_ticks;
    CpuSnapshot {
        per_core: cpus.iter().map(|c| c.cpu_usage()).collect(),
        aggregate: sys.global_cpu_usage(),
        frequency: cpus.iter().map(|c| c.frequency()).collect(),
        cpu_brand: cpus
            .first()
            .map(|c| c.brand().to_owned())
            .unwrap_or_default(),
        package_temp: None,
        per_core_temp: vec![None; cpus.len()],
        physical_core_count: read_physical_core_count(),
        governor: None,
        cpu_modes,
    }
}

/// Returns (free, active, inactive, wired, compressed, available, swap_in_bytes, swap_out_bytes).
#[allow(dead_code)] // used in Task 5: macOS memory details
pub fn read_mem_details() -> (u64, u64, u64, u64, u64, u64, u64, u64) {
    (0, 0, 0, 0, 0, 0, 0, 0)
}

/// Returns (rx_dropped, tx_dropped) for the named interface.
#[allow(dead_code)] // used in Task 6: network drop counters
pub fn read_net_drops(_iface_name: &str) -> (u64, u64) {
    (0, 0)
}

#[allow(dead_code)] // used in Task 7: process enrichment via libproc
pub fn enrich_process_entry(_entry: &mut ProcessEntry, _pid: u32) {}

pub fn enumerate_threads(_sys: &System) -> Vec<ProcessEntry> {
    Vec::new()
}
