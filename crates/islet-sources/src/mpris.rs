//! Event-driven MPRIS2 D-Bus subscriber and media player controller via `zbus`.

use std::collections::HashMap;

use futures_util::StreamExt;
use islet_core::{EventSender, IsletEvent, PlaybackStatus, TrackMetadata};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};
use zbus::fdo::{DBusProxy, PropertiesProxy};
use zbus::names::{BusName, InterfaceName};
use zbus::zvariant::{Optional, OwnedValue, Value};
use zbus::Connection;

use crate::art::resolve_art_and_accent;

const MPRIS_PREFIX: &str = "org.mpris.MediaPlayer2.";
const MPRIS_PATH: &str = "/org/mpris/MediaPlayer2";
const PLAYER_IFACE: &str = "org.mpris.MediaPlayer2.Player";
const ROOT_IFACE: &str = "org.mpris.MediaPlayer2";

/// Outbound media control commands triggered by clicking or interacting with the pill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MprisCommand {
    PlayPause(String),
    Next(String),
    Previous(String),
    Raise(String),
    /// Seeks by a relative offset in microseconds (`org.mpris.MediaPlayer2.Player.Seek`).
    SeekRelativeUs(String, i64),
}

pub async fn run_mpris_watcher(
    sender: EventSender,
    mut cmd_rx: mpsc::UnboundedReceiver<MprisCommand>,
) {
    let conn = match Connection::session().await {
        Ok(c) => c,
        Err(err) => {
            warn!(%err, "Session D-Bus unavailable; MPRIS module disabled gracefully");
            return;
        }
    };

    info!("MPRIS2 D-Bus source connected");

    if let Ok(dbus) = DBusProxy::new(&conn).await {
        if let Ok(names) = dbus.list_names().await {
            for name in names {
                if name.as_str().starts_with(MPRIS_PREFIX) {
                    spawn_player_listener(conn.clone(), name.to_string(), sender.clone());
                }
            }
        }

        if let Ok(mut name_stream) = dbus.receive_name_owner_changed().await {
            let conn_for_names = conn.clone();
            let sender_for_names = sender.clone();
            tokio::spawn(async move {
                while let Some(signal) = name_stream.next().await {
                    let Ok(args) = signal.args() else { continue };
                    let bus_name = args.name.as_str();
                    if !bus_name.starts_with(MPRIS_PREFIX) {
                        continue;
                    }

                    match (args.old_owner.is_none(), args.new_owner.is_none()) {
                        (true, false) => {
                            debug!(bus_name, "New MPRIS player appeared on D-Bus");
                            spawn_player_listener(
                                conn_for_names.clone(),
                                bus_name.to_owned(),
                                sender_for_names.clone(),
                            );
                        }
                        (false, true) => {
                            debug!(bus_name, "MPRIS player left D-Bus");
                            sender_for_names.send(IsletEvent::MprisPlayerRemoved {
                                player_id: bus_name.to_owned(),
                            });
                        }
                        _ => {}
                    }
                }
            });
        }
    }

    while let Some(cmd) = cmd_rx.recv().await {
        dispatch_command(&conn, cmd).await;
    }
}

fn spawn_player_listener(conn: Connection, bus_name: String, sender: EventSender) {
    tokio::spawn(async move {
        let Ok(well_known) = BusName::try_from(bus_name.as_str()) else {
            return;
        };
        let Ok(builder) = PropertiesProxy::builder(&conn).destination(well_known) else {
            return;
        };
        let Ok(builder) = builder.path(MPRIS_PATH) else {
            return;
        };
        let Ok(props) = builder.build().await else {
            return;
        };

        if let Ok(iface) = InterfaceName::try_from(PLAYER_IFACE) {
            if let Ok(all) = props.get_all(Optional::from(Some(iface))).await {
                emit_parsed_properties(&bus_name, &all, &sender).await;
            }
        }

        let Ok(mut stream) = props.receive_properties_changed().await else {
            return;
        };
        while let Some(signal) = stream.next().await {
            if let Ok(args) = signal.args() {
                if args.interface_name.as_str() == PLAYER_IFACE {
                    if let Ok(iface) = InterfaceName::try_from(PLAYER_IFACE) {
                        if let Ok(all) = props.get_all(Optional::from(Some(iface))).await {
                            emit_parsed_properties(&bus_name, &all, &sender).await;
                        }
                    }
                }
            }
        }
    });
}

async fn emit_parsed_properties(
    player_id: &str,
    props: &HashMap<String, OwnedValue>,
    sender: &EventSender,
) {
    let status = props
        .get("PlaybackStatus")
        .and_then(|v| <&str>::try_from(v).ok())
        .map_or(PlaybackStatus::Stopped, parse_playback_status);

    let position_ms = props
        .get("Position")
        .and_then(|v| i64::try_from(v).ok())
        .filter(|&us| us >= 0)
        .map(|us| (us as u64) / 1000);

    let mut metadata = props.get("Metadata").and_then(parse_mpris_metadata_value);

    if let Some(ref mut meta) = metadata {
        meta.position_ms = position_ms;
        if let Some(ref raw_url) = meta.art_url.clone() {
            let (cached_path, accent) = resolve_art_and_accent(raw_url).await;
            if cached_path.is_some() {
                meta.art_url = cached_path;
            }
            meta.accent_rgb = accent;
        }
    }

    sender.send(IsletEvent::MprisUpdate {
        player_id: player_id.to_owned(),
        status,
        metadata,
    });
}

async fn dispatch_command(conn: &Connection, cmd: MprisCommand) {
    match cmd {
        MprisCommand::PlayPause(id) => {
            let _ = conn
                .call_method(Some(id.as_str()), MPRIS_PATH, Some(PLAYER_IFACE), "PlayPause", &())
                .await;
        }
        MprisCommand::Next(id) => {
            let _ = conn
                .call_method(Some(id.as_str()), MPRIS_PATH, Some(PLAYER_IFACE), "Next", &())
                .await;
        }
        MprisCommand::Previous(id) => {
            let _ = conn
                .call_method(Some(id.as_str()), MPRIS_PATH, Some(PLAYER_IFACE), "Previous", &())
                .await;
        }
        MprisCommand::Raise(id) => {
            let _ = conn
                .call_method(Some(id.as_str()), MPRIS_PATH, Some(ROOT_IFACE), "Raise", &())
                .await;
        }
        MprisCommand::SeekRelativeUs(id, offset_us) => {
            if let Err(err) = conn
                .call_method(
                    Some(id.as_str()),
                    MPRIS_PATH,
                    Some(PLAYER_IFACE),
                    "Seek",
                    &(offset_us,),
                )
                .await
            {
                debug!(%err, "MPRIS Seek call failed");
            }
        }
    }
}

#[must_use]
pub fn parse_playback_status(raw: &str) -> PlaybackStatus {
    match raw.trim() {
        "Playing" => PlaybackStatus::Playing,
        "Paused" => PlaybackStatus::Paused,
        _ => PlaybackStatus::Stopped,
    }
}

#[must_use]
pub fn parse_mpris_metadata_value(val: &OwnedValue) -> Option<TrackMetadata> {
    let Value::Dict(dict) = val.downcast_ref::<Value>().ok()? else {
        return None;
    };

    let title = dict
        .get::<&str, &str>(&"xesam:title")
        .ok()
        .flatten()
        .unwrap_or("Unknown Track")
        .to_owned();

    let artist = dict
        .get::<&str, Value>(&"xesam:artist")
        .ok()
        .flatten()
        .and_then(|v| match v {
            Value::Array(arr) => arr
                .iter()
                .filter_map(|item| <&str>::try_from(item).ok())
                .collect::<Vec<_>>()
                .join(", ")
                .into(),
            Value::Str(s) => Some(s.to_string()),
            _ => None,
        })
        .unwrap_or_default();

    let album = dict
        .get::<&str, &str>(&"xesam:album")
        .ok()
        .flatten()
        .unwrap_or_default()
        .to_owned();

    let art_url = dict
        .get::<&str, &str>(&"mpris:artUrl")
        .ok()
        .flatten()
        .map(str::to_owned);

    let duration_ms = dict
        .get::<&str, i64>(&"mpris:length")
        .ok()
        .flatten()
        .filter(|&us| us > 0)
        .map(|us| (us as u64) / 1000)
        .or_else(|| {
            dict.get::<&str, u64>(&"mpris:length")
                .ok()
                .flatten()
                .map(|us| us / 1000)
        });

    Some(TrackMetadata {
        title,
        artist,
        album,
        art_url,
        accent_rgb: None,
        duration_ms,
        position_ms: None,
    })
}
