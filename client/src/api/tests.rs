//! The tests for `api`, against a server the test serves itself.

use super::*;
use crate::test_page::{serve_answer, serve_answers};
use std::time::Duration;

/// How long a test waits for a request to turn up on the server it started.
const PATIENCE: Duration = Duration::from_secs(5);

/// A JSON answer, with the headers a real one carries.
fn json(body: &str) -> (String, String) {
    (
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n".to_owned(),
        body.to_owned(),
    )
}

/// A sign-in as the server answers it: the session cookie, and the body that comes with it.
///
/// The cookie is spelled out the way the server spells it (see `SESSION_COOKIE_NAME` and the
/// `cookies.set` beside it), attributes and all, because reading the value out of that header
/// is the part worth testing.
fn signed_in(token: &str) -> (String, String) {
    (
        format!(
            "HTTP/1.1 200 OK\r\n\
             content-type: application/json\r\n\
             set-cookie: token={token}; Path=/; HttpOnly; SameSite=Lax\r\n"
        ),
        format!("{{\"session\":{{\"id\":\"{token}\",\"user_id\":\"user-1\"}}}}"),
    )
}

/// The request the server was asked with, once it has been asked.
fn request_of(server: &crate::test_page::Served) -> String {
    server
        .request
        .recv_timeout(PATIENCE)
        .expect("the request the server was sent")
        .to_lowercase()
}

#[test]
fn signing_in_keeps_the_token_and_not_the_header_it_came_in() {
    let server = serve_answers(vec![signed_in("abc123")]);
    let mut api = Api::new(server.url.as_str());

    api.login("otto", "supersecret").expect("a signed-in client");

    // The value alone: no cookie name, no attributes, and no trailing whitespace, because
    // this is what gets written to disk and handed back on the next run.
    assert_eq!(api.token(), Some("abc123"));
}

#[test]
fn the_session_travels_on_the_requests_that_follow() {
    let server = serve_answers(vec![signed_in("abc123"), json("{\"marks\":[]}")]);
    let mut api = Api::new(server.url.as_str());

    api.login("otto", "supersecret").expect("a signed-in client");
    assert!(api.list_marks().expect("the mark list").is_empty());

    // The sign-in request is the one without it; the one after is the one that proves the
    // session was kept.
    let sign_in = request_of(&server);
    assert!(!sign_in.contains("cookie:"), "{sign_in}");

    let listing = request_of(&server);
    assert!(listing.contains("cookie: token=abc123"), "{listing}");
}

#[test]
fn a_client_started_from_a_stored_token_uses_it_the_way_a_fresh_one_does() {
    // This is the shape a restored session takes: no sign-in call at all, the token in hand
    // from the start.
    let server = serve_answers(vec![json("{\"marks\":[]}")]);
    let api = Api::with_token(server.url.as_str(), "kept-from-last-time");

    api.list_marks().expect("the mark list");

    let request = request_of(&server);
    assert!(
        request.contains("cookie: token=kept-from-last-time"),
        "{request}"
    );
}

#[test]
fn a_sign_in_that_sets_no_session_is_an_error() {
    let server = serve_answer(
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n",
        "{\"session\":null}",
    );
    let mut api = Api::new(server.url.as_str());

    let error = api
        .login("otto", "supersecret")
        .expect_err("a sign-in with no session");

    assert!(error.to_string().contains("no session cookie"), "{error}");
    // Nothing to keep, so nothing is kept.
    assert_eq!(api.token(), None);
}

#[test]
fn a_rejected_sign_in_keeps_the_servers_own_wording() {
    let server = serve_answer(
        "HTTP/1.1 401 Unauthorized\r\ncontent-type: application/json\r\n",
        "{\"error\":\"Incorrect username or password\"}",
    );
    let mut api = Api::new(server.url.as_str());

    let error = api
        .login("otto", "wrong")
        .expect_err("a rejected sign-in");

    assert_eq!(error.to_string(), "Incorrect username or password");
    assert_eq!(api.token(), None);
}
