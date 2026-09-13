//! The tests for `icon_cache`, in directories of their own rather than the one this machine keeps
//! favicons in.

use super::*;

/// A cache in a directory of this test's own, removed again when the test ends.
///
/// The real one is under the machine's cache directory, which a test has no business writing to:
/// what it left there would be read by the next window to be opened.
struct Scratch(IconFiles);

impl Scratch {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "marks-client-icons-{}-{name}",
            std::process::id()
        ));

        let _ = fs::remove_dir_all(&dir);

        Self(IconFiles::in_dir(dir.join(APP_DIR)))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if let Some(parent) = self.0.dir.parent() {
            let _ = fs::remove_dir_all(parent);
        }
    }
}

/// Some bytes that stand in for a favicon: nothing here decodes them, only stores them.
fn bytes() -> Vec<u8> {
    vec![0x89, b'P', b'N', b'G', 1, 2, 3, 4]
}

const ICON: &str = "3f7e64ad-bace-42ce-843b-86ac7a180ec7";

#[test]
fn a_favicon_that_was_kept_is_read_back() {
    let cache = Scratch::new("round-trip");

    assert_eq!(cache.0.read(ICON), None, "nothing kept yet");

    cache.0.write(ICON, &bytes());

    assert_eq!(cache.0.read(ICON), Some(bytes()));
}

#[test]
fn nothing_kept_is_nothing_read() {
    let cache = Scratch::new("absent");

    // A directory that was never written to, and one that has been swept, read the same way.
    assert_eq!(cache.0.read(ICON), None);

    cache.0.write(ICON, &bytes());
    cache.0.forget(ICON);

    assert_eq!(cache.0.read(ICON), None);
    // Forgetting what was never there is not a complaint either.
    cache.0.forget(ICON);
}

#[test]
fn an_empty_file_is_not_a_favicon() {
    let cache = Scratch::new("empty");
    fs::create_dir_all(&cache.0.dir).expect("a directory");
    fs::write(cache.0.dir.join(ICON), []).expect("a file");

    // A file with nothing in it is what a write that was cut short leaves; it is not an icon, and
    // answering with it would draw an empty square rather than ask for the real one.
    assert_eq!(cache.0.read(ICON), None);
}

#[test]
fn no_half_written_file_is_left_behind() {
    let cache = Scratch::new("partial");

    cache.0.write(ICON, &bytes());

    let left: Vec<String> = fs::read_dir(&cache.0.dir)
        .expect("the cache directory")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();

    assert_eq!(left, [ICON], "the name written under was left behind");
}

#[test]
fn one_servers_favicons_are_not_anothers() {
    // The same id from two servers: an id means only what the server that issued it says, so the
    // two are kept apart rather than overwriting each other.
    let first = IconFiles::for_server("http://localhost:5173").expect("a cache");
    let second = IconFiles::for_server("https://marks.example.com").expect("a cache");

    assert_ne!(first.dir, second.dir);
    assert!(first.dir.ends_with("http_localhost_5173"), "{:?}", first.dir);
    assert!(
        second.dir.ends_with("https_marks.example.com"),
        "{:?}",
        second.dir
    );
}

#[test]
fn the_files_are_kept_under_the_cache_directory_and_nowhere_else() {
    let dir = IconFiles::for_server("http://localhost:5173")
        .expect("a cache")
        .dir;

    // The cache directory, which is for copies of things that are somewhere else — not the config
    // directory, which is for what the user edits, and not the data directory, which is for what
    // the client itself owns (see `session_file`).
    assert!(dir.starts_with(dirs::cache_dir().expect("a cache directory")));
    assert!(!dir.starts_with(dirs::config_dir().expect("a config directory")));
    assert!(!dir.starts_with(dirs::data_local_dir().expect("a data directory")));
}

#[test]
fn an_address_that_is_not_a_url_still_names_a_directory() {
    // The settings can hold anything a user typed, and a cache directory is a poor reason to
    // refuse to draw a window.
    let name = server_name("not a url at all");

    assert!(!name.is_empty());
    assert!(
        name.chars().all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_')),
        "{name:?}"
    );
    assert!(!name.contains(std::path::MAIN_SEPARATOR));
}
