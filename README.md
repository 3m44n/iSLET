<h1 align="center">
  <img src="island.png" alt="🏝️" height="85" valign="middle">&nbsp;&nbsp;
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="img-white.png">
    <source media="(prefers-color-scheme: light)" srcset="img.png">
    <img alt="iSLET" src="img.png" height="85" valign="middle">
  </picture>
</h1>

<h3 align="center">Ultra-Fluid Dynamic Island & Floating Hardware HUD for Wayland</h3>

<p align="center">
  A premium, interruptible capsule HUD built with <b>Rust</b>, <b>GTK4 Layer Shell</b>, <b>PipeWire DSP</b>, and <b>D-Bus MPRIS2</b>.<br>
  Designed natively for <b>Hyprland</b>, <b>Sway</b>, <b>River</b>, and <b>Niri</b>.
</p>

<p align="center">
  <a href="https://aur.archlinux.org/packages/islet"><img src="https://img.shields.io/aur/version/islet?color=blue&logo=arch-linux&label=AUR" alt="AUR"></a>
  <a href="https://github.com/3m44n/iSLET/releases"><img src="https://img.shields.io/github/v/release/3m44n/iSLET?color=purple&logo=github&label=Release" alt="GitHub release"></a>
  <a href="https://github.com/3m44n/iSLET/actions"><img src="https://img.shields.io/github/actions/workflow/status/3m44n/iSLET/release.yml?logo=githubactions&label=Build" alt="Build Status"></a>
  <img src="https://img.shields.io/badge/Rust-1.75+-orange?logo=rust" alt="Rust 1.75+">
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-MIT-green" alt="MIT License"></a>
</p>

---

## ✨ Features

- 🌊 **Fluid Spring Physics**: Apple-grade interactive capsule morphing driven by a velocity-preserving spring ($k=320, d=24$) that seamlessly retargets mid-flight with zero snapping.
- 🎯 **True Pixel-Accurate Capsule Mask**: Rasterizes a mathematical semicircular `cairo::Region` input mask on every tick—clicks even 1 pixel outside the curved ends pass straight through to background windows.
- 🎵 **Smart MPRIS2 Media HUD**: Tracks all D-Bus players via `zbus`, caches album art in `$XDG_CACHE_HOME/islet/`, extracts vibrant dominant RGB via `color-thief` for an adaptive glowing background halo, and scrolls long titles smoothly.
- 📊 **Demand-Driven PipeWire FFT Equalizer**: Streams audio from the default sink monitor, computes a precomputed Hann-windowed 1024-point FFT (`rustfft`) downsampled to 12–24 glowing gradient bars at $\le 60$ FPS, with procedural wave fallback and **$0.0\%$ idle CPU suspension**.
- ⚡ **Dynamic Battery & Power Alert HUD**: Kernel `/sys/class/power_supply/` watcher that pulses glowing green when plugging in AC power or warns on critical battery levels.
- 🔊 **Zero-Polling Hardware Sliders**: Realtime audio volume (`wpctl` + `pactl subscribe`) and display brightness (`/sys/class/backlight/*` inotify) with mute shake animation and priority state restoration.
- 🔔 **Interactive Notification Pills**: Beautiful transient notification popups with title/body layout and smooth spring expansion.
- 🖥️ **Multi-Monitor Follower**: Automatically tracks focused outputs via Hyprland socket2 IPC.
- 🎨 **Live CSS & Config Hot-Reload**: Instant hot-reloading for `$XDG_CONFIG_HOME/islet/config.toml` and `style.css`.
- 🔌 **Unix Socket IPC & CLI (`islet-ctl`)**: Script, trigger, or bind any pill action from keybinds and shell scripts.

---

## 🚀 Quick Install

### Universal 1-Liner (Any Linux Distro)
```bash
curl -fsSL https://raw.githubusercontent.com/3m44n/iSLET/main/install.sh | bash
```

### Arch Linux (AUR)
```bash
yay -S islet
# or
paru -S islet
```
*(Precompiled binary release — installs in $\sim 2$ seconds without compiling!)*

### Ubuntu / Debian
Download the `.deb` package from the [Releases](https://github.com/3m44n/iSLET/releases) page:
```bash
sudo apt install ./islet_*_amd64.deb
```

### Nix / NixOS
```bash
nix run github:3m44n/iSLET
```

### Build from Source
```bash
git clone https://github.com/3m44n/iSLET.git && cd iSLET
cargo build --release --workspace
sudo install -Dm755 target/release/islet target/release/islet-ctl -t /usr/local/bin/
```

---

## 🔧 Compositor Setup

### Hyprland (`~/.config/hypr/hyprland.conf`)
```ini
exec-once = islet

# Frosted glass blur behind the pill
layerrule = blur, islet
layerrule = ignorealpha 0.2, islet

# Keybind to toggle pill visibility
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

---

## 🎮 CLI Controls (`islet-ctl`)

Control the running daemon from scripts, waybar, or compositor shortcuts:

```bash
# Hardware HUD overlays
islet-ctl show volume 75          # Show volume at 75%
islet-ctl show brightness 80      # Show brightness at 80%
islet-ctl show battery 90 charging # Show charging battery pill

# Media & Notifications
islet-ctl toggle                  # Toggle pill visibility
islet-ctl notify "Update" "All packages synced!"
islet-ctl ping                    # Check daemon heartbeat
```

---

## 🎨 Theming Guide (`~/.config/islet/style.css`)

iSLET hot-reloads `~/.config/islet/style.css` instantly on save. Two ready-made themes are bundled in `assets/`:

```bash
# Catppuccin Mocha:
cp assets/theme-catppuccin-mocha.css ~/.config/islet/style.css

# Dark Glass (Obsidian):
cp assets/theme-dark-glass.css ~/.config/islet/style.css
```

| CSS Selector | Description |
|---|---|
| `.islet-pill` | Capsule container in resting state |
| `.islet-pill.expanded` | Expanded state (Media, Sliders, Notifications) |
| `.islet-art` | Album art image element |
| `.islet-title` | Track title text |
| `.islet-artist` | Artist subtitle text |
| `.islet-slider-bar` | Hardware slider progress bar |
| `.islet-notif-badge` | Notification bell icon container |
| `.islet-notif-title` | Notification title text |
| `.islet-notif-body` | Notification body text |

---

## 🧠 Architecture

```text
┌─────────────────────────────────────────────────────────────────────────┐
│                    islet-sources (Tokio Runtime Thread)                 │
│                                                                         │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  ┌─────────────┐  │
│  │ MPRIS2 zbus  │  │ Audio Volume │  │ Power/Battery│  │ PipeWire    │  │
│  │  + Art/RGB   │  │ pactl/wpctl  │  │ sysfs inotify│  │ rustfft DSP │  │
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
│   StateMachine (islet-core) ──► PillSurface (Spring Morphing k=320/24)  │
│   • Priority Override (Slider > Media)   • Cairo A8 Capsule Input Mask  │
│   • Sequence-tagged AutoCollapse         • Frame-Clock Spectrum (≤60Hz) │
└─────────────────────────────────────────────────────────────────────────┘
```

---

## 📊 Comparison

| Capability | **iSLET** | `swayosd` | `dunst` / `mako` |
|---|---|---|---|
| **Surface Style** | Unified Morphing Dynamic Island capsule | Floating rectangular OSD | Stacked notification cards |
| **MPRIS2 Media + Art Glow** | ✅ Built-in with dominant-color RGB glow | ❌ No | ⚠️ Text notifications only |
| **Realtime Audio FFT Visualizer** | ✅ Hann-windowed PipeWire FFT | ❌ No | ❌ No |
| **Battery & Charging Alert** | ✅ Built-in kernel sysfs watcher | ❌ No | ⚠️ Generic notification |
| **Spring Interpolation** | ✅ Fluid $k=320, d=24$ velocity-preserving spring | ❌ Fixed linear fade | ❌ Static box geometry |
| **Click-Through Rounded Corners** | ✅ Mathematical scanline Cairo mask | ❌ Full rectangle | ❌ Full rectangle |
| **Idle CPU Usage** | **$0.0\%$** (timers/FFT sleep when idle) | $0.0\%$ | $0.0\%$ |

---

## 📄 License

Licensed under the [MIT License](LICENSE).
