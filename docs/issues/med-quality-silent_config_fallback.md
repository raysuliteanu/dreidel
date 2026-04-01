# [MED] Invalid config values silently fall back to defaults — DONE

## Location
- `src/app.rs:123` — `LayoutPreset::from_str(&config.layout.preset).unwrap_or_default()`
- `src/app.rs:124–128` — manual string match for `StatusBarPosition`

## Description

### Layout preset
An unknown `layout.preset` value in `config.toml` or via `--preset` is silently treated as
the default preset:

```rust
let preset = LayoutPreset::from_str(&config.layout.preset).unwrap_or_default();
```

If a user typos `"sidbar"` instead of `"sidebar"`, they get the default layout with no
feedback.

### Status bar position
The `status_bar` config field is matched with a raw string compare and a catch-all default:

```rust
let status_pos = match config.layout.status_bar.as_str() {
    "bottom" => StatusBarPosition::Bottom,
    "hidden" => StatusBarPosition::Hidden,
    _ => StatusBarPosition::Top,
};
```

Any invalid value (e.g. `"topmost"`) silently uses `Top`. Unlike `LayoutPreset`,
`StatusBarPosition` does not even attempt a parse — it has no `FromStr` implementation.

## Impact
- Users with a misconfigured `config.toml` see no error; the app starts with incorrect
  behavior and no diagnostic.
- Config validation bugs are invisible in production.

## Recommended Fix

### For `LayoutPreset`
Replace `unwrap_or_default()` with an error or warning:
```rust
let preset = LayoutPreset::from_str(&config.layout.preset)
    .unwrap_or_else(|_| {
        tracing::warn!(
            value = %config.layout.preset,
            "unknown layout preset; using default"
        );
        LayoutPreset::default()
    });
```

### For `StatusBarPosition`
Implement `FromStr` (or use `strum::EnumString`) and parse with an error/warning:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, strum::EnumString)]
#[strum(serialize_all = "lowercase")]
pub enum StatusBarPosition {
    #[default]
    Top,
    Bottom,
    Hidden,
}
```

Then:
```rust
let status_pos = StatusBarPosition::from_str(&config.layout.status_bar)
    .unwrap_or_else(|_| {
        tracing::warn!(value = %config.layout.status_bar, "unknown status_bar position; using top");
        StatusBarPosition::default()
    });
```
