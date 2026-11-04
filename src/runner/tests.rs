use super::test_support::config;
use super::*;
use crate::config::Overrides;
use std::fs;

/// Templates `a`, `b` (input `a`) and `c` in the run; `c` uses claude.
fn chain() -> Resolved {
    let mut catalog = Catalog::default();
    for (name, text) in [
        ("a", "A."),
        ("b", "---\ninput: a\n---\nB."),
        ("c", "---\nllm: claude\n---\nC."),
    ] {
        let template =
            crate::template::Template::parse(name, crate::template::Layer::Project, None, text)
                .unwrap();
        catalog.templates.insert(name.to_string(), template);
    }
    let ollama = BackendId::Ollama { model: "m".into() };
    let backends = [
        ("a", ollama.clone()),
        ("b", ollama),
        ("c", BackendId::Claude { model: None }),
    ];
    Resolved {
        catalog,
        order: vec!["a".into(), "b".into(), "c".into()],
        backends: backends
            .into_iter()
            .map(|(n, id)| (n.to_string(), id))
            .collect(),
        ..Resolved::default()
    }
}

fn force(arg: Option<ForceArg>, resolved: &Resolved) -> (Option<Force>, Vec<String>) {
    resolve_force(arg.as_ref(), resolved).unwrap()
}

fn name(s: &str) -> Option<ForceArg> {
    Some(ForceArg::Name(s.to_string()))
}

#[test]
fn force_resolution() {
    let r = chain();
    let all = vec!["a".to_string(), "b".into(), "c".into()];
    assert_eq!(force(None, &r), (None, vec![]));
    assert_eq!(
        force(Some(ForceArg::All), &r),
        (Some(Force::All), all.clone())
    );
    assert_eq!(
        force(name("audio"), &r),
        (Some(Force::From(StepName::Audio)), all)
    );
    assert_eq!(force(name("a"), &r), (None, vec!["a".into(), "b".into()]));
    assert_eq!(force(name("b"), &r), (None, vec!["b".into()]));
    assert_eq!(force(name("c"), &r), (None, vec!["c".into()]));
}

/// A catalog of `(name, template text)` pairs. The texts are parsed under a placeholder
/// name, so `chapters` (reserved for the parser) can still be a catalog key.
fn catalog_of(templates: &[(&str, &str)]) -> Catalog {
    let mut catalog = Catalog::default();
    for (name, text) in templates {
        let template = crate::template::Template::parse(
            "placeholder",
            crate::template::Layer::Project,
            None,
            text,
        )
        .unwrap();
        catalog.templates.insert(name.to_string(), template);
    }
    catalog
}

/// Points `name` at `dep` as its input, as `input: chapters` will once the parser allows it.
fn read_input_from(catalog: &mut Catalog, name: &str, dep: &str) {
    catalog.templates.get_mut(name).unwrap().input = Input::Template(dep.to_string());
}

const YOUTUBE_LIKE: &str = "---\ninput: summary\noutput: fields\nfields: [body, chapters]\n---\nY.";

#[test]
fn deps_of_input_then_chapters() {
    let catalog = catalog_of(&[("summary", "S."), ("youtube", YOUTUBE_LIKE)]);
    assert_eq!(
        deps_of(&catalog, "youtube"),
        Some(vec!["summary".to_string(), "chapters".to_string()])
    );
}

#[test]
fn deps_of_no_duplicate() {
    let mut catalog = catalog_of(&[("toc", YOUTUBE_LIKE)]);
    read_input_from(&mut catalog, "toc", "chapters");
    assert_eq!(deps_of(&catalog, "toc"), Some(vec!["chapters".to_string()]));
}

#[test]
fn deps_of_without_the_field_is_the_input_only() {
    let catalog = catalog_of(&[
        ("summary", "S."),
        (
            "post",
            "---\ninput: summary\noutput: fields\nfields: [body, tags]\n---\nP.",
        ),
    ]);
    assert_eq!(deps_of(&catalog, "post"), Some(vec!["summary".to_string()]));
    assert_eq!(deps_of(&catalog, "summary"), Some(vec![]));
    assert_eq!(deps_of(&catalog, "zzz"), None);
}

#[test]
fn force_chapters_covers_dependents() {
    let mut catalog = catalog_of(&[
        ("summary", "S."),
        ("chapters", "C."),
        ("youtube", YOUTUBE_LIKE),
        ("toc", "T."),
        ("linkedin", "---\ninput: summary\n---\nL."),
    ]);
    read_input_from(&mut catalog, "toc", "chapters");
    let resolved = Resolved {
        catalog,
        order: ["summary", "chapters", "youtube", "toc", "linkedin"]
            .map(String::from)
            .to_vec(),
        ..Resolved::default()
    };
    assert_eq!(
        force(name("chapters"), &resolved),
        (
            None,
            vec!["chapters".to_string(), "youtube".into(), "toc".into()]
        )
    );
}

#[test]
fn force_unknown_name_is_usage() {
    let err = resolve_force(name("zzz").as_ref(), &chain()).unwrap_err();
    assert!(
        matches!(&err, Error::Usage(m) if m.contains("`zzz`") && m.contains("(a, b, c)")),
        "{err}"
    );
    let err = resolve_force(name("zzz").as_ref(), &Resolved::default()).unwrap_err();
    assert!(
        matches!(&err, Error::Usage(m) if m.starts_with("unknown step `zzz`")),
        "{err}"
    );
}

#[test]
fn distinct_backends_and_off_machine_templates() {
    let r = chain();
    assert_eq!(
        r.backend_ids(),
        [
            BackendId::Ollama { model: "m".into() },
            BackendId::Claude { model: None }
        ]
    );
    assert_eq!(
        r.off_machine(&config(&[])),
        [("c", "claude (Anthropic)".to_string())]
    );
}

fn off_machine_for(url: &str) -> Vec<(String, String)> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, format!("[endpoints.box]\nurl = \"{url}\"\n")).unwrap();
    let config = Config::resolve(Overrides::default(), &|_| None, Some(&path)).unwrap();
    let mut r = chain();
    let id = BackendId::Endpoint {
        name: "box".into(),
        model: "m".into(),
    };
    r.backends.insert("c".to_string(), id);
    r.off_machine(&config)
        .into_iter()
        .map(|(name, target)| (name.to_string(), target))
        .collect()
}

#[test]
fn notice_for_endpoint_names_it_and_its_host() {
    assert_eq!(
        off_machine_for("https://openrouter.ai/api/v1"),
        [("c".to_string(), "box (openrouter.ai)".to_string())]
    );
}

#[test]
fn notice_for_endpoint_on_the_lan() {
    assert_eq!(
        off_machine_for("http://192.168.1.20:1234/v1"),
        [("c".to_string(), "box (192.168.1.20)".to_string())]
    );
}

#[test]
fn notice_for_endpoint_absent_on_loopback() {
    for url in [
        "http://localhost:1234/v1",
        "http://127.0.0.1:1234/v1",
        "http://[::1]:1234/v1",
    ] {
        assert_eq!(off_machine_for(url), [], "{url}");
    }
}
