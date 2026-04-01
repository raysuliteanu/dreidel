// SPDX-License-Identifier: GPL-3.0-only

use anyhow::Result;
use vergen_gix::{Emitter, GixBuilder};

fn main() -> Result<()> {
    let gix = GixBuilder::all_git()?;
    Emitter::default().add_instructions(&gix)?.emit()
}
