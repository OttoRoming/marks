use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use eframe::egui;

use crate::api::{Api, ApiError};
use crate::icons::{IconCache, IconState};
use crate::mark::{Mark, web_link};
use crate::title::fetch_title;

/// Where the client looks for the server unless `MARKS_URL` says otherwise: the SvelteKit dev
/// server, which is where `npm run dev` puts it.
///
/// `localhost` rather than `127.0.0.1`, because vite binds the name: on a dual-stack machine
/// that is `[::1]`, and a literal IPv4 address would be refused.
const DEFAULT_BASE_URL: &str = "http://localhost:5173";

/// The server rejects a longer name (`markCreateSchema`), so a name derived from the search
/// box is cut here instead of being refused after a round trip.
const MAX_NAME_CHARS: usize = 200;

/// Height of the strip above the search field. The window has no title bar, so this is what
/// there is to drag it by.
const DRAG_HANDLE_HEIGHT: f32 = 10.0;

/// How much of the search text the footer hint repeats before cutting it short.
const HINT_CHARS: usize = 36;

/// Something a worker thread has to tell the UI.
///
/// Every request reports back as one of these; the UI thread never waits on the network, it
/// only reacts to what has arrived here.
pub enum Event {
    /// The credentials were accepted and `api` carries the session cookie from now on.
    SignedIn { api: Arc<Api>, username: String },
    SignInFailed(String),
    Marks(Vec<Mark>),
    MarksFailed(String),
    /// A request was refused with 401, so the session is gone and the modal comes back.
    SessionLost,
    Created(Mark),
    CreateFailed(String),
    Deleted { mark_id: String, name: String },
    DeleteFailed(String),
    Icon {
        mark_id: String,
        image: Option<egui::ColorImage>,
    },
}

/// Which button a sign-in attempt came from.
#[derive(Clone, Copy, PartialEq, Eq)]
enum AuthMode {
    Login,
    Signup,
}

/// The sign-in dialog's own state.
#[derive(Default)]
struct AuthForm {
    username: String,
    password: String,
    /// True while a sign-in request is in flight, which disables the form.
    busy: bool,
    error: Option<String>,
    /// Set when the username field should take the caret on the next frame.
    focus_username: bool,
}

/// The last thing worth telling the user, shown above the key hints.
struct Notice {
    text: String,
    error: bool,
}

impl Notice {
    fn info(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            error: false,
        }
    }

    fn error(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            error: true,
        }
    }
}

/// The launcher shortcuts, read from one frame's input in one go.
struct Keys {
    save: bool,
    open: bool,
    up: bool,
    down: bool,
    remove: bool,
    close: bool,
}

/// The window: a search field, the marks it matches, and a sign-in dialog until there is a
/// session.
pub struct MarksApp {
    base_url: String,
    /// The two ends of the channel every worker reports on, kept together because they are
    /// made in one place and cloned from there.
    events_tx: Sender<Event>,
    events_rx: Receiver<Event>,

    /// The client, once there is a session. While it is `None` the sign-in modal is up.
    api: Option<Arc<Api>>,
    username: Option<String>,
    /// The favicon pool, and the icons it has delivered so far.
    icons: Option<IconCache>,

    marks: Vec<Mark>,
    query: String,
    selected: usize,

    auth: AuthForm,
    notice: Option<Notice>,
}

impl MarksApp {
    /// Builds the app for `ctx`, which the favicon pool needs a handle to.
    ///
    /// Taking a context rather than an [`eframe::CreationContext`] is what lets the tests at
    /// the bottom of this file run real frames without eframe.
    pub fn new(ctx: &egui::Context) -> Self {
        let base_url = std::env::var("MARKS_URL")
            .unwrap_or_else(|_| DEFAULT_BASE_URL.to_owned())
            .trim_end_matches('/')
            .to_owned();

        // A token can be handed in, which starts the client signed in without the dialog. The
        // session still lives in memory only: `MARKS_TOKEN` is how a session that exists
        // elsewhere (a browser, a script) is reused here.
        let handed_in = std::env::var("MARKS_TOKEN")
            .ok()
            .map(|token| token.trim().to_owned())
            .filter(|token| !token.is_empty())
            .map(|token| Arc::new(Api::with_token(base_url.clone(), &token)));

        let (events_tx, events_rx) = mpsc::channel();

        let mut app = Self {
            base_url,
            events_tx,
            events_rx,
            api: handed_in,
            username: None,
            icons: None,
            marks: Vec::new(),
            query: String::new(),
            selected: 0,
            // The session lives in memory only, so the first thing the app shows is normally
            // the sign-in dialog, with the caret already in the username field.
            auth: AuthForm {
                focus_username: true,
                ..Default::default()
            },
            notice: None,
        };

        if let Some(api) = app.api.clone() {
            app.icons = Some(IconCache::new(
                Arc::clone(&api),
                app.events_tx.clone(),
                ctx.clone(),
            ));
            app.load_marks(ctx, api);
        }

        app
    }

    /// Takes everything the workers have reported since the last frame.
    fn drain_events(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.events_rx.try_recv() {
            match event {
                Event::SignedIn { api, username } => {
                    // The favicon pool needs the cookie, so it starts with the session.
                    self.icons = Some(IconCache::new(
                        Arc::clone(&api),
                        self.events_tx.clone(),
                        ctx.clone(),
                    ));
                    self.load_marks(ctx, Arc::clone(&api));
                    self.api = Some(api);
                    self.username = Some(username);
                    self.auth = AuthForm::default();
                    self.notice = None;
                }
                Event::SignInFailed(message) => {
                    self.auth.busy = false;
                    self.auth.error = Some(message);
                    // After a failed attempt the username is the likely thing to fix.
                    self.auth.focus_username = true;
                }
                Event::Marks(marks) => {
                    self.marks = marks;
                    self.selected = 0;
                    self.notice = None;
                }
                Event::MarksFailed(message) => self.notice = Some(Notice::error(message)),
                Event::SessionLost => {
                    self.signed_out("That session is no longer valid - sign in again.");
                }
                Event::Created(mark) => {
                    self.notice = Some(Notice::info(format!("Saved \"{}\".", mark.name)));
                    // The search box did its job; clearing it shows the new mark in place.
                    self.query.clear();

                    let new_id = mark.id.clone();
                    self.marks.push(mark);
                    // The server orders marks by name, so the list is re-ordered here rather
                    // than reloaded to find out where the new one belongs.
                    self.marks.sort_by(|left, right| {
                        left.name.to_lowercase().cmp(&right.name.to_lowercase())
                    });
                    self.selected = filter_marks(&self.marks, &self.query)
                        .iter()
                        .position(|candidate| candidate.id == new_id)
                        .unwrap_or(0);
                }
                Event::CreateFailed(message) => self.notice = Some(Notice::error(message)),
                Event::Deleted { mark_id, name } => {
                    self.marks.retain(|mark| mark.id != mark_id);
                    if let Some(icons) = &mut self.icons {
                        icons.forget(&mark_id);
                    }
                    self.notice = Some(Notice::info(format!("Deleted \"{name}\".")));
                    self.clamp_selection();
                }
                Event::DeleteFailed(message) => self.notice = Some(Notice::error(message)),
                Event::Icon { mark_id, image } => {
                    if let Some(icons) = &mut self.icons {
                        // Uploading the texture is the one part of the download that has to
                        // happen here, on the thread that owns the context.
                        icons.store(ctx, &mark_id, image);
                    }
                }
            }
        }
    }

    /// Runs `job` on a thread of its own and hands its event to a later frame.
    ///
    /// Nothing else in this app touches the network, so a slow or unreachable server shows up
    /// as a late event rather than as a frozen window.
    fn spawn<F>(&self, ctx: &egui::Context, job: F)
    where
        F: FnOnce() -> Event + Send + 'static,
    {
        let events = self.events_tx.clone();
        let ctx = ctx.clone();

        let spawned = thread::Builder::new()
            .name("marks-request".to_owned())
            .spawn(move || {
                let event = job();

                // A failed send means the window is gone and nobody is listening.
                if events.send(event).is_ok() {
                    ctx.request_repaint();
                }
            });

        if let Err(error) = spawned {
            eprintln!("marks-client: could not start a request thread: {error}");
        }
    }

    /// Asks for the mark list; the answer comes back as [`Event::Marks`].
    fn load_marks(&self, ctx: &egui::Context, api: Arc<Api>) {
        self.spawn(ctx, move || match api.list_marks() {
            Ok(marks) => Event::Marks(marks),
            Err(ApiError::Unauthorized) => Event::SessionLost,
            Err(error) => Event::MarksFailed(error.to_string()),
        });
    }

    /// Sends the credentials to the server on a worker thread, leaving the dialog up until the
    /// answer arrives.
    fn authenticate(&mut self, ctx: &egui::Context, mode: AuthMode) {
        let username = self.auth.username.trim().to_owned();
        let password = self.auth.password.clone();

        if username.is_empty() || password.is_empty() {
            self.auth.error = Some("A username and a password are both needed.".to_owned());
            return;
        }

        self.auth.busy = true;
        self.auth.error = None;
        self.notice = None;

        let base = self.base_url.clone();
        self.spawn(ctx, move || {
            let mut api = Api::new(base);
            let attempt = match mode {
                AuthMode::Login => api.login(&username, &password),
                AuthMode::Signup => api.signup(&username, &password),
            };

            match attempt {
                Ok(()) => Event::SignedIn {
                    api: Arc::new(api),
                    username,
                },
                Err(error) => Event::SignInFailed(error.to_string()),
            }
        });
    }

    /// Drops the session and puts the sign-in dialog back up.
    fn signed_out(&mut self, reason: &str) {
        self.api = None;
        self.icons = None;
        self.username = None;
        self.marks.clear();
        self.selected = 0;
        self.auth = AuthForm {
            error: Some(reason.to_owned()),
            focus_username: true,
            ..Default::default()
        };
    }

    /// Keeps the selection inside the list after it shrinks.
    fn clamp_selection(&mut self) {
        let count = filter_marks(&self.marks, &self.query).len();
        self.selected = self.selected.min(count.saturating_sub(1));
    }

    /// Moves the selection, stopping at either end of the list.
    fn move_selection(&mut self, delta: isize) {
        let count = filter_marks(&self.marks, &self.query).len();
        if count == 0 {
            return;
        }

        self.selected = self.selected.saturating_add_signed(delta).min(count - 1);
    }

    /// Saves what is in the search box as a new mark (Ctrl+Enter).
    fn save_query(&mut self, ctx: &egui::Context) {
        let Some(api) = self.api.clone() else {
            return;
        };

        let typed = self.query.trim();
        if typed.is_empty() {
            self.notice = Some(Notice::error("Type a link or a note to save first."));
            return;
        }

        // A bare host is stored with its scheme, so the mark both opens and gets a favicon:
        // the server derives the icon from the URL, not from the text it was typed as.
        let content = stored_content(typed);
        // What the mark is called if the page does not name itself (see `named_after_title`).
        let fallback = mark_name(&content);

        self.notice = Some(Notice::info(format!("Saving \"{fallback}\"...")));
        // The page is fetched here rather than before the spawn, so that a slow site delays
        // the save rather than the window: the worker is already off the UI thread.
        self.spawn(ctx, move || {
            let name = named_after_title(&content, fallback);

            match api.create_mark(&name, &content) {
                Ok(mark) => Event::Created(mark),
                Err(ApiError::Unauthorized) => Event::SessionLost,
                Err(error) => Event::CreateFailed(error.to_string()),
            }
        });
    }

    /// Opens the selected mark in the browser (Enter).
    fn open_selected(&mut self, ctx: &egui::Context) {
        let Some(mark) = selected_mark(&self.marks, &self.query, self.selected) else {
            return;
        };

        match mark.link() {
            Some(url) => ctx.open_url(egui::OpenUrl::new_tab(url)),
            // A mark may be a plain note; say so rather than doing nothing at all.
            None => {
                self.notice = Some(Notice::error(format!("\"{}\" is not a link.", mark.name)));
            }
        }
    }

    /// Deletes the selected mark (Ctrl+D).
    fn delete_selected(&mut self, ctx: &egui::Context) {
        let Some(api) = self.api.clone() else {
            return;
        };
        let Some(mark) = selected_mark(&self.marks, &self.query, self.selected) else {
            return;
        };

        let mark_id = mark.id.clone();
        let name = mark.name.clone();

        self.spawn(ctx, move || match api.delete_mark(&mark_id) {
            Ok(()) => Event::Deleted { mark_id, name },
            Err(ApiError::Unauthorized) => Event::SessionLost,
            Err(error) => Event::DeleteFailed(error.to_string()),
        });
    }

    /// Handles the launcher keys, consuming them so that no widget sees them first.
    fn launcher_keys(&mut self, ctx: &egui::Context) {
        let keys = ctx.input_mut(|input| Keys {
            // The most specific shortcut is read first: `consume_key` ignores extra shift and
            // alt, but not ctrl, so Ctrl+Enter must not be taken for plain Enter.
            save: input.consume_key(egui::Modifiers::CTRL, egui::Key::Enter),
            open: input.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
            up: input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
            down: input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
            // Ctrl+D rather than Delete: the search field has the keyboard, and there Delete
            // and Backspace belong to the text being edited.
            remove: input.consume_key(egui::Modifiers::CTRL, egui::Key::D),
            close: input.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
        });

        if keys.close {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if keys.up {
            self.move_selection(-1);
        }
        if keys.down {
            self.move_selection(1);
        }
        if keys.remove {
            self.delete_selected(ctx);
        }
        if keys.save {
            self.save_query(ctx);
        }
        if keys.open {
            self.open_selected(ctx);
        }
    }

    /// Keys that matter while the sign-in dialog is up: Enter submits, Escape quits.
    fn modal_keys(&mut self, ctx: &egui::Context) -> Option<AuthMode> {
        // Read here, before the form is drawn, so that Enter reaches this and not the field:
        // in a single-line text edit it would otherwise go nowhere.
        let (enter, escape) = ctx.input_mut(|input| {
            (
                input.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
                input.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
            )
        });

        if escape {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        (enter && !self.auth.busy).then_some(AuthMode::Login)
    }

    /// The sign-in dialog: two fields and one button per action.
    fn auth_modal(&mut self, ctx: &egui::Context) -> Option<AuthMode> {
        let mut request = None;

        egui::Modal::new(egui::Id::new("sign-in")).show(ctx, |ui| {
            ui.set_width(300.0);
            ui.heading("Marks");
            ui.label(
                egui::RichText::new(format!("Sign in to {}", self.base_url))
                    .small()
                    .weak(),
            );
            ui.add_space(8.0);

            ui.label("Username");
            let username = ui.add_enabled(
                !self.auth.busy,
                egui::TextEdit::singleline(&mut self.auth.username).desired_width(f32::INFINITY),
            );

            ui.add_space(4.0);
            ui.label("Password");
            ui.add_enabled(
                !self.auth.busy,
                egui::TextEdit::singleline(&mut self.auth.password)
                    .password(true)
                    .desired_width(f32::INFINITY),
            );

            if let Some(error) = &self.auth.error {
                ui.add_space(6.0);
                ui.colored_label(ui.visuals().error_fg_color, error);
            }

            ui.add_space(10.0);
            ui.add_enabled_ui(!self.auth.busy, |ui| {
                ui.horizontal(|ui| {
                    if ui.button("Log in").clicked() {
                        request = Some(AuthMode::Login);
                    }
                    if ui.button("Sign up").clicked() {
                        request = Some(AuthMode::Signup);
                    }
                });
            });

            // The dialog is the only thing to type into while it is up, so the caret starts
            // where the user has to start.
            if self.auth.focus_username {
                username.request_focus();
                self.auth.focus_username = false;
            }
        });

        request
    }

    /// The strip above the search field, which is what moves the window.
    ///
    /// The window has no title bar — it is meant to look like a launcher — so without this
    /// there would be nothing to drag it by.
    fn drag_handle(&mut self, ui: &mut egui::Ui) {
        let (_, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), DRAG_HANDLE_HEIGHT),
            egui::Sense::drag(),
        );

        if response.drag_started() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
    }

    /// The one field the whole window is built around.
    fn search_field(&mut self, ui: &mut egui::Ui) {
        let response = ui.add(
            egui::TextEdit::singleline(&mut self.query)
                .hint_text("Search marks, or type a link to save")
                .desired_width(f32::INFINITY),
        );

        // A launcher has nothing else to click, so the caret lives here — except while the
        // sign-in dialog is up, where its own fields want it.
        if self.api.is_some() && !response.has_focus() {
            response.request_focus();
        }
    }

    /// The matching marks, as rows of favicon, name and link.
    fn list(&mut self, ui: &mut egui::Ui) {
        // The row a click landed on, acted on once the list has been released.
        let mut activated = None;

        {
            let filtered = filter_marks(&self.marks, &self.query);
            self.selected = self.selected.min(filtered.len().saturating_sub(1));

            if filtered.is_empty() {
                ui.add_space(6.0);
                ui.weak(self.empty_message());
            } else {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for (index, mark) in filtered.iter().enumerate() {
                            // Asking for the icon is what queues its download, so the rows that
                            // are drawn are the favicons that get fetched.
                            let icon = match self.icons.as_mut() {
                                Some(icons) => icons.icon(mark),
                                None => IconState::Missing,
                            };

                            if mark_row(ui, &icon, mark, index == self.selected).clicked() {
                                activated = Some(index);
                            }
                        }
                    });
            }
        }

        if let Some(index) = activated {
            self.selected = index;
            self.open_selected(ui.ctx());
        }
    }

    /// What to say when the list has nothing to draw.
    fn empty_message(&self) -> String {
        if self.api.is_none() {
            "Sign in to load your marks.".to_owned()
        } else if self.query.trim().is_empty() {
            "No marks yet - type a link and press Ctrl+Enter to save the first one.".to_owned()
        } else {
            "Nothing matches. Ctrl+Enter saves what you typed as a new mark.".to_owned()
        }
    }

    /// The footer's write hint, shown only when there is something to save.
    ///
    /// Ctrl+Enter is the one action whose meaning depends on what is typed, so it is spelled
    /// out with the text it would save; the other keys are the same whatever is in the field.
    fn save_hint(&self) -> Option<String> {
        let typed = self.query.trim();

        (!typed.is_empty()).then(|| {
            format!(
                "Ctrl+Enter  save \"{}\" as a new mark",
                shorten(typed, HINT_CHARS)
            )
        })
    }

    /// The bottom line: what just happened, and the keys that work.
    fn footer(&mut self, ui: &mut egui::Ui) {
        if let Some(notice) = &self.notice {
            if notice.error {
                ui.colored_label(ui.visuals().error_fg_color, &notice.text);
            } else {
                ui.label(&notice.text);
            }
        }

        ui.horizontal_wrapped(|ui| {
            if self.api.is_none() {
                ui.weak("Esc  close");
                return;
            }

            if let Some(hint) = self.save_hint() {
                ui.strong(hint);
                ui.separator();
            }

            if let Some(username) = &self.username {
                ui.weak(format!("signed in as {username}"));
                ui.separator();
            }

            ui.weak("↑↓ move   Enter open   Ctrl+D delete   Esc close");
        });
    }

    /// Draws one frame of the window.
    ///
    /// Kept apart from the [`eframe::App`] implementation so that a frame can be run without
    /// eframe — by the tests below, which drive the real keybindings and the real requests.
    pub fn show(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();

        self.drain_events(&ctx);

        // While the sign-in dialog is up there is no list to drive, so the launcher keys are
        // left alone: Enter belongs to the form.
        let mut sign_in = None;
        if self.api.is_none() {
            sign_in = self.modal_keys(&ctx);
        } else {
            self.launcher_keys(&ctx);
        }

        // The key hints are a panel of their own rather than the last item of the central
        // one: a panel reserves its height, so the scrolling list cannot grow over it and
        // push it out of the window.
        egui::Panel::bottom(egui::Id::new("key-hints")).show(ui, |ui| {
            self.footer(ui);
        });

        egui::CentralPanel::default().show(ui, |ui| {
            self.drag_handle(ui);
            self.search_field(ui);
            ui.separator();
            self.list(ui);
        });

        if self.api.is_none() {
            sign_in = sign_in.or(self.auth_modal(&ctx));
            if let Some(mode) = sign_in {
                self.authenticate(&ctx, mode);
            }
        }
    }
}

impl eframe::App for MarksApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
    }
}

/// Draws one row: its favicon, its name, and the link underneath.
fn mark_row(ui: &mut egui::Ui, icon: &IconState, mark: &Mark, selected: bool) -> egui::Response {
    let background = if selected {
        ui.visuals().selection.bg_fill
    } else {
        egui::Color32::TRANSPARENT
    };

    egui::Frame::new()
        .fill(background)
        .corner_radius(egui::CornerRadius::same(4))
        .inner_margin(egui::Margin::symmetric(8, 5))
        .show(ui, |ui| {
            // Without this the highlight would only be as wide as the text in the row.
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                icon_widget(ui, icon);
                ui.vertical(|ui| {
                    ui.strong(&mark.name);
                    ui.weak(egui::RichText::new(&mark.content).small().monospace());
                });
            });
        })
        .response
        // The row is clicked as a whole, so the pointer does not have to find the text.
        .interact(egui::Sense::click())
}

/// Draws the favicon slot: the icon, a spinner while it is on its way, or nothing.
fn icon_widget(ui: &mut egui::Ui, icon: &IconState) {
    const ICON_SIZE: f32 = 20.0;

    let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(ICON_SIZE), egui::Sense::hover());

    match icon {
        IconState::Ready(texture) => {
            ui.painter().image(
                texture.id(),
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
        // Worth showing, because a row that looked finished while its icon was still coming
        // would be a small lie; egui animates the spinner and asks for the repaints itself.
        IconState::Loading => {
            ui.put(rect, egui::Spinner::new().size(ICON_SIZE));
        }
        // No stored favicon: the slot stays empty so that the names still line up.
        IconState::Missing => {}
    }
}

/// The marks the query keeps, in the order the list draws them.
///
/// A free function rather than a method so that the borrow it takes is limited to `marks`:
/// drawing the rows needs `self.icons` mutably while this list is still alive.
fn filter_marks<'a>(marks: &'a [Mark], query: &str) -> Vec<&'a Mark> {
    let needle = query.trim().to_lowercase();

    marks
        .iter()
        .filter(|mark| {
            needle.is_empty()
                || mark.name.to_lowercase().contains(&needle)
                || mark.content.to_lowercase().contains(&needle)
        })
        .collect()
}

/// The mark the selection points at, if the filter still leaves one there.
fn selected_mark<'a>(marks: &'a [Mark], query: &str, selected: usize) -> Option<&'a Mark> {
    filter_marks(marks, query).get(selected).copied()
}

/// What to store for a mark created from the search box: the typed text, with `https://` added
/// when it is a bare host such as `github.com`.
fn stored_content(typed: &str) -> String {
    match web_link(typed) {
        Some(url) => url.to_string(),
        None => typed.to_owned(),
    }
}

/// Names a mark created from the search box.
///
/// A link is named after its host, which is also what its favicon stands for; anything else
/// keeps the text it was typed as. The server caps a name at 200 characters, so it is cut here
/// rather than refused after a round trip.
///
/// This is the name a link is saved under when its page has no title to give; see
/// [`named_after_title`] for the one it usually gets instead.
fn mark_name(content: &str) -> String {
    let name = web_link(content)
        .and_then(|url| url.host_str().map(str::to_owned))
        .unwrap_or_else(|| content.to_owned());

    name.chars().take(MAX_NAME_CHARS).collect()
}

/// The name to save a new mark under: the page's own title when it has one, and the name it
/// would have been given from its host otherwise.
///
/// A note is never fetched, and neither is a link that cannot be reached, that answers with
/// something other than HTML, or that names no title: `fallback` covers all of those at once,
/// because naming a mark better is never worth losing one over.
///
/// Only creation goes through this. Renaming a mark leaves its name alone, and so does
/// editing the link of one already saved, since the title of a page the user chose a name for
/// is not this client's to overwrite.
fn named_after_title(content: &str, fallback: String) -> String {
    let Some(url) = web_link(content) else {
        return fallback;
    };

    match fetch_title(&url) {
        // Cut to what the server accepts rather than refused after a round trip, as in
        // `mark_name`.
        Some(title) => title.chars().take(MAX_NAME_CHARS).collect(),
        None => fallback,
    }
}

/// Cuts `text` to `max` characters for a hint, showing that it was cut.
fn shorten(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }

    let mut shortened: String = text.chars().take(max).collect();
    shortened.push('…');

    shortened
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// The account the live tests use: created on the first run, reused after that.
    const TEST_USER: &str = "client-test";
    const TEST_PASSWORD: &str = "client-test-password";

    /// How long a live request may take before a test calls it a failure.
    const TIMEOUT: Duration = Duration::from_secs(20);

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

    /// Runs one frame of the whole app with `events` as this frame's input.
    fn frame(app: &mut MarksApp, ctx: &egui::Context, events: Vec<egui::Event>) {
        let input = egui::RawInput {
            events,
            ..Default::default()
        };
        // The frame's output (textures, shapes) is not what these tests look at.
        let _ = ctx.run_ui(input, |ui| app.show(ui));
    }

    /// Runs frames until `done` holds, and reports whether it ever did.
    fn frame_until(
        app: &mut MarksApp,
        ctx: &egui::Context,
        done: impl Fn(&MarksApp) -> bool,
    ) -> bool {
        let deadline = Instant::now() + TIMEOUT;

        loop {
            frame(app, ctx, Vec::new());
            if done(app) {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn press(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    /// A client signed in as the test account, for putting fixtures on the server. The test
    /// account is created the first time this ever runs.
    fn test_api() -> Api {
        let mut api = Api::new(DEFAULT_BASE_URL);
        if api.login(TEST_USER, TEST_PASSWORD).is_err() {
            api = Api::new(DEFAULT_BASE_URL);
            api.signup(TEST_USER, TEST_PASSWORD)
                .expect("could not create the test account");
        }

        api
    }

    /// Signs in through the app's own dialog, creating the test account the first time.
    fn sign_in(app: &mut MarksApp, ctx: &egui::Context) {
        app.auth.username = TEST_USER.to_owned();
        app.auth.password = TEST_PASSWORD.to_owned();

        app.authenticate(ctx, AuthMode::Login);
        if !frame_until(app, ctx, |app| app.api.is_some()) {
            app.authenticate(ctx, AuthMode::Signup);
            assert!(
                frame_until(app, ctx, |app| app.api.is_some()),
                "could not sign in or sign up: {:?}",
                app.auth.error
            );
        }

        // A frame or two so the search field can take the caret before anything is typed.
        frame(app, ctx, Vec::new());
        frame(app, ctx, Vec::new());
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
    fn the_ctrl_enter_hint_is_only_there_when_there_is_something_to_save() {
        let ctx = egui::Context::default();
        let mut app = MarksApp::new(&ctx);

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

    #[test]
    #[ignore = "needs internet access"]
    fn a_link_is_named_after_the_title_of_the_page_it_points_at() {
        // example.com is reserved for documentation and has answered with the same title for
        // decades, which makes it the one title worth writing down in a test.
        let content = "https://example.com/";

        assert_eq!(
            named_after_title(content, mark_name(content)),
            "Example Domain"
        );
    }

    #[test]
    #[ignore = "needs a running dev server"]
    fn ctrl_enter_names_a_new_link_after_the_title_of_its_page() {
        // The page is served from this machine, so the title that comes back is one this test
        // wrote rather than whatever the internet has to say today. The marks server is the
        // real one, so this covers the whole path: typed text, fetched title, stored name.
        let page = crate::title::serve(
            "text/html",
            "<html><head><title>Local Test Page</title></head><body>hello</body></html>",
        );

        let ctx = egui::Context::default();
        let mut app = MarksApp::new(&ctx);
        sign_in(&mut app, &ctx);

        assert!(
            frame_until(&mut app, &ctx, |app| app.api.is_some()),
            "not signed in"
        );

        frame(
            &mut app,
            &ctx,
            vec![egui::Event::Text(page.as_str().to_owned())],
        );
        frame(&mut app, &ctx, Vec::new());
        assert_eq!(app.query, page.as_str(), "typing did not reach the field");

        frame(
            &mut app,
            &ctx,
            vec![press(egui::Key::Enter, egui::Modifiers::CTRL)],
        );

        assert!(
            frame_until(&mut app, &ctx, |app| app
                .marks
                .iter()
                .any(|mark| mark.name == "Local Test Page")),
            "the mark was not named after its page: {:?}",
            app.notice.as_ref().map(|notice| notice.text.clone())
        );

        // The name is the client's; what the server stored is what matters, so ask it.
        let saved = app
            .marks
            .iter()
            .find(|mark| mark.name == "Local Test Page")
            .cloned()
            .expect("the new mark");
        let api = app.api.clone().expect("signed in");
        let stored = api.list_marks().expect("the server list");
        let on_server = stored
            .iter()
            .find(|mark| mark.id == saved.id)
            .expect("the server never stored the mark");
        assert_eq!(on_server.name, "Local Test Page");
        assert_eq!(on_server.content, page.as_str());

        api.delete_mark(&saved.id).expect("clean up");
    }

    #[test]
    #[ignore = "needs a running dev server"]
    fn typing_filters_the_list() {
        let api = test_api();
        let stamp = std::process::id();
        let alpha = api
            .create_mark(&format!("alpha-{stamp}"), "alpha note")
            .expect("a first mark");
        let beta = api
            .create_mark(&format!("beta-{stamp}"), "beta note")
            .expect("a second mark");

        // The list is loaded when the session starts, so the fixtures go up first.
        let ctx = egui::Context::default();
        let mut app = MarksApp::new(&ctx);
        sign_in(&mut app, &ctx);

        assert!(
            frame_until(&mut app, &ctx, |app| {
                app.marks.iter().any(|mark| mark.id == alpha.id)
                    && app.marks.iter().any(|mark| mark.id == beta.id)
            }),
            "the mark list never arrived"
        );

        // Typing goes through the real text field.
        frame(&mut app, &ctx, vec![egui::Event::Text(format!("alpha-{stamp}"))]);
        frame(&mut app, &ctx, Vec::new());

        assert_eq!(app.query, format!("alpha-{stamp}"));
        let shown = filter_marks(&app.marks, &app.query);
        assert_eq!(names(&shown), [format!("alpha-{stamp}")]);

        api.delete_mark(&alpha.id).expect("clean up");
        api.delete_mark(&beta.id).expect("clean up");
    }

    #[test]
    #[ignore = "needs a running dev server"]
    fn ctrl_enter_saves_what_is_typed_and_ctrl_d_removes_it() {
        let ctx = egui::Context::default();
        let mut app = MarksApp::new(&ctx);
        sign_in(&mut app, &ctx);

        assert!(
            frame_until(&mut app, &ctx, |app| app.api.is_some()),
            "not signed in"
        );

        frame(&mut app, &ctx, vec![egui::Event::Text("example.com".to_owned())]);
        frame(&mut app, &ctx, Vec::new());
        assert_eq!(app.query, "example.com", "typing did not reach the field");

        frame(&mut app, &ctx, vec![press(egui::Key::Enter, egui::Modifiers::CTRL)]);
        assert!(
            frame_until(&mut app, &ctx, |app| {
                app.query.is_empty()
                    && app
                        .marks
                        .iter()
                        .any(|mark| mark.content.starts_with("https://example.com"))
            }),
            "Ctrl+Enter did not save the typed link: {:?}",
            app.notice.as_ref().map(|notice| notice.text.clone())
        );

        let saved = app
            .marks
            .iter()
            .find(|mark| mark.content.starts_with("https://example.com"))
            .cloned()
            .expect("the new mark");

        // The page names itself when its title could be fetched, which is the usual case for a
        // live test; a machine that cannot reach it keeps the host it was named after. Either
        // way the save went through, which is what this test is about.
        assert!(
            matches!(saved.name.as_str(), "Example Domain" | "example.com"),
            "unexpected name {:?}",
            saved.name
        );

        let api = app.api.clone().expect("signed in");
        assert!(
            api.list_marks()
                .expect("the server list")
                .iter()
                .any(|mark| mark.id == saved.id),
            "the server never stored the mark"
        );

        // The mark that was just saved is the selected one, so Ctrl+D removes it again.
        assert_eq!(
            selected_mark(&app.marks, &app.query, app.selected).map(|mark| mark.id.clone()),
            Some(saved.id.clone()),
            "the new mark should be the selected row"
        );

        frame(&mut app, &ctx, vec![press(egui::Key::D, egui::Modifiers::CTRL)]);
        assert!(
            frame_until(&mut app, &ctx, |app| {
                !app.marks.iter().any(|mark| mark.id == saved.id)
            }),
            "Ctrl+D did not delete the mark"
        );
        assert!(
            !api.list_marks()
                .expect("the server list")
                .iter()
                .any(|mark| mark.id == saved.id),
            "the server still has the mark"
        );
    }

    #[test]
    #[ignore = "needs a running dev server"]
    fn a_favicon_is_fetched_decoded_and_uploaded() {
        let api = test_api();
        let stamp = std::process::id();
        let linked = api
            .create_mark(&format!("github-{stamp}"), "https://github.com/emilk/egui")
            .expect("a mark");
        assert!(
            linked.icon_id.is_some(),
            "the server stored no favicon for github.com"
        );

        let ctx = egui::Context::default();
        let mut app = MarksApp::new(&ctx);
        sign_in(&mut app, &ctx);

        // Drawing the row is what queues the download, so the frames do the work.
        assert!(
            frame_until(&mut app, &ctx, |app| app
                .marks
                .iter()
                .any(|mark| mark.id == linked.id)),
            "the mark never arrived"
        );
        assert!(
            frame_until(&mut app, &ctx, |app| app
                .icons
                .as_ref()
                .is_some_and(|icons| icons.is_ready(&linked.id))),
            "the favicon never became a texture"
        );

        api.delete_mark(&linked.id).expect("clean up");
    }

    #[test]
    #[ignore = "needs a running dev server"]
    fn the_arrows_move_the_selection_and_enter_leaves_a_note_alone() {
        let api = test_api();
        let stamp = std::process::id();
        let note = api
            .create_mark(&format!("note-{stamp}"), "just a note")
            .expect("a note");

        let ctx = egui::Context::default();
        let mut app = MarksApp::new(&ctx);
        sign_in(&mut app, &ctx);

        assert!(
            frame_until(&mut app, &ctx, |app| app.marks
                .iter()
                .any(|mark| mark.id == note.id)),
            "the note never arrived"
        );

        app.query = format!("note-{stamp}");
        frame(&mut app, &ctx, Vec::new());

        let before = app.selected;
        frame(&mut app, &ctx, vec![press(egui::Key::ArrowDown, egui::Modifiers::NONE)]);
        assert_eq!(app.selected, before.min(filter_marks(&app.marks, &app.query).len() - 1));

        // Enter on a note must complain rather than try to open anything.
        frame(&mut app, &ctx, vec![press(egui::Key::Enter, egui::Modifiers::NONE)]);
        assert!(
            app.notice.as_ref().is_some_and(|notice| notice.error),
            "Enter on a note should report that it is not a link"
        );

        api.delete_mark(&note.id).expect("clean up");
    }
}
