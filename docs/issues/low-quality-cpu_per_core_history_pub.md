# [LOW] `CpuComponent::per_core_history` is unnecessarily `pub` — DONE

## Location
`src/components/cpu.rs:45`

## Description
```rust
pub per_core_history: Vec<VecDeque<f64>>,
```

This field is accessed outside the `CpuComponent` impl only in the test
`history_ring_buffer_bounded` at line 436, which is in the same module (inside
`#[cfg(test)]`). There is no need for `pub` visibility — `pub(crate)` or simply restricting
access to tests would be sufficient.

Exposing it as fully `pub` (accessible outside the crate) leaks an internal implementation
detail: callers could observe or manipulate the ring buffer without going through the
component's update cycle.

## Impact
- Exposes internal state as part of the crate's public API.
- Tests relying on direct field access bypass the component interface and may produce
  false confidence.

## Recommended Fix
Change to `pub(crate)` or use a test-only accessor:

```rust
#[cfg(test)]
pub fn per_core_history(&self) -> &[VecDeque<f64>] {
    &self.per_core_history
}
```

Then make the field private:
```rust
per_core_history: Vec<VecDeque<f64>>,
```
