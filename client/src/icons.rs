use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

use eframe::egui;

use crate::api::Api;
use crate::app::Event;
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

/// One favicon to fetch. Only the id travels: the worker asks the server for the bytes, so the
/// URL does not have to be reconstructed here.
struct Job {
    mark_id: String,
}

impl IconCache {
    /// Starts the worker pool. The workers outlive every mark on screen and end with the app,
    /// when the job channel closes.
    pub fn new(api: Arc<Api>, events: Sender<Event>, ctx: egui::Context) -> Self {
        let (jobs, receiver) = mpsc::channel::<Job>();

        // `Receiver` is not `Sync`, so the single queue lives behind a mutex that a worker
        // holds only while it takes the next job — never while it downloads.
        let receiver = Arc::new(Mutex::new(receiver));

        for index in 0..WORKERS {
            let receiver = Arc::clone(&receiver);
            let api = Arc::clone(&api);
            let events = events.clone();
            let ctx = ctx.clone();

            let worker = thread::Builder::new()
                .name(format!("favicon-{index}"))
                .spawn(move || work(receiver, api, events, ctx));

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

    /// Whether `mark_id`'s favicon has arrived and been uploaded as a texture.
    ///
    /// Only the tests need to ask: the window learns this by drawing the row.
    ///
    /// A query rather than a lookup through [`IconCache::icon`], which would queue a download
    /// as a side effect: the tests use this to wait for one to finish.
    #[cfg(test)]
    pub fn is_ready(&self, mark_id: &str) -> bool {
        matches!(self.entries.get(mark_id), Some(IconState::Ready(_)))
    }

    /// Drops a deleted mark's favicon, so a long session does not accumulate them.
    pub fn forget(&mut self, mark_id: &str) {
        self.entries.remove(mark_id);
    }
}

/// Takes jobs until the queue closes, fetching and decoding one favicon at a time.
fn work(receiver: Arc<Mutex<Receiver<Job>>>, api: Arc<Api>, events: Sender<Event>, ctx: egui::Context) {
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
        let image = api
            .fetch_icon(&job.mark_id)
            .ok()
            .and_then(|bytes| decode(&bytes));

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
