//! Event-driven PipeWire / PulseAudio volume watcher (`pactl subscribe` + `wpctl get-volume`).
//!
//! Uses zero polling loops: blocks asynchronously on `pactl subscribe` stdout lines and
//! queries `wpctl` (or `pactl` fallback) only when a sink/server event arrives.

use islet_core::{EventSender, IsletEvent};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tracing::{debug, info, warn};

use std::process::Stdio;

/// Spawns the event-driven audio volume watcher.
///
/// Never crashes the daemon if PipeWire/PulseAudio is missing; logs a warning and exits cleanly.
pub async fn run_audio_watcher(sender: EventSender) {
    let Ok(mut child) = Command::new("pactl")
        .arg("subscribe")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        warn!("`pactl` not found or audio server unavailable; volume watcher disabled gracefully");
        return;
    };

    let Some(stdout) = child.stdout.take() else {
        return;
    };

    info!("Audio volume subscriber online (event-driven via PipeWire/PulseAudio)");
    let mut lines = BufReader::new(stdout).lines();
    let mut last_state: Option<(u8, bool)> = query_current_volume().await;

    while let Ok(Some(line)) = lines.next_line().await {
        // React only to sink or server change events (`Event 'change' on sink #...`).
        if !line.contains("'change'") || (!line.contains("sink") && !line.contains("server")) {
            continue;
        }

        if let Some((percent, muted)) = query_current_volume().await {
            if last_state != Some((percent, muted)) {
                last_state = Some((percent, muted));
                debug!(percent, muted, "Audio volume changed");
                sender.send(IsletEvent::VolumeChanged { percent, muted });
            }
        }
    }
}

async fn query_current_volume() -> Option<(u8, bool)> {
    // 1. Try WirePlumber `wpctl get-volume @DEFAULT_AUDIO_SINK@` (fast, native PipeWire).
    if let Ok(output) = Command::new("wpctl")
        .args(["get-volume", "@DEFAULT_AUDIO_SINK@"])
        .output()
        .await
    {
        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout);
            if let Some(parsed) = parse_wpctl_volume(&text) {
                return Some(parsed);
            }
        }
    }

    // 2. Fallback to `pactl get-sink-volume` + `pactl get-sink-mute`.
    let vol_out = Command::new("pactl")
        .args(["get-sink-volume", "@DEFAULT_SINK@"])
        .output()
        .await
        .ok()?;
    let mute_out = Command::new("pactl")
        .args(["get-sink-mute", "@DEFAULT_SINK@"])
        .output()
        .await
        .ok()?;

    let vol_str = String::from_utf8_lossy(&vol_out.stdout);
    let mute_str = String::from_utf8_lossy(&mute_out.stdout);
    parse_pactl_volume_and_mute(&vol_str, &mute_str)
}

/// Parses WirePlumber output such as `"Volume: 0.72"` or `"Volume: 0.45 [MUTED]"`.
#[must_use]
pub fn parse_wpctl_volume(output: &str) -> Option<(u8, bool)> {
    let muted = output.contains("[MUTED]");
    let number_str = output
        .split_whitespace()
        .find(|tok| tok.chars().next().is_some_and(|c| c.is_ascii_digit()))?;
    let ratio: f64 = number_str.parse().ok()?;
    let percent = (ratio * 100.0).round().clamp(0.0, 150.0) as u8;
    Some((percent, muted))
}

/// Parses `pactl get-sink-volume` and `pactl get-sink-mute` output strings.
#[must_use]
pub fn parse_pactl_volume_and_mute(vol_out: &str, mute_out: &str) -> Option<(u8, bool)> {
    let percent_token = vol_out
        .split_whitespace()
        .find(|tok| tok.ends_with('%'))?
        .trim_end_matches('%');
    let percent = percent_token.parse::<u16>().ok()?.min(150) as u8;
    let muted = mute_out.to_ascii_lowercase().contains("yes");
    Some((percent, muted))
}
