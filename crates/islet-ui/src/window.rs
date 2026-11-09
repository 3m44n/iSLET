//! Wayland `wlr-layer-shell` window with multi-monitor binding and hot-reloaded config/CSS.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use gdk4::prelude::*;
use gtk4::prelude::*;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use islet_core::{
    EventBus, IsletConfig, IsletEvent, MonitorFollow, PillState, PlaybackStatus, StateMachine,
    DEFAULT_MARGIN_TOP, MAX_SURFACE_HEIGHT, MAX_SURFACE_WIDTH,
};
use islet_sources::{spawn_background_sources, MprisCommand, SourceHandles, VisualizerControl};
use tracing::{debug, info};

use crate::pill::PillSurface;

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

    // Dynamic CSS provider for hot-reloaded `style.css`
    let user_css_provider = gtk4::CssProvider::new();
    if let Some(display) = gdk4::Display::default() {
        gtk4::style_context_add_provider_for_display(
            &display,
            &user_css_provider,
            gtk4::STYLE_PROVIDER_PRIORITY_USER,
        );
    }

    let EventBus {
        sender,
        mut receiver,
    } = EventBus::new();

    let SourceHandles {
        mpris_tx,
        visualizer_tx,
    } = spawn_background_sources(sender.clone());

    let pill = Rc::new(PillSurface::new(
        &window,
        sender.clone(),
        mpris_tx.clone(),
        config.visualizer_bars,
    ));
    window.set_child(Some(pill.root_widget()));
    window.present();

    info!("Islet layer-shell window, hardware watchers, and hot-reload online");

    let state_machine = Rc::new(RefCell::new(StateMachine::new(config)));
    let pill_for_loop = Rc::clone(&pill);
    let sender_for_timers = sender;
    let window_for_loop = window.clone();

    glib::spawn_future_local(async move {
        while let Some(event) = receiver.recv().await {
            if let IsletEvent::SpectrumFrame(ref bars) = event {
                pill_for_loop.update_spectrum(bars);
                continue;
            }

            debug!(?event, "Processing IsletEvent");

            match &event {
                IsletEvent::PillClicked => {
                    if let Some((player_id, _, _)) = state_machine.borrow().active_player() {
                        let _ = mpris_tx.send(MprisCommand::PlayPause(player_id.to_owned()));
                    }
                }
                IsletEvent::PillRightClicked => {
                    if let Some((player_id, _, _)) = state_machine.borrow().active_player() {
                        let _ = mpris_tx.send(MprisCommand::Raise(player_id.to_owned()));
                    }
                }
                IsletEvent::CssReloaded(css) => {
                    user_css_provider.load_from_string(css);
                }
                IsletEvent::FocusedMonitorChanged(connector) => {
                    if state_machine.borrow().config().monitor_follow == MonitorFollow::Focused {
                        bind_window_to_connector(&window_for_loop, Some(connector.as_str()));
                    }
                }
                IsletEvent::ConfigReloaded(cfg) => {
                    window_for_loop.set_margin(Edge::Top, cfg.margin_top.max(0));
                    pill_for_loop.set_reduce_motion(cfg.reduce_motion);
                    match &cfg.monitor_follow {
                        MonitorFollow::Primary => bind_window_to_connector(&window_for_loop, None),
                        MonitorFollow::Output(name) => {
                            bind_window_to_connector(&window_for_loop, Some(name.as_str()));
                        }
                        MonitorFollow::Focused => {}
                    }
                }
                _ => {}
            }

            let (effect, active_meta, active_id, active_status, bars_cfg) = {
                let mut sm = state_machine.borrow_mut();
                let fx = sm.handle_event(event);
                let (id, status, meta) = sm
                    .active_player()
                    .map_or((None, PlaybackStatus::Stopped, None), |(i, s, m)| {
                        (Some(i.to_owned()), s, m.cloned())
                    });
                (fx, meta, id, status, sm.config().visualizer_bars)
            };

            let viz_active = matches!(effect.state, PillState::Media { .. })
                && active_status == PlaybackStatus::Playing;
            let _ = visualizer_tx.send(VisualizerControl {
                active: viz_active,
                bars: bars_cfg,
            });

            pill_for_loop.apply_transition(
                &effect,
                active_id.as_deref(),
                active_status,
                active_meta.as_ref(),
                viz_active,
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

/// Binds the `gtk4-layer-shell` surface to a target monitor by connector name (e.g. `"DP-1"`),
/// or the primary/first monitor when `target_connector` is `None`.
fn bind_window_to_connector(window: &gtk4::ApplicationWindow, target_connector: Option<&str>) {
    let Some(display) = gdk4::Display::default() else {
        return;
    };
    let monitors = display.monitors();

    for i in 0..monitors.n_items() {
        let Some(obj) = monitors.item(i) else { continue };
        let Ok(monitor) = obj.downcast::<gdk4::Monitor>() else {
            continue;
        };

        match target_connector {
            Some(wanted) => {
                if monitor.connector().as_deref() == Some(wanted) {
                    window.set_monitor(Some(&monitor));
                    info!(connector = wanted, "Moved Islet overlay to target monitor");
                    return;
                }
            }
            None => {
                window.set_monitor(Some(&monitor));
                return;
            }
        }
    }
}
