# [LOW] `DiskDeviceSnapshot::kind` is `String` but only takes three known values

## Location
`src/stats/snapshots.rs:124`
`src/stats/mod.rs` — `build_disk()` function

## Description
`DiskDeviceSnapshot::kind` is declared as `String`:

```rust
pub struct DiskDeviceSnapshot {
    ...
    pub kind: String,
    ...
}
```

In `build_disk()`, only three values are ever assigned:
```rust
kind: match disk.kind() {
    DiskKind::SSD => "SSD",
    DiskKind::HDD => "HDD",
    _ => "Unknown",
}.to_string(),
```

Storing this as a `String` means:
- Comparison in display/filtering code requires string equality rather than enum matching.
- A typo in the string literal (e.g. `"Ssd"`) would compile silently.
- The set of valid values is not encoded in the type system.

## Impact
- Low — the values are set in one place and read in another, so the risk of divergence is
  contained. No exhaustive matching is missed because there is only one write site.

## Recommended Fix
Replace with an enum:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskKindSummary {
    Ssd,
    Hdd,
    Unknown,
}

impl fmt::Display for DiskKindSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ssd => write!(f, "SSD"),
            Self::Hdd => write!(f, "HDD"),
            Self::Unknown => write!(f, "Unknown"),
        }
    }
}
```

This is `Copy`, eliminating any allocation, and makes the valid values explicit.
