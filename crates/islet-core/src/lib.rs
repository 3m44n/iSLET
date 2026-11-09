//! Core geometry, configuration, event bus, and state machine for Islet.

pub mod config;
pub mod event;
pub mod state;

use serde::{Deserialize, Serialize};

pub use config::{IdleMode, IsletConfig, MonitorFollow};
pub use event::{EventBus, EventSender, IsletEvent};
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

    /// Compact idle pill size (`120x32` logical px).
    pub const IDLE: Self = Self::new(120.0, 32.0);

    /// Expanded media HUD size (`340x76` logical px).
    pub const MEDIA_EXPANDED: Self = Self::new(340.0, 76.0);

    /// Linearly interpolate between two dimensions by factor `t`.
    #[must_use]
    pub fn lerp(self, target: Self, t: f64) -> Self {
        Self {
            width: (target.width - self.width).mul_add(t, self.width).max(24.0),
            height: (target.height - self.height).mul_add(t, self.height).max(12.0),
        }
    }

    /// Corner radius for a true capsule/pill (half of current height).
    #[must_use]
    pub fn corner_radius(self) -> f64 {
        self.height * 0.5
    }
}

pub const SURFACE_SHADOW_PADDING: i32 = 24;
pub const MAX_SURFACE_WIDTH: i32 = 420 + (SURFACE_SHADOW_PADDING * 2);
pub const MAX_SURFACE_HEIGHT: i32 = 120 + (SURFACE_SHADOW_PADDING * 2);
pub const DEFAULT_MARGIN_TOP: i32 = 8;

/// Cubic ease-out curve `1 - (1 - t)^3`.
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

        // 1. Start playing media
        let media_fx = sm.handle_event(IsletEvent::MprisUpdate {
            player_id: "mpv".into(),
            status: PlaybackStatus::Playing,
            metadata: None,
        });
        let (stale_media_seq, _) = media_fx.schedule_collapse.unwrap();

        // 2. Volume event arrives mid-media
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

        // 3. Stale media timer firing during slider must be ignored
        let ignored = sm.handle_event(IsletEvent::AutoCollapse {
            seq: stale_media_seq,
        });
        assert!(matches!(ignored.state, PillState::Slider(_)));

        // 4. Slider timeout restores compact Media state
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
    fn mute_toggle_sets_pulse_flag() {
        let mut sm = StateMachine::new(IsletConfig::default());
        sm.handle_event(IsletEvent::VolumeChanged {
            percent: 50,
            muted: false,
        });
        let mute_fx = sm.handle_event(IsletEvent::VolumeChanged {
            percent: 50,
            muted: true,
        });
        assert_eq!(
            mute_fx.state,
            PillState::Slider(SliderKind::Volume {
                percent: 50,
                muted: true,
                pulse: true,
            })
        );
    }

    #[test]
    fn player_registry_prefers_playing_and_remembers_last_active() {
        let mut reg = PlayerRegistry::default();
        reg.update_player("firefox".into(), PlaybackStatus::Playing, None);
        reg.update_player("spotify".into(), PlaybackStatus::Paused, None);

        // Playing wins over Paused even if Paused updated later
        let (active, status, _) = reg.select_active_player().unwrap();
        assert_eq!(active, "firefox");
        assert_eq!(status, PlaybackStatus::Playing);

        // When firefox pauses, firefox remains remembered as last active Playing player
        reg.update_player("firefox".into(), PlaybackStatus::Paused, None);
        let (active_after, _, _) = reg.select_active_player().unwrap();
        assert_eq!(active_after, "firefox");
    }

    #[test]
    fn config_parsing_and_fallback() {
        let custom = r#"
            margin_top = 14
            idle_mode = "hidden"
            media_collapse_ms = 5000
            reduce_motion = true
            visualizer_bars = 30
        "#;
        let cfg = IsletConfig::from_toml_str(custom);
        assert_eq!(cfg.margin_top, 14);
        assert_eq!(cfg.idle_mode, IdleMode::Hidden);
        assert_eq!(cfg.visualizer_bars, 24); // clamped to 12..=24

        let broken = IsletConfig::from_toml_str("not valid [[[ toml");
        assert_eq!(broken, IsletConfig::default());
    }
}
