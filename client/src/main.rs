//! A desktop client for the marks server.
//!
//! The window is a launcher in the shape of `drun`: one search field, the marks it matches
//! underneath with their favicons, and a sign-in dialog until there is a session. Each module
//! covers one concern:
//!
//! - [`api`] speaks to the server and is blocking, so it is only ever called off the UI thread,
//! - [`icons`] downloads favicons on a pool of worker threads and caches them as textures,
//! - [`app`] is the window itself: state, keybindings and drawing,
//! - [`mark`] is the shape a mark has on both sides of the wire.

mod api;
mod app;
mod icons;
mod mark;

use eframe::egui;

use app::MarksApp;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            // A launcher is a panel, not a document: a narrow window and no title bar. The
            // strip at the top of the panel is what moves it (see `MarksApp::drag_handle`).
            .with_title("Marks")
            .with_inner_size([620.0, 440.0])
            .with_min_inner_size([420.0, 240.0])
            .with_decorations(false),
        ..Default::default()
    };

    eframe::run_native(
        "Marks",
        options,
        Box::new(|cc| Ok(Box::new(MarksApp::new(&cc.egui_ctx)))),
    )
}
