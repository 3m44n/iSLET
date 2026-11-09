//! Wayland `wlr-layer-shell` window setup driven by the `islet-core` EventBus and StateMachine.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use gtk4::prelude::*;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use islet_core::{
    EventBus, IsletConfig, IsletEvent, PlaybackStatus, StateMachine, TrackMetadata,
    DEFAULT_MARGIN_TOP, MAX_SURFACE_HEIGHT, MAX_SURFACE_WIDTH,
};
use tracing::{debug, info};

use crate::pill::PillSurface;

/// Constructs the top-center `Overlay` layer-shell window and attaches the event-driven `PillSurface`.
pub fn build_overlay_window(app: &adw::Application) {
    let config = IsletConfig::default();
    let window = gtk4::ApplicationWindow::builder()
        .application(app)
        .title("Islet")
        .decorated(false)
        .resizable(false)
        .default_width(MAX_SURFACE_WIDTH)
        .default_height(MAX_SURFACE_HEIGHT)
        .css_classes(["islet-window"])
        .build();

    window.init_layer_shell();
    window.set_namespace("islet");
    window.set_layer(Layer::Overlay);

    window.set_anchor(Edge::Top, true);
    window.set_anchor(Edge::Bottom, false);
    window.set_anchor(Edge::Left, false);
    window.set_anchor(Edge::Right, false);
    window.set_margin(Edge::Top, config.margin_top.max(DEFAULT_MARGIN_TOP));

    window.set_keyboard_mode(KeyboardMode::None);
    window.set_exclusive_zone(0);

    let EventBus {
        sender,
        mut receiver,
    } = EventBus::new();

    let pill = Rc::new(PillSurface::new(&window, sender.clone()));
    window.set_child(Some(pill.root_widget()));
    window.present();

    info!("Islet layer-shell window and EventBus online");

    // Spawn the main-context event consumer that drives the StateMachine.
    let state_machine = Rc::new(RefCell::new(StateMachine::new(config)));
    let pill_for_loop = Rc::clone(&pill);
    let sender_for_timers = sender.clone();

    glib::spawn_future_local(async move {
        while let Some(event) = receiver.recv().await {
            debug!(?event, "Processing IsletEvent");
            let effect = state_machine.borrow_mut().handle_event(event);
            pill_for_loop.apply_transition(&effect);

            if let Some((seq, timeout_ms)) = effect.schedule_collapse {
                let tx = sender_for_timers.clone();
                glib::timeout_add_local_once(Duration::from_millis(timeout_ms), move || {
                    tx.send(IsletEvent::AutoCollapse { seq });
                });
            }
        }
    });

    // Milestone 2 demo sequence: cycle through Media -> Volume override -> Restore Media -> Collapse
    let demo_tx = sender;
    let step = Rc::new( std::cell::Cell::new(0_u8));
    glib::timeout_add_local(Duration::from_millis(2500), move || {
        let current_step = step.get();
        step.set((current_step + 1) % 3);

        match current_step {
            0 => {
                demo_tx.send(IsletEvent::MprisUpdate {
                    player_id: "spotify".into(),
                    status: PlaybackStatus::Playing,
                    metadata: Some(TrackMetadata {
                        title: "Midnight City".into(),
                        artist: "M83".into(),
                        ..Default::default()
                    }),
                });
            }
            1 => {
                demo_tx.send(IsletEvent::VolumeChanged {
                    percent: 72,
                    muted: false,
                });
            }
            _ => {
                demo_tx.send(IsletEvent::BrightnessChanged { percent: 85 });
            }
        }
        glib::ControlFlow::Continue
    });
}
