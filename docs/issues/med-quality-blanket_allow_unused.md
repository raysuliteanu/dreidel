# [MED] Blanket `#![allow(unused)]` / `#![allow(dead_code)]` suppress entire modules

## Location
- `src/action.rs:3` — `#![allow(unused)]`
- `src/stats/snapshots.rs:3` — `#![allow(dead_code)]`

## Description
Both files suppress all unused-identifier warnings for the entire module with a file-level
inner attribute. This makes it impossible to detect genuinely dead code that accumulates
over time.

**`action.rs`**: The stated rationale is that some `Action` variants are part of the event
vocabulary but not yet dispatched (e.g. `Error(String)`, `Suspend`, `Resume`,
`FocusGained`, `FocusLost`). This is reasonable, but the blanket suppression also hides
unintentionally unused variants.

**`snapshots.rs`**: Several `ProcessEntry` fields (`start_time`, `run_time`, `parent_pid`,
`threads`) appear to be unused outside of struct construction. The blanket allow prevents
clippy from flagging them, making it impossible to audit which fields are load-bearing.

## Impact
- Dead code accumulates invisibly. Future removals of features may leave orphaned
  `Action` variants or snapshot fields that clippy can no longer identify.
- Code reviewers cannot rely on "no dead_code warnings" as a signal of clean state.

## Recommended Fix
Remove the file-level attributes. Apply `#[allow(dead_code)]` individually to each
variant/field that is intentionally unused, with a comment explaining why:

```rust
// Reserved for future error reporting to the UI layer.
#[allow(dead_code)]
Error(String),

// Used only on platforms that support suspend (e.g. Ctrl+Z / SIGTSTP).
#[allow(dead_code)]
Suspend,
```

For snapshot fields that are genuinely unused, either remove them or document why they
are retained (e.g. "reserved for future detail view").
