// SPDX-License-Identifier: GPL-3.0-only

//! Background stats collector.
//!
//! [`spawn_collector`] launches a dedicated OS thread, running a current-thread
//! Tokio runtime, that refreshes system metrics on two intervals: a fast
//! cadence (default 1 s) for CPU, GPU, memory, network, disk, and process data,
//! and a slow cadence (default 5 s) for per-process thread enumeration (Linux
//! and macOS). Each tick builds snapshot structs and sends them as [`Action`]
//! variants on a bounded `mpsc` channel.
//!
//! The collector gets its own thread because `sysinfo::Gpus` is not `Send` on
//! Linux (it holds `dlopen`ed NVML/Vulkan handles), and because sysinfo's
//! refreshes are blocking calls that shouldn't occupy a worker of the main
//! runtime.

pub mod snapshots;
pub use snapshots::*;

#[cfg(target_os = "macos")]
mod macos;

use crate::action::Action;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use sysinfo::Components;
use sysinfo::{
    DiskKind, DiskRefreshKind, Disks, Gpus, Networks, ProcessRefreshKind, RefreshKind, System,
};
use tokio::sync::mpsc::Sender;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, warn};

/// How often the disk list and storage (total/free space) are refreshed. I/O
/// counters are refreshed every tick. Storage rarely changes second to second,
/// and on macOS each full refresh re-enumerates volumes and asks CacheDelete
/// for purgeable space, which was the single most expensive collector step.
const DISK_STORAGE_REFRESH: std::time::Duration = std::time::Duration::from_secs(10);

/// How often macOS temperature sensors are read. Each read is ~100 ms of
/// blocking IOHID IPC on Apple Silicon, and package temperature doesn't need
/// per-tick resolution. (btop reads every refresh, asynchronously.)
#[cfg(target_os = "macos")]
const MACOS_TEMP_REFRESH: std::time::Duration = std::time::Duration::from_secs(10);

pub fn spawn_collector(
    tx: Sender<Action>,
    token: CancellationToken,
    refresh_ms: u64,
    thread_refresh_ms: u64,
) {
    std::thread::Builder::new()
        .name("stats-collector".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .build()
                .expect("current-thread runtime with only the time driver should always build");
            rt.block_on(run_collector(tx, token, refresh_ms, thread_refresh_ms));
        })
        .expect("spawning the stats collector thread should not fail");
}

async fn run_collector(
    tx: Sender<Action>,
    token: CancellationToken,
    refresh_ms: u64,
    thread_refresh_ms: u64,
) {
    let mut init_lap = Lap::start();
    let (mut sys, mut nets, mut disks) = match init_sources() {
        Ok(sources) => sources,
        Err(e) => {
            error!("stats collector: failed to initialize sysinfo: {e}");
            return;
        }
    };
    let t_init_sources = init_lap.split();
    // Temperature sensors. On Linux they are refreshed every tick; on macOS
    // they are read every MACOS_TEMP_REFRESH, after a batch is published so
    // the slow read never delays data. On macOS they're optional: failure just
    // means no temperature.
    #[cfg(target_os = "linux")]
    let mut components = match Components::new_with_refreshed_list() {
        Ok(components) => components,
        Err(e) => {
            error!("stats collector: failed to initialize sensors: {e}");
            return;
        }
    };
    #[cfg(target_os = "macos")]
    let mut components = Components::new()
        .inspect_err(|e| warn!("stats collector: temperature sensors unavailable: {e}"))
        .ok();
    #[cfg(target_os = "macos")]
    let mut package_temp: Option<f32> = None;
    #[cfg(target_os = "macos")]
    let mut last_temp_refresh: Option<std::time::Instant> = None;
    // GPUs are optional: a failure here just means no GPU rows in the CPU panel.
    let mut gpus = Gpus::new_with_refreshed_list()
        .inspect_err(|e| warn!("stats collector: GPU enumeration unavailable: {e}"))
        .ok();
    let t_init_gpus = init_lap.split();
    debug!(
        target: "dreidel::perf",
        total_us = init_lap.total_us(),
        sources_us = t_init_sources,
        gpus_us = t_init_gpus,
        "collector init"
    );

    let mut last_disk_storage_refresh = std::time::Instant::now();
    // sysinfo's network and disk I/O counters are deltas since the previous
    // refresh; the UI shows them per second, so they are scaled by the
    // measured time between refreshes.
    let mut last_refresh = std::time::Instant::now();

    let refresh_period = std::time::Duration::from_millis(refresh_ms);
    let mut fast_interval = tokio::time::interval(refresh_period);
    // The first fast tick runs right after System::new_all(), so its CPU
    // deltas cover only a few ms. Take the second sample as soon as sysinfo
    // allows, so real CPU numbers appear well before a full refresh interval.
    let mut early_second_tick = true;
    let mut slow_interval =
        tokio::time::interval(std::time::Duration::from_millis(thread_refresh_ms));

    // Cached thread entries from the most recent slow-tick enumeration.
    // Merged into ProcUpdate on every fast tick so the UI always has thread
    // data, but the expensive /proc/<pid>/task/ walk only happens at the
    // slower cadence.
    #[allow(unused_mut)] // mut only needed on linux and macos
    let mut cached_threads: Vec<ProcessEntry> = Vec::new();

    // Previous aggregate CPU jiffies, used to compute mode-percentage deltas
    // between ticks. Linux-only; on other platforms the state stays `None` and
    // `CpuSnapshot::cpu_modes` is never populated.
    #[cfg(target_os = "linux")]
    let mut prev_cpu_total: Option<CpuTotals> = None;

    #[cfg(target_os = "macos")]
    let mut prev_cpu_ticks: Option<macos::MacosCpuTicks> = None;

    loop {
        // Wait for whichever interval fires first.  A slow tick only
        // re-enumerates threads against the process list from the last fast
        // refresh; the result is published with the next fast tick. Running
        // the full refresh on slow ticks too would add a whole extra
        // collection cycle every `thread_refresh_ms`.
        tokio::select! {
            _ = token.cancelled() => break,
            _ = slow_interval.tick() => {
                let start = std::time::Instant::now();
                #[cfg(target_os = "linux")]
                {
                    cached_threads = enumerate_threads(&sys);
                }
                #[cfg(target_os = "macos")]
                {
                    cached_threads = macos::enumerate_threads(&sys);
                }
                debug!(
                    target: "dreidel::perf",
                    us = start.elapsed().as_micros() as u64,
                    threads = cached_threads.len(),
                    "thread enumeration"
                );
                continue;
            }
            _ = fast_interval.tick() => {}
        }

        // Per-step timings, logged at debug level under the `dreidel::perf`
        // target (`-vv`) to attribute collector CPU cost without a profiler.
        let mut lap = Lap::start();
        let since_last_refresh = lap.start.duration_since(last_refresh);
        last_refresh = lap.start;

        // Equivalent to `refresh_all()` minus per-process GPU stats, which the
        // `gpu` feature would otherwise collect (a `/proc/<pid>/fd` walk per
        // process on Linux) and which nothing displays.
        sys.refresh_specifics(
            RefreshKind::everything().with_processes(
                ProcessRefreshKind::everything()
                    .without_gpu_usage()
                    .without_gpu_memory(),
            ),
        );
        let t_sys = lap.split();
        nets.refresh(false);
        let t_nets = lap.split();
        if last_disk_storage_refresh.elapsed() >= DISK_STORAGE_REFRESH {
            disks.refresh(false);
            last_disk_storage_refresh = std::time::Instant::now();
        } else {
            // Per-disk refresh skips re-enumerating the volume list.
            let io_only = DiskRefreshKind::nothing().with_io_usage();
            for disk in disks.iter_mut() {
                disk.refresh_specifics(io_only);
            }
        }
        let t_disks = lap.split();
        #[cfg(target_os = "linux")]
        components.refresh(false);
        let t_components = lap.split();
        if let Some(gpus) = gpus.as_mut() {
            gpus.refresh(false);
        }
        let t_gpus = lap.split();

        let mut proc_snap = build_proc(&sys);
        let t_build_proc = lap.split();
        proc_snap.processes.extend(cached_threads.clone());
        let t_merge_threads = lap.split();

        #[cfg(target_os = "linux")]
        let mut cpu_snap = build_cpu(&sys, &components, &mut prev_cpu_total);
        #[cfg(target_os = "macos")]
        let mut cpu_snap = macos::build_cpu_macos(&sys, &mut prev_cpu_ticks);
        #[cfg(target_os = "macos")]
        {
            cpu_snap.package_temp = package_temp;
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        let mut cpu_snap = build_cpu_stub(&sys);
        cpu_snap.gpus = gpus.as_ref().map(build_gpus).unwrap_or_default();
        let t_build_cpu = lap.split();

        // ProcUpdate must stay last: App renders once when it arrives, so it
        // marks the end of a complete batch.
        let actions = [
            Action::SysUpdate(build_sys(&sys)),
            Action::CpuUpdate(cpu_snap),
            Action::MemUpdate(build_mem(&sys)),
            Action::NetUpdate(build_net(&nets, since_last_refresh)),
            Action::DiskUpdate(build_disk(&disks, since_last_refresh)),
            Action::ProcUpdate(proc_snap.into()),
        ];
        let t_build_rest = lap.split();

        debug!(
            target: "dreidel::perf",
            total_us = lap.total_us(),
            sys_refresh_us = t_sys,
            nets_us = t_nets,
            disks_us = t_disks,
            components_us = t_components,
            gpus_us = t_gpus,
            build_proc_us = t_build_proc,
            merge_threads_us = t_merge_threads,
            build_cpu_us = t_build_cpu,
            build_rest_us = t_build_rest,
            "collector tick"
        );

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

        #[cfg(target_os = "macos")]
        if let Some(components) = components.as_mut()
            && last_temp_refresh.is_none_or(|t| t.elapsed() >= MACOS_TEMP_REFRESH)
        {
            let start = std::time::Instant::now();
            components.refresh(true);
            package_temp =
                macos::package_temp(components.iter().map(|c| (c.label(), c.temperature())));
            last_temp_refresh = Some(start);
            debug!(
                target: "dreidel::perf",
                us = start.elapsed().as_micros() as u64,
                sensors = components.len(),
                ?package_temp,
                "temperature read"
            );
        }

        if std::mem::take(&mut early_second_tick) {
            let early =
                sysinfo::MINIMUM_CPU_UPDATE_INTERVAL + std::time::Duration::from_millis(100);
            fast_interval.reset_after(early.min(refresh_period));
        }
    }
}

/// Stopwatch that reports microseconds elapsed since the previous split.
struct Lap {
    start: std::time::Instant,
    last: std::time::Instant,
}

impl Lap {
    fn start() -> Self {
        let now = std::time::Instant::now();
        Self {
            start: now,
            last: now,
        }
    }

    fn split(&mut self) -> u64 {
        let now = std::time::Instant::now();
        let us = now.duration_since(self.last).as_micros() as u64;
        self.last = now;
        us
    }

    fn total_us(&self) -> u64 {
        self.last.duration_since(self.start).as_micros() as u64
    }
}

/// Convert a counter delta accumulated over `elapsed` into a per-second rate.
fn per_second(delta: u64, elapsed: std::time::Duration) -> u64 {
    let secs = elapsed.as_secs_f64();
    if secs > 0.0 {
        (delta as f64 / secs).round() as u64
    } else {
        delta
    }
}

fn init_sources() -> Result<(System, Networks, Disks), sysinfo::Error> {
    let mut lap = Lap::start();
    let sys = System::new_all()?;
    let t_sys = lap.split();
    let nets = Networks::new_with_refreshed_list()?;
    let t_nets = lap.split();
    let disks = Disks::new_with_refreshed_list()?;
    let t_disks = lap.split();
    debug!(
        target: "dreidel::perf",
        sys_us = t_sys,
        nets_us = t_nets,
        disks_us = t_disks,
        "init sources"
    );
    Ok((sys, nets, disks))
}

fn build_gpus(gpus: &Gpus) -> Vec<GpuInfo> {
    gpus.iter()
        .map(|g| GpuInfo {
            name: g.model().unwrap_or("GPU").to_owned(),
            usage: g.usage(),
            mem_used: g.used_memory(),
            mem_total: g.total_memory(),
        })
        .collect()
}

fn build_sys(_sys: &System) -> SysSnapshot {
    SysSnapshot {
        hostname: System::host_name().unwrap_or_default(),
        uptime: System::uptime().unwrap_or_default(),
        load_avg: System::load_average()
            .map(|la| [la.one, la.five, la.fifteen])
            .unwrap_or_default(),
        timestamp: jiff::ZonedDateTime::now(),
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
        per_core: cpus.iter().map(|c| c.usage()).collect(),
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
        gpus: Vec::new(),
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn build_cpu_stub(sys: &System) -> CpuSnapshot {
    let cpus = sys.cpus();
    CpuSnapshot {
        per_core: cpus.iter().map(|c| c.usage()).collect(),
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
        gpus: Vec::new(),
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
    #[cfg(target_os = "macos")]
    let (
        macos_ram_free,
        macos_ram_active,
        macos_ram_inactive,
        macos_ram_wired,
        macos_ram_compressed,
        macos_ram_available,
        macos_swap_in,
        macos_swap_out,
    ) = macos::read_mem_details();
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let (ram_free, ram_buffers, ram_cached, ram_available) = (0u64, 0u64, 0u64, 0u64);
    #[cfg(target_os = "macos")]
    let (ram_free, ram_buffers, ram_cached, ram_available) =
        (macos_ram_free, 0u64, 0u64, macos_ram_available);

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
        #[cfg(target_os = "macos")]
        swap_in_bytes: macos_swap_in,
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        swap_in_bytes: 0,
        #[cfg(target_os = "linux")]
        swap_out_bytes: read_vmstat_field("pswpout").unwrap_or(0) * 4096,
        #[cfg(target_os = "macos")]
        swap_out_bytes: macos_swap_out,
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        swap_out_bytes: 0,
        #[cfg(target_os = "macos")]
        ram_active: macos_ram_active,
        #[cfg(not(target_os = "macos"))]
        ram_active: 0,
        #[cfg(target_os = "macos")]
        ram_inactive: macos_ram_inactive,
        #[cfg(not(target_os = "macos"))]
        ram_inactive: 0,
        #[cfg(target_os = "macos")]
        ram_wired: macos_ram_wired,
        #[cfg(not(target_os = "macos"))]
        ram_wired: 0,
        #[cfg(target_os = "macos")]
        ram_compressed: macos_ram_compressed,
        #[cfg(not(target_os = "macos"))]
        ram_compressed: 0,
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

fn build_net(nets: &Networks, elapsed: std::time::Duration) -> NetSnapshot {
    #[cfg(target_os = "linux")]
    let dev_stats = procfs::net::dev_status().unwrap_or_default();
    #[cfg(target_os = "macos")]
    let mac_drops = macos::read_net_drops();

    NetSnapshot {
        interfaces: nets
            .iter()
            .map(|(name, data)| {
                let name = name.to_string_lossy().into_owned();
                let mut ipv4_addresses: Vec<String> = Vec::new();
                let mut ipv6_addresses: Vec<String> = Vec::new();
                for n in data.ip_networks() {
                    let s = format!("{}/{}", n.addr, n.prefix);
                    if n.addr.is_ipv4() {
                        ipv4_addresses.push(s);
                    } else {
                        ipv6_addresses.push(s);
                    }
                }
                // Sort each family for stable, predictable display order.
                ipv4_addresses.sort_unstable();
                ipv6_addresses.sort_unstable();

                #[cfg(target_os = "macos")]
                let (mac_rx_drop, mac_tx_drop) = mac_drops.get(&name).copied().unwrap_or((0, 0));

                InterfaceSnapshot {
                    name: name.clone(),
                    rx_bytes: per_second(data.received(), elapsed),
                    tx_bytes: per_second(data.transmitted(), elapsed),
                    rx_packets: per_second(data.packets_received(), elapsed),
                    tx_packets: per_second(data.packets_transmitted(), elapsed),
                    rx_errors: data.errors_on_received(),
                    tx_errors: data.errors_on_transmitted(),
                    total_rx_bytes: data.total_received(),
                    total_tx_bytes: data.total_transmitted(),
                    mac_address: data.mac_address().to_string(),
                    ipv4_addresses,
                    ipv6_addresses,
                    mtu: data.mtu(),
                    #[cfg(target_os = "linux")]
                    rx_dropped: dev_stats.get(&name).map(|s| s.recv_drop).unwrap_or(0),
                    #[cfg(target_os = "macos")]
                    rx_dropped: mac_rx_drop,
                    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
                    rx_dropped: 0,
                    #[cfg(target_os = "linux")]
                    tx_dropped: dev_stats.get(&name).map(|s| s.sent_drop).unwrap_or(0),
                    #[cfg(target_os = "macos")]
                    tx_dropped: mac_tx_drop,
                    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
                    tx_dropped: 0,
                }
            })
            .collect(),
    }
}

fn build_disk(disks: &Disks, elapsed: std::time::Duration) -> DiskSnapshot {
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
                    read_bytes: per_second(usage.read_bytes, elapsed),
                    write_bytes: per_second(usage.written_bytes, elapsed),
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
                #[allow(unused_mut)] // fields set via procfs only on linux
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

                #[cfg(target_os = "macos")]
                macos::enrich_process_entry(&mut entry, pid);

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
                        // sysinfo exposes thread TIDs as top-level /proc entries.
                        // Tgid != pid means this entry is a thread, not a process.
                        if status.tgid != pid as i32 {
                            entry.is_thread = true;
                            entry.parent_pid = Some(status.tgid as u32);
                        }
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
            // Exclude thread TIDs that sysinfo exposes as top-level /proc entries.
            // Their is_thread flag was set above via the Tgid check.  Thread data is
            // provided by enumerate_threads instead, avoiding duplicate entries.
            .filter(|e| !e.is_thread)
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

        let Ok(proc) = procfs::process::Process::new(pid as i32) else {
            continue;
        };

        // sysinfo exposes thread TIDs as top-level /proc entries alongside real
        // processes.  Reading /proc/[TID]/task/ would return all siblings of that
        // thread group, creating spurious parent→child entries that form cycles in
        // the tree.  Only enumerate tasks for real process group leaders (TID == TGID).
        if let Ok(status) = proc.status()
            && status.tgid != pid as i32
        {
            continue;
        }

        if let Ok(tasks) = proc.tasks() {
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
        // As of sysinfo 0.40: Tracing, Wakekill, Waking, Parked, LockBlocked,
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
        spawn_collector(tx, child, 100, 500);

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

    #[test]
    fn per_second_scales_delta_by_elapsed_time() {
        use std::time::Duration;
        assert_eq!(per_second(2000, Duration::from_secs(2)), 1000);
        assert_eq!(per_second(1000, Duration::from_secs(1)), 1000);
        assert_eq!(per_second(300, Duration::from_millis(500)), 600);
        // Zero elapsed (should not happen) leaves the delta untouched rather
        // than dividing by zero.
        assert_eq!(per_second(42, Duration::ZERO), 42);
    }

    /// The first tick follows initialization by a few ms, so its CPU deltas
    /// are meaningless; a second sample must follow shortly rather than a full
    /// refresh interval later.
    #[tokio::test]
    async fn second_tick_follows_soon_after_startup() {
        let (tx, mut rx) = mpsc::channel(1024);
        let token = tokio_util::sync::CancellationToken::new();
        spawn_collector(tx, token.child_token(), 10_000, 60_000);

        let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(1500);
        let mut sys_updates = 0;
        while let Ok(Some(action)) = tokio::time::timeout_at(deadline, rx.recv()).await {
            if matches!(action, crate::action::Action::SysUpdate(_)) {
                sys_updates += 1;
            }
        }
        token.cancel();
        assert_eq!(
            sys_updates, 2,
            "expected an early second tick, then the 10 s interval"
        );
    }

    /// Slow (thread-enumeration) ticks must not trigger an extra full refresh
    /// and publish: only the fast interval drives updates.
    #[tokio::test]
    async fn slow_tick_does_not_publish_updates() {
        let (tx, mut rx) = mpsc::channel(1024);
        let token = tokio_util::sync::CancellationToken::new();
        spawn_collector(tx, token.child_token(), 1000, 100);

        let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(1500);
        let mut sys_updates = 0;
        while let Ok(Some(action)) = tokio::time::timeout_at(deadline, rx.recv()).await {
            if matches!(action, crate::action::Action::SysUpdate(_)) {
                sys_updates += 1;
            }
        }
        token.cancel();
        // Fast ticks at ~0 ms, the early second tick (~300 ms), and ~1300 ms.
        assert!(
            (1..=3).contains(&sys_updates),
            "expected 1-3 SysUpdates from fast ticks in 1.5 s, got {sys_updates}"
        );
    }
}
