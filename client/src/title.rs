use std::io::Read;
use std::time::Duration;

use ureq::Agent;

/// How long a page has to answer before the mark is saved under its fallback name.
///
/// Only a save waits on this, and the user is watching the window while it does, so it is
/// shorter than the client's own request timeout: a page slower than this is not worth a title.
const TIMEOUT: Duration = Duration::from_secs(5);

/// How much of a page is read while looking for a title.
///
/// A title lives in the head, so this is far more than any real page needs; the cap is here
/// because the URL came from a search box and could point at anything at all.
const MAX_HTML_BYTES: u64 = 512 * 1024;

/// How much is read at a time before the closing tag is looked for again.
const CHUNK_BYTES: usize = 8 * 1024;

/// The longest character reference worth considering: the longest name in the HTML table is 31
/// characters, and a numeric one is shorter still.
const MAX_ENTITY_CHARS: usize = 32;

/// How the client introduces itself.
///
/// Named rather than left to ureq's default, because a site that answers a browser may answer
/// an unfamiliar agent with a 403 or with a challenge page — which has a title, but not the
/// page's own, and is why the title is read from the document only when the request succeeded.
const USER_AGENT: &str = concat!("marks-client/", env!("CARGO_PKG_VERSION"));

/// The title of the page at `url`, or `None` when there is not one to be had.
///
/// This never fails loudly. Every way it can go wrong — no route to the host, a slow answer, a
/// status that is not a success, a body that is not HTML, a document that names no title —
/// is a `None`, and the caller saves the mark under the name it would have had anyway. A
/// bookmark is worth keeping whether or not its page can be reached from here.
pub fn fetch_title(url: &url::Url) -> Option<String> {
    // The status is read below rather than turned into an error by ureq, so that a 404 page
    // is simply a page without a title instead of a failed request.
    let config = Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(TIMEOUT))
        .build();

    let mut response = Agent::new_with_config(config)
        .get(url.as_str())
        .header("user-agent", USER_AGENT)
        // Asked for by name: a server that content-negotiates could otherwise answer with
        // something smaller and less useful than the document.
        .header("accept", "text/html,application/xhtml+xml")
        .call()
        .ok()?;

    if !response.status().is_success() {
        return None;
    }

    // An image or a PDF has no title, and there is nothing to gain by reading one. A response
    // that names no type is read anyway: plenty of small sites omit the header, and a title
    // that is not really HTML simply fails to parse below.
    if let Some(content_type) = response.headers().get("content-type") {
        let content_type = content_type.to_str().unwrap_or_default().to_ascii_lowercase();

        if !content_type.contains("html") {
            return None;
        }
    }

    extract_title(&read_head(response.body_mut()))
}

/// Reads as much of a response body as it takes to find a title, and no more.
///
/// Stopping as soon as `</title` has been read is what keeps a large page from being
/// downloaded in full for the sake of its first kilobyte. Both the cap and the tolerated read
/// error cover the same case — a document that never closes its title, or is larger than
/// [`MAX_HTML_BYTES`] — where the bytes gathered so far are still worth trying to parse.
fn read_head(body: &mut ureq::Body) -> String {
    let mut reader = body.with_config().limit(MAX_HTML_BYTES).reader();
    let mut bytes = Vec::new();
    let mut chunk = [0u8; CHUNK_BYTES];

    loop {
        match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => {
                bytes.extend_from_slice(&chunk[..read]);

                // Searched over everything read so far, so a closing tag split across two
                // chunks is found by the next pass rather than missed.
                if find_ci(&bytes, b"</title", 0).is_some() {
                    break;
                }
            }
            Err(_) => break,
        }
    }

    decode_html(&bytes)
}

/// Decodes a document's bytes as text.
///
/// The `Content-Type` header is not consulted for this, since it is left out as often as it is
/// wrong. UTF-8 is tried first because it is what the web is written in; a document that is
/// not UTF-8 is then read as Latin-1, which is lossless for the single-byte encodings that
/// came before it and right for every ASCII byte in between.
fn decode_html(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_owned(),
        Err(_) => bytes.iter().map(|byte| char::from(*byte)).collect(),
    }
}

/// The `<title>` of an HTML document, tidied into something worth using as a mark's name.
///
/// This scans the document rather than parsing it: a whole HTML parser would be a large
/// dependency for the single element wanted here. The two ways a plain search for `<title`
/// would go wrong — the tag named in a comment, or the string appearing in a script — are
/// stepped over explicitly, and anything the scan cannot make sense of answers `None`, which
/// leaves the caller its fallback name.
fn extract_title(html: &str) -> Option<String> {
    let bytes = html.as_bytes();
    let mut index = 0;

    while let Some(open) = find_ci(bytes, b"<", index) {
        // A comment is not markup, and may well mention a title in passing.
        if starts_with_ci(bytes, open, b"<!--") {
            // Unterminated comments leave no markup behind them that could be trusted.
            index = find_ci(bytes, b"-->", open)? + 3;
            continue;
        }

        let Some((name, name_end)) = tag_name_at(bytes, open) else {
            index = open + 1;
            continue;
        };

        // A script or a style holds text rather than elements, and that text may contain
        // anything — including the string `<title>`.
        if name.eq_ignore_ascii_case(b"script") || name.eq_ignore_ascii_case(b"style") {
            index = skip_element(bytes, name, name_end);
            continue;
        }

        if name.eq_ignore_ascii_case(b"title") {
            // The text runs from just past this tag to the closing one. A title that is never
            // closed ends the search: what follows is not known to be a title at all.
            let text_start = tag_end(bytes, name_end)? + 1;
            let end = find_ci(bytes, b"</title", text_start)?;

            return tidy(&html[text_start..end]);
        }

        index = open + 1;
    }

    None
}

/// The tag name at `open`, with the index just past it, when `<` really opens a tag.
///
/// The name is handed back as bytes and compared case-insensitively by the caller: HTML tag
/// names are ASCII, and staying in bytes keeps every index usable as a string slice.
fn tag_name_at(bytes: &[u8], open: usize) -> Option<(&[u8], usize)> {
    if bytes.get(open) != Some(&b'<') {
        return None;
    }

    let mut index = open + 1;
    if bytes.get(index) == Some(&b'/') {
        index += 1;
    }

    let start = index;
    while bytes
        .get(index)
        .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'-')
    {
        index += 1;
    }

    (index > start).then(|| (&bytes[start..index], index))
}

/// The index of the `>` that closes the tag at or after `from`.
///
/// Quotes are respected, because an attribute value may contain one of these itself, as in
/// `<title data-note="a > b">`.
fn tag_end(bytes: &[u8], from: usize) -> Option<usize> {
    let mut quote = None;

    for (index, byte) in bytes.iter().enumerate().skip(from) {
        let byte = *byte;

        match quote {
            // Inside an attribute value: only the quote that opened it can close it.
            Some(open) if byte == open => quote = None,
            Some(_) => {}
            None if byte == b'"' || byte == b'\'' => quote = Some(byte),
            None if byte == b'>' => return Some(index),
            None => {}
        }
    }

    None
}

/// The index to carry on from after a text-holding element such as `<script>`.
fn skip_element(bytes: &[u8], name: &[u8], from: usize) -> usize {
    let mut closing = Vec::with_capacity(name.len() + 2);
    closing.extend_from_slice(b"</");
    closing.extend_from_slice(name);

    match find_ci(bytes, &closing, from) {
        Some(found) => tag_end(bytes, found).map_or(bytes.len(), |end| end + 1),
        // Never closed: skips the rest, which is the safe reading of a document like that.
        None => bytes.len(),
    }
}

/// The first index at or after `from` where `needle` appears, ignoring ASCII case.
fn find_ci(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    (from..haystack.len()).find(|index| starts_with_ci(haystack, *index, needle))
}

/// Whether `haystack` holds `needle` at `index`, ignoring ASCII case.
///
/// ASCII-insensitively rather than lowercasing the document first: lowercasing can change a
/// string's length, which would leave every index found in it pointing at the wrong place.
fn starts_with_ci(haystack: &[u8], index: usize, needle: &[u8]) -> bool {
    haystack.len() >= index + needle.len()
        && haystack[index..index + needle.len()].eq_ignore_ascii_case(needle)
}

/// A raw title as it would be read: references resolved, and runs of whitespace — a `<title>`
/// in a source file is often indented over several lines — collapsed to single spaces.
///
/// `None` for a title that is empty or is nothing but whitespace, which says no more about a
/// page than no title at all does.
fn tidy(raw: &str) -> Option<String> {
    let collapsed = decode_entities(raw)
        .chars()
        // Any Unicode whitespace, which includes the non-breaking space that `&nbsp;` and
        // `&#160;` decode to: a name is not the place to preserve one.
        .map(|character| if character.is_whitespace() { ' ' } else { character })
        .collect::<String>()
        .split(' ')
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ");

    (!collapsed.is_empty()).then_some(collapsed)
}

/// Resolves the character references in `text`.
///
/// Only the references that turn up in titles are known by name: the full HTML table runs to
/// some two thousand entries, which is more than a module like this should carry. One that is
/// not known is left exactly as it was written, so `&notanentity;` is not quietly eaten.
fn decode_entities(text: &str) -> String {
    let mut decoded = String::with_capacity(text.len());
    let mut rest = text;

    while let Some(ampersand) = rest.find('&') {
        decoded.push_str(&rest[..ampersand]);
        rest = &rest[ampersand..];

        match entity(rest) {
            Some((replacement, length)) => {
                decoded.push_str(&replacement);
                rest = &rest[length..];
            }
            None => {
                // Not a reference after all: this `&` is part of the text.
                decoded.push('&');
                rest = &rest[1..];
            }
        }
    }

    decoded.push_str(rest);

    decoded
}

/// The character reference at the start of `text`, which begins with `&`, and its length.
///
/// `None` means the `&` is not a reference and should be written out as itself.
fn entity(text: &str) -> Option<(String, usize)> {
    // Bounded, because the `;` that ends a reference is not far from its `&`; searching
    // further would only find the punctuation of some later sentence.
    let end = text.find(';').filter(|end| *end <= MAX_ENTITY_CHARS)?;
    let body = &text[1..end];
    let length = end + 1;

    if let Some(number) = body.strip_prefix('#') {
        let code = match number.strip_prefix(['x', 'X']) {
            Some(hexadecimal) => u32::from_str_radix(hexadecimal, 16).ok()?,
            None => number.parse().ok()?,
        };

        // A control character is not something to put in a name, and an unpaired surrogate is
        // not a character at all. Both leave the reference as it was written.
        return char::from_u32(code)
            .filter(|character| !character.is_control())
            .map(|character| (character.to_string(), length));
    }

    NAMED_ENTITIES
        .iter()
        .find(|(name, _)| *name == body)
        .map(|(_, replacement)| ((*replacement).to_owned(), length))
}

/// The named references a title is likely to hold: punctuation a page writes as an entity
/// because it is not on the keyboard, plus the handful of symbols that go with it.
///
/// More can be added as they turn up; anything missing stays as written, which is visible
/// rather than wrong.
const NAMED_ENTITIES: &[(&str, &str)] = &[
    // The five that are entities whether or not anything needs them to be.
    ("amp", "&"),
    ("lt", "<"),
    ("gt", ">"),
    ("quot", "\""),
    ("apos", "'"),
    // Spaces, all read as the ordinary space they are meant to look like.
    ("nbsp", " "),
    ("ensp", " "),
    ("emsp", " "),
    ("thinsp", " "),
    // Dashes and ellipses, which titles are full of.
    ("ndash", "–"),
    ("mdash", "—"),
    ("horbar", "―"),
    ("minus", "−"),
    ("hellip", "…"),
    ("middot", "·"),
    ("bull", "•"),
    ("prime", "′"),
    ("Prime", "″"),
    // Quotation marks and brackets.
    ("lsquo", "‘"),
    ("rsquo", "’"),
    ("sbquo", "‚"),
    ("ldquo", "“"),
    ("rdquo", "”"),
    ("bdquo", "„"),
    ("laquo", "«"),
    ("raquo", "»"),
    ("lsaquo", "‹"),
    ("rsaquo", "›"),
    ("lpar", "("),
    ("rpar", ")"),
    // Marks that sit next to a word.
    ("copy", "©"),
    ("reg", "®"),
    ("trade", "™"),
    ("deg", "°"),
    ("plusmn", "±"),
    ("times", "×"),
    ("divide", "÷"),
    ("micro", "µ"),
    ("para", "¶"),
    ("sect", "§"),
    ("dagger", "†"),
    ("Dagger", "‡"),
    ("permil", "‰"),
    ("frac12", "½"),
    ("frac14", "¼"),
    ("frac34", "¾"),
    ("sup2", "²"),
    ("sup3", "³"),
    ("sup1", "¹"),
    ("iquest", "¿"),
    ("iexcl", "¡"),
    ("brvbar", "¦"),
    // Money.
    ("euro", "€"),
    ("pound", "£"),
    ("yen", "¥"),
    ("cent", "¢"),
    ("curren", "¤"),
    // Arrows, which a title uses to point somewhere.
    ("larr", "←"),
    ("uarr", "↑"),
    ("rarr", "→"),
    ("darr", "↓"),
    ("harr", "↔"),
];

/// Serves `body` as `content_type` to the first request on a port of the system's choosing, and
/// hands back the URL it can be read from.
///
/// A real socket rather than a canned string, so the parts of a fetch that are easy to get
/// wrong — the status, the content type, reading the body in chunks — are the parts under test.
/// It answers one request and then stops.
///
/// Shared with `app`'s tests, which save a mark pointed at a page served from here: a title
/// that comes from this machine is one a test can depend on, unlike one off the internet.
#[cfg(test)]
pub(crate) fn serve(content_type: &str, body: &str) -> url::Url {
    use std::io::Write;
    use std::net::TcpListener;
    use std::thread;

    let listener = TcpListener::bind("127.0.0.1:0").expect("a free port");
    let port = listener.local_addr().expect("an address").port();

    let response = format!(
        "HTTP/1.1 200 OK\r\n\
         content-type: {content_type}\r\n\
         content-length: {}\r\n\
         connection: close\r\n\
         \r\n\
         {body}",
        body.len()
    );

    thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            // The request is read before the answer is written: a client still sending when
            // the reply arrives is a client that may never see it.
            let mut request = [0u8; 1024];
            let _ = std::io::Read::read(&mut stream, &mut request);
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }
    });

    url::Url::parse(&format!("http://127.0.0.1:{port}/")).expect("a url")
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn title_of(html: &str) -> Option<String> {
        extract_title(html)
    }

    #[test]
    fn a_title_is_read_and_its_layout_whitespace_collapsed() {
        assert_eq!(title_of(PAGE).as_deref(), Some("Example Domain"));
    }

    #[test]
    fn a_page_served_over_http_is_fetched_and_named() {
        let url = serve("text/html; charset=utf-8", PAGE);

        assert_eq!(fetch_title(&url).as_deref(), Some("Example Domain"));
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
        let url = serve("text/html", &format!("{padding}<title>Far too late</title>"));

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
}
