//! Islet Wayland Overlay Daemon — Entry Point (Milestone 1 Skeleton).

mod pill;
mod window;

use glib::ExitCode;
use gtk4::prelude::*;
use tracing::info;
use tracing_subscriber::EnvFilter;

const APP_ID: &str = "dev.islet.Overlay";

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    info!("Starting Islet Wayland overlay daemon (Milestone 1 Skeleton)");

    let app = adw::Application::builder().application_id(APP_ID).build();

    app.connect_startup(|_| {
        load_stylesheet();
    });

    app.connect_activate(window::build_overlay_window);

    app.run()
}

/// Loads `assets/style.css` (or `$XDG_CONFIG_HOME/islet/style.css` fallback) into the default display.
fn load_stylesheet() {
    let Some(display) = gdk4::Display::default() else {
        tracing::warn!("No default GDK display found while loading CSS");
        return;
    };

    let provider = gtk4::CssProvider::new();
    let bundled_css = include_str!("../../../assets/style.css");
    provider.load_from_string(bundled_css);

    gtk4::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}
