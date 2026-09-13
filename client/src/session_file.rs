//! The session, kept where the next run of the client can find it.
//!
//! It is written under the **local data** directory — `$XDG_DATA_HOME/marks-client` on Linux,
//! `~/.local/share/marks-client` by default — which is the directory a native application
//! keeps its own state in, and the one Tauri resolves for `app_local_data_dir`. It is
//! deliberately not the config directory: nothing here is meant to be read or edited by hand,
//! which is all the config directory is for.
//!
//! A session is a bearer token rather than a password, and it expires: the server stops
//! accepting it the moment it does. It is still worth keeping out of everyone else's reach, so
//! the file is written owner-only inside a directory that is owner-only too.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The directory this client owns under the local data directory.
const APP_DIR: &str = "marks-client";

/// The file the session is kept in.
const FILE_NAME: &str = "session.json";

/// What is written to disk: the token, and the server that issued it.
///
/// The server is part of it because a token means nothing to any other server. Keeping the two
/// together is what stops a session from one marks server being offered to whichever one
/// `MARKS_URL` happens to point at next.
#[derive(Deserialize, Serialize)]
struct Stored {
    base_url: String,
    token: String,
}

/// Where this client keeps its state: `$XDG_DATA_HOME/marks-client` on Linux.
///
/// `None` when the system has no local data directory to speak of — no `HOME`, typically —
/// which leaves the client working exactly as it did before it kept a session at all.
pub fn data_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|dir| dir.join(APP_DIR))
}

/// The session token `base_url` issued in an earlier run, if it is still there.
pub fn load(base_url: &str) -> Option<String> {
    read_from(&data_dir()?, base_url)
}

/// The session kept on disk, and the server that issued it, whoever that is.
///
/// [`load`] asks for the session of a named server, which is what a window that already knows which
/// server it is talking to wants. This asks what is there at all, which is what a window that has
/// not been told yet wants: this client has no server of its own to fall back on, so the address it
/// should offer is the one the last run's session was kept for.
pub fn stored() -> Option<(String, String)> {
    stored_in(&data_dir()?)
}

/// Keeps `token` for `base_url`, so that the next run starts signed in.
///
/// Not being able to write is not worth failing a sign-in over: the session works for as long
/// as this window is open either way, so a problem is reported and nothing else happens.
pub fn save(base_url: &str, token: &str) {
    let Some(dir) = data_dir() else {
        eprintln!("marks-client: no local data directory to keep the session in");
        return;
    };

    if let Err(error) = write_to(&dir, base_url, token) {
        eprintln!("marks-client: could not keep the session: {error}");
    }
}

/// Forgets the session the server has just refused.
///
/// `token` is the one that was refused, and the stored session goes only when it is that one.
/// A session handed in through `MARKS_TOKEN` being rejected is not a reason to throw away a
/// session on disk that may well still be good, and a session belonging to another server is
/// not this server's to forget either.
pub fn forget(base_url: &str, token: &str) {
    let Some(dir) = data_dir() else {
        return;
    };

    if let Err(error) = remove_from(&dir, base_url, token) {
        eprintln!("marks-client: could not forget the session: {error}");
    }
}

/// The session in `dir`, when there is one and it belongs to `base_url`.
fn read_from(dir: &Path, base_url: &str) -> Option<String> {
    stored_in(dir)
        .filter(|(stored, _)| stored == base_url)
        .map(|(_, token)| token)
}

/// The session in `dir`, with the server it belongs to: the whole of the file, when it is one.
fn stored_in(dir: &Path) -> Option<(String, String)> {
    let contents = fs::read_to_string(dir.join(FILE_NAME)).ok()?;

    // A file that is truncated, hand-edited, or left over from another version of this client
    // is simply not a session. Answering `None` puts the sign-in dialog up, which is the right
    // answer to all three, and leaves the file for `save` to overwrite.
    let stored: Stored = serde_json::from_str(&contents).ok()?;

    // A session with no server is not one, whatever token came with it: the address is what the
    // sign-in dialog would be filled in with, and a token with nowhere to present it is nothing
    // this client can use.
    (!stored.base_url.is_empty() && !stored.token.is_empty()).then_some((stored.base_url, stored.token))
}

/// Writes `token` into `dir`, making the directory if it is not there yet.
fn write_to(dir: &Path, base_url: &str, token: &str) -> std::io::Result<()> {
    let stored = Stored {
        base_url: base_url.to_owned(),
        token: token.to_owned(),
    };
    let contents = serde_json::to_string(&stored).map_err(std::io::Error::other)?;

    fs::create_dir_all(dir)?;
    restrict_dir(dir)?;
    write_file(&dir.join(FILE_NAME), &contents)
}

/// Removes the stored session, unless it is some other session than the one being forgotten.
fn remove_from(dir: &Path, base_url: &str, token: &str) -> std::io::Result<()> {
    let path = dir.join(FILE_NAME);

    // Read before removing, so that a session that is not this one survives.
    match fs::read_to_string(&path) {
        // Another server's, or another token for this one: left where it is.
        Ok(contents) if is_another_session(&contents, base_url, token) => return Ok(()),
        // No session to forget, which is the state being asked for anyway.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        // Anything else — unreadable, or not a session at all — is a file to remove rather
        // than one to preserve.
        _ => {}
    }

    fs::remove_file(path)
}

/// Whether `contents` is a stored session other than the one described.
///
/// A file that cannot be read as a session is not another session: there is nothing in it worth
/// keeping, and it is in the way of the next sign-in writing over it.
fn is_another_session(contents: &str, base_url: &str, token: &str) -> bool {
    serde_json::from_str::<Stored>(contents)
        .is_ok_and(|stored| stored.base_url != base_url || stored.token != token)
}

/// Writes the file with the permissions it should keep.
///
/// The mode is set as the file is opened rather than afterwards, because a file that is
/// readable by everyone for as long as it takes to chmod is still a file that was readable by
/// everyone.
#[cfg(unix)]
fn write_file(path: &Path, contents: &str) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;

    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;

    file.write_all(contents.as_bytes())
}

#[cfg(not(unix))]
fn write_file(path: &Path, contents: &str) -> std::io::Result<()> {
    fs::write(path, contents)
}

/// Narrows the directory's own permissions, where the platform has permissions to narrow.
///
/// The file is the thing worth protecting, but a directory only its owner can open is the
/// cheaper half of the same idea.
#[cfg(unix)]
fn restrict_dir(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn restrict_dir(_dir: &Path) -> std::io::Result<()> {
    Ok(())
}

/// The tests, in a file of their own: `session_file/tests.rs`, compiled only for test builds.
#[cfg(test)]
mod tests;
