# [HIGH] Blocking `thread::sleep` in `Tui::stop()` called from async context — DONE

## Location
`src/tui.rs:135–150`

## Description
`Tui::stop()` is a synchronous function that busy-waits in a loop up to 100 iterations,
calling `std::thread::sleep(Duration::from_millis(1))` each iteration (up to 100 ms total).
It is called from `Tui::exit()`, which is called from `App::run()` inside a Tokio runtime.
Blocking an OS thread inside a Tokio context stalls the scheduler for up to 100 ms on every
shutdown, which is the exact pattern warned against in async Rust.

A secondary bug: when the loop exits due to the 100-iteration limit, the function logs an
`error!` but still returns `Ok(())`. Callers cannot distinguish a clean shutdown from a timeout.

## Impact
- Tokio worker thread blocked for up to 100 ms on shutdown.
- Timeout condition is undetectable by callers — silent failure masked as success.
- `stop()` is `pub`, so future callers could invoke it from truly async code.

## Recommended Fix
Replace the spin-sleep loop with a `tokio::time::timeout` await on the join handle,
or restructure `stop()` to be `async`. At minimum the timeout path should return an `Err`.

```rust
pub async fn stop(&mut self) -> anyhow::Result<()> {
    self.cancellation_token.cancel();
    tokio::time::timeout(
        Duration::from_millis(100),
        &mut self.task,
    )
    .await
    .context("TUI event loop did not stop within 100 ms")?
    .context("TUI event loop panicked")
}
```
