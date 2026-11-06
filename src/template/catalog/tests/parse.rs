use super::*;

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
