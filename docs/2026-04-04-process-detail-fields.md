## Process Detail Field Gap Analysis

Current process detail view shows:

- PID
- Name
- Command line
- User
- Status
- CPU%
- MEM% and resident bytes
- Virtual bytes
- Nice
- Thread count
- Read bytes
- Write bytes

Already collected in `ProcessEntry` but not shown:

- Parent PID
- Kernel priority
- Shared memory bytes
- Total CPU time
- Start time
- Runtime
- Whether the row is a thread

High-value fields to add next:

- Parent PID
- Kernel priority
- Shared memory bytes
- Total CPU time
- Start time
- Runtime
- Executable path
- Current working directory

Second-tier fields to add next:

- Root directory
- Effective UID
- GID
- Session ID
- TTY
- User CPU time
- System CPU time
- Minor page faults
- Major page faults
- Voluntary context switches
- Nonvoluntary context switches
- Open file descriptor count
- Swap bytes
- I/O syscall counters
- Character I/O bytes
- Cancelled write bytes

Lower-value or noisier fields intentionally deferred:

- Full environment
- Capability masks
- Seccomp
- Cgroups
- Limits
- Full smaps breakdown
