//! The tests for `config`: the settings file, and what becomes of one that is wrong.

use super::*;

/// A settings file of this test's own, removed again when the test ends.
///
/// Tests never touch the real config directory: a test that wrote there would leave a settings
/// file behind for the next run of the window to pick up.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "marks-client-config-{}-{name}",
            std::process::id()
        ));

        let _ = fs::remove_dir_all(&dir);

        Self(dir.join(FILE_NAME))
    }

    /// A configuration that reads and writes this file.
    fn config(&self) -> Config {
        Config::load_from(self.0.clone())
    }

    /// What the settings file says, as it says it.
    fn contents(&self) -> String {
        fs::read_to_string(&self.0).expect("the settings file")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if let Some(parent) = self.0.parent() {
            let _ = fs::remove_dir_all(parent);
        }
    }
}

#[test]
fn the_window_opens_the_way_it_always_has() {
    let window = Window::default();

    assert!(!window.fixed_size, "a launcher should be resizable by default");
    assert_eq!((window.width, window.height), (620, 440));
}

#[test]
fn a_setting_that_was_written_is_read_back() {
    let file = Scratch::new("round-trip");
    let mut config = file.config();

    config.window.fixed_size = true;
    config.window.width = 800;
    config.window.height = 600;
    config.save();

    let read_back = file.config();

    assert!(read_back.window.fixed_size);
    assert_eq!((read_back.window.width, read_back.window.height), (800, 600));
}

#[test]
fn a_client_nobody_has_configured_runs_on_the_defaults() {
    let file = Scratch::new("absent");

    let config = file.config();

    assert_eq!(config.window.width, Window::default().width);
    assert_eq!(config.window.height, Window::default().height);
    assert!(!config.window.fixed_size);
    // And it knows where it would write, so the first change is kept.
    assert_eq!(config.path(), Some(file.0.as_path()));
}

#[test]
fn a_file_that_is_not_toml_is_not_fatal() {
    let file = Scratch::new("nonsense");
    fs::create_dir_all(file.0.parent().expect("a parent")).expect("a directory");

    for contents in ["", "not toml at all", "[window\nwidth = ", "window = 3"] {
        fs::write(&file.0, contents).expect("a file written");

        let config = file.config();

        assert_eq!(
            config.window.width,
            Window::default().width,
            "contents: {contents}"
        );
    }
}

#[test]
fn a_file_that_names_one_setting_leaves_the_rest_alone() {
    let file = Scratch::new("partial");
    fs::create_dir_all(file.0.parent().expect("a parent")).expect("a directory");
    fs::write(&file.0, "[window]\nfixed_size = true\n").expect("a file written");

    let config = file.config();

    // What the file says wins; what it does not mention is the default.
    assert!(config.window.fixed_size);
    assert_eq!(config.window.width, Window::default().width);
    assert_eq!(config.window.height, Window::default().height);
}

#[test]
fn a_setting_this_version_does_not_know_is_left_alone() {
    let file = Scratch::new("newer");
    fs::create_dir_all(file.0.parent().expect("a parent")).expect("a directory");
    fs::write(
        &file.0,
        "[window]\nwidth = 700\nheight = 500\n\n[future]\nsomething = true\n",
    )
    .expect("a file written");

    // A file written by a later version has to keep working here: the settings that are known
    // are read, and the ones that are not are simply not this version's business.
    let config = file.config();

    assert_eq!((config.window.width, config.window.height), (700, 500));
}

#[test]
fn a_size_no_window_could_be_is_brought_within_reach() {
    let file = Scratch::new("absurd");
    fs::create_dir_all(file.0.parent().expect("a parent")).expect("a directory");
    fs::write(&file.0, "[window]\nwidth = 3\nheight = 999999\n").expect("a file written");

    let config = file.config();

    assert_eq!(config.window.width, MIN_WIDTH, "too narrow to draw the list");
    assert_eq!(config.window.height, MAX_SIDE, "larger than any display");
}

#[test]
fn a_fixed_window_cannot_be_resized_and_one_that_is_not_has_a_floor() {
    let fixed = Window {
        fixed_size: true,
        width: 800,
        height: 600,
    };
    let viewport = fixed.viewport();

    assert_eq!(viewport.resizable, Some(false));
    assert_eq!(viewport.inner_size, Some(egui::vec2(800.0, 600.0)));
    // Pinned at both ends, not merely asked not to be resized: a window manager is free to
    // ignore "not resizable", and a size that is its own minimum and maximum is obeyed.
    assert_eq!(viewport.min_inner_size, Some(egui::vec2(800.0, 600.0)));
    assert_eq!(viewport.max_inner_size, Some(egui::vec2(800.0, 600.0)));

    let free = Window {
        fixed_size: false,
        ..fixed
    };
    let viewport = free.viewport();

    assert_eq!(viewport.resizable, Some(true));
    assert_eq!(viewport.inner_size, Some(egui::vec2(800.0, 600.0)));
    // Free to be resized, but not below the size the list needs, and with no ceiling at all.
    assert_eq!(viewport.min_inner_size, Some(floor()));
    assert_eq!(viewport.max_inner_size, None);
}

#[test]
fn the_default_order_draws_with_hack() {
    // What a client nobody has configured draws with: Hack, the only font egui carries that has
    // the arrows the footer is written with.
    let fonts = Fonts::default();

    assert_eq!(fonts.priority.first().map(String::as_str), Some("Hack"));
    assert_eq!(fonts.priority, crate::fonts::default_priority());
}

#[test]
fn a_font_order_that_was_written_is_read_back() {
    let file = Scratch::new("fonts-round-trip");
    let mut config = file.config();

    config.fonts.priority = vec!["Ubuntu-Light".to_owned(), "Hack".to_owned()];
    config.save();

    assert_eq!(file.config().fonts.priority, ["Ubuntu-Light", "Hack"]);
    // Written as a list in the file, where it can be read and reordered by hand.
    assert!(file.contents().contains("[fonts]"), "{}", file.contents());
    assert!(file.contents().contains("priority = ["), "{}", file.contents());
}

#[test]
fn a_font_the_client_does_not_carry_is_left_in_the_order() {
    let file = Scratch::new("fonts-unknown");
    fs::create_dir_all(file.0.parent().expect("a parent")).expect("a directory");
    fs::write(
        &file.0,
        "[fonts]\npriority = [\"Hack\", \"Comic Sans\", \"Ubuntu-Light\"]\n",
    )
    .expect("a file written");

    // It may be a font on this machine — nothing here can tell without reading every font the
    // machine has, which is not something reading a settings file does — so it is left where it
    // is. One that turns out to be no font at all is left out when the order is put onto the
    // window, and the panel offers the fonts that are actually there.
    assert_eq!(
        file.config().fonts.priority,
        ["Hack", "Comic Sans", "Ubuntu-Light"]
    );
}

#[test]
fn a_font_named_twice_is_drawn_once() {
    let file = Scratch::new("fonts-twice");
    fs::create_dir_all(file.0.parent().expect("a parent")).expect("a directory");
    fs::write(
        &file.0,
        "[fonts]\npriority = [\"Hack\", \"Hack\", \"Ubuntu-Light\"]\n",
    )
    .expect("a file written");

    assert_eq!(file.config().fonts.priority, ["Hack", "Ubuntu-Light"]);
}

#[test]
fn an_empty_font_order_becomes_the_default_one() {
    let file = Scratch::new("fonts-empty");
    fs::create_dir_all(file.0.parent().expect("a parent")).expect("a directory");
    fs::write(&file.0, "[fonts]\npriority = []\n").expect("a file written");

    // Nothing to draw with is not a state to leave the window in.
    assert_eq!(file.config().fonts.priority, crate::fonts::default_priority());
}

#[test]
fn settings_are_kept_in_the_config_directory_and_not_in_the_data_one() {
    let path = path().expect("a config directory");

    assert!(
        path.ends_with(Path::new(APP_DIR).join(FILE_NAME)),
        "{}",
        path.display()
    );
    assert!(path.starts_with(dirs::config_dir().expect("a config directory")));

    // The session goes the other way round (see `session_file`): settings are the user's, state
    // is the client's.
    assert!(!path.starts_with(dirs::data_local_dir().expect("a data directory")));
}

#[test]
fn a_new_file_says_what_it_is() {
    let file = Scratch::new("header");

    file.config().save();

    let contents = fs::read_to_string(&file.0).expect("the settings file");
    assert!(contents.starts_with('#'), "no comment at the top: {contents}");
    assert!(contents.contains("Ctrl+,"), "{contents}");
}

#[test]
fn a_save_keeps_the_settings_and_replaces_the_rest_of_the_file() {
    let file = Scratch::new("replaced");
    fs::create_dir_all(file.0.parent().expect("a parent")).expect("a directory");
    // Settings put there by hand, with a comment around them.
    fs::write(&file.0, "# mine\n[window]\nwidth = 700\nheight = 500\n").expect("a file written");

    let config = file.config();
    config.save();

    let contents = fs::read_to_string(&file.0).expect("the settings file");

    // What the file said is what the client now holds, and what it wrote back still says it...
    assert!(contents.contains("width = 700"), "{contents}");
    assert!(contents.contains("height = 500"), "{contents}");
    assert_eq!(file.config().window.width, 700);

    // ...but the file itself is the client's, comments and all. A hand-written comment is not
    // preserved, which the file says for itself in its own header.
    assert!(!contents.contains("# mine"), "{contents}");
    assert!(contents.contains("comments added by hand do not survive"), "{contents}");
}
