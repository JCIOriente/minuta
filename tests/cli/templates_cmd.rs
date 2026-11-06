use std::fs;

use assert_cmd::assert::OutputAssertExt;
use predicates::str::contains;

use crate::fake_tools::*;

#[test]
fn templates_list_shows_layers() {
    let env = Env::new();
    env.template(
        "home/.minuta/templates",
        "acta",
        "---\ndescription: Formal minutes\nllm: claude\n---\nMinutes.",
    );
    env.template("templates", "notes", "---\ninput: acta\n---\nNotes.");
    env.template("templates", "summary", "My own summary.");
    env.template("templates", "broken", "---\nnope: 1\n---\nX.");
    let out = env.minuta(&["templates", "list"]);
    out.clone().assert().success();
    let text = stdout(&out);
    let rows: Vec<Vec<&str>> = text
        .lines()
        .map(|l| {
            l.split("  ")
                .map(str::trim)
                .filter(|c| !c.is_empty())
                .collect()
        })
        .collect();
    assert_eq!(
        rows[0],
        ["NAME", "LAYER", "INPUT", "OUTPUT", "LLM", "DESCRIPTION"],
        "{text}"
    );
    assert!(
        rows.contains(&vec![
            "acta",
            "user",
            "transcript",
            "markdown",
            "claude",
            "Formal minutes"
        ]),
        "{text}"
    );
    assert!(
        rows.contains(&vec!["notes", "project", "acta", "markdown", "(default)"]),
        "{text}"
    );
    assert!(
        rows.contains(&vec![
            "summary",
            "project",
            "transcript",
            "markdown",
            "(default)"
        ]),
        "{text}"
    );
    let broken = env.path("templates/broken.md").display().to_string();
    let invalid = text
        .lines()
        .find(|l| l.contains(&broken))
        .unwrap_or_else(|| panic!("no invalid entry in {text}"));
    assert!(invalid.contains("nope"), "{invalid}");
}

#[test]
fn templates_list_shows_built_in() {
    let env = Env::new();
    let out = env.minuta(&["templates", "list"]);
    out.clone().assert().success();
    let text = stdout(&out);
    let summary = text.lines().find(|l| l.starts_with("summary")).unwrap();
    let cols: Vec<&str> = summary
        .split("  ")
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .collect();
    assert_eq!(
        cols,
        [
            "summary",
            "built-in",
            "transcript",
            "markdown",
            "(default)",
            "Short summary of the recording"
        ]
    );
}

#[test]
fn templates_list_shows_built_in_chapters() {
    let env = Env::new();
    let out = env.minuta(&["templates", "list"]);
    out.clone().assert().success();
    let text = stdout(&out);
    let chapters = text.lines().find(|l| l.starts_with("chapters")).unwrap();
    let cols: Vec<&str> = chapters
        .split("  ")
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .collect();
    assert_eq!(
        cols,
        [
            "chapters",
            "built-in",
            "transcript",
            "chapters",
            "(default)",
            "Timestamped chapters (YouTube rules)"
        ]
    );
}

#[test]
fn templates_show_chapters() {
    let env = Env::new();
    let out = env.minuta(&["templates", "show", "chapters"]);
    out.clone().assert().success();
    assert_eq!(stderr(&out), "built-in\n");
    assert_eq!(
        stdout(&out),
        "---\n\
         description: Timestamped chapters (YouTube rules)\n\
         input: transcript\n\
         output: chapters\n\
         limits:\n\
         \x20 chapters: { max: 20, max_chars: 1500 }\n\
         ---\n\
         Divide the recording \"{{job_name}}\" ({{duration}}) into chapters by topic, for the YouTube\n\
         description. Start a chapter where the speakers move to a new topic, not at every pause. Each\n\
         title names the topic in a few words that a viewer would recognize. Use only topics that are\n\
         in the input.\n"
    );
}

#[test]
fn templates_list_shows_output_column() {
    let env = Env::new();
    env.template(
        "templates",
        "post",
        "---\ndescription: A post\noutput: fields\nfields: [body]\n---\nWrite.",
    );
    env.template("templates", "plain", "---\ndescription: Text\n---\nWrite.");
    let out = env.minuta(&["templates", "list"]);
    out.clone().assert().success();
    let text = stdout(&out);
    let rows: Vec<Vec<&str>> = text
        .lines()
        .map(|l| {
            l.split("  ")
                .map(str::trim)
                .filter(|c| !c.is_empty())
                .collect()
        })
        .collect();
    assert!(
        rows.contains(&vec![
            "post",
            "project",
            "transcript",
            "fields",
            "(default)",
            "A post"
        ]),
        "{text}"
    );
    assert!(
        rows.contains(&vec![
            "plain",
            "project",
            "transcript",
            "markdown",
            "(default)",
            "Text"
        ]),
        "{text}"
    );
}

#[test]
fn templates_list_output_chapters() {
    let env = Env::new();
    env.template(
        "templates",
        "chapters",
        "---\ndescription: Chapters\noutput: chapters\n---\nWrite.",
    );
    let out = env.minuta(&["templates", "list"]);
    out.clone().assert().success();
    let text = stdout(&out);
    let rows: Vec<Vec<&str>> = text
        .lines()
        .map(|l| {
            l.split("  ")
                .map(str::trim)
                .filter(|c| !c.is_empty())
                .collect()
        })
        .collect();
    assert!(
        rows.contains(&vec![
            "chapters",
            "project",
            "transcript",
            "chapters",
            "(default)",
            "Chapters"
        ]),
        "{text}"
    );
}

#[test]
fn templates_show_prints_winning_file() {
    let env = Env::new();
    let out = env.minuta(&["templates", "show", "summary"]);
    out.clone().assert().success();
    assert_eq!(stderr(&out), "built-in\n");
    assert!(
        stdout(&out).starts_with("---\ndescription: Short summary"),
        "{}",
        stdout(&out)
    );

    env.template("templates", "summary", "Mine.\n");
    let out = env.minuta(&["templates", "show", "summary"]);
    out.clone().assert().success();
    assert_eq!(
        stderr(&out),
        // The project layer is `./templates`, found through the canonical current directory.
        format!(
            "{}\n",
            fs::canonicalize(env.path("templates/summary.md"))
                .unwrap()
                .display()
        )
    );
    assert_eq!(stdout(&out), "Mine.\n");
}

#[test]
fn templates_show_unknown_exits_2() {
    let env = Env::new();
    env.minuta(&["templates", "show", "nope"])
        .assert()
        .code(2)
        .stdout("")
        .stderr(contains("unknown template 'nope'"));
}

/// Every char that could move the cursor or start an escape sequence.
fn control_chars(text: &str) -> Vec<char> {
    text.chars()
        .filter(|c| c.is_control() && *c != '\n')
        .collect()
}

#[test]
fn templates_list_neutralizes_control_characters() {
    let env = Env::new();
    env.template(
        "templates",
        "evil",
        "---\ndescription: \"ok\\e[2K\\rfake\\nsummary  built-in\"\n---\nX.",
    );
    // An ignored file whose name, shown in the path and the error, has a control character.
    env.template("templates", "bro\x1b[31mken", "X.");
    let out = env.minuta(&["templates", "list"]);
    out.clone().assert().success();
    let text = stdout(&out);
    assert_eq!(control_chars(&text), Vec::<char>::new(), "{text:?}");
    let evil: Vec<&str> = text.lines().filter(|l| l.contains("fake")).collect();
    assert_eq!(evil.len(), 1, "{text}");
    assert!(evil[0].starts_with("evil "), "{text}");
    let broken: Vec<&str> = text.lines().filter(|l| l.contains("bro?[31mken")).collect();
    assert_eq!(broken.len(), 1, "{text}");
    assert!(broken[0].starts_with("invalid "), "{text}");
    assert_eq!(
        text.lines().filter(|l| l.starts_with("summary")).count(),
        1,
        "no fake row: {text}"
    );
}

/// A template whose `output` value carries an SGR sequence and an OSC title change.
const EVIL_OUTPUT: &str = "---\noutput: \"x\\e[31mRED\\e]0;pwned\\a\"\n---\nX.";

#[test]
fn template_errors_reach_stderr_without_control_characters() {
    let env = Env::new();
    env.template("templates", "acta", EVIL_OUTPUT);
    env.template("templates", "notes", "---\ninput: acta\n---\nNotes.");
    let fake = ollama("unused");
    let cases: [&[&str]; 3] = [
        &["run", "clase-4.mp4", "-t", "acta"],
        &["run", "clase-4.mp4", "-t", "notes"],
        &["templates", "show", "acta"],
    ];
    for args in cases {
        let out = env.minuta_llm(args, &fake);
        assert_eq!(out.status.code(), Some(2), "{args:?}: {}", stderr(&out));
        let err = stderr(&out);
        assert!(
            err.contains("RED"),
            "{args:?}: the error names the value: {err:?}"
        );
        assert_eq!(control_chars(&err), Vec::<char>::new(), "{args:?}: {err:?}");
    }
    assert!(!env.path("minuta-out").exists());
}
