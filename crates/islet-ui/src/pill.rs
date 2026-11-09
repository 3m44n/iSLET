//! Animated pill container and fractional-scale-aware Wayland input region mask.

use std::cell::Cell;
use std::rc::Rc;

use adw::prelude::*;
use cairo::{Context, Format, ImageSurface, Region};
use gtk4::prelude::*;
use islet_core::{PillDimensions, MAX_SURFACE_WIDTH, SURFACE_SHADOW_PADDING};
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
    pub fn new(window: &gtk4::ApplicationWindow) -> Self {
        let initial = PillDimensions::IDLE;
        let current_dims = Rc::new(Cell::new(initial));
        let start_dims = Rc::new(Cell::new(initial));
        let target_dims = Rc::new(Cell::new(initial));

        // Outer fixed container reserves static surface bounds so shadow/glow never clip
        // and Wayland surface size doesn't thrash on every animation tick.
        let outer_box = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Vertical)
            .halign(gtk4::Align::Center)
            .valign(gtk4::Align::Start)
            .margin_top(SURFACE_SHADOW_PADDING / 2)
            .build();

        // The actual rounded pill box whose width/height animate smoothly.
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

        // Click controller inside the pill bounds to verify click reception vs outside pass-through.
        let click_gesture = gtk4::GestureClick::new();
        click_gesture.connect_released(|_, _, x, y| {
            tracing::info!(x, y, "Pill clicked inside active input region!");
        });
        pill_frame.add_controller(click_gesture);

        // Configure interruptible SpringAnimation driven by the GTK4 frame clock.
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

        // Critically damped spring (~320ms settling time) for fluid Dynamic Island feel.
        let spring_params = adw::SpringParams::new(0.88, 1.0, 260.0);
        let spring_anim = adw::SpringAnimation::new(&pill_frame, 0.0, 1.0, spring_params, anim_target);
        spring_anim.set_clamp(false);

        // Apply the initial click-through input region as soon as the GDK surface maps.
        let dims_on_map = Rc::clone(&current_dims);
        window.connect_map(move |win| {
            update_wayland_input_region(win, dims_on_map.get());
        });

        // Re-apply input region when fractional scale factor changes across monitors.
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

    /// Smoothly morphs the pill to `target`, retargeting seamlessly if called mid-flight.
    pub fn morph_to(&self, target: PillDimensions, expanded: bool) {
        // Capture current instantaneous dimensions so mid-animation retargeting never snaps.
        self.start_dims.set(self.current_dims.get());
        self.target_dims.set(target);

        if expanded {
            self.pill_frame.add_css_class("expanded");
            self.status_label.set_label("Islet • Media Expanded (340×76)");
        } else {
            self.pill_frame.remove_css_class("expanded");
            self.status_label.set_label("Islet • Idle");
        }

        // Preserve velocity if interrupted mid-flight.
        let current_velocity = self.spring_anim.velocity();
        self.spring_anim.set_initial_velocity(current_velocity);
        self.spring_anim.play();
    }
}

/// Restricts the Wayland surface input region strictly to the rounded pill capsule.
///
/// Any pointer click or scroll event outside the rasterized capsule mask passes straight
/// through to underlying windows on the desktop.
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

/// Rasterizes a true rounded capsule on an `A8` alpha mask surface and converts it into a `cairo::Region`.
///
/// Using `ImageSurface::create(Format::A8, ...)` + `Region::create_from_surface` guarantees that
/// even the corner quadrants outside the circular pill ends are click-through.
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

    // Draw capsule path with semicircular left and right caps.
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
