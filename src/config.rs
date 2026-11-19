// SPDX-License-Identifier: GPL-3.0-only

//! Application configuration loaded from `~/.config/dreidel/config.toml`.
//!
//! All fields have serde defaults so a missing or partial config file is fine.
//! Sub-structs: [`GeneralConfig`], [`LayoutConfig`], [`ProcessConfig`],
//! [`KeyBindings`].

use crate::theme::Theme;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct GeneralConfig {
    #[serde(deserialize_with = "deserialize_duration", default = "default_refresh")]
    pub refresh_rate_ms: u64,
    /// How often to enumerate per-process threads via `/proc/<pid>/task/`.
    /// Thread enumeration is expensive (thousands of syscalls) and thread
    /// lists don't change fast enough to justify running every tick.
    #[serde(
        deserialize_with = "deserialize_duration",
        default = "default_thread_refresh"
    )]
    pub thread_refresh_ms: u64,
    pub theme: Theme,
    pub channel_capacity: usize,
}

/// Parse a human-friendly duration such as `"500ms"`, `"2s"` or `"1m 30s"` into
/// whole milliseconds. Negative durations are rejected.
pub fn parse_duration_ms(s: &str) -> Result<u64, String> {
    let d: jiff::SignedDuration = s
        .trim()
        .parse()
        .map_err(|e| format!("invalid duration {s:?}: {e}"))?;
    if d.is_negative() {
        return Err(format!("duration must not be negative: {s:?}"));
    }
    u64::try_from(d.as_millis()).map_err(|_| format!("duration too large: {s:?}"))
}

fn deserialize_duration<'de, D: serde::Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
    let s = String::deserialize(d)?;
    parse_duration_ms(&s).map_err(serde::de::Error::custom)
}

// TODO: address magic number; it's duplicated below in default()
fn default_refresh() -> u64 {
    1000
}

fn default_thread_refresh() -> u64 {
    5000
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            refresh_rate_ms: 1000,
            thread_refresh_ms: 5000,
            theme: Theme::Auto,
            // TODO: address magic number
            channel_capacity: 128,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct LayoutConfig {
    pub preset: String,
    pub status_bar: String,
    pub show: Vec<String>,
    pub left_top: Option<String>,
    pub left_bot: Option<String>,
    pub right: Option<String>,
    pub top_left: Option<String>,
    pub top_right_top: Option<String>,
    pub top_right_bot: Option<String>,
    pub bottom: Option<String>,
    pub top: Option<String>,
    pub mid_left: Option<String>,
    pub mid_right: Option<String>,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self {
            preset: "sidebar".into(),
            status_bar: "top".into(),
            show: vec!["cpu".into(), "net".into(), "disk".into(), "process".into()],
            left_top: None,
            left_bot: None,
            right: None,
            top_left: None,
            top_right_top: None,
            top_right_bot: None,
            bottom: None,
            top: None,
            mid_left: None,
            mid_right: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct ProcessConfig {
    pub default_sort: String,
    pub default_sort_dir: String,
    pub show_tree: bool,
}

impl Default for ProcessConfig {
    fn default() -> Self {
        Self {
            default_sort: "cpu".into(),
            default_sort_dir: "desc".into(),
            show_tree: false,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct KeyBindings {
    pub focus_proc: char,
    pub focus_cpu: char,
    pub focus_net: char,
    pub focus_disk: char,
    pub fullscreen: char,
    pub help: char,
}

impl Default for KeyBindings {
    fn default() -> Self {
        Self {
            focus_proc: 'p',
            focus_cpu: 'c',
            focus_net: 'n',
            focus_disk: 'd',
            fullscreen: 'f',
            help: '?',
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    pub general: GeneralConfig,
    pub layout: LayoutConfig,
    pub process: ProcessConfig,
    pub keybindings: KeyBindings,
}

/// Default config file path: `$XDG_CONFIG_HOME/dreidel/config.toml`, else
/// `~/.config/dreidel/config.toml`, on every platform (including macOS, where
/// `dirs::config_dir()` would point at `~/Library/Application Support`).
pub fn default_config_path() -> std::path::PathBuf {
    config_path_from(std::env::var_os("XDG_CONFIG_HOME"), dirs::home_dir())
}

fn config_path_from(
    xdg_config_home: Option<std::ffi::OsString>,
    home: Option<std::path::PathBuf>,
) -> std::path::PathBuf {
    // Like the XDG spec (and `dirs` on Linux), ignore a relative XDG_CONFIG_HOME.
    xdg_config_home
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| home.map(|h| h.join(".config")))
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("dreidel/config.toml")
}

impl Config {
    /// Load config from [`default_config_path`] (or `path` when given).
    /// Returns default config if the file does not exist.
    pub fn load(path: Option<&std::path::Path>) -> anyhow::Result<Self> {
        use anyhow::Context;
        let default_path = default_config_path();
        let path = path.unwrap_or(&default_path);
        if !path.exists() {
            return Ok(Self::default());
        }
        let contents = std::fs::read_to_string(path)
            .with_context(|| format!("reading config {}", path.display()))?;
        toml::from_str(&contents).with_context(|| format!("parsing config {}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Theme;

    #[test]
    fn config_path_uses_absolute_xdg_config_home() {
        let p = config_path_from(Some("/xdg".into()), Some("/home/u".into()));
        assert_eq!(p, std::path::PathBuf::from("/xdg/dreidel/config.toml"));
    }

    #[test]
    fn config_path_ignores_relative_xdg_config_home() {
        let p = config_path_from(Some("rel".into()), Some("/home/u".into()));
        assert_eq!(
            p,
            std::path::PathBuf::from("/home/u/.config/dreidel/config.toml")
        );
    }

    #[test]
    fn config_path_defaults_to_dot_config_under_home() {
        let p = config_path_from(None, Some("/Users/u".into()));
        assert_eq!(
            p,
            std::path::PathBuf::from("/Users/u/.config/dreidel/config.toml")
        );
    }

    #[test]
    fn parse_duration_accepts_humantime_style_inputs() {
        assert_eq!(parse_duration_ms("2s"), Ok(2000));
        assert_eq!(parse_duration_ms("500ms"), Ok(500));
        assert_eq!(parse_duration_ms("1m 30s"), Ok(90_000));
        assert_eq!(parse_duration_ms("1h30m"), Ok(5_400_000));
        assert_eq!(parse_duration_ms("1min"), Ok(60_000));
        assert_eq!(parse_duration_ms("2secs"), Ok(2000));
        assert_eq!(parse_duration_ms(" 5s "), Ok(5000));
    }

    #[test]
    fn parse_duration_rejects_bad_inputs() {
        assert!(parse_duration_ms("-2s").is_err(), "negative");
        assert!(parse_duration_ms("2").is_err(), "missing unit");
        assert!(parse_duration_ms("fast").is_err(), "not a duration");
        assert!(parse_duration_ms("").is_err(), "empty");
    }

    #[test]
    fn config_duration_fields_parse_from_toml() {
        let c: Config =
            toml::from_str("[general]\nrefresh_rate_ms = \"1500ms\"\nthread_refresh_ms = \"10s\"")
                .expect("valid durations");
        assert_eq!(c.general.refresh_rate_ms, 1500);
        assert_eq!(c.general.thread_refresh_ms, 10_000);
        assert!(toml::from_str::<Config>("[general]\nrefresh_rate_ms = \"-1s\"").is_err());
    }

    #[test]
    fn default_config_is_valid() {
        let c = Config::default();
        assert_eq!(c.general.refresh_rate_ms, 1000);
    }

    #[test]
    fn config_parses_from_toml() {
        let toml_str = r#"
            [general]
            refresh_rate = "2s"
            theme = "dark"
        "#;
        let c: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(c.general.theme, Theme::Dark);
    }
}
