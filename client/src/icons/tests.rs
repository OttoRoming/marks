//! The tests for `icons`: what is fetched, what is kept, and what is not asked for twice.

use super::*;
use crate::test_page;

/// A favicon to serve: a real PNG, because what comes back is decoded before it is kept.
fn png() -> Vec<u8> {
    use image::ImageEncoder as _;

    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(&[255, 0, 0, 255, 0, 255, 0, 255], 2, 1, image::ExtendedColorType::Rgba8)
        .expect("a png");

    bytes
}

/// One mark's favicon to fetch.
fn job(mark_id: &str, icon_id: &str) -> Job {
    Job {
        mark_id: mark_id.to_owned(),
        icon_id: icon_id.to_owned(),
    }
}

/// A cache in a directory of this test's own, removed again when the test ends.
///
/// The one under the machine's cache directory is not a test's to write to: what it left there
/// would be read by the next window to be opened.
struct Scratch {
    files: IconFiles,
    root: std::path::PathBuf,
}

impl Scratch {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "marks-client-icon-pool-{}-{name}",
            std::process::id()
        ));

        let _ = fs::remove_dir_all(&root);

        Self {
            files: IconFiles::in_dir(root.clone()),
            root,
        }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// What the server answers a request for a favicon with: the bytes, as an image.
fn serving_a_favicon() -> test_page::Served {
    let bytes = png();

    test_page::serve_bytes("HTTP/1.1 200 OK\r\ncontent-type: image/png\r\n", &bytes)
}

#[test]
fn a_favicon_that_was_fetched_once_is_not_fetched_again() {
    let cache = Scratch::new("second-run");
    let server = serving_a_favicon();
    let api = Api::new(server.url.as_str());
    let job = job("mark-1", "icon-1");

    // The first run: the server has it, and is asked for it.
    let fetched = image_for(&api, Some(&cache.files), &job).expect("a favicon");
    assert!(
        cache.files.read("icon-1").is_some(),
        "what was fetched was not kept"
    );

    // The server answers one request and then stops, so a second one would fail: coming back with
    // the image again is the cache doing its work, and nothing else.
    let again = image_for(&api, Some(&cache.files), &job).expect("a favicon from the cache");

    assert_eq!(fetched.size, again.size);
    assert_eq!(
        fetched.pixels.len(),
        again.pixels.len(),
        "the cached image is not the one that was fetched"
    );
}

#[test]
fn a_window_with_nowhere_to_keep_them_still_draws_them() {
    // No cache directory, or none the client may write to: every favicon is fetched, and nothing
    // is kept — which is what it was before any of this existed.
    let server = serving_a_favicon();
    let api = Api::new(server.url.as_str());

    assert!(image_for(&api, None, &job("mark-1", "icon-1")).is_some());
}

#[test]
fn a_kept_favicon_that_will_not_decode_is_thrown_away_and_fetched_again() {
    let cache = Scratch::new("poisoned");
    // Something that is not an image at all, kept where an image should be.
    cache.files.write("icon-1", b"not an image, whatever it is");

    let server = serving_a_favicon();
    let api = Api::new(server.url.as_str());

    // Asked for rather than drawn as a broken image, and the file that was in the way is gone.
    let image = image_for(&api, Some(&cache.files), &job("mark-1", "icon-1"));
    assert!(image.is_some(), "the good copy was not fetched in its place");

    let kept = cache.files.read("icon-1").expect("the good copy, kept");
    assert_ne!(kept, b"not an image, whatever it is");
}

#[test]
fn a_favicon_that_cannot_be_fetched_is_not_kept() {
    let cache = Scratch::new("nothing-to-keep");
    // A server that will not be there.
    let api = Api::new("http://127.0.0.1:1");

    assert!(image_for(&api, Some(&cache.files), &job("mark-1", "icon-1")).is_none());
    assert_eq!(
        cache.files.read("icon-1"),
        None,
        "something was kept for a favicon that was never fetched"
    );
}
