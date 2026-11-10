//! PipeWire default sink monitor capture, `rustfft` spectrum analyzer, and procedural fallback.
//!
//! Automatically suspends capture and FFT timers whenever nothing is playing or the pill is hidden
//! so idle CPU usage stays at 0%.

use std::f32::consts::PI;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use islet_core::{EventSender, IsletEvent};
use rustfft::num_complex::Complex;
use rustfft::FftPlanner;
use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};
use tokio::sync::watch;
use tracing::{debug, info, warn};

const FFT_SIZE: usize = 1024;
const DEFAULT_BARS: usize = 16;

/// Control state for starting/stopping the visualizer pipeline with zero idle wakeups.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisualizerControl {
    pub active: bool,
    pub bars: usize,
}

impl Default for VisualizerControl {
    fn default() -> Self {
        Self {
            active: false,
            bars: DEFAULT_BARS,
        }
    }
}

/// Runs the demand-driven audio visualizer task.
///
/// Sleeps completely (`changed().await`) when `active == false`. When `active == true`,
/// streams PCM from PipeWire's default sink monitor (`pw-cat` / `parec`), computes a
/// Hann-windowed 1024-point FFT via `rustfft`, and falls back to a procedural harmonic
/// waveform if monitor capture is unavailable.
pub async fn run_visualizer(
    sender: EventSender,
    mut ctrl_rx: watch::Receiver<VisualizerControl>,
) {
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(FFT_SIZE);

    loop {
        // 1. Sleep with zero CPU usage while paused, stopped, or hidden.
        while !ctrl_rx.borrow().active {
            if ctrl_rx.changed().await.is_err() {
                return;
            }
        }

        let bars = ctrl_rx.borrow().bars.clamp(12, 24);
        info!(bars, "Visualizer activated (playback active and pill visible)");

        // 2. Try spawning PipeWire / PulseAudio monitor capture process.
        if let Some(mut child) = spawn_monitor_capture() {
            debug!("Capturing live PCM from PipeWire default sink monitor");
            run_live_fft_capture(&mut child, &fft, &sender, &mut ctrl_rx).await;
            let _ = child.kill().await;
        } else {
            warn!("PipeWire monitor capture unavailable; using procedural animated waveform fallback");
            run_procedural_fallback(&sender, &mut ctrl_rx).await;
        }

        // Send a zeroed frame on stop so the UI bars decay cleanly to baseline.
        let bars_now = ctrl_rx.borrow().bars.clamp(12, 24);
        sender.send(IsletEvent::SpectrumFrame(vec![0.0; bars_now]));
        debug!("Visualizer suspended (zero idle CPU)");
    }
}

fn spawn_monitor_capture() -> Option<Child> {
    // 1. Try PulseAudio/PipeWire-pulse `parec` targeting `@DEFAULT_MONITOR@`
    if let Ok(child) = Command::new("parec")
        .args([
            "--format=float32le",
            "--rate=48000",
            "--channels=1",
            "--latency-msec=16",
            "@DEFAULT_MONITOR@",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        return Some(child);
    }

    // 2. Fallback to native PipeWire `pw-cat --record`
    Command::new("pw-cat")
        .args([
            "--record",
            "--format",
            "f32",
            "--rate",
            "48000",
            "--channels",
            "1",
            "-",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()
}

async fn run_live_fft_capture(
    child: &mut Child,
    fft: &Arc<dyn rustfft::Fft<f32>>,
    sender: &EventSender,
    ctrl_rx: &mut watch::Receiver<VisualizerControl>,
) {
    let Some(mut stdout) = child.stdout.take() else {
        return;
    };

    let mut byte_buf = vec![0_u8; FFT_SIZE * 4];
    let mut pcm_samples = vec![0.0_f32; FFT_SIZE];
    let mut fft_buffer = vec![Complex::new(0.0_f32, 0.0_f32); FFT_SIZE];
    let mut phase = 0.0_f32;

    loop {
        let ctrl = *ctrl_rx.borrow();
        if !ctrl.active {
            break;
        }
        let bar_count = ctrl.bars.clamp(12, 24);

        tokio::select! {
            _ = ctrl_rx.changed() => {
                if !ctrl_rx.borrow().active {
                    break;
                }
            }
            read_res = stdout.read_exact(&mut byte_buf) => {
                match read_res {
                    Ok(_) => {
                        for (i, chunk) in byte_buf.chunks_exact(4).enumerate() {
                            pcm_samples[i] = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                        }
                        let mut spectrum = compute_fft_bars_into(&pcm_samples, fft.as_ref(), bar_count, &mut fft_buffer);
                        // If monitor stream is completely silent (e.g. sandboxed player), blend subtle procedural motion.
                        if spectrum.iter().all(|&v| v < 0.02) {
                            phase += 0.14;
                            spectrum = procedural_spectrum_frame(phase, bar_count);
                        }
                        sender.send(IsletEvent::SpectrumFrame(spectrum));
                    }
                    Err(_) => {
                        // Capture pipe closed; switch to procedural fallback for remainder of active session.
                        run_procedural_fallback(sender, ctrl_rx).await;
                        break;
                    }
                }
            }
        }
    }
}

async fn run_procedural_fallback(
    sender: &EventSender,
    ctrl_rx: &mut watch::Receiver<VisualizerControl>,
) {
    let mut ticker = tokio::time::interval(Duration::from_millis(20)); // 50 Hz updates
    let mut phase = 0.0_f32;

    loop {
        tokio::select! {
            _ = ctrl_rx.changed() => {
                if !ctrl_rx.borrow().active {
                    break;
                }
            }
            _ = ticker.tick() => {
                let ctrl = *ctrl_rx.borrow();
                if !ctrl.active {
                    break;
                }
                phase += 0.16;
                let frame = procedural_spectrum_frame(phase, ctrl.bars.clamp(12, 24));
                sender.send(IsletEvent::SpectrumFrame(frame));
            }
        }
    }
}

fn get_hann_window() -> &'static [f32; FFT_SIZE] {
    use std::sync::OnceLock;
    static HANN: OnceLock<[f32; FFT_SIZE]> = OnceLock::new();
    HANN.get_or_init(|| {
        let mut table = [0.0_f32; FFT_SIZE];
        for (i, val) in table.iter_mut().enumerate() {
            *val = 0.5 * (1.0 - (2.0 * PI * (i as f32) / ((FFT_SIZE - 1) as f32)).cos());
        }
        table
    })
}

/// Applies a precomputed Hann window to `samples`, executes `rustfft`, and downsamples magnitudes into `bar_count` log-spaced bars (`0.0..=1.0`).
#[must_use]
pub fn compute_fft_bars(
    samples: &[f32],
    fft: &dyn rustfft::Fft<f32>,
    bar_count: usize,
) -> Vec<f32> {
    let mut buffer = vec![Complex::new(0.0_f32, 0.0_f32); FFT_SIZE];
    compute_fft_bars_into(samples, fft, bar_count, &mut buffer)
}

/// Zero-allocation variant reusing a caller-provided scratch buffer.
pub fn compute_fft_bars_into(
    samples: &[f32],
    fft: &dyn rustfft::Fft<f32>,
    bar_count: usize,
    buffer: &mut [Complex<f32>],
) -> Vec<f32> {
    let n = samples.len().min(FFT_SIZE);
    let bars = bar_count.clamp(12, 24);
    if n < 16 {
        return vec![0.0; bars];
    }

    let hann = get_hann_window();
    for i in 0..FFT_SIZE {
        let sample = samples.get(i).copied().unwrap_or(0.0);
        buffer[i] = Complex::new(sample * hann[i], 0.0);
    }

    fft.process(buffer);

    // Use the positive frequency half-spectrum (`1 .. FFT_SIZE / 2`).
    let usable_bins = (FFT_SIZE / 2) - 1;
    let mut output = Vec::with_capacity(bars);
    let norm = (FFT_SIZE as f32).sqrt();

    for bar_idx in 0..bars {
        // Exponentially spaced bin ranges so bass and midrange get rich resolution.
        let t0 = (bar_idx as f32) / (bars as f32);
        let t1 = ((bar_idx + 1) as f32) / (bars as f32);
        let start_bin = (1.0 + (usable_bins as f32).powf(t0)).floor() as usize;
        let end_bin = (1.0 + (usable_bins as f32).powf(t1))
            .ceil()
            .max((start_bin + 1) as f32) as usize;

        let slice_end = end_bin.min(usable_bins);
        let slice_start = start_bin.min(slice_end.saturating_sub(1)).max(1);

        let mut peak = 0.0_f32;
        for bin in &buffer[slice_start..=slice_end] {
            let mag = (bin.norm() / norm) * 2.8;
            if mag > peak {
                peak = mag;
            }
        }

        // Convert to perceptual curve clamped to `0.0..=1.0`.
        let perceptual = peak.sqrt().clamp(0.0, 1.0);
        output.push(perceptual);
    }

    output
}

/// Generates a smooth, organic multi-sine waveform across `bar_count` bars when audio capture is unavailable.
#[must_use]
pub fn procedural_spectrum_frame(phase: f32, bar_count: usize) -> Vec<f32> {
    let bars = bar_count.clamp(12, 24);
    (0..bars)
        .map(|i| {
            let x = (i as f32) / (bars as f32);
            // Bell-curve envelope so center/bass bars are naturally taller than outer edges.
            let envelope = (PI * x).sin().powf(0.65);
            let wave1 = (phase + (i as f32) * 0.55).sin().abs();
            let wave2 = (phase * 1.45 - (i as f32) * 0.35).cos().abs() * 0.6;
            let combined = ((wave1 + wave2) * 0.58 * envelope).clamp(0.08, 0.96);
            combined
        })
        .collect()
}
