// SPDX-License-Identifier: GPL-3.0-only

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessStatus {
    Running,
    Sleeping,
    Idle,
    Stopped,
    Zombie,
    Dead,
    Unknown,
}

impl std::fmt::Display for ProcessStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Running => "running",
            Self::Sleeping => "sleeping",
            Self::Idle => "idle",
            Self::Stopped => "stopped",
            Self::Zombie => "zombie",
            Self::Dead => "dead",
            Self::Unknown => "unknown",
        };
        f.write_str(s)
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct CpuSnapshot {
    pub aggregate: f32,
    pub per_core: Vec<f32>,
    pub frequency: Vec<u64>,
    /// Number of physical (non-hyperthreaded) cores. Linux-only; `None` on other platforms.
    pub physical_core_count: Option<u32>,
    pub cpu_brand: String,
    /// Package-level CPU temperature in °C. Linux-only; `None` on other platforms.
    pub package_temp: Option<f32>,
    /// Per-logical-core temperature in °C. Linux-only; empty or all-`None` on other platforms.
    pub per_core_temp: Vec<Option<f32>>,
    /// CPU scaling governor, e.g. "powersave". Linux-only; `None` on other platforms.
    pub governor: Option<String>,
}

#[cfg(any(test, feature = "test-stubs"))]
#[allow(dead_code)]
impl CpuSnapshot {
    pub fn stub() -> Self {
        Self {
            aggregate: 35.0,
            per_core: vec![42.0, 18.0, 75.0, 5.0],
            frequency: vec![3400, 3400, 3400, 3400],
            physical_core_count: Some(4),
            cpu_brand: "Intel(R) Core(TM) i7-9750H CPU @ 2.60GHz".into(),
            package_temp: Some(62.0),
            per_core_temp: vec![Some(55.0), Some(58.0), Some(60.0), Some(52.0)],
            governor: Some("powersave".into()),
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct MemSnapshot {
    pub ram_total: u64,
    pub ram_used: u64,
    pub swap_total: u64,
    pub swap_used: u64,
    /// Cumulative swap-in bytes from /proc/vmstat. Linux-only; `0` on other platforms.
    pub swap_in_bytes: u64,
    /// Cumulative swap-out bytes from /proc/vmstat. Linux-only; `0` on other platforms.
    pub swap_out_bytes: u64,
}

#[cfg(any(test, feature = "test-stubs"))]
#[allow(dead_code)]
impl MemSnapshot {
    pub fn stub() -> Self {
        Self {
            ram_total: 17_179_869_184,
            ram_used: 6_442_450_944,
            swap_total: 4_294_967_296,
            swap_used: 0,
            swap_in_bytes: 0,
            swap_out_bytes: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct NetSnapshot {
    pub interfaces: Vec<InterfaceSnapshot>,
}

#[derive(Debug, Clone)]
pub struct InterfaceSnapshot {
    pub name: String,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_packets: u64,
    pub tx_packets: u64,
    pub rx_errors: u64,
    pub tx_errors: u64,
    pub total_rx_bytes: u64,
    pub total_tx_bytes: u64,
    pub mac_address: String,
    pub ip_addresses: Vec<String>,
    pub mtu: u64,
    /// Cumulative receive drops from /proc/net/dev. Linux-only; `0` on other platforms.
    pub rx_dropped: u64,
    /// Cumulative transmit drops from /proc/net/dev. Linux-only; `0` on other platforms.
    pub tx_dropped: u64,
}

#[cfg(any(test, feature = "test-stubs"))]
#[allow(dead_code)]
impl NetSnapshot {
    pub fn stub() -> Self {
        Self {
            interfaces: vec![InterfaceSnapshot {
                name: "eth0".into(),
                rx_bytes: 4_800_000,
                tx_bytes: 1_200_000,
                rx_packets: 3_200,
                tx_packets: 850,
                rx_errors: 0,
                tx_errors: 0,
                total_rx_bytes: 48_318_382_080,
                total_tx_bytes: 12_884_901_888,
                mac_address: "aa:bb:cc:dd:ee:ff".into(),
                ip_addresses: vec!["192.168.1.100/24".into(), "fe80::1/64".into()],
                mtu: 1500,
                rx_dropped: 0,
                tx_dropped: 0,
            }],
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct DiskDeviceSnapshot {
    pub name: String,
    pub read_bytes: u64,
    pub write_bytes: u64,
    pub usage_pct: f32,
    pub total_read_bytes: u64,
    pub total_write_bytes: u64,
    pub mount_point: String,
    pub file_system: String,
    pub kind: String,
    pub is_removable: bool,
    pub is_read_only: bool,
    pub total_space: u64,
    pub available_space: u64,
}

#[derive(Debug, Clone)]
pub struct DiskSnapshot {
    pub devices: Vec<DiskDeviceSnapshot>,
}

#[cfg(any(test, feature = "test-stubs"))]
#[allow(dead_code)]
impl DiskSnapshot {
    pub fn stub() -> Self {
        Self {
            devices: vec![DiskDeviceSnapshot {
                name: "sda".into(),
                read_bytes: 0,
                write_bytes: 102_400,
                usage_pct: 45.0,
                total_read_bytes: 1_073_741_824,
                total_write_bytes: 536_870_912,
                mount_point: "/".into(),
                file_system: "ext4".into(),
                kind: "SSD".into(),
                is_removable: false,
                is_read_only: false,
                total_space: 500_107_862_016,
                available_space: 275_059_200_000,
            }],
        }
    }
}

#[derive(Debug, Clone)]
pub struct SysSnapshot {
    pub hostname: String,
    pub uptime: u64,
    pub load_avg: [f64; 3],
    pub timestamp: chrono::DateTime<chrono::Local>,
}

#[cfg(any(test, feature = "test-stubs"))]
#[allow(dead_code)]
impl SysSnapshot {
    pub fn stub() -> Self {
        Self {
            hostname: "dev-box".into(),
            uptime: 273_600,
            load_avg: [1.24, 0.98, 0.87],
            timestamp: chrono::Local::now(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProcessEntry {
    pub pid: u32,
    pub name: String,
    pub cmd: Vec<String>,
    pub user: String,
    pub cpu_pct: f32,
    pub mem_bytes: u64,
    pub mem_pct: f32,
    pub virt_bytes: u64,
    pub status: ProcessStatus,
    pub start_time: u64,
    pub run_time: u64,
    pub nice: i32,
    pub threads: u32,
    pub read_bytes: u64,
    pub write_bytes: u64,
    pub parent_pid: Option<u32>,
    /// Kernel raw priority from /proc/<pid>/stat field 18.
    /// For normal processes: 0-39 (20 = nice 0). Negative = RT process.
    pub priority: i32,
    /// Shared memory in bytes (SHR) from /proc/<pid>/statm field 3 x page_size.
    pub shr_bytes: u64,
    /// Total CPU time in seconds: (utime + stime) / ticks_per_second.
    pub cpu_time_secs: f64,
    pub exe: Option<String>,
    pub cwd: Option<String>,
    pub root: Option<String>,
    pub effective_user: Option<String>,
    pub group: Option<String>,
    pub effective_group: Option<String>,
    pub session_id: Option<u32>,
    pub tty: Option<String>,
    pub user_cpu_time_secs: f64,
    pub system_cpu_time_secs: f64,
    pub minor_faults: u64,
    pub major_faults: u64,
    pub voluntary_ctxt_switches: Option<u64>,
    pub nonvoluntary_ctxt_switches: Option<u64>,
    pub fd_count: Option<usize>,
    pub swap_bytes: Option<u64>,
    pub io_read_calls: Option<u64>,
    pub io_write_calls: Option<u64>,
    pub io_read_chars: Option<u64>,
    pub io_write_chars: Option<u64>,
    pub cancelled_write_bytes: Option<u64>,
    /// True when this entry represents a kernel thread (task) rather than a
    /// standalone process. Threads share their parent's address space so
    /// memory columns are not meaningful.
    pub is_thread: bool,
}

#[derive(Debug, Clone)]
pub struct ProcSnapshot {
    pub processes: Vec<ProcessEntry>,
}

#[cfg(any(test, feature = "test-stubs"))]
#[allow(dead_code)]
impl ProcSnapshot {
    pub fn stub() -> Self {
        Self {
            processes: vec![
                ProcessEntry {
                    pid: 1,
                    name: "systemd".into(),
                    cmd: vec!["/sbin/init".into()],
                    user: "root".into(),
                    cpu_pct: 0.1,
                    mem_bytes: 12_582_912,
                    mem_pct: 0.1,
                    virt_bytes: 176_160_768,
                    status: ProcessStatus::Sleeping,
                    start_time: 0,
                    run_time: 86_400,
                    nice: 0,
                    threads: 1,
                    read_bytes: 0,
                    write_bytes: 0,
                    parent_pid: None,
                    priority: 20,
                    shr_bytes: 8_388_608,
                    cpu_time_secs: 323.0,
                    exe: Some("/usr/lib/systemd/systemd".into()),
                    cwd: Some("/".into()),
                    root: Some("/".into()),
                    effective_user: Some("0".into()),
                    group: Some("0".into()),
                    effective_group: Some("0".into()),
                    session_id: Some(1),
                    tty: None,
                    user_cpu_time_secs: 200.0,
                    system_cpu_time_secs: 123.0,
                    minor_faults: 100,
                    major_faults: 1,
                    voluntary_ctxt_switches: Some(12),
                    nonvoluntary_ctxt_switches: Some(3),
                    fd_count: Some(64),
                    swap_bytes: Some(0),
                    io_read_calls: Some(0),
                    io_write_calls: Some(0),
                    io_read_chars: Some(0),
                    io_write_chars: Some(0),
                    cancelled_write_bytes: Some(0),
                    is_thread: false,
                },
                ProcessEntry {
                    pid: 500,
                    name: "sshd".into(),
                    cmd: vec!["/usr/sbin/sshd".into()],
                    user: "root".into(),
                    cpu_pct: 0.0,
                    mem_bytes: 5_242_880,
                    mem_pct: 0.0,
                    virt_bytes: 15_728_640,
                    status: ProcessStatus::Sleeping,
                    start_time: 0,
                    run_time: 86_400,
                    nice: 0,
                    threads: 1,
                    read_bytes: 0,
                    write_bytes: 0,
                    parent_pid: Some(1),
                    priority: 20,
                    shr_bytes: 3_145_728,
                    cpu_time_secs: 1.0,
                    exe: Some("/usr/sbin/sshd".into()),
                    cwd: Some("/".into()),
                    root: Some("/".into()),
                    effective_user: Some("0".into()),
                    group: Some("0".into()),
                    effective_group: Some("0".into()),
                    session_id: Some(500),
                    tty: None,
                    user_cpu_time_secs: 0.7,
                    system_cpu_time_secs: 0.3,
                    minor_faults: 20,
                    major_faults: 0,
                    voluntary_ctxt_switches: Some(2),
                    nonvoluntary_ctxt_switches: Some(1),
                    fd_count: Some(12),
                    swap_bytes: Some(0),
                    io_read_calls: Some(0),
                    io_write_calls: Some(0),
                    io_read_chars: Some(0),
                    io_write_chars: Some(0),
                    cancelled_write_bytes: Some(0),
                    is_thread: false,
                },
                ProcessEntry {
                    pid: 501,
                    name: "bash".into(),
                    cmd: vec!["/bin/bash".into()],
                    user: "ray".into(),
                    cpu_pct: 0.1,
                    mem_bytes: 4_194_304,
                    mem_pct: 0.0,
                    virt_bytes: 12_582_912,
                    status: ProcessStatus::Sleeping,
                    start_time: 0,
                    run_time: 3_600,
                    nice: 0,
                    threads: 1,
                    read_bytes: 0,
                    write_bytes: 0,
                    parent_pid: Some(500),
                    priority: 20,
                    shr_bytes: 2_097_152,
                    cpu_time_secs: 0.5,
                    exe: Some("/bin/bash".into()),
                    cwd: Some("/home/ray/src/dreidel".into()),
                    root: Some("/".into()),
                    effective_user: Some("1000".into()),
                    group: Some("1000".into()),
                    effective_group: Some("1000".into()),
                    session_id: Some(500),
                    tty: Some("136:1".into()),
                    user_cpu_time_secs: 0.4,
                    system_cpu_time_secs: 0.1,
                    minor_faults: 8,
                    major_faults: 0,
                    voluntary_ctxt_switches: Some(15),
                    nonvoluntary_ctxt_switches: Some(1),
                    fd_count: Some(9),
                    swap_bytes: Some(0),
                    io_read_calls: Some(12),
                    io_write_calls: Some(4),
                    io_read_chars: Some(1024),
                    io_write_chars: Some(512),
                    cancelled_write_bytes: Some(0),
                    is_thread: false,
                },
                ProcessEntry {
                    pid: 12345,
                    name: "firefox".into(),
                    cmd: vec!["firefox".into()],
                    user: "ray".into(),
                    cpu_pct: 18.4,
                    mem_bytes: 536_870_912,
                    mem_pct: 3.2,
                    virt_bytes: 2_147_483_648,
                    status: ProcessStatus::Running,
                    start_time: 0,
                    run_time: 3_600,
                    nice: -5,
                    threads: 42,
                    read_bytes: 0,
                    write_bytes: 0,
                    parent_pid: Some(1),
                    priority: 15,
                    shr_bytes: 134_217_728,
                    cpu_time_secs: 123.4,
                    exe: Some("/usr/bin/firefox".into()),
                    cwd: Some("/home/ray".into()),
                    root: Some("/".into()),
                    effective_user: Some("1000".into()),
                    group: Some("1000".into()),
                    effective_group: Some("1000".into()),
                    session_id: Some(500),
                    tty: Some("136:1".into()),
                    user_cpu_time_secs: 100.0,
                    system_cpu_time_secs: 23.4,
                    minor_faults: 20_000,
                    major_faults: 10,
                    voluntary_ctxt_switches: Some(5_000),
                    nonvoluntary_ctxt_switches: Some(250),
                    fd_count: Some(300),
                    swap_bytes: Some(16_777_216),
                    io_read_calls: Some(5_000),
                    io_write_calls: Some(2_500),
                    io_read_chars: Some(134_217_728),
                    io_write_chars: Some(67_108_864),
                    cancelled_write_bytes: Some(4096),
                    is_thread: false,
                },
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_snapshot_stub_has_expected_shape() {
        let s = CpuSnapshot::stub();
        assert!(!s.per_core.is_empty());
        assert!(s.aggregate >= 0.0 && s.aggregate <= 100.0);
    }

    #[test]
    fn mem_snapshot_used_never_exceeds_total() {
        let s = MemSnapshot::stub();
        assert!(s.ram_used <= s.ram_total);
        assert!(s.swap_used <= s.swap_total);
    }
}
