use minuta::error::{Error, Result};
use minuta::template::catalog::{
    select, sha256_hex, state, Catalog, Entry, Manifest, Selection, State,
};

fn a() -> String {
    "a".repeat(64)
}

fn b() -> String {
    "b".repeat(64)
}

fn c() -> String {
    "c".repeat(64)
}

fn catalog_text(entries: &str) -> String {
    format!("version = 1\n{entries}")
}

fn entry_text(name: &str, sha: &str) -> String {
    format!("\n[[template]]\nname = \"{name}\"\ndescription = \"d\"\nsha256 = \"{sha}\"\n")
}

fn parse_err(text: &str) -> String {
    match Catalog::parse(text) {
        Err(Error::Parse(m)) => m,
        other => panic!("expected parse error, got {other:?}"),
    }
}

fn usage(r: Result<impl std::fmt::Debug>) -> String {
    match r {
        Err(Error::Usage(m)) => m,
        other => panic!("expected usage error, got {other:?}"),
    }
}

// ---- catalog ----

#[test]
fn catalog_parses_valid() {
    let text = format!(
        "version = 1\n\n[[template]]\nname = \"instagram\"\ndescription = \"Reel caption\"\nplatform = \"instagram\"\ntags = [\"social\", \"short\"]\ninput = \"social -> summary -> transcript\"\nsha256 = \"{}\"\n\n[[template]]\nname = \"summary-long\"\ndescription = \"Long summary\"\nsha256 = \"{}\"\n",
        a(),
        b()
    );
    let catalog = Catalog::parse(&text).unwrap();
    assert_eq!(
        catalog.entries,
        vec![
            Entry {
                name: "instagram".into(),
                description: "Reel caption".into(),
                platform: Some("instagram".into()),
                tags: vec!["social".into(), "short".into()],
                input: Some("social -> summary -> transcript".into()),
                sha256: a(),
            },
            Entry {
                name: "summary-long".into(),
                description: "Long summary".into(),
                platform: None,
                tags: vec![],
                input: None,
                sha256: b(),
            },
        ]
    );
}

#[test]
fn catalog_empty_is_valid() {
    let catalog = Catalog::parse("version = 1\n").unwrap();
    assert_eq!(catalog.entries, vec![]);
}

#[test]
fn catalog_rejects_invalid_toml() {
    let m = parse_err("version = [");
    assert!(m.starts_with("template catalog: "), "{m}");
}

#[test]
fn catalog_rejects_missing_version() {
    let m = parse_err(&entry_text("a", &a()));
    assert!(m.starts_with("template catalog: "), "{m}");
    assert!(m.contains("missing field `version`"), "{m}");
}

#[test]
fn catalog_rejects_bad_version() {
    assert_eq!(
        parse_err("version = 2\n"),
        "template catalog: unsupported version 2 (this minuta reads version 1; upgrade minuta)"
    );
}

#[test]
fn catalog_rejects_version_zero() {
    assert_eq!(
        parse_err("version = 0\n"),
        "template catalog: unsupported version 0 (this minuta reads version 1; upgrade minuta)"
    );
}

#[test]
fn catalog_rejects_bad_name() {
    for name in ["Upper", "-lead", "has space", "dot.md", "a/b", ""] {
        let m = parse_err(&catalog_text(&entry_text(name, &a())));
        assert_eq!(
            m,
            format!("template catalog: invalid template name {name:?} (use lowercase letters, digits, '-' and '_', starting with a letter or digit)"),
            "name {name:?}"
        );
    }
}

#[test]
fn catalog_bad_name_with_control_characters_is_escaped() {
    let m = parse_err(&catalog_text(&entry_text("x\\u001b[2Jy", &a())));
    assert_eq!(
        m,
        "template catalog: invalid template name \"x\\u{1b}[2Jy\" (use lowercase letters, digits, '-' and '_', starting with a letter or digit)"
    );
}

#[test]
fn catalog_rejects_duplicate_name() {
    let text = catalog_text(&format!(
        "{}{}",
        entry_text("same", &a()),
        entry_text("same", &b())
    ));
    assert_eq!(
        parse_err(&text),
        "template catalog: duplicate template \"same\""
    );
}

#[test]
fn catalog_rejects_bad_sha() {
    let cases = [
        "short".to_string(),
        "A".repeat(64),
        "g".repeat(64),
        "a".repeat(63),
        "a".repeat(65),
        String::new(),
    ];
    for sha in cases {
        let m = parse_err(&catalog_text(&entry_text("ok", &sha)));
        assert_eq!(
            m,
            "template catalog: template \"ok\" has an invalid sha256 (expected 64 lowercase hex characters)",
            "sha {sha:?}"
        );
    }
}

#[test]
fn catalog_rejects_unknown_key() {
    let text = catalog_text(&format!("{}colour = \"red\"\n", entry_text("ok", &a())));
    let m = parse_err(&text);
    assert!(m.starts_with("template catalog: "), "{m}");
    assert!(m.contains("unknown field `colour`"), "{m}");
}

#[test]
fn catalog_rejects_unknown_top_level_key() {
    let m = parse_err("version = 1\nmirror = \"x\"\n");
    assert!(m.contains("unknown field `mirror`"), "{m}");
}

#[test]
fn catalog_requires_description() {
    let text = format!(
        "version = 1\n[[template]]\nname = \"ok\"\nsha256 = \"{}\"\n",
        a()
    );
    let m = parse_err(&text);
    assert!(m.contains("missing field `description`"), "{m}");
}

// ---- manifest ----

#[test]
fn manifest_round_trip() {
    let mut manifest = Manifest::default();
    manifest.set("youtube", &b());
    manifest.set("instagram", &a());
    let text = manifest.to_toml();
    let (parsed, warning) = Manifest::parse(&text);
    assert_eq!(warning, None);
    assert_eq!(parsed, manifest);
    assert_eq!(parsed.get("instagram"), Some(a().as_str()));
    assert_eq!(parsed.get("youtube"), Some(b().as_str()));
    assert_eq!(parsed.get("missing"), None);
}

#[test]
fn manifest_to_toml_is_sorted_and_exact() {
    let mut manifest = Manifest::default();
    manifest.set("youtube", &b());
    manifest.set("instagram", &a());
    assert_eq!(
        manifest.to_toml(),
        format!(
            "version = 1\n\n[[file]]\nname = \"instagram\"\nsha256 = \"{}\"\n\n[[file]]\nname = \"youtube\"\nsha256 = \"{}\"\n",
            a(),
            b()
        )
    );
}

#[test]
fn manifest_set_replaces_existing_entry() {
    let mut manifest = Manifest::default();
    manifest.set("instagram", &a());
    manifest.set("instagram", &b());
    assert_eq!(manifest.get("instagram"), Some(b().as_str()));
}

#[test]
fn manifest_empty_text_is_empty_without_warning() {
    let (manifest, warning) = Manifest::parse("");
    assert_eq!(manifest, Manifest::default());
    assert_eq!(warning, None);
}

const MANIFEST_WARNING: &str = "invalid manifest, treating every file as modified: ";

fn invalid_manifest(text: &str) -> String {
    let (manifest, warning) = Manifest::parse(text);
    assert_eq!(manifest, Manifest::default(), "text {text:?}");
    warning.unwrap_or_else(|| panic!("no warning for {text:?}"))
}

#[test]
fn manifest_invalid_toml_is_empty_with_warning() {
    let w = invalid_manifest("version = [");
    assert!(w.starts_with(MANIFEST_WARNING), "{w}");
}

#[test]
fn manifest_bad_version_is_empty_with_warning() {
    assert_eq!(
        invalid_manifest("version = 2\n"),
        format!("{MANIFEST_WARNING}unsupported version 2")
    );
}

#[test]
fn manifest_duplicate_is_empty_with_warning() {
    let text = format!(
        "version = 1\n[[file]]\nname = \"x\"\nsha256 = \"{}\"\n[[file]]\nname = \"x\"\nsha256 = \"{}\"\n",
        a(),
        b()
    );
    assert_eq!(
        invalid_manifest(&text),
        format!("{MANIFEST_WARNING}duplicate template \"x\"")
    );
}

#[test]
fn manifest_bad_sha_is_empty_with_warning() {
    let text = "version = 1\n[[file]]\nname = \"x\"\nsha256 = \"nope\"\n";
    assert_eq!(
        invalid_manifest(text),
        format!("{MANIFEST_WARNING}template \"x\" has an invalid sha256")
    );
}

#[test]
fn manifest_bad_name_is_empty_with_warning() {
    let text = format!(
        "version = 1\n[[file]]\nname = \"../x\"\nsha256 = \"{}\"\n",
        a()
    );
    assert_eq!(
        invalid_manifest(&text),
        format!("{MANIFEST_WARNING}invalid template name \"../x\"")
    );
}

#[test]
fn manifest_unknown_key_is_empty_with_warning() {
    let w = invalid_manifest("version = 1\nextra = 1\n");
    assert!(w.starts_with(MANIFEST_WARNING), "{w}");
    assert!(w.contains("unknown field `extra`"), "{w}");
}

// ---- state ----

#[test]
fn state_new() {
    assert_eq!(state(None, None, &a()), State::New);
}

#[test]
fn state_new_even_when_manifest_remembers_it() {
    assert_eq!(state(None, Some(&a()), &a()), State::New);
}

#[test]
fn state_up_to_date() {
    assert_eq!(state(Some(&a()), Some(&a()), &a()), State::UpToDate);
}

#[test]
fn state_update_available() {
    assert_eq!(state(Some(&a()), Some(&a()), &b()), State::UpdateAvailable);
}

#[test]
fn state_modified_when_local_differs_from_manifest() {
    assert_eq!(state(Some(&c()), Some(&a()), &b()), State::Modified);
}

#[test]
fn state_modified_when_local_differs_from_manifest_and_catalog_unchanged() {
    assert_eq!(state(Some(&c()), Some(&a()), &a()), State::Modified);
}

#[test]
fn state_modified_when_no_manifest_entry() {
    assert_eq!(state(Some(&c()), None, &b()), State::Modified);
}

#[test]
fn state_identical_to_catalog_is_up_to_date_without_manifest_entry() {
    assert_eq!(state(Some(&b()), None, &b()), State::UpToDate);
}

#[test]
fn state_identical_to_catalog_is_up_to_date_when_manifest_differs() {
    assert_eq!(state(Some(&b()), Some(&a()), &b()), State::UpToDate);
}

#[test]
fn state_labels() {
    assert_eq!(State::New.label(), "new");
    assert_eq!(State::UpToDate.label(), "up to date");
    assert_eq!(State::UpdateAvailable.label(), "update available");
    assert_eq!(State::Modified.label(), "modified");
}

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

// ---- hash ----

#[test]
fn sha256_hex_matches_known_vector() {
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn sha256_hex_of_empty_input() {
    assert_eq!(
        sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

// ---- review follow-ups ----

#[test]
fn catalog_rejects_bad_name_after_a_valid_entry() {
    let text = catalog_text(&format!(
        "{}{}",
        entry_text("ok", &a()),
        entry_text("Bad", &b())
    ));
    assert_eq!(
        parse_err(&text),
        "template catalog: invalid template name \"Bad\" (use lowercase letters, digits, '-' and '_', starting with a letter or digit)"
    );
}

#[test]
fn catalog_rejects_bad_sha_after_a_valid_entry() {
    let text = catalog_text(&format!(
        "{}{}",
        entry_text("ok", &a()),
        entry_text("second", "nope")
    ));
    assert_eq!(
        parse_err(&text),
        "template catalog: template \"second\" has an invalid sha256 (expected 64 lowercase hex characters)"
    );
}

#[test]
fn manifest_bad_name_after_a_valid_entry_is_empty_with_warning() {
    let text = format!(
        "version = 1\n[[file]]\nname = \"ok\"\nsha256 = \"{}\"\n[[file]]\nname = \"Bad\"\nsha256 = \"{}\"\n",
        a(),
        b()
    );
    assert_eq!(
        invalid_manifest(&text),
        format!("{MANIFEST_WARNING}invalid template name \"Bad\"")
    );
}

#[test]
fn manifest_bad_sha_after_a_valid_entry_is_empty_with_warning() {
    let text = format!(
        "version = 1\n[[file]]\nname = \"ok\"\nsha256 = \"{}\"\n[[file]]\nname = \"second\"\nsha256 = \"nope\"\n",
        a()
    );
    assert_eq!(
        invalid_manifest(&text),
        format!("{MANIFEST_WARNING}template \"second\" has an invalid sha256")
    );
}

#[test]
fn manifest_warning_escapes_control_characters_from_the_file() {
    let w = invalid_manifest("version = 1\n\"\\u001b]0;x\\u0007\" = 1\n");
    assert!(w.contains("unknown field"), "{w}");
    assert!(w.contains("\\u{1b}]0;x\\u{7}"), "{w}");
    assert!(
        !w.chars().any(|c| c.is_control() && c != '\n' && c != '\t'),
        "{w:?}"
    );
}

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

#[test]
#[should_panic(expected = "invalid template name \"../x\"")]
fn manifest_set_rejects_invalid_name() {
    Manifest::default().set("../x", &a());
}

#[test]
#[should_panic(expected = "invalid sha256 \"nope\"")]
fn manifest_set_rejects_invalid_sha() {
    Manifest::default().set("ok", "nope");
}
