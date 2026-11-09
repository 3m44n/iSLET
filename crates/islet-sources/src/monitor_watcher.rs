//! Event-driven Wayland focused-monitor follower for Hyprland, Sway, and Niri.

use std::path::PathBuf;

use islet_core::{EventSender, IsletEvent};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::UnixStream;
use tracing::{debug, info};

/// Listens to compositor IPC event sockets (`HYPRLAND_INSTANCE_SIGNATURE`) for focused monitor changes.
///
/// If running on a compositor without a supported IPC socket, exits quietly and lets GTK use the
/// configured or primary monitor.
pub async fn run_focused_monitor_watcher(sender: EventSender) {
    if let Some(hypr_sock) = hyprland_event_socket_path() {
        if let Ok(stream) = UnixStream::connect(&hypr_sock).await {
            info!(socket = %hypr_sock.display(), "Connected to Hyprland event socket for multi-monitor tracking");
            let mut lines = BufReader::new(stream).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if let Some(mon_name) = parse_hyprland_focusedmon(&line) {
                    debug!(monitor = mon_name, "Hyprland focused monitor changed");
                    sender.send(IsletEvent::FocusedMonitorChanged(mon_name.to_owned()));
                }
            }
        }
    }
}

fn hyprland_event_socket_path() -> Option<PathBuf> {
    let sig = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok()?;
    if let Ok(runtime) = std::env::var("XDG_RUNTIME_DIR") {
        let p = PathBuf::from(runtime)
            .join("hypr")
            .join(&sig)
            .join(".socket2.sock");
        if p.exists() {
            return Some(p);
        }
    }
    let fallback = PathBuf::from("/tmp/hypr").join(sig).join(".socket2.sock");
    fallback.exists().then_some(fallback)
}

/// Parses Hyprland socket2 event lines of the form `"focusedmon>>DP-1,1"`.
#[must_use]
pub fn parse_hyprland_focusedmon(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("focusedmon>>")?;
    let (mon, _) = rest.split_once(',')?;
    (!mon.is_empty()).then_some(mon.trim())
}
