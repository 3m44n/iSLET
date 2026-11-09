//! Wayland `wlr-layer-shell` window setup and periodic demo timer for Milestone 1.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use gtk4::prelude::*;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use islet_core::{
    PillDimensions, DEFAULT_MARGIN_TOP, MAX_SURFACE_HEIGHT, MAX_SURFACE_WIDTH,
};
use tracing::{debug, info};

use crate::pill::PillSurface;

/// Constructs the top-center `Overlay` layer-shell window and attaches the animated `PillSurface`.
pub fn build_overlay_window(app: &adw::Application) {
    let window = gtk4::ApplicationWindow::builder()
        .application(app)
        .title("Islet")
        .decorated(false)
        .resizable(false)
        .default_width(MAX_SURFACE_WIDTH)
        .default_height(MAX_SURFACE_HEIGHT)
        .css_classes(["islet-window"])
        .build();

    // 1. Initialize wlr-layer-shell before the surface is realized.
    window.init_layer_shell();
    window.set_namespace("islet");
    window.set_layer(Layer::Overlay);

    // 2. Anchor exclusively to Top so the compositor centers horizontally.
    window.set_anchor(Edge::Top, true);
    window.set_anchor(Edge::Bottom, false);
    window.set_anchor(Edge::Left, false);
    window.set_anchor(Edge::Right, false);
    window.set_margin(Edge::Top, DEFAULT_MARGIN_TOP);

    // 3. Never steal keyboard focus and never push tiled windows (`exclusive_zone = 0`).
    window.set_keyboard_mode(KeyboardMode::None);
    window.set_exclusive_zone(0);

    // 4. Build the animated pill surface widget and mount it inside the fixed overlay envelope.
    let pill = Rc::new(PillSurface::new(&window));
    window.set_child(Some(pill.root_widget()));

    window.present();
    info!(
        "Layer-shell window initialized: layer=Overlay, anchor=Top, exclusive_zone=0, margin_top={DEFAULT_MARGIN_TOP}"
    );

    // 5. Milestone 1 verification timer: toggle between Idle (120x32) and Media (340x76) every 3s.
    let expanded = Rc::new(Cell::new(false));
    let pill_timer = Rc::clone(&pill);
    glib::timeout_add_local(Duration::from_secs(3), move || {
        let next_expanded = !expanded.get();
        expanded.set(next_expanded);

        let target = if next_expanded {
            PillDimensions::MEDIA_EXPANDED
        } else {
            PillDimensions::IDLE
        };

        debug!(
            expanded = next_expanded,
            target_width = target.width,
            target_height = target.height,
            "Retargeting Milestone 1 pill morph animation"
        );
        pill_timer.morph_to(target, next_expanded);
        glib::ControlFlow::Continue
    });
}
