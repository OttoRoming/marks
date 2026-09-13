//! Finding a mark by typing at it.
//!
//! The matching itself is [`fuzzy_matcher`]'s: skim's matcher, which is the one worth having here
//! because it reports *where* a pattern matched as well as how well, and the window both picks
//! those characters out of the row and orders the list by that score. This module is the thin
//! adapter between that matcher and what the rest of the window wants to know — it does no
//! matching of its own, and exists so that the answer is worked out in one place, so that
//! filtering, ordering and highlighting cannot disagree about why a row is in the list.

use std::sync::OnceLock;

use fuzzy_matcher::FuzzyMatcher as _;
use fuzzy_matcher::skim::SkimMatcherV2;

/// What the matcher found, and how good it was.
pub struct Found {
    /// How well the query matched, higher being better.
    ///
    /// Only comparable with other scores for the same query, which is all it is used for: it is
    /// what puts the row that answers the query best at the top of the list.
    pub score: i64,

    /// The characters the query matched, as byte offsets into the text they were found in.
    ///
    /// In order with nothing repeated: the highlighting slices the text with them, and a
    /// position out of order would be a slice backwards.
    pub indices: Vec<usize>,
}

/// The best match `query` finds in `text`, or `None` when it finds none.
///
/// `Some` with no offsets for a query with nothing in it, which matches everything and picks
/// nothing out.
pub fn find(text: &str, query: &str) -> Option<Found> {
    // A space is how a query is broken into the words someone remembers, not a character to
    // find: the matcher treats it as one, which is the only thing about it that is wrong for a
    // search box — "you ube" is how youtube.com gets typed.
    let query: String = query.chars().filter(|c| !c.is_whitespace()).collect();

    let (score, hits) = matcher().fuzzy_indices(text, &query)?;

    Some(Found {
        score,
        indices: character_offsets(text, &hits),
    })
}

/// The matcher, built once and shared.
///
/// It keeps a scratch buffer per thread for the matrix it scores with, which is worth holding on
/// to rather than starting again for every mark in the list.
fn matcher() -> &'static SkimMatcherV2 {
    static MATCHER: OnceLock<SkimMatcherV2> = OnceLock::new();

    MATCHER.get_or_init(|| {
        // Case is never something a search box is asked about, and the matcher's own default is
        // to make a query with a capital in it case-sensitive — under which "SVELTE" would stop
        // finding "Svelte", which is not what someone who left caps lock on is asking for.
        SkimMatcherV2::default().ignore_case()
    })
}

/// Where the characters the matcher picked out are, in bytes.
///
/// The matcher counts characters, and a string is indexed by bytes: the two part company at the
/// first character that is not one byte long, which a page's title is quite likely to have.
fn character_offsets(text: &str, hits: &[usize]) -> Vec<usize> {
    let starts: Vec<usize> = text.char_indices().map(|(offset, _)| offset).collect();

    let mut offsets: Vec<usize> = hits
        .iter()
        .filter_map(|hit| starts.get(*hit).copied())
        .collect();

    offsets.sort_unstable();
    offsets.dedup();

    offsets
}

/// The tests, in a file of their own: `search/tests.rs`, compiled only for test builds.
#[cfg(test)]
mod tests;
