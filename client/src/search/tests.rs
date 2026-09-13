use super::*;

/// What `query` matched in `text`, written out as the characters themselves.
fn matched(text: &str, query: &str) -> Option<String> {
    let found = find(text, query)?;

    Some(picked(text, &found.indices))
}

/// The characters at `offsets`, as they would be shown.
fn picked(text: &str, offsets: &[usize]) -> String {
    offsets
        .iter()
        .map(|at| text[*at..].chars().next().unwrap())
        .collect()
}

#[test]
fn a_query_broken_into_words_finds_a_name_written_as_one() {
    // The whole point: "you ube" is how someone remembers youtube.com, not how it is spelled.
    assert_eq!(matched("youtube.com", "you ube").as_deref(), Some("youube"));
    assert_eq!(matched("youtube.com", "youube").as_deref(), Some("youube"));
    assert_eq!(matched("youtube.com", "ytb").as_deref(), Some("ytb"));
}

#[test]
fn the_characters_have_to_come_in_the_order_they_were_typed() {
    // A subsequence, not a bag of letters: everything in the query has to be there, in order.
    assert_eq!(matched("youtube.com", "ube you"), None);
    assert_eq!(matched("youtube.com", "euy"), None);
    // And every character of it has to be there at all.
    assert_eq!(matched("youtube.com", "youtube.com/watch"), None);
    assert_eq!(matched("buy milk", "youtube"), None);
}

#[test]
fn case_is_not_something_the_reader_has_to_remember() {
    // The capital T is skipped, as any character the query does not ask for is.
    assert_eq!(matched("YouTube", "you ube").as_deref(), Some("Youube"));
    assert_eq!(matched("youtube.com", "YOU UBE").as_deref(), Some("youube"));
    assert_eq!(matched("Café Zürich", "zür").as_deref(), Some("Zür"));
}

#[test]
fn a_space_in_the_query_is_not_something_to_find() {
    // Spaces break a query into words; they are not characters of the text being searched, so
    // they are dropped rather than looked for.
    assert_eq!(matched("Café Zürich", "fé Zü").as_deref(), Some("féZü"));
    assert_eq!(find("a b", " ").map(|found| found.indices), Some(Vec::new()));
}

#[test]
fn an_accent_is_a_letter_of_its_own() {
    // Case is folded, accents are not: "unicode" does not find "Ünïcödé". Folding those would
    // mean a table of every accented letter in every script, which is a larger thing than this
    // search is for — and the text being searched is a name that can be typed with the accent.
    assert!(find("Ünïcödé Ñämé", "unicode").is_none());
    assert_eq!(matched("Ünïcödé Ñämé", "nïcöd").as_deref(), Some("nïcöd"));
}

#[test]
fn the_offsets_are_where_the_characters_are() {
    // What the highlighting is built from, so they have to slice the text they came from —
    // including past the multi-byte characters of a title in another language.
    let text = "Café Zürich";
    let found = find(text, "fé Zü").expect("a match");

    assert_eq!(picked(text, &found.indices), "féZü");

    for at in found.indices {
        assert!(text.is_char_boundary(at), "{at} is not a character boundary");
    }
}

#[test]
fn a_query_with_nothing_in_it_matches_and_picks_nothing_out() {
    // An empty search box shows everything, and highlights nothing, which is the same answer as
    // a query of nothing but spaces.
    for query in ["", "   ", "\t"] {
        assert_eq!(find("youtube.com", query).map(|found| found.indices), Some(Vec::new()));
        assert_eq!(matched("youtube.com", query).as_deref(), Some(""));
    }
}

#[test]
fn text_that_is_empty_is_matched_by_nothing_but_an_empty_query() {
    assert_eq!(find("", "").map(|found| found.indices), Some(Vec::new()));
    assert!(find("", "anything").is_none());
}

#[test]
fn a_name_that_begins_with_the_query_scores_over_the_same_letters_scattered_through_a_link() {
    // What the window's ordering rests on. These are the two rows from the window: YouTube's own
    // name, against a news link whose address carries a y, an o and a u in three different words.
    let name = find("YouTube", "you").expect("the name matches").score;
    let link = find(
        "https://www.svt.se/nyheter/inrikes/jimmie-akessons-oro-muslimer-kan-avgora-valet",
        "you",
    )
    .expect("the link matches")
    .score;

    assert!(
        name > link,
        "the name scored {name} and the scattered link {link}"
    );
}

#[test]
fn every_character_of_a_match_is_a_character_of_the_query() {
    // A property worth holding across a range of awkward text: what is picked out is exactly the
    // query, and nothing else, whatever the shape of either.
    let cases = [
        ("youtube.com", "you ube"),
        ("https://github.com/emilk/egui", "github egui"),
        ("Café Zürich", "café"),
        ("Ünïcödé Ñämé", "Ñämé"),
    ];

    for (text, query) in cases {
        let picked = matched(text, query).unwrap_or_else(|| panic!("{query:?} did not find {text:?}"));
        let asked: String = query.chars().filter(|c| !c.is_whitespace()).collect();

        assert_eq!(
            picked.to_lowercase(),
            asked.to_lowercase(),
            "{query:?} in {text:?}"
        );
    }
}
