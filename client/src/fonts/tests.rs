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
    install(&ctx, &default_priority());
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

    install(&ctx, &wanted);
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
    install(&ctx, &[]);
    draw_a_frame(&ctx);

    assert_eq!(
        drawn_order(&ctx, egui::FontFamily::Proportional),
        default_priority()
    );
}
