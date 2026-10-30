//! Consistency of the repo's template catalog: `templates/index.toml` and the files next to it.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use minuta::error::Error;
use minuta::template::catalog::{sha256_hex, Catalog};
use minuta::template::{Layer, Template};
use tempfile::TempDir;

const VALID: &str = "---\ndescription: A valid template\n---\nbody\n";
const OTHER: &str = "---\ndescription: Another valid template\n---\nother body\n";

/// Checks that `dir/index.toml` and the `dir/*.md` files agree. Returns every problem found:
/// entries in catalog order, then files without an entry by sorted stem, then non-UTF-8
/// names sorted.
fn check_catalog_dir(dir: &Path) -> Result<(), Vec<String>> {
    let index = dir.join("index.toml");
    let text = fs::read_to_string(&index)
        .map_err(|e| vec![format!("cannot read {}: {e}", index.display())])?;
    let catalog = Catalog::parse(&text).map_err(|e| vec![e.to_string()])?;

    let mut problems = Vec::new();
    let mut non_utf8: BTreeSet<String> = BTreeSet::new();
    // Every `*.md` directly in the folder, read once, by stem.
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let listing =
        fs::read_dir(dir).map_err(|e| vec![format!("cannot list {}: {e}", dir.display())])?;
    for item in listing {
        let item = item.map_err(|e| vec![format!("cannot list {}: {e}", dir.display())])?;
        let path = item.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            non_utf8.insert(item.file_name().to_string_lossy().into_owned());
            continue;
        };
        let bytes =
            fs::read(&path).map_err(|e| vec![format!("cannot read {}: {e}", path.display())])?;
        files.insert(stem.to_string(), bytes);
    }

    let listed: BTreeSet<&str> = catalog.entries.iter().map(|e| e.name.as_str()).collect();
    let mut renamed: BTreeSet<&str> = BTreeSet::new();
    for entry in &catalog.entries {
        let name = &entry.name;
        let Some(bytes) = files.get(name) else {
            // The same bytes under another stem: the name was changed in only one place.
            let other = files.iter().find(|(stem, bytes)| {
                !listed.contains(stem.as_str()) && sha256_hex(bytes) == entry.sha256
            });
            if let Some((stem, _)) = other {
                renamed.insert(stem);
                problems.push(format!(
                    "template \"{name}\": name differs from the file stem: templates/{stem}.md has its sha256"
                ));
            } else {
                problems.push(format!(
                    "template \"{name}\": templates/{name}.md is missing"
                ));
            }
            continue;
        };
        let actual = sha256_hex(bytes);
        if actual != entry.sha256 {
            problems.push(format!(
                "template \"{name}\": sha256 mismatch: index.toml has {}, templates/{name}.md has {actual}; run: shasum -a 256 templates/{name}.md",
                entry.sha256
            ));
        }
        let path = PathBuf::from(format!("templates/{name}.md"));
        let parsed = std::str::from_utf8(bytes)
            .map_err(|e| e.to_string())
            .and_then(|text| {
                Template::parse(name, Layer::Project, Some(path), text).map_err(|e| match e {
                    Error::Usage(message) => message,
                    other => other.to_string(),
                })
            });
        if let Err(reason) = parsed {
            problems.push(format!(
                "template \"{name}\": templates/{name}.md does not parse: {reason}"
            ));
        }
    }
    for stem in files.keys() {
        if !listed.contains(stem.as_str()) && !renamed.contains(stem.as_str()) {
            problems.push(format!(
                "templates/{stem}.md has no entry in templates/index.toml"
            ));
        }
    }
    problems.extend(
        non_utf8
            .iter()
            .map(|name| format!("templates/{name} has a non UTF-8 name")),
    );
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

fn entry_text(name: &str, text: &str) -> String {
    format!(
        "\n[[template]]\nname = \"{name}\"\ndescription = \"d\"\nsha256 = \"{}\"\n",
        sha256_hex(text.as_bytes())
    )
}

/// A folder with `index.toml` made of `version = 1` and `entries`, plus the given files.
fn folder(entries: &str, files: &[(&str, &str)]) -> TempDir {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("index.toml"),
        format!("version = 1\n{entries}"),
    )
    .unwrap();
    for (name, text) in files {
        fs::write(tmp.path().join(name), text).unwrap();
    }
    tmp
}

fn problems(dir: &TempDir) -> Vec<String> {
    match check_catalog_dir(dir.path()) {
        Err(problems) => problems,
        Ok(()) => panic!("expected problems, the catalog passed"),
    }
}

#[test]
fn repo_catalog_parses() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("templates");
    assert_eq!(check_catalog_dir(&dir), Ok(()));
}

#[test]
fn catalog_check_passes_empty_catalog() {
    let dir = folder("", &[]);
    assert_eq!(check_catalog_dir(dir.path()), Ok(()));
}

#[test]
fn catalog_check_passes_fixture_catalog() {
    let entries = format!("{}{}", entry_text("one", VALID), entry_text("two", OTHER));
    let dir = folder(&entries, &[("one.md", VALID), ("two.md", OTHER)]);
    assert_eq!(check_catalog_dir(dir.path()), Ok(()));
}

#[test]
fn catalog_check_flags_missing_file() {
    let entries = format!("{}{}", entry_text("one", VALID), entry_text("two", OTHER));
    let dir = folder(&entries, &[("one.md", VALID)]);
    assert_eq!(
        problems(&dir),
        vec!["template \"two\": templates/two.md is missing".to_string()]
    );
}

#[test]
fn catalog_check_flags_hash_mismatch_with_hint() {
    let entries = format!("{}{}", entry_text("one", VALID), entry_text("two", OTHER));
    let edited = format!("{OTHER}edited\n");
    let dir = folder(&entries, &[("one.md", VALID), ("two.md", &edited)]);
    assert_eq!(
        problems(&dir),
        vec![format!(
            "template \"two\": sha256 mismatch: index.toml has {}, templates/two.md has {}; run: shasum -a 256 templates/two.md",
            sha256_hex(OTHER.as_bytes()),
            sha256_hex(edited.as_bytes())
        )]
    );
}

#[test]
fn catalog_check_flags_unparsable_template() {
    let broken = "---\nbody without a closing delimiter\n";
    let entries = format!("{}{}", entry_text("one", VALID), entry_text("two", broken));
    let dir = folder(&entries, &[("one.md", VALID), ("two.md", broken)]);
    assert_eq!(
        problems(&dir),
        vec!["template \"two\": templates/two.md does not parse: template two: templates/two.md: frontmatter is not closed (expected a closing '---' line)".to_string()]
    );
}

#[test]
fn catalog_check_flags_name_stem_mismatch() {
    // The entry says "two" but the file (same bytes) is called "deux.md".
    let entries = format!("{}{}", entry_text("one", VALID), entry_text("two", OTHER));
    let dir = folder(&entries, &[("one.md", VALID), ("deux.md", OTHER)]);
    assert_eq!(
        problems(&dir),
        vec![
            "template \"two\": name differs from the file stem: templates/deux.md has its sha256"
                .to_string()
        ]
    );
}

#[test]
fn catalog_check_flags_file_without_entry() {
    let dir = folder(
        &entry_text("one", VALID),
        &[("one.md", VALID), ("extra.md", OTHER)],
    );
    assert_eq!(
        problems(&dir),
        vec!["templates/extra.md has no entry in templates/index.toml".to_string()]
    );
}

#[test]
fn catalog_check_reports_every_problem_in_order() {
    let broken = "---\nbody without a closing delimiter\n";
    let entries = [
        entry_text("a-missing", VALID),
        entry_text("b-stale", VALID),
        entry_text("c-broken", broken),
        entry_text("d-ok", VALID),
        entry_text("e-stale", OTHER),
        entry_text("f-missing", OTHER),
    ]
    .concat();
    let stale_b = format!("{VALID}edited\n");
    let stale_e = format!("{OTHER}edited\n");
    let dir = folder(
        &entries,
        &[
            ("b-stale.md", &stale_b),
            ("c-broken.md", broken),
            ("d-ok.md", VALID),
            ("e-stale.md", &stale_e),
            ("y-extra.md", "---\n---\ny\n"),
            ("z-extra.md", "---\n---\nz\n"),
        ],
    );
    assert_eq!(
        problems(&dir),
        vec![
            "template \"a-missing\": templates/a-missing.md is missing".to_string(),
            format!(
                "template \"b-stale\": sha256 mismatch: index.toml has {}, templates/b-stale.md has {}; run: shasum -a 256 templates/b-stale.md",
                sha256_hex(VALID.as_bytes()),
                sha256_hex(stale_b.as_bytes())
            ),
            "template \"c-broken\": templates/c-broken.md does not parse: template c-broken: templates/c-broken.md: frontmatter is not closed (expected a closing '---' line)".to_string(),
            format!(
                "template \"e-stale\": sha256 mismatch: index.toml has {}, templates/e-stale.md has {}; run: shasum -a 256 templates/e-stale.md",
                sha256_hex(OTHER.as_bytes()),
                sha256_hex(stale_e.as_bytes())
            ),
            "template \"f-missing\": templates/f-missing.md is missing".to_string(),
            "templates/y-extra.md has no entry in templates/index.toml".to_string(),
            "templates/z-extra.md has no entry in templates/index.toml".to_string(),
        ]
    );
}

#[test]
fn catalog_check_matches_names_exactly() {
    // Entry `one` is not satisfied by `One.md`, and `One.md` is not listed by `one`.
    let dir = folder(&entry_text("one", VALID), &[("One.md", OTHER)]);
    assert_eq!(
        problems(&dir),
        vec![
            "template \"one\": templates/one.md is missing".to_string(),
            "templates/One.md has no entry in templates/index.toml".to_string(),
        ]
    );
}

#[test]
fn catalog_check_ignores_files_that_are_not_md() {
    let dir = folder(
        &entry_text("one", VALID),
        &[("one.md", VALID), ("notes.txt", "not a template\n")],
    );
    assert_eq!(check_catalog_dir(dir.path()), Ok(()));
}

#[cfg(target_os = "linux")]
#[test]
fn catalog_check_flags_non_utf8_name_and_keeps_going() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    let dir = folder(
        &format!("{}{}", entry_text("one", VALID), entry_text("two", OTHER)),
        &[("one.md", VALID)],
    );
    fs::write(dir.path().join(OsStr::from_bytes(b"bad\xff.md")), VALID).unwrap();
    assert_eq!(
        problems(&dir),
        vec![
            "template \"two\": templates/two.md is missing".to_string(),
            "templates/bad\u{FFFD}.md has a non UTF-8 name".to_string(),
        ]
    );
}

#[cfg(target_os = "linux")]
#[test]
fn catalog_check_orders_non_utf8_names_after_other_problems() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    let dir = folder(
        &entry_text("one", VALID),
        &[("one.md", VALID), ("extra.md", OTHER)],
    );
    for name in [&b"b\xff.md"[..], &b"a\xff.md"[..]] {
        fs::write(dir.path().join(OsStr::from_bytes(name)), VALID).unwrap();
    }
    assert_eq!(
        problems(&dir),
        vec![
            "templates/extra.md has no entry in templates/index.toml".to_string(),
            "templates/a\u{FFFD}.md has a non UTF-8 name".to_string(),
            "templates/b\u{FFFD}.md has a non UTF-8 name".to_string(),
        ]
    );
}

#[test]
fn catalog_check_flags_hash_mismatch_and_parse_failure_together() {
    let broken = "---\nbody without a closing delimiter\n";
    let edited = format!("{broken}edited\n");
    let dir = folder(
        &format!("{}{}", entry_text("one", VALID), entry_text("two", broken)),
        &[("one.md", VALID), ("two.md", &edited)],
    );
    assert_eq!(
        problems(&dir),
        vec![
            format!(
                "template \"two\": sha256 mismatch: index.toml has {}, templates/two.md has {}; run: shasum -a 256 templates/two.md",
                sha256_hex(broken.as_bytes()),
                sha256_hex(edited.as_bytes())
            ),
            "template \"two\": templates/two.md does not parse: template two: templates/two.md: frontmatter is not closed (expected a closing '---' line)".to_string(),
        ]
    );
}

#[test]
fn catalog_templates_declare_transcript_input() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("templates");
    let index = Catalog::parse(&fs::read_to_string(dir.join("index.toml")).unwrap()).unwrap();
    for entry in &index.entries {
        if let Some(input) = &entry.input {
            assert!(!input.contains("summary"), "{}: {input}", entry.name);
        }
        let text = fs::read_to_string(dir.join(format!("{}.md", entry.name))).unwrap();
        let t = Template::parse(&entry.name, Layer::Project, None, &text).unwrap();
        assert_ne!(
            t.input,
            minuta::template::Input::Template("summary".into()),
            "{}",
            entry.name
        );
        assert!(!t.body.contains("summary of"), "{}", entry.name);
        assert!(!t.body.contains("input summary"), "{}", entry.name);
    }
}

#[test]
fn catalog_hashes_match_files() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("templates");
    let index = Catalog::parse(&fs::read_to_string(dir.join("index.toml")).unwrap()).unwrap();
    for entry in &index.entries {
        let bytes = fs::read(dir.join(format!("{}.md", entry.name))).unwrap();
        assert_eq!(sha256_hex(&bytes), entry.sha256, "{}", entry.name);
    }
}
