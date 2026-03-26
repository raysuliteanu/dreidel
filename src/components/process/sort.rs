use serde::{Deserialize, Serialize};
use strum::{Display, EnumIter, EnumString};

use crate::stats::snapshots::ProcessEntry;

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Default,
    Display,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[strum(serialize_all = "lowercase")]
pub enum SortColumn {
    #[default]
    Cpu,
    Mem,
    Pid,
    Name,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortDir {
    #[default]
    Desc,
    Asc,
}

pub fn sort_processes(procs: &mut [ProcessEntry], col: SortColumn, dir: SortDir) {
    procs.sort_by(|a, b| {
        let ord = match col {
            SortColumn::Cpu => a
                .cpu_pct
                .partial_cmp(&b.cpu_pct)
                .unwrap_or(std::cmp::Ordering::Equal),
            SortColumn::Mem => a.mem_bytes.cmp(&b.mem_bytes),
            SortColumn::Pid => a.pid.cmp(&b.pid),
            SortColumn::Name => a.name.cmp(&b.name),
        };
        if dir == SortDir::Desc {
            ord.reverse()
        } else {
            ord
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::snapshots::{ProcSnapshot, ProcessEntry};

    fn make_entry(base: &ProcessEntry, pid: u32, name: &str, cpu: f32) -> ProcessEntry {
        ProcessEntry {
            pid,
            name: name.into(),
            cpu_pct: cpu,
            ..base.clone()
        }
    }

    #[test]
    fn sort_by_cpu_desc_puts_highest_first() {
        let base = ProcSnapshot::stub().processes.remove(0);
        let mut procs = vec![
            make_entry(&base, 1, "low", 1.0),
            make_entry(&base, 2, "high", 90.0),
        ];
        sort_processes(&mut procs, SortColumn::Cpu, SortDir::Desc);
        assert!(procs[0].cpu_pct >= procs[1].cpu_pct);
    }

    #[test]
    fn sort_by_name_asc_is_alphabetical() {
        let base = ProcSnapshot::stub().processes.remove(0);
        let mut procs = vec![
            make_entry(&base, 1, "zebra", 0.0),
            make_entry(&base, 2, "aardvark", 0.0),
        ];
        sort_processes(&mut procs, SortColumn::Name, SortDir::Asc);
        assert!(procs[0].name <= procs[1].name);
    }
}
