// SPDX-License-Identifier: GPL-3.0-only

//! Process list filtering by name substring, exact PID, or status.
//!
//! [`ProcessFilter`] is parsed from the user’s filter input string and
//! applied via [`ProcessFilter::matches`] to each [`ProcessEntry`].

use crate::stats::snapshots::ProcessEntry;

#[derive(Debug, Clone, PartialEq)]
pub enum ProcessFilter {
    None,
    Name(String),
    Pid(u32),
    State(String),
}

impl ProcessFilter {
    pub fn matches(&self, p: &ProcessEntry) -> bool {
        match self {
            Self::None => true,
            Self::Name(s) => p.name.to_lowercase().contains(&s.to_lowercase()),
            Self::Pid(pid) => p.pid == *pid,
            // ProcessStatus::Display outputs lowercase ("running", "sleeping", etc.)
            Self::State(s) => p
                .status
                .to_string()
                .to_lowercase()
                .contains(&s.to_lowercase()),
        }
    }

    /// Parse raw filter input: pure number → Pid; `s:<text>` → State; else Name.
    pub fn parse(input: &str) -> Self {
        let s = input.trim();
        if s.is_empty() {
            return Self::None;
        }
        if let Ok(pid) = s.parse::<u32>() {
            return Self::Pid(pid);
        }
        if let Some(rest) = s.strip_prefix("s:") {
            return Self::State(rest.into());
        }
        Self::Name(s.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::snapshots::ProcSnapshot;

    #[test]
    fn filter_by_name_substring() {
        let procs = ProcSnapshot::stub().processes;
        let f = ProcessFilter::Name("fire".into());
        let result: Vec<_> = procs.iter().filter(|p| f.matches(p)).collect();
        assert!(
            result
                .iter()
                .all(|p| p.name.to_lowercase().contains("fire"))
        );
    }

    #[test]
    fn filter_by_pid_exact() {
        let procs = ProcSnapshot::stub().processes;
        let f = ProcessFilter::Pid(12345);
        let result: Vec<_> = procs.iter().filter(|p| f.matches(p)).collect();
        assert!(result.iter().all(|p| p.pid == 12345));
    }

    #[test]
    fn filter_by_state() {
        let procs = ProcSnapshot::stub().processes;
        // ProcSnapshot::stub() has a process with status Running (displays as "running")
        let f = ProcessFilter::State("running".into());
        let result: Vec<_> = procs.iter().filter(|p| f.matches(p)).collect();
        assert!(
            !result.is_empty(),
            "expected at least one running process in stub"
        );
    }
}
