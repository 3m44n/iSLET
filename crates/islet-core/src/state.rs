//! Deterministic state machine, active player selector, and transition rules.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::config::{IdleMode, IsletConfig};
use crate::event::IsletEvent;
use crate::PillDimensions;

/// Playback state reported by MPRIS2 players.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PlaybackStatus {
    Playing,
    Paused,
    #[default]
    Stopped,
}

/// Track details and extracted accent color for the Media state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TrackMetadata {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub art_url: Option<String>,
    pub accent_rgb: Option<(u8, u8, u8)>,
    pub duration_ms: Option<u64>,
}

/// Slider overlay variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SliderKind {
    Volume {
        percent: u8,
        muted: bool,
        /// True when triggered by a mute toggle so the UI plays a shake/pulse animation.
        pulse: bool,
    },
    Brightness {
        percent: u8,
    },
}

/// Visual states of the Islet pill.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PillState {
    Hidden,
    Idle,
    Media {
        expanded: bool,
        controls_visible: bool,
    },
    Slider(SliderKind),
    Notification {
        title: String,
        body: String,
    },
}

impl PillState {
    /// Target logical dimensions (`width, height`) for each pill state.
    #[must_use]
    pub const fn dimensions(&self) -> PillDimensions {
        match self {
            Self::Hidden => PillDimensions::new(36.0, 12.0),
            Self::Idle => PillDimensions::IDLE,
            Self::Media {
                expanded: false,
                controls_visible: false,
            } => PillDimensions::new(168.0, 34.0),
            Self::Media {
                expanded: true,
                controls_visible: false,
            } => PillDimensions::MEDIA_EXPANDED,
            Self::Media {
                controls_visible: true,
                ..
            } => PillDimensions::new(360.0, 96.0),
            Self::Slider(_) => PillDimensions::new(260.0, 44.0),
            Self::Notification { .. } => PillDimensions::new(340.0, 72.0),
        }
    }
}

/// Output instruction returned after processing an `IsletEvent`.
#[derive(Debug, Clone, PartialEq)]
pub struct TransitionEffect {
    pub state: PillState,
    pub dimensions: PillDimensions,
    /// When `Some((seq, duration_ms))`, the caller should schedule `IsletEvent::AutoCollapse { seq }`.
    pub schedule_collapse: Option<(u64, u64)>,
}

#[derive(Debug, Clone)]
struct PlayerEntry {
    status: PlaybackStatus,
    metadata: Option<TrackMetadata>,
    last_active_seq: u64,
}

/// Tracks multiple MPRIS players, preferring `Playing` over `Paused` and remembering the last active player.
#[derive(Debug, Default, Clone)]
pub struct PlayerRegistry {
    players: HashMap<String, PlayerEntry>,
    last_active_id: Option<String>,
    activity_counter: u64,
}

impl PlayerRegistry {
    pub fn update_player(
        &mut self,
        player_id: String,
        status: PlaybackStatus,
        metadata: Option<TrackMetadata>,
    ) {
        self.activity_counter += 1;
        let seq = self.activity_counter;

        let entry = self.players.entry(player_id.clone()).or_insert(PlayerEntry {
            status,
            metadata: None,
            last_active_seq: seq,
        });
        entry.status = status;
        if metadata.is_some() {
            entry.metadata = metadata;
        }
        entry.last_active_seq = seq;

        if status == PlaybackStatus::Playing || self.last_active_id.is_none() {
            self.last_active_id = Some(player_id);
        }
    }

    pub fn remove_player(&mut self, player_id: &str) {
        self.players.remove(player_id);
        if self.last_active_id.as_deref() == Some(player_id) {
            self.last_active_id = self.select_active_player().map(|(id, _, _)| id.to_owned());
        }
    }

    /// Selects the highest-priority active player:
    /// 1. Any `Playing` player (most recently updated wins).
    /// 2. The remembered `last_active_id` if it is `Paused`.
    /// 3. Most recently active `Paused` player.
    #[must_use]
    pub fn select_active_player(&self) -> Option<(&str, PlaybackStatus, Option<&TrackMetadata>)> {
        // 1. Prefer Playing players
        if let Some((id, entry)) = self
            .players
            .iter()
            .filter(|(_, e)| e.status == PlaybackStatus::Playing)
            .max_by_key(|(_, e)| e.last_active_seq)
        {
            return Some((id.as_str(), entry.status, entry.metadata.as_ref()));
        }

        // 2. Remembered last active player if Paused
        if let Some(last_id) = &self.last_active_id {
            if let Some(entry) = self.players.get(last_id) {
                if entry.status == PlaybackStatus::Paused {
                    return Some((last_id.as_str(), entry.status, entry.metadata.as_ref()));
                }
            }
        }

        // 3. Any Paused player
        self.players
            .iter()
            .filter(|(_, e)| e.status == PlaybackStatus::Paused)
            .max_by_key(|(_, e)| e.last_active_seq)
            .map(|(id, e)| (id.as_str(), e.status, e.metadata.as_ref()))
    }
}

/// Core Islet state machine.
#[derive(Debug, Clone)]
pub struct StateMachine {
    config: IsletConfig,
    state: PillState,
    /// State to restore after a temporary override (`Slider` or `Notification`) expires.
    restorable_state: PillState,
    players: PlayerRegistry,
    last_muted: Option<bool>,
    hovering: bool,
    collapse_seq: u64,
}

impl StateMachine {
    #[must_use]
    pub fn new(config: IsletConfig) -> Self {
        let initial = Self:: resting_state_for(&config, None);
        Self {
            config,
            state: initial.clone(),
            restorable_state: initial,
            players: PlayerRegistry::default(),
            last_muted: None,
            hovering: false,
            collapse_seq: 0,
        }
    }

    #[must_use]
    pub const fn state(&self) -> &PillState {
        &self.state
    }

    #[must_use]
    pub const fn config(&self) -> &IsletConfig {
        &self.config
    }

    #[must_use]
    pub fn active_player(&self) -> Option<(&str, PlaybackStatus, Option<&TrackMetadata>)> {
        self.players.select_active_player()
    }

    /// Processes an incoming event and returns the new state, target dimensions, and optional timer.
    pub fn handle_event(&mut self, event: IsletEvent) -> TransitionEffect {
        match event {
            IsletEvent::MprisUpdate {
                player_id,
                status,
                metadata,
            } => {
                self.players.update_player(player_id, status, metadata);
                let base_media = PillState::Media {
                    expanded: false,
                    controls_visible: false,
                };
                self.restorable_state = if status == PlaybackStatus::Stopped {
                    Self::resting_state_for(&self.config, None)
                } else {
                    base_media
                };

                // If a Slider is currently overriding the pill, keep the slider on screen
                // and update `restorable_state` so Media restores when the slider finishes.
                if matches!(self.state, PillState::Slider(_)) {
                    return self.effect(None);
                }

                if status == PlaybackStatus::Stopped {
                    self.state = self.restorable_state.clone();
                    self.effect(None)
                } else {
                    self.state = PillState::Media {
                        expanded: true,
                        controls_visible: self.hovering && self.config.hover_expand,
                    };
                    let seq = self.bump_seq();
                    self.effect(Some((seq, self.config.media_collapse_ms)))
                }
            }

            IsletEvent::MprisPlayerRemoved { player_id } => {
                self.players.remove_player(&player_id);
                let active_status = self.players.select_active_player().map(|(_, s, _)| s);
                self.restorable_state = Self::resting_state_for(&self.config, active_status);
                if !matches!(self.state, PillState::Slider(_) | PillState::Notification { .. }) {
                    self.state = self.restorable_state.clone();
                }
                self.effect(None)
            }

            IsletEvent::VolumeChanged { percent, muted } => {
                let pulse = self.last_muted.is_some_and(|prev| prev != muted);
                self.last_muted = Some(muted);

                self.save_restorable_if_not_transient();
                self.state = PillState::Slider(SliderKind::Volume {
                    percent: percent.min(150),
                    muted,
                    pulse,
                });
                let seq = self.bump_seq();
                self.effect(Some((seq, self.config.slider_timeout_ms)))
            }

            IsletEvent::BrightnessChanged { percent } => {
                self.save_restorable_if_not_transient();
                self.state = PillState::Slider(SliderKind::Brightness {
                    percent: percent.min(100),
                });
                let seq = self.bump_seq();
                self.effect(Some((seq, self.config.slider_timeout_ms)))
            }

            IsletEvent::Notify { title, body } => {
                self.save_restorable_if_not_transient();
                self.state = PillState::Notification { title, body };
                let seq = self.bump_seq();
                self.effect(Some((seq, self.config.notification_timeout_ms)))
            }

            IsletEvent::HoverChanged(is_hovering) => {
                self.hovering = is_hovering;
                if !self.config.hover_expand {
                    return self.effect(None);
                }

                if is_hovering {
                    // Expand to show media controls on hover.
                    self.bump_seq();
                    self.state = PillState::Media {
                        expanded: true,
                        controls_visible: true,
                    };
                    self.effect(None)
                } else {
                    let seq = self.bump_seq();
                    self.state = self.restorable_state.clone();
                    self.effect(Some((seq, self.config.media_collapse_ms)))
                }
            }

            IsletEvent::AutoCollapse { seq } => {
                // Ignore stale timers that were superseded by a newer event or active hover.
                if seq != self.collapse_seq || self.hovering {
                    return self.effect(None);
                }
                self.state = self.restorable_state.clone();
                self.effect(None)
            }

            IsletEvent::ToggleVisibility => {
                self.bump_seq();
                if self.state == PillState::Hidden {
                    let active_status = self.players.select_active_player().map(|(_, s, _)| s);
                    self.state = Self::resting_state_for(&self.config, active_status);
                    if self.state == PillState::Hidden {
                        self.state = PillState::Idle;
                    }
                } else {
                    self.state = PillState::Hidden;
                }
                self.effect(None)
            }

            IsletEvent::ConfigReloaded(new_cfg) => {
                self.config = new_cfg;
                let active_status = self.players.select_active_player().map(|(_, s, _)| s);
                self.restorable_state = Self::resting_state_for(&self.config, active_status);
                self.effect(None)
            }

            IsletEvent::SpectrumFrame(_)
            | IsletEvent::PillClicked
            | IsletEvent::PillRightClicked
            | IsletEvent::CssReloaded(_) => self.effect(None),
        }
    }

    fn save_restorable_if_not_transient(&mut self) {
        if !matches!(
            self.state,
            PillState::Slider(_) | PillState::Notification { .. }
        ) {
            // If we were in expanded Media, restore to compact/expanded Media rather than losing Media context.
            self.restorable_state = match &self.state {
                PillState::Media { .. } => PillState::Media {
                    expanded: false,
                    controls_visible: false,
                },
                other => other.clone(),
            };
        }
    }

    fn resting_state_for(config: &IsletConfig, active_status: Option<PlaybackStatus>) -> PillState {
        if matches!(active_status, Some(PlaybackStatus::Playing | PlaybackStatus::Paused)) {
            return PillState::Media {
                expanded: false,
                controls_visible: false,
            };
        }
        match config.idle_mode {
            IdleMode::Pill => PillState::Idle,
            IdleMode::Hidden => PillState::Hidden,
        }
    }

    fn bump_seq(&mut self) -> u64 {
        self.collapse_seq += 1;
        self.collapse_seq
    }

    fn effect(&self, schedule_collapse: Option<(u64, u64)>) -> TransitionEffect {
        TransitionEffect {
            state: self.state.clone(),
            dimensions: self.state.dimensions(),
            schedule_collapse,
        }
    }
}
