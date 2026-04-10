// SPDX-License-Identifier: GPL-3.0-only

//! Binary entry point for dreidel.
//!
//! Parses CLI arguments, loads the config file, detects the terminal theme,
//! and launches the [`App`] event loop on a Tokio runtime.

use anyhow::Context;
use clap::Parser;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod action;
mod app;
mod cli;
mod components;
mod config;
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

    if args.detect_theme {
        print_detected_theme();
        return Ok(());
    }

    let mut cfg = config::Config::load(args.config.as_deref()).context("loading config")?;
    apply_cli_overrides(&mut cfg, &args)?;

    // Resolve Auto before entering raw mode — termbg queries the terminal via
    // OSC 11 which requires normal (non-raw) terminal state.
    let detected_theme = if cfg.general.theme == theme::Theme::Auto {
        let detected = detect_theme();
        cfg.general.theme = detected;
        Some(detected)
    } else {
        tracing::info!(theme = %cfg.general.theme, "using configured theme, skipping detection");
        None
    };
    let active_theme = cfg.general.theme;
    tracing::info!(%active_theme, ?detected_theme, "theme resolved");

    let rt = tokio::runtime::Runtime::new().context("creating tokio runtime")?;
    rt.block_on(async {
        let mut app = app::App::new(cfg, detected_theme).context("creating App")?;
        app.run().await.context("running App")
    })
}

/// Query the terminal background color via OSC 11 and return Light or Dark.
/// Falls back to Dark if the terminal doesn't respond or the query times out.
fn detect_theme() -> theme::Theme {
    let timeout = std::time::Duration::from_millis(100);
    match termbg::theme(timeout) {
        Ok(termbg::Theme::Light) => {
            tracing::info!("termbg detected light terminal background");
            theme::Theme::Light
        }
        Ok(termbg::Theme::Dark) => {
            tracing::info!("termbg detected dark terminal background");
            theme::Theme::Dark
        }
        Err(ref e) => {
            tracing::warn!(error = %e, "termbg detection failed, defaulting to dark");
            theme::Theme::Dark
        }
    }
}

/// Print theme detection diagnostics and exit.
fn print_detected_theme() {
    let timeout = std::time::Duration::from_millis(100);

    println!("Terminal type: {:?}", termbg::terminal());
    println!(
        "stdin is_terminal: {}",
        std::io::IsTerminal::is_terminal(&std::io::stdin())
    );
    println!(
        "stdout is_terminal: {}",
        std::io::IsTerminal::is_terminal(&std::io::stdout())
    );
    println!(
        "stderr is_terminal: {}",
        std::io::IsTerminal::is_terminal(&std::io::stderr())
    );
    if let Ok(term) = std::env::var("TERM") {
        println!("TERM: {term}");
    }
    if let Ok(val) = std::env::var("COLORFGBG") {
        println!("COLORFGBG: {val}");
    }

    match termbg::rgb(timeout) {
        Ok(rgb) => {
            println!("Background RGB: r={} g={} b={}", rgb.r, rgb.g, rgb.b);
            // ITU-R BT.601 luminance (same formula termbg uses)
            let y = rgb.r as f64 * 0.299 + rgb.g as f64 * 0.587 + rgb.b as f64 * 0.114;
            let theme = if y > 32768.0 { "Light" } else { "Dark" };
            println!("Luminance: {y:.0} (threshold: 32768)");
            println!("Detected theme: {theme}");
        }
        Err(ref e) => {
            println!("Theme detection failed: {e:?}");
            println!("Would default to: Dark");
        }
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
    if let Some(r) = &args.thread_refresh
        && let Ok(d) = humantime::parse_duration(r)
    {
        cfg.general.thread_refresh_ms = d.as_millis() as u64;
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

const DEFAULT_CONFIG_TEMPLATE: &str = include_str!("default-config.toml");

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
