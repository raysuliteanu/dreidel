// SPDX-License-Identifier: GPL-3.0-only

//! Criterion benchmarks for the process tree builder.
//!
//! Run with:
//!   cargo bench --bench tree_build
//!
//! Results are written to `target/criterion/`.

use std::collections::HashSet;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};

use dreidel::components::process::filter::ProcessFilter;
use dreidel::components::process::sort::{SortColumn, SortDir};
use dreidel::components::process::tree::build_tree;
use dreidel::stats::snapshots::{ProcessEntry, ProcessStatus};

/// Build a synthetic process list of `n` entries arranged in a realistic tree.
///
/// Structure:
/// - ~10 root processes (pid 1..10)
/// - Each root has `n / 10` children distributed evenly
/// - ~20% of children have one grandchild each
/// - Names and cmd vecs are realistic lengths
fn make_processes(n: usize) -> Vec<ProcessEntry> {
    let root_count = 10.min(n);
    let mut procs = Vec::with_capacity(n);

    // Roots
    for i in 0..root_count {
        procs.push(make_entry(
            (i + 1) as u32,
            &format!("daemon-{i}"),
            None,
            false,
        ));
    }

    // Distribute remaining processes as children/grandchildren
    let mut pid = (root_count + 1) as u32;
    let children_per_root = if root_count > 0 {
        (n - root_count) / root_count
    } else {
        0
    };

    for root_idx in 0..root_count {
        let parent_pid = (root_idx + 1) as u32;
        for c in 0..children_per_root {
            if pid as usize > n {
                break;
            }
            let child_pid = pid;
            procs.push(make_entry(
                child_pid,
                &format!("worker-{root_idx}-{c}"),
                Some(parent_pid),
                false,
            ));
            pid += 1;

            // ~20% of children get a grandchild
            if c % 5 == 0 && (pid as usize) <= n {
                procs.push(make_entry(
                    pid,
                    &format!("helper-{root_idx}-{c}"),
                    Some(child_pid),
                    false,
                ));
                pid += 1;
            }
        }
    }

    // Fill any remainder
    while procs.len() < n {
        procs.push(make_entry(pid, &format!("extra-{pid}"), Some(1), false));
        pid += 1;
    }

    procs.truncate(n);
    procs
}

/// Build a synthetic list with threads: `n` processes, each with `threads_per`
/// thread entries.
fn make_processes_with_threads(n: usize, threads_per: usize) -> Vec<ProcessEntry> {
    let mut procs = make_processes(n);
    let mut tid = (n as u32 + 1) * 100; // high TIDs to avoid collision
    let proc_pids: Vec<u32> = procs.iter().map(|p| p.pid).collect();

    for &ppid in &proc_pids {
        for t in 0..threads_per {
            procs.push(make_entry(
                tid,
                &format!("[thread-{ppid}:{t}]"),
                Some(ppid),
                true,
            ));
            tid += 1;
        }
    }

    procs
}

fn make_entry(pid: u32, name: &str, parent: Option<u32>, is_thread: bool) -> ProcessEntry {
    ProcessEntry {
        pid,
        name: name.to_string(),
        cmd: vec![format!("/usr/bin/{name}"), "--flag".into(), "arg".into()],
        user: "benchuser".to_string(),
        cpu_pct: (pid % 100) as f32,
        mem_bytes: (pid as u64) * 1_000_000,
        mem_pct: (pid % 50) as f32 * 0.1,
        virt_bytes: (pid as u64) * 2_000_000,
        status: ProcessStatus::Running,
        start_time: 0,
        run_time: 3600,
        nice: 0,
        threads: 4,
        read_bytes: 0,
        write_bytes: 0,
        parent_pid: parent,
        priority: 20,
        shr_bytes: 0,
        cpu_time_secs: 123.4,
        is_thread,
    }
}

fn bench_build_tree(c: &mut Criterion) {
    let mut group = c.benchmark_group("build_tree");

    for &size in &[500, 2_000, 5_000, 10_000] {
        let procs = make_processes(size);
        let expanded: HashSet<u32> = procs.iter().map(|p| p.pid).collect();

        group.bench_with_input(
            BenchmarkId::new("all_expanded", size),
            &(procs, expanded),
            |b, (procs, expanded)| {
                b.iter(|| {
                    build_tree(
                        procs,
                        SortColumn::Cpu,
                        SortDir::Desc,
                        &ProcessFilter::None,
                        expanded,
                    )
                });
            },
        );
    }

    // Benchmark with mostly-collapsed tree (only roots expanded)
    for &size in &[2_000, 5_000, 10_000] {
        let procs = make_processes(size);
        let expanded: HashSet<u32> = (1..=10).collect(); // only roots

        group.bench_with_input(
            BenchmarkId::new("roots_only_expanded", size),
            &(procs, expanded),
            |b, (procs, expanded)| {
                b.iter(|| {
                    build_tree(
                        procs,
                        SortColumn::Cpu,
                        SortDir::Desc,
                        &ProcessFilter::None,
                        expanded,
                    )
                });
            },
        );
    }

    group.finish();
}

fn bench_build_tree_with_threads(c: &mut Criterion) {
    let mut group = c.benchmark_group("build_tree_with_threads");

    // Realistic: 500 procs × 4 threads = 2500 total entries
    for &(procs, threads) in &[(500, 4), (1_000, 4), (2_000, 2)] {
        let entries = make_processes_with_threads(procs, threads);
        let total = entries.len();
        let expanded: HashSet<u32> = entries.iter().map(|p| p.pid).collect();

        group.bench_with_input(
            BenchmarkId::new(format!("{procs}p_{threads}t"), total),
            &(entries, expanded),
            |b, (entries, expanded)| {
                b.iter(|| {
                    build_tree(
                        entries,
                        SortColumn::Cpu,
                        SortDir::Desc,
                        &ProcessFilter::None,
                        expanded,
                    )
                });
            },
        );
    }

    group.finish();
}

fn bench_build_tree_with_filter(c: &mut Criterion) {
    let mut group = c.benchmark_group("build_tree_filtered");

    for &size in &[2_000, 5_000] {
        let procs = make_processes(size);
        let expanded: HashSet<u32> = procs.iter().map(|p| p.pid).collect();
        // Filter that matches ~10% of processes
        let filter = ProcessFilter::Name("worker-0".into());

        group.bench_with_input(
            BenchmarkId::new("name_filter_10pct", size),
            &(procs, expanded, filter),
            |b, (procs, expanded, filter)| {
                b.iter(|| build_tree(procs, SortColumn::Cpu, SortDir::Desc, filter, expanded));
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_build_tree,
    bench_build_tree_with_threads,
    bench_build_tree_with_filter,
);
criterion_main!(benches);
