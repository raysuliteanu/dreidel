// SPDX-License-Identifier: GPL-3.0-only

//! dreidel — a fast, keyboard-driven terminal system monitor.
//!
//! This library crate re-exports the public modules used by integration tests
//! and benchmarks. The binary entry point lives in `src/main.rs`.

pub mod action;
pub mod components;
pub mod config;
pub mod layout;
pub mod stats;
pub mod theme;
pub mod tui;
