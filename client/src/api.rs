//! Talking to a marks server.
//!
//! One client per server, holding the session token that makes its requests the signed-in user's
//! ([`Api::with_token`], and `session_file` for where that token comes from). Every call is
//! blocking, so it belongs on a worker thread: `MarksApp::spawn` is the only thing that makes one,
//! and it is the reason the window never waits on the network.
//!
//! The server writes its failures as `{"error": "..."}` (see `parseJsonBody` on the server side),
//! and that wording is passed through rather than replaced — the server knows why it said no. A
//! 401 is the exception: it is read as [`ApiError::Unauthorized`], so that the window can put the
//! sign-in dialog back up instead of showing the message.

use std::fmt;
use std::time::Duration;

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::json;
use ureq::http::Response;
use ureq::{Agent, Body};

use crate::mark::Mark;

/// How long a single request may take before it counts as failed. Everything here is small
/// and the server is expected to be nearby; a longer wait than this is a server that is not
/// there, and the window should say so rather than hang on it.
const TIMEOUT: Duration = Duration::from_secs(10);

/// A ceiling on a favicon body: far above any real icon, and low enough that a wrong answer
/// cannot make the decoder chew through something enormous.
const MAX_ICON_BYTES: u64 = 1024 * 1024;

/// The name the server gives its session cookie, which is `SESSION_COOKIE_NAME` on the other
/// side of the wire. Both the header this client sends and the one the server answers with are
/// read by this name, so it is written down once.
const SESSION_COOKIE: &str = "token";

/// Why a request failed, reduced to what the UI does about it.
#[derive(Debug)]
pub enum ApiError {
    /// The server answered 401: the session cookie is missing, unknown, or expired.
    Unauthorized,
    /// Anything else, phrased for the status line.
    Message(String),
}

impl fmt::Display for ApiError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // Reached when a 401 arrives somewhere that does not hand it to the sign-in
            // modal; the wording matches the server's own.
            Self::Unauthorized => write!(formatter, "Not signed in"),
            Self::Message(message) => write!(formatter, "{message}"),
        }
    }
}

type ApiResult<T> = Result<T, ApiError>;

/// A client for one marks server.
///
/// Requests are blocking, so every call belongs on a worker thread: the UI thread only ever
/// reads what came back (see `MarksApp::spawn`). After [`Api::login`] or [`Api::signup`] the
/// session token travels on every later request, which is what makes an [`Api`] the thing
/// the rest of the app shares once it is `Arc`-wrapped.
pub struct Api {
    agent: Agent,
    base: String,
    /// The value of the session cookie the server set, without the name or the attributes it
    /// travels with. Kept as the value rather than as the whole header so that a caller can
    /// hold on to it (see [`Api::token`] and `session_file`).
    token: Option<String>,
}

impl Api {
    /// Builds a client for `base`, e.g. `http://127.0.0.1:5173`.
    pub fn new(base: impl Into<String>) -> Self {
        // Statuses are read here rather than turned into errors by ureq, so that a rejected
        // request can be reported with the server's own wording (see `parseJsonBody`).
        let config = Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(TIMEOUT))
            .build();

        Self {
            agent: Agent::new_with_config(config),
            base: base.into().trim_end_matches('/').to_owned(),
            token: None,
        }
    }

    /// A client that starts out signed in, for a session token obtained elsewhere.
    ///
    /// The server names its session cookie `token` (see `SESSION_COOKIE_NAME`), so a bare
    /// token value is all this needs.
    pub fn with_token(base: impl Into<String>, token: &str) -> Self {
        let mut api = Self::new(base);
        api.token = Some(token.to_owned());

        api
    }

    /// The session token this client signs its requests with, if it has one.
    ///
    /// Handed out so that the session can outlive the process: whoever signs in writes what
    /// comes back here somewhere the next run can read it (`session_file::save`).
    pub fn token(&self) -> Option<&str> {
        self.token.as_deref()
    }

    /// Signs in to an existing account and keeps the session cookie the server sets.
    pub fn login(&mut self, username: &str, password: &str) -> ApiResult<()> {
        self.authenticate("/api/auth/login", username, password)
    }

    /// Creates an account and keeps the session cookie the server sets.
    pub fn signup(&mut self, username: &str, password: &str) -> ApiResult<()> {
        self.authenticate("/api/auth/signup", username, password)
    }

    /// The signed-in user's marks, in the server's order (by name).
    pub fn list_marks(&self) -> ApiResult<Vec<Mark>> {
        let mut response = self.get("/api/marks")?;

        Ok(read_json::<MarksBody>(&mut response)?.marks)
    }

    /// Saves a new mark and returns it as the server stored it, id and icon included.
    pub fn create_mark(&self, name: &str, content: &str) -> ApiResult<Mark> {
        let mut request = self
            .agent
            .post(self.url("/api/marks"))
            .header("content-type", "application/json");
        request = self.authorize(request);

        let mut response = request
            .send_json(json!({ "name": name, "content": content }))
            .map_err(transport)?;

        Ok(read_json::<MarkBody>(&mut response)?.mark)
    }

    /// Deletes a mark. The server answers 204, so there is nothing to read back.
    pub fn delete_mark(&self, mark_id: &str) -> ApiResult<()> {
        // Mark ids are server-generated UUIDs, so they are safe to place in a path as they are.
        let mut response = self.delete(&format!("/api/marks/{mark_id}"))?;

        read_empty(&mut response)
    }

    /// The raw favicon bytes the server stored for a mark.
    pub fn fetch_icon(&self, mark_id: &str) -> ApiResult<Vec<u8>> {
        let mut response = self.get(&format!("/api/marks/{mark_id}/icon"))?;

        let status = response.status().as_u16();
        if status == 401 {
            return Err(ApiError::Unauthorized);
        }
        if !(200..300).contains(&status) {
            return Err(ApiError::Message(error_message(&mut response)));
        }

        response
            .body_mut()
            .with_config()
            .limit(MAX_ICON_BYTES)
            .read_to_vec()
            .map_err(|error| ApiError::Message(format!("Could not read the favicon: {error}")))
    }

    fn authenticate(&mut self, path: &str, username: &str, password: &str) -> ApiResult<()> {
        let mut response = self
            .agent
            .post(self.url(path))
            .header("content-type", "application/json")
            .send_json(json!({ "username": username, "password": password }))
            .map_err(transport)?;

        // 401 is an ordinary outcome here rather than an expired session, so it keeps the
        // server's wording ("Incorrect username or password").
        if response.status().as_u16() == 401 {
            return Err(ApiError::Message(error_message(&mut response)));
        }

        // 200 for an existing account, 201 for a new one; both carry the session cookie.
        read_json::<serde_json::Value>(&mut response)?;

        self.token = session_token(&response);
        if self.token.is_none() {
            return Err(ApiError::Message(
                "The server accepted the credentials but sent no session cookie.".to_owned(),
            ));
        }

        Ok(())
    }

    fn get(&self, path: &str) -> ApiResult<Response<Body>> {
        self.authorize(self.agent.get(self.url(path))).call().map_err(transport)
    }

    fn delete(&self, path: &str) -> ApiResult<Response<Body>> {
        self.authorize(self.agent.delete(self.url(path))).call().map_err(transport)
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    /// The session token, when there is one, is attached to whatever request is being built.
    ///
    /// This is what lets the same helper serve both the unauthenticated sign-in calls and
    /// every call that follows them.
    fn authorize<S>(&self, request: ureq::RequestBuilder<S>) -> ureq::RequestBuilder<S> {
        match &self.token {
            Some(token) => request.header("cookie", format!("{SESSION_COOKIE}={token}")),
            None => request,
        }
    }
}

/// Reads a JSON body, or turns the status into the error the UI should show.
fn read_json<T: DeserializeOwned>(response: &mut Response<Body>) -> ApiResult<T> {
    let status = response.status().as_u16();
    if status == 401 {
        return Err(ApiError::Unauthorized);
    }
    if !(200..300).contains(&status) {
        return Err(ApiError::Message(error_message(response)));
    }

    response
        .body_mut()
        .read_json::<T>()
        .map_err(|error| ApiError::Message(format!("Unexpected answer from the server: {error}")))
}

/// Checks a response that carries no body, such as the 204 from `DELETE /api/marks/[id]`.
fn read_empty(response: &mut Response<Body>) -> ApiResult<()> {
    let status = response.status().as_u16();
    if status == 401 {
        return Err(ApiError::Unauthorized);
    }
    if !(200..300).contains(&status) {
        return Err(ApiError::Message(error_message(response)));
    }

    Ok(())
}

/// The server writes its own failures as `{"error": "..."}` (see `parseJsonBody`), so that
/// message is worth far more than the bare status code.
fn error_message(response: &mut Response<Body>) -> String {
    let status = response.status().as_u16();

    response
        .body_mut()
        .read_json::<FailureBody>()
        .map(|failure| failure.error)
        .unwrap_or_else(|_| format!("The server answered {status}."))
}

/// The `set-cookie` header reads `token=<value>; Path=/; HttpOnly; ...`; only the value is
/// wanted here, since the attributes describe browser behaviour a native client has none of.
fn session_token(response: &Response<Body>) -> Option<String> {
    let header = response.headers().get("set-cookie")?.to_str().ok()?;
    let pair = header.split(';').next()?.trim();
    let value = pair.strip_prefix(SESSION_COOKIE)?.strip_prefix('=')?.trim();

    (!value.is_empty()).then(|| value.to_owned())
}

/// Maps a transport failure (no connection, timeout, malformed answer) onto a message.
fn transport(error: ureq::Error) -> ApiError {
    ApiError::Message(format!("Could not reach the server: {error}"))
}

#[derive(Deserialize)]
struct MarksBody {
    marks: Vec<Mark>,
}

#[derive(Deserialize)]
struct MarkBody {
    mark: Mark,
}

#[derive(Deserialize)]
struct FailureBody {
    error: String,
}

/// The tests, in a file of their own: `api/tests.rs`, compiled only for test builds.
#[cfg(test)]
mod tests;
