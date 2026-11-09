//! Animated pill container and fractional-scale-aware Wayland input region mask.

use std::cell::Cell;
use std::rc::Rc;

use adw::prelude::*;
use cairo::{Context, Format, ImageSurface, Region};
use gtk4::prelude::*;
use islet_core::{
    EventSender, IsletEvent, PillDimensions, PillState, SliderKind, TransitionEffect,
    MAX_SURFACE_WIDTH, SURFACE_SHADOW_PADDING,
};
use tracing::trace;

/// Manages the visual pill widget, spring/timed size morphs, and Cairo input-region mask.
pub struct PillSurface {
    outer_box: gtk4::Box,
    pill_frame: gtk4::Box,
    status_label: gtk4::Label,
    current_dims: Rc<Cell<PillDimensions>>,
    spring_anim: adw::SpringAnimation,
    start_dims: Rc<Cell<PillDimensions>>,
    target_dims: Rc<Cell<PillDimensions>>,
}

impl PillSurface {
    #[must_use]
    pub fn new(window: &gtk4::ApplicationWindow, sender: EventSender) -> Self {
        let initial = PillDimensions::IDLE;
        let current_dims = Rc::new(Cell::new(initial));
        let start_dims = Rc::new(Cell::new(initial));
        let target_dims = Rc::new(Cell::new(initial));

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

        let status_label = gtk4::Label::builder()
            .label("Islet • Idle")
            .hexpand(true)
            .halign(gtk4::Align::Center)
            .valign(gtk4::Align::Center)
            .css_classes(["islet-label"])
            .build();

        pill_frame.append(&status_label);
        outer_box.append(&pill_frame);

        // Left-click and hover controllers wired directly to EventBus.
        let click_tx = sender.clone();
        let click_gesture = gtk4::GestureClick::new();
        click_gesture.connect_released(move |_, _, _, _| {
            click_tx.send(IsletEvent::PillClicked);
        });
        pill_frame.add_controller(click_gesture);

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
            status_label,
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

    /// Applies a `TransitionEffect` from the state machine, smoothly retargeting mid-flight animations.
    pub fn apply_transition(&self, effect: &TransitionEffect) {
        self.start_dims.set(self.current_dims.get());
        self.target_dims.set(effect.dimensions);

        match &effect.state {
            PillState::Hidden => {
                self.pill_frame.remove_css_class("expanded");
                self.status_label.set_label("");
            }
            PillState::Idle => {
                self.pill_frame.remove_css_class("expanded");
                self.status_label.set_label("Islet • Idle");
            }
            PillState::Media {
                expanded,
                controls_visible,
            } => {
                if *expanded || *controls_visible {
                    self.pill_frame.add_css_class("expanded");
                } else {
                    self.pill_frame.remove_css_class("expanded");
                }
                let text = if *controls_visible {
                    "⏮  ⏯  ⏭   Media Controls"
                } else if *expanded {
                    "🎵 Media Expanded"
                } else {
                    "🎵 Playing"
                };
                self.status_label.set_label(text);
            }
            PillState::Slider(SliderKind::Volume { percent, muted, .. }) => {
                self.pill_frame.add_css_class("expanded");
                let icon = if *muted { "🔇" } else { "🔊" };
                self.status_label
                    .set_label(&format!("{icon} Volume {percent}%"));
            }
            PillState::Slider(SliderKind::Brightness { percent }) => {
                self.pill_frame.add_css_class("expanded");
                self.status_label
                    .set_label(&format!("☀️ Brightness {percent}%"));
            }
            PillState::Notification { title, body } => {
                self.pill_frame.add_css_class("expanded");
                self.status_label.set_label(&format!("{title}: {body}"));
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
