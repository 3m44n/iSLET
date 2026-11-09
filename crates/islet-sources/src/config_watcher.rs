//! Hot-reload watcher (`inotify`) for `$XDG_CONFIG_HOME/islet/config.toml` and `style.css`.

use std::path::{Path, PathBuf};

use futures_util::StreamExt;
use inotify::{Inotify, WatchMask};
use islet_core::{EventSender, IsletConfig, IsletEvent};
use tracing::{info, warn};

const DEFAULT_CONFIG_TOML: &str = r#"# Islet Configuration (`$XDG_CONFIG_HOME/islet/config.toml`)
# Changes to this file and `style.css` are hot-reloaded automatically!

margin_top = 8
idle_mode = "pill"          # "pill" or "hidden"
media_collapse_ms = 3500
slider_timeout_ms = 1800
notification_timeout_ms = 4000
hover_expand = true
reduce_motion = false
visualizer_bars = 16        # 12..=24
monitor_follow = "focused"  # "focused", "primary", or { output = "DP-1" }
"#;

/// Watches `$XDG_CONFIG_HOME/islet/` for changes to `config.toml` and `style.css` with zero polling.
pub async fn run_config_and_css_watcher(sender: EventSender) {
    let dir = config_directory();
    if let Err(err) = tokio::fs::create_dir_all(&dir).await {
        warn!(%err, "Failed to create config directory; hot-reload disabled");
        return;
    }

    let config_path = dir.join("config.toml");
    let css_path = dir.join("style.css");

    // Seed default config.toml if absent so the user can edit it right away.
    if !config_path.exists() {
        let _ = tokio::fs::write(&config_path, DEFAULT_CONFIG_TOML).await;
    }

    // Load initial config and custom CSS if present.
    if let Ok(raw) = tokio::fs::read_to_string(&config_path).await {
        sender.send(IsletEvent::ConfigReloaded(IsletConfig::from_toml_str(&raw)));
    }
    if let Ok(css) = tokio::fs::read_to_string(&css_path).await {
        if !css.trim().is_empty() {
            sender.send(IsletEvent::CssReloaded(css));
        }
    }

    let Ok(inotify) = Inotify::init() else {
        warn!("Failed to initialize inotify for config hot-reload");
        return;
    };

    if let Err(err) = inotify.watches().add(
        &dir,
        WatchMask::CLOSE_WRITE | WatchMask::MOVED_TO | WatchMask::CREATE,
    ) {
        warn!(%err, path = %dir.display(), "Failed to watch config directory");
        return;
    }

    info!(dir = %dir.display(), "Config and CSS hot-reload watcher online");

    let buffer = [0_u8; 1024];
    let Ok(mut stream) = inotify.into_event_stream(buffer) else {
        return;
    };

    while let Some(Ok(event)) = stream.next().await {
        let Some(name) = event.name.and_then(|n| n.to_str().map(str::to_owned)) else {
            continue;
        };

        if name == "config.toml" {
            if let Ok(raw) = tokio::fs::read_to_string(&config_path).await {
                let cfg = IsletConfig::from_toml_str(&raw);
                info!("Hot-reloaded config.toml");
                sender.send(IsletEvent::ConfigReloaded(cfg));
            }
        } else if name == "style.css" {
            if let Ok(css) = tokio::fs::read_to_string(&css_path).await {
                info!("Hot-reloaded style.css");
                sender.send(IsletEvent::CssReloaded(css));
            }
        }
    }
}

#[must_use]
pub fn config_directory() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        PathBuf::from(xdg).join("islet")
    } else if let Ok(home) = std::env::var("HOME") {
        Path::new(&home).join(".config").join("islet")
    } else {
        PathBuf::from("/tmp/islet-config")
    }
}
