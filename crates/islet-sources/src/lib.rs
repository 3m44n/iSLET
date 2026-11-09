//! Event-driven background sources for Islet (MPRIS2, Audio, Backlight, Visualizer, Config/CSS, Multi-Monitor).

pub mod art;
pub mod audio;
pub mod backlight;
pub mod config_watcher;
pub mod monitor_watcher;
pub mod mpris;
pub mod visualizer;

use islet_core::EventSender;
use tokio::sync::{mpsc, watch};

pub use mpris::MprisCommand;
pub use visualizer::VisualizerControl;

/// Handles returned to the UI layer for controlling MPRIS players and pausing/resuming the FFT visualizer.
pub struct SourceHandles {
    pub mpris_tx: mpsc::UnboundedSender<MprisCommand>,
    pub visualizer_tx: watch::Sender<VisualizerControl>,
}

/// Starts all background hardware/D-Bus/FFT/inotify listeners on a dedicated Tokio runtime thread.
#[must_use]
pub fn spawn_background_sources(sender: EventSender) -> SourceHandles {
    let (mpris_tx, mpris_rx) = mpsc::unbounded_channel();
    let (visualizer_tx, visualizer_rx) = watch::channel(VisualizerControl::default());

    std::thread::Builder::new()
        .name("islet-sources-rt".into())
        .spawn(move || {
            let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                tracing::error!("Failed to build Tokio runtime for islet-sources");
                return;
            };

            rt.block_on(async move {
                let mpris_sender = sender.clone();
                let audio_sender = sender.clone();
                let bright_sender = sender.clone();
                let viz_sender = sender.clone();
                let cfg_sender = sender.clone();
                let mon_sender = sender;

                tokio::join!(
                    mpris::run_mpris_watcher(mpris_sender, mpris_rx),
                    audio::run_audio_watcher(audio_sender),
                    backlight::run_backlight_watcher(bright_sender),
                    visualizer::run_visualizer(viz_sender, visualizer_rx),
                    config_watcher::run_config_and_css_watcher(cfg_sender),
                    monitor_watcher::run_focused_monitor_watcher(mon_sender),
                );
            });
        })
        .ok();

    SourceHandles {
        mpris_tx,
        visualizer_tx,
    }
}

#[cfg(test)]
mod tests {
    use super::art::percent_decode;
    use super::audio::{parse_pactl_volume_and_mute, parse_wpctl_volume};
    use super::backlight::parse_brightness_percent;
    use super::monitor_watcher::parse_hyprland_focusedmon;
    use super::mpris::{parse_mpris_metadata_value, parse_playback_status};
    use super::visualizer::{compute_fft_bars, procedural_spectrum_frame};
    use islet_core::PlaybackStatus;
    use rustfft::FftPlanner;
    use std::collections::HashMap;
    use std::f32::consts::PI;
    use zbus::zvariant::{OwnedValue, Value};

    #[test]
    fn test_mpris_playback_status_parsing() {
        assert_eq!(parse_playback_status("Playing"), PlaybackStatus::Playing);
        assert_eq!(parse_playback_status("Paused"), PlaybackStatus::Paused);
        assert_eq!(parse_playback_status("Stopped"), PlaybackStatus::Stopped);
    }

    #[test]
    fn test_mpris_metadata_dictionary_parsing() {
        let mut map: HashMap<&str, Value<'_>> = HashMap::new();
        map.insert("xesam:title", Value::from("Veridis Quo"));
        map.insert("xesam:artist", Value::from(vec!["Daft Punk"]));
        map.insert("xesam:album", Value::from("Discovery"));
        map.insert(
            "mpris:artUrl",
            Value::from("file:///home/user/Music/cover%20art.jpg"),
        );
        map.insert("mpris:length", Value::I64(345_000_000));

        let owned: OwnedValue = Value::from(map).try_into().unwrap();
        let meta = parse_mpris_metadata_value(&owned).expect("metadata should parse");

        assert_eq!(meta.title, "Veridis Quo");
        assert_eq!(meta.artist, "Daft Punk");
        assert_eq!(meta.album, "Discovery");
        assert_eq!(meta.duration_ms, Some(345_000));
        assert_eq!(
            percent_decode("file:///home/user/Music/cover%20art.jpg"),
            "file:///home/user/Music/cover art.jpg"
        );
    }

    #[test]
    fn test_wpctl_and_pactl_volume_parsing() {
        assert_eq!(parse_wpctl_volume("Volume: 0.75
"), Some((75, false)));
        assert_eq!(
            parse_wpctl_volume("Volume: 0.30 [MUTED]
"),
            Some((30, true))
        );

        let pactl_vol = "Volume: front-left: 42598 /  65% / -11.22 dB";
        assert_eq!(
            parse_pactl_volume_and_mute(pactl_vol, "Mute: no"),
            Some((65, false))
        );
        assert_eq!(
            parse_pactl_volume_and_mute(pactl_vol, "Mute: yes"),
            Some((65, true))
        );
    }

    #[test]
    fn test_backlight_percent_calculation() {
        assert_eq!(parse_brightness_percent("120
", 240), Some(50));
        assert_eq!(parse_brightness_percent("240", 240), Some(100));
        assert_eq!(parse_brightness_percent("0", 240), Some(0));
        assert_eq!(parse_brightness_percent("100", 0), None);
    }

    #[test]
    fn test_fft_spectrum_downsampling_and_procedural_fallback() {
        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(1024);

        let samples: Vec<f32> = (0..1024)
            .map(|i| (2.0 * PI * 440.0 * (i as f32) / 48_000.0).sin())
            .collect();

        let bars = compute_fft_bars(&samples, fft.as_ref(), 16);
        assert_eq!(bars.len(), 16);
        assert!(bars.iter().any(|&v| v > 0.25));
        assert!(bars.iter().all(|&v| (0.0..=1.0).contains(&v)));

        let fallback = procedural_spectrum_frame(1.2, 16);
        assert_eq!(fallback.len(), 16);
        assert!(fallback.iter().all(|&v| (0.0..=1.0).contains(&v)));
    }

    #[test]
    fn test_hyprland_focusedmon_parsing() {
        assert_eq!(parse_hyprland_focusedmon("focusedmon>>DP-1,2"), Some("DP-1"));
        assert_eq!(parse_hyprland_focusedmon("workspace>>2"), None);
    }
}
