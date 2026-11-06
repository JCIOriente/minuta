//! The "Curated templates" tables in `docs/templates.md` and `docs/templates.es.md` list every
//! template of `templates/index.toml`, and only those.

use std::fs;
use std::path::Path;

use super::*;

const EN_HEADING: &str = "## Curated templates";
const ES_HEADING: &str = "## Plantillas del catálogo";

/// Returns the problems found in the table under `heading` in `doc`: a section that is
/// missing, a catalog template without a row, a row for a template that is not in the
/// catalog. With `with_description`, a row must also carry the catalog description verbatim.
fn check_table(doc: &str, heading: &str, entries: &[Entry], with_description: bool) -> Vec<String> {
    let Some(start) = doc.lines().position(|l| l == heading) else {
        return vec![format!("missing section {heading:?}")];
    };
    let rows: Vec<&str> = doc
        .lines()
        .skip(start + 1)
        .take_while(|l| !l.starts_with("## "))
        .filter(|l| l.starts_with("| `"))
        .collect();
    let mut problems = Vec::new();
    for entry in entries {
        let prefix = format!("| `{}` |", entry.name);
        match rows.iter().find(|r| r.starts_with(&prefix)) {
            None => problems.push(format!("template \"{}\" has no row", entry.name)),
            Some(row) if with_description && !row.contains(&entry.description) => {
                problems.push(format!(
                    "template \"{}\": row lacks the description {:?}",
                    entry.name, entry.description
                ))
            }
            Some(_) => {}
        }
    }
    for row in rows {
        let name = row
            .trim_start_matches("| `")
            .split('`')
            .next()
            .unwrap_or("");
        if !entries.iter().any(|e| e.name == name) {
            problems.push(format!("row for \"{name}\", which is not in the catalog"));
        }
    }
    problems
}

fn entry(name: &str, description: &str) -> Entry {
    Entry {
        name: name.to_string(),
        description: description.to_string(),
        platform: None,
        tags: Vec::new(),
        input: None,
        sha256: a(),
    }
}

fn repo_entries() -> Vec<Entry> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let text = fs::read_to_string(root.join("templates/index.toml")).unwrap();
    Catalog::parse(&text).unwrap().entries
}

fn repo_doc(name: &str) -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    fs::read_to_string(root.join("docs").join(name)).unwrap()
}

#[test]
fn docs_table_reports_missing_stale_and_changed_rows() {
    let entries = [entry("one", "First one"), entry("two", "Second one")];
    let doc = "# T\n\n## Curated templates\n\n| Name | Description |\n| --- | --- |\n\
               | `one` | Old words |\n| `gone` | Removed |\n\n## Next\n\n| `two` | Second one |\n";
    assert_eq!(
        check_table(doc, EN_HEADING, &entries, true),
        vec![
            "template \"one\": row lacks the description \"First one\"".to_string(),
            "template \"two\" has no row".to_string(),
            "row for \"gone\", which is not in the catalog".to_string(),
        ]
    );
    assert_eq!(
        check_table("# T\n", EN_HEADING, &entries, true),
        vec!["missing section \"## Curated templates\"".to_string()]
    );
}

#[test]
fn docs_list_every_catalog_template_in_english() {
    let problems = check_table(&repo_doc("templates.md"), EN_HEADING, &repo_entries(), true);
    assert_eq!(problems, Vec::<String>::new());
}

#[test]
fn docs_list_every_catalog_template_in_spanish() {
    let problems = check_table(
        &repo_doc("templates.es.md"),
        ES_HEADING,
        &repo_entries(),
        false,
    );
    assert_eq!(problems, Vec::<String>::new());
}
