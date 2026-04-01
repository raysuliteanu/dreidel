// SPDX-License-Identifier: GPL-3.0-only

use clap::Parser;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "dreidel", about = "A modern TUI system monitor", version = version())]
pub struct Args {
    #[arg(long, help = "Color theme: auto | light | dark")]
    pub theme: Option<String>,

    #[arg(long, value_name = "RATE", help = "Refresh rate e.g. 500ms, 1s, 2s")]
    pub refresh_rate: Option<String>,

    #[arg(
        long,
        value_name = "LAYOUT",
        help = "Layout preset: sidebar | classic | dashboard | grid"
    )]
    pub preset: Option<String>,

    #[arg(
        long,
        value_delimiter = ',',
        value_name = "COMPONENTS",
        help = "Components to show: cpu,net,disk,process"
    )]
    pub show: Option<Vec<String>>,

    #[arg(
        long,
        value_delimiter = ',',
        value_name = "COMPONENTS",
        help = "Components to hide (hide wins over --show)"
    )]
    pub hide: Option<Vec<String>>,

    #[arg(
        long,
        value_name = "POS",
        help = "Status bar position: top | bottom | hidden"
    )]
    pub status_bar: Option<String>,

    #[arg(long, value_name = "PATH", help = "Alternate config file path")]
    pub config: Option<PathBuf>,

    #[arg(long, help = "Print default config to stdout and exit")]
    pub init_config: bool,

    #[arg(short, long, action = clap::ArgAction::Count, help = "Increase verbosity (-v, -vv)")]
    pub verbose: u8,
}

// clap's version attribute requires &'static str. Since we need a short SHA
// suffix computed from a compile-time constant, we build the string once and
// leak it to satisfy the 'static bound.
fn version() -> &'static str {
    static V: std::sync::OnceLock<&'static str> = std::sync::OnceLock::new();
    V.get_or_init(|| {
        let sha = &env!("VERGEN_GIT_SHA")[..7];
        Box::leak(format!("{} ({sha})", env!("CARGO_PKG_VERSION")).into_boxed_str())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_theme_flag() {
        let args = Args::try_parse_from(["dreidel", "--theme", "dark"]).unwrap();
        assert_eq!(args.theme, Some("dark".to_string()));
    }

    #[test]
    fn init_config_flag_parses() {
        let args = Args::try_parse_from(["dreidel", "--init-config"]).unwrap();
        assert!(args.init_config);
    }
}
