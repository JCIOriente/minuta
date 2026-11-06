use super::*;

// ---- output language precedence ----

#[test]
fn output_lang_flag_wins() {
    assert_eq!(
        output_lang("summary", Some("fr"), Some("de"), Some("pt"), "es").unwrap(),
        "fr"
    );
}

#[test]
fn output_lang_template_beats_config() {
    assert_eq!(
        output_lang("summary", None, Some("de"), Some("pt"), "es").unwrap(),
        "de"
    );
}

#[test]
fn output_lang_config_beats_detected() {
    assert_eq!(
        output_lang("summary", None, None, Some("pt"), "es").unwrap(),
        "pt"
    );
}

#[test]
fn output_lang_detected_default() {
    assert_eq!(
        output_lang("summary", None, None, None, "es").unwrap(),
        "es"
    );
}

#[test]
fn output_lang_unknown_detected_fails_the_template() {
    let err = output_lang("summary", None, None, None, "xx").unwrap_err();
    assert!(
        matches!(err, Error::Template { ref name, ref message }
            if name == "summary" && message.contains("'xx'") && message.contains("--output-lang")),
        "{err}"
    );
}

#[test]
fn output_lang_unknown_detected_is_fine_when_another_source_is_set() {
    assert_eq!(
        output_lang("summary", None, None, Some("pt"), "xx").unwrap(),
        "pt"
    );
}

#[test]
fn output_lang_validates_the_chosen_code() {
    for (case, result) in [
        ("flag", output_lang("summary", Some("zz"), None, None, "es")),
        (
            "template",
            output_lang("summary", None, Some("zz"), None, "es"),
        ),
        (
            "config",
            output_lang("summary", None, None, Some("zz"), "es"),
        ),
    ] {
        assert!(
            matches!(result, Err(Error::Usage(ref m)) if m.contains("zz")),
            "{case}: {result:?}"
        );
    }
}

// ---- backend precedence ----

fn id(s: &str) -> BackendId {
    s.parse().unwrap()
}

#[test]
fn backend_flag_wins() {
    let got = backend_id(Some(&id("claude")), Some(&id("ollama:a")), &id("ollama:b"));
    assert_eq!(got.to_string(), "claude");
}

#[test]
fn backend_template_beats_default() {
    let got = backend_id(None, Some(&id("ollama:a")), &id("ollama:b"));
    assert_eq!(got.to_string(), "ollama:a");
}

#[test]
fn backend_default() {
    let got = backend_id(None, None, &id("ollama:b"));
    assert_eq!(got.to_string(), "ollama:b");
}
