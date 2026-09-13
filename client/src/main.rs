//! A desktop client for the marks server.
//!
//! The window is a launcher in the shape of `drun`: one search field, the marks it matches
//! underneath with their favicons, and a sign-in dialog until there is a session. Each module
//! covers one concern:
//!
//! - [`api`] speaks to the server and is blocking, so it is only ever called off the UI thread,
//! - [`icons`] downloads favicons on a pool of worker threads and caches them as textures,
//! - [`app`] is the window itself: state, keybindings and drawing,
//! - [`mark`] is the shape a mark has on both sides of the wire,
//! - [`title`] reads the title of the page a link points at, to name the mark after it,
//! - [`config`] is the settings the window runs by, read from the config directory,
//! - [`fonts`] is the order the window asks its fonts in, which is a setting,
//! - [`search`] is how a query finds a mark, as a subsequence rather than a substring,
//! - [`session_file`] keeps the session under the local data directory, so that signing in
//!   survives the window being closed.

mod api;
mod app;
mod config;
mod fonts;
mod icons;
mod mark;
mod search;
mod session_file;
mod title;

// A page served from this machine, for the tests that have to fetch one. Declared here rather
// than inside either test module because both of them need it.
#[cfg(test)]
mod test_page;

use app::{MarksApp, starting_session};
use config::Config;

fn main() -> eframe::Result {
    // Read once, here, rather than inside the window: the environment and the files on disk are
    // the things a window should not be reaching for itself.
    let config = Config::load();
    let (base_url, token) = starting_session();

    let options = eframe::NativeOptions {
        // The window opens the size the settings ask for, and is resizable unless they say
        // otherwise (see `Window::viewport`).
        viewport: config.window.viewport(),
        ..Default::default()
    };

    eframe::run_native(
        "Marks",
        options,
        Box::new(move |cc| Ok(Box::new(MarksApp::new(&cc.egui_ctx, base_url, token, config)))),
    )
}
