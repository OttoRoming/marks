//! The window: what the launcher is made of, and how it is driven.
//!
//! [`MarksApp`] holds everything on screen — the marks, the search box, the settings, the session
//! — and draws it one frame at a time ([`MarksApp::show`]). Nothing here talks to the network: a
//! request is handed to a worker thread (`MarksApp::spawn`) which reports what came back as an
//! `Event`, and the frames after it act on that. A slow or unreachable server therefore costs a
//! late answer rather than a frozen window.
//!
//! The keyboard is the interface, a launcher having one field and no buttons to hunt for:
//! Ctrl+Enter saves what is typed, Enter opens the selected mark, Ctrl+E changes it, Ctrl+D asks to
//! delete it, Ctrl+, opens the settings panel, and Escape closes whatever is in front of the list.

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use eframe::egui;

use crate::api::{Api, ApiError};
use crate::config::{self, Config, MAX_SIDE, MIN_HEIGHT, MIN_WIDTH};
use crate::fonts::{self, SystemFonts};
use crate::icon_cache::IconFiles;
use crate::icons::{IconCache, IconState};
use crate::mark::{Mark, web_link};
use crate::search;
use crate::session_file;
use crate::title::fetch_title;

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
    ///
    /// The address comes back with them because it is not necessarily the one the window started
    /// with: the sign-in dialog asks which server, and the answer can be one this run has never
    /// seen — which then decides where the favicons are cached, and what gets written down.
    SignedIn {
        api: Arc<Api>,
        username: String,
        base_url: String,
    },
    SignInFailed(String),
    Marks(Vec<Mark>),
    MarksFailed(String),
    /// A request was refused with 401, so the session is gone and the modal comes back.
    SessionLost,
    Created(Mark),
    CreateFailed(String),
    /// A mark was changed, and this is it as the server now stores it.
    Updated(Mark),
    UpdateFailed(String),
    Deleted { mark_id: String, name: String },
    DeleteFailed(String),
    Icon {
        mark_id: String,
        image: Option<egui::ColorImage>,
    },
    /// The fonts this machine has, read on a worker thread because reading them is slow.
    SystemFonts(Arc<SystemFonts>),
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
    /// The server to sign in to, as typed.
    ///
    /// The dialog is the only place this client is told which server to talk to: it has none of its
    /// own, so this is where every session begins.
    base_url: String,
    username: String,
    password: String,
    /// True while a sign-in request is in flight, which disables the form.
    busy: bool,
    error: Option<String>,
    /// Set when the form should take the caret on the next frame: at the first field that still
    /// needs filling in, which is the server's address when there is not one yet.
    focus_first_field: bool,
}

/// The mark being edited, and what is typed into the dialog about it.
///
/// A copy of the mark rather than a reference to it: the list is being read while this is typed at,
/// and a mark that changed under the fields would be a dialog arguing with itself.
struct EditForm {
    /// What is being changed, and what the change is sent to.
    mark_id: String,
    name: String,
    content: String,
    /// True while the change is in flight, which disables the form.
    busy: bool,
    error: Option<String>,
    /// Set when the name field should take the caret on the next frame, which is when the dialog
    /// has just been opened.
    focus_name: bool,
}

/// What the edit dialog was asked to do, once the frame it was drawn in is over.
#[derive(Clone, Copy, PartialEq, Eq)]
enum EditRequest {
    Save,
    Cancel,
}

/// Something the window will do, once the user has said to.
///
/// One variant so far — deleting a mark — and the shape is what makes the confirmation reusable:
/// the dialog asks the question and the window carries out whatever was asked about, so a second
/// kind of action that cannot be taken back needs a variant here and nowhere else.
enum Pending {
    Delete { mark_id: String, name: String },
}

/// A question the window is asking before doing something it cannot take back.
struct Confirmation {
    question: String,
    /// What goes with the question: what the action costs, for a question about something that
    /// cannot be taken back.
    note: String,
    /// What the button that goes through with it says, so that the button is the answer rather than
    /// a bare "yes".
    confirm: String,
    action: Pending,
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
    /// Ctrl+E: change the mark that is selected.
    edit: bool,
    open: bool,
    up: bool,
    down: bool,
    remove: bool,
    config: bool,
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

    /// The settings the window is running by, and where they are kept.
    config: Config,
    /// The fonts this machine has, once they have been read.
    system_fonts: Option<Arc<SystemFonts>>,
    /// Whether that reading is under way, so that it is asked for once.
    loading_fonts: bool,
    /// What is typed in the panel's font search.
    font_query: String,
    /// Whether the configuration panel is up.
    config_open: bool,
    /// Whether the window's settings have been changed since the client was opened.
    ///
    /// The panel says nothing about a window manager ignoring a size until one has been asked
    /// for: a line explaining a lack of change is worth reading after the change, and is only
    /// noise before it. It stays said once it has been, for as long as the window is open.
    window_settings_changed: bool,

    auth: AuthForm,
    /// The mark being changed, while a dialog is open for it.
    edit: Option<EditForm>,
    /// The question the window is asking before doing something it cannot take back.
    confirmation: Option<Confirmation>,
    notice: Option<Notice>,
}

impl MarksApp {
    /// Builds the app for `ctx`, with the settings to run by, the server to talk to, and the
    /// session to start from handed in.
    ///
    /// Taking those rather than reading them is what lets the environment and the files on disk
    /// stay out of here: `main` reads them ([`starting_session`] and `Config::load`), and a test
    /// can build a window without a session file, a `MARKS_URL`, or anything else off the
    /// machine.
    ///
    /// Taking a context rather than an [`eframe::CreationContext`] is what lets the tests run
    /// real frames without eframe.
    pub fn new(
        ctx: &egui::Context,
        base_url: String,
        token: Option<String>,
        config: Config,
    ) -> Self {
        // Before anything is drawn: the fonts are what everything after this is drawn with. The
        // machine's own fonts are not read yet, so a setting naming one of those is honoured a
        // moment later, when they arrive (see `ensure_system_fonts`).
        fonts::install(ctx, &config.fonts.priority, None);

        // A session to start from means the window opens signed in, so closing it and opening
        // it again does not ask for the password a second time. It takes both halves: a token, and a
        // server to present it to — which is why a token with no address beside it is no session
        // here, however it arrived.
        let start = match token {
            Some(token) if !base_url.is_empty() => {
                Some(Arc::new(Api::with_token(base_url.clone(), &token)))
            }
            _ => None,
        };

        // What the sign-in dialog starts filled in with: the address this run was given, from
        // `MARKS_URL` or from the session the last run kept. Empty when there was nothing to go on,
        // so that the dialog asks which server rather than guessing at one.
        let asked_about = base_url.clone();

        let (events_tx, events_rx) = mpsc::channel();

        let mut app = Self {
            base_url,
            events_tx,
            events_rx,
            api: start,
            username: None,
            icons: None,
            marks: Vec::new(),
            query: String::new(),
            selected: 0,
            config,
            system_fonts: None,
            loading_fonts: false,
            font_query: String::new(),
            config_open: false,
            window_settings_changed: false,
            // Without a session to start from, the first thing the app shows is the sign-in
            // dialog, with the caret already in the username field.
            auth: AuthForm {
                base_url: asked_about,
                focus_first_field: true,
                ..Default::default()
            },
            notice: None,
            edit: None,
            confirmation: None,
        };

        // A setting that names a font this machine has cannot be honoured until the machine's
        // fonts have been read, which is slow enough to belong off this thread. Until then the
        // window draws in the fonts egui carries.
        if app.config.fonts.priority.iter().any(|name| !Self::bundled(name)) {
            app.ensure_system_fonts(ctx);
        }

        if let Some(api) = app.api.clone() {
            app.icons = Some(IconCache::new(
                Arc::clone(&api),
                IconFiles::for_server(&app.base_url),
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
                Event::SignedIn {
                    api,
                    username,
                    base_url,
                } => {
                    // The address first: the favicon pool is kept per server, so it has to be told
                    // which server this session turned out to be for before it is started.
                    self.base_url = base_url;
                    // The favicon pool needs the cookie, so it starts with the session.
                    self.icons = Some(IconCache::new(
                        Arc::clone(&api),
                        IconFiles::for_server(&self.base_url),
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
                    // After a failed attempt, the first field still empty is the likely thing to
                    // fix — the address if it was never given, and the username otherwise.
                    self.auth.focus_first_field = true;
                }
                Event::Marks(marks) => {
                    self.marks = marks;
                    self.selected = 0;
                    self.notice = None;
                }
                Event::MarksFailed(message) => self.notice = Some(Notice::error(message)),
                Event::SessionLost => {
                    self.signed_out(Some("That session is no longer valid - sign in again."));
                }
                Event::Created(mark) => {
                    self.notice = Some(Notice::info(format!("Saved \"{}\".", mark.name)));
                    // The search box did its job; clearing it shows the new mark in place.
                    self.query.clear();
                    self.settle_mark(mark);
                }
                Event::Updated(mark) => {
                    self.notice = Some(Notice::info(format!("Saved \"{}\".", mark.name)));
                    // The dialog has done its job and the change has landed: it comes down, and the
                    // row it was opened from carries what was typed into it.
                    self.edit = None;
                    self.settle_mark(mark);
                }
                Event::UpdateFailed(message) => {
                    // The dialog stays up with what was typed still in it: a change that was refused
                    // is worth another try, and there is no reason to make the user type it again.
                    if let Some(edit) = &mut self.edit {
                        edit.busy = false;
                        edit.error = Some(message);
                    }
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
                Event::SystemFonts(fonts) => {
                    self.loading_fonts = false;
                    self.system_fonts = Some(fonts);
                    // The settings may name a font that could not be loaded until now.
                    self.install_fonts(ctx);
                }
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

        // The address is settled here, before anything is sent anywhere: an address that is not one
        // is worth saying so about, and there is nothing to send it to. What the field is left
        // holding is the address as the client will use it, so that what is on screen and what is
        // being talked to are the same thing.
        let base = match instance_address(&self.auth.base_url) {
            Ok(base) => base,
            Err(message) => {
                self.auth.error = Some(message);
                return;
            }
        };

        self.auth.base_url = base.clone();
        self.auth.busy = true;
        self.auth.error = None;
        self.notice = None;

        self.spawn(ctx, move || {
            let mut api = Api::new(base.clone());
            let attempt = match mode {
                AuthMode::Login => api.login(&username, &password),
                AuthMode::Signup => api.signup(&username, &password),
            };

            match attempt {
                Ok(()) => {
                    // The server has just issued this session, so this is the one place that
                    // knows a token worth keeping. Written here, on the worker, because the
                    // file is not the UI thread's business.
                    if let Some(token) = api.token() {
                        session_file::save(&base, token);
                    }

                    Event::SignedIn {
                        api: Arc::new(api),
                        username,
                        base_url: base,
                    }
                }
                Err(error) => Event::SignInFailed(error.to_string()),
            }
        });
    }

    /// Drops the session and puts the sign-in dialog back up.
    ///
    /// `reason` is what to say about it, and is nothing at all when the user asked for this: a
    /// session that was refused is worth explaining, and a logout is not.
    fn signed_out(&mut self, reason: Option<&str>) {
        // The token is not worth keeping on disk either — and only that one, so a session handed in
        // through the environment being refused leaves a good stored session for the same server
        // alone. This is also what a logout does: the session the user is leaving goes with them.
        if let Some(token) = self.api.as_ref().and_then(|api| api.token()) {
            session_file::forget(&self.base_url, token);
        }

        self.api = None;
        self.icons = None;
        self.username = None;
        self.marks.clear();
        self.selected = 0;
        // The sign-in dialog takes the window back, so the panel has no business being up.
        self.config_open = false;
        self.auth = AuthForm {
            // The address stays: it is not a secret, and it is what the user would otherwise have
            // to type again to get back in. Everything else about the session is gone.
            base_url: self.base_url.clone(),
            error: reason.map(str::to_owned),
            focus_first_field: true,
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

    /// Asks before deleting the selected mark (Ctrl+D).
    ///
    /// Nothing is deleted here, and nothing is sent: this is the one thing the window does that
    /// cannot be taken back, so it is asked about first — in the same dialog, whether the ask came
    /// from the keyboard or from anywhere else that may grow one.
    fn ask_to_delete(&mut self) {
        let Some(mark) = selected_mark(&self.marks, &self.query, self.selected) else {
            return;
        };

        self.confirmation = Some(Confirmation {
            question: format!("Delete \"{}\"?", mark.name),
            note: "The mark goes for good, and the favicon kept for it goes with it.".to_owned(),
            confirm: "Delete".to_owned(),
            action: Pending::Delete {
                mark_id: mark.id.clone(),
                name: mark.name.clone(),
            },
        });
    }

    /// Deletes a mark, once the user has said to.
    fn delete_mark(&mut self, ctx: &egui::Context, mark_id: String, name: String) {
        let Some(api) = self.api.clone() else {
            return;
        };

        self.spawn(ctx, move || match api.delete_mark(&mark_id) {
            Ok(()) => Event::Deleted { mark_id, name },
            Err(ApiError::Unauthorized) => Event::SessionLost,
            Err(error) => Event::DeleteFailed(error.to_string()),
        });
    }

    /// Opens the edit dialog on the selected mark (Ctrl+E).
    ///
    /// What goes into the dialog is a copy of the mark rather than a place in the list: the list is
    /// being read while the dialog is typed at, and the mark can move in it — a rename re-orders it
    /// — so the dialog cannot be holding anything that moves.
    fn edit_selected(&mut self) {
        let Some(mark) = selected_mark(&self.marks, &self.query, self.selected) else {
            return;
        };

        let (mark_id, name, content) = (mark.id.clone(), mark.name.clone(), mark.content.clone());

        self.edit = Some(EditForm {
            mark_id,
            name,
            content,
            busy: false,
            error: None,
            focus_name: true,
        });
    }

    /// Sends the change that was typed into the edit dialog.
    fn save_edit(&mut self, ctx: &egui::Context) {
        let Some(edit) = self.edit.as_mut() else {
            return;
        };

        // The name is cut here rather than being refused after a round trip, and by the same rule
        // the search box's names are (see `MAX_NAME_CHARS`).
        let name: String = edit.name.trim().chars().take(MAX_NAME_CHARS).collect();
        let content = edit.content.trim().to_owned();

        if name.is_empty() || content.is_empty() {
            edit.error = Some("A mark needs a name and something in it.".to_owned());
            return;
        }

        let mark_id = edit.mark_id.clone();
        edit.busy = true;
        edit.error = None;

        let Some(api) = self.api.clone() else {
            return;
        };

        self.spawn(ctx, move || match api.update_mark(&mark_id, &name, &content) {
            Ok(mark) => Event::Updated(mark),
            Err(ApiError::Unauthorized) => Event::SessionLost,
            Err(error) => Event::UpdateFailed(error.to_string()),
        });
    }

    /// Puts a mark the server has just answered with where it belongs in the list, and selects it.
    ///
    /// The server orders marks by name, so a renamed one moves: the list is re-ordered here rather
    /// than reloaded to find out where it went.
    fn settle_mark(&mut self, mark: Mark) {
        let mark_id = mark.id.clone();

        match self.marks.iter_mut().find(|held| held.id == mark_id) {
            Some(held) => *held = mark,
            None => self.marks.push(mark),
        }

        self.marks.sort_by_key(|mark| mark.name.to_lowercase());
        self.selected = filter_marks(&self.marks, &self.query)
            .iter()
            .position(|candidate| candidate.id == mark_id)
            .unwrap_or(0);
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
            // Ctrl+E changes the selected mark, the same way Enter opens it.
            edit: input.consume_key(egui::Modifiers::CTRL, egui::Key::E),
            // Ctrl+, is what every program puts its settings behind.
            config: input.consume_key(egui::Modifiers::CTRL, egui::Key::Comma),
            close: input.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
        });

        if keys.close {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if keys.config {
            self.config_open = true;
            return;
        }
        if keys.up {
            self.move_selection(-1);
        }
        if keys.down {
            self.move_selection(1);
        }
        if keys.remove {
            self.ask_to_delete();
        }
        if keys.edit {
            self.edit_selected();
        }
        if keys.save {
            self.save_query(ctx);
        }
        if keys.open {
            self.open_selected(ctx);
        }
    }

    /// Keys that matter while the configuration panel is up: either of them closes it.
    ///
    /// Escape closes the panel rather than the window here, because while the panel is up that
    /// is the thing the user is looking at.
    fn config_keys(&mut self, ctx: &egui::Context) {
        let (toggle, escape) = ctx.input_mut(|input| {
            (
                input.consume_key(egui::Modifiers::CTRL, egui::Key::Comma),
                input.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
            )
        });

        if toggle || escape {
            self.config_open = false;
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

            // The server comes first, because it is the first thing that has to be right: this
            // client has no server of its own to fall back on, and a session belongs to one server.
            ui.add_space(8.0);
            ui.label("Server");
            let server = ui.add_enabled(
                !self.auth.busy,
                egui::TextEdit::singleline(&mut self.auth.base_url)
                    .hint_text("https://marks.example.com")
                    .desired_width(f32::INFINITY),
            );

            ui.add_space(4.0);
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

            // The dialog is the only thing to type into while it is up, so the caret starts where
            // the user has to start: at the server's address when there is not one yet, and at the
            // username when there is.
            if self.auth.focus_first_field {
                if self.auth.base_url.trim().is_empty() {
                    server.request_focus();
                } else {
                    username.request_focus();
                }

                self.auth.focus_first_field = false;
            }
        });

        request
    }

    /// The dialog a mark is changed in: its name, its content, and the two ways out.
    ///
    /// Enter saves and Escape leaves it alone, both read before the form is drawn so that Enter
    /// reaches this rather than going nowhere in a single-line field.
    fn edit_modal(&mut self, ctx: &egui::Context) -> Option<EditRequest> {
        let edit = self.edit.as_mut()?;
        let mut request = None;

        let (enter, escape) = ctx.input_mut(|input| {
            (
                input.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
                input.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
            )
        });

        if escape {
            request = Some(EditRequest::Cancel);
        } else if enter && !edit.busy {
            request = Some(EditRequest::Save);
        }

        egui::Modal::new(egui::Id::new("edit-mark")).show(ctx, |ui| {
            ui.set_width(320.0);
            ui.heading("Change this mark");

            ui.add_space(8.0);
            ui.label("Name");
            let name = ui.add_enabled(
                !edit.busy,
                egui::TextEdit::singleline(&mut edit.name).desired_width(f32::INFINITY),
            );

            ui.add_space(4.0);
            ui.label("Content");
            ui.add_enabled(
                !edit.busy,
                egui::TextEdit::singleline(&mut edit.content).desired_width(f32::INFINITY),
            );

            if let Some(error) = &edit.error {
                ui.add_space(6.0);
                ui.colored_label(ui.visuals().error_fg_color, error);
            }

            ui.add_space(10.0);
            ui.add_enabled_ui(!edit.busy, |ui| {
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        request = Some(EditRequest::Save);
                    }
                    if ui.button("Cancel").clicked() {
                        request = Some(EditRequest::Cancel);
                    }
                });
            });

            // The name is what the list shows and what the user came here to change as often as the
            // content, so the caret starts there — and there is only the one field it can start in.
            if edit.focus_name {
                name.request_focus();
                edit.focus_name = false;
            }
        });

        request
    }

    /// The question the window asks before doing something it cannot take back.
    ///
    /// Escape answers it with no, and no key answers it with yes: the buttons are the only way
    /// through, because what is being asked about is the one thing this window cannot undo.
    fn confirm_modal(&mut self, ctx: &egui::Context) -> Option<bool> {
        let confirmation = self.confirmation.as_ref()?;
        let mut answer = None;

        let escape =
            ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));

        if escape {
            answer = Some(false);
        }

        egui::Modal::new(egui::Id::new("confirmation")).show(ctx, |ui| {
            ui.set_width(320.0);
            ui.heading(&confirmation.question);
            ui.add_space(6.0);
            ui.weak(&confirmation.note);

            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.button(&confirmation.confirm).clicked() {
                    answer = Some(true);
                }
                if ui.button("Cancel").clicked() {
                    answer = Some(false);
                }
            });
        });

        answer
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

        // A launcher has nothing else to click, so the caret lives here — except while a dialog
        // is up, where the fields in it want the caret instead.
        if self.api.is_some() && !self.config_open && !response.has_focus() {
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

                            if mark_row(ui, &icon, mark, &self.query, index == self.selected).clicked() {
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
                // The sign-in dialog has the keyboard, so Esc is the only key worth naming.
                ui.weak(hints(&["Esc close"]));
                return;
            }

            if let Some(hint) = self.save_hint() {
                ui.strong(hint);
                ui.weak(HINT_SEPARATOR);
            }

            if let Some(username) = &self.username {
                ui.weak(format!("signed in as {username}"));
                ui.weak(HINT_SEPARATOR);
            }

            ui.weak(hints(&[
                "↑↓ move",
                "Enter open",
                "Ctrl+E change",
                // "delete" rather than "ask to delete": the hint names the key that starts the
                // thing, and what happens next is the dialog's business.
                "Ctrl+D delete",
                "Ctrl+, settings",
                "Esc close",
            ]));
        });
    }

    /// The configuration panel (Ctrl+,): what the window is allowed to be, and what it draws with.
    ///
    /// Everything it changes takes effect at once, on the window that is open, so that a size
    /// or a font can be tried rather than imagined; what the panel settles on is written to the
    /// settings file as it is settled.
    fn config_panel(&mut self, ctx: &egui::Context, window: egui::Rect) {
        // The panel is where fonts are chosen, so this is where the machine's are asked for.
        self.ensure_system_fonts(ctx);

        // Gathered while the panel is drawn and acted on after it, because the panel is reading
        // from `self` while these would be changing it.
        let mut fixed_size = None;
        // The window should follow the numbers as they are dragged...
        let mut apply = false;
        // ...and the file is written once the edit is over, rather than once per frame of a drag.
        let mut keep = false;
        // A font moving up or down the list, taken out of it, or put back into it, worked out as
        // the list is drawn.
        let mut moved = None;
        let mut removed = None;
        let mut added = None;
        // Leaving the session, which is a bigger decision than any of the others and the only one
        // that ends the panel's business.
        let mut log_out = false;

        // What is left of the window once the panel's own margins are taken out of it. A window
        // can be set smaller than the panel is tall — the smallest it may be is — and without
        // somewhere to scroll, the top of the panel is drawn above the window and cannot be
        // reached at all.
        let room = (window.height() - 80.0).max(100.0);

        egui::Modal::new(egui::Id::new("configuration")).show(ctx, |ui| {
            ui.set_width(380.0);

            egui::ScrollArea::vertical().max_height(room).show(ui, |ui| {
            ui.heading("Configuration");

            // Who this window is signed in as, and to what, with the one way back to the sign-in
            // dialog. At the top rather than in a section of its own at the foot: the panel scrolls
            // in a window that is short, and leaving a session should not be the control that has to
            // be scrolled to.
            //
            // The username is not always known: a session handed in through the environment is a
            // session without a sign-in, and this run never learned whose it is.
            ui.add_space(6.0);
            ui.weak(match &self.username {
                Some(username) => format!("Signed in as {username} to {}", self.base_url),
                None => format!("Signed in to {}", self.base_url),
            });
            ui.add_space(4.0);
            if ui
                .button("Log out")
                .on_hover_text("Forget this session, and sign in to any server")
                .clicked()
            {
                log_out = true;
            }

            ui.add_space(10.0);

            ui.strong("Window");
            ui.add_space(4.0);

            let mut fixed = self.config.window.fixed_size;
            if ui
                .checkbox(&mut fixed, "Keep the window at one size")
                .on_hover_text("A window that is not fixed can be resized as usual")
                .changed()
            {
                fixed_size = Some(fixed);
            }

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label("Opens at");
                let width = ui.add(
                    egui::DragValue::new(&mut self.config.window.width)
                        .range(MIN_WIDTH..=MAX_SIDE)
                        .suffix(" px"),
                );
                ui.label("x");
                let height = ui.add(
                    egui::DragValue::new(&mut self.config.window.height)
                        .range(MIN_HEIGHT..=MAX_SIDE)
                        .suffix(" px"),
                );

                apply = width.changed() || height.changed();
                keep = edit_finished(&width) || edit_finished(&height);
            });

            ui.weak(if self.config.window.fixed_size {
                "The window stays at this size."
            } else {
                "The window opens at this size, and can be resized after."
            });
            // Only once a size has been asked for: a window manager has the last word on a
            // window's size, and a tiling one has the only word, so this is asked for rather
            // than imposed — which is worth saying after a change, and is noise before one.
            if self.window_settings_changed {
                ui.label(restart_note(ui));
            }

            ui.add_space(12.0);
            ui.strong("Fonts");
            ui.add_space(4.0);
            ui.weak("The first font that has a character is the one that draws it.");

            // A copy, because the list is being read while a move, an addition or a removal is
            // being decided on.
            let priority = self.config.fonts.priority.clone();
            let last = priority.len().saturating_sub(1);
            // The last font cannot be taken out: the window has to be drawn in something.
            let removable = priority.len() > 1;

            for (index, name) in priority.iter().enumerate() {
                ui.horizontal(|ui| {
                    if index == 0 {
                        ui.strong(name);
                    } else {
                        ui.label(name);
                    }

                    // Buttons right-aligned, and every row the same width whether or not one of
                    // them is disabled.
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add_enabled(removable, icon_button(REMOVE_ICON))
                            .on_hover_text("Draw without this font")
                            .clicked()
                        {
                            removed = Some(index);
                        }
                        if ui
                            .add_enabled(index < last, icon_button(LATER_ICON))
                            .on_hover_text("Draw with this font after the ones above it")
                            .clicked()
                        {
                            moved = Some((index, 1));
                        }
                        if ui
                            .add_enabled(index > 0, icon_button(SOONER_ICON))
                            .on_hover_text("Draw with this font before the ones below it")
                            .clicked()
                        {
                            moved = Some((index, -1));
                        }
                    });
                });
            }

            // Adding one: egui's own select, with the field that searches it at the top of the
            // dropdown.
            //
            // The button, its arrow, the frame, the scrolling, the closing on a press anywhere else
            // and the keyboard are all egui's. A dropdown of the panel's own — which this was — has
            // to get the focus, the frame and the order of a press and its release right in every
            // case, and that is what a widget is for.
            //
            // A select has no way to search what it offers, and a machine can have a thousand
            // families, so the field goes inside the dropdown: that is what a searchable select in
            // egui is made of, and it is also why the dropdown is asked to stay up while its own
            // field and its own rows are pressed — a press inside would otherwise be the press that
            // closed it.
            ui.add_space(6.0);

            // Egui's own menu colour is the colour of the dialog this panel is drawn on, and a
            // dropdown the same colour as what is behind it does not read as a list of things to
            // choose from. It is set into the panel in the colour the panel sets its own fields
            // into, which follows the theme rather than being mixed here.
            let inset = ui.visuals().extreme_bg_color;
            let button = ui.make_persistent_id(egui::IdSalt::new(ADD_FONT));

            // Read before the dropdown is drawn, so that on the frame it opens this is still last
            // frame's answer: that is how "it has just opened" is told from "it is open", and so
            // how the search field takes the keyboard once rather than on every frame.
            let was_open = egui::ComboBox::is_open(ctx, button);

            egui::ComboBox::from_id_salt(ADD_FONT)
                .selected_text("Add a font")
                .width(ui.available_width())
                // Not the default: egui's default closes a menu on any press, and the presses
                // inside this one are how the search is typed at and how a font is picked.
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                // `.into()` because `popup_style` asks for a `StyleModifier`, which egui does not
                // name at its root.
                .popup_style((move |style: &mut egui::Style| style.visuals.window_fill = inset).into())
                .show_ui(ui, |ui| {
                    // The dropdown is one colour, so the field is its first row rather than a box
                    // set into it; what says it is a field is the caret in it.
                    let search = ui.add(
                        egui::TextEdit::singleline(&mut self.font_query)
                            .hint_text("Search the fonts on this machine")
                            .desired_width(f32::INFINITY)
                            .background_color(inset),
                    );

                    // Asked for again whenever nothing at all has the keyboard, which is what a
                    // press on one of the rows below leaves behind: the dropdown stays up after a
                    // font is added, and without this the next name would have to be clicked for.
                    if !was_open || ui.memory(|memory| memory.focused().is_none()) {
                        search.request_focus();
                    }

                    ui.add_space(4.0);

                    match &self.system_fonts {
                        None => {
                            ui.weak("Looking for the fonts on this machine...");
                        }
                        Some(system) => {
                            let (shown, answered) = fonts_to_add(&self.font_query, system, &priority);

                            if shown.is_empty() {
                                ui.weak("Nothing by that name is left to add.");
                            }

                            // Scrolled inside the dropdown rather than by it: the search field stays
                            // where it is while the list under it moves, and a dropdown tall enough
                            // to hold every font a machine has is taller than the window.
                            egui::ScrollArea::vertical().max_height(LIST_HEIGHT).show(ui, |ui| {
                                for name in &shown {
                                    if ui.selectable_label(false, name).clicked() {
                                        added = Some(name.clone());
                                    }
                                }
                            });

                            if answered > shown.len() {
                                ui.weak(format!(
                                    "and {} more: type more of the name",
                                    answered - shown.len()
                                ));
                            }
                        }
                    }
                });

            ui.add_space(12.0);
            ui.weak(match self.config.path() {
                Some(path) => format!("Settings are kept in {}", path.display()),
                None => "This system has no config directory to keep settings in.".to_owned(),
            });
            ui.add_space(4.0);
            ui.weak("Ctrl+, or Esc closes this panel");
            });
        });

        // Logging out is the end of the session rather than a change to it: nothing else that was
        // worked out while the panel was drawn is worth carrying out afterwards, and the panel has
        // just been taken down with the session.
        if log_out {
            self.signed_out(None);
            return;
        }

        // One decision, applied and written in the same breath; then the size, which is applied
        // as it moves and written when it settles.
        if let Some(name) = added {
            self.add_font(ctx, &name);
        } else if let Some(index) = removed {
            self.remove_font(ctx, index);
        } else if let Some((index, delta)) = moved {
            self.move_font(ctx, index, delta);
        } else if let Some(fixed) = fixed_size {
            self.set_fixed_size(ctx, fixed);
        } else if keep {
            self.settle_window(ctx);
        } else if apply {
            self.config.window.clamp();
            self.apply_window_settings(ctx);
        }
    }

    /// Moves the font at `index` one place up or down the priority, and writes the order down.
    fn move_font(&mut self, ctx: &egui::Context, index: usize, delta: isize) {
        let priority = &mut self.config.fonts.priority;
        let Some(target) = index
            .checked_add_signed(delta)
            .filter(|target| *target < priority.len())
        else {
            // Off either end of the list, which the buttons do not offer but a call could ask.
            return;
        };

        priority.swap(index, target);

        self.settle_fonts(ctx);
    }

    /// Takes the font at `index` out of the order, and writes the new one down.
    ///
    /// A font that is not in the order is not drawn with at all, and the window falls through to
    /// the ones after it. The last one cannot go — a font order with nothing in it is a window
    /// with no text in it — which the panel's button says by being disabled.
    fn remove_font(&mut self, ctx: &egui::Context, index: usize) {
        // Nothing left to draw with, or nothing at that place in the list.
        if self.config.fonts.priority.len() <= 1 || index >= self.config.fonts.priority.len() {
            return;
        }

        self.config.fonts.priority.remove(index);

        self.settle_fonts(ctx);
    }

    /// Puts `name` at the end of the order, and writes the new one down.
    ///
    /// Last rather than first: a font that has just been added is one more to fall back to, and
    /// the ones already in the order are the ones the window is drawn in. Moving it up is the
    /// next thing the panel offers.
    fn add_font(&mut self, ctx: &egui::Context, name: &str) {
        let held = self.config.fonts.priority.iter().any(|font| font == name);

        // One of the fonts egui carries, or a family this machine has: the two kinds of name that
        // something can be found behind. A name that is neither is refused, because
        // [`crate::fonts::install`] leaves it out of the order anyway — a font that is silently
        // passed over every time the window is drawn is worse than one that was never added.
        //
        // The machine's families are the ones that matter here. The panel's dropdown offers every
        // one of them, and since the order starts out naming all four of egui's fonts, every row it
        // can offer is one of the machine's — so refusing those made a press on any row do nothing
        // at all, under a dropdown that had just offered it.
        let known = fonts::AVAILABLE.contains(&name)
            || self
                .system_fonts
                .as_ref()
                .is_some_and(|system| system.families().iter().any(|family| family == name));

        if held || !known {
            return;
        }

        self.config.fonts.priority.push(name.to_owned());

        self.settle_fonts(ctx);
    }

    /// Puts the fonts onto the window that is open, and the order on disk.
    fn settle_fonts(&mut self, ctx: &egui::Context) {
        self.config.fonts.settle();
        self.install_fonts(ctx);
        self.config.save();
    }

    /// Puts the order the settings hold onto the window.
    ///
    /// Separate from `settle_fonts` because it is also what happens when the machine's fonts
    /// arrive: the order has not changed then, but the fonts that can satisfy it have.
    fn install_fonts(&self, ctx: &egui::Context) {
        fonts::install(ctx, &self.config.fonts.priority, self.system_fonts.as_deref());
    }

    /// Asks for the fonts on this machine, if they have not been read yet.
    ///
    /// On a worker thread, like every other request this window makes: a machine can have
    /// thousands of fonts and reading their names takes long enough to be seen. The order is put
    /// onto the window again when they arrive, since a font the settings named may only now be
    /// loadable.
    fn ensure_system_fonts(&mut self, ctx: &egui::Context) {
        if self.system_fonts.is_some() || self.loading_fonts {
            return;
        }

        self.loading_fonts = true;
        self.spawn(ctx, || Event::SystemFonts(Arc::new(SystemFonts::load())));
    }

    /// Whether `name` is one of the fonts egui carries, and so needs nothing read to use.
    fn bundled(name: &str) -> bool {
        fonts::AVAILABLE.contains(&name)
    }

    /// Keeps the window at one size, or frees it again, and writes the choice down.
    fn set_fixed_size(&mut self, ctx: &egui::Context, fixed: bool) {
        self.config.window.fixed_size = fixed;
        self.settle_window(ctx);
    }

    /// Brings the window in line with the settings, and the settings in line with the disk.
    ///
    /// What the panel calls once an edit is over: the numbers themselves are edited in place,
    /// so this is where they are brought within reach, put on the window, and written down.
    fn settle_window(&mut self, ctx: &egui::Context) {
        self.config.window.clamp();
        self.apply_window_settings(ctx);
        self.config.save();

        // A size has been asked for, so the panel can now say what to do if it does not arrive.
        self.window_settings_changed = true;
    }

    /// Puts the settings onto the window that is already open.
    ///
    /// The same three things the window was opened with (see `Window::viewport`), sent again
    /// because a window that is up is told what to be rather than rebuilt.
    fn apply_window_settings(&self, ctx: &egui::Context) {
        let window = &self.config.window;
        let size = window.size();

        // The order of these is not arbitrary. A window is made unresizable by *pinning* it: on
        // X11 `set_resizable(false)` writes whatever size the window is at that moment as both
        // the smallest it may be and the largest, so asking for it first pins the window to the
        // size it is about to leave and the new size never arrives. Constraints and size first,
        // and the pinning last.
        if window.fixed_size {
            // Held at exactly one size, by both ends of it rather than by the hint alone: a
            // window manager is free to ignore "not resizable", and none of them ignores a
            // smallest size equal to the largest.
            ctx.send_viewport_cmd(egui::ViewportCommand::MaxInnerSize(size));
            ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(size));
        } else {
            // No largest size — which is what `INFINITY` says to egui — and the floor the list
            // needs to be drawn in.
            ctx.send_viewport_cmd(egui::ViewportCommand::MaxInnerSize(egui::Vec2::INFINITY));
            ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(config::floor()));
        }

        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
        ctx.send_viewport_cmd(egui::ViewportCommand::Resizable(!window.fixed_size));
    }

    /// Draws one frame of the window.
    ///
    /// Kept apart from the [`eframe::App`] implementation so that a frame can be run without
    /// eframe — by the tests below, which drive the real keybindings and the real requests.
    pub fn show(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();

        self.drain_events(&ctx);

        // While a dialog is up there is no list to drive, so the launcher keys are left alone: Enter
        // belongs to the form or to the question. The configuration panel is the same, and each
        // dialog reads the keys that are its own.
        let mut sign_in = None;
        if self.api.is_none() {
            sign_in = self.modal_keys(&ctx);
        } else if self.config_open {
            self.config_keys(&ctx);
        } else if self.edit.is_none() && self.confirmation.is_none() {
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
        } else if self.config_open {
            self.config_panel(&ctx, ui.max_rect());
        }

        // The dialogs ask, and the window acts on the answers here: neither of them changes anything
        // itself, so that what is asked and what is done about it are written in one place.
        if let Some(request) = self.edit_modal(&ctx) {
            match request {
                EditRequest::Save => self.save_edit(&ctx),
                EditRequest::Cancel => self.edit = None,
            }
        }

        if let Some(confirmed) = self.confirm_modal(&ctx) {
            match self.confirmation.take() {
                Some(Confirmation {
                    action: Pending::Delete { mark_id, name },
                    ..
                }) if confirmed => self.delete_mark(&ctx, mark_id, name),
                // Answered with "no", or gone for some other reason: the answer to a question about
                // something that cannot be taken back is, by default, that nothing happens.
                _ => {}
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
///
/// `query` is what is in the search box: the characters it found in either line are drawn
/// brighter than the rest of it, so a row that a fuzzy match put here says why it is here.
fn mark_row(
    ui: &mut egui::Ui,
    icon: &IconState,
    mark: &Mark,
    query: &str,
    selected: bool,
) -> egui::Response {
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
                    let name = ui.visuals().strong_text_color();
                    ui.label(highlighted(ui, &mark.name, query, egui::TextStyle::Body, name));

                    let link = ui.visuals().weak_text_color();
                    ui.label(highlighted(
                        ui,
                        &mark.content,
                        query,
                        egui::TextStyle::Monospace,
                        link,
                    ));
                });
            });
        })
        .response
        // The row is clicked as a whole, so the pointer does not have to find the text.
        .interact(egui::Sense::click())
}

/// A line of text with the characters the query matched picked out of it.
///
/// Those characters are drawn in the window's strong colour and the rest of the line dimmed
/// around them — but only when the query found something here. A line nothing was matched in,
/// and every line while the search box is empty, is drawn in `plain` from end to end, so a row
/// with no query in play looks exactly as it always did.
fn highlighted(
    ui: &egui::Ui,
    text: &str,
    query: &str,
    style: egui::TextStyle,
    plain: egui::Color32,
) -> egui::text::LayoutJob {
    let font = style.resolve(ui.style());
    let hits = search::find(text, query)
        .map(|found| found.indices)
        .unwrap_or_default();

    let (plain, matched) = if hits.is_empty() {
        (plain, plain)
    } else {
        (ui.visuals().weak_text_color(), ui.visuals().strong_text_color())
    };

    let mut job = egui::text::LayoutJob::default();
    let mut cursor = 0;

    for &hit in &hits {
        if hit > cursor {
            job.append(&text[cursor..hit], 0.0, stretch(plain, font.clone()));
        }

        let end = hit + text[hit..].chars().next().map_or(0, char::len_utf8);
        job.append(&text[hit..end], 0.0, stretch(matched, font.clone()));
        cursor = end;
    }

    if cursor < text.len() {
        job.append(&text[cursor..], 0.0, stretch(plain, font));
    }

    job
}

/// One stretch of a line, in one colour and one font.
fn stretch(color: egui::Color32, font: egui::FontId) -> egui::TextFormat {
    egui::TextFormat {
        font_id: font,
        color,
        ..Default::default()
    }
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

/// The session token handed in through the environment, when there is one.
///
/// `MARKS_TOKEN` is how a session that exists elsewhere — a browser, a script, another window
/// — is reused here without signing in again.
fn handed_in_token() -> Option<String> {
    std::env::var("MARKS_TOKEN")
        .ok()
        .map(|token| token.trim().to_owned())
        .filter(|token| !token.is_empty())
}

/// The server to talk to, and the session to start from, from the environment and from the
/// session kept on disk.
///
/// There is no server this client falls back on. The sign-in dialog asks which one, and the answer
/// is kept with the session, so a window that has run before opens against the server the last run
/// used — and one that never has opens with the dialog asking and nothing filled in.
///
/// `MARKS_URL` names a server outright, and `MARKS_TOKEN` hands in a session for it: the two
/// together are how a session that exists elsewhere — a browser, a script, another window — is
/// reused without signing in again. The environment wins, because a session handed in was asked for
/// on purpose and a stale one on disk should not overrule it. A `MARKS_TOKEN` with no `MARKS_URL`
/// beside it is nothing this client can use: there is no server to present it to.
///
/// Called by `main` only: everything here reads something off the machine, which is exactly
/// what a test should not be doing.
pub fn starting_session() -> (String, Option<String>) {
    let named = std::env::var("MARKS_URL")
        .ok()
        .map(|url| url.trim().trim_end_matches('/').to_owned())
        .filter(|url| !url.is_empty());

    // The session kept on disk, whichever server it was kept for: with nothing named, its address
    // is the only one there is to go on.
    let stored = session_file::stored();
    let base_url = server_to_start_from(
        named,
        stored.as_ref().map(|(base_url, _)| base_url.as_str()),
    );

    let token = if base_url.is_empty() {
        None
    } else {
        handed_in_token().or_else(|| session_file::load(&base_url))
    };

    (base_url, token)
}

/// Which server to open against: the one the environment names, or the one the last run's session
/// was kept for, and nothing at all when neither says.
///
/// A function of those two answers rather than of the machine, so that the rule this client is built
/// on — that it has no server of its own — is one a test can hold it to without reading anything off
/// the machine it happens to be running on.
fn server_to_start_from(named: Option<String>, stored: Option<&str>) -> String {
    named
        .or_else(|| stored.map(str::to_owned))
        .unwrap_or_default()
}

/// The server address as typed into the sign-in dialog, in the form the rest of this client uses:
/// a scheme, a host, and no trailing slash.
///
/// A bare host is taken as `http://`, the way the search box takes a bare host as a mark: the
/// address is typed by hand, and nobody types the scheme first. Anything without a host in it is
/// refused, because a client pointed at nothing can only fail later and less clearly — and the
/// dialog is the one place where the mistake is still cheap to correct.
fn instance_address(typed: &str) -> Result<String, String> {
    let trimmed = typed.trim().trim_end_matches('/');

    if trimmed.is_empty() {
        return Err("Which server? Type its address.".to_owned());
    }

    let with_scheme = if trimmed.contains("://") {
        trimmed.to_owned()
    } else {
        format!("http://{trimmed}")
    };

    match url::Url::parse(&with_scheme) {
        Ok(url) if matches!(url.scheme(), "http" | "https") && url.host_str().is_some() => {
            // Through the parser and back, so that what is used, what is written down, and what the
            // dialog then shows are all the same string.
            Ok(url.as_str().trim_end_matches('/').to_owned())
        }
        _ => Err(format!("{trimmed} is not a server address.")),
    }
}

/// Whether an edit to a number is over, so that what it left can be written down.
///
/// A number being dragged reports a change on every frame of the drag, and a settings file is
/// not worth writing sixty times a second for one of them; a value that is typed or stepped is
/// over as soon as it changes.
fn edit_finished(response: &egui::Response) -> bool {
    response.drag_stopped() || response.lost_focus() || (response.changed() && !response.dragged())
}

/// The marks the query keeps, the best answer to it first.
///
/// A free function rather than a method so that the borrow it takes is limited to `marks`:
/// drawing the rows needs `self.icons` mutably while this list is still alive.
fn filter_marks<'a>(marks: &'a [Mark], query: &str) -> Vec<&'a Mark> {
    let mut found: Vec<(i64, &Mark)> = marks
        .iter()
        .filter_map(|mark| score(mark, query).map(|score| (score, mark)))
        .collect();

    // Best first. The sort is stable, so anything the matcher scored the same keeps the order it
    // arrived in — which is every mark while the search box is empty, and so the list is left
    // exactly as the server sent it until there is something to rank.
    found.sort_by_key(|(score, _)| std::cmp::Reverse(*score));

    found.into_iter().map(|(_, mark)| mark).collect()
}

/// How well `mark` answers the query, or `None` when neither its name nor its link does.
///
/// The better of the two: a mark found by its name and one found by the page it points at are
/// both answers to the query, and which of them answers it better is what the matcher's score
/// says. Typing "you" scores the YouTube mark's name far above a news link whose address happens
/// to carry a y, an o and a u across three different words.
fn score(mark: &Mark, query: &str) -> Option<i64> {
    let name = search::find(&mark.name, query).map(|found| found.score);
    let link = search::find(&mark.content, query).map(|found| found.score);

    match (name, link) {
        (Some(name), Some(link)) => Some(name.max(link)),
        (name, link) => name.or(link),
    }
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

/// What the panel's buttons are drawn as: one font moving up the order, one moving down, and one
/// leaving it.
///
/// Icons rather than words, and from the client's own icon font rather than from the fonts the
/// settings choose — which is what [`fonts::ICON_FAMILY`] is for, and why these cannot be drawn
/// as a box by a font order that has been rearranged.
const SOONER_ICON: &str = egui_phosphor::regular::CARET_UP;
const LATER_ICON: &str = egui_phosphor::regular::CARET_DOWN;
const REMOVE_ICON: &str = egui_phosphor::regular::X;

/// How large those are drawn: the size of the text they sit beside.
const ICON_SIZE: f32 = 14.0;

/// The font an icon is drawn in: the client's own, never one of the settings' fonts.
///
/// A free function so that the decision is in one place and can be asserted on: the button below
/// is one line that uses it, and *that* is the thing worth checking.
fn icon_font() -> egui::FontId {
    egui::FontId::new(ICON_SIZE, egui::FontFamily::Name(fonts::ICON_FAMILY.into()))
}

/// A button whose label is one icon.
fn icon_button(icon: &str) -> egui::Button<'static> {
    egui::Button::new(egui::RichText::new(icon.to_owned()).font(icon_font()))
}

/// The fonts the panel offers to add, best answer to the query first, and how many answered it.
///
/// Every family this machine has and every font egui carries, less the ones the order already
/// names. The count is the whole of it rather than the part shown, so that the panel can say how
/// many were left out.
fn fonts_to_add(query: &str, system: &SystemFonts, priority: &[String]) -> (Vec<String>, usize) {
    let mut found: Vec<(i64, String)> = fonts::AVAILABLE
        .iter()
        .map(|name| (*name).to_owned())
        .chain(system.families().iter().cloned())
        .filter(|name| !priority.iter().any(|held| held == name))
        .filter_map(|name| search::find(&name, query).map(|found| (found.score, name)))
        .collect();

    // Best first, and the order they came in for anything scored the same — which is everything
    // while the field is empty, so the list reads as egui's fonts and then the machine's, sorted.
    found.sort_by_key(|(score, _)| std::cmp::Reverse(*score));

    let answered = found.len();

    (
        found
            .into_iter()
            .take(ADD_LIMIT)
            .map(|(_, name)| name)
            .collect(),
        answered,
    )
}

/// What identifies the add control: the salt egui hashes into the dropdown's id.
///
/// Both halves of the picker need it — the widget, and `ComboBox::is_open`. `from_id_salt` hashes
/// what it is given with `IdSalt::new`, so a call site that passed an `IdSalt` would hash it twice
/// and never match the widget it is asking about.
const ADD_FONT: &str = "add-font";

/// How tall the list inside the dropdown is allowed to grow before it scrolls.
///
/// Under egui's own cap on a dropdown's height, so that the search field above it stays where it
/// is: what scrolls is the list, not the dropdown it is set in.
const LIST_HEIGHT: f32 = 140.0;

/// How many fonts the panel lists at once: enough to choose from, few enough to draw every frame.
const ADD_LIMIT: usize = 60;

/// What the panel says about a size change that the window manager may not have made.
///
/// Shown only once the window's settings have been changed, because it is the answer to "why did
/// nothing happen" rather than a description of anything: before a change it answers a question
/// nobody has asked.
const RESTART_NOTE: &str =
    "Some window managers only apply a size when the client is opened again.";

/// [`RESTART_NOTE`], in the colour of something worth noticing.
///
/// The theme's warning colour rather than a yellow of its own. It is orange in both of egui's
/// themes, which is as close to yellow as is readable — the same yellow that stands out on a
/// dark window is the one thing on a light one that is harder to read than the small print it
/// would be replacing.
fn restart_note(ui: &egui::Ui) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    job.append(
        RESTART_NOTE,
        0.0,
        stretch(
            ui.visuals().warn_fg_color,
            egui::TextStyle::Body.resolve(ui.style()),
        ),
    );

    job
}

/// What goes between one hint along the bottom of the window and the next.
///
/// A middle dot is the character for this, but a middle dot is what the footer already had the
/// look of: `·` is a hairline, and this wants to read as a separator. egui has no bold to reach
/// for — `strong()` is a brighter colour, not a heavier stroke, and none of the fonts it carries
/// has a bold face — so the weight comes from the character instead. Measured at the size the
/// footer is drawn, a bullet puts down 8.8 of ink against the middle dot's 2.7 in Hack, and 7.0
/// against 1.4 in Ubuntu-Light; the dot operator `\cdot` is (U+22C5) is no heavier than the
/// middle dot, and is missing from Ubuntu-Light altogether.
///
/// Whichever font the settings put first has to have it, which both of egui's Latin fonts do. It
/// is also in the set the window's fonts are checked for (`fonts::WRITTEN`), so an order that
/// cannot draw it is caught by the tests rather than shown as a box.
const HINT_SEPARATOR: &str = "•";

/// The hints as one line, each separated from the next by [`HINT_SEPARATOR`].
fn hints(items: &[&str]) -> String {
    items.join(&format!(" {HINT_SEPARATOR} "))
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

/// The tests, in a file of their own: `app/tests.rs`, compiled only for test builds.
#[cfg(test)]
mod tests;
