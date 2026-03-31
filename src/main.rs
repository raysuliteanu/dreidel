// SPDX-License-Identifier: GPL-3.0-only

use anyhow::Context;
use clap::Parser;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

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
    let args = cli::Args::parse();

    // Log to a file so tracing output doesn't corrupt the TUI.
    let log_dir = dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("dreidel");
    std::fs::create_dir_all(&log_dir).context("creating log dir")?;
    let log_file =
        std::fs::File::create(log_dir.join("dreidel.log")).context("creating log file")?;
    let level = match args.verbose {
        0 => tracing::Level::WARN,
        1 => tracing::Level::INFO,
        _ => tracing::Level::DEBUG,
    };
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_writer(log_file))
        .with(tracing_subscriber::filter::LevelFilter::from_level(level))
        .init();

    if args.init_config {
        print!("{}", DEFAULT_CONFIG_TEMPLATE);
        return Ok(());
    }

    let mut cfg = config::Config::load(args.config.as_deref()).context("loading config")?;
    apply_cli_overrides(&mut cfg, &args)?;

    // Resolve Auto before entering raw mode — termbg queries the terminal via
    // OSC 11 which requires normal (non-raw) terminal state.
    if cfg.general.theme == theme::Theme::Auto {
        cfg.general.theme = detect_theme();
    }

    let rt = tokio::runtime::Runtime::new().context("creating tokio runtime")?;
    rt.block_on(async {
        let mut app = app::App::new(cfg).context("creating App")?;
        app.run().await.context("running App")
    })
}

/// Query the terminal background color via OSC 11 and return Light or Dark.
/// Falls back to Dark if the terminal doesn't respond or the query times out.
fn detect_theme() -> theme::Theme {
    match termbg::theme(std::time::Duration::from_millis(100)) {
        Ok(termbg::Theme::Light) => theme::Theme::Light,
        _ => theme::Theme::Dark,
    }
}

fn apply_cli_overrides(cfg: &mut config::Config, args: &cli::Args) -> anyhow::Result<()> {
    if let Some(t) = &args.theme
        && let Ok(theme) = t.parse()
    {
        cfg.general.theme = theme;
    }
    if let Some(r) = &args.refresh_rate
        && let Ok(d) = humantime::parse_duration(r)
    {
        cfg.general.refresh_rate_ms = d.as_millis() as u64;
    }
    if let Some(p) = &args.preset {
        cfg.layout.preset = p.clone();
    }
    if let Some(pos) = &args.status_bar {
        cfg.layout.status_bar = pos.clone();
    }
    // --hide wins over --show
    if let Some(show) = &args.show {
        for name in show {
            validate_component_name(name)?;
        }
        cfg.layout.show = show.clone();
    }
    if let Some(hide) = &args.hide {
        for name in hide {
            validate_component_name(name)?;
        }
        cfg.layout.show.retain(|c| !hide.contains(c));
    }
    Ok(())
}

const VALID_COMPONENTS: &str = "cpu, net, disk, process";

fn validate_component_name(name: &str) -> anyhow::Result<()> {
    match name {
        "cpu" | "net" | "disk" | "process" => Ok(()),
        _ => anyhow::bail!("unknown component {name:?}; valid values are: {VALID_COMPONENTS}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn validate_component_name_accepts_all_valid_names() {
        for name in &["cpu", "net", "disk", "process"] {
            assert!(
                validate_component_name(name).is_ok(),
                "{name} should be valid"
            );
        }
    }

    #[test]
    fn validate_component_name_rejects_unknown() {
        assert!(validate_component_name("foo").is_err());
        // These look plausible but are not valid component names.
        assert!(validate_component_name("mem").is_err());
        assert!(validate_component_name("CPU").is_err()); // case-sensitive
    }

    #[test]
    fn apply_cli_overrides_rejects_unknown_show_component() {
        let mut cfg = config::Config::default();
        let args = cli::Args::try_parse_from(["dreidel", "--show", "net,foo"]).unwrap();
        assert!(apply_cli_overrides(&mut cfg, &args).is_err());
    }

    #[test]
    fn apply_cli_overrides_rejects_unknown_hide_component() {
        let mut cfg = config::Config::default();
        let args = cli::Args::try_parse_from(["dreidel", "--hide", "bar"]).unwrap();
        assert!(apply_cli_overrides(&mut cfg, &args).is_err());
    }

    #[test]
    fn apply_cli_overrides_valid_show_updates_config() {
        let mut cfg = config::Config::default();
        let args = cli::Args::try_parse_from(["dreidel", "--show", "net,cpu"]).unwrap();
        apply_cli_overrides(&mut cfg, &args).unwrap();
        assert_eq!(cfg.layout.show, vec!["net", "cpu"]);
    }

    #[test]
    fn apply_cli_overrides_valid_hide_removes_component() {
        let mut cfg = config::Config::default();
        let args = cli::Args::try_parse_from(["dreidel", "--hide", "net"]).unwrap();
        apply_cli_overrides(&mut cfg, &args).unwrap();
        assert!(!cfg.layout.show.contains(&"net".to_string()));
    }
}

const DEFAULT_CONFIG_TEMPLATE: &str = r#"# dreidel default configuration
# Generated by: dreidel --init-config
# All options are commented out — uncomment and edit as needed.

[general]
# Refresh interval for all components. Examples: "500ms", "1s", "2s"
# refresh_rate = "1s"

# Color theme. "auto" detects light/dark from terminal background.
# theme = "auto"   # "auto" | "light" | "dark"

# Bounded action channel capacity.
# channel_capacity = 128

[layout]
# Base layout preset.
# preset = "sidebar"   # "sidebar" | "classic" | "dashboard"

# Status bar position.
# status_bar = "top"   # "top" | "bottom" | "hidden"

# Which components to show (omit to hide).
# show = ["cpu", "mem", "net", "disk", "process"]

# Slot overrides — replace default component in a named slot.
# Sidebar slots: left_top, left_mid, left_bot, right
# Classic slots: top_left, top_right_top, top_right_bot, bottom
# Dashboard slots: top, mid_left, mid_right, bottom
# left_top = "cpu"

[process]
# Default sort column: "cpu" | "mem" | "pid" | "name"
# default_sort = "cpu"
# default_sort_dir = "desc"   # "asc" | "desc"
# show_tree = false

[keybindings]
# focus_proc = "p"
# focus_cpu  = "c"
# focus_mem  = "m"
# focus_net  = "n"
# focus_disk = "d"
# fullscreen = "f"
# help       = "?"
"#;
