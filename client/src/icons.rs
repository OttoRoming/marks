//! The favicons beside the marks.
//!
//! They are stored by the server, fetched one at a time from `/api/marks/<id>/icon`, decoded, and
//! kept as textures so that a row is not downloaded and decoded again every frame.
//! [`IconCache::icon`] never blocks — it answers with what it has and queues a download when it has
//! nothing — and a small pool of worker threads does the downloading, which is why the list fills
//! in rather than waiting.
//!
//! What was fetched is also kept on disk, under the cache directory (`icon_cache`), so that
//! opening the window is not a round trip for every favicon on every run.
//!
//! A favicon that cannot be fetched or decoded is simply absent: the mark is still a mark, and
//! nothing here is worth failing a request over.

use std::collections::HashMap;
#[cfg(test)]
use std::fs;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

use eframe::egui;

use crate::api::Api;
use crate::app::Event;
use crate::icon_cache::IconFiles;
use crate::mark::Mark;

/// How many favicons are downloaded at once.
///
/// A launcher usually has a handful of marks and the server is local, so this is about
/// keeping a slow host from serialising behind four fast ones, not about saturating anything.
const WORKERS: usize = 4;

/// Favicons arrive between 16 and 64 pixels wide and are drawn at 20, so they are decoded to
/// roughly that size: big enough to stay sharp, small enough to stay cheap.
const ICON_PIXELS: u32 = 32;

/// What the UI can draw for one mark's favicon.
#[derive(Clone)]
pub enum IconState {
    /// Queued or in flight; the row is redrawn when the result arrives.
    Loading,
    Ready(egui::TextureHandle),
    /// Nothing to draw: the mark has no stored favicon, or fetching it failed.
    Missing,
}

/// The favicons the list has seen, and the workers that fetch the ones it has not.
///
/// Both halves are needed to keep the window responsive: [`IconCache::icon`] never blocks and
/// never waits on the network, and the pool behind it downloads several icons at once on
/// threads of its own, waking the UI through [`egui::Context::request_repaint`] when one lands.
pub struct IconCache {
    entries: HashMap<String, IconState>,
    jobs: Sender<Job>,
}

/// One favicon to fetch.
///
/// Both ids travel: the mark's, because that is what the server is asked for bytes by, and the
/// icon's, because that is what those bytes are kept as on disk — one row of the server's `icon`
/// table, shared by every mark on the same hostname.
struct Job {
    mark_id: String,
    icon_id: String,
}

impl IconCache {
    /// Starts the worker pool. The workers outlive every mark on screen and end with the app,
    /// when the job channel closes.
    pub fn new(
        api: Arc<Api>,
        files: Option<IconFiles>,
        events: Sender<Event>,
        ctx: egui::Context,
    ) -> Self {
        let (jobs, receiver) = mpsc::channel::<Job>();

        // Shared by the workers, each of which writes only its own icon's file.
        let files = Arc::new(files);

        // `Receiver` is not `Sync`, so the single queue lives behind a mutex that a worker
        // holds only while it takes the next job — never while it downloads.
        let receiver = Arc::new(Mutex::new(receiver));

        for index in 0..WORKERS {
            let receiver = Arc::clone(&receiver);
            let api = Arc::clone(&api);
            let files = Arc::clone(&files);
            let events = events.clone();
            let ctx = ctx.clone();

            let worker = thread::Builder::new()
                .name(format!("favicon-{index}"))
                .spawn(move || work(receiver, api, files, events, ctx));

            // Without these threads the list has no icons at all, and that is worth being
            // loud about at startup rather than showing an empty slot forever.
            if let Err(error) = worker {
                eprintln!("marks-client: could not start a favicon worker: {error}");
            }
        }

        Self {
            entries: HashMap::new(),
            jobs,
        }
    }

    /// The state to draw for `mark`, queueing its favicon the first time it is asked for.
    pub fn icon(&mut self, mark: &Mark) -> IconState {
        if let Some(state) = self.entries.get(&mark.id) {
            return state.clone();
        }

        let state = if mark.icon_id.is_some() {
            // A send failure means the pool is gone, which only happens on shutdown.
            let _ = self.jobs.send(Job {
                mark_id: mark.id.clone(),
                // Only ever asked for with an icon to ask about: the branch above is the one that
                // handles a mark without one.
                icon_id: mark.icon_id.clone().unwrap_or_default(),
            });
            IconState::Loading
        } else {
            // The server stored no favicon for this mark (a note, or a fetch that found
            // nothing). Remembering that keeps the row from asking again every frame.
            IconState::Missing
        };

        self.entries.insert(mark.id.clone(), state.clone());

        state
    }

    /// Applies a downloaded favicon, uploading it as a texture.
    ///
    /// Called on the UI thread when the worker's event is drained, because uploading is the
    /// one part of this that needs the egui context.
    pub fn store(&mut self, ctx: &egui::Context, mark_id: &str, image: Option<egui::ColorImage>) {
        let state = match image {
            Some(image) => IconState::Ready(ctx.load_texture(
                format!("favicon:{mark_id}"),
                image,
                egui::TextureOptions::LINEAR,
            )),
            None => IconState::Missing,
        };

        self.entries.insert(mark_id.to_owned(), state);
    }

    /// Drops a deleted mark's favicon, so a long session does not accumulate them.
    pub fn forget(&mut self, mark_id: &str) {
        self.entries.remove(mark_id);
    }
}

/// The decoded favicon for one mark: from the disk if it is there, and from the server if not.
///
/// What comes back from the server is kept before it is handed over, so that the next run — and
/// the next mark on the same hostname — costs a read rather than a request.
fn image_for(api: &Api, files: Option<&IconFiles>, job: &Job) -> Option<egui::ColorImage> {
    if let Some(stored) = files.and_then(|files| files.read(&job.icon_id)) {
        if let Some(image) = decode(&stored) {
            return Some(image);
        }

        // Kept, and not something that can be drawn: no use to anyone, and in the way of the copy
        // that can be fetched in its place.
        if let Some(files) = files {
            files.forget(&job.icon_id);
        }
    }

    let bytes = api.fetch_icon(&job.mark_id).ok()?;
    let image = decode(&bytes)?;

    // Kept only once it is known to be drawable: a request that answered with something useless
    // is not worth remembering.
    if let Some(files) = files {
        files.write(&job.icon_id, &bytes);
    }

    Some(image)
}

/// Takes jobs until the queue closes, fetching and decoding one favicon at a time.
fn work(
    receiver: Arc<Mutex<Receiver<Job>>>,
    api: Arc<Api>,
    files: Arc<Option<IconFiles>>,
    events: Sender<Event>,
    ctx: egui::Context,
) {
    loop {
        let job = {
            // Held only for the `recv` below: whoever waits here is waiting for work, not for
            // another worker's download.
            let queue = receiver.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            queue.recv()
        };

        let Ok(job) = job else {
            // The sender went away with the window.
            return;
        };

        // A favicon that cannot be fetched or decoded is simply absent; the mark stays usable.
        let image = image_for(&api, files.as_ref().as_ref(), &job);

        if events
            .send(Event::Icon {
                mark_id: job.mark_id,
                image,
            })
            .is_err()
        {
            return;
        }

        // The UI thread sleeps between frames; this is what makes it draw the new icon.
        ctx.request_repaint();
    }
}

/// Decodes the bytes the server serves into something egui can upload.
///
/// The server sniffs the format and passes the bytes through untouched (see
/// `sniffImageType`), so the format is recognised here from the bytes themselves: PNG, ICO,
/// JPEG, GIF or WebP — exactly the five the client was built with decoders for.
fn decode(bytes: &[u8]) -> Option<egui::ColorImage> {
    let image = image::load_from_memory(bytes).ok()?;

    // Shrunk once here rather than leaving the renderer to scale a larger texture on every
    // frame. `thumbnail` keeps the aspect ratio, which matters for the few wide favicons.
    let rgba = image.thumbnail(ICON_PIXELS, ICON_PIXELS).to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];

    Some(egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()))
}

/// The tests, in a file of their own: `icons/tests.rs`, compiled only for test builds.
#[cfg(test)]
mod tests;
