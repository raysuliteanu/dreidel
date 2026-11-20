# CLAUDE.md

@AGENTS.md

## Claude Code hooks

The checks described in the "Development Checks" section of AGENTS.md are
enforced via Claude Code hooks in this project's Claude Code settings:

- **PostToolUse** on `.rs` file edits → `cargo clippy -D warnings`
- **Pre-push** → `cargo fmt --check`

No manual action is needed — they fire automatically.
