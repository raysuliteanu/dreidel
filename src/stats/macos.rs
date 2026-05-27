// SPDX-License-Identifier: GPL-3.0-only

//! macOS-specific stats collection.
//!
//! All functions are called from `stats/mod.rs` behind `#[cfg(target_os = "macos")]`
//! guards. Failures are logged at `tracing::debug!` level and return zero/None so
//! the rest of the snapshot is unaffected.

use libc::{
    CPU_STATE_IDLE, CPU_STATE_NICE, CPU_STATE_SYSTEM, CPU_STATE_USER, PROCESSOR_CPU_LOAD_INFO,
    host_processor_info, mach_msg_type_number_t, natural_t, processor_cpu_load_info_data_t,
    processor_cpu_load_info_t,
};

use crate::stats::snapshots::{CpuModes, CpuSnapshot, ProcessEntry};
use sysinfo::System;

/// Per-CPU tick counts from `host_processor_info(PROCESSOR_CPU_LOAD_INFO)`.
/// Stored across ticks to compute mode-percentage deltas.
#[derive(Debug, Clone)]
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
    let mut cpu_info: processor_cpu_load_info_t = std::ptr::null_mut();
    let mut cpu_count: natural_t = 0;
    let mut info_count: mach_msg_type_number_t = 0;

    // SAFETY: Standard Mach host_processor_info call. We check the return value
    // and immediately copy out the data before calling vm_deallocate.
    // mach_host_self / mach_task_self are deprecated in libc in favour of mach2,
    // but mach2 does not expose host_processor_info, so we must use libc here.
    #[allow(deprecated)]
    let host = unsafe { libc::mach_host_self() };
    let ret = unsafe {
        host_processor_info(
            host,
            PROCESSOR_CPU_LOAD_INFO,
            &mut cpu_count,
            &mut cpu_info as *mut _ as *mut _,
            &mut info_count,
        )
    };

    if ret != libc::KERN_SUCCESS || cpu_info.is_null() {
        tracing::debug!("host_processor_info failed: {ret}");
        return (None, None);
    }

    let n = cpu_count as usize;
    let mut user = vec![0u64; n];
    let mut system = vec![0u64; n];
    let mut idle = vec![0u64; n];
    let mut nice = vec![0u64; n];

    // SAFETY: host_processor_info guarantees cpu_count valid entries.
    unsafe {
        for i in 0..n {
            let cpu = &*(cpu_info as *mut processor_cpu_load_info_data_t).add(i);
            user[i] = cpu.cpu_ticks[CPU_STATE_USER as usize] as u64;
            system[i] = cpu.cpu_ticks[CPU_STATE_SYSTEM as usize] as u64;
            idle[i] = cpu.cpu_ticks[CPU_STATE_IDLE as usize] as u64;
            nice[i] = cpu.cpu_ticks[CPU_STATE_NICE as usize] as u64;
        }
        // Deallocate the Mach memory returned by host_processor_info.
        let page_size = libc::sysconf(libc::_SC_PAGESIZE) as usize;
        let byte_count = info_count as usize * std::mem::size_of::<u32>();
        let rounded = byte_count.div_ceil(page_size) * page_size;
        #[allow(deprecated)]
        libc::vm_deallocate(
            libc::mach_task_self(),
            cpu_info as libc::vm_address_t,
            rounded,
        );
    }

    let new_ticks = MacosCpuTicks {
        user: user.clone(),
        system: system.clone(),
        idle: idle.clone(),
        nice: nice.clone(),
    };

    let modes = prev.and_then(|p| {
        if p.user.len() != n {
            return None;
        }
        let mut d_user = 0u64;
        let mut d_system = 0u64;
        let mut d_idle = 0u64;
        let mut d_nice = 0u64;
        for i in 0..n {
            d_user += user[i].saturating_sub(p.user[i]);
            d_system += system[i].saturating_sub(p.system[i]);
            d_idle += idle[i].saturating_sub(p.idle[i]);
            d_nice += nice[i].saturating_sub(p.nice[i]);
        }
        let total = d_user + d_system + d_idle + d_nice;
        if total == 0 {
            return None;
        }
        let scale = 100.0 / total as f32;
        Some(CpuModes {
            user: d_user as f32 * scale,
            system: d_system as f32 * scale,
            idle: d_idle as f32 * scale,
            nice: d_nice as f32 * scale,
            iowait: 0.0,
            irq: 0.0,
            softirq: 0.0,
            steal: 0.0,
        })
    });

    (modes, Some(new_ticks))
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
