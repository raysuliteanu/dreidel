// SPDX-License-Identifier: GPL-3.0-only

//! macOS-specific stats collection.
//!
//! All functions are called from `stats/mod.rs` behind `#[cfg(target_os = "macos")]`
//! guards. Failures are logged at `tracing::debug!` level and return zero/None so
//! the rest of the snapshot is unaffected.

use libc::{
    CPU_STATE_IDLE, CPU_STATE_NICE, CPU_STATE_SYSTEM, CPU_STATE_USER, HOST_VM_INFO64,
    HOST_VM_INFO64_COUNT, PROCESSOR_CPU_LOAD_INFO, host_processor_info, host_statistics64,
    mach_msg_type_number_t, natural_t, processor_cpu_load_info_data_t, processor_cpu_load_info_t,
    vm_statistics64_data_t,
};

use libproc::libproc::file_info::ListFDs;
use libproc::libproc::task_info::TaskAllInfo;
use libproc::libproc::thread_info::ThreadInfo;
use libproc::proc_pid::{ListThreads, listpidinfo, pidinfo};

use crate::stats::snapshots::{CpuModes, CpuSnapshot, ProcessEntry, ProcessStatus};
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
        #[allow(deprecated)]
        libc::vm_deallocate(
            libc::mach_task_self(),
            cpu_info as libc::vm_address_t,
            info_count as usize * std::mem::size_of::<u32>(),
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
pub fn read_mem_details() -> (u64, u64, u64, u64, u64, u64, u64, u64) {
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as u64;
    if page_size == 0 {
        return (0, 0, 0, 0, 0, 0, 0, 0);
    }

    let mut vm_info = std::mem::MaybeUninit::<vm_statistics64_data_t>::zeroed();
    let mut count: libc::mach_msg_type_number_t = HOST_VM_INFO64_COUNT;

    // SAFETY: host_statistics64 fills vm_info with exactly HOST_VM_INFO64_COUNT words
    // when KERN_SUCCESS is returned.
    #[allow(deprecated)]
    let ret = unsafe {
        host_statistics64(
            libc::mach_host_self(),
            HOST_VM_INFO64,
            vm_info.as_mut_ptr() as *mut _,
            &mut count,
        )
    };

    if ret != libc::KERN_SUCCESS {
        tracing::debug!("host_statistics64(HOST_VM_INFO64) failed: {ret}");
        return (0, 0, 0, 0, 0, 0, 0, 0);
    }

    // SAFETY: KERN_SUCCESS guarantees the struct is fully initialised.
    let vm = unsafe { vm_info.assume_init() };

    let free = vm.free_count as u64 * page_size;
    let active = vm.active_count as u64 * page_size;
    let inactive = vm.inactive_count as u64 * page_size;
    let wired = vm.wire_count as u64 * page_size;
    let compressed = vm.compressor_page_count as u64 * page_size;
    let available = free + inactive;
    let swap_in = vm.pageins * page_size;
    let swap_out = vm.pageouts * page_size;

    (
        free, active, inactive, wired, compressed, available, swap_in, swap_out,
    )
}

/// Returns (rx_dropped, tx_dropped) for the named interface via `sysctl(NET_RT_IFLIST2)`.
pub fn read_net_drops(iface_name: &str) -> (u64, u64) {
    use libc::{CTL_NET, NET_RT_IFLIST2, PF_ROUTE, RTM_IFINFO2, if_msghdr2, sysctl};
    use std::mem;

    let mut mib: [libc::c_int; 6] = [CTL_NET, PF_ROUTE, 0, 0, NET_RT_IFLIST2, 0];
    let mut buf_len: libc::size_t = 0;

    // First call: query the required buffer size (null buf pointer).
    // SAFETY: Standard sysctl size-query pattern.
    let ret = unsafe {
        sysctl(
            mib.as_mut_ptr(),
            6,
            std::ptr::null_mut(),
            &mut buf_len,
            std::ptr::null_mut(),
            0,
        )
    };
    if ret != 0 || buf_len == 0 {
        return (0, 0);
    }

    let mut buf = vec![0u8; buf_len];

    // Second call: fetch the data.
    // SAFETY: buf is allocated to the size returned by the first call.
    let ret = unsafe {
        sysctl(
            mib.as_mut_ptr(),
            6,
            buf.as_mut_ptr() as *mut libc::c_void,
            &mut buf_len,
            std::ptr::null_mut(),
            0,
        )
    };
    if ret != 0 {
        return (0, 0);
    }

    let mut offset = 0usize;
    while offset + mem::size_of::<if_msghdr2>() <= buf_len {
        // SAFETY: We verified offset + size fits within buf_len.
        let hdr = unsafe { &*(buf.as_ptr().add(offset) as *const if_msghdr2) };
        let msg_len = hdr.ifm_msglen as usize;
        if msg_len == 0 {
            break;
        }

        if hdr.ifm_type as libc::c_int == RTM_IFINFO2 {
            // The sockaddr_dl carrying the interface name follows immediately after
            // the fixed if_msghdr2 header.
            let sdl_offset = offset + mem::size_of::<if_msghdr2>();
            if sdl_offset + mem::size_of::<libc::sockaddr_dl>() <= buf_len {
                // SAFETY: bounds-checked above.
                let sdl = unsafe { &*(buf.as_ptr().add(sdl_offset) as *const libc::sockaddr_dl) };
                let nlen = sdl.sdl_nlen as usize;
                if nlen > 0 && nlen <= sdl.sdl_data.len() {
                    // SAFETY: nlen is within sdl_data's bounds (checked above).
                    let name_bytes = unsafe {
                        std::slice::from_raw_parts(sdl.sdl_data.as_ptr() as *const u8, nlen)
                    };
                    if let Ok(name) = std::str::from_utf8(name_bytes)
                        && name == iface_name
                    {
                        return (hdr.ifm_data.ifi_iqdrops, hdr.ifm_snd_drops as u64);
                    }
                }
            }
        }
        offset += msg_len;
    }

    (0, 0)
}

pub fn enrich_process_entry(entry: &mut ProcessEntry, pid: u32) {
    let pid_i = pid as i32;

    // Task all info: nice, priority, tty from pbsd; threads, cpu times, faults,
    // context switches from ptinfo. One call gets everything.
    if let Ok(info) = pidinfo::<TaskAllInfo>(pid_i, 0) {
        let bsd = info.pbsd;
        let ti = info.ptinfo;

        entry.nice = bsd.pbi_nice;
        entry.priority = ti.pti_priority;

        let tty_dev = bsd.e_tdev;
        if tty_dev != u32::MAX && tty_dev != 0 {
            let major = (tty_dev >> 24) & 0xFF;
            let minor = tty_dev & 0x00FF_FFFF;
            entry.tty = Some(format!("{major}:{minor}"));
        }

        // pti_total_user / pti_total_system are nanoseconds
        entry.user_cpu_time_secs = ti.pti_total_user as f64 / 1_000_000_000.0;
        entry.system_cpu_time_secs = ti.pti_total_system as f64 / 1_000_000_000.0;
        entry.cpu_time_secs = entry.user_cpu_time_secs + entry.system_cpu_time_secs;

        entry.threads = ti.pti_threadnum.max(0) as u32;
        entry.minor_faults = ti.pti_faults.max(0) as u64;
        entry.major_faults = ti.pti_pageins.max(0) as u64;
        // macOS gives aggregate context switches only; nonvoluntary stays None
        entry.voluntary_ctxt_switches = Some(ti.pti_csw.max(0) as u64);
    }

    // FD count: list all file descriptors for this process
    // EPERM or other errors → leave fd_count as None
    if let Ok(fds) = listpidinfo::<ListFDs>(pid_i, 256) {
        entry.fd_count = Some(fds.len());
    }
}

pub fn enumerate_threads(sys: &System) -> Vec<ProcessEntry> {
    let mut thread_entries = Vec::new();

    for (sysinfo_pid, p) in sys.processes() {
        let pid = sysinfo_pid.as_u32();
        let pid_i = pid as i32;
        let proc_name = p.name().to_string_lossy();
        let proc_user = p.user_id().map(|u| u.to_string()).unwrap_or_default();

        // Use the thread count from TaskAllInfo as the capacity hint; fall back to a
        // small default so we still attempt enumeration even without task info.
        let hint = pidinfo::<TaskAllInfo>(pid_i, 0)
            .map(|info| info.ptinfo.pti_threadnum as usize)
            .unwrap_or(4);

        let Ok(tids) = listpidinfo::<ListThreads>(pid_i, hint) else {
            continue;
        };

        for tid in tids {
            // On macOS the main thread's TID is not the same as PID (unlike Linux).
            // Skip the first TID only if it happens to equal the PID — this is a
            // conservative heuristic; macOS does not guarantee TID == PID.
            if tid as u32 == pid {
                continue;
            }

            let cpu_time_secs = if let Ok(info) = pidinfo::<ThreadInfo>(pid_i, tid) {
                // pth_user_time and pth_system_time are in microseconds
                (info.pth_user_time + info.pth_system_time) as f64 / 1_000_000.0
            } else {
                0.0
            };

            thread_entries.push(ProcessEntry {
                pid: tid as u32,
                name: format!("[{proc_name}:{tid}]"),
                cmd: Vec::new(),
                user: proc_user.clone(),
                cpu_pct: 0.0,
                mem_bytes: 0,
                mem_pct: 0.0,
                virt_bytes: 0,
                status: ProcessStatus::Unknown,
                start_time: 0,
                run_time: 0,
                nice: 0,
                threads: 0,
                read_bytes: 0,
                write_bytes: 0,
                parent_pid: Some(pid),
                priority: 0,
                shr_bytes: 0,
                cpu_time_secs,
                exe: None,
                cwd: None,
                root: None,
                effective_user: None,
                group: None,
                effective_group: None,
                session_id: None,
                tty: None,
                user_cpu_time_secs: 0.0,
                system_cpu_time_secs: 0.0,
                minor_faults: 0,
                major_faults: 0,
                voluntary_ctxt_switches: None,
                nonvoluntary_ctxt_switches: None,
                fd_count: None,
                swap_bytes: None,
                io_read_calls: None,
                io_write_calls: None,
                io_read_chars: None,
                io_write_chars: None,
                cancelled_write_bytes: None,
                is_thread: true,
            });
        }
    }

    thread_entries
}
