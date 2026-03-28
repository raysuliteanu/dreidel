// SPDX-License-Identifier: GPL-3.0-only

use clap::Parser;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "toppers", about = "A modern TUI system monitor", version = env!("TOPPERS_VERSION"))]
pub struct Args {
    #[arg(long, help = "Color theme: auto | light | dark")]
    pub theme: Option<String>,

    #[arg(long, value_name = "RATE", help = "Refresh rate e.g. 500ms, 1s, 2s")]
    pub refresh_rate: Option<String>,

    #[arg(
        long,
        value_name = "LAYOUT",
        help = "Layout preset: sidebar | classic | dashboard"
    )]
    pub preset: Option<String>,

    #[arg(
        long,
        value_delimiter = ',',
        value_name = "COMPONENTS",
        help = "Components to show: cpu,mem,net,disk,proc"
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

    #[arg(long, help = "Show debug sidebar on startup")]
    pub debug: bool,

    #[arg(short, long, action = clap::ArgAction::Count, help = "Increase verbosity (-v, -vv)")]
    pub verbose: u8,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_theme_flag() {
        let args = Args::try_parse_from(["toppers", "--theme", "dark"]).unwrap();
        assert_eq!(args.theme, Some("dark".to_string()));
    }

    #[test]
    fn init_config_flag_parses() {
        let args = Args::try_parse_from(["toppers", "--init-config"]).unwrap();
        assert!(args.init_config);
    }
}
