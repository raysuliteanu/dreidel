// SPDX-License-Identifier: GPL-3.0-only

//! Background stats collector.
//!
//! [`spawn_collector`] launches a Tokio task that refreshes system metrics on
//! two intervals: a fast cadence (default 1 s) for CPU, memory, network, disk,
//! and process data, and a slow cadence (default 5 s) for per-process thread
//! enumeration (Linux only). Each tick builds snapshot structs and sends them
//! as [`Action`] variants on a bounded `mpsc` channel.

pub mod snapshots;
pub use snapshots::*;

use crate::action::Action;
use sysinfo::{Components, DiskKind, Disks, Networks, System};
use tokio::sync::mpsc::Sender;
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};

pub fn spawn_collector(
    tx: Sender<Action>,
    token: CancellationToken,
    refresh_ms: u64,
    thread_refresh_ms: u64,
) {
    tokio::spawn(run_collector(tx, token, refresh_ms, thread_refresh_ms));
}

pub async fn run_collector(
    tx: Sender<Action>,
    token: CancellationToken,
    refresh_ms: u64,
    thread_refresh_ms: u64,
) {
    let mut sys = System::new_all();
    let mut nets = Networks::new_with_refreshed_list();
    let mut disks = Disks::new_with_refreshed_list();
    let mut components = Components::new_with_refreshed_list();

    let mut fast_interval = tokio::time::interval(std::time::Duration::from_millis(refresh_ms));
    let mut slow_interval =
        tokio::time::interval(std::time::Duration::from_millis(thread_refresh_ms));

    // Cached thread entries from the most recent slow-tick enumeration.
    // Merged into ProcUpdate on every fast tick so the UI always has thread
    // data, but the expensive /proc/<pid>/task/ walk only happens at the
    // slower cadence.
    #[allow(unused_mut)] // mut only needed on linux
    let mut cached_threads: Vec<ProcessEntry> = Vec::new();

    // Previous aggregate CPU jiffies, used to compute mode-percentage deltas
    // between ticks. Linux-only; on other platforms the state stays `None` and
    // `CpuSnapshot::cpu_modes` is never populated.
    #[cfg(target_os = "linux")]
    let mut prev_cpu_total: Option<CpuTotals> = None;

    loop {
        // Wait for whichever interval fires first.  When both are ready
        // simultaneously tokio::select! picks one — the slow tick runs its
        // enumeration and the fast tick runs on the next iteration.
        let slow_tick = tokio::select! {
            _ = token.cancelled() => break,
            _ = slow_interval.tick() => true,
            _ = fast_interval.tick() => false,
        };

        sys.refresh_all();
        nets.refresh(false);
        disks.refresh(false);
        components.refresh(false);

        // Enumerate threads only on the slow cadence.
        #[cfg(target_os = "linux")]
        if slow_tick {
            cached_threads = enumerate_threads(&sys);
        }
        // Suppress unused-variable warning on non-Linux.
        #[cfg(not(target_os = "linux"))]
        let _ = slow_tick;

        let mut proc_snap = build_proc(&sys);
        proc_snap.processes.extend(cached_threads.clone());

        #[cfg(target_os = "linux")]
        let cpu_snap = build_cpu(&sys, &components, &mut prev_cpu_total);
        #[cfg(not(target_os = "linux"))]
        let cpu_snap = build_cpu(&sys, &components);

        let actions = [
            Action::SysUpdate(build_sys(&sys)),
            Action::CpuUpdate(cpu_snap),
            Action::MemUpdate(build_mem(&sys)),
            Action::NetUpdate(build_net(&nets)),
            Action::DiskUpdate(build_disk(&disks)),
            Action::ProcUpdate(proc_snap),
        ];

        for action in actions {
            match tx.try_send(action) {
                Ok(_) => {}
                Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                    debug!("stats collector: channel full, dropping update");
                }
                Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                    warn!("stats collector: channel closed, stopping");
                    return;
                }
            }
        }
    }
}

fn build_sys(_sys: &System) -> SysSnapshot {
    SysSnapshot {
        hostname: System::host_name().unwrap_or_default(),
        uptime: System::uptime(),
        load_avg: {
            let la = System::load_average();
            [la.one, la.five, la.fifteen]
        },
        timestamp: chrono::Local::now(),
    }
}

/// Snapshot of raw CPU jiffies from `/proc/stat`, used to compute per-mode
/// percentage deltas between collector ticks.
#[cfg(target_os = "linux")]
#[derive(Debug, Clone, Copy)]
struct CpuTotals {
    user: u64,
    nice: u64,
    system: u64,
    idle: u64,
    iowait: u64,
    irq: u64,
    softirq: u64,
    steal: u64,
}

#[cfg(target_os = "linux")]
fn read_cpu_totals() -> Option<CpuTotals> {
    use procfs::CurrentSI;
    let stats = procfs::KernelStats::current().ok()?;
    let t = stats.total;
    Some(CpuTotals {
        user: t.user,
        nice: t.nice,
        system: t.system,
        idle: t.idle,
        iowait: t.iowait.unwrap_or(0),
        irq: t.irq.unwrap_or(0),
        softirq: t.softirq.unwrap_or(0),
        steal: t.steal.unwrap_or(0),
    })
}

/// Compute mode percentages from two CPU jiffy snapshots. Returns `None` when
/// the delta is zero (e.g. suspended system or back-to-back reads).
#[cfg(target_os = "linux")]
fn compute_cpu_modes(prev: CpuTotals, curr: CpuTotals) -> Option<CpuModes> {
    let d_user = curr.user.saturating_sub(prev.user);
    let d_nice = curr.nice.saturating_sub(prev.nice);
    let d_system = curr.system.saturating_sub(prev.system);
    let d_idle = curr.idle.saturating_sub(prev.idle);
    let d_iowait = curr.iowait.saturating_sub(prev.iowait);
    let d_irq = curr.irq.saturating_sub(prev.irq);
    let d_softirq = curr.softirq.saturating_sub(prev.softirq);
    let d_steal = curr.steal.saturating_sub(prev.steal);

    let total = d_user + d_nice + d_system + d_idle + d_iowait + d_irq + d_softirq + d_steal;
    if total == 0 {
        return None;
    }
    let scale = 100.0 / total as f32;
    Some(CpuModes {
        user: d_user as f32 * scale,
        nice: d_nice as f32 * scale,
        system: d_system as f32 * scale,
        idle: d_idle as f32 * scale,
        iowait: d_iowait as f32 * scale,
        irq: d_irq as f32 * scale,
        softirq: d_softirq as f32 * scale,
        steal: d_steal as f32 * scale,
    })
}

#[cfg(target_os = "linux")]
fn build_cpu(
    sys: &System,
    components: &Components,
    prev_cpu_total: &mut Option<CpuTotals>,
) -> CpuSnapshot {
    let cpus = sys.cpus();
    let cpu_modes = read_cpu_totals().and_then(|curr| {
        let modes = prev_cpu_total.and_then(|prev| compute_cpu_modes(prev, curr));
        *prev_cpu_total = Some(curr);
        modes
    });
    CpuSnapshot {
        per_core: cpus.iter().map(|c| c.cpu_usage()).collect(),
        aggregate: sys.global_cpu_usage(),
        frequency: cpus.iter().map(|c| c.frequency()).collect(),
        cpu_brand: cpus
            .first()
            .map(|c| c.brand().to_owned())
            .unwrap_or_default(),
        package_temp: components
            .iter()
            .find(|c| {
                let l = c.label().to_lowercase();
                l.contains("package") || l.ends_with(" cpu") || l == "cpu"
            })
            .and_then(|c| c.temperature()),
        per_core_temp: build_per_core_temps(components, cpus.len()),
        physical_core_count: read_physical_core_count(),
        governor: read_cpu_governor(),
        cpu_modes,
    }
}

#[cfg(not(target_os = "linux"))]
fn build_cpu(sys: &System, _components: &Components) -> CpuSnapshot {
    let cpus = sys.cpus();
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
        physical_core_count: None,
        governor: None,
        cpu_modes: None,
    }
}

/// Read the number of physical cores from /proc/cpuinfo by counting unique
/// (physical id, core id) pairs.
#[cfg(target_os = "linux")]
fn read_physical_core_count() -> Option<u32> {
    let content = std::fs::read_to_string("/proc/cpuinfo").ok()?;
    let mut pairs: std::collections::HashSet<(String, String)> = std::collections::HashSet::new();
    let mut physical_id: Option<String> = None;
    let mut core_id: Option<String> = None;
    for line in content.lines() {
        if let Some(val) = line
            .strip_prefix("physical id")
            .and_then(|s| s.strip_prefix('\t').or(s.strip_prefix(' ')))
            .and_then(|s| s.strip_prefix(':'))
        {
            physical_id = Some(val.trim().to_owned());
        } else if let Some(val) = line
            .strip_prefix("core id")
            .and_then(|s| s.strip_prefix('\t').or(s.strip_prefix(' ')))
            .and_then(|s| s.strip_prefix(':'))
        {
            core_id = Some(val.trim().to_owned());
        } else if line.is_empty()
            && let (Some(p), Some(c)) = (physical_id.take(), core_id.take())
        {
            pairs.insert((p, c));
        }
    }
    // Flush the last block (file may not end with a blank line)
    if let (Some(p), Some(c)) = (physical_id, core_id) {
        pairs.insert((p, c));
    }
    if pairs.is_empty() {
        None
    } else {
        Some(pairs.len() as u32)
    }
}

/// Build per-logical-core temperature vec by mapping physical core sensors
/// (from `sysinfo::Components` with labels like "Core 0", "Core 4") to
/// logical core indices via `/sys/devices/system/cpu/cpuN/topology/core_id`.
#[cfg(target_os = "linux")]
fn build_per_core_temps(components: &Components, logical_count: usize) -> Vec<Option<f32>> {
    // Step 1: collect physical_core_id → temperature from hwmon sensors.
    let mut phys_temp: std::collections::HashMap<u32, f32> = std::collections::HashMap::new();
    for c in components.iter() {
        let label = c.label();
        // sysinfo labels are "<driver> <sensor_label>", e.g.
        // "coretemp Core 0", "coretemp Package id 0".
        // We want the "Core N" part regardless of driver prefix.
        if let Some(pos) = label.find("Core ")
            && let Some(rest) = label.get(pos + 5..)
            && let Ok(phys_id) = rest.trim().parse::<u32>()
            && let Some(temp) = c.temperature()
        {
            phys_temp.insert(phys_id, temp);
        }
    }

    if phys_temp.is_empty() {
        return vec![None; logical_count];
    }

    // Step 2: map logical core index → physical core id via sysfs topology.
    (0..logical_count)
        .map(|cpu| {
            let path = format!("/sys/devices/system/cpu/cpu{cpu}/topology/core_id");
            std::fs::read_to_string(&path)
                .ok()
                .and_then(|s| s.trim().parse::<u32>().ok())
                .and_then(|phys_id| phys_temp.get(&phys_id).copied())
        })
        .collect()
}

/// Read the scaling governor for cpu0 from sysfs.
#[cfg(target_os = "linux")]
fn read_cpu_governor() -> Option<String> {
    std::fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor")
        .ok()
        .map(|s| s.trim().to_owned())
}

fn build_mem(sys: &System) -> MemSnapshot {
    #[cfg(target_os = "linux")]
    let (ram_free, ram_buffers, ram_cached, ram_available) = read_mem_details();
    #[cfg(not(target_os = "linux"))]
    let (ram_free, ram_buffers, ram_cached, ram_available) = (0u64, 0u64, 0u64, 0u64);

    MemSnapshot {
        ram_used: sys.used_memory(),
        ram_total: sys.total_memory(),
        ram_free,
        ram_buffers,
        ram_cached,
        ram_available,
        swap_used: sys.used_swap(),
        swap_total: sys.total_swap(),
        #[cfg(target_os = "linux")]
        swap_in_bytes: read_vmstat_field("pswpin").unwrap_or(0) * 4096,
        #[cfg(not(target_os = "linux"))]
        swap_in_bytes: 0,
        #[cfg(target_os = "linux")]
        swap_out_bytes: read_vmstat_field("pswpout").unwrap_or(0) * 4096,
        #[cfg(not(target_os = "linux"))]
        swap_out_bytes: 0,
    }
}

/// Read `MemFree`, `Buffers`, `Cached`, and `MemAvailable` from `/proc/meminfo`
/// via procfs. Returns zeros on any read failure.
#[cfg(target_os = "linux")]
fn read_mem_details() -> (u64, u64, u64, u64) {
    use procfs::Current;
    let Ok(mi) = procfs::Meminfo::current() else {
        return (0, 0, 0, 0);
    };
    (
        mi.mem_free,
        mi.buffers,
        mi.cached,
        mi.mem_available.unwrap_or(0),
    )
}

/// Read a single numeric field from /proc/vmstat (Linux only).
/// pswpin/pswpout are cumulative page counts; caller multiplies by PAGE_SIZE (4096) for bytes.
#[cfg(target_os = "linux")]
fn read_vmstat_field(field: &str) -> Option<u64> {
    let content = std::fs::read_to_string("/proc/vmstat").ok()?;
    content
        .lines()
        .find(|l| l.starts_with(field))?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}

fn build_net(nets: &Networks) -> NetSnapshot {
    #[cfg(target_os = "linux")]
    let dev_stats = procfs::net::dev_status().unwrap_or_default();

    NetSnapshot {
        interfaces: nets
            .iter()
            .map(|(name, data)| {
                let ip_addresses = data
                    .ip_networks()
                    .iter()
                    .map(|n| format!("{}/{}", n.addr, n.prefix))
                    .collect();

                InterfaceSnapshot {
                    name: name.clone(),
                    rx_bytes: data.received(),
                    tx_bytes: data.transmitted(),
                    rx_packets: data.packets_received(),
                    tx_packets: data.packets_transmitted(),
                    rx_errors: data.errors_on_received(),
                    tx_errors: data.errors_on_transmitted(),
                    total_rx_bytes: data.total_received(),
                    total_tx_bytes: data.total_transmitted(),
                    mac_address: data.mac_address().to_string(),
                    ip_addresses,
                    mtu: data.mtu(),
                    #[cfg(target_os = "linux")]
                    rx_dropped: dev_stats.get(name).map(|s| s.recv_drop).unwrap_or(0),
                    #[cfg(not(target_os = "linux"))]
                    rx_dropped: 0,
                    #[cfg(target_os = "linux")]
                    tx_dropped: dev_stats.get(name).map(|s| s.sent_drop).unwrap_or(0),
                    #[cfg(not(target_os = "linux"))]
                    tx_dropped: 0,
                }
            })
            .collect(),
    }
}

fn build_disk(disks: &Disks) -> DiskSnapshot {
    // sysinfo iterates mount points, so the same physical device can appear
    // multiple times (e.g. bind mounts). Keep only the first occurrence of
    // each device name so the UI doesn't show duplicates.
    let mut seen = std::collections::HashSet::new();
    DiskSnapshot {
        devices: disks
            .iter()
            .filter(|d| seen.insert(d.name().to_string_lossy().into_owned()))
            .map(|d| {
                let usage = d.usage();
                DiskDeviceSnapshot {
                    name: d.name().to_string_lossy().into_owned(),
                    read_bytes: usage.read_bytes,
                    write_bytes: usage.written_bytes,
                    usage_pct: if d.total_space() > 0 {
                        100.0 * (d.total_space() - d.available_space()) as f32
                            / d.total_space() as f32
                    } else {
                        0.0
                    },
                    total_read_bytes: usage.total_read_bytes,
                    total_write_bytes: usage.total_written_bytes,
                    kind: match d.kind() {
                        DiskKind::HDD => "HDD".into(),
                        DiskKind::SSD => "SSD".into(),
                        DiskKind::Unknown(_) => "Unknown".into(),
                    },
                    file_system: d.file_system().to_string_lossy().into_owned(),
                    mount_point: d.mount_point().to_string_lossy().into_owned(),
                    is_removable: d.is_removable(),
                    is_read_only: d.is_read_only(),
                    total_space: d.total_space(),
                    available_space: d.available_space(),
                }
            })
            .collect(),
    }
}

fn build_proc(sys: &System) -> ProcSnapshot {
    ProcSnapshot {
        processes: sys
            .processes()
            .values()
            .map(|p| {
                let disk = p.disk_usage();
                let pid = p.pid().as_u32();
                let mut entry = ProcessEntry {
                    pid,
                    name: p.name().to_string_lossy().into_owned(),
                    cmd: p
                        .cmd()
                        .iter()
                        .map(|s| s.to_string_lossy().into_owned())
                        .collect(),
                    user: p.user_id().map(|u| u.to_string()).unwrap_or_default(),
                    cpu_pct: p.cpu_usage(),
                    mem_bytes: p.memory(),
                    mem_pct: if sys.total_memory() > 0 {
                        100.0 * p.memory() as f32 / sys.total_memory() as f32
                    } else {
                        0.0
                    },
                    virt_bytes: p.virtual_memory(),
                    status: map_process_status(p.status()),
                    start_time: p.start_time(),
                    run_time: p.run_time(),
                    nice: 0,
                    threads: 0,
                    read_bytes: disk.read_bytes,
                    write_bytes: disk.written_bytes,
                    parent_pid: p.parent().map(|pid| pid.as_u32()),
                    priority: 0,
                    shr_bytes: 0,
                    cpu_time_secs: p.accumulated_cpu_time() as f64 / 1000.0,
                    exe: p.exe().map(|path| path.to_string_lossy().into_owned()),
                    cwd: p.cwd().map(|path| path.to_string_lossy().into_owned()),
                    root: p.root().map(|path| path.to_string_lossy().into_owned()),
                    effective_user: p.effective_user_id().map(|u| u.to_string()),
                    group: p.group_id().map(|g| g.to_string()),
                    effective_group: p.effective_group_id().map(|g| g.to_string()),
                    session_id: p.session_id().map(|sid| sid.as_u32()),
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
                    is_thread: false,
                };

                #[cfg(target_os = "linux")]
                if let Ok(proc) = procfs::process::Process::new(pid as i32) {
                    if let Ok(stat) = proc.stat() {
                        entry.priority = stat.priority as i32;
                        entry.nice = stat.nice as i32;
                        entry.threads = stat.num_threads as u32;
                        entry.user_cpu_time_secs =
                            stat.utime as f64 / procfs::ticks_per_second() as f64;
                        entry.system_cpu_time_secs =
                            stat.stime as f64 / procfs::ticks_per_second() as f64;
                        entry.cpu_time_secs = entry.user_cpu_time_secs + entry.system_cpu_time_secs;
                        entry.minor_faults = stat.minflt;
                        entry.major_faults = stat.majflt;
                        let (tty_major, tty_minor) = stat.tty_nr();
                        if tty_major != 0 || tty_minor != 0 {
                            entry.tty = Some(format!("{tty_major}:{tty_minor}"));
                        }
                    }
                    if let Ok(statm) = proc.statm() {
                        entry.shr_bytes = statm.shared * procfs::page_size();
                    }
                    if let Ok(status) = proc.status() {
                        entry.voluntary_ctxt_switches = status.voluntary_ctxt_switches;
                        entry.nonvoluntary_ctxt_switches = status.nonvoluntary_ctxt_switches;
                        entry.swap_bytes = status.vmswap.map(|kb| kb * 1024);
                    }
                    if let Ok(io) = proc.io() {
                        entry.io_read_calls = Some(io.syscr);
                        entry.io_write_calls = Some(io.syscw);
                        entry.io_read_chars = Some(io.rchar);
                        entry.io_write_chars = Some(io.wchar);
                        entry.cancelled_write_bytes = Some(io.cancelled_write_bytes);
                    }
                    if let Ok(fd_count) = proc.fd_count() {
                        entry.fd_count = Some(fd_count);
                    }
                }

                entry
            })
            .collect(),
    }
}

/// Enumerate threads for every process via `/proc/<pid>/task/`.
///
/// This is expensive (thousands of syscalls) and is called on a slower cadence
/// than the main stats refresh.  Returns standalone `ProcessEntry` items with
/// `is_thread = true` that get merged into the `ProcSnapshot` by the collector.
#[cfg(target_os = "linux")]
fn enumerate_threads(sys: &System) -> Vec<ProcessEntry> {
    let tps = procfs::ticks_per_second() as f64;
    let mut thread_entries = Vec::new();

    for (sysinfo_pid, p) in sys.processes() {
        let pid = sysinfo_pid.as_u32();
        let proc_name = p.name().to_string_lossy();
        let proc_user = p.user_id().map(|u| u.to_string()).unwrap_or_default();

        if let Ok(proc) = procfs::process::Process::new(pid as i32)
            && let Ok(tasks) = proc.tasks()
        {
            for task_result in tasks {
                let Ok(task) = task_result else { continue };
                let tid = task.tid as u32;
                // Skip the main thread (TID == PID).
                if tid == pid {
                    continue;
                }
                if let Ok(stat) = task.stat() {
                    thread_entries.push(ProcessEntry {
                        pid: tid,
                        name: format!("[{proc_name}:{tid}]"),
                        cmd: Vec::new(),
                        user: proc_user.clone(),
                        cpu_pct: 0.0,
                        mem_bytes: 0,
                        mem_pct: 0.0,
                        virt_bytes: 0,
                        status: match stat.state() {
                            Ok(procfs::process::ProcState::Running) => ProcessStatus::Running,
                            Ok(procfs::process::ProcState::Sleeping) => ProcessStatus::Sleeping,
                            Ok(procfs::process::ProcState::Waiting) => ProcessStatus::Sleeping,
                            Ok(
                                procfs::process::ProcState::Stopped
                                | procfs::process::ProcState::Tracing,
                            ) => ProcessStatus::Stopped,
                            Ok(procfs::process::ProcState::Zombie) => ProcessStatus::Zombie,
                            Ok(procfs::process::ProcState::Dead) => ProcessStatus::Dead,
                            Ok(procfs::process::ProcState::Idle) => ProcessStatus::Idle,
                            _ => ProcessStatus::Unknown,
                        },
                        start_time: 0,
                        run_time: 0,
                        nice: stat.nice as i32,
                        threads: 0,
                        read_bytes: 0,
                        write_bytes: 0,
                        parent_pid: Some(pid),
                        priority: stat.priority as i32,
                        shr_bytes: 0,
                        cpu_time_secs: (stat.utime + stat.stime) as f64 / tps,
                        exe: None,
                        cwd: None,
                        root: None,
                        effective_user: None,
                        group: None,
                        effective_group: None,
                        session_id: None,
                        tty: None,
                        user_cpu_time_secs: stat.utime as f64 / tps,
                        system_cpu_time_secs: stat.stime as f64 / tps,
                        minor_faults: stat.minflt,
                        major_faults: stat.majflt,
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
        }
    }

    thread_entries
}

fn map_process_status(status: sysinfo::ProcessStatus) -> ProcessStatus {
    match status {
        sysinfo::ProcessStatus::Run => ProcessStatus::Running,
        sysinfo::ProcessStatus::Sleep => ProcessStatus::Sleeping,
        sysinfo::ProcessStatus::Idle => ProcessStatus::Idle,
        sysinfo::ProcessStatus::Stop => ProcessStatus::Stopped,
        sysinfo::ProcessStatus::Zombie => ProcessStatus::Zombie,
        sysinfo::ProcessStatus::Dead => ProcessStatus::Dead,
        // Variants not yet mapped — update if sysinfo adds states we care about.
        // As of sysinfo 0.38: Tracing, Wakekill, Waking, Parked, LockBlocked,
        // UninterruptibleDiskSleep, Suspended, Unknown fall through here.
        _ => ProcessStatus::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn collector_sends_cpu_update() {
        let (tx, mut rx) = mpsc::channel(32);
        let token = tokio_util::sync::CancellationToken::new();
        let child = token.child_token();
        tokio::spawn(run_collector(tx, child, 100, 500));

        let mut got_cpu = false;
        for _ in 0..20 {
            if let Ok(action) =
                tokio::time::timeout(std::time::Duration::from_millis(500), rx.recv()).await
                && matches!(action, Some(crate::action::Action::CpuUpdate(_)))
            {
                got_cpu = true;
                break;
            }
        }
        token.cancel();
        assert!(got_cpu, "expected CpuUpdate from collector");
    }
}
