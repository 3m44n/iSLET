//! Event-driven backlight brightness watcher using `inotify` on `/sys/class/backlight/*`.

use std::path::{Path, PathBuf};

use futures_util::StreamExt;
use inotify::{Inotify, WatchMask};
use islet_core::{EventSender, IsletEvent};
use tracing::{debug, info, warn};

/// Watches `/sys/class/backlight/*` via kernel `inotify` events without polling.
///
/// Gracefully disables itself on desktop machines with no backlight sysfs directory.
pub async fn run_backlight_watcher(sender: EventSender) {
    let Some(device_dir) = find_first_backlight_device() else {
        info!("No /sys/class/backlight device found; brightness watcher disabled gracefully");
        return;
    };

    let max_path = device_dir.join("max_brightness");
    let Ok(max_raw) = tokio::fs::read_to_string(&max_path).await else {
        warn!(path = %max_path.display(), "Cannot read max_brightness; brightness watcher disabled");
        return;
    };
    let Some(max_val) = max_raw.trim().parse::<u64>().ok().filter(|&m| m > 0) else {
        return;
    };

    let actual_path = device_dir.join("actual_brightness");
    let brightness_path = if actual_path.exists() {
        actual_path
    } else {
        device_dir.join("brightness")
    };

    let Ok(inotify) = Inotify::init() else {
        warn!("Failed to initialize inotify for backlight");
        return;
    };

    if let Err(err) = inotify
        .watches()
        .add(&brightness_path, WatchMask::MODIFY | WatchMask::CLOSE_WRITE)
    {
        warn!(%err, path = %brightness_path.display(), "Failed to add inotify watch on backlight");
        return;
    }

    info!(device = %device_dir.display(), "Backlight inotify watcher online");

    let buffer = [0_u8; 512];
    let Ok(mut stream) = inotify.into_event_stream(buffer) else {
        return;
    };

    let mut last_percent: Option<u8> = None;
    while stream.next().await.is_some() {
        if let Ok(raw) = tokio::fs::read_to_string(&brightness_path).await {
            if let Some(pct) = parse_brightness_percent(&raw, max_val) {
                if last_percent != Some(pct) {
                    last_percent = Some(pct);
                    debug!(percent = pct, "Backlight brightness changed");
                    sender.send(IsletEvent::BrightnessChanged { percent: pct });
                }
            }
        }
    }
}

fn find_first_backlight_device() -> Option<PathBuf> {
    let entries = std::fs::read_dir(Path::new("/sys/class/backlight")).ok()?;
    entries.flatten().map(|e| e.path()).next()
}

/// Converts a raw sysfs brightness value string and `max_brightness` into `0..=100%`.
#[must_use]
pub fn parse_brightness_percent(current_raw: &str, max_brightness: u64) -> Option<u8> {
    if max_brightness == 0 {
        return None;
    }
    let current: u64 = current_raw.trim().parse().ok()?;
    let pct = ((current as f64 / max_brightness as f64) * 100.0)
        .round()
        .clamp(0.0, 100.0) as u8;
    Some(pct)
}
