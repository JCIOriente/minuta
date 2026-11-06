//! Where the outputs go: requested and implicit templates.

use super::*;

const SOCIAL_JSON: &str = "{\n  \"version\": 1,\n  \"template\": \"social\",\n  \"output_lang\": \"es\",\n  \"fields\": {\n    \"title\": \"Hola\",\n    \"body\": \"Corto.\"\n  }\n}\n";

fn run_implicit<'a>(
    template: &'a Template,
    backend: &'a dyn Backend,
    implicit: bool,
    implicit_inputs: &'a [String],
) -> TemplateRun<'a> {
    TemplateRun {
        implicit,
        implicit_inputs,
        ..run(template, backend, "es")
    }
}

/// Every file under `dir` (relative, sorted), skipping the transcript the job starts with and
/// the call cache.
fn files_under(dir: &Path) -> Vec<String> {
    fn walk(dir: &Path, base: &Path, out: &mut Vec<String>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, base, out);
            } else {
                out.push(path.strip_prefix(base).unwrap().display().to_string());
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.retain(|f| f != TRANSCRIPT_FILE && !f.starts_with(".minuta/llm-cache/"));
    out.sort();
    out
}

#[test]
fn requested_template_writes_md_at_root_and_json_in_assets() {
    let (dir, _) = job();
    let t = tpl("social", FIELDS);
    let b = ScriptedBackend::new(ID, CONTEXT, vec![Ok(VALID.into())]);
    execute(&run(&t, &b, "es"), dir.path()).unwrap();
    assert_eq!(files_under(dir.path()), ["assets/social.json", "social.md"]);
    assert_eq!(
        fs::read_to_string(template_json(dir.path(), "social")).unwrap(),
        SOCIAL_JSON
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("social.md")).unwrap(),
        "Hola\n\nCorto.\n"
    );
}

#[test]
fn implicit_dependency_writes_to_assets() {
    let (dir, _) = job();
    let t = tpl("social", FIELDS);
    let b = ScriptedBackend::new(ID, CONTEXT, vec![Ok(VALID.into())]);
    let r = run_implicit(&t, &b, true, &[]);
    execute(&r, dir.path()).unwrap();
    assert_eq!(
        files_under(dir.path()),
        ["assets/social.json", "assets/social.md"]
    );
    assert_eq!(
        r.artifacts(dir.path()),
        vec![
            template_json(dir.path(), "social"),
            implicit_md(dir.path(), "social")
        ]
    );
    assert_eq!(
        fs::read_to_string(template_json(dir.path(), "social")).unwrap(),
        SOCIAL_JSON
    );
    assert_eq!(
        fs::read_to_string(implicit_md(dir.path(), "social")).unwrap(),
        "Hola\n\nCorto.\n"
    );
}

#[test]
fn implicit_markdown_dependency_writes_only_md_to_assets() {
    let (dir, _) = job();
    let t = tpl("summary", TEXT);
    let b = ScriptedBackend::new(ID, CONTEXT, vec![Ok("Resumen.".into())]);
    execute(&run_implicit(&t, &b, true, &[]), dir.path()).unwrap();
    assert_eq!(files_under(dir.path()), ["assets/summary.md"]);
}

#[test]
fn dependent_reads_implicit_inputs_from_assets() {
    let (dir, _) = job();
    fs::write(template_json(dir.path(), "chapters"), CHAPTERS_ARTIFACT).unwrap();
    fs::write(implicit_md(dir.path(), "summary"), "El resumen.\n").unwrap();
    let implicit = ["chapters".to_string(), "summary".to_string()];

    let youtube = tpl("youtube", YOUTUBE);
    let b = ScriptedBackend::new(ID, CONTEXT, vec![Ok(VALID.into())]);
    let r = run_implicit(&youtube, &b, false, &implicit);
    execute(&r, dir.path()).unwrap();
    let json = fs::read_to_string(template_json(dir.path(), "youtube")).unwrap();
    assert_eq!(
        json,
        "{\n  \"version\": 1,\n  \"template\": \"youtube\",\n  \"output_lang\": \"es\",\n  \
         \"fields\": {\n    \"title\": \"Hola\",\n    \"chapters\": [\n      {\n        \
         \"start\": 0,\n        \"title\": \"Introducción\"\n      },\n      {\n        \
         \"start\": 12,\n        \"title\": \"Empezamos\"\n      }\n    ],\n    \
         \"body\": \"Corto.\"\n  }\n}\n"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("youtube.md")).unwrap(),
        "Hola\n\n0:00 Introducción\n0:12 Empezamos\n\nCorto.\n"
    );

    let brief = tpl("brief", "---\ninput: summary\n---\nShorten it.\n");
    let b = ScriptedBackend::new(ID, CONTEXT, vec![Ok("Corto.".into())]);
    let r = run_implicit(&brief, &b, false, &implicit);
    execute(&r, dir.path()).unwrap();
    assert_eq!(b.calls()[0].1, "El resumen.\n");
    // The same fingerprint as when the inputs sit in their normal places.
    assert_eq!(r.fingerprint(&ctx(dir.path()), "reunion").unwrap(), {
        let (normal, _) = job();
        fs::write(normal.path().join("summary.md"), "El resumen.\n").unwrap();
        run(&brief, &b, "es")
            .fingerprint(&ctx(normal.path()), "reunion")
            .unwrap()
    });
}

#[test]
fn requesting_an_implicit_template_moves_its_outputs() {
    let (dir, _) = job();
    let t = tpl("social", FIELDS);
    let b = ScriptedBackend::new(ID, CONTEXT, vec![Ok(VALID.into()), Ok(VALID.into())]);
    execute(&run_implicit(&t, &b, true, &[]), dir.path()).unwrap();
    let requested = run(&t, &b, "es");
    execute(&requested, dir.path()).unwrap();
    assert_eq!(files_under(dir.path()), ["assets/social.json", "social.md"]);
}

#[test]
fn move_implicit_moves_files_without_running() {
    let (dir, _) = job();
    let t = tpl("social", FIELDS);
    let b = ScriptedBackend::new(ID, CONTEXT, vec![Ok(VALID.into())]);
    execute(&run_implicit(&t, &b, true, &[]), dir.path()).unwrap();
    let requested = run(&t, &b, "es");
    assert!(requested.move_implicit(&ctx(dir.path())).unwrap());
    assert_eq!(files_under(dir.path()), ["assets/social.json", "social.md"]);
    assert_eq!(
        fs::read_to_string(template_json(dir.path(), "social")).unwrap(),
        SOCIAL_JSON
    );
    assert!(!requested.move_implicit(&ctx(dir.path())).unwrap());
    assert_eq!(b.calls().len(), 1, "no new call");
}

#[test]
fn move_implicit_removes_a_copy_when_the_normal_file_exists() {
    let (dir, _) = job();
    let t = tpl("summary", TEXT);
    let b = ScriptedBackend::new(ID, CONTEXT, vec![Ok("Resumen.".into())]);
    execute(&run_implicit(&t, &b, true, &[]), dir.path()).unwrap();
    fs::write(dir.path().join("summary.md"), "Resumen.\n").unwrap();
    assert!(run(&t, &b, "es").move_implicit(&ctx(dir.path())).unwrap());
    assert_eq!(files_under(dir.path()), ["summary.md"]);
}

#[test]
fn implicit_copy_that_cannot_be_removed_is_an_error() {
    let (dir, _) = job();
    let t = tpl("social", FIELDS);
    let b = ScriptedBackend::new(ID, CONTEXT, vec![Ok(VALID.into())]);
    let blocker = implicit_md(dir.path(), "social");
    fs::create_dir_all(&blocker).unwrap();
    let err = execute(&run(&t, &b, "es"), dir.path()).unwrap_err();
    assert!(
        matches!(err, Error::Io(ref e) if e.to_string().starts_with(&format!("{}: ", blocker.display()))),
        "{err}"
    );
}

#[test]
fn implicit_copy_that_cannot_be_read_is_an_error() {
    let (dir, _) = job();
    let t = tpl("social", FIELDS);
    let b = ScriptedBackend::new(ID, CONTEXT, vec![]);
    let blocker = implicit_md(dir.path(), "social");
    fs::create_dir_all(&blocker).unwrap();
    let err = run(&t, &b, "es")
        .move_implicit(&ctx(dir.path()))
        .unwrap_err();
    assert!(
        matches!(err, Error::Io(ref e) if e.to_string().starts_with(&format!("{}: ", blocker.display()))),
        "{err}"
    );
}

/// Replaces `assets/` with a symlink to a new folder that holds the transcript; returns it.
#[cfg(unix)]
fn symlink_assets(dir: &Path) -> TempDir {
    let outside = TempDir::new().unwrap();
    let assets = dir.join("assets");
    fs::rename(
        assets.join("transcript.json"),
        outside.path().join("transcript.json"),
    )
    .unwrap();
    fs::remove_dir_all(&assets).unwrap();
    std::os::unix::fs::symlink(outside.path(), &assets).unwrap();
    outside
}

#[cfg(unix)]
#[test]
fn outputs_refuse_a_symlinked_assets_folder() {
    let (dir, _) = job();
    let outside = symlink_assets(dir.path());
    let t = tpl("social", FIELDS);
    let b = ScriptedBackend::new(ID, CONTEXT, vec![Ok(VALID.into())]);
    assert!(execute(&run(&t, &b, "es"), dir.path()).is_err());
    assert_eq!(files_under(outside.path()), ["transcript.json"]);
    assert!(!dir.path().join("social.md").try_exists().unwrap());
}
