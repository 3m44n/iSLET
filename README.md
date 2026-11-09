# Islet 🏝️

> A fluid, low-resource **Dynamic Island / Floating Media & Hardware HUD** for Wayland compositors (Hyprland, Sway, River, Niri), built with Rust, GTK4, `wlr-layer-shell`, D-Bus MPRIS2, and PipeWire.

## ✨ Features

- **Fluid Interruptible Morphing**: Spring-driven (`adw::SpringAnimation`) width/height capsule morphing (250–350ms) that smoothly retargets mid-flight without snapping.
- **True Capsule Input Pass-Through**: Rasterizes an exact semicircular-ended `cairo::Region` mask so clicks even 1px outside the curved pill corners pass straight through to windows underneath.
- **Smart MPRIS2 Media HUD**: Tracks all D-Bus players via `zbus`, prefers `Playing` over `Paused` (remembering the last active player), caches album art in `$XDG_CACHE_HOME/islet/`, extracts dominant RGB via `color-thief` for a soft ambient glow, and scrolls long titles with end-pauses.
- **Demand-Driven PipeWire FFT Visualizer**: Streams the default sink monitor (`parec`/`pw-cat`), computes a Hann-windowed 1024-point FFT (`rustfft`) downsampled to 12–24 bars at ≤60 FPS on the GTK4 frame clock, falls back to a procedural harmonic wave if capture is unavailable, and **completely suspends when paused or hidden (`0.0%` idle CPU)**.
- **Zero-Polling Hardware Sliders**: Event-driven audio volume (`pactl subscribe` + `wpctl`) and kernel `inotify` on `/sys/class/backlight/*` with mute shake/pulse animation and priority state restoration.
- **Live Config & CSS Hot-Reload**: Watches `$XDG_CONFIG_HOME/islet/config.toml` and `style.css` via `inotify`.
- **Unix Socket IPC & CLI (`islet-ctl`)**: Control or script the pill from keybinds or scripts via `$XDG_RUNTIME_DIR/islet.sock`.

## 🚀 Quick Install

```bash
git clone https://github.com/3m44n/iSLET.git && cd iSLET
cargo build --release --workspace
sudo install -Dm755 target/release/islet target/release/islet-ctl -t /usr/local/bin/
```

## 🔧 Compositor Setup Snippets

### Hyprland (`~/.config/hypr/hyprland.conf`)
```ini
exec-once = islet

# Optional frosted-glass blur behind the pill
layerrule = blur, islet
layerrule = ignorealpha 0.2, islet

bind = SUPER, I, exec, islet-ctl toggle
```

### Sway (`~/.config/sway/config`)
```ini
exec islet
bindsym $mod+i exec islet-ctl toggle
```

### Niri (`~/.config/niri/config.kdl`)
```kdl
spawn-at-startup "islet"
binds {
    Mod+I { spawn "islet-ctl" "toggle"; }
}
```

## 🎨 Theming Guide (`~/.config/islet/style.css`)

Islet hot-reloads `~/.config/islet/style.css` the moment you save it. Two ready-made themes are bundled in `assets/`:

```bash
# Activate Catppuccin Mocha theme:
cp assets/theme-catppuccin-mocha.css ~/.config/islet/style.css

# Or activate Dark Glass theme:
cp assets/theme-dark-glass.css ~/.config/islet/style.css
```

| CSS Variable (`@define-color`) | Default | Description |
|---|---|---|
| `@islet_bg` | `rgba(14, 14, 18, 0.93)` | Collapsed pill background |
| `@islet_bg_expanded` | `rgba(18, 18, 24, 0.96)` | Expanded Media/Slider pill background |
| `@islet_border` | `rgba(255, 255, 255, 0.11)` | Subtle outer capsule border |
| `@islet_fg` | `#cdd6f4` | Primary title & percentage text color |
| `@islet_subtext` | `#a6adc8` | Artist subtitle color |
| `@islet_accent` | `#89b4fa` | Slider progress bar fill color |

## 🧠 How It Works

```text
┌─────────────────────────────────────────────────────────────────────────┐
│                    islet-sources (Tokio Runtime Thread)                 │
│                                                                         │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  ┌─────────────┐  │
│  │ MPRIS2 zbus  │  │ Audio Volume │  │  Backlight   │  │  PipeWire   │  │
│  │  + Art/RGB   │  │pactl/wpctl ev│  │sysfs inotify │  │  rustfft    │  │
│  └──────┬───────┘  └──────┬───────┘  └──────┬───────┘  └──────┬──────┘  │
│         │                 │                 │                 │         │
│         └─────────────────┴────────┬────────┴─────────────────┘         │
│                                    ▼                                    │
│                      EventBus (mpsc::unbounded)                         │
└────────────────────────────────────┼────────────────────────────────────┘
                                     ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                     islet-ui (GTK4 / GLib Main Thread)                  │
│                                                                         │
│   StateMachine (islet-core) ──► PillSurface (adw::SpringAnimation)      │
│   • Priority Override (Slider > Media)   • Cairo A8 Capsule Input Mask  │
│   • Sequence-tagged AutoCollapse         • Frame-Clock Spectrum (≤60Hz) │
└─────────────────────────────────────────────────────────────────────────┘
```

## 📊 Comparison vs. Existing Wayland Overlays

| Capability | **Islet** | `swayosd` | `mako` / `dunst` |
|---|---|---|---|
| **Unified Surface** | Single morphing top-center Dynamic Island pill | Separate OSD popup window | Stacked notification boxes |
| **MPRIS2 Media + Art Glow** | Built-in with dominant-color glow & marquee | No | Text/icon notifications only |
| **Live FFT Audio Spectrum** | Built-in PipeWire FFT + procedural fallback | No | No |
| **Interruptible Spring Morphs** | Velocity-preserving `adw::SpringAnimation` | Fixed fade/pop | Static box geometry |
| **Click-Through Rounded Ends** | Pixel-accurate Cairo capsule mask | Full rectangle | Full rectangle |
| **Idle CPU Usage** | `0.0%` (all timers/FFT suspended when idle) | `0.0%` | `0.0%` |
