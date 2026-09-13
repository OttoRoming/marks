//! The fonts the window draws with, and the order it asks them in.
//!
//! egui carries four fonts, and draws text with the first of them that has the character being
//! drawn. That order is a setting (`Fonts::priority`), because it changes the look of the whole
//! window: the fonts are not interchangeable — Hack is a monospace font, Ubuntu-Light is not —
//! and the first one is what nearly all of the text ends up in.
//!
//! It also decides which characters can be drawn at all. egui's proportional family has no
//! arrows in it, which is what made "↑↓ move" come out as two empty boxes; Hack has them, and
//! is where the default list starts.

use eframe::egui;

/// The fonts egui carries, and so the only ones a list may name.
///
/// The names are the ones egui knows them by internally, which is what a list is written in.
/// A font installed on the machine cannot be named here: loading one would mean finding it,
/// reading it and trusting it, which is a larger thing than an order to draw in.
pub const AVAILABLE: &[&str] = &[
    "Hack",
    "Ubuntu-Light",
    "NotoEmoji-Regular",
    "emoji-icon-font",
];

/// The order the window draws in unless the settings say otherwise.
///
/// Hack first, which is the monospace font and the only one of the four with arrows in it —
/// this is the order that draws everything the window writes. The others follow it rather than
/// being left out, so that the characters Hack does not have (emoji, and the handful of symbols
/// egui's icon font is there for) are still drawn.
pub const DEFAULT_PRIORITY: &[&str] = AVAILABLE;

/// What the window writes.
///
/// The arrows in the footer, and the ellipsis, quotation marks, dashes and apostrophes that
/// arrive with the title of a web page — which is where a mark's name comes from.
#[cfg(test)]
pub const WRITTEN: &str = "↑↓…“”–—’";

/// Puts `priority` on the window as the order it draws in.
///
/// Called as the window is built and again whenever the settings change. Both families are set
/// to the same list: the monospace one is only used for the link under a mark's name, and it
/// should follow the same choice rather than quietly keep an order of its own.
///
/// A list with nothing usable in it falls back to [`DEFAULT_PRIORITY`], because a family with
/// no fonts at all is a window with no text in it.
pub fn install(ctx: &egui::Context, priority: &[String]) {
    let mut fonts = egui::FontDefinitions::default();

    let chosen: Vec<String> = if priority.is_empty() {
        default_priority()
    } else {
        priority.to_vec()
    };

    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts.families.insert(family, chosen.clone());
    }

    ctx.set_fonts(fonts);
}

/// [`DEFAULT_PRIORITY`] as the settings hold it.
pub fn default_priority() -> Vec<String> {
    DEFAULT_PRIORITY.iter().map(|name| (*name).to_owned()).collect()
}

/// The tests, in a file of their own: `fonts/tests.rs`, compiled only for test builds.
#[cfg(test)]
mod tests;
