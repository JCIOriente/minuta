use minuta::error::Error;
use minuta::transcript::{from_whisper_json, to_srt, to_txt, Segment, Transcript};

const SAMPLE: &str = include_str!("fixtures/whisper/sample.json");
const EMPTY: &str = include_str!("fixtures/whisper/empty.json");
const CLASS: &str = include_str!("fixtures/whisper/class.json");

fn whisper(segments: &[(u64, u64, &str)]) -> String {
    let items: Vec<String> = segments
        .iter()
        .map(|(from, to, text)| {
            format!(r#"{{"offsets":{{"from":{from},"to":{to}}},"text":"{text}"}}"#)
        })
        .collect();
    format!(
        r#"{{"result":{{"language":"en"}},"transcription":[{}]}}"#,
        items.join(",")
    )
}

fn seg(start: f64, end: f64, text: &str) -> Segment {
    Segment {
        start,
        end,
        text: text.into(),
    }
}

fn transcript(segments: Vec<Segment>) -> Transcript {
    Transcript {
        language: "en".into(),
        segments,
    }
}

#[test]
fn parses_offsets_to_seconds() {
    let t = from_whisper_json(SAMPLE).unwrap();
    assert_eq!(t.segments.len(), 2);
    assert_eq!(t.segments[0].start, 0.0);
    assert_eq!(t.segments[0].end, 3.5);
    assert_eq!(t.segments[1].start, 3.5);
    assert_eq!(t.segments[1].end, 7.25);
    assert_eq!(t.segments[0].text, " Buenos días a todos.");
}

#[test]
fn reads_language() {
    assert_eq!(from_whisper_json(SAMPLE).unwrap().language, "es");
}

#[test]
fn overlap_is_clamped() {
    let t = from_whisper_json(&whisper(&[(0, 4000, "a"), (3000, 6000, "b")])).unwrap();
    assert_eq!(t.segments.len(), 2);
    assert_eq!(t.segments[0].end, 4.0);
    assert_eq!(t.segments[1].start, 4.0);
    assert_eq!(t.segments[1].end, 6.0);
}

#[test]
fn contained_segment_clamped_not_inverted() {
    let t = from_whisper_json(&whisper(&[
        (0, 4000, "a"),
        (1000, 3000, "b"),
        (5000, 6000, "c"),
    ]))
    .unwrap();
    let mut prev_end = 0.0;
    for s in &t.segments {
        assert!(s.end >= s.start, "inverted: {s:?}");
        assert!(s.start >= prev_end, "overlap: {s:?}");
        prev_end = s.end;
    }
    let spans: Vec<(&str, f64, f64)> = t
        .segments
        .iter()
        .map(|s| (s.text.as_str(), s.start, s.end))
        .collect();
    assert_eq!(spans, [("a", 0.0, 4.0), ("b", 4.0, 4.0), ("c", 5.0, 6.0)]);
}

#[test]
fn inverted_segment_dropped() {
    let t = from_whisper_json(&whisper(&[
        (0, 1000, "a"),
        (5000, 2000, "bad"),
        (3000, 4000, "c"),
    ]))
    .unwrap();
    let texts: Vec<&str> = t.segments.iter().map(|s| s.text.as_str()).collect();
    assert_eq!(texts, ["a", "c"]);
    assert_eq!(t.segments[1].start, 3.0);
    assert_eq!(t.segments[1].end, 4.0);
}

#[test]
fn empty_segments_ok() {
    let t = from_whisper_json(EMPTY).unwrap();
    assert!(t.segments.is_empty());
    assert_eq!(t.language, "en");
}

#[test]
fn malformed_json_is_parse_error() {
    assert!(matches!(from_whisper_json("{"), Err(Error::Parse(_))));
}

#[test]
fn srt_timestamp_format() {
    let t = transcript(vec![seg(0.0, 3.5, "one"), seg(3.5, 7.25, "two")]);
    assert_eq!(
        to_srt(&t),
        "1\n00:00:00,000 --> 00:00:03,500\none\n\n2\n00:00:03,500 --> 00:00:07,250\ntwo\n"
    );
}

#[test]
fn srt_over_one_hour() {
    let t = transcript(vec![seg(3661.007, 3725.5, "late")]);
    assert_eq!(to_srt(&t), "1\n01:01:01,007 --> 01:02:05,500\nlate\n");
}

#[test]
fn srt_preserves_utf8() {
    let t = transcript(vec![
        seg(0.0, 1.0, "café ñandú"),
        seg(1.0, 2.0, "你好，世界"),
    ]);
    let srt = to_srt(&t);
    assert!(srt.contains("café ñandú"));
    assert!(srt.contains("你好，世界"));
}

#[test]
fn txt_one_line_per_segment() {
    let t = transcript(vec![seg(0.0, 1.0, " hello "), seg(1.0, 2.0, "world\n")]);
    assert_eq!(to_txt(&t), "hello\nworld\n");
}

#[test]
fn json_roundtrip() {
    let t = transcript(vec![seg(0.0, 1.5, "hi")]);
    let json = serde_json::to_value(&t).unwrap();
    assert_eq!(
        json,
        serde_json::json!({
            "language": "en",
            "segments": [{"start": 0.0, "end": 1.5, "text": "hi"}]
        })
    );
    let back: Transcript = serde_json::from_value(json).unwrap();
    assert_eq!(back, t);
}

#[test]
fn class_fixture_parses() {
    let t = from_whisper_json(CLASS).unwrap();
    assert_eq!(t.language, "es");
    assert_eq!(t.segments.len(), 25);
    assert_eq!(t.segments[24].end, 335.5);
    assert!(t.segments.windows(2).all(|w| w[0].start < w[1].start));
    assert!(t
        .segments
        .windows(2)
        .any(|w| w[1].start - w[0].start < 10.0));
    assert_eq!(
        t.segments[4],
        seg(32.5, 55.0, " Muy bien. La propiedad es el conjunto de reglas con el que Rust gestiona la memoria sin recolector de basura. Cada valor tiene un único dueño, y cuando el dueño sale de su ámbito, el valor se libera automáticamente.")
    );
}
