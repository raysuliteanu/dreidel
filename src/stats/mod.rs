// SPDX-License-Identifier: GPL-3.0-only

pub mod snapshots;
pub use snapshots::*;

use crate::action::Action;
use sysinfo::{Components, DiskKind, Disks, Networks, System};
use tokio::sync::mpsc::Sender;
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};

pub fn spawn_collector(tx: Sender<Action>, token: CancellationToken, refresh_ms: u64) {
    tokio::spawn(run_collector(tx, token, refresh_ms));
}

pub async fn run_collector(tx: Sender<Action>, token: CancellationToken, refresh_ms: u64) {
    let mut sys = System::new_all();
    let mut nets = Networks::new_with_refreshed_list();
    let mut disks = Disks::new_with_refreshed_list();
    let mut components = Components::new_with_refreshed_list();
    let mut interval = tokio::time::interval(std::time::Duration::from_millis(refresh_ms));

    loop {
        tokio::select! {
            _ = token.cancelled() => break,
            _ = interval.tick() => {}
        }

        sys.refresh_all();
        nets.refresh(false);
        disks.refresh(false);
        components.refresh(false);

        let actions = [
            Action::SysUpdate(build_sys(&sys)),
            Action::CpuUpdate(build_cpu(&sys, &components)),
            Action::MemUpdate(build_mem(&sys)),
            Action::NetUpdate(build_net(&nets)),
            Action::DiskUpdate(build_disk(&disks)),
            Action::ProcUpdate(build_proc(&sys)),
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

fn build_cpu(sys: &System, components: &Components) -> CpuSnapshot {
    let cpus = sys.cpus();
    CpuSnapshot {
        per_core: cpus.iter().map(|c| c.cpu_usage()).collect(),
        aggregate: sys.global_cpu_usage(),
        frequency: cpus.iter().map(|c| c.frequency()).collect(),
        cpu_brand: cpus
            .first()
            .map(|c| c.brand().to_owned())
            .unwrap_or_default(),
        #[cfg(target_os = "linux")]
        temperature: components
            .iter()
            .find(|c| c.label().to_lowercase().contains("cpu"))
            .and_then(|c| c.temperature()),
        #[cfg(target_os = "linux")]
        physical_core_count: read_physical_core_count(),
        #[cfg(target_os = "linux")]
        governor: read_cpu_governor(),
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

/// Read the scaling governor for cpu0 from sysfs.
#[cfg(target_os = "linux")]
fn read_cpu_governor() -> Option<String> {
    std::fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor")
        .ok()
        .map(|s| s.trim().to_owned())
}

fn build_mem(sys: &System) -> MemSnapshot {
    MemSnapshot {
        ram_used: sys.used_memory(),
        ram_total: sys.total_memory(),
        swap_used: sys.used_swap(),
        swap_total: sys.total_swap(),
        #[cfg(target_os = "linux")]
        swap_in_bytes: read_vmstat_field("pswpin").unwrap_or(0) * 4096,
        #[cfg(target_os = "linux")]
        swap_out_bytes: read_vmstat_field("pswpout").unwrap_or(0) * 4096,
    }
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
                    #[cfg(target_os = "linux")]
                    tx_dropped: dev_stats.get(name).map(|s| s.sent_drop).unwrap_or(0),
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
                    cpu_time_secs: 0.0,
                };

                #[cfg(target_os = "linux")]
                if let Ok(proc) = procfs::process::Process::new(pid as i32) {
                    if let Ok(stat) = proc.stat() {
                        entry.priority = stat.priority as i32;
                        entry.nice = stat.nice as i32;
                        entry.threads = stat.num_threads as u32;
                        entry.cpu_time_secs =
                            (stat.utime + stat.stime) as f64 / procfs::ticks_per_second() as f64;
                    }
                    if let Ok(statm) = proc.statm() {
                        entry.shr_bytes = statm.shared * procfs::page_size();
                    }
                }

                entry
            })
            .collect(),
    }
}

fn map_process_status(status: sysinfo::ProcessStatus) -> ProcessStatus {
    match status {
        sysinfo::ProcessStatus::Run => ProcessStatus::Running,
        sysinfo::ProcessStatus::Sleep => ProcessStatus::Sleeping,
        sysinfo::ProcessStatus::Idle => ProcessStatus::Idle,
        sysinfo::ProcessStatus::Stop => ProcessStatus::Stopped,
        sysinfo::ProcessStatus::Zombie => ProcessStatus::Zombie,
        sysinfo::ProcessStatus::Dead => ProcessStatus::Dead,
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
        tokio::spawn(run_collector(tx, child, 100));

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
