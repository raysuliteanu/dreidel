// SPDX-License-Identifier: GPL-3.0-only

//! Build script — embeds git metadata (commit SHA) via `vergen-gix` for the
//! `--version` output.

use anyhow::Result;
use vergen_gix::{Emitter, GixBuilder};

fn main() -> Result<()> {
    let gix = GixBuilder::all_git()?;
    Emitter::default().add_instructions(&gix)?.emit()
}
