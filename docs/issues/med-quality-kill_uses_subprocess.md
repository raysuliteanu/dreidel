# [MED] Process kill uses `std::process::Command` (subprocess) instead of syscall

## Location
`src/components/process/mod.rs:630–648`

## Description
The kill action sends SIGTERM by spawning the external `kill` binary:

```rust
std::process::Command::new("kill")
    .arg("-TERM")
    .arg(pid.to_string())
    .output()
    ...
```

While injection is not possible here (`pid` is a `u32` passed as a separate `.arg()`, not
string-interpolated), this is unnecessarily heavy:

- Spawns a child process (fork + exec) just to call `kill(2)`.
- Introduces a runtime dependency on the `kill` binary being present in `$PATH`; on some
  minimal Linux environments or containers it may not be.
- Adds latency and memory overhead compared to a direct syscall.
- The error from the command output is checked but the mapping to a user-facing message
  loses information.

## Impact
- Functional correctness risk on minimal environments lacking `kill` in PATH.
- Unnecessary OS-level overhead for a common operation.

## Recommended Fix
Use `nix::sys::signal::kill` or `libc::kill` directly:

```rust
// With the `nix` crate (already transitive via procfs):
use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;

kill(Pid::from_raw(pid as i32), Signal::SIGTERM)
    .context("sending SIGTERM")?;
```

Or via `libc` directly (already available transitively):
```rust
let ret = unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
if ret != 0 {
    return Err(std::io::Error::last_os_error()).context("sending SIGTERM");
}
```

Check whether `nix` is already an indirect dependency before adding it explicitly.
