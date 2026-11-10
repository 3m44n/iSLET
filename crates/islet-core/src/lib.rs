//! Core geometry, configuration, event bus, IPC protocol, and state machine for Islet.

pub mod config;
pub mod event;
pub mod ipc;
pub mod state;

use serde::{Deserialize, Serialize};

pub use config::{IdleMode, IsletConfig, MonitorFollow};
pub use event::{EventBus, EventSender, IsletEvent};
pub use ipc::{socket_path, IpcCommand};
pub use state::{
    PillState, PlaybackStatus, PlayerRegistry, SliderKind, StateMachine, TrackMetadata,
    TransitionEffect,
};

/// Logical dimensions (in application pixels before HiDPI scale) for the pill.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PillDimensions {
    pub width: f64,
    pub height: f64,
}

impl PillDimensions {
    pub const fn new(width: f64, height: f64) -> Self {
        Self { width, height }
    }

    pub const IDLE: Self = Self::new(120.0, 32.0);
    pub const MEDIA_EXPANDED: Self = Self::new(340.0, 76.0);

    #[must_use]
    pub fn lerp(self, target: Self, t: f64) -> Self {
        Self {
            width: (target.width - self.width).mul_add(t, self.width).max(24.0),
            height: (target.height - self.height).mul_add(t, self.height).max(12.0),
        }
    }

    #[must_use]
    pub fn corner_radius(self) -> f64 {
        self.height * 0.5
    }
}

pub const SURFACE_SHADOW_PADDING: i32 = 24;
pub const MAX_SURFACE_WIDTH: i32 = 420 + (SURFACE_SHADOW_PADDING * 2);
pub const MAX_SURFACE_HEIGHT: i32 = 120 + (SURFACE_SHADOW_PADDING * 2);
pub const DEFAULT_MARGIN_TOP: i32 = 8;

#[must_use]
pub fn ease_out_cubic(t: f64) -> f64 {
    let clamped = t.clamp(0.0, 1.0);
    1.0 - (1.0 - clamped).powi(3)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mpris_play_expands_then_collapses_to_compact_media() {
        let mut sm = StateMachine::new(IsletConfig::default());
        assert_eq!(*sm.state(), PillState::Idle);

        let effect = sm.handle_event(IsletEvent::MprisUpdate {
            player_id: "spotify".into(),
            status: PlaybackStatus::Playing,
            metadata: Some(TrackMetadata {
                title: "Strobe".into(),
                artist: "deadmau5".into(),
                ..Default::default()
            }),
        });

        assert_eq!(
            effect.state,
            PillState::Media {
                expanded: true,
                controls_visible: false
            }
        );
        let (seq, timeout) = effect.schedule_collapse.expect("should schedule collapse");
        assert_eq!(timeout, 3500);

        let collapsed = sm.handle_event(IsletEvent::AutoCollapse { seq });
        assert_eq!(
            collapsed.state,
            PillState::Media {
                expanded: false,
                controls_visible: false
            }
        );
    }

    #[test]
    fn volume_slider_overrides_media_and_restores_media_after_timeout() {
        let mut sm = StateMachine::new(IsletConfig::default());

        let media_fx = sm.handle_event(IsletEvent::MprisUpdate {
            player_id: "mpv".into(),
            status: PlaybackStatus::Playing,
            metadata: None,
        });
        let (stale_media_seq, _) = media_fx.schedule_collapse.unwrap();

        let vol_fx = sm.handle_event(IsletEvent::VolumeChanged {
            percent: 65,
            muted: false,
        });
        assert_eq!(
            vol_fx.state,
            PillState::Slider(SliderKind::Volume {
                percent: 65,
                muted: false,
                pulse: false,
            })
        );
        let (vol_seq, _) = vol_fx.schedule_collapse.unwrap();

        let ignored = sm.handle_event(IsletEvent::AutoCollapse {
            seq: stale_media_seq,
        });
        assert!(matches!(ignored.state, PillState::Slider(_)));

        let restored = sm.handle_event(IsletEvent::AutoCollapse { seq: vol_seq });
        assert_eq!(
            restored.state,
            PillState::Media {
                expanded: false,
                controls_visible: false,
            }
        );
    }

    #[test]
    fn ipc_wire_roundtrip() {
        let cmds = [
            IpcCommand::ShowVolume(70),
            IpcCommand::ShowBrightness(85),
            IpcCommand::ShowBattery { percent: 80, charging: true },
            IpcCommand::Toggle,
            IpcCommand::Notify {
                title: "Battery Low".into(),
                body: "15% remaining".into(),
            },
            IpcCommand::Ping,
        ];
        for cmd in cmds {
            let line = cmd.to_wire_line();
            let parsed = IpcCommand::from_wire_line(&line).expect("should parse wire command");
            assert_eq!(parsed, cmd);
        }
    }
}
