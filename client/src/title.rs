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

/// The tests, and the little HTTP server they read their pages from, in a file of their own:
/// `title/tests.rs`, compiled only for test builds.
#[cfg(test)]
mod tests;
