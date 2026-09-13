//! A page served from this machine, for the tests that have to fetch one.
//!
//! Tests that need a web page read it from here rather than from the internet: a title served
//! by the test itself is one the test can depend on, and one it can produce without a network
//! or a server of the project's own. Nothing outside a test build compiles this module.
//!
//! A hand-rolled socket rather than a crate, because the awkward cases these tests are about —
//! a status that is not a success, a missing `content-type`, a redirect, a body far past the
//! size cap — are the ones a convenience server makes easy to paper over.

/// A page served from this machine, and the request it was asked with.
pub(crate) struct Served {
    pub(crate) url: url::Url,
    /// What the client sent, for the tests that check what it says about itself. The server
    /// sends this once it has read the request, which is before it answers.
    pub(crate) request: std::sync::mpsc::Receiver<String>,
}

/// Serves one answer to one request on a port of the system's choosing.
///
/// A real socket rather than a canned string, so the parts of a fetch that are easy to get
/// wrong — the request it sends, the status, the content type, reading the body in chunks — are
/// the parts under test. `head` is the status line and any headers; the length and the closing
/// are added here so that no test has to count bytes.
///
/// It answers a single request and then stops, which is all most of these tests need. A test
/// that needs two hops (a redirect, say) starts two of these and points one at the other.
pub(crate) fn serve_answer(head: &str, body: &str) -> Served {
    serve_bodies(vec![(head.to_owned(), body.as_bytes().to_vec())])
}

/// Serves `body` as bytes rather than as text, for the answers that are not text at all — a
/// favicon, say, which has no business being a string and would be mangled by being made one.
pub(crate) fn serve_bytes(head: &str, body: &[u8]) -> Served {
    serve_bodies(vec![(head.to_owned(), body.to_vec())])
}

/// Serves one answer per request, in the order they are given, on a single port.
///
/// Needed by anything that takes two requests to say what it means: signing in is a POST that
/// sets the session cookie followed by the call that uses it, and only the second of those
/// shows whether the first was understood.
///
/// Every answer is written on a connection of its own, so a client that keeps connections
/// alive is not what these tests are measuring.
pub(crate) fn serve_answers(answers: Vec<(String, String)>) -> Served {
    serve_bodies(
        answers
            .into_iter()
            .map(|(head, body)| (head, body.into_bytes()))
            .collect(),
    )
}

/// The work behind all of them: one answer per request, written as bytes.
fn serve_bodies(answers: Vec<(String, Vec<u8>)>) -> Served {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::thread;

    let listener = TcpListener::bind("127.0.0.1:0").expect("a free port");
    let port = listener.local_addr().expect("an address").port();

    let (requests, request) = mpsc::channel();

    thread::spawn(move || {
        for (head, body) in answers {
            let Ok((mut stream, _)) = listener.accept() else {
                // The test is over and the listener is gone.
                return;
            };

            // Read before writing: a client still sending when the reply arrives is a client
            // that may never see it. Read to the blank line rather than once, because a
            // request that arrives in two packets would otherwise be recorded half-written.
            let mut sent = Vec::new();
            let mut buffer = [0u8; 512];

            while sent.len() < 4096 {
                match stream.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(read) => {
                        sent.extend_from_slice(&buffer[..read]);

                        if sent.windows(4).any(|window| window == b"\r\n\r\n") {
                            break;
                        }
                    }
                }
            }

            let _ = requests.send(String::from_utf8_lossy(&sent).into_owned());

            let head = format!(
                "{head}content-length: {}\r\nconnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&body);
            let _ = stream.flush();
        }
    });

    Served {
        url: url::Url::parse(&format!("http://127.0.0.1:{port}/")).expect("a url"),
        request,
    }
}

/// Serves `body` as `content_type` with a `200 OK`.
pub(crate) fn serve(content_type: &str, body: &str) -> url::Url {
    serve_answer(
        &format!("HTTP/1.1 200 OK\r\ncontent-type: {content_type}\r\n"),
        body,
    )
    .url
}
