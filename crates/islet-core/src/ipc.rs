//! Unix domain socket IPC wire protocol (`$XDG_RUNTIME_DIR/islet.sock`).

use std::path::PathBuf;
use std::str::FromStr;

use crate::event::IsletEvent;

/// Commands sent by `islet-ctl` over `$XDG_RUNTIME_DIR/islet.sock`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IpcCommand {
    ShowVolume(u8),
    ShowBrightness(u8),
    ShowBattery { percent: u8, charging: bool },
    Toggle,
    Notify { title: String, body: String },
    Ping,
}

/// Server response sent back to `islet-ctl` after handling a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpcResponse {
    /// Command accepted and forwarded to the state machine.
    Ok,
    /// Daemon is alive (returned for `PING`).
    Pong,
    /// Command was malformed or unrecognised.
    ErrInvalidCommand,
    /// Daemon is shutting down or the socket is closing.
    ErrShutdown,
}

impl IpcResponse {
    /// Serializes the response into a single newline-terminated wire line.
    #[must_use]
    pub fn to_wire_line(self) -> &'static str {
        match self {
            Self::Ok => "OK\n",
            Self::Pong => "PONG\n",
            Self::ErrInvalidCommand => "ERR invalid command\n",
            Self::ErrShutdown => "ERR shutdown\n",
        }
    }
}

impl FromStr for IpcResponse {
    type Err = ();

    /// Parses a single daemon wire line into an `IpcResponse`.
    fn from_str(line: &str) -> Result<Self, Self::Err> {
        let trimmed = line.trim();
        if trimmed.eq_ignore_ascii_case("OK") {
            return Ok(Self::Ok);
        }
        if trimmed.eq_ignore_ascii_case("PONG") {
            return Ok(Self::Pong);
        }
        if trimmed.starts_with("ERR") {
            if trimmed.to_ascii_lowercase().contains("shutdown") {
                return Ok(Self::ErrShutdown);
            }
            return Ok(Self::ErrInvalidCommand);
        }
        Err(())
    }
}

impl IpcCommand {
    /// Serializes a command into a single newline-terminated wire line.
    #[must_use]
    pub fn to_wire_line(&self) -> String {
        match self {
            Self::ShowVolume(v) => format!("SHOW_VOLUME {v}\n"),
            Self::ShowBrightness(b) => format!("SHOW_BRIGHTNESS {b}\n"),
            Self::ShowBattery { percent, charging } => {
                format!("SHOW_BATTERY {percent} {}\n", i32::from(*charging))
            }
            Self::Toggle => "TOGGLE\n".to_owned(),
            Self::Notify { title, body } => {
                let clean_t = title.replace(['\n', '\r', '\t'], " ");
                let clean_b = body.replace(['\n', '\r', '\t'], " ");
                format!("NOTIFY {clean_t}\t{clean_b}\n")
            }
            Self::Ping => "PING\n".to_owned(),
        }
    }

    /// Parses a single wire line received on `$XDG_RUNTIME_DIR/islet.sock`.
    #[must_use]
    pub fn from_wire_line(line: &str) -> Option<Self> {
        let trimmed = line.trim();
        if trimmed.eq_ignore_ascii_case("TOGGLE") {
            return Some(Self::Toggle);
        }
        if trimmed.eq_ignore_ascii_case("PING") {
            return Some(Self::Ping);
        }
        if let Some(rest) = trimmed.strip_prefix("SHOW_VOLUME ") {
            let val = rest.trim().parse::<u8>().ok()?.min(150);
            return Some(Self::ShowVolume(val));
        }
        if let Some(rest) = trimmed.strip_prefix("SHOW_BRIGHTNESS ") {
            let val = rest.trim().parse::<u8>().ok()?.min(100);
            return Some(Self::ShowBrightness(val));
        }
        if let Some(rest) = trimmed.strip_prefix("SHOW_BATTERY ") {
            return parse_show_battery(rest);
        }
        if let Some(rest) = trimmed.strip_prefix("NOTIFY ") {
            let (title, body) = rest.split_once('\t').unwrap_or((rest, ""));
            return Some(Self::Notify {
                title: title.trim().to_owned(),
                body: body.trim().to_owned(),
            });
        }
        None
    }

    /// Converts an `IpcCommand` into the corresponding `IsletEvent` (if any).
    #[must_use]
    pub fn into_event(self) -> Option<IsletEvent> {
        match self {
            Self::ShowVolume(percent) => Some(IsletEvent::VolumeChanged {
                percent,
                muted: percent == 0,
            }),
            Self::ShowBrightness(percent) => Some(IsletEvent::BrightnessChanged { percent }),
            Self::ShowBattery { percent, charging } => {
                Some(IsletEvent::BatteryChanged { percent, charging })
            }
            Self::Toggle => Some(IsletEvent::ToggleVisibility),
            Self::Notify { title, body } => Some(IsletEvent::Notify { title, body }),
            Self::Ping => None,
        }
    }
}

/// Parses the payload after a `SHOW_BATTERY ` prefix into a `ShowBattery` command.
fn parse_show_battery(rest: &str) -> Option<IpcCommand> {
    let mut parts = rest.split_whitespace();
    let pct = parts.next()?.parse::<u8>().ok()?.min(100);
    let charging = parts
        .next()
        .is_some_and(|s| s == "1" || s.eq_ignore_ascii_case("true") || s.eq_ignore_ascii_case("charging"));
    Some(IpcCommand::ShowBattery { percent: pct, charging })
}

/// Resolves `$XDG_RUNTIME_DIR/islet.sock` (with `/tmp/islet-<uid>.sock` fallback).
#[must_use]
pub fn socket_path() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") {
        PathBuf::from(dir).join("islet.sock")
    } else {
        PathBuf::from("/tmp/islet.sock")
    }
}