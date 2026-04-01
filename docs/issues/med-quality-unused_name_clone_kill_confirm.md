# [MED] Unused `_name` variable with unnecessary clone in `KillConfirm` branch — DONE

## Location
`src/components/process/mod.rs:195`

## Description
Inside the `KillConfirm` branch of the process component's key handler:

```rust
let _name = name.clone();
```

`_name` is never used — the leading underscore is the Rust convention for intentionally
unused bindings, but here there is no reason for the binding to exist at all. The clone
produces a heap allocation that is immediately dropped.

## Impact
- Unnecessary `String` allocation and deallocation per keypress in kill-confirm mode.
- Dead code that creates confusion for readers ("why is this here?").

## Recommended Fix
Remove both the `let _name = ...` binding and the `.clone()` call entirely. If `name` itself
is only needed inside this branch and the intent was to move it somewhere, verify whether it
is actually needed at all in this context.
