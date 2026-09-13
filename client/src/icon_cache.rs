//! The favicons, kept on disk between runs.
//!
//! The server stores them — one `icon` row per hostname, with the bytes in it — but a window that
//! has none of its own asks for every one of them again on every run, and decodes every one of
//! them again. They are written under the **cache** directory instead:
//! `$XDG_CACHE_HOME/marks-client/<server>/<icon id>`, which is what that directory is for. What is
//! there is a copy of something the server still has, so deleting it costs a round trip and nothing
//! else.
//!
//! A favicon is named by the `icon_id` the server issued for it, which is a row in its own table
//! and does not change once written, and the files are kept apart by server — an id means only what
//! the server that issued it says it means.

use std::fs;
use std::path::PathBuf;

/// The directory this client's cache lives under, inside the cache directory.
const APP_DIR: &str = "marks-client";

/// What a file is called while it is being written.
///
/// Written beside its real name and then moved onto it: a rename is atomic and a write is not, so
/// a window closed halfway through one would otherwise leave a half-written icon to be read next
/// time as a broken one.
const PARTIAL: &str = ".part";

/// The favicons this machine has already been given, kept for one server.
pub struct IconFiles {
    /// The directory they are kept in: `<cache>/marks-client/<server>`.
    dir: PathBuf,
}

impl IconFiles {
    /// The cache belonging to `base_url`, or `None` when this system has no cache directory to
    /// keep one in — a window without a cache is a slower window, not a broken one.
    pub fn for_server(base_url: &str) -> Option<Self> {
        let dir = dirs::cache_dir()?.join(APP_DIR).join(server_name(base_url));

        Some(Self { dir })
    }

    /// A cache in `dir` rather than under the machine's cache directory, which is what the tests
    /// use: the real one would be read by the next window to be opened.
    #[cfg(test)]
    pub(crate) fn in_dir(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// The bytes stored for `icon_id`, when there are any to read.
    pub fn read(&self, icon_id: &str) -> Option<Vec<u8>> {
        fs::read(self.dir.join(icon_id))
            .ok()
            .filter(|bytes| !bytes.is_empty())
    }

    /// Stores `bytes` as the favicon for `icon_id`.
    ///
    /// A write that fails is reported and otherwise ignored: the icon it was for is already in
    /// hand, and what a cache that cannot be written costs is a round trip next time.
    pub fn write(&self, icon_id: &str, bytes: &[u8]) {
        if let Err(error) = self.write_to(icon_id, bytes) {
            eprintln!("marks-client: could not keep the favicon for {icon_id}: {error}");
        }
    }

    fn write_to(&self, icon_id: &str, bytes: &[u8]) -> std::io::Result<()> {
        fs::create_dir_all(&self.dir)?;

        let mut partial = icon_id.to_owned();
        partial.push_str(PARTIAL);

        let partial = self.dir.join(partial);
        fs::write(&partial, bytes)?;

        fs::rename(partial, self.dir.join(icon_id))
    }

    /// Throws away what is stored for `icon_id`.
    ///
    /// What a stored image turns out to be worth when it will not decode — and it has to go, or
    /// the good copy that would be fetched in its place never would be.
    pub fn forget(&self, icon_id: &str) {
        let _ = fs::remove_file(self.dir.join(icon_id));
    }
}

/// A directory name for the server at `base_url`.
///
/// A cache is kept per server because an icon id means only what the server that issued it says:
/// two servers could issue the same one, and the favicon for a mark on one of them is not the
/// favicon for the mark of that id on the other. The port and the scheme are part of it for the
/// same reason — `localhost:5173` and a site at `localhost` are not the same server, and neither
/// are `http://` and `https://` of one host.
fn server_name(base_url: &str) -> String {
    let named = match url::Url::parse(base_url) {
        Ok(url) => match url.host_str() {
            Some(host) => match url.port() {
                Some(port) => format!("{}_{host}_{port}", url.scheme()),
                None => format!("{}_{host}", url.scheme()),
            },
            None => base_url.to_owned(),
        },
        Err(_) => base_url.to_owned(),
    };

    // Anything that is not a name character becomes one, so that the result is a directory name
    // whatever the settings hold and whatever a caller passes.
    named
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

/// The tests, in a file of their own: `icon_cache/tests.rs`, compiled only for test builds.
#[cfg(test)]
mod tests;
