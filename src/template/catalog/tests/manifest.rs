//! `Manifest`: reading, writing and recording `.pulled.toml`, and the warning for an invalid one.

use super::*;

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

// ---- review follow-ups ----

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
#[should_panic(expected = "invalid template name \"../x\"")]
fn manifest_set_rejects_invalid_name() {
    Manifest::default().set("../x", &a());
}

#[test]
#[should_panic(expected = "invalid sha256 \"nope\"")]
fn manifest_set_rejects_invalid_sha() {
    Manifest::default().set("ok", "nope");
}
