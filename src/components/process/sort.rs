// SPDX-License-Identifier: GPL-3.0-only

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
    // Normal-view columns (also appear in extended view)
    Pid,
    Name,
    #[default]
    Cpu,
    Mem,
    Status,
    // Extended-view-only columns, in their left-to-right display order
    User,
    Priority,
    Nice,
    Virt,
    Res,
    Shr,
    Time,
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
            SortColumn::Status => a.status.to_string().cmp(&b.status.to_string()),
            SortColumn::User => a.user.cmp(&b.user),
            SortColumn::Priority => a.priority.cmp(&b.priority),
            SortColumn::Nice => a.nice.cmp(&b.nice),
            SortColumn::Virt => a.virt_bytes.cmp(&b.virt_bytes),
            SortColumn::Res => a.mem_bytes.cmp(&b.mem_bytes),
            SortColumn::Shr => a.shr_bytes.cmp(&b.shr_bytes),
            SortColumn::Time => a
                .cpu_time_secs
                .partial_cmp(&b.cpu_time_secs)
                .unwrap_or(std::cmp::Ordering::Equal),
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
    fn sort_column_enum_contains_all_expected_variants() {
        use strum::IntoEnumIterator;
        let all: Vec<SortColumn> = SortColumn::iter().collect();
        // Normal-view columns
        assert!(all.contains(&SortColumn::Pid));
        assert!(all.contains(&SortColumn::Name));
        assert!(all.contains(&SortColumn::Cpu));
        assert!(all.contains(&SortColumn::Mem));
        assert!(all.contains(&SortColumn::Status));
        // Extended-view-only columns
        assert!(all.contains(&SortColumn::User));
        assert!(all.contains(&SortColumn::Priority));
        assert!(all.contains(&SortColumn::Nice));
        assert!(all.contains(&SortColumn::Virt));
        assert!(all.contains(&SortColumn::Res));
        assert!(all.contains(&SortColumn::Shr));
        assert!(all.contains(&SortColumn::Time));
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
