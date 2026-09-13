//! The tests for `fonts`: the order the settings ask for, and the characters it can draw.

use super::*;
use ab_glyph::Font as _;

/// Runs a frame, which is what makes fonts available at all: egui cannot know the pixel scale
/// before the first one.
fn draw_a_frame(ctx: &egui::Context) {
    let _ = ctx.run_ui(egui::RawInput::default(), |_ui| {});
}

/// The order the window is drawing in, as egui holds it.
fn drawn_order(ctx: &egui::Context, family: egui::FontFamily) -> Vec<String> {
    ctx.fonts(|fonts| fonts.definitions().families[&family].clone())
}

/// Whether the font egui carries under `name` has a glyph for `c`.
///
/// Asked of the font file, rather than through `Fonts::has_glyph`, which answers a different
/// question: it reports a character as missing when the face that has it is also the family's
/// *replacement* face, and the replacement character is `◻`, which Hack has. So a font that
/// leads a family — which is what the default order puts Hack in — reads as having nothing at
/// all, however much it has. The font's own character map has no such opinion.
fn the_font_has(name: &str, c: char) -> bool {
    let bundled = egui::FontDefinitions::default();
    let data = bundled
        .font_data
        .get(name)
        .unwrap_or_else(|| panic!("egui does not carry a font called {name}"));
    let font = ab_glyph::FontRef::try_from_slice(&data.font).expect("a font egui ships");

    font.glyph_id(c).0 != 0
}


/// A machine, with two fonts on it, made of files that are to hand.
///
/// The real thing is read off whatever machine the test runs on — slow, and different everywhere —
/// so a test builds one. The families are whatever those files call themselves, since that is
/// where a family name lives, and they are chosen so as not to be names egui holds its own fonts
/// under: the two kinds have to stay told apart.
fn machine() -> SystemFonts {
    let bundled = egui::FontDefinitions::default();

    SystemFonts::from_data(vec![
        bundled.font_data["NotoEmoji-Regular"].font.to_vec(),
        egui_phosphor::Variant::Regular.font_data().font.to_vec(),
    ])
}

#[test]
fn a_machine_has_the_families_its_font_files_name() {
    let machine = machine();
    let families = machine.families();

    assert_eq!(families.len(), 2, "{families:?}");
    assert!(
        families.iter().all(|name| !AVAILABLE.contains(&name.as_str())),
        "the two kinds of font have to be distinguishable: {families:?}"
    );

    // Sorted, and each named once: a machine has several faces to a family, and the settings
    // choose families.
    let mut sorted = families.to_vec();
    sorted.sort();
    assert_eq!(families, sorted.as_slice());
}

#[test]
fn a_font_on_the_machine_can_be_drawn_with() {
    let machine = machine();
    let wanted = machine.families()[0].clone();
    let ctx = egui::Context::default();

    install(&ctx, std::slice::from_ref(&wanted), Some(&machine));
    draw_a_frame(&ctx);

    assert_eq!(
        drawn_order(&ctx, egui::FontFamily::Proportional),
        [wanted.as_str()]
    );

    // And it is loaded, rather than merely named: a name in a family with no data behind it draws
    // nothing at all.
    let loaded = ctx.fonts(|fonts| fonts.definitions().font_data.clone());
    assert!(loaded.contains_key(&wanted), "not loaded: {:?}", loaded.keys());
}

#[test]
fn a_name_that_is_no_font_at_all_is_left_out_of_the_order() {
    let machine = machine();
    let wanted = machine.families()[0].clone();
    let ctx = egui::Context::default();

    install(
        &ctx,
        &["Comic Sans MS".to_owned(), wanted.clone()],
        Some(&machine),
    );
    draw_a_frame(&ctx);

    // Left out, rather than left in with nothing behind it: a family naming a font that is not
    // there is a font that draws boxes.
    assert_eq!(drawn_order(&ctx, egui::FontFamily::Proportional), [wanted]);
}

#[test]
fn an_order_of_nothing_but_missing_fonts_falls_back_to_the_defaults() {
    let ctx = egui::Context::default();

    install(
        &ctx,
        &["Comic Sans MS".to_owned(), "Papyrus".to_owned()],
        Some(&machine()),
    );
    draw_a_frame(&ctx);

    // A window with no fonts in it is a window with no text in it.
    assert_eq!(
        drawn_order(&ctx, egui::FontFamily::Proportional),
        default_priority()
    );
}

#[test]
fn every_font_the_default_order_names_is_one_egui_carries() {
    let bundled = egui::FontDefinitions::default();

    for name in DEFAULT_PRIORITY {
        assert!(
            bundled.font_data.contains_key(*name),
            "{name} is named in the default order but is not a font egui has"
        );
    }

    // And the list of names a setting may use is that same set, no more.
    assert_eq!(AVAILABLE.len(), DEFAULT_PRIORITY.len());
}

#[test]
fn the_default_order_draws_with_hack() {
    let ctx = egui::Context::default();
    install(&ctx, &default_priority(), None);
    draw_a_frame(&ctx);

    // First in the order means it is the font the text is drawn in, not a fallback for the
    // characters the others are missing.
    assert_eq!(drawn_order(&ctx, egui::FontFamily::Proportional).first().map(String::as_str), Some("Hack"));
    assert_eq!(drawn_order(&ctx, egui::FontFamily::Monospace).first().map(String::as_str), Some("Hack"));
}

#[test]
fn the_default_font_can_draw_the_arrows() {
    // What the default order is for: egui's proportional font has no arrows, and the footer is
    // written with two of them.
    for c in "↑↓".chars() {
        assert!(
            the_font_has(DEFAULT_PRIORITY[0], c),
            "{} cannot draw {c:?}, which the window writes",
            DEFAULT_PRIORITY[0]
        );
    }

    // And the rest of what the window writes, from the default order as a whole: the arrows
    // above, and the punctuation that arrives with the title of a page.
    for c in WRITTEN.chars() {
        assert!(
            DEFAULT_PRIORITY.iter().any(|name| the_font_has(name, c)),
            "no font in the default order can draw {c:?}"
        );
    }
}

#[test]
fn the_order_a_setting_gives_is_the_order_the_window_draws_in() {
    let ctx = egui::Context::default();
    let wanted = vec![
        "Ubuntu-Light".to_owned(),
        "emoji-icon-font".to_owned(),
        "Hack".to_owned(),
    ];

    install(&ctx, &wanted, None);
    draw_a_frame(&ctx);

    assert_eq!(drawn_order(&ctx, egui::FontFamily::Proportional), wanted);
    // Both families, so that the link under a mark's name follows the same choice.
    assert_eq!(drawn_order(&ctx, egui::FontFamily::Monospace), wanted);
}

#[test]
fn an_order_with_nothing_in_it_falls_back_to_the_default() {
    // A family with no fonts in it is a window with no text in it, so an empty list is not
    // something to draw with: the default order is used instead.
    let ctx = egui::Context::default();
    install(&ctx, &[], None);
    draw_a_frame(&ctx);

    assert_eq!(
        drawn_order(&ctx, egui::FontFamily::Proportional),
        default_priority()
    );
}

