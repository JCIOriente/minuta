use super::*;

// ---- select ----

fn sample() -> Catalog {
    let text = format!(
        "version = 1

[[template]]
name = \"instagram\"
description = \"d\"
platform = \"instagram\"
tags = [\"Social\", \"short\"]
sha256 = \"{}\"

[[template]]
name = \"youtube\"
description = \"d\"
platform = \"youtube\"
tags = [\"social\"]
sha256 = \"{}\"

[[template]]
name = \"podcast-notes\"
description = \"d\"
platform = \"audio\"
sha256 = \"{}\"
",
        a(),
        b(),
        c()
    );
    Catalog::parse(&text).unwrap()
}

fn names(entries: Vec<&Entry>) -> Vec<&str> {
    entries.into_iter().map(|e| e.name.as_str()).collect()
}

#[test]
fn select_all() {
    let catalog = sample();
    assert_eq!(
        names(select(&catalog, Selection::All).unwrap()),
        ["instagram", "youtube", "podcast-notes"]
    );
}

#[test]
fn select_by_names_keeps_given_order() {
    let catalog = sample();
    let wanted = ["youtube".to_string(), "instagram".to_string()];
    assert_eq!(
        names(select(&catalog, Selection::Names(&wanted)).unwrap()),
        ["youtube", "instagram"]
    );
}

#[test]
fn select_by_names_drops_repeats() {
    let catalog = sample();
    let wanted = ["youtube".to_string(), "youtube".to_string()];
    assert_eq!(
        names(select(&catalog, Selection::Names(&wanted)).unwrap()),
        ["youtube"]
    );
}

#[test]
fn select_unknown_name_lists_available() {
    let catalog = sample();
    let wanted = ["youtube".to_string(), "tiktok".to_string()];
    assert_eq!(
        usage(select(&catalog, Selection::Names(&wanted))),
        "unknown template \"tiktok\" (available: instagram, youtube, podcast-notes)"
    );
}

#[test]
fn select_unknown_name_with_control_characters_is_escaped() {
    let catalog = sample();
    let wanted = ["\u{1b}[2J".to_string()];
    assert_eq!(
        usage(select(&catalog, Selection::Names(&wanted))),
        "unknown template \"\\u{1b}[2J\" (available: instagram, youtube, podcast-notes)"
    );
}

#[test]
fn select_filter_matches_name() {
    let catalog = sample();
    assert_eq!(
        names(select(&catalog, Selection::Filter("podcast")).unwrap()),
        ["podcast-notes"]
    );
}

#[test]
fn select_filter_matches_platform_only() {
    let catalog = sample();
    assert_eq!(
        names(select(&catalog, Selection::Filter("audio")).unwrap()),
        ["podcast-notes"]
    );
}

#[test]
fn select_filter_matches_tag_only() {
    let catalog = sample();
    assert_eq!(
        names(select(&catalog, Selection::Filter("short")).unwrap()),
        ["instagram"]
    );
}

#[test]
fn select_filter_is_case_insensitive_on_both_sides() {
    let catalog = sample();
    assert_eq!(
        names(select(&catalog, Selection::Filter("SOCIAL")).unwrap()),
        ["instagram", "youtube"]
    );
}

#[test]
fn select_filter_without_match_is_usage_error() {
    let catalog = sample();
    assert_eq!(
        usage(select(&catalog, Selection::Filter("nothing"))),
        "no template matches filter \"nothing\""
    );
}

// ---- review follow-ups ----

#[test]
fn select_filter_blank_is_usage_error() {
    let catalog = sample();
    for blank in ["", " ", "\t\n"] {
        assert_eq!(
            usage(select(&catalog, Selection::Filter(blank))),
            "filter must not be empty",
            "filter {blank:?}"
        );
    }
}
