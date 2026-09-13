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
    let app = MarksApp::new(&ctx, base_url.to_owned(), token.map(str::to_owned), config);

    (app, ctx)
}

/// Runs one frame of the whole window with `events` as this frame's input.
fn frame(app: &mut MarksApp, ctx: &egui::Context, events: Vec<egui::Event>) {
    let input = egui::RawInput {
        events,
        ..Default::default()
    };
    // The frame's output (shapes, viewport commands) is not what these tests look at.
    let _ = ctx.run_ui(input, |ui| app.show(ui));
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
        ctx.run_ui(egui::RawInput::default(), |ui| app.show(ui))
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

    let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
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

    let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
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
