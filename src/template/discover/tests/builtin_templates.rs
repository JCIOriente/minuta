use super::*;
use crate::chapters::ChaptersSpec;
use crate::fields::{Field, Format, Limit};
use crate::template::Input;

#[test]
fn builtin_summary_parses() {
    let e = env();
    let c = discover(&Dirs {
        user: None,
        project: e.project.clone(),
    })
    .unwrap();
    let t = c.get("summary").unwrap();
    assert_eq!(t.input, Input::Transcript);
    let out = t.render(&vars());
    assert!(out.contains("reunion"), "{out}");
    assert!(out.contains("1:02:03"), "{out}");
    for word in ["overview", "Key points", "Decisions", "Action items"] {
        assert!(out.contains(word), "{word}");
    }
    assert!(!out.contains("{{"), "{out}");
}

fn builtin(name: &str) -> Template {
    let e = env();
    let c = discover(&Dirs {
        user: None,
        project: e.project.clone(),
    })
    .unwrap();
    c.get(name).unwrap().clone()
}

fn builtin_limit(name: &str, field: Field) -> Limit {
    let t = builtin(name);
    spec(&t)
        .fields
        .iter()
        .find(|f| f.field == field)
        .unwrap()
        .limit
}

fn builtin_shape(name: &str, fields: &[Field]) -> String {
    let t = builtin(name);
    assert_eq!(t.layer, Layer::BuiltIn);
    assert_eq!(t.input, Input::Transcript);
    let s = spec(&t);
    assert_eq!(s.format, Format::Plain);
    let got: Vec<Field> = s.fields.iter().map(|f| f.field).collect();
    assert_eq!(got, fields);
    assert!(t.description.is_some());
    let out = t.render(&vars());
    assert!(!out.contains("{{"), "{out}");
    assert!(out.contains("reunion"), "{out}");
    assert!(out.contains("Do not invent"), "{out}");
    out
}

#[test]
fn builtin_youtube_shape() {
    let out = builtin_shape(
        "youtube",
        &[
            Field::Title,
            Field::Body,
            Field::Chapters,
            Field::Hashtags,
            Field::Tags,
            Field::ThumbnailText,
        ],
    );
    assert!(out.contains("searchable"), "{out}");
    assert!(out.contains("first two lines"), "{out}");
}

#[test]
fn builtin_youtube_chapters_instruction() {
    let out = builtin("youtube").render(&vars());
    let lines: Vec<&str> = out.lines().filter(|l| l.contains("chapters")).collect();
    assert_eq!(
        lines,
        ["- Do not write chapters or timestamps in the body. The chapter list is added after it."]
    );
}

#[test]
fn builtin_youtube_declares_hashtags_after_chapters() {
    assert_eq!(
        builtin_limit("youtube", Field::Hashtags),
        Limit {
            max_chars: Some(80),
            max: Some(5),
        }
    );
    let out = builtin("youtube").render(&vars());
    let hashtags = out.find("- hashtags:").expect("a hashtags instruction");
    assert!(out.find("- body:").unwrap() < hashtags, "{out}");
    assert!(hashtags < out.find("- tags:").unwrap(), "{out}");
}

#[test]
fn builtin_youtube_no_placeholder() {
    let out = builtin("youtube").render(&vars());
    assert_eq!(out.matches("[Add chapters").count(), 0, "{out}");
}

#[test]
fn builtin_linkedin_shape() {
    let out = builtin_shape("linkedin", &[Field::Body, Field::Hashtags]);
    assert!(out.contains("is a hook"), "{out}");
    assert!(out.contains("short paragraphs"), "{out}");
    assert!(out.contains("Do not include links"), "{out}");
}

#[test]
fn builtin_instagram_shape() {
    let out = builtin_shape("instagram", &[Field::Body, Field::Hashtags]);
    assert!(out.contains("Open with a hook"), "{out}");
    assert!(out.contains("optional call to action"), "{out}");
}

#[test]
fn builtin_chapters_shape() {
    let t = builtin("chapters");
    assert_eq!(t.layer, Layer::BuiltIn);
    assert_eq!(t.input, Input::Transcript);
    assert_eq!(
        t.output,
        Output::Chapters(ChaptersSpec {
            max: Some(20),
            max_chars: Some(1500)
        })
    );
    assert_eq!(
        t.description.as_deref(),
        Some("Timestamped chapters (YouTube rules)")
    );
}

#[test]
fn builtin_chapters_max_limit() {
    let Output::Chapters(spec) = builtin("chapters").output else {
        panic!("chapters is not a chapters output");
    };
    assert_eq!(spec.max, Some(20));
}

#[test]
fn builtin_chapters_max_chars_limit() {
    let Output::Chapters(spec) = builtin("chapters").output else {
        panic!("chapters is not a chapters output");
    };
    assert_eq!(spec.max_chars, Some(1500));
}

#[test]
fn builtin_chapters_prompt_exact() {
    assert_eq!(
        builtin("chapters").render(&vars()),
        "Divide the recording \"reunion\" (1:02:03) into chapters by topic, for the YouTube\n\
         description. Start a chapter where the speakers move to a new topic, not at every pause. Each\n\
         title names the topic in a few words that a viewer would recognize. Use only topics that are\n\
         in the input.\n"
    );
}

fn chars(n: usize) -> Limit {
    Limit {
        max_chars: Some(n),
        max: None,
    }
}

#[test]
fn builtin_youtube_title_limit() {
    assert_eq!(builtin_limit("youtube", Field::Title), chars(100));
}

#[test]
fn builtin_youtube_body_limit() {
    assert_eq!(builtin_limit("youtube", Field::Body), chars(3200));
}

#[test]
fn builtin_youtube_tags_max_limit() {
    assert_eq!(builtin_limit("youtube", Field::Tags).max, Some(15));
}

#[test]
fn builtin_youtube_tags_max_chars_limit() {
    assert_eq!(builtin_limit("youtube", Field::Tags).max_chars, Some(500));
}

#[test]
fn builtin_youtube_thumbnail_text_limit() {
    assert_eq!(builtin_limit("youtube", Field::ThumbnailText), chars(40));
}

#[test]
fn builtin_linkedin_body_limit() {
    assert_eq!(builtin_limit("linkedin", Field::Body), chars(2800));
}

#[test]
fn builtin_linkedin_hashtags_max_limit() {
    assert_eq!(builtin_limit("linkedin", Field::Hashtags).max, Some(5));
}

#[test]
fn builtin_linkedin_hashtags_max_chars_limit() {
    assert_eq!(
        builtin_limit("linkedin", Field::Hashtags).max_chars,
        Some(150)
    );
}

#[test]
fn builtin_instagram_body_limit() {
    assert_eq!(builtin_limit("instagram", Field::Body), chars(1800));
}

#[test]
fn builtin_instagram_hashtags_max_limit() {
    assert_eq!(builtin_limit("instagram", Field::Hashtags).max, Some(30));
}

#[test]
fn builtin_instagram_hashtags_max_chars_limit() {
    assert_eq!(
        builtin_limit("instagram", Field::Hashtags).max_chars,
        Some(390)
    );
}

#[test]
fn social_builtins_read_transcript() {
    for name in ["linkedin", "instagram", "youtube"] {
        let t = builtin(name);
        assert_eq!(t.input, Input::Transcript, "{name}");
        assert!(!t.body.contains("summary"), "{name}: {}", t.body);
    }
}

#[test]
fn social_run_creates_no_summary() {
    let e = env();
    let c = discover(&Dirs {
        user: None,
        project: e.project.clone(),
    })
    .unwrap();
    let deps_of = |name: &str| {
        c.get(name).ok().map(|t| match &t.input {
            Input::Transcript => Vec::new(),
            Input::Template(dep) => vec![dep.clone()],
        })
    };
    let requested: Vec<String> = ["linkedin", "instagram", "youtube"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let order = crate::template::graph::order(&requested, &deps_of).unwrap();
    assert_eq!(order, requested);
    let order = crate::template::graph::order(&["summary".to_string()], &deps_of).unwrap();
    assert_eq!(order, ["summary"]);
}
