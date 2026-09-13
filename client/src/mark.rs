use serde::Deserialize;

/// One saved mark, exactly the fields `/api/marks` returns (see `markSelection` on the
/// server). The favicon bytes are not part of it: those come from `/api/marks/<id>/icon`,
/// which is why only `icon_id` is here.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct Mark {
    pub id: String,
    /// Doubles as the bookmark's title.
    pub name: String,
    /// The URL the mark points at. The server derives the favicon from its hostname.
    pub content: String,
    /// Set when the server stored a favicon for this mark.
    pub icon_id: Option<String>,
}

impl Mark {
    /// The link this mark opens, or `None` when it is a plain note.
    pub fn link(&self) -> Option<url::Url> {
        web_link(&self.content)
    }
}

/// Parses `content` as a link a browser could open.
///
/// A bare host such as `github.com`, `localhost:5173` or `example.com:8080/path` is read as
/// `https://…`, the way an address bar reads it, but only when it really is a host: a plain
/// note like `milk` or `buy milk` must not quietly become a URL. The server applies the same
/// rule when it derives a favicon (`faviconHostname`), so a mark that opens is a mark that
/// gets an icon.
pub fn web_link(content: &str) -> Option<url::Url> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return None;
    }

    // A scheme that was written out is taken at its word, which leaves `file:`, `javascript:`
    // and `mailto:` as notes rather than handing them to the browser.
    if let Ok(url) = url::Url::parse(trimmed) {
        if is_web(&url) {
            return Some(url);
        }
    }

    if !looks_like_host(trimmed) {
        return None;
    }

    let candidate = url::Url::parse(&format!("https://{trimmed}")).ok()?;
    is_web(&candidate).then_some(candidate)
}

fn is_web(url: &url::Url) -> bool {
    matches!(url.scheme(), "http" | "https") && url.host_str().is_some()
}

/// Whether `text` begins with something that reads as a host rather than as a scheme.
///
/// The leading run before any `/`, `:` or `?` is what a host would be, and it has to carry a
/// dot or be `localhost`: that is what separates `example.com:8080` from `mailto:someone@example.com`.
fn looks_like_host(text: &str) -> bool {
    let host = text.split(['/', ':', '?', '#']).next().unwrap_or("");

    host.contains('.') || host == "localhost"
}
