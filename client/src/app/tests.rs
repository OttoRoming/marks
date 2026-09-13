//! The tests for `app`: the keybindings, the search, the settings panel, and what a frame draws.

use super::*;
use crate::config::Config;
use crate::test_page;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// How long a test waits for a worker thread's answer to reach the window.
const PATIENCE: Duration = Duration::from_secs(20);

/// A mark as the server sends it, for the tests that only need a list to filter.
fn mark(name: &str, content: &str) -> Mark {
    Mark {
        id: name.to_lowercase(),
        name: name.to_owned(),
        content: content.to_owned(),
        icon_id: None,
    }
}

fn names(marks: &[&Mark]) -> Vec<String> {
    marks.iter().map(|mark| mark.name.clone()).collect()
}

/// A window built without anything off the machine: the server and the session are both handed
/// in, and so are the settings, so neither the environment nor a file takes any part in a test
/// here.
fn window(base_url: &str, token: Option<&str>) -> (MarksApp, egui::Context) {
    window_with(base_url, token, Config::default())
}

/// The same, with settings a test has prepared — including, usually, a file of its own to write
/// them to.
fn window_with(base_url: &str, token: Option<&str>, config: Config) -> (MarksApp, egui::Context) {
    let ctx = egui::Context::default();
    let mut app = MarksApp::new(&ctx, base_url.to_owned(), token.map(str::to_owned), config);

    // The machine's fonts, without reading the machine's fonts: a window built here has none of
    // its own unless the test brings some, and nothing in this file should be opening thousands of
    // files to find out what this machine has installed.
    app.system_fonts = Some(Arc::new(SystemFonts::from_data(Vec::new())));

    (app, ctx)
}

/// One frame's input, of a window of the size the client opens at.
///
/// The size matters: egui's own default screen is ten thousand points square, and a layout that
/// only breaks in a window of the size a window actually is — buttons placed at the right-hand
/// edge of it, say — is a layout that a test with that default would never notice. These are the
/// measurements the window opens with, so the frames drawn here are drawn in a window.
fn input(events: Vec<egui::Event>) -> egui::RawInput {
    let default = crate::config::Window::default();

    egui::RawInput {
        events,
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(default.width as f32, default.height as f32),
        )),
        ..Default::default()
    }
}

/// Runs one frame of the whole window with `events` as this frame's input.
fn frame(app: &mut MarksApp, ctx: &egui::Context, events: Vec<egui::Event>) {
    // The frame's output (shapes, viewport commands) is not what these tests look at.
    let _ = ctx.run_ui(input(events), |ui| app.show(ui));
}

/// One key press, as the window would receive it.
fn press(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

/// Takes what the workers have said until `done` holds, and reports whether it ever did.
///
/// Those answers arrive on a channel from a thread doing a real request, so this is the waiting
/// that stands in for the frames the window would be drawing.
fn wait_until(app: &mut MarksApp, ctx: &egui::Context, done: impl Fn(&MarksApp) -> bool) -> bool {
    let deadline = Instant::now() + PATIENCE;

    loop {
        app.drain_events(ctx);
        if done(app) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// A settings file of this test's own, removed again when the test ends.
struct Settings(PathBuf);

impl Settings {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "marks-client-app-{}-{name}",
            std::process::id()
        ));

        let _ = fs::remove_dir_all(&dir);

        Self(dir.join("config.toml"))
    }

    /// Settings that read and write this file.
    fn config(&self) -> Config {
        Config::load_from(self.0.clone())
    }

    fn contents(&self) -> String {
        fs::read_to_string(&self.0).expect("the settings file")
    }
}

impl Drop for Settings {
    fn drop(&mut self) {
        if let Some(parent) = self.0.parent() {
            let _ = fs::remove_dir_all(parent);
        }
    }
}

/// What the server answers a request carrying a session it will not accept.
///
/// This is what an expired session — or one withdrawn somewhere else — looks like from here:
/// every authenticated route answers 401 (see the `unauthorized` helper on the server).
fn refused_answer() -> (String, String) {
    (
        "HTTP/1.1 401 Unauthorized\r\ncontent-type: application/json\r\n".to_owned(),
        r#"{"error":"Unauthorized"}"#.to_owned(),
    )
}

/// A listing the server accepts, for the tests that give it an answer it never gets to use.
fn marks_ok() -> (String, String) {
    (
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n".to_owned(),
        r#"{"marks":[]}"#.to_owned(),
    )
}

/// A listing with one mark in it, which is how a test knows the session was accepted.
fn marks_with_one() -> (String, String) {
    (
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n".to_owned(),
        r#"{"marks":[{"id":"mark-1","name":"Otto","content":"https://example.com/","icon_id":null}]}"#
            .to_owned(),
    )
}

/// A server that refuses the session and answers nothing else.
fn refused() -> test_page::Served {
    let (head, body) = refused_answer();

    test_page::serve_answer(&head, &body)
}

#[test]
fn a_session_the_server_refuses_puts_the_sign_in_dialog_back_up() {
    let server = refused();
    let (mut app, ctx) = window(server.url.as_str(), Some("expired"));

    // The window opens signed in: a session was handed to it, so the dialog is down...
    assert!(app.api.is_some(), "the session was not taken up");
    assert!(app.auth.error.is_none());

    // ...and the first thing it does is ask for the marks, which the server refuses.
    assert!(
        wait_until(&mut app, &ctx, |app| app.api.is_none()),
        "a refused session left the window signed in"
    );

    // The dialog is back, it says why, and the caret is where the user has to start.
    assert_eq!(
        app.auth.error.as_deref(),
        Some("That session is no longer valid - sign in again.")
    );
    assert!(
        app.auth.focus_username,
        "the caret belongs in the username field"
    );

    // And nothing the refused session had is left on screen.
    assert!(app.marks.is_empty());
    assert!(app.icons.is_none());
    assert!(app.username.is_none());
}

#[test]
fn a_window_that_has_been_signed_out_does_not_offer_the_session_again() {
    // A second answer is ready in case the window asks again; the point of the test is that it
    // never does.
    let server = test_page::serve_answers(vec![refused_answer(), marks_ok()]);
    let (mut app, ctx) = window(server.url.as_str(), Some("expired"));

    assert!(
        wait_until(&mut app, &ctx, |app| app.api.is_none()),
        "a refused session left the window signed in"
    );

    // The one request the window made was the mark list it opened with.
    assert!(
        server.request.try_recv().is_ok(),
        "the window never asked for its marks"
    );

    // Every call it can make goes through `self.api`, which is `None` until the dialog has been
    // used, so saving what is typed reaches for nothing.
    app.query = "example.com".to_owned();
    app.save_query(&ctx);

    // Long enough for a second request to have arrived and left its mark, if there were one.
    std::thread::sleep(Duration::from_millis(300));
    app.drain_events(&ctx);

    assert!(
        server.request.try_recv().is_err(),
        "the refused session was offered to the server again"
    );
    assert!(
        app.notice.is_none(),
        "a save was attempted: {:?}",
        app.notice.map(|notice| notice.text)
    );
    assert!(app.marks.is_empty());
    assert_eq!(
        app.auth.error.as_deref(),
        Some("That session is no longer valid - sign in again.")
    );
}

#[test]
fn a_session_that_expires_while_the_window_is_open_signs_it_out() {
    // Accepted once, refused from then on: what a session reaching its expiry with the window
    // already open looks like from here.
    let server = test_page::serve_answers(vec![marks_with_one(), refused_answer()]);
    let (mut app, ctx) = window(server.url.as_str(), Some("was-valid"));

    assert!(
        wait_until(&mut app, &ctx, |app| !app.marks.is_empty()),
        "the session was not accepted to begin with"
    );
    assert!(app.api.is_some());

    // The session dies between one action and the next: the user types a link and saves it.
    app.query = "example.com".to_owned();
    app.save_query(&ctx);

    assert!(
        wait_until(&mut app, &ctx, |app| app.api.is_none()),
        "an expired session went unnoticed"
    );

    // The dialog is back with the reason, and the list it was showing went with the session.
    assert_eq!(
        app.auth.error.as_deref(),
        Some("That session is no longer valid - sign in again.")
    );
    assert!(app.marks.is_empty(), "the signed-out window kept a list");

    // Two requests were made and no more: the listing, and the save that found the session
    // gone. A signed-out window does not keep trying.
    assert!(server.request.try_recv().is_ok(), "the listing request");
    assert!(
        server.request.try_recv().is_ok(),
        "the save that found the session gone"
    );
    std::thread::sleep(Duration::from_millis(300));
    assert!(
        server.request.try_recv().is_err(),
        "a request was made after signing out"
    );
}

#[test]
fn a_window_with_no_session_asks_the_server_for_nothing() {
    // No token handed in and none on disk: the dialog is up from the first frame, and nothing
    // is asked of the server until it has been used.
    let server = refused();
    let (mut app, ctx) = window(server.url.as_str(), None);

    app.drain_events(&ctx);

    assert!(app.api.is_none());
    assert!(app.auth.error.is_none());
    assert!(app.marks.is_empty());
}

/// A window that is signed in, so that the launcher keys are the ones in play.
///
/// The server is a canned one that accepts anything: the launcher keys are only read while
/// there is a session, and a window with the sign-in dialog up belongs to the dialog instead.
fn signed_in(config: Config) -> (MarksApp, egui::Context) {
    let server = test_page::serve_answers(vec![marks_ok()]);

    window_with(server.url.as_str(), Some("a-session"), config)
}

/// A stand-in for the fonts a machine has: two real font files, and the families they give
/// themselves.
///
/// `SystemFonts` is read off the machine in the window — slow, and different on every machine — so
/// a test brings its own. The bytes are files that are to hand (egui's, and the icon crate's), and
/// the names are whatever is inside them: a family name is part of the font, which is what a
/// database reads it from, so a test cannot invent one. They are chosen to be names that are not
/// also the names egui holds its own fonts under, so that the two kinds stay told apart.
fn table() -> SystemFonts {
    let bundled = egui::FontDefinitions::default();

    SystemFonts::from_data(vec![
        bundled.font_data["NotoEmoji-Regular"].font.to_vec(),
        egui_phosphor::Variant::Regular.font_data().font.to_vec(),
    ])
}

/// What the panel would offer to add for `app`, with `query` typed in.
fn addable(app: &MarksApp, query: &str) -> Vec<String> {
    let system = app
        .system_fonts
        .clone()
        .expect("the panel has the machine's fonts");

    fonts_to_add(query, &system, &app.config.fonts.priority).0
}

/// A click at `pos`, over the two frames a real one arrives in.
fn click(app: &mut MarksApp, ctx: &egui::Context, pos: egui::Pos2) {
    for pressed in [true, false] {
        frame(
            app,
            ctx,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
}

/// Where a piece of the window's text is drawn, for a test that has to click it.
fn text_pos(app: &mut MarksApp, ctx: &egui::Context, wanted: &str) -> egui::Pos2 {
    drawn_text(app, ctx)
        .into_iter()
        .find(|(_, _, drawn)| drawn.starts_with(wanted))
        .map(|(pos, _, _)| pos)
        .unwrap_or_else(|| panic!("no {wanted:?} drawn"))
}

#[test]
fn ctrl_comma_opens_the_configuration_panel_and_ctrl_comma_closes_it() {
    let (mut app, ctx) = signed_in(Config::default());

    assert!(!app.config_open);

    frame(
        &mut app,
        &ctx,
        vec![press(egui::Key::Comma, egui::Modifiers::CTRL)],
    );
    assert!(app.config_open, "Ctrl+, did not open the panel");

    frame(
        &mut app,
        &ctx,
        vec![press(egui::Key::Comma, egui::Modifiers::CTRL)],
    );
    assert!(!app.config_open, "Ctrl+, did not close it again");
}

#[test]
fn escape_closes_the_configuration_panel_rather_than_the_window() {
    let (mut app, ctx) = signed_in(Config::default());

    frame(
        &mut app,
        &ctx,
        vec![press(egui::Key::Comma, egui::Modifiers::CTRL)],
    );
    assert!(app.config_open);

    let output = ctx.run_ui(
        egui::RawInput {
            events: vec![press(egui::Key::Escape, egui::Modifiers::NONE)],
            ..Default::default()
        },
        |ui| app.show(ui),
    );

    assert!(!app.config_open, "Esc left the panel up");
    // A panel is not the window: Esc while it is up must not ask the window to close.
    let closing = output
        .viewport_output
        .values()
        .any(|viewport| viewport.commands.contains(&egui::ViewportCommand::Close));

    assert!(!closing, "Esc closed the window instead of the panel");
}

#[test]
fn the_panel_leaves_the_launcher_keys_alone_while_it_is_up() {
    let (mut app, ctx) = signed_in(Config::default());
    app.query = "buy milk".to_owned();

    frame(
        &mut app,
        &ctx,
        vec![press(egui::Key::Comma, egui::Modifiers::CTRL)],
    );

    // Ctrl+Enter belongs to the list, and the panel is what has the keyboard.
    frame(
        &mut app,
        &ctx,
        vec![press(egui::Key::Enter, egui::Modifiers::CTRL)],
    );

    assert_eq!(app.query, "buy milk", "the panel let a save through");
    assert!(app.notice.is_none(), "{:?}", app.notice.map(|it| it.text));
    assert!(app.config_open, "the panel closed itself");
}

#[test]
fn a_fixed_size_is_applied_to_the_window_and_written_down() {
    let settings = Settings::new("fixed");
    let (mut app, ctx) = window_with("http://localhost:5173", None, settings.config());

    assert!(!app.config.window.fixed_size);

    app.set_fixed_size(&ctx, true);

    // In effect here and now...
    assert!(app.config.window.fixed_size);
    assert!(
        ctx.run_ui(input(Vec::new()), |ui| app.show(ui))
            .viewport_output
            .values()
            .any(|viewport| viewport
                .commands
                .contains(&egui::ViewportCommand::Resizable(false))),
        "the window was not told to stop being resizable"
    );

    // ...and still there the next time the settings are read.
    assert!(settings.contents().contains("fixed_size = true"));
    assert!(settings.config().window.fixed_size, "not read back");
}

#[test]
fn a_size_no_window_could_be_is_brought_within_reach_and_written_down() {
    let settings = Settings::new("size");
    let (mut app, ctx) = window_with("http://localhost:5173", None, settings.config());

    // As the panel leaves them when a number is typed into it.
    app.config.window.width = 3;
    app.config.window.height = 999_999;
    app.settle_window(&ctx);

    assert_eq!(app.config.window.width, MIN_WIDTH, "too narrow to draw the list");
    assert_eq!(app.config.window.height, MAX_SIDE, "larger than any display");

    let read_back = settings.config().window;
    assert_eq!(read_back.width, MIN_WIDTH);
    assert_eq!(read_back.height, MAX_SIDE);
}

#[test]
fn moving_a_font_up_the_order_puts_it_first_and_writes_it_down() {
    let settings = Settings::new("fonts");
    let (mut app, ctx) = window_with("http://localhost:5173", None, settings.config());
    // A frame first: egui does not know what the fonts are until one has been drawn.
    frame(&mut app, &ctx, Vec::new());

    // Hack leads by default; move the font behind it in front of it.
    let second = app.config.fonts.priority[1].clone();
    app.move_font(&ctx, 1, -1);

    assert_eq!(app.config.fonts.priority.first(), Some(&second));

    // The window draws in the order it was just given, from the next frame on: egui rebuilds
    // its fonts at the end of a frame, so that is when a change to them lands.
    frame(&mut app, &ctx, Vec::new());
    let drawn = ctx
        .fonts(|fonts| fonts.definitions().families[&egui::FontFamily::Proportional].clone());
    assert_eq!(drawn, app.config.fonts.priority);

    // ...and the order is on disk for the next run.
    assert_eq!(settings.config().fonts.priority, app.config.fonts.priority);
}

#[test]
fn a_font_cannot_be_moved_off_either_end_of_the_order() {
    let settings = Settings::new("fonts-ends");
    let (mut app, ctx) = window_with("http://localhost:5173", None, settings.config());
    let before = app.config.fonts.priority.clone();

    app.move_font(&ctx, 0, -1);
    app.move_font(&ctx, before.len() - 1, 1);

    assert_eq!(app.config.fonts.priority, before, "the order changed");
    // Nothing was written either: there was nothing to write.
    assert!(!settings.0.exists(), "a settings file appeared from nowhere");
}

#[test]
fn a_settings_file_that_says_nothing_leaves_the_window_as_it_was() {
    let settings = Settings::new("defaults");
    let (app, _) = window_with("http://localhost:5173", None, settings.config());

    assert_eq!(app.config.window.width, 620);
    assert_eq!(app.config.window.height, 440);
    assert!(!app.config.window.fixed_size);
}

#[test]
fn a_frame_is_drawn_with_rows_a_fuzzy_query_matched() {
    // The only test that draws a row with a query in play: the highlighting is laid out from
    // byte offsets, and a frame is where a wrong one would be noticed.
    let server = test_page::serve_answers(vec![marks_with_one()]);
    let (mut app, ctx) = window(server.url.as_str(), Some("a-session"));

    assert!(
        wait_until(&mut app, &ctx, |app| !app.marks.is_empty()),
        "the mark never arrived"
    );

    // "you ube"-shaped: a query broken up, against the mark's name and its link.
    for query in ["otto", "Otto", "exmple", "", "nothing here"] {
        app.query = query.to_owned();

        frame(&mut app, &ctx, Vec::new());

        let shown = filter_marks(&app.marks, &app.query).len();
        assert_eq!(
            shown,
            if query == "nothing here" { 0 } else { 1 },
            "a frame with {query:?} did not draw the list it should have"
        );
    }
}

#[test]
fn the_mark_that_answers_the_query_best_comes_first() {
    // From the window, as it was: typing "you" put a news link above the YouTube mark, because
    // the link's address happens to have a y, an o and a u in it — spread across "nyheter",
    // "oro" and "muslimer". The matcher scores that as the poor match it is, and the list now
    // follows what it says.
    let marks = vec![
        mark(
            "Jimmie Åkessons oro: Muslimer kan avgöra valet | SVT Nyheter",
            "https://www.svt.se/nyheter/inrikes/jimmie-akessons-oro-muslimer-kan-avgora-valet",
        ),
        mark("YouTube", "https://youtube.com/"),
    ];

    assert_eq!(
        names(&filter_marks(&marks, "you")),
        [
            "YouTube",
            "Jimmie Åkessons oro: Muslimer kan avgöra valet | SVT Nyheter"
        ]
    );

    // And what is drawn is what is selected: the list and the selection come from the same order.
    assert_eq!(
        selected_mark(&marks, "you", 0).map(|mark| mark.name.as_str()),
        Some("YouTube")
    );
}

#[test]
fn marks_the_query_answers_equally_well_keep_the_servers_order() {
    // With nothing typed, everything matches and everything scores the same, so the list is left
    // exactly as it arrived rather than being shuffled by a ranking that has nothing to go on.
    let marks = vec![
        mark("alpha", "https://a.example/"),
        mark("beta", "https://b.example/"),
        mark("gamma", "https://c.example/"),
    ];

    for query in ["", "   "] {
        assert_eq!(
            names(&filter_marks(&marks, query)),
            ["alpha", "beta", "gamma"]
        );
    }
}

#[test]
fn a_mark_is_ranked_by_whichever_of_its_two_lines_answers_better() {
    // The name is not what decides it, and neither is the link: whichever of them the query
    // answers better is the score the mark is ranked by. Here the name "x" says nothing about
    // "github", and the link says all of it.
    let marks = vec![
        mark("x", "https://github.com/emilk/egui"),
        mark("zeta", "https://example.com/"),
    ];

    assert_eq!(
        names(&filter_marks(&marks, "github")),
        ["x"],
        "found by its link"
    );
}

#[test]
fn a_query_broken_into_words_still_finds_the_mark() {
    let marks = vec![
        mark("youtube.com", "https://youtube.com/"),
        mark("Svelte", "https://svelte.dev/docs/kit"),
    ];

    // The case this is here for: two words remembered, against a name written as one.
    assert_eq!(names(&filter_marks(&marks, "you ube")), ["youtube.com"]);
    assert_eq!(names(&filter_marks(&marks, "youtube")), ["youtube.com"]);
    assert_eq!(names(&filter_marks(&marks, "ytb")), ["youtube.com"]);
    // Still found by what it points at, not only by what it is called, and still found whatever
    // the case it is typed in.
    assert_eq!(names(&filter_marks(&marks, "SVELTE.DEV")), ["Svelte"]);
    assert_eq!(names(&filter_marks(&marks, "kit")), ["Svelte"]);

    // And a query with nothing of a mark in it keeps nothing.
    assert!(filter_marks(&marks, "nothing here").is_empty());
    assert!(filter_marks(&marks, "ube you").is_empty(), "order matters");
}

#[test]
fn the_characters_a_query_matched_are_the_ones_picked_out() {
    let ctx = egui::Context::default();
    let mut job = None;
    let mut colors = (egui::Color32::default(), egui::Color32::default());

    let _ = ctx.run_ui(input(Vec::new()), |ui| {
        colors = (
            ui.visuals().weak_text_color(),
            ui.visuals().strong_text_color(),
        );
        job = Some(highlighted(
            ui,
            "youtube.com",
            "you ube",
            egui::TextStyle::Body,
            ui.visuals().strong_text_color(),
        ));
    });

    let job = job.expect("a laid-out line");
    let runs: Vec<(&str, bool)> = job
        .sections
        .iter()
        .map(|section| {
            let range = section.byte_range.clone();
            let text = &job.text[range.start.0..range.end.0];

            // Matched stretches are drawn in the strong colour, the rest dimmed around them.
            let lit = section.format.color == colors.1;
            assert!(lit || section.format.color == colors.0, "an unexpected colour");

            (text, lit)
        })
        .collect();

    // The characters the query found, and the `t` it went past on the way — with each run of
    // matched characters drawn as one stretch rather than one per character.
    assert_eq!(
        runs,
        [("you", true), ("t", false), ("ube", true), (".com", false)]
    );
}

#[test]
fn a_row_with_no_match_in_it_is_drawn_as_it_always_was() {
    let ctx = egui::Context::default();
    let mut jobs = Vec::new();

    let _ = ctx.run_ui(input(Vec::new()), |ui| {
        let plain = ui.visuals().weak_text_color();

        for query in ["", "   ", "nothing here"] {
            jobs.push(highlighted(
                ui,
                "https://youtube.com/",
                query,
                egui::TextStyle::Monospace,
                plain,
            ));
        }
    });

    for job in jobs {
        // One stretch, end to end, in the colour the line is always drawn in.
        assert_eq!(job.sections.len(), 1, "the line was broken up for nothing");
        assert_eq!(job.sections[0].byte_range.start.0, 0);
        assert_eq!(job.sections[0].byte_range.end.0, job.text.len());

        let plain = job.sections[0].format.color;
        assert_eq!(job.text, "https://youtube.com/");
        assert!(job.sections.iter().all(|section| section.format.color == plain));
    }
}

/// Every piece of text this frame drew, with where it landed and what it is clipped to.
///
/// The window's widgets do not say what they drew, so this looks at the shapes the frame left
/// behind: text ends up as galleys, and a galley carries the string it was laid out from.
fn drawn_text(app: &mut MarksApp, ctx: &egui::Context) -> Vec<(egui::Pos2, egui::Rect, String)> {
    let output = ctx.run_ui(input(Vec::new()), |ui| app.show(ui));
    let mut found = Vec::new();

    collect(&output.shapes, egui::Rect::EVERYTHING, &mut found);

    found
}

/// Whether this frame drew `text` where it could be seen.
///
/// Which is the only way to tell a line that appears from one that is merely decided on.
fn drew(app: &mut MarksApp, ctx: &egui::Context, text: &str) -> bool {
    drawn_text(app, ctx)
        .into_iter()
        .any(|(pos, clip, drawn)| clip.contains(pos) && drawn.contains(text))
}

/// Every piece of text this frame drew outside `window`, with where it landed.
///
/// The panel is laid out from the width it is given, and a widget pushed past the edge of the
/// window is drawn where nobody can see it — which is a bug only a test looking at *where*
/// things went can find.
fn stray_text(app: &mut MarksApp, ctx: &egui::Context, window: egui::Rect) -> Vec<String> {
    let mut raw = input(Vec::new());
    raw.screen_rect = Some(window);

    let output = ctx.run_ui(raw, |ui| app.show(ui));
    let mut found = Vec::new();
    collect(&output.shapes, egui::Rect::EVERYTHING, &mut found);

    found
        .into_iter()
        // Visible, and not in the window: text that is scrolled out of a panel is drawn outside
        // the window on purpose, and clipped away where it sits.
        .filter(|(pos, clip, _)| clip.contains(*pos) && !window.contains(*pos))
        .map(|(pos, _, text)| format!("{text:?} at {pos:?}"))
        .collect()
}

/// Every rectangle this frame painted: where it is, what colour, and how round.
///
/// The corners are here because a colour is not enough to tell one rectangle from another — egui
/// paints every field in the colour the list of fonts is set into, so the search box above it is
/// the same colour and only its shape says which is which.
fn painted_rects(
    app: &mut MarksApp,
    ctx: &egui::Context,
) -> Vec<(egui::Rect, egui::Color32, egui::CornerRadius)> {
    let output = ctx.run_ui(input(Vec::new()), |ui| app.show(ui));
    let mut rects = Vec::new();

    for clipped in &output.shapes {
        rects_of(&clipped.shape, &mut rects);
    }

    rects
}

fn rects_of(shape: &egui::Shape, out: &mut Vec<(egui::Rect, egui::Color32, egui::CornerRadius)>) {
    match shape {
        egui::Shape::Rect(rect) => out.push((rect.rect, rect.fill, rect.corner_radius)),
        egui::Shape::Vec(shapes) => {
            for shape in shapes {
                rects_of(shape, out);
            }
        }
        _ => {}
    }
}

/// Every piece of text in `shapes`, with the rectangle it is clipped to.
fn collect(
    shapes: &[egui::epaint::ClippedShape],
    clip: egui::Rect,
    out: &mut Vec<(egui::Pos2, egui::Rect, String)>,
) {
    for clipped in shapes {
        walk(&clipped.shape, clipped.clip_rect.intersect(clip), out);
    }
}

fn walk(shape: &egui::Shape, clip: egui::Rect, out: &mut Vec<(egui::Pos2, egui::Rect, String)>) {
    match shape {
        egui::Shape::Text(text) => out.push((text.pos, clip, text.galley.job.text.clone())),
        egui::Shape::Vec(shapes) => {
            for shape in shapes {
                walk(shape, clip, out);
            }
        }
        _ => {}
    }
}

#[test]
fn the_note_about_a_size_not_applying_only_comes_after_a_change_is_asked_for() {
    let settings = Settings::new("note-when");
    let (mut app, ctx) = signed_in(settings.config());
    app.config_open = true;

    // The panel is up, and nothing has been asked of the window yet: there is nothing to explain,
    // and nothing is said.
    frame(&mut app, &ctx, Vec::new());
    assert!(
        !drew(&mut app, &ctx, RESTART_NOTE),
        "the panel explained a change nobody had asked for"
    );

    // Switching the option is a change, so the panel may now say what to do if it does not
    // arrive — and it says it.
    app.set_fixed_size(&ctx, true);
    assert!(
        drew(&mut app, &ctx, RESTART_NOTE),
        "a change was asked for and nothing was said about it"
    );
}

#[test]
fn asking_for_a_different_size_also_brings_the_note_out() {
    // The switch is not the only thing the note is about: a size that does not arrive is the same
    // surprise, from the same window manager.
    let settings = Settings::new("note-when-size");
    let (mut app, ctx) = signed_in(settings.config());
    app.config_open = true;

    frame(&mut app, &ctx, Vec::new());
    assert!(!drew(&mut app, &ctx, RESTART_NOTE));

    // What the panel does when a size is settled on.
    app.config.window.width = 700;
    app.settle_window(&ctx);

    assert!(drew(&mut app, &ctx, RESTART_NOTE));
}

#[test]
fn the_note_about_a_size_that_may_not_apply_is_drawn_as_a_warning() {
    let ctx = egui::Context::default();
    let mut note = None;
    let mut colours = (egui::Color32::default(), egui::Color32::default());

    let _ = ctx.run_ui(input(Vec::new()), |ui| {
        colours = (
            ui.visuals().warn_fg_color,
            ui.visuals().weak_text_color(),
        );
        note = Some(restart_note(ui));
    });

    let note = note.expect("a note");
    let (warn, weak) = colours;

    assert_eq!(note.text, RESTART_NOTE);
    // Drawn as a warning rather than as another line of small print: the theme's warning colour,
    // which is what makes it stand out, and not the weak colour the lines around it use.
    assert_eq!(note.sections.len(), 1);
    assert_eq!(note.sections[0].format.color, warn);
    assert_ne!(warn, weak, "the two are the same colour in this theme");
}

#[test]
fn the_hints_along_the_bottom_are_separated_by_dots() {
    // The hints are joined by the separator, whatever it is; that it is drawn as a dot, and one
    // the fonts can draw, is asserted with the fonts themselves.
    assert_eq!(
        hints(&["↑↓ move", "Enter open"]),
        format!("↑↓ move {HINT_SEPARATOR} Enter open")
    );
    assert_eq!(
        hints(&["Enter open", "Esc close"]),
        format!("Enter open {HINT_SEPARATOR} Esc close")
    );
    // One hint on its own has nothing to be separated from, and the line the sign-in dialog
    // leaves up is exactly that.
    assert_eq!(hints(&["Esc close"]), "Esc close");
    assert_eq!(hints(&[]), "");

    // The separator is a character the window's fonts are checked for, so a font order that
    // cannot draw it fails the suite rather than showing a box.
    assert!(
        crate::fonts::WRITTEN.contains(HINT_SEPARATOR),
        "the separator is not a character the fonts are checked for"
    );
}

#[test]
fn a_font_can_be_taken_out_of_the_order() {
    let settings = Settings::new("remove-font");
    let (mut app, ctx) = signed_in(settings.config());
    app.config_open = true;

    assert_eq!(
        app.config.fonts.priority,
        crate::fonts::default_priority(),
        "the order to start from"
    );

    app.remove_font(&ctx, 1);

    // Gone from the order, and the window draws without it...
    let remaining = app.config.fonts.priority.clone();
    assert!(!remaining.contains(&"Ubuntu-Light".to_owned()));
    assert_eq!(remaining.len(), 3);

    // ...which is what the frame after it shows: the order the window draws in, and no row for
    // the font that was taken out of it.
    frame(&mut app, &ctx, Vec::new());
    let drawn =
        ctx.fonts(|fonts| fonts.definitions().families[&egui::FontFamily::Proportional].clone());
    assert_eq!(drawn, remaining);
    // It is gone from the order, and the panel offers it back: that is what the search list is,
    // so it is still drawn — in the list of fonts to add rather than in the order.
    assert!(
        addable(&app, "ubuntu-light").contains(&"Ubuntu-Light".to_owned()),
        "a font that was removed cannot be added back"
    );

    // And it is that way on disk for the next run.
    assert_eq!(settings.config().fonts.priority, remaining);
}

#[test]
fn the_last_font_cannot_be_taken_out() {
    let settings = Settings::new("remove-last-font");
    let (mut app, ctx) = signed_in(settings.config());

    // Down to one, which is as far as it goes — a bounded number of tries, so that a list which
    // would not shrink fails this test rather than hanging it.
    for _ in 0..crate::fonts::AVAILABLE.len() + 2 {
        app.remove_font(&ctx, 1);
    }
    let last = app.config.fonts.priority.clone();
    assert_eq!(last, ["Hack"]);

    app.remove_font(&ctx, 0);

    // A font order with nothing in it is a window with no text in it, so the last one stays.
    assert_eq!(app.config.fonts.priority, last);
}

#[test]
fn a_font_that_was_taken_out_can_be_put_back() {
    let settings = Settings::new("add-font");
    let (mut app, ctx) = signed_in(settings.config());

    app.remove_font(&ctx, 0);
    assert!(!app.config.fonts.priority.contains(&"Hack".to_owned()));
    // Which is what the panel offers to put back.
    assert!(addable(&app, "hack").contains(&"Hack".to_owned()));

    app.add_font(&ctx, "Hack");

    // At the end, so that a font just added is one more to fall back to rather than the one
    // everything is suddenly drawn in; moving it up is the next thing the panel offers.
    assert_eq!(app.config.fonts.priority.last().map(String::as_str), Some("Hack"));
    assert_eq!(app.config.fonts.priority.len(), 4);
    assert!(addable(&app, "").is_empty(), "the order names every font there is");
    assert_eq!(settings.config().fonts.priority, app.config.fonts.priority);

    frame(&mut app, &ctx, Vec::new());
    let drawn =
        ctx.fonts(|fonts| fonts.definitions().families[&egui::FontFamily::Proportional].clone());
    assert_eq!(drawn, app.config.fonts.priority);
}

#[test]
fn a_font_cannot_be_added_twice_or_invented() {
    let settings = Settings::new("add-font-twice");
    let (mut app, ctx) = signed_in(settings.config());
    let before = app.config.fonts.priority.clone();

    // One that is already in the order, one this client does not carry, and one that is not a
    // font at all.
    for name in ["Hack", "Comic Sans", ""] {
        app.add_font(&ctx, name);
    }

    assert_eq!(app.config.fonts.priority, before);
}

#[test]
fn a_font_this_machine_has_can_be_added_by_pressing_it_in_the_dropdown() {
    // The dropdown offers every family this machine has, and pressing one used to do nothing at
    // all: the name was checked against the four fonts egui carries, and a family this machine has
    // is not one of those. The press was taken, the name was refused in silence, and the panel
    // looked broken beside a dropdown that had just offered it.
    //
    // Every row the dropdown can offer is one of these, which is why this went unnoticed: the order
    // starts out naming all four of egui's fonts, so the only fonts left to offer are the machine's.
    let settings = Settings::new("add-machine-font");
    let (mut app, ctx) = signed_in(settings.config());

    let machine = table();
    let family = machine.families()[0].to_owned();
    assert!(
        !crate::fonts::AVAILABLE.contains(&family.as_str()),
        "this test is about a font egui does not carry, and {family:?} is one it does"
    );

    app.system_fonts = Some(Arc::new(machine));
    app.config_open = true;

    for _ in 0..3 {
        frame(&mut app, &ctx, Vec::new());
    }

    // The dropdown opened, with that family as one of its rows, and the row pressed.
    let button = text_pos(&mut app, &ctx, "Add a font");
    click(&mut app, &ctx, button);

    let row = text_pos(&mut app, &ctx, &family);
    click(&mut app, &ctx, row);

    assert_eq!(
        app.config.fonts.priority.last(),
        Some(&family),
        "pressing a font this machine has did not put it in the order"
    );

    // And the window is drawn in it: a name in the order that no data was found for is a font that
    // draws nothing, which would leave the panel looking as though nothing had happened either.
    frame(&mut app, &ctx, Vec::new());
    let drawn = ctx
        .fonts(|fonts| fonts.definitions().families[&egui::FontFamily::Proportional].clone());

    assert!(
        drawn.contains(&family),
        "the font went into the order but not onto the window: {drawn:?}"
    );
}

#[test]
fn what_can_be_added_is_the_machines_fonts_and_the_ones_egui_carries() {
    let system = table();
    let machine = system.families().to_vec();
    assert_eq!(machine.len(), 2, "the table holds two fonts: {machine:?}");

    // One of the machine's fonts, and one of egui's, are already in the order.
    let priority = vec![machine[0].clone(), "Hack".to_owned()];

    let (offered, answered) = fonts_to_add("", &system, &priority);

    assert_eq!(answered, offered.len(), "nothing was left out");
    assert_eq!(
        offered,
        [
            "Ubuntu-Light",
            "NotoEmoji-Regular",
            "emoji-icon-font",
            machine[1].as_str()
        ],
        "egui's fonts in the order egui has them, then the machine's, minus the ones in the order"
    );
}

#[test]
fn the_fonts_offered_are_searched_for_rather_than_listed() {
    let system = table();
    let machine = system.families().to_vec();
    let wanted = &machine[1];

    // Typed at the way the marks are: the name in any case, or the first letters of it, finds it
    // and puts it first.
    assert_eq!(
        fonts_to_add(&wanted.to_lowercase(), &system, &[]).0.first(),
        Some(wanted)
    );
    assert_eq!(
        fonts_to_add(&wanted[..3], &system, &[]).0.first(),
        Some(wanted)
    );

    // And a name nothing answers to leaves nothing to offer.
    assert_eq!(fonts_to_add("qqqqq", &system, &[]).0, Vec::<String>::new());
}

#[test]
fn the_dropdown_is_set_into_the_panel_in_the_colour_the_panels_fields_are() {
    let settings = Settings::new("dropdown-inset");
    let (mut app, ctx) = signed_in(settings.config());

    // Something to offer, so that the dropdown that comes up has something in it.
    app.remove_font(&ctx, 0);
    app.config_open = true;

    // A panel fades in, and what it is drawn at is a fraction of its colour until the fade is over.
    // A window also has to be *focused* for egui to call an area settled, which a test's window
    // never is, so the fade never finishes here: it is turned off instead, which is the only way
    // this can be about the colour of anything.
    ctx.options_mut(|options| {
        Arc::make_mut(&mut options.dark_style).animation_time = 0.0;
        Arc::make_mut(&mut options.light_style).animation_time = 0.0;
    });

    for _ in 0..3 {
        frame(&mut app, &ctx, Vec::new());
    }

    // The colour egui sets a panel's fields into — which is the darker of the two the panel and a
    // dropdown could be — and the corners egui rounds a menu's frame by. Both are read from the
    // style rather than written down here: what matters is that the dropdown is drawn in egui's
    // colours, not in any of this file's.
    let theme = ctx.theme();
    let (inset, rounded) = ctx.options(|options| {
        let style = match theme {
            egui::Theme::Dark => &options.dark_style,
            egui::Theme::Light => &options.light_style,
        };

        (
            style.visuals.extreme_bg_color,
            style.visuals.menu_corner_radius,
        )
    });

    // Both halves, because the colour alone is not enough: egui paints every field in this colour,
    // and the panel's own frame is a different one. A frame of this shape is the dropdown's.
    let framed = |app: &mut MarksApp, ctx: &egui::Context| {
        painted_rects(app, ctx)
            .into_iter()
            .filter(|(_, fill, corners)| *fill == inset && *corners == rounded)
            .count()
    };

    // Nothing of it is drawn before it has been pressed: the panel sets its own fields into this
    // colour, and a count that was already above nothing would prove nothing.
    assert_eq!(
        framed(&mut app, &ctx),
        0,
        "something the shape and colour of the dropdown is drawn before the dropdown is up"
    );

    // Pressed, as a select is opened.
    let button = text_pos(&mut app, &ctx, "Add a font");
    click(&mut app, &ctx, button);

    assert!(
        framed(&mut app, &ctx) > 0,
        "the dropdown is not set into the panel; it is the colour of the dialog around it"
    );
}

#[test]
fn the_dropdown_of_fonts_comes_up_when_its_button_is_pressed_and_goes_on_a_press_elsewhere() {
    let settings = Settings::new("picker-focus");
    let (mut app, ctx) = signed_in(settings.config());

    // Something to offer, so that a dropdown with something in it is what is being looked for.
    app.remove_font(&ctx, 0);
    app.config_open = true;

    for _ in 0..3 {
        frame(&mut app, &ctx, Vec::new());
    }

    // The panel is up and nothing has been pressed: the dropdown should be nowhere — neither a row
    // of it nor the field it is searched with. What is there is the control that opens it.
    assert!(
        !drew(&mut app, &ctx, "Hack"),
        "the dropdown is up before anything was pressed"
    );
    assert!(
        !drew(&mut app, &ctx, "Search the fonts on this machine"),
        "the search field is up before the dropdown it belongs to was opened"
    );
    assert!(
        drew(&mut app, &ctx, "Add a font"),
        "there is no control to open the dropdown with"
    );

    // Pressing that control brings it up, with the field that searches it.
    let button = text_pos(&mut app, &ctx, "Add a font");
    click(&mut app, &ctx, button);

    assert!(
        drew(&mut app, &ctx, "Hack"),
        "the dropdown did not come up when its button was pressed"
    );
    assert!(
        drew(&mut app, &ctx, "Search the fonts on this machine"),
        "the dropdown has no search field in it"
    );

    // Pressing one of its rows adds that font and leaves the dropdown up, so that several fonts can
    // be added one after another without opening it again each time. With this machine offering
    // nothing of its own and that one font having been taken out of the order, there is nothing
    // left to offer afterwards, which is what the dropdown says when it has run out.
    let row = text_pos(&mut app, &ctx, "Hack");
    click(&mut app, &ctx, row);

    assert_eq!(
        app.config.fonts.priority.last().map(String::as_str),
        Some("Hack"),
        "pressing a font did not add it"
    );
    assert!(
        drew(&mut app, &ctx, "Nothing by that name is left to add."),
        "the dropdown went away as soon as one of its rows was used"
    );

    // And a press anywhere else closes it. It is the dropdown that goes: what opened it stays.
    let elsewhere = text_pos(&mut app, &ctx, "Window");
    click(&mut app, &ctx, elsewhere);

    assert!(
        !drew(&mut app, &ctx, "Nothing by that name is left to add."),
        "the dropdown is still up after a press somewhere else"
    );
    assert!(
        drew(&mut app, &ctx, "Add a font"),
        "the button went away with the dropdown"
    );
}

#[test]
fn the_panel_offers_a_way_to_add_a_font_even_when_there_is_nothing_to_add() {
    // It was drawn only while a font was missing, so on a client nobody had configured there was
    // no add control at all — which is what a control that appears on a condition is worth.
    let settings = Settings::new("add-control");
    let (mut app, ctx) = signed_in(settings.config());
    app.config_open = true;

    assert_eq!(
        app.config.fonts.priority,
        crate::fonts::default_priority(),
        "this test is about the case with nothing missing"
    );

    // A modal lays itself out on one frame and draws on the next.
    frame(&mut app, &ctx, Vec::new());
    frame(&mut app, &ctx, Vec::new());

    assert!(
        drew(&mut app, &ctx, "Add a font"),
        "the panel has no way to add a font until one is needed"
    );
    assert!(
        drew(&mut app, &ctx, REMOVE_ICON),
        "the panel has no remove control"
    );

    // And the control is honest when pressed with nothing to offer: the order already names every
    // font egui carries, and a window built by these tests has no fonts of its own — so the
    // dropdown comes up saying so rather than empty.
    let button = text_pos(&mut app, &ctx, "Add a font");
    click(&mut app, &ctx, button);

    assert!(
        drew(&mut app, &ctx, "Nothing by that name is left to add."),
        "the dropdown came up with nothing to offer and did not say so"
    );
}

#[test]
fn the_panel_draws_every_control_inside_the_window_it_is_drawn_in() {
    // The window these tests draw in is the size the client opens at, and the panel is wide and
    // tall enough to fill most of it: a control laid out past an edge is a control that is drawn
    // where nobody can see it, which only a test looking at *where* things went can find.
    let settings = Settings::new("panel-fits");
    let (mut app, ctx) = signed_in(settings.config());
    app.config_open = true;

    let windows = [
        input(Vec::new()).screen_rect.expect("a window"),
        egui::Rect::from_min_size(egui::Pos2::ZERO, crate::config::floor()),
    ];

    for window in windows {
        // A frame or two first: the panel remembers the size it was drawn at, so a window that has
        // just changed size takes a frame to be laid out in it.
        for _ in 0..3 {
            let mut raw = input(Vec::new());
            raw.screen_rect = Some(window);
            let _ = ctx.run_ui(raw, |ui| app.show(ui));
        }

        let strayed = stray_text(&mut app, &ctx, window);

        assert!(strayed.is_empty(), "drawn outside {window:?}: {strayed:?}");
    }
}

#[test]
fn the_panel_icons_are_drawn_in_the_clients_own_font_and_not_the_settings_one() {
    // The wiring rather than the ingredients: the icons exist in the client's font and nowhere
    // else, so drawing them from the fonts the settings choose would draw boxes. This is the one
    // line that decides which, and the buttons are built from it.
    assert_eq!(
        icon_font().family,
        egui::FontFamily::Name(crate::fonts::ICON_FAMILY.into()),
        "the icons would be drawn in a font the order can take away"
    );
}

#[test]
fn the_panel_buttons_are_icons_from_a_font_the_settings_cannot_take_away() {
    // Icons rather than words, and from the client's own font rather than from the fonts the
    // settings choose between — an icon drawn in a font somebody has taken out of the order is an
    // icon that comes out as a box.
    let settings = Settings::new("panel-icons");
    let (mut app, ctx) = signed_in(settings.config());
    app.config_open = true;

    frame(&mut app, &ctx, Vec::new());
    frame(&mut app, &ctx, Vec::new());

    for (icon, what) in [
        (SOONER_ICON, "up"),
        (LATER_ICON, "down"),
        (REMOVE_ICON, "remove"),
    ] {
        assert!(drew(&mut app, &ctx, icon), "no {what} icon on the panel");
    }
    assert!(
        !drew(&mut app, &ctx, "Remove"),
        "the word the icon replaced is still there"
    );

    // And they are characters of the font they are drawn in, rather than boxes: asked of the font
    // file, because `has_glyph` cannot answer for the only font in a family.
    let (name, bytes) = ctx.fonts(|fonts| {
        let definitions = fonts.definitions();
        let icon_family = egui::FontFamily::Name(crate::fonts::ICON_FAMILY.into());
        let name = definitions.families[&icon_family]
            .first()
            .expect("a font in the icon family")
            .clone();

        (name.clone(), definitions.font_data[&name].font.clone())
    });

    use ab_glyph::Font as _;

    let font = ab_glyph::FontRef::try_from_slice(&bytes).expect("the icon font");

    for icon in [SOONER_ICON, LATER_ICON, REMOVE_ICON] {
        let character = icon.chars().next().expect("an icon is one character");

        assert!(
            font.glyph_id(character).0 != 0,
            "{name} has no glyph for U+{:04X}",
            character as u32
        );
    }

    // And no font the settings choose between has them, which is what makes drawing the icons from
    // this family the reason they come out as chevrons rather than as boxes.
    let bundled = ctx.fonts(|fonts| fonts.definitions().font_data.clone());

    for (font_name, data) in bundled {
        if font_name == name {
            continue;
        }

        let text_font = ab_glyph::FontRef::try_from_slice(&data.font).expect("a font egui ships");

        for icon in [SOONER_ICON, LATER_ICON, REMOVE_ICON] {
            let character = icon.chars().next().expect("an icon is one character");

            assert!(
                text_font.glyph_id(character).0 == 0,
                "{font_name} has U+{:04X} too, so this proves nothing about which font drew it",
                character as u32
            );
        }
    }
}

#[test]
fn the_query_keeps_the_marks_it_matches() {
    let marks = vec![
        mark("GitHub", "https://github.com/emilk/egui"),
        mark("Svelte", "https://svelte.dev/docs/kit"),
    ];

    assert_eq!(names(&filter_marks(&marks, "")), ["GitHub", "Svelte"]);
    assert_eq!(names(&filter_marks(&marks, "hub")), ["GitHub"]);
    // The query is matched against the link as well as the name, and case does not matter.
    assert_eq!(names(&filter_marks(&marks, "SVELTE.DEV")), ["Svelte"]);
    assert!(filter_marks(&marks, "nothing here").is_empty());
}

#[test]
fn a_typed_link_is_stored_as_one_and_named_after_its_host() {
    assert_eq!(stored_content("example.com"), "https://example.com/");
    assert_eq!(mark_name("https://example.com/some/page"), "example.com");

    // A note keeps the text it was typed as, and is not a link.
    assert_eq!(stored_content("buy milk"), "buy milk");
    assert_eq!(mark_name("buy milk"), "buy milk");
    assert!(web_link("buy milk").is_none());
}

#[test]
fn a_note_is_named_after_its_text_and_is_never_fetched() {
    // Nothing off the machine is asked for a name when there is no page to ask: the
    // fallback is the answer, and typing a note stays as quick as it ever was.
    let content = "buy milk";

    assert_eq!(named_after_title(content, mark_name(content)), "buy milk");
}

#[test]
fn a_link_that_cannot_be_reached_keeps_its_host_name() {
    // Port 1 on loopback refuses immediately, so this is an offline test that still goes
    // through the real fetch: the failure leaves the host name it would have had.
    let content = "http://127.0.0.1:1/";

    assert_eq!(named_after_title(content, mark_name(content)), "127.0.0.1");
}

#[test]
fn a_link_is_named_after_the_title_of_the_page_it_points_at() {
    // The page is served from this machine, so the title that comes back is one this test
    // wrote. This is the naming policy whole: a link that resolves to a page is named after
    // that page, not after its host, and no network or marks server is involved either way.
    let page = test_page::serve(
        "text/html; charset=utf-8",
        "<head><title>Local Test Page</title></head>",
    );
    let content = page.as_str();

    assert_eq!(mark_name(content), "127.0.0.1");
    assert_eq!(
        named_after_title(content, mark_name(content)),
        "Local Test Page"
    );
}

#[test]
fn a_title_longer_than_the_server_accepts_is_cut_to_fit() {
    // A page can say anything as its title, and the server refuses a name past 200
    // characters, so the cut has to happen here rather than after a failed round trip.
    let long = "t".repeat(MAX_NAME_CHARS + 50);
    let page = test_page::serve("text/html", &format!("<title>{long}</title>"));
    let content = page.as_str();

    let name = named_after_title(content, mark_name(content));

    assert_eq!(name.chars().count(), MAX_NAME_CHARS);
    assert!(long.starts_with(&name));
}

/// The viewport commands one frame of the window asks for, in the order it asks for them.
fn viewport_commands(app: &mut MarksApp, ctx: &egui::Context) -> Vec<egui::ViewportCommand> {
    let output = ctx.run_ui(input(Vec::new()), |ui| app.show(ui));

    output
        .viewport_output
        .values()
        .flat_map(|viewport| viewport.commands.clone())
        .collect()
}

/// Whether `wanted` appears among `commands` in that order, not necessarily next to each other.
///
/// A frame asks for more than the settings alone — egui has its own business with the window —
/// so what matters is the order of the ones this test is about.
fn asks_in_order(wanted: &[egui::ViewportCommand], commands: &[egui::ViewportCommand]) -> bool {
    let mut rest = commands;

    for want in wanted {
        let Some(at) = rest.iter().position(|command| command == want) else {
            return false;
        };

        rest = &rest[at + 1..];
    }

    true
}

#[test]
fn fixing_the_size_constrains_the_window_and_does_so_after_resizing_it() {
    let settings = Settings::new("pinning");
    let (mut app, ctx) = window_with("http://localhost:5173", None, settings.config());

    app.set_fixed_size(&ctx, true);

    let size = egui::vec2(
        app.config.window.width as f32,
        app.config.window.height as f32,
    );
    let commands = viewport_commands(&mut app, &ctx);

    // Both ends of the size, then the size itself, and only then "not resizable". That last one
    // is what pins a window where it is: asking for it first would pin it to the size it is
    // leaving, and the new size would never be reached.
    assert!(
        asks_in_order(
            &[
                egui::ViewportCommand::MaxInnerSize(size),
                egui::ViewportCommand::MinInnerSize(size),
                egui::ViewportCommand::InnerSize(size),
                egui::ViewportCommand::Resizable(false),
            ],
            &commands
        ),
        "{commands:?}"
    );
}

#[test]
fn freeing_the_size_lets_the_window_be_resized_again() {
    let settings = Settings::new("freeing");
    let (mut app, ctx) = window_with("http://localhost:5173", None, settings.config());

    app.set_fixed_size(&ctx, true);
    viewport_commands(&mut app, &ctx);

    app.set_fixed_size(&ctx, false);

    let size = egui::vec2(
        app.config.window.width as f32,
        app.config.window.height as f32,
    );
    let commands = viewport_commands(&mut app, &ctx);

    assert!(
        asks_in_order(
            &[
                // No largest size, said as `INFINITY`, or the window stays as tall and as wide
                // as it was pinned to a moment ago.
                egui::ViewportCommand::MaxInnerSize(egui::Vec2::INFINITY),
                egui::ViewportCommand::MinInnerSize(crate::config::floor()),
                egui::ViewportCommand::InnerSize(size),
                egui::ViewportCommand::Resizable(true),
            ],
            &commands
        ),
        "{commands:?}"
    );
}

#[test]
fn the_ctrl_enter_hint_is_only_there_when_there_is_something_to_save() {
    let (mut app, _) = window("http://localhost:5173", None);

    assert_eq!(app.save_hint(), None);

    app.query = "  example.com  ".to_owned();
    let hint = app.save_hint().expect("a hint while something is typed");
    assert!(hint.contains("Ctrl+Enter"), "{hint}");
    assert!(hint.contains("example.com"), "{hint}");
}

#[test]
fn a_hint_repeats_the_query_until_it_is_too_long() {
    assert_eq!(shorten("example.com", HINT_CHARS), "example.com");
    assert_eq!(shorten("exactly-ten", 11), "exactly-ten");

    let cut = shorten(&"x".repeat(100), 10);
    assert_eq!(cut.chars().count(), 11);
    assert!(cut.ends_with('…'));
}


