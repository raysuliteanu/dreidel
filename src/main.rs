// Allow dead code during scaffolding — modules are stubs that will be wired
// together in later tasks. Without this, clippy -D warnings rejects every
// pub item that isn't yet referenced from main().
#![allow(dead_code)]

mod action;
mod app;
mod cli;
mod components;
mod config;
mod errors;
mod layout;
mod stats;
mod theme;
mod tui;

fn main() -> anyhow::Result<()> {
    println!("toppers");
    Ok(())
}
