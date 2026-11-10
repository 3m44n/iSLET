//! Event-driven battery capacity and charging status watcher (`/sys/class/power_supply/BAT*`).

use std::path::{Path, PathBuf};
use std::time::Duration;

use futures_util::StreamExt;
use inotify::{Inotify, WatchMask};
use islet_core::{EventSender, IsletEvent};
use tracing::{debug, info};

/// Monitors battery percentage and AC charging status.
///
/// Disables itself gracefully on desktop systems without battery devices.
pub async fn run_battery_watcher(sender: EventSender) {
    let Some(bat_dir) = find_battery_device() else {
        info!("No /sys/class/power_supply/BAT* device found; battery watcher disabled gracefully");
        return;
    };

    let capacity_path = bat_dir.join("capacity");
    let status_path = bat_dir.join("status");

    let Ok(inotify) = Inotify::init() else {
        return;
    };

    // Watch status and capacity for changes
    let _ = inotify
        .watches()
        .add(&bat_dir, WatchMask::MODIFY | WatchMask::CLOSE_WRITE | WatchMask::ATTRIB);

    info!(device = %bat_dir.display(), "Battery status watcher online");

    let buffer = [0_u8; 512];
    let Ok(mut stream) = inotify.into_event_stream(buffer) else {
        return;
    };

    let mut last_state = read_battery_state(&capacity_path, &status_path).await;
    let mut poll_interval = tokio::time::interval(Duration::from_secs(30));

    loop {
        tokio::select! {
            _ = stream.next() => {
                if let Some((percent, charging)) = read_battery_state(&capacity_path, &status_path).await {
                    if last_state != Some((percent, charging)) {
                        let prev_charging = last_state.map(|s| s.1);
                        last_state = Some((percent, charging));

                        // Alert on charging status change (e.g. plugged in/unplugged) or critical battery
                        let charging_flipped = prev_charging.is_some_and(|p| p != charging);
                        let is_critical = !charging && (percent <= 20 && percent % 5 == 0);

                        if charging_flipped || is_critical {
                            debug!(percent, charging, "Battery state notification triggered");
                            sender.send(IsletEvent::BatteryChanged { percent, charging });
                        }
                    }
                }
            }
            _ = poll_interval.tick() => {
                if let Some((percent, charging)) = read_battery_state(&capacity_path, &status_path).await {
                    if last_state != Some((percent, charging)) {
                        let prev_charging = last_state.map(|s| s.1);
                        last_state = Some((percent, charging));
                        if prev_charging.is_some_and(|p| p != charging) {
                            sender.send(IsletEvent::BatteryChanged { percent, charging });
                        }
                    }
                }
            }
        }
    }
}

async fn read_battery_state(capacity_path: &Path, status_path: &Path) -> Option<(u8, bool)> {
    let cap_raw = tokio::fs::read_to_string(capacity_path).await.ok()?;
    let percent = cap_raw.trim().parse::<u8>().ok()?.min(100);

    let status_raw = tokio::fs::read_to_string(status_path).await.unwrap_or_default();
    let status_trimmed = status_raw.trim();
    let charging = status_trimmed.eq_ignore_ascii_case("Charging")
        || is_ac_online().await;

    Some((percent, charging))
}

async fn is_ac_online() -> bool {
    // Check any AC/ADP device in /sys/class/power_supply
    let Ok(entries) = std::fs::read_dir(Path::new("/sys/class/power_supply")) else {
        return false;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with("AC") || name_str.starts_with("ADP") {
            let online_file = entry.path().join("online");
            if let Ok(content) = tokio::fs::read_to_string(&online_file).await {
                if content.trim() == "1" {
                    return true;
                }
            }
        }
    }
    false
}

fn find_battery_device() -> Option<PathBuf> {
    let entries = std::fs::read_dir(Path::new("/sys/class/power_supply")).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with("BAT") {
            return Some(entry.path());
        }
    }
    None
}
