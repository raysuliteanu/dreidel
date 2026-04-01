# [MED] Wildcard `_ => ProcessStatus::Unknown` silently absorbs new `sysinfo` variants — DONE

## Location
`src/stats/mod.rs:300–310` — `map_process_status` function

## Description
The function that maps `sysinfo::ProcessStatus` to the local `ProcessStatus` enum uses a
wildcard arm:

```rust
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
```

When `sysinfo` adds new process status variants (which it does between releases), the
wildcard silently maps them to `Unknown` with no indication that the local enum is out
of date. There is no compile-time signal that the match is non-exhaustive with respect
to the upstream enum.

## Impact
- New `sysinfo` process states will show as "Unknown" in the process table.
- No compile error or warning prompts updating the mapping after a `sysinfo` upgrade.

## Recommended Fix
Add a comment above the wildcard arm listing the current variants that fall through, so
future maintainers know whether a new variant should be added or legitimately maps to
`Unknown`:

```rust
// Variants not yet mapped to a local status — update if sysinfo adds states we care about.
// As of sysinfo 0.38: Tracing, Waking, Parked, LockBlocked, UninterruptibleDiskSleep
_ => ProcessStatus::Unknown,
```

Longer term, consider adding a `#[non_exhaustive]` note in `ProcessStatus` itself or
a test that runs `cargo tree` to alert on `sysinfo` version bumps.
