//! The tests for `title`, against pages served from this machine: none of them needs the internet.

use super::*;
use crate::test_page::{serve, serve_answer};
use std::time::Duration;

/// A page with a title, in the shape a real one arrives in: a doctype, a head, and the
/// title indented over its own lines.
const PAGE: &str = "\
<!DOCTYPE html>
<html lang=\"en\">
  <head>
    <meta charset=\"utf-8\">
    <title>
      Example Domain
    </title>
  </head>
</html>";

/// A document served as HTML from this machine, which is what every fetching test reads.
fn html(body: &str) -> url::Url {
    serve("text/html; charset=utf-8", body)
}

fn title_of(document: &str) -> Option<String> {
    extract_title(document)
}

#[test]
fn a_title_is_read_and_its_layout_whitespace_collapsed() {
    assert_eq!(title_of(PAGE).as_deref(), Some("Example Domain"));
}

#[test]
fn a_page_served_over_http_is_fetched_and_named() {
    // The whole way round: a real request over a real socket, the document, its title.
    assert_eq!(fetch_title(&html(PAGE)).as_deref(), Some("Example Domain"));
}

#[test]
fn a_page_is_named_after_its_title_and_not_its_host() {
    // The point of the feature, on a title unlike any hostname: entities resolved and
    // layout whitespace collapsed on the way through, not just in `extract_title`.
    let url = html("<head>\n  <title>Otto&rsquo;s &amp; Marks</title>\n</head>");

    assert_eq!(fetch_title(&url).as_deref(), Some("Otto’s & Marks"));
    assert_ne!(url.host_str(), Some("Otto’s & Marks"));
}

#[test]
fn a_page_that_omits_its_content_type_is_still_read() {
    // Plenty of small sites send no type at all, and a title is not worth refusing one for.
    let url = serve_answer("HTTP/1.1 200 OK\r\n", "<title>Untyped</title>").url;

    assert_eq!(fetch_title(&url).as_deref(), Some("Untyped"));
}

#[test]
fn a_page_that_is_not_there_has_no_title() {
    // A 404 answers with a body of its own, and a real one usually has a title — the
    // status is what keeps the error page's title from being taken for the page's.
    let url = serve_answer(
        "HTTP/1.1 404 Not Found\r\ncontent-type: text/html\r\n",
        "<title>Not found</title>",
    )
    .url;

    assert_eq!(fetch_title(&url), None);
}

#[test]
fn a_redirect_is_followed_to_the_page_that_names_itself() {
    // Typed links redirect as a matter of course: a bare host reaches the site, which
    // sends the browser on to `www.` or to https. The title has to come from the end.
    let destination = html("<title>Arrived</title>");
    let redirect = serve_answer(
        &format!("HTTP/1.1 302 Found\r\nlocation: {destination}\r\n"),
        "",
    )
    .url;

    assert_eq!(fetch_title(&redirect).as_deref(), Some("Arrived"));
}

#[test]
fn the_client_says_who_it_is_and_what_it_asked_for() {
    let served = serve_answer(
        "HTTP/1.1 200 OK\r\ncontent-type: text/html\r\n",
        "<title>Anything</title>",
    );

    let _ = fetch_title(&served.url);

    let request = served
        .request
        .recv_timeout(Duration::from_secs(5))
        .expect("the request the server was sent")
        .to_lowercase();

    // A site that answers a browser may answer an unnamed agent with a challenge page
    // instead of the document, so both of these are deliberate.
    assert!(request.starts_with("get / http/1.1"), "{request}");
    assert!(
        request.contains(&format!("user-agent: {}", USER_AGENT.to_lowercase())),
        "{request}"
    );
    assert!(request.contains("accept: text/html"), "{request}");
}

#[test]
fn a_response_that_is_not_html_is_not_read_for_a_title() {
    // A PDF, an image or a JSON API has no title, and the document under it is not
    // something to look for a `<title>` in.
    let url = serve("application/pdf", "<title>Not a title</title>");

    assert_eq!(fetch_title(&url), None);
}

#[test]
fn a_title_beyond_the_size_cap_is_given_up_on() {
    // The cap is what keeps a mistyped link to something enormous from being read to its
    // end: a title this far in is not one, and the fetch stops rather than growing.
    let padding = "x".repeat(MAX_HTML_BYTES as usize + 1024);
    let url = html(&format!("{padding}<title>Far too late</title>"));

    assert_eq!(fetch_title(&url), None);
}

#[test]
fn a_title_is_matched_whatever_its_case_or_attributes() {
    assert_eq!(title_of("<TITLE>Shouty</TITLE>").as_deref(), Some("Shouty"));
    assert_eq!(
        title_of(r#"<title lang="en" data-note="a > b">Attributed</title>"#).as_deref(),
        Some("Attributed")
    );
    // A longer name is a different element, not a title.
    assert_eq!(title_of("<titlex>No</titlex>"), None);
}

#[test]
fn a_title_named_in_a_comment_or_a_script_is_not_the_page_title() {
    // The string in the comment is stepped over, so the real title is the one found.
    assert_eq!(
        title_of("<!-- <title>Old name</title> --><title>Real</title>").as_deref(),
        Some("Real")
    );
    // Same for a script, whose text may contain anything at all.
    assert_eq!(
        title_of(r#"<script>var t = "<title>Not this</title>";</script><title>Real</title>"#)
            .as_deref(),
        Some("Real")
    );
    assert_eq!(
        title_of("<style>/* <title>Not this</title> */</style><title>Real</title>").as_deref(),
        Some("Real")
    );
}

#[test]
fn a_document_without_a_usable_title_answers_nothing() {
    assert_eq!(title_of("<html><body>No head here</body></html>"), None);
    assert_eq!(title_of("<title></title>"), None);
    assert_eq!(title_of("<title>   \n  </title>"), None);
    // Never closed: what follows is not known to be a title.
    assert_eq!(title_of("<title>Cut off before the end"), None);
    assert_eq!(title_of(""), None);
}

#[test]
fn entity_references_are_resolved() {
    assert_eq!(
        title_of("<title>Tom &amp; Jerry</title>").as_deref(),
        Some("Tom & Jerry")
    );
    assert_eq!(
        title_of("<title>&ldquo;Quoted&rdquo;</title>").as_deref(),
        Some("\u{201c}Quoted\u{201d}")
    );
    // Numbers, in both bases, and their uppercase and lowercase spellings.
    assert_eq!(
        title_of("<title>It&#39;s here&#x2014;now</title>").as_deref(),
        Some("It's here\u{2014}now")
    );
    // A non-breaking space is collapsed like any other space.
    assert_eq!(
        title_of("<title>Marks&nbsp;&ndash; a launcher</title>").as_deref(),
        Some("Marks – a launcher")
    );
}

#[test]
fn an_ampersand_that_is_not_a_reference_is_left_alone() {
    assert_eq!(
        title_of("<title>Marks & Marks</title>").as_deref(),
        Some("Marks & Marks")
    );
    // Unknown names, unterminated references and nonsense numbers all survive as written
    // rather than vanishing or turning into something wrong.
    assert_eq!(
        title_of("<title>&notanentity; &amp &&#; &#xzz;</title>").as_deref(),
        Some("&notanentity; &amp &&#; &#xzz;")
    );
}

#[test]
fn a_title_that_is_not_utf8_is_still_read() {
    // A page in a single-byte encoding: Latin-1 "é" is one byte that is not valid UTF-8.
    let page = b"<head><title>Caf\xe9</title></head>";

    assert!(decode_html(page).contains("Caf\u{e9}"));
}

#[test]
fn a_link_that_cannot_be_reached_has_no_title() {
    // Port 1 on loopback is refused immediately, so this stays a fast, offline test of the
    // one path that matters most: a fetch that fails names nothing, and the caller keeps
    // its fallback.
    let url = url::Url::parse("http://127.0.0.1:1/").expect("a valid url");

    assert_eq!(fetch_title(&url), None);
}
