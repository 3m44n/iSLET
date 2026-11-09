//! Animated pill container with dominant-color glow, hover seek bar, idle clock, and `reduce_motion` support.

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cairo::{RectangleInt, Region};
use gdk4::prelude::*;
use gtk4::prelude::*;
use islet_core::{
    EventSender, IsletEvent, PillDimensions, PillState, PlaybackStatus, SliderKind, TrackMetadata,
    TransitionEffect, MAX_SURFACE_WIDTH, SURFACE_SHADOW_PADDING,
};
use islet_sources::MprisCommand;
use tokio::sync::mpsc;
use tracing::trace;

use crate::visualizer_widget::SpectrumWidget;

pub struct PillSurface {
    outer_box: gtk4::Box,
    pill_frame: gtk4::Box,
    view_stack: gtk4::Stack,
    idle_label: gtk4::Label,
    art_picture: gtk4::Picture,
    title_label: gtk4::Label,
    artist_label: gtk4::Label,
    spectrum: SpectrumWidget,
    controls_box: gtk4::Box,
    play_pause_btn: gtk4::Button,
    seek_scale: gtk4::Scale,
    updating_seek: Rc<Cell<bool>>,
    accent_provider: gtk4::CssProvider,
    active_player_id: Rc<RefCell<Option<String>>>,
    full_title: Rc<RefCell<String>>,
    marquee_offset: Rc<Cell<usize>>,
    marquee_pause_ticks: Rc<Cell<u8>>,
    marquee_running: Rc<Cell<bool>>,
    reduce_motion: Rc<Cell<bool>>,
    slider_icon: gtk4::Label,
    slider_bar: gtk4::ProgressBar,
    slider_pct_label: gtk4::Label,
    current_dims: Rc<Cell<PillDimensions>>,
    target_dims: Rc<Cell<PillDimensions>>,
    vel_w: Rc<Cell<f64>>,
    vel_h: Rc<Cell<f64>>,
    anim_tick_id: Rc<RefCell<Option<gtk4::TickCallbackId>>>,
    window: gtk4::ApplicationWindow,
}

impl PillSurface {
    #[must_use]
    pub fn new(
        window: &gtk4::ApplicationWindow,
        sender: EventSender,
        mpris_tx: mpsc::UnboundedSender<MprisCommand>,
        visualizer_bars: usize,
    ) -> Self {
        let initial = PillDimensions::IDLE;
        let current_dims = Rc::new(Cell::new(initial));
        let target_dims = Rc::new(Cell::new(initial));
        let vel_w = Rc::new(Cell::new(0.0_f64));
        let vel_h = Rc::new(Cell::new(0.0_f64));
        let anim_tick_id = Rc::new(RefCell::new(None));
        let active_player_id: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
        let reduce_motion = Rc::new(Cell::new(false));
        let updating_seek = Rc::new(Cell::new(false));

        let accent_provider = gtk4::CssProvider::new();
        if let Some(display) = gdk4::Display::default() {
            gtk4::style_context_add_provider_for_display(
                &display,
                &accent_provider,
                gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
            );
        }

        let outer_box = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Vertical)
            .halign(gtk4::Align::Center)
            .valign(gtk4::Align::Start)
            .margin_top(SURFACE_SHADOW_PADDING / 2)
            .build();

        let pill_frame = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .halign(gtk4::Align::Center)
            .valign(gtk4::Align::Center)
            .width_request(initial.width as i32)
            .height_request(initial.height as i32)
            .css_classes(["islet-pill"])
            .build();

        let view_stack = gtk4::Stack::builder()
            .transition_type(gtk4::StackTransitionType::Crossfade)
            .transition_duration(220)
            .hexpand(true)
            .vexpand(true)
            .build();

        // --- 1. Idle View ---
        let idle_label = gtk4::Label::builder()
            .label(&format!("●  {}", current_hh_mm()))
            .halign(gtk4::Align::Center)
            .valign(gtk4::Align::Center)
            .css_classes(["islet-label"])
            .build();
        view_stack.add_named(&idle_label, Some("idle"));

        // --- 2. Media View ---
        let media_root = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Vertical)
            .valign(gtk4::Align::Center)
            .spacing(6)
            .build();

        let media_top_row = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .spacing(10)
            .valign(gtk4::Align::Center)
            .build();

        let art_picture = gtk4::Picture::builder()
            .width_request(42)
            .height_request(42)
            .can_shrink(true)
            .css_classes(["islet-art"])
            .build();

        let text_col = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Vertical)
            .valign(gtk4::Align::Center)
            .hexpand(true)
            .spacing(2)
            .build();

        let title_label = gtk4::Label::builder()
            .label("No Track")
            .halign(gtk4::Align::Start)
            .ellipsize(gtk4::pango::EllipsizeMode::None)
            .single_line_mode(true)
            .css_classes(["islet-title"])
            .build();

        let artist_label = gtk4::Label::builder()
            .label("")
            .halign(gtk4::Align::Start)
            .ellipsize(gtk4::pango::EllipsizeMode::End)
            .single_line_mode(true)
            .css_classes(["islet-artist"])
            .build();

        text_col.append(&title_label);
        text_col.append(&artist_label);

        let spectrum = SpectrumWidget::new(visualizer_bars);

        media_top_row.append(&art_picture);
        media_top_row.append(&text_col);
        media_top_row.append(spectrum.widget());

        let controls_box = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .halign(gtk4::Align::Fill)
            .valign(gtk4::Align::Center)
            .spacing(10)
            .visible(false)
            .css_classes(["islet-controls"])
            .build();

        let prev_btn = gtk4::Button::with_label("⏮");
        let play_pause_btn = gtk4::Button::with_label("⏯");
        let next_btn = gtk4::Button::with_label("⏭");
        for btn in [&prev_btn, &play_pause_btn, &next_btn] {
            btn.add_css_class("islet-ctrl-btn");
        }

        let seek_scale = gtk4::Scale::with_range(gtk4::Orientation::Horizontal, 0.0, 100.0, 1.0);
        seek_scale.set_draw_value(false);
        seek_scale.set_hexpand(true);
        seek_scale.add_css_class("islet-seek-bar");

        let pid_prev = Rc::clone(&active_player_id);
        let tx_prev = mpris_tx.clone();
        prev_btn.connect_clicked(move |_| {
            if let Some(id) = pid_prev.borrow().clone() {
                let _ = tx_prev.send(MprisCommand::Previous(id));
            }
        });

        let pid_pp = Rc::clone(&active_player_id);
        let tx_pp = mpris_tx.clone();
        play_pause_btn.connect_clicked(move |_| {
            if let Some(id) = pid_pp.borrow().clone() {
                let _ = tx_pp.send(MprisCommand::PlayPause(id));
            }
        });

        let pid_next = Rc::clone(&active_player_id);
        let tx_next = mpris_tx.clone();
        next_btn.connect_clicked(move |_| {
            if let Some(id) = pid_next.borrow().clone() {
                let _ = tx_next.send(MprisCommand::Next(id));
            }
        });

        let pid_seek = Rc::clone(&active_player_id);
        let tx_seek = mpris_tx;
        let updating_seek_cb = Rc::clone(&updating_seek);
        let last_seek_pct = Rc::new(Cell::new(0.0_f64));
        let last_seek_for_cb = Rc::clone(&last_seek_pct);
        seek_scale.connect_value_changed(move |scale| {
            if updating_seek_cb.get() {
                last_seek_for_cb.set(scale.value());
                return;
            }
            let new_val = scale.value();
            let delta_pct = new_val - last_seek_for_cb.get();
            last_seek_for_cb.set(new_val);
            if delta_pct.abs() > 1.0 {
                if let Some(id) = pid_seek.borrow().clone() {
                    let offset_us = (delta_pct * 500_000.0) as i64;
                    let _ = tx_seek.send(MprisCommand::SeekRelativeUs(id, offset_us));
                }
            }
        });

        controls_box.append(&prev_btn);
        controls_box.append(&play_pause_btn);
        controls_box.append(&next_btn);
        controls_box.append(&seek_scale);

        media_root.append(&media_top_row);
        media_root.append(&controls_box);
        view_stack.add_named(&media_root, Some("media"));

        // --- 3. Slider View ---
        let slider_row = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .valign(gtk4::Align::Center)
            .spacing(10)
            .build();

        let slider_icon = gtk4::Label::builder()
            .label("🔊")
            .css_classes(["islet-slider-icon"])
            .build();

        let slider_bar = gtk4::ProgressBar::builder()
            .hexpand(true)
            .valign(gtk4::Align::Center)
            .css_classes(["islet-slider-bar"])
            .build();

        let slider_pct_label = gtk4::Label::builder()
            .label("0%")
            .width_chars(4)
            .halign(gtk4::Align::End)
            .css_classes(["islet-slider-pct"])
            .build();

        slider_row.append(&slider_icon);
        slider_row.append(&slider_bar);
        slider_row.append(&slider_pct_label);
        view_stack.add_named(&slider_row, Some("slider"));

        pill_frame.append(&view_stack);
        outer_box.append(&pill_frame);

        // Gestures
        let left_tx = sender.clone();
        let left_click = gtk4::GestureClick::builder().button(1).build();
        left_click.connect_released(move |_, _, _, _| {
            left_tx.send(IsletEvent::PillClicked);
        });
        pill_frame.add_controller(left_click);

        let right_tx = sender.clone();
        let right_click = gtk4::GestureClick::builder().button(3).build();
        right_click.connect_released(move |_, _, _, _| {
            right_tx.send(IsletEvent::PillRightClicked);
        });
        pill_frame.add_controller(right_click);

        let hover_enter_tx = sender.clone();
        let hover_leave_tx = sender;
        let motion_ctrl = gtk4::EventControllerMotion::new();
        motion_ctrl.connect_enter(move |_, _, _| {
            hover_enter_tx.send(IsletEvent::HoverChanged(true));
        });
        motion_ctrl.connect_leave(move |_| {
            hover_leave_tx.send(IsletEvent::HoverChanged(false));
        });
        pill_frame.add_controller(motion_ctrl);

        let full_title = Rc::new(RefCell::new(String::new()));
        let marquee_offset = Rc::new(Cell::new(0_usize));
        let marquee_pause_ticks = Rc::new(Cell::new(8_u8));
        let marquee_running = Rc::new(Cell::new(false));

        let dims_on_map = Rc::clone(&current_dims);
        window.connect_map(move |win| {
            update_wayland_input_region(win, dims_on_map.get());
        });

        let dims_on_scale = Rc::clone(&current_dims);
        window.connect_scale_factor_notify(move |win| {
            update_wayland_input_region(win, dims_on_scale.get());
        });

        Self {
            outer_box,
            pill_frame,
            view_stack,
            idle_label,
            art_picture,
            title_label,
            artist_label,
            spectrum,
            controls_box,
            play_pause_btn,
            seek_scale,
            updating_seek,
            accent_provider,
            active_player_id,
            full_title,
            marquee_offset,
            marquee_pause_ticks,
            marquee_running,
            reduce_motion,
            slider_icon,
            slider_bar,
            slider_pct_label,
            current_dims,
            target_dims,
            vel_w,
            vel_h,
            anim_tick_id,
            window: window.clone(),
        }
    }

    #[must_use]
    pub fn root_widget(&self) -> &gtk4::Box {
        &self.outer_box
    }

    pub fn update_spectrum(&self, bars: &[f32]) {
        self.spectrum.set_target_frame(bars);
    }

    pub fn set_reduce_motion(&self, enabled: bool) {
        self.reduce_motion.set(enabled);
        self.view_stack.set_transition_type(if enabled {
            gtk4::StackTransitionType::None
        } else {
            gtk4::StackTransitionType::Crossfade
        });
    }

    pub fn apply_transition(
        &self,
        effect: &TransitionEffect,
        player_id: Option<&str>,
        status: PlaybackStatus,
        metadata: Option<&TrackMetadata>,
        visualizer_active: bool,
    ) {
        *self.active_player_id.borrow_mut() = player_id.map(str::to_owned);
        self.target_dims.set(effect.dimensions);
        self.pill_frame.remove_css_class("mute-pulse");

        self.spectrum
            .set_running(visualizer_active && !self.reduce_motion.get());

        match &effect.state {
            PillState::Hidden => {
                self.stop_marquee();
                self.pill_frame.set_opacity(0.0);
            }
            PillState::Idle => {
                self.stop_marquee();
                self.pill_frame.set_opacity(1.0);
                self.pill_frame.remove_css_class("expanded");
                self.idle_label
                    .set_label(&format!("●  {}", current_hh_mm()));
                self.view_stack.set_visible_child_name("idle");
            }
            PillState::Media {
                expanded,
                controls_visible,
            } => {
                self.pill_frame.set_opacity(1.0);
                if *expanded || *controls_visible {
                    self.pill_frame.add_css_class("expanded");
                } else {
                    self.pill_frame.remove_css_class("expanded");
                }

                self.play_pause_btn.set_label(if status == PlaybackStatus::Playing {
                    "⏸"
                } else {
                    "▶"
                });
                self.controls_box.set_visible(*controls_visible);
                self.art_picture.set_visible(*expanded || *controls_visible);
                self.artist_label.set_visible(*expanded || *controls_visible);

                if let Some(meta) = metadata {
                    self.spectrum.set_accent_color(meta.accent_rgb);
                    self.apply_accent_glow(meta.accent_rgb);

                    if let (Some(pos), Some(dur)) = (meta.position_ms, meta.duration_ms) {
                        if dur > 0 {
                            let pct = ((pos as f64 / dur as f64) * 100.0).clamp(0.0, 100.0);
                            self.updating_seek.set(true);
                            self.seek_scale.set_value(pct);
                            self.updating_seek.set(false);
                        }
                    }

                    let mut current_full = self.full_title.borrow_mut();
                    if *current_full != meta.title {
                        *current_full = meta.title.clone();
                        self.marquee_offset.set(0);
                        self.marquee_pause_ticks.set(8);
                        self.title_label.set_label(&meta.title);
                    }
                    self.artist_label.set_label(&meta.artist);

                    if let Some(ref art_path) = meta.art_url {
                        self.art_picture.set_filename(Some(Path::new(art_path)));
                    }
                    drop(current_full);
                    self.ensure_marquee_if_needed(
                        status == PlaybackStatus::Playing && !self.reduce_motion.get(),
                    );
                } else {
                    self.stop_marquee();
                }
                self.view_stack.set_visible_child_name("media");
            }
            PillState::Slider(SliderKind::Volume {
                percent,
                muted,
                pulse,
            }) => {
                self.stop_marquee();
                self.pill_frame.set_opacity(1.0);
                self.pill_frame.add_css_class("expanded");
                if *pulse && !self.reduce_motion.get() {
                    self.pill_frame.add_css_class("mute-pulse");
                }

                let icon = if *muted || *percent == 0 {
                    "🔇"
                } else if *percent < 50 {
                    "🔉"
                } else {
                    "🔊"
                };
                self.slider_icon.set_label(icon);
                self.slider_bar.set_fraction(if *muted {
                    0.0
                } else {
                    (f64::from(*percent) / 100.0).clamp(0.0, 1.0)
                });
                self.slider_pct_label
                    .set_label(&if *muted { "Mute".to_owned() } else { format!("{percent}%") });
                self.view_stack.set_visible_child_name("slider");
            }
            PillState::Slider(SliderKind::Brightness { percent }) => {
                self.stop_marquee();
                self.pill_frame.set_opacity(1.0);
                self.pill_frame.add_css_class("expanded");
                self.slider_icon.set_label("☀️");
                self.slider_bar
                    .set_fraction((f64::from(*percent) / 100.0).clamp(0.0, 1.0));
                self.slider_pct_label.set_label(&format!("{percent}%"));
                self.view_stack.set_visible_child_name("slider");
            }
            PillState::Notification { title, body } => {
                self.stop_marquee();
                self.pill_frame.set_opacity(1.0);
                self.pill_frame.add_css_class("expanded");
                self.idle_label.set_label(&format!("🔔 {title} — {body}"));
                self.view_stack.set_visible_child_name("idle");
            }
        }

        if self.reduce_motion.get() {
            if let Some(id) = self.anim_tick_id.borrow_mut().take() {
                id.remove();
            }
            self.vel_w.set(0.0);
            self.vel_h.set(0.0);
            self.current_dims.set(effect.dimensions);
            self.pill_frame
                .set_width_request(effect.dimensions.width.round() as i32);
            self.pill_frame
                .set_height_request(effect.dimensions.height.round() as i32);
            update_wayland_input_region(&self.window, effect.dimensions);
        } else {
            self.start_spring_morph();
        }
    }

    /// Runs a velocity-preserving critically damped spring morph on the GTK4 frame clock.
    /// Automatically unregisters the tick callback once settled so idle CPU is 0%.
    fn start_spring_morph(&self) {
        if self.anim_tick_id.borrow().is_some() {
            // Already running; updated `self.target_dims` will be picked up seamlessly on the next frame!
            return;
        }

        let current_ref = Rc::clone(&self.current_dims);
        let target_ref = Rc::clone(&self.target_dims);
        let vel_w_ref = Rc::clone(&self.vel_w);
        let vel_h_ref = Rc::clone(&self.vel_h);
        let tick_slot = Rc::clone(&self.anim_tick_id);
        let window_weak = self.window.downgrade();
        let last_us = Cell::new(0_i64);

        let id = self.pill_frame.add_tick_callback(move |pill, clock| {
            let now_us = clock.frame_time();
            let prev_us = last_us.get();
            last_us.set(now_us);

            let dt = if prev_us == 0 {
                1.0 / 60.0
            } else {
                ((now_us - prev_us) as f64 / 1_000_000.0).clamp(0.001, 0.033)
            };

            let cur = current_ref.get();
            let tgt = target_ref.get();
            let mut vw = vel_w_ref.get();
            let mut vh = vel_h_ref.get();

            // Spring constants: stiffness = 260, damping = 28 (smooth, responsive Dynamic Island feel)
            const STIFFNESS: f64 = 260.0;
            const DAMPING: f64 = 28.0;

            let force_w = (tgt.width - cur.width) * STIFFNESS - vw * DAMPING;
            let force_h = (tgt.height - cur.height) * STIFFNESS - vh * DAMPING;

            vw += force_w * dt;
            vh += force_h * dt;

            let next_w = (cur.width + vw * dt).max(24.0);
            let next_h = (cur.height + vh * dt).max(12.0);

            vel_w_ref.set(vw);
            vel_h_ref.set(vh);

            let settled = (tgt.width - next_w).abs() < 0.35
                && (tgt.height - next_h).abs() < 0.35
                && vw.abs() < 1.0
                && vh.abs() < 1.0;

            let final_dims = if settled {
                vel_w_ref.set(0.0);
                vel_h_ref.set(0.0);
                tgt
            } else {
                PillDimensions::new(next_w, next_h)
            };

            current_ref.set(final_dims);
            pill.set_width_request(final_dims.width.round() as i32);
            pill.set_height_request(final_dims.height.round() as i32);

            if let Some(win) = window_weak.upgrade() {
                update_wayland_input_region(&win, final_dims);
            }

            if settled {
                *tick_slot.borrow_mut() = None;
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        });

        *self.anim_tick_id.borrow_mut() = Some(id);
    }

    fn apply_accent_glow(&self, rgb: Option<(u8, u8, u8)>) {
        let (r, g, b) = rgb.unwrap_or((137, 180, 250));
        let dynamic_css = format!(
            ".islet-pill.expanded {{ border-color: rgba({r}, {g}, {b}, 0.35); box-shadow: 0 12px 28px rgba(0, 0, 0, 0.55), 0 0 22px rgba({r}, {g}, {b}, 0.28); }} .islet-art {{ box-shadow: 0 0 14px rgba({r}, {g}, {b}, 0.45); }} .islet-seek-bar highlight {{ background-color: rgb({r}, {g}, {b}); }}"
        );
        self.accent_provider.load_from_string(&dynamic_css);
    }

    fn ensure_marquee_if_needed(&self, is_playing: bool) {
        const WINDOW_CHARS: usize = 20;
        let needs_scroll = is_playing && self.full_title.borrow().chars().count() > WINDOW_CHARS;
        if !needs_scroll {
            self.stop_marquee();
            return;
        }
        if self.marquee_running.get() {
            return;
        }
        self.marquee_running.set(true);

        let running_flag = Rc::clone(&self.marquee_running);
        let title_lbl = self.title_label.clone();
        let full_ref = Rc::clone(&self.full_title);
        let offset_ref = Rc::clone(&self.marquee_offset);
        let pause_ref = Rc::clone(&self.marquee_pause_ticks);

        glib::timeout_add_local(Duration::from_millis(180), move || {
            if !running_flag.get() {
                return glib::ControlFlow::Break;
            }
            let text = full_ref.borrow();
            let chars: Vec<char> = text.chars().collect();
            if chars.len() <= WINDOW_CHARS {
                running_flag.set(false);
                return glib::ControlFlow::Break;
            }

            let pause = pause_ref.get();
            if pause > 0 {
                pause_ref.set(pause - 1);
                return glib::ControlFlow::Continue;
            }

            let next_off = offset_ref.get() + 1;
            if next_off + WINDOW_CHARS > chars.len() {
                offset_ref.set(0);
                pause_ref.set(10);
                let slice: String = chars.iter().take(WINDOW_CHARS).collect();
                title_lbl.set_label(&slice);
            } else {
                offset_ref.set(next_off);
                if next_off + WINDOW_CHARS == chars.len() {
                    pause_ref.set(8);
                }
                let slice: String = chars.iter().skip(next_off).take(WINDOW_CHARS).collect();
                title_lbl.set_label(&slice);
            }
            glib::ControlFlow::Continue
        });
    }

    fn stop_marquee(&self) {
        self.marquee_running.set(false);
    }
}

fn current_hh_mm() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let hours = (secs / 3600) % 24;
    let mins = (secs / 60) % 60;
    format!("{hours:02}:{mins:02}")
}

fn update_wayland_input_region(window: &gtk4::ApplicationWindow, dims: PillDimensions) {
    let Some(surface) = window.surface() else {
        return;
    };

    let width = dims.width.round().max(1.0) as i32;
    let height = dims.height.round().max(1.0) as i32;
    let pill_x = (MAX_SURFACE_WIDTH - width) / 2;
    let pill_y = SURFACE_SHADOW_PADDING / 2;

    let region = build_rounded_pill_region(pill_x, pill_y, width, height);
    surface.set_input_region(&region);
    trace!(pill_x, pill_y, width, height, "Updated Wayland capsule input region");
}

/// Builds an exact rounded-capsule `cairo::Region` from horizontal scanline spans.
///
/// Clicks outside the semicircular left and right ends pass straight through to windows below.
fn build_rounded_pill_region(x: i32, y: i32, width: i32, height: i32) -> Region {
    let h = height.max(1);
    let w = width.max(1);
    let r = f64::from(h) * 0.5;
    let mut rects = Vec::with_capacity(h as usize);

    for row in 0..h {
        let dy = (f64::from(row) + 0.5) - r;
        let chord_half = (r * r - dy * dy).max(0.0).sqrt();
        let inset = (r - chord_half).floor().max(0.0) as i32;
        let span_w = (w - inset * 2).max(1);
        rects.push(RectangleInt::new(x + inset, y + row, span_w, 1));
    }

    Region::create_rectangles(&rects)
}
