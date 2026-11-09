//! Wayland `wlr-layer-shell` window wired to MPRIS2, Audio Volume, and Backlight sources.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use gtk4::prelude::*;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use islet_core::{
    EventBus, IsletConfig, IsletEvent, StateMachine, DEFAULT_MARGIN_TOP, MAX_SURFACE_HEIGHT,
    MAX_SURFACE_WIDTH,
};
use islet_sources::{spawn_background_sources, MprisCommand};
use tracing::{debug, info};

use crate::pill::PillSurface;

/// Constructs the top-center `Overlay` layer-shell window and connects all live system sources.
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

    // Start real MPRIS2, PipeWire/pactl Volume, and Backlight inotify sources.
    let mpris_cmd_tx = spawn_background_sources(sender.clone());

    let pill = Rc::new(PillSurface::new(
        &window,
        sender.clone(),
        mpris_cmd_tx.clone(),
    ));
    window.set_child(Some(pill.root_widget()));
    window.present();

    info!("Islet layer-shell window and live hardware/MPRIS sources online");

    let state_machine = Rc::new(RefCell::new(StateMachine::new(config)));
    let pill_for_loop = Rc::clone(&pill);
    let sender_for_timers = sender;

    glib::spawn_future_local(async move {
        while let Some(event) = receiver.recv().await {
            debug!(?event, "Processing IsletEvent");

            // Handle left/right clicks on the active MPRIS player before state transition.
            match &event {
                IsletEvent::PillClicked => {
                    if let Some((player_id, _, _)) = state_machine.borrow().active_player() {
                        let _ = mpris_cmd_tx.send(MprisCommand::PlayPause(player_id.to_owned()));
                    }
                }
                IsletEvent::PillRightClicked => {
                    if let Some((player_id, _, _)) = state_machine.borrow().active_player() {
                        let _ = mpris_cmd_tx.send(MprisCommand::Raise(player_id.to_owned()));
                    }
                }
                _ => {}
            }

            let (effect, active_meta, active_id, active_status) = {
                let mut sm = state_machine.borrow_mut();
                let fx = sm.handle_event(event);
                let (id, status, meta) = sm
                    .active_player()
                    .map_or((None, islet_core::PlaybackStatus::Stopped, None), |(i, s, m)| {
                        (Some(i.to_owned()), s, m.cloned())
                    });
                (fx, meta, id, status)
            };

            pill_for_loop.apply_transition(
                &effect,
                active_id.as_deref(),
                active_status,
                active_meta.as_ref(),
            );

            if let Some((seq, timeout_ms)) = effect.schedule_collapse {
                let tx = sender_for_timers.clone();
                glib::timeout_add_local_once(Duration::from_millis(timeout_ms), move || {
                    tx.send(IsletEvent::AutoCollapse { seq });
                });
            }
        }
    });
}
