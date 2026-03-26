#![allow(dead_code)]

#[derive(Debug, Clone)]
pub struct CpuSnapshot {
    pub per_core: Vec<f32>, // 0.0–100.0 per logical core
    pub aggregate: f32,
    pub frequency: Vec<u64>, // MHz per core
    #[cfg(target_os = "linux")]
    pub temperature: Option<f32>, // degrees C
}

impl CpuSnapshot {
    pub fn stub() -> Self {
        Self {
            per_core: vec![42.0, 18.0, 75.0, 5.0],
            aggregate: 35.0,
            frequency: vec![3400, 3400, 3400, 3400],
            #[cfg(target_os = "linux")]
            temperature: Some(62.0),
        }
    }
}

#[derive(Debug, Clone)]
pub struct MemSnapshot {
    pub ram_used: u64,
    pub ram_total: u64,
    pub swap_used: u64,
    pub swap_total: u64,
    #[cfg(target_os = "linux")]
    pub swap_in_bytes: u64,
    #[cfg(target_os = "linux")]
    pub swap_out_bytes: u64,
}

impl MemSnapshot {
    pub fn stub() -> Self {
        Self {
            ram_used: 6_442_450_944,
            ram_total: 17_179_869_184,
            swap_used: 0,
            swap_total: 4_294_967_296,
            #[cfg(target_os = "linux")]
            swap_in_bytes: 0,
            #[cfg(target_os = "linux")]
            swap_out_bytes: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct InterfaceSnapshot {
    pub name: String,
    pub rx_bytes: u64, // bytes/s since last tick
    pub tx_bytes: u64,
}

#[derive(Debug, Clone)]
pub struct NetSnapshot {
    pub interfaces: Vec<InterfaceSnapshot>,
}

impl NetSnapshot {
    pub fn stub() -> Self {
        Self {
            interfaces: vec![InterfaceSnapshot {
                name: "eth0".into(),
                rx_bytes: 4_800_000,
                tx_bytes: 1_200_000,
            }],
        }
    }
}

#[derive(Debug, Clone)]
pub struct DiskDeviceSnapshot {
    pub name: String,
    pub read_bytes: u64, // bytes/s
    pub write_bytes: u64,
    pub usage_pct: f32, // 0.0–100.0
}

#[derive(Debug, Clone)]
pub struct DiskSnapshot {
    pub devices: Vec<DiskDeviceSnapshot>,
}

impl DiskSnapshot {
    pub fn stub() -> Self {
        Self {
            devices: vec![DiskDeviceSnapshot {
                name: "sda".into(),
                read_bytes: 0,
                write_bytes: 102_400,
                usage_pct: 45.0,
            }],
        }
    }
}

#[derive(Debug, Clone)]
pub struct SysSnapshot {
    pub hostname: String,
    pub uptime: u64,        // seconds
    pub load_avg: [f64; 3], // 1m, 5m, 15m
    pub timestamp: chrono::DateTime<chrono::Local>,
}

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
        match self {
            ProcessStatus::Running => write!(f, "running"),
            ProcessStatus::Sleeping => write!(f, "sleeping"),
            ProcessStatus::Idle => write!(f, "idle"),
            ProcessStatus::Stopped => write!(f, "stopped"),
            ProcessStatus::Zombie => write!(f, "zombie"),
            ProcessStatus::Dead => write!(f, "dead"),
            ProcessStatus::Unknown => write!(f, "unknown"),
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
    pub start_time: u64, // unix timestamp
    pub run_time: u64,   // seconds
    pub nice: i32,
    pub threads: u32,
    pub read_bytes: u64,
    pub write_bytes: u64,
    pub parent_pid: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct ProcSnapshot {
    pub processes: Vec<ProcessEntry>,
}

impl ProcSnapshot {
    pub fn stub() -> Self {
        Self {
            processes: vec![ProcessEntry {
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
                run_time: 3600,
                nice: 0,
                threads: 42,
                read_bytes: 0,
                write_bytes: 0,
                parent_pid: Some(1),
            }],
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
