# [HIGH] Key-handler errors silently discarded on focused component — DONE

## Location
`src/app.rs:247–251`

## Description
When dispatching a key event to the focused component, errors from `handle_key_event` are
silently converted to `None` via `.ok()`:

```rust
let consumed = self
    .components
    .iter_mut()
    .find(|(id, _)| *id == focused_id)
    .and_then(|(_, comp)| comp.handle_key_event(*key).ok().flatten());
```

If a component's key handler returns `Err(e)`, the error is thrown away, `consumed` becomes
`None`, and the global key handler runs instead — as if the component never received the key.
This can cause unexpected behavior (e.g. a component in a sub-state that intended to consume
a key ends up triggering a global action) and makes bugs in key handlers invisible.

## Impact
- Errors from key handlers are unlogged and unrecoverable.
- The global handler may fire on key events that should have been consumed.
- Debugging key-handling bugs in components is extremely difficult without any log signal.

## Recommended Fix
Propagate or log the error rather than silencing it:

```rust
let consumed = self
    .components
    .iter_mut()
    .find(|(id, _)| *id == focused_id)
    .and_then(|(_, comp)| match comp.handle_key_event(*key) {
        Ok(action) => action,
        Err(e) => {
            tracing::warn!(component = ?focused_id, error = %e, "key handler error");
            None
        }
    });
```

Alternatively, propagate via `?` and let the outer `handle_events` method return the error
to `App::run`, where it can be treated as a fatal condition.
