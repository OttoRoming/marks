//! The tests for `session_file`, in directories of their own rather than the one this machine
//! keeps a session in.

use super::*;

/// A directory to keep a session in, of this test's own, removed again when the test ends.
///
/// Tests never touch the real local data directory: a test that wrote to the one this machine
/// uses would leave a session behind for the next run of the window to pick up.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "marks-client-session-{}-{name}",
            std::process::id()
        ));

        // A run that was interrupted leaves its directory behind; starting from nothing is
        // what makes a test's result its own.
        let _ = fs::remove_dir_all(&dir);

        Self(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Handed to the functions under test as the `Path` it wraps.
impl std::ops::Deref for Scratch {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.0
    }
}

const SERVER: &str = "http://marks.test";
const OTHER_SERVER: &str = "http://other.test";

#[test]
fn a_session_is_still_there_when_the_client_next_asks_for_it() {
    let dir = Scratch::new("round-trip");

    write_to(&dir, SERVER, "token-value").expect("a session written");

    assert_eq!(read_from(&dir, SERVER).as_deref(), Some("token-value"));
}

#[test]
fn a_session_is_kept_for_the_server_that_issued_it() {
    let dir = Scratch::new("other-server");

    write_to(&dir, SERVER, "token-value").expect("a session written");

    // Offered to the server it came from it is a session; to any other it is nothing, and in
    // particular it is not sent there.
    assert_eq!(read_from(&dir, OTHER_SERVER), None);
    assert_eq!(read_from(&dir, SERVER).as_deref(), Some("token-value"));
}

#[test]
fn what_is_stored_can_be_read_without_knowing_which_server_it_is_for() {
    // Which is what the window needs at startup: it is not told which server to talk to, so the
    // address in the sign-in dialog can only come from what the last run left behind.
    let dir = Scratch::new("whatever-is-stored");

    write_to(&dir, SERVER, "token-value").expect("a session written");

    assert_eq!(
        stored_in(&dir),
        Some((SERVER.to_owned(), "token-value".to_owned()))
    );
}

#[test]
fn a_file_with_no_server_in_it_is_not_a_session_to_offer_anyone() {
    // A token with nowhere to present it is not a session, and an address is what the dialog would
    // be filled in with: a file missing either half is no help to a window that has neither.
    let dir = Scratch::new("no-server");
    fs::create_dir_all(&*dir).expect("a directory");
    let path = dir.join(FILE_NAME);

    for contents in [
        r#"{"base_url":"","token":"token-value"}"#,
        r#"{"base_url":"http://marks.test","token":""}"#,
    ] {
        fs::write(&path, contents).expect("a file written");

        assert_eq!(stored_in(&dir), None, "contents: {contents}");
    }
}

#[test]
fn writing_creates_the_directory_it_keeps_the_session_in() {
    // The guard is held rather than taken as a temporary, so that it is still there to clean up
    // after the test has made the directory below it.
    let scratch = Scratch::new("nested");
    let dir = scratch.join("marks-client");

    assert!(!dir.exists());

    write_to(&dir, SERVER, "token-value").expect("a session written");

    assert!(dir.join(FILE_NAME).is_file());
}

#[test]
fn no_session_is_no_session() {
    // A directory that was never written to, and one that was forgotten, read the same way.
    let dir = Scratch::new("nothing");

    assert_eq!(read_from(&dir, SERVER), None);

    write_to(&dir, SERVER, "token-value").expect("a session written");
    remove_from(&dir, SERVER, "token-value").expect("a session forgotten");

    assert_eq!(read_from(&dir, SERVER), None);
}

#[test]
fn a_file_that_is_not_a_session_is_not_used() {
    let dir = Scratch::new("nonsense");
    fs::create_dir_all(&*dir).expect("a directory");
    let path = dir.join(FILE_NAME);

    // Truncated, not JSON at all, JSON of the wrong shape, and a session with no token in it:
    // every one of them is answered with the sign-in dialog rather than a panic.
    for contents in [
        "",
        "not json",
        "{}",
        r#"{"base_url":"http://marks.test"}"#,
        r#"{"base_url":"http://marks.test","token":""}"#,
        r#"{"base_url":null,"token":"x"}"#,
    ] {
        fs::write(&path, contents).expect("a file written");

        assert_eq!(read_from(&dir, SERVER), None, "contents: {contents}");
    }
}

#[test]
fn forgetting_takes_the_session_that_was_refused_and_leaves_the_rest() {
    let dir = Scratch::new("forgetting");

    write_to(&dir, SERVER, "token-value").expect("a session written");

    // Another token for the same server is not the one that was refused, so the session on
    // disk — which may well still be good — is left where it is.
    remove_from(&dir, SERVER, "some-other-token").expect("nothing to forget");
    assert_eq!(read_from(&dir, SERVER).as_deref(), Some("token-value"));

    // And a session belonging to another server is not this server's to forget.
    remove_from(&dir, OTHER_SERVER, "token-value").expect("nothing to forget");
    assert_eq!(read_from(&dir, SERVER).as_deref(), Some("token-value"));

    // The token that was refused is the one that goes.
    remove_from(&dir, SERVER, "token-value").expect("a session forgotten");
    assert_eq!(read_from(&dir, SERVER), None);
}

#[test]
fn forgetting_a_session_that_was_never_there_is_harmless() {
    let dir = Scratch::new("never-there");

    // Nothing to remove, and nothing to complain about: this is what a 401 on a session that
    // was never stored looks like.
    remove_from(&dir, SERVER, "token-value").expect("nothing to forget");

    write_to(&dir, SERVER, "token-value").expect("a session written");
    remove_from(&dir, SERVER, "token-value").expect("a session forgotten");
    remove_from(&dir, SERVER, "token-value").expect("forgotten twice");
}

#[test]
fn forgetting_a_file_that_is_not_a_session_removes_it() {
    let dir = Scratch::new("forgetting-nonsense");
    fs::create_dir_all(&*dir).expect("a directory");
    let path = dir.join(FILE_NAME);

    // Nothing in there is a session, so nothing in there is worth preserving either: a file
    // left behind is only in the way of the next sign-in writing over it.
    for contents in ["", "not json", "{}"] {
        fs::write(&path, contents).expect("a file written");

        remove_from(&dir, SERVER, "token-value").expect("a file forgotten");

        assert!(!path.exists(), "contents: {contents}");
    }
}

#[cfg(unix)]
#[test]
fn the_session_is_readable_only_by_its_owner() {
    use std::os::unix::fs::PermissionsExt;

    let dir = Scratch::new("permissions");

    write_to(&dir, SERVER, "token-value").expect("a session written");

    let mode = |path: &Path| {
        fs::metadata(path)
            .expect("a file that exists")
            .permissions()
            .mode()
            & 0o777
    };

    assert_eq!(mode(&dir.join(FILE_NAME)), 0o600, "the session file");
    assert_eq!(mode(&dir), 0o700, "the directory holding it");
}

#[test]
fn state_is_kept_in_the_local_data_directory_and_not_in_the_config_one() {
    let dir = data_dir().expect("a local data directory");

    // The two halves of what matters here: under the local data directory (`$XDG_DATA_HOME`
    // on Linux, `%LOCALAPPDATA%` on Windows), in a directory of this client's own.
    assert!(dir.ends_with(APP_DIR), "{}", dir.display());
    assert_eq!(dir.parent(), dirs::data_local_dir().as_deref());

    // And never in the config directory, which is where settings the user edits belong.
    assert_ne!(
        dir,
        dirs::config_dir().expect("a config directory").join(APP_DIR)
    );
}

#[test]
fn the_session_is_written_as_json_that_can_be_read_back() {
    let dir = Scratch::new("format");

    write_to(&dir, SERVER, "token-value").expect("a session written");

    let contents = fs::read_to_string(dir.join(FILE_NAME)).expect("the session file");
    let stored: Stored = serde_json::from_str(&contents).expect("what was written");

    assert_eq!(stored.base_url, SERVER);
    assert_eq!(stored.token, "token-value");
}
