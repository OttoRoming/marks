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
//!
//! A list may also name any family the machine has: those are read through fontconfig where there
//! is one ([`SystemFonts`]), on a worker thread, and their data loaded when the order is put onto
//! the window.
//!
//! The window's *own* controls are the exception to all of it: an icon in a font somebody has taken
//! out of the list is an icon that disappears, so the buttons are drawn from a font of their own
//! that comes with the client. See [`ICON_FAMILY`].

use std::sync::Arc;

use eframe::egui;

/// The fonts egui carries, named the way egui names them.
///
/// What a settings file calls a font is either one of these or a family the machine has; see
/// [`SystemFonts`]. The names here are egui's own, which is why they are not quite the names the
/// files give themselves: `Ubuntu-Light` is a key, where the font calls itself "Ubuntu Light".
pub const AVAILABLE: &[&str] = &[
    "Hack",
    "Ubuntu-Light",
    "NotoEmoji-Regular",
    "emoji-icon-font",
];

/// The name the icon font is held under inside egui. Not one of [`AVAILABLE`]: it is the client's
/// own, for its own buttons, rather than a choice the settings make.
const ICON_FONT: &str = "phosphor";

/// The family the window's controls are drawn in, whatever the settings name.
///
/// The panel's buttons are drawn from this rather than from the fonts the settings choose between:
/// a font can be taken out of the order, and a control drawn in a font that is not in it is a
/// control that disappears, or a box where a chevron should be. This family is not reachable from
/// the settings at all.
pub const ICON_FAMILY: &str = "icons";

/// The order the window draws in unless the settings say otherwise.
///
/// Hack first, which is the monospace font and the only one of the four with arrows in it —
/// this is the order that draws everything the window writes. The others follow it rather than
/// being left out, so that the characters Hack does not have (emoji, and the handful of symbols
/// egui's icon font is there for) are still drawn.
pub const DEFAULT_PRIORITY: &[&str] = AVAILABLE;

/// What the window writes.
///
/// The arrows in the footer and the bullet between those hints, and the ellipsis, quotation
/// marks, dashes and apostrophes that arrive with the title of a web page — which is where a
/// mark's name comes from.
#[cfg(test)]
pub const WRITTEN: &str = "↑↓•…“”–—’";

/// The fonts this machine has installed, read once and kept.
///
/// Read on a worker thread rather than on the way to a frame: there can be thousands of them, and
/// opening every file to read its name is not something to do between two draws (see
/// `MarksApp::system_fonts`). Anything the settings name that egui does not carry is looked up
/// here, and so is the list the panel offers to add from.
pub struct SystemFonts {
    database: fontdb::Database,
    /// The family names, sorted and each named once, a machine having several faces per family.
    families: Vec<String>,
}

impl SystemFonts {
    /// Reads the fonts installed on this machine.
    ///
    /// Found the way the rest of the desktop finds them — through fontconfig where there is one —
    /// so that a font installed and visible in other programs can be chosen here too.
    pub fn load() -> Self {
        let mut database = fontdb::Database::new();
        database.load_system_fonts();

        Self::from_database(database)
    }

    /// A database of font data already in hand, which is what the tests use: reading the fonts of
    /// whatever machine a test runs on would make it a test of that machine.
    #[cfg(test)]
    pub(crate) fn from_data(blobs: Vec<Vec<u8>>) -> Self {
        let mut database = fontdb::Database::new();

        for blob in blobs {
            database.load_font_data(blob);
        }

        Self::from_database(database)
    }

    fn from_database(database: fontdb::Database) -> Self {
        let mut families: Vec<String> = database
            .faces()
            .flat_map(|face| face.families.iter().map(|(name, _)| name.clone()))
            .collect();

        families.sort();
        families.dedup();

        Self { database, families }
    }

    /// Every family on this machine, sorted, each named once.
    pub fn families(&self) -> &[String] {
        &self.families
    }

    /// The data for `family`, and which face of the file to use.
    ///
    /// The regular face of it: a setting names a font rather than a weight, and a list that could
    /// ask for bold-italic would be a list of another kind.
    fn face(&self, family: &str) -> Option<(Vec<u8>, u32)> {
        let query = fontdb::Query {
            families: &[fontdb::Family::Name(family)],
            weight: fontdb::Weight::NORMAL,
            style: fontdb::Style::Normal,
            stretch: fontdb::Stretch::Normal,
        };
        let id = self.database.query(&query)?;

        self.database
            .with_face_data(id, |data, index| (data.to_vec(), index))
    }
}

/// Puts `priority` on the window as the order it draws in.
///
/// Called as the window is built and again whenever the settings change. Both families are set
/// to the same list: the monospace one is only used for the link under a mark's name, and it
/// should follow the same choice rather than quietly keep an order of its own.
///
/// A name is either one of the fonts egui carries or a family this machine has ([`SystemFonts`]).
/// One that is neither is left out of the order — a family naming a font that is not there would
/// fall through to the font after it in any case — and a list that leaves nothing falls back to
/// [`DEFAULT_PRIORITY`], because a family with no fonts at all is a window with no text in it.
pub fn install(ctx: &egui::Context, priority: &[String], system: Option<&SystemFonts>) {
    let mut fonts = egui::FontDefinitions::default();

    // The client's own font, for its own controls: added to the font data here, and named only by
    // the icon family below, so that nothing the settings do can take it away.
    //
    // The crate offers `add_to_fonts`, which puts it into the proportional family instead — the
    // family the settings own, and one this function is about to overwrite. Its font data is
    // wanted; its opinion about where the font belongs is not.
    fonts.font_data.insert(
        ICON_FONT.to_owned(),
        Arc::new(egui_phosphor::Variant::Regular.font_data()),
    );

    let mut chosen: Vec<String> = Vec::new();

    for name in priority {
        // One of the fonts egui carries: already loaded, and named in the settings as egui names it.
        if fonts.font_data.contains_key(name) {
            chosen.push(name.clone());
            continue;
        }

        // Otherwise it has to be a font this machine has, and the data for it has to be read now:
        // a name in a family with no data behind it is a font that cannot draw anything.
        let Some((data, index)) = system.and_then(|system| system.face(name)) else {
            // Worth saying only when there was somewhere to look: before the machine's fonts have
            // been read, a font that is not here *yet* is not a font that is missing.
            if system.is_some() {
                eprintln!("marks-client: {name} is not a font this machine has; skipped");
            }
            continue;
        };

        fonts.font_data.insert(
            name.clone(),
            Arc::new(egui::FontData {
                font: data.into(),
                index,
                tweak: egui::FontTweak::default(),
            }),
        );
        chosen.push(name.clone());
    }

    let chosen = if chosen.is_empty() {
        default_priority()
    } else {
        chosen
    };

    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts.families.insert(family, chosen.clone());
    }

    fonts.families.insert(
        egui::FontFamily::Name(ICON_FAMILY.into()),
        vec![ICON_FONT.to_owned()],
    );

    ctx.set_fonts(fonts);
}

/// [`DEFAULT_PRIORITY`] as the settings hold it.
pub fn default_priority() -> Vec<String> {
    DEFAULT_PRIORITY.iter().map(|name| (*name).to_owned()).collect()
}

/// The tests, in a file of their own: `fonts/tests.rs`, compiled only for test builds.
#[cfg(test)]
mod tests;
