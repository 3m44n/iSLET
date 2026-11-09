//! Configuration schema (`$XDG_CONFIG_HOME/islet/config.toml`) with defaults.

use serde::{Deserialize, Serialize};

/// Behavior when there is no active media or transient popup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum IdleMode {
    /// Keep a compact pill/dot visible at top-center.
    #[default]
    Pill,
    /// Fully hide the pill when nothing is playing.
    Hidden,
}

/// Which monitor the overlay should attach to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum MonitorFollow {
    #[default]
    Focused,
    Primary,
    Output(String),
}

/// User-configurable settings loaded from `config.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct IsletConfig {
    pub margin_top: i32,
    pub idle_mode: IdleMode,
    pub media_collapse_ms: u64,
    pub slider_timeout_ms: u64,
    pub notification_timeout_ms: u64,
    pub hover_expand: bool,
    pub reduce_motion: bool,
    pub visualizer_bars: usize,
    pub monitor_follow: MonitorFollow,
}

impl Default for IsletConfig {
    fn default() -> Self {
        Self {
            margin_top: 8,
            idle_mode: IdleMode::Pill,
            media_collapse_ms: 3500,
            slider_timeout_ms: 1800,
            notification_timeout_ms: 4000,
            hover_expand: true,
            reduce_motion: false,
            visualizer_bars: 16,
            monitor_follow: MonitorFollow::Focused,
        }
    }
}

impl IsletConfig {
    /// Parses a TOML string into `IsletConfig`, falling back to defaults on syntax error.
    #[must_use]
    pub fn from_toml_str(raw: &str) -> Self {
        match toml::from_str::<Self>(raw) {
            Ok(mut cfg) => {
                cfg.visualizer_bars = cfg.visualizer_bars.clamp(12, 24);
                cfg
            }
            Err(err) => {
                tracing::warn!(%err, "Invalid config.toml; falling back to default settings");
                Self::default()
            }
        }
    }
}
