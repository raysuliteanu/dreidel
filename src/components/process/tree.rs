// SPDX-License-Identifier: GPL-3.0-only

//! Builds a flattened, depth-annotated tree from a list of [`ProcessEntry`]
//! items using their `parent_pid` links.  The output is a `Vec<TreeRow>` ready
//! for rendering in display order (pre-order DFS).

use std::collections::{HashMap, HashSet};

use crate::stats::snapshots::ProcessEntry;

use super::filter::ProcessFilter;
use super::sort::{SortColumn, SortDir, sort_processes};

/// Maximum visual indentation depth.  Nodes deeper than this are rendered with
/// a collapsed "·· " prefix so the Name/Command column stays readable.
pub const MAX_INDENT_DEPTH: u16 = 4;

/// A single row in the flattened tree output.
#[derive(Debug, Clone)]
pub struct TreeRow {
    pub entry: ProcessEntry,
    /// 0 = root-level process.
    pub depth: u16,
    /// True when this node is the last child of its parent at this depth.
    pub is_last_sibling: bool,
    /// True when this node has children that could be displayed.
    pub has_children: bool,
    /// True when this node is expanded (children visible).
    pub is_expanded: bool,
    /// Prefix guide-rail pattern.  Each element corresponds to an ancestor
    /// depth and indicates whether a vertical continuation line (`│`) should
    /// be drawn.  Length == `depth`.
    pub guide_rails: Vec<bool>,
}

impl TreeRow {
    /// Build the text prefix for the Name/Command column.
    ///
    /// Examples (depth 0–3):
    /// ```text
    /// systemd                     (depth 0, root)
    /// ├── journald                (depth 1, not last)
    /// └── sshd                    (depth 1, last)
    /// │   └── bash                (depth 2, last, parent not last)
    ///     └── zsh                 (depth 2, last, parent is last)
    /// ```
    pub fn tree_prefix(&self) -> String {
        if self.depth == 0 {
            return String::new();
        }

        let mut buf = String::new();

        // For depths beyond the clamp, emit a "·· " leader so the user knows
        // nesting continues without eating more horizontal space.
        let visual_depth = self.depth.min(MAX_INDENT_DEPTH);
        let clamped = self.depth > MAX_INDENT_DEPTH;

        // Guide rails for ancestor levels (skip depth-0, start from depth-1).
        // We render rails for depths 1..visual_depth (the current node's own
        // connector replaces the rail at visual_depth).
        let rail_end = (visual_depth as usize).saturating_sub(1);
        for i in 0..rail_end {
            if clamped && i == 0 {
                buf.push_str("·· ");
                continue;
            }
            let rail_idx = if clamped {
                // When clamped, map visual positions back to the original guide
                // rails.  Position 0 already handled (·· ).  Remaining positions
                // map to the tail of the guide_rails vec.
                self.guide_rails.len() - (visual_depth as usize) + i
            } else {
                i
            };
            if rail_idx < self.guide_rails.len() && self.guide_rails[rail_idx] {
                buf.push_str("│   ");
            } else {
                buf.push_str("    ");
            }
        }

        // The connector for this node.
        if self.is_last_sibling {
            buf.push_str("└── ");
        } else {
            buf.push_str("├── ");
        }

        buf
    }
}

/// Build a flattened tree from `processes`.
///
/// 1. Filter processes (keeping ancestors of matches so the tree path is
///    visible).
/// 2. Group by parent, sort each group.
/// 3. DFS to produce `Vec<TreeRow>` in display order, skipping children of
///    collapsed nodes.
pub fn build_tree(
    processes: &[ProcessEntry],
    sort_col: SortColumn,
    sort_dir: SortDir,
    filter: &ProcessFilter,
    expanded: &HashSet<u32>,
) -> Vec<TreeRow> {
    if processes.is_empty() {
        return Vec::new();
    }

    // Index all PIDs for quick parent-exists checks.
    let all_pids: HashSet<u32> = processes.iter().map(|p| p.pid).collect();

    // Determine which PIDs match the filter directly.
    let direct_matches: HashSet<u32> = if matches!(filter, ProcessFilter::None) {
        // No filter — every process matches.
        all_pids.clone()
    } else {
        processes
            .iter()
            .filter(|p| filter.matches(p))
            .map(|p| p.pid)
            .collect()
    };

    // Walk ancestors of every direct match to build the full "visible" set.
    let visible_pids = if matches!(filter, ProcessFilter::None) {
        all_pids.clone()
    } else {
        let parent_map: HashMap<u32, Option<u32>> =
            processes.iter().map(|p| (p.pid, p.parent_pid)).collect();
        let mut visible = direct_matches.clone();
        for &pid in &direct_matches {
            let mut cur = pid;
            while let Some(Some(ppid)) = parent_map.get(&cur) {
                if !visible.insert(*ppid) {
                    break; // already visited
                }
                cur = *ppid;
            }
        }
        visible
    };

    // Build children map: parent_pid → sorted vec of children.
    let mut children_map: HashMap<Option<u32>, Vec<ProcessEntry>> = HashMap::new();
    for p in processes {
        if !visible_pids.contains(&p.pid) {
            continue;
        }
        // A process is a root if its parent_pid is None or its parent is not in
        // the visible set (orphaned child whose parent was filtered out or exited).
        let key = match p.parent_pid {
            Some(ppid) if visible_pids.contains(&ppid) && ppid != p.pid => Some(ppid),
            _ => None,
        };
        children_map.entry(key).or_default().push(p.clone());
    }

    // Sort each group.
    for group in children_map.values_mut() {
        sort_processes(group, sort_col, sort_dir);
    }

    // DFS traversal.
    let mut result = Vec::with_capacity(processes.len());
    dfs_build(&children_map, expanded, &mut result);
    result
}

/// Iterative DFS that produces the flattened tree rows.
fn dfs_build(
    children_map: &HashMap<Option<u32>, Vec<ProcessEntry>>,
    expanded: &HashSet<u32>,
    result: &mut Vec<TreeRow>,
) {
    // Stack entries: (entry, depth, is_last_sibling, guide_rails)
    let mut stack: Vec<(ProcessEntry, u16, bool, Vec<bool>)> = Vec::new();

    // Push roots in reverse so first root is popped first.
    if let Some(roots) = children_map.get(&None) {
        let len = roots.len();
        for (i, root) in roots.iter().enumerate().rev() {
            stack.push((root.clone(), 0, i == len - 1, Vec::new()));
        }
    }

    while let Some((entry, depth, is_last, guide_rails)) = stack.pop() {
        let pid = entry.pid;
        let children = children_map.get(&Some(pid));
        let has_children = children.is_some_and(|c| !c.is_empty());
        let is_expanded = expanded.contains(&pid);

        result.push(TreeRow {
            entry,
            depth,
            is_last_sibling: is_last,
            has_children,
            is_expanded,
            guide_rails: guide_rails.clone(),
        });

        // Push children in reverse order if expanded.
        if has_children
            && is_expanded
            && let Some(kids) = children
        {
            let kid_count = kids.len();
            // Build guide rails for children: current rails + whether
            // this node is NOT the last sibling (so a continuation line
            // should be drawn).
            let mut child_rails = guide_rails;
            child_rails.push(!is_last);

            for (i, kid) in kids.iter().enumerate().rev() {
                let kid_is_last = i == kid_count - 1;
                stack.push((kid.clone(), depth + 1, kid_is_last, child_rails.clone()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::snapshots::{ProcSnapshot, ProcessEntry};

    fn make_process(pid: u32, name: &str, parent: Option<u32>) -> ProcessEntry {
        let base = ProcSnapshot::stub().processes.into_iter().next().unwrap();
        ProcessEntry {
            pid,
            name: name.into(),
            parent_pid: parent,
            cpu_pct: 0.0,
            ..base
        }
    }

    fn make_thread(tid: u32, name: &str, parent: u32) -> ProcessEntry {
        let mut e = make_process(tid, name, Some(parent));
        e.is_thread = true;
        e
    }

    fn all_expanded(procs: &[ProcessEntry]) -> HashSet<u32> {
        procs.iter().map(|p| p.pid).collect()
    }

    #[test]
    fn empty_input_produces_empty_output() {
        let result = build_tree(
            &[],
            SortColumn::Cpu,
            SortDir::Desc,
            &ProcessFilter::None,
            &HashSet::new(),
        );
        assert!(result.is_empty());
    }

    #[test]
    fn single_root_process() {
        let procs = vec![make_process(1, "init", None)];
        let expanded = all_expanded(&procs);
        let rows = build_tree(
            &procs,
            SortColumn::Pid,
            SortDir::Asc,
            &ProcessFilter::None,
            &expanded,
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].depth, 0);
        assert_eq!(rows[0].entry.pid, 1);
        assert!(!rows[0].has_children);
    }

    #[test]
    fn parent_child_hierarchy() {
        let procs = vec![
            make_process(1, "init", None),
            make_process(100, "sshd", Some(1)),
            make_process(200, "bash", Some(100)),
        ];
        let expanded = all_expanded(&procs);
        let rows = build_tree(
            &procs,
            SortColumn::Pid,
            SortDir::Asc,
            &ProcessFilter::None,
            &expanded,
        );
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].depth, 0); // init
        assert_eq!(rows[1].depth, 1); // sshd
        assert_eq!(rows[2].depth, 2); // bash
        assert!(rows[0].has_children);
        assert!(rows[1].has_children);
        assert!(!rows[2].has_children);
    }

    #[test]
    fn collapsed_node_hides_children() {
        let procs = vec![
            make_process(1, "init", None),
            make_process(100, "sshd", Some(1)),
            make_process(200, "bash", Some(100)),
        ];
        // Only expand init (pid 1), NOT sshd (pid 100).
        let mut expanded = HashSet::new();
        expanded.insert(1);
        let rows = build_tree(
            &procs,
            SortColumn::Pid,
            SortDir::Asc,
            &ProcessFilter::None,
            &expanded,
        );
        assert_eq!(rows.len(), 2, "bash should be hidden (sshd collapsed)");
        assert_eq!(rows[0].entry.pid, 1);
        assert_eq!(rows[1].entry.pid, 100);
        assert!(!rows[1].is_expanded);
    }

    #[test]
    fn filter_preserves_ancestor_path() {
        let procs = vec![
            make_process(1, "init", None),
            make_process(100, "sshd", Some(1)),
            make_process(200, "bash", Some(100)),
            make_process(300, "nginx", Some(1)),
        ];
        let expanded = all_expanded(&procs);
        // Filter for "bash" — should show init → sshd → bash but NOT nginx.
        let rows = build_tree(
            &procs,
            SortColumn::Pid,
            SortDir::Asc,
            &ProcessFilter::Name("bash".into()),
            &expanded,
        );
        let pids: Vec<u32> = rows.iter().map(|r| r.entry.pid).collect();
        assert_eq!(pids, vec![1, 100, 200], "ancestor path must be preserved");
    }

    #[test]
    fn orphaned_process_becomes_root() {
        // pid 500's parent (pid 999) doesn't exist in the list.
        let procs = vec![
            make_process(1, "init", None),
            make_process(500, "orphan", Some(999)),
        ];
        let expanded = all_expanded(&procs);
        let rows = build_tree(
            &procs,
            SortColumn::Pid,
            SortDir::Asc,
            &ProcessFilter::None,
            &expanded,
        );
        assert_eq!(rows.len(), 2);
        // Both should be at depth 0 (roots).
        assert_eq!(rows[0].depth, 0);
        assert_eq!(rows[1].depth, 0);
    }

    #[test]
    fn threads_appear_as_children() {
        let procs = vec![
            make_process(1, "init", None),
            make_process(100, "firefox", Some(1)),
            make_thread(101, "thread-101", 100),
            make_thread(102, "thread-102", 100),
        ];
        let expanded = all_expanded(&procs);
        let rows = build_tree(
            &procs,
            SortColumn::Pid,
            SortDir::Asc,
            &ProcessFilter::None,
            &expanded,
        );
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[2].depth, 2); // thread under firefox under init
        assert!(rows[2].entry.is_thread);
        assert_eq!(rows[3].depth, 2);
        assert!(rows[3].entry.is_thread);
    }

    #[test]
    fn tree_prefix_depth_0() {
        let row = TreeRow {
            entry: make_process(1, "init", None),
            depth: 0,
            is_last_sibling: false,
            has_children: false,
            is_expanded: false,
            guide_rails: vec![],
        };
        assert_eq!(row.tree_prefix(), "");
    }

    #[test]
    fn tree_prefix_depth_1_not_last() {
        let row = TreeRow {
            entry: make_process(2, "child", Some(1)),
            depth: 1,
            is_last_sibling: false,
            has_children: false,
            is_expanded: false,
            guide_rails: vec![true],
        };
        assert_eq!(row.tree_prefix(), "├── ");
    }

    #[test]
    fn tree_prefix_depth_1_last() {
        let row = TreeRow {
            entry: make_process(2, "child", Some(1)),
            depth: 1,
            is_last_sibling: true,
            has_children: false,
            is_expanded: false,
            guide_rails: vec![false],
        };
        assert_eq!(row.tree_prefix(), "└── ");
    }

    #[test]
    fn tree_prefix_depth_2_with_rail() {
        let row = TreeRow {
            entry: make_process(3, "grandchild", Some(2)),
            depth: 2,
            is_last_sibling: true,
            has_children: false,
            is_expanded: false,
            guide_rails: vec![true, false],
        };
        // Parent is not last (rail[0]=true) → "│   ", then "└── "
        assert_eq!(row.tree_prefix(), "│   └── ");
    }

    #[test]
    fn tree_prefix_depth_2_no_rail() {
        let row = TreeRow {
            entry: make_process(3, "grandchild", Some(2)),
            depth: 2,
            is_last_sibling: false,
            has_children: false,
            is_expanded: false,
            guide_rails: vec![false, true],
        };
        // Parent is last (rail[0]=false) → "    ", then "├── "
        assert_eq!(row.tree_prefix(), "    ├── ");
    }

    #[test]
    fn tree_prefix_clamped_depth() {
        // Depth 6 > MAX_INDENT_DEPTH (4).  Should clamp to 4 levels of visual
        // indent with a "·· " marker.
        let row = TreeRow {
            entry: make_process(7, "deep", Some(6)),
            depth: 6,
            is_last_sibling: true,
            has_children: false,
            is_expanded: false,
            guide_rails: vec![true, true, true, true, true, false],
        };
        let prefix = row.tree_prefix();
        assert!(
            prefix.starts_with("·· "),
            "clamped prefix must start with '·· '; got: {prefix:?}"
        );
        assert!(
            prefix.ends_with("└── "),
            "clamped prefix must end with connector; got: {prefix:?}"
        );
    }

    #[test]
    fn sorting_within_tree_levels() {
        let procs = vec![
            make_process(1, "init", None),
            ProcessEntry {
                cpu_pct: 10.0,
                ..make_process(200, "zebra", Some(1))
            },
            ProcessEntry {
                cpu_pct: 50.0,
                ..make_process(100, "alpha", Some(1))
            },
        ];
        let expanded = all_expanded(&procs);

        // Sort by CPU descending — alpha (50%) should come before zebra (10%).
        let rows = build_tree(
            &procs,
            SortColumn::Cpu,
            SortDir::Desc,
            &ProcessFilter::None,
            &expanded,
        );
        assert_eq!(rows[1].entry.name, "alpha");
        assert_eq!(rows[2].entry.name, "zebra");
    }

    #[test]
    fn is_last_sibling_set_correctly() {
        let procs = vec![
            make_process(1, "init", None),
            make_process(10, "first", Some(1)),
            make_process(20, "middle", Some(1)),
            make_process(30, "last", Some(1)),
        ];
        let expanded = all_expanded(&procs);
        let rows = build_tree(
            &procs,
            SortColumn::Pid,
            SortDir::Asc,
            &ProcessFilter::None,
            &expanded,
        );
        // rows: init(0), first(1), middle(1), last(1)
        assert!(!rows[1].is_last_sibling, "first child not last");
        assert!(!rows[2].is_last_sibling, "middle child not last");
        assert!(rows[3].is_last_sibling, "last child is last");
    }
}
