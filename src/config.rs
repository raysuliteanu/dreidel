// SPDX-License-Identifier: GPL-3.0-only

use crate::theme::Theme;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct GeneralConfig {
    #[serde(deserialize_with = "deserialize_duration", default = "default_refresh")]
    pub refresh_rate_ms: u64,
    pub theme: Theme,
    pub channel_capacity: usize,
}

fn deserialize_duration<'de, D: serde::Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
    let s = String::deserialize(d)?;
    humantime::parse_duration(&s)
        .map(|d| d.as_millis() as u64)
        .map_err(serde::de::Error::custom)
}

// TODO: address magic number; it's duplicated below in default()
fn default_refresh() -> u64 {
    1000
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            refresh_rate_ms: 1000,
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

impl Config {
    /// Load config from XDG path (~/.config/dreidel/config.toml).
    /// Returns default config if the file does not exist.
    pub fn load(path: Option<&std::path::Path>) -> anyhow::Result<Self> {
        use anyhow::Context;
        let default_path = dirs::config_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("dreidel/config.toml");
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
