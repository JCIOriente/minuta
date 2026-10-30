//! Consistency of the repo's template catalog: `templates/index.toml` and the files next to it.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use minuta::error::Error;
use minuta::template::catalog::{sha256_hex, Catalog};
use minuta::template::{Layer, Template};
use tempfile::TempDir;

const VALID: &str = "---\ndescription: A valid template\n---\nbody\n";
const OTHER: &str = "---\ndescription: Another valid template\n---\nother body\n";

/// Checks that `dir/index.toml` and the `dir/*.md` files agree. Returns every problem found.
fn check_catalog_dir(dir: &Path) -> Result<(), Vec<String>> {
    let index = dir.join("index.toml");
    let text = fs::read_to_string(&index)
        .map_err(|e| vec![format!("cannot read {}: {e}", index.display())])?;
    let catalog = Catalog::parse(&text).map_err(|e| vec![e.to_string()])?;

    // Every `*.md` directly in the folder, by stem, with its hash.
    let mut files: BTreeMap<String, String> = BTreeMap::new();
    let listing =
        fs::read_dir(dir).map_err(|e| vec![format!("cannot list {}: {e}", dir.display())])?;
    for item in listing {
        let item = item.map_err(|e| vec![format!("cannot list {}: {e}", dir.display())])?;
        let path = item.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            return Err(vec![format!("{} has a non UTF-8 name", path.display())]);
        };
        let bytes =
            fs::read(&path).map_err(|e| vec![format!("cannot read {}: {e}", path.display())])?;
        files.insert(stem.to_string(), sha256_hex(&bytes));
    }

    let listed: BTreeSet<&str> = catalog.entries.iter().map(|e| e.name.as_str()).collect();
    let mut problems = Vec::new();
    let mut renamed: BTreeSet<String> = BTreeSet::new();
    for entry in &catalog.entries {
        let name = &entry.name;
        let path = dir.join(format!("{name}.md"));
        let exists = path
            .try_exists()
            .map_err(|e| vec![format!("cannot check {}: {e}", path.display())])?;
        if !exists {
            // The same bytes under another stem: the name was changed in only one place.
            let other = files
                .iter()
                .find(|(stem, hash)| !listed.contains(stem.as_str()) && **hash == entry.sha256);
            if let Some((stem, _)) = other {
                renamed.insert(stem.clone());
                problems.push(format!(
                    "template \"{name}\": name differs from the file stem: templates/{stem}.md has its sha256"
                ));
            } else {
                problems.push(format!(
                    "template \"{name}\": templates/{name}.md is missing"
                ));
            }
            continue;
        }
        let bytes =
            fs::read(&path).map_err(|e| vec![format!("cannot read {}: {e}", path.display())])?;
        let actual = sha256_hex(&bytes);
        if actual != entry.sha256 {
            problems.push(format!(
                "template \"{name}\": sha256 mismatch: index.toml has {}, templates/{name}.md has {actual}; run: shasum -a 256 templates/{name}.md",
                entry.sha256
            ));
        }
        let parsed = std::str::from_utf8(&bytes)
            .map_err(|e| e.to_string())
            .and_then(|text| {
                Template::parse(name, Layer::Project, None, text).map_err(|e| match e {
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
        if !listed.contains(stem.as_str()) && !renamed.contains(stem) {
            problems.push(format!(
                "templates/{stem}.md has no entry in templates/index.toml"
            ));
        }
    }
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
        vec!["template \"two\": templates/two.md does not parse: template two: built-in: frontmatter is not closed (expected a closing '---' line)".to_string()]
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
