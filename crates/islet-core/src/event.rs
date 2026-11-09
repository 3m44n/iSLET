//! Unified event bus connecting background sources, UI gestures, and IPC commands.

use tokio::sync::mpsc;

use crate::config::IsletConfig;
use crate::state::{PlaybackStatus, TrackMetadata};

/// All events that drive the Islet state machine and UI layer.
#[derive(Debug, Clone, PartialEq)]
pub enum IsletEvent {
    /// MPRIS player updated metadata or playback status.
    MprisUpdate {
        player_id: String,
        status: PlaybackStatus,
        metadata: Option<TrackMetadata>,
    },
    /// An MPRIS player disappeared from D-Bus.
    MprisPlayerRemoved {
        player_id: String,
    },
    /// Audio volume or mute state changed (0..=150%).
    VolumeChanged {
        percent: u8,
        muted: bool,
    },
    /// Backlight brightness changed (0..=100%).
    BrightnessChanged {
        percent: u8,
    },
    /// New FFT spectrum bars (normalized 0.0..=1.0).
    SpectrumFrame(Vec<f32>),
    /// Transient notification from IPC (`islet-ctl notify "Title" "Body"`).
    Notify {
        title: String,
        body: String,
    },
    /// Pointer entered or left the pill input region.
    HoverChanged(bool),
    /// Left click on pill (toggle play/pause).
    PillClicked,
    /// Right click on pill (raise active player window).
    PillRightClicked,
    /// Toggle visibility (`islet-ctl toggle`).
    ToggleVisibility,
    /// Sequence-tagged auto-collapse timer expired.
    AutoCollapse {
        seq: u64,
    },
    /// Focused Wayland monitor connector changed (e.g. `"DP-1"`, `"eDP-1"`).
    FocusedMonitorChanged(String),
    /// Hot-reloaded `config.toml`.
    ConfigReloaded(IsletConfig),
    /// Hot-reloaded `style.css`.
    CssReloaded(String),
}

/// Cloneable handle for pushing events into the central bus from any thread/task.
#[derive(Debug, Clone)]
pub struct EventSender {
    tx: mpsc::UnboundedSender<IsletEvent>,
}

impl EventSender {
    /// Sends an event to the state machine loop without blocking.
    pub fn send(&self, event: IsletEvent) -> bool {
        self.tx.send(event).is_ok()
    }
}

/// Central event bus pairing a cloneable `EventSender` with the consumer receiver.
pub struct EventBus {
    pub sender: EventSender,
    pub receiver: mpsc::UnboundedReceiver<IsletEvent>,
}

impl EventBus {
    #[must_use]
    pub fn new() -> Self {
        let (tx, receiver) = mpsc::unbounded_channel();
        Self {
            sender: EventSender { tx },
            receiver,
        }
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}
