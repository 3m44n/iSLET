//! Animated pill container with Media HUD (album art, marquee title, controls) and Volume/Brightness Sliders.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use cairo::{Context, Format, ImageSurface, Region};
use gtk4::prelude::*;
use islet_core::{
    EventSender, IsletEvent, PillDimensions, PillState, PlaybackStatus, SliderKind, TrackMetadata,
    TransitionEffect, MAX_SURFACE_WIDTH, SURFACE_SHADOW_PADDING,
};
use islet_sources::MprisCommand;
use tokio::sync::mpsc;
use tracing::trace;

/// Manages the visual pill widget, cross-fading stack views, marquee text, and Cairo input-region mask.
pub struct PillSurface {
    outer_box: gtk4::Box,
    pill_frame: gtk4::Box,
    view_stack: gtk4::Stack,
    // Idle view
    idle_label: gtk4::Label,
    // Media view
    art_picture: gtk4::Picture,
    title_label: gtk4::Label,
    artist_label: gtk4::Label,
    controls_box: gtk4::Box,
    play_pause_btn: gtk4::Button,
    active_player_id: Rc<RefCell<Option<String>>>,
    full_title: Rc<RefCell<String>>,
    marquee_offset: Rc<Cell<usize>>,
    marquee_pause_ticks: Rc<Cell<u8>>,
    // Slider view (Volume / Brightness)
    slider_icon: gtk4::Label,
    slider_bar: gtk4::ProgressBar,
    slider_pct_label: gtk4::Label,
    // Geometry & spring animation
    current_dims: Rc<Cell<PillDimensions>>,
    spring_anim: adw::SpringAnimation,
    start_dims: Rc<Cell<PillDimensions>>,
    target_dims: Rc<Cell<PillDimensions>>,
}

impl PillSurface {
    #[must_use]
    pub fn new(
        window: &gtk4::ApplicationWindow,
        sender: EventSender,
        mpris_tx: mpsc::UnboundedSender<MprisCommand>,
    ) -> Self {
        let initial = PillDimensions::IDLE;
        let current_dims = Rc::new(Cell::new(initial));
        let start_dims = Rc::new(Cell::new(initial));
        let target_dims = Rc::new(Cell::new(initial));
        let active_player_id: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));

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
            .label("●  Islet")
            .halign(gtk4::Align::Center)
            .valign(gtk4::Align::Center)
            .css_classes(["islet-label"])
            .build();
        view_stack.add_named(&idle_label, Some("idle"));

        // --- 2. Media View (Album Art + Marquee Title/Artist + Hover Controls) ---
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
        media_top_row.append(&art_picture);
        media_top_row.append(&text_col);

        // Transport controls shown on hover
        let controls_box = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .halign(gtk4::Align::Center)
            .spacing(14)
            .visible(false)
            .css_classes(["islet-controls"])
            .build();

        let prev_btn = gtk4::Button::with_label("⏮");
        let play_pause_btn = gtk4::Button::with_label("⏯");
        let next_btn = gtk4::Button::with_label("⏭");
        for btn in [&prev_btn, &play_pause_btn, &next_btn] {
            btn.add_css_class("islet-ctrl-btn");
        }

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
        let tx_next = mpris_tx;
        next_btn.connect_clicked(move |_| {
            if let Some(id) = pid_next.borrow().clone() {
                let _ = tx_next.send(MprisCommand::Next(id));
            }
        });

        controls_box.append(&prev_btn);
        controls_box.append(&play_pause_btn);
        controls_box.append(&next_btn);

        media_root.append(&media_top_row);
        media_root.append(&controls_box);
        view_stack.add_named(&media_root, Some("media"));

        // --- 3. Slider View (Volume & Brightness) ---
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

        // Left-click (Play/Pause) and Right-click (Raise window) gestures
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

        // Hover controller
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

        // Marquee text scroller (scrolls titles > 24 chars with pause at ends)
        let full_title = Rc::new(RefCell::new(String::new()));
        let marquee_offset = Rc::new(Cell::new(0_usize));
        let marquee_pause_ticks = Rc::new(Cell::new(8_u8));
        {
            let title_lbl = title_label.clone();
            let full_ref = Rc::clone(&full_title);
            let offset_ref = Rc::clone(&marquee_offset);
            let pause_ref = Rc::clone(&marquee_pause_ticks);

            glib::timeout_add_local(Duration::from_millis(180), move || {
                let text = full_ref.borrow();
                let chars: Vec<char> = text.chars().collect();
                const WINDOW_CHARS: usize = 24;

                if chars.len() <= WINDOW_CHARS {
                    return glib::ControlFlow::Continue;
                }

                let pause = pause_ref.get();
                if pause > 0 {
                    pause_ref.set(pause - 1);
                    return glib::ControlFlow::Continue;
                }

                let next_off = offset_ref.get() + 1;
                if next_off + WINDOW_CHARS > chars.len() {
                    offset_ref.set(0);
                    pause_ref.set(10); // Pause at end before wrapping
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

        // Interruptible SpringAnimation for pill width/height morphing
        let dims_for_cb = Rc::clone(&current_dims);
        let start_for_cb = Rc::clone(&start_dims);
        let target_for_cb = Rc::clone(&target_dims);
        let pill_for_cb = pill_frame.clone();
        let window_weak = window.downgrade();

        let anim_target = adw::CallbackAnimationTarget::new(move |progress| {
            let interpolated = start_for_cb.get().lerp(target_for_cb.get(), progress);
            dims_for_cb.set(interpolated);

            pill_for_cb.set_width_request(interpolated.width.round() as i32);
            pill_for_cb.set_height_request(interpolated.height.round() as i32);

            if let Some(win) = window_weak.upgrade() {
                update_wayland_input_region(&win, interpolated);
            }
        });

        let spring_params = adw::SpringParams::new(0.88, 1.0, 260.0);
        let spring_anim =
            adw::SpringAnimation::new(&pill_frame, 0.0, 1.0, spring_params, anim_target);
        spring_anim.set_clamp(false);

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
            controls_box,
            play_pause_btn,
            active_player_id,
            full_title,
            marquee_offset,
            marquee_pause_ticks,
            slider_icon,
            slider_bar,
            slider_pct_label,
            current_dims,
            spring_anim,
            start_dims,
            target_dims,
        }
    }

    #[must_use]
    pub fn root_widget(&self) -> &gtk4::Box {
        &self.outer_box
    }

    /// Updates the pill's active view, metadata, slider fill, and smoothly retargets the morph spring.
    pub fn apply_transition(
        &self,
        effect: &TransitionEffect,
        player_id: Option<&str>,
        status: PlaybackStatus,
        metadata: Option<&TrackMetadata>,
    ) {
        *self.active_player_id.borrow_mut() = player_id.map(str::to_owned);
        self.start_dims.set(self.current_dims.get());
        self.target_dims.set(effect.dimensions);
        self.pill_frame.remove_css_class("mute-pulse");

        match &effect.state {
            PillState::Hidden => {
                self.pill_frame.set_opacity(0.0);
            }
            PillState::Idle => {
                self.pill_frame.set_opacity(1.0);
                self.pill_frame.remove_css_class("expanded");
                self.idle_label.set_label("●  Islet");
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
                    let mut current_full = self.full_title.borrow_mut();
                    if *current_full != meta.title {
                        *current_full = meta.title.clone();
                        self.marquee_offset.set(0);
                        self.marquee_pause_ticks.set(8);
                        self.title_label.set_label(&meta.title);
                    }
                    self.artist_label.set_label(&meta.artist);

                    if let Some(ref art_path) = meta.art_url {
                        self.art_picture.set_filename(Some(art_path));
                    }
                }
                self.view_stack.set_visible_child_name("media");
            }
            PillState::Slider(SliderKind::Volume {
                percent,
                muted,
                pulse,
            }) => {
                self.pill_frame.set_opacity(1.0);
                self.pill_frame.add_css_class("expanded");
                if *pulse {
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
                self.slider_bar
                    .set_fraction(if *muted { 0.0 } else { (f64::from(*percent) / 100.0).clamp(0.0, 1.0) });
                self.slider_pct_label
                    .set_label(&if *muted { "Mute".to_owned() } else { format!("{percent}%") });
                self.view_stack.set_visible_child_name("slider");
            }
            PillState::Slider(SliderKind::Brightness { percent }) => {
                self.pill_frame.set_opacity(1.0);
                self.pill_frame.add_css_class("expanded");
                self.slider_icon.set_label("☀️");
                self.slider_bar
                    .set_fraction((f64::from(*percent) / 100.0).clamp(0.0, 1.0));
                self.slider_pct_label.set_label(&format!("{percent}%"));
                self.view_stack.set_visible_child_name("slider");
            }
            PillState::Notification { title, body } => {
                self.pill_frame.set_opacity(1.0);
                self.pill_frame.add_css_class("expanded");
                self.idle_label.set_label(&format!("🔔 {title} — {body}"));
                self.view_stack.set_visible_child_name("idle");
            }
        }

        let current_velocity = self.spring_anim.velocity();
        self.spring_anim.set_initial_velocity(current_velocity);
        self.spring_anim.play();
    }
}

fn update_wayland_input_region(window: &gtk4::ApplicationWindow, dims: PillDimensions) {
    let Some(surface) = window.surface() else {
        return;
    };

    let width = dims.width.round().max(1.0) as i32;
    let height = dims.height.round().max(1.0) as i32;
    let pill_x = (MAX_SURFACE_WIDTH - width) / 2;
    let pill_y = SURFACE_SHADOW_PADDING / 2;

    if let Ok(region) = build_rounded_pill_region(pill_x, pill_y, width, height) {
        surface.set_input_region(&region);
        trace!(pill_x, pill_y, width, height, "Updated Wayland capsule input region");
    }
}

fn build_rounded_pill_region(
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> Result<Region, cairo::Error> {
    let mask_w = x + width + 2;
    let mask_h = y + height + 2;
    let mask = ImageSurface::create(Format::A8, mask_w, mask_h)?;
    let cr = Context::new(&mask)?;

    let radius = f64::from(height) * 0.5;
    let fx = f64::from(x);
    let fy = f64::from(y);
    let fw = f64::from(width);
    let fh = f64::from(height);

    cr.new_sub_path();
    cr.arc(
        fx + fw - radius,
        fy + radius,
        radius,
        -std::f64::consts::FRAC_PI_2,
        std::f64::consts::FRAC_PI_2,
    );
    cr.arc(
        fx + radius,
        fy + radius,
        radius,
        std::f64::consts::FRAC_PI_2,
        3.0 * std::f64::consts::FRAC_PI_2,
    );
    cr.close_path();
    cr.set_source_rgba(1.0, 1.0, 1.0, 1.0);
    cr.fill()?;
    drop(cr);

    mask.flush();
    Ok(Region::create_from_surface(&mask)?)
}
