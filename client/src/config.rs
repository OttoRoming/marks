//! The settings the window runs by, and the file they are read from.
//!
//! They live under the **config** directory — `$XDG_CONFIG_HOME/marks-client/config.toml` on
//! Linux, `~/.config/marks-client/config.toml` by default — which is where settings someone is
//! meant to edit belong. The session is the other way round: it is state the client owns rather
//! than something to be edited, so that is kept in the local data directory (see `session_file`).
//!
//! The file is TOML, and the panel behind Ctrl+, writes it. Nothing here is worth refusing to
//! open a window over: a file that is missing, unreadable or nonsense leaves the client running
//! on the same defaults it would have had without a configuration system at all.

use std::fs;
use std::path::{Path, PathBuf};

use eframe::egui;
use serde::{Deserialize, Serialize};

use crate::fonts;

/// The directory this client's settings live in, under the config directory.
const APP_DIR: &str = "marks-client";

/// The file the settings live in.
const FILE_NAME: &str = "config.toml";

/// How narrow the window may be asked to be.
///
/// The floor the window has always had as its minimum size, so that no setting can make it too
/// small to draw the list in.
pub const MIN_WIDTH: u32 = 420;

/// How short it may be asked to be. See [`MIN_WIDTH`].
pub const MIN_HEIGHT: u32 = 240;

/// How large it may be asked to be.
///
/// A guard against a mistyped number rather than a limit anyone should meet: no display is this
/// big, and a window that is would be a window nobody could get out of.
pub const MAX_SIDE: u32 = 16384;

/// The comment a settings file starts with.
///
/// Written on every save, and written as the file is written: the client regenerates the whole
/// file from the settings it holds, so it is the client's file, and comments added by hand
/// around the settings are not kept.
const HEADER: &str = "\
# Settings for the marks client.
#
# Written by the client: Ctrl+, opens a panel that changes these settings and writes them back
# here, so comments added by hand do not survive. The settings themselves can be edited by hand,
# and are read the next time the window opens.

";

/// Everything the client can be configured with, as the file spells it.
#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    /// What the window is allowed to be.
    pub window: Window,

    /// What it draws with.
    pub fonts: Fonts,

    /// The file these were read from, and are written back to.
    ///
    /// `None` when the system has no config directory to speak of, which leaves the window on
    /// its defaults and writing nothing anywhere.
    #[serde(skip)]
    path: Option<PathBuf>,
}

/// The fonts the window draws with, in the order it asks them.
#[derive(Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct Fonts {
    /// The fonts to draw with, most wanted first.
    ///
    /// The first one that has the character being drawn is the one that draws it, so the first
    /// name here is the font nearly all of the window ends up in. A name is one of the fonts egui
    /// carries ([`fonts::AVAILABLE`]) or a family this machine has; one that is neither is left
    /// out of the order when the window is drawn, rather than drawn as boxes. The window is set out
    /// of the settings in the panel behind Ctrl+, which searches every font the machine has, so
    /// this is not usually written by hand.
    pub priority: Vec<String>,
}

impl Default for Fonts {
    fn default() -> Self {
        Self {
            priority: fonts::default_priority(),
        }
    }
}

impl Fonts {
    /// Holds the list to names worth drawing with: each named once, nothing empty, and never
    /// nothing at all.
    ///
    /// A name the client does not carry is left where it is rather than dropped, because it may
    /// well be a font on this machine — which cannot be told apart from a name that is no font at
    /// all without reading every font the machine has, and that is done on a worker thread some
    /// while later (see `fonts::SystemFonts`). A name that turns out to be nothing is left out of
    /// the order when it is put onto the window, and the panel shows it as it is.
    ///
    /// A list that leaves nothing at all becomes the default, because a family with no fonts in it
    /// is a window with no text in it.
    ///
    /// Called as the file is read, and by the panel once it has changed the order.
    pub(crate) fn settle(&mut self) {
        let mut kept: Vec<String> = Vec::new();

        for name in self.priority.drain(..) {
            let name = name.trim();

            if name.is_empty() {
                continue;
            }
            if kept.iter().any(|already| already == name) {
                continue;
            }

            kept.push(name.to_owned());
        }

        self.priority = if kept.is_empty() {
            fonts::default_priority()
        } else {
            kept
        };
    }
}

/// What the window is allowed to be: how big it opens, and whether that is also how big it stays.
#[derive(Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct Window {
    /// Whether the window keeps the size below instead of being resizable.
    pub fixed_size: bool,

    /// How wide the window opens, in egui's points.
    ///
    /// A point is a logical pixel, so on a display with a scale factor of two a window 620
    /// points wide covers 1240 physical pixels. A window that is fixed to a size also stays at
    /// it; one that is not is free to be resized once it is up.
    pub width: u32,

    /// How tall the window opens. See [`Window::width`].
    pub height: u32,
}

impl Default for Window {
    /// The size the window has always opened at: a launcher's panel, free to be resized.
    fn default() -> Self {
        Self {
            fixed_size: false,
            width: 620,
            height: 440,
        }
    }
}

impl Window {
    /// The window as egui should open it.
    ///
    /// A window that keeps its size is also one that cannot be resized: that is what a fixed
    /// size means, both to whoever writes it in the file and to whoever ticks it in the panel.
    pub fn viewport(&self) -> egui::ViewportBuilder {
        let viewport = egui::ViewportBuilder::default()
            // A launcher is a panel, not a document: no title bar, and a narrow window. The
            // strip at the top is what moves it (see `MarksApp::drag_handle`).
            .with_title("Marks")
            .with_decorations(false)
            .with_inner_size(self.size())
            .with_resizable(!self.fixed_size);

        if self.fixed_size {
            // Held by both ends rather than by "not resizable" alone, which a window manager is
            // free to ignore; this is the same pinning the panel applies (`apply_window_settings`).
            viewport
                .with_min_inner_size(self.size())
                .with_max_inner_size(self.size())
        } else {
            viewport.with_min_inner_size(floor())
        }
    }

    /// The size this asks the window to be.
    pub fn size(&self) -> egui::Vec2 {
        egui::vec2(self.width as f32, self.height as f32)
    }

    /// Holds the numbers to something a window can actually be.
    ///
    /// A file is written by hand, so it can say anything: a size under the floor would hide the
    /// list, and one mistyped extra digit would ask for a window larger than any display. The
    /// panel is where these numbers are edited once the window is up, so this is public within
    /// the client rather than only used as the file is read.
    pub(crate) fn clamp(&mut self) {
        self.width = self.width.clamp(MIN_WIDTH, MAX_SIDE);
        self.height = self.height.clamp(MIN_HEIGHT, MAX_SIDE);
    }
}

impl Config {
    /// Reads the settings from wherever this system keeps them.
    pub fn load() -> Self {
        let Some(path) = path() else {
            eprintln!("marks-client: no config directory; using the default settings");
            return Self::default();
        };

        Self::load_from(path)
    }

    /// Reads the settings from `path`, falling back to the defaults for anything missing or
    /// unreadable.
    ///
    /// The worst a bad file can do is give the window its default size, and the panel writes a
    /// clean one the next time it is used.
    pub fn load_from(path: PathBuf) -> Self {
        let config = match fs::read_to_string(&path) {
            Ok(contents) => match toml::from_str::<Self>(&contents) {
                Ok(config) => config,
                Err(error) => {
                    eprintln!(
                        "marks-client: {} is not a settings file ({error}); using the defaults",
                        path.display()
                    );
                    Self::default()
                }
            },
            // No file at all is the ordinary case for a client nobody has configured yet.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(error) => {
                eprintln!("marks-client: could not read {}: {error}", path.display());
                Self::default()
            }
        };

        // Whatever the file said, what comes back is a window that can be drawn.
        Self {
            path: Some(path),
            ..config
        }
        .settled()
    }

    /// Holds the settings to what the window can actually do.
    ///
    /// A file is written by hand as often as by the panel, so it can ask for a window that is
    /// too small to draw in, or name a font this client does not have.
    fn settled(mut self) -> Self {
        self.window.clamp();
        self.fonts.settle();
        self
    }

    /// Where these settings came from, for the panel to show.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Writes the settings back to the file they came from.
    ///
    /// A write that fails is reported and otherwise ignored: the settings are in effect for as
    /// long as the window is open whether or not they reached the disk.
    pub fn save(&self) {
        let Some(path) = &self.path else {
            return;
        };

        if let Err(error) = self.save_to(path) {
            eprintln!("marks-client: could not write {}: {error}", path.display());
        }
    }

    /// Writes these to `path`, making the directory if it is not there.
    ///
    /// The whole file is regenerated from the settings, rather than edited in place: the client
    /// owns this file, and `toml` through serde writes values rather than documents. Settings
    /// put there by hand are read — the file is not the only way in, but it is not the way the
    /// client writes. See [`HEADER`], which the file says for itself.
    fn save_to(&self, path: &Path) -> std::io::Result<()> {
        let body = toml::to_string_pretty(self).map_err(std::io::Error::other)?;

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        fs::write(path, format!("{HEADER}{body}"))
    }
}

/// Where the settings live: `$XDG_CONFIG_HOME/marks-client/config.toml` on Linux.
pub fn path() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join(APP_DIR).join(FILE_NAME))
}

/// The smallest window the list can be drawn in.
pub fn floor() -> egui::Vec2 {
    egui::vec2(MIN_WIDTH as f32, MIN_HEIGHT as f32)
}

/// The tests, in a file of their own: `config/tests.rs`, compiled only for test builds.
#[cfg(test)]
mod tests;
