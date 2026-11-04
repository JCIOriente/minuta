use super::test_support::{request, seg, transcript, Recorder, BODY, LANG};
use super::*;

#[test]
fn blocks_span_30_seconds() {
    let t = transcript(vec![
        seg(0.0, 10.0, " a "),
        seg(10.0, 20.0, "b"),
        seg(20.0, 31.0, "c"),
        seg(31.0, 40.0, "d"),
        seg(40.0, 61.0, "e"),
        seg(61.0, 65.0, "f"),
    ]);
    assert_eq!(
        transcript_text(&t),
        "[00:00:00] a b c\n[00:00:31] d e\n[00:01:01] f"
    );
}

#[test]
fn block_timestamp_format_over_one_hour() {
    let t = transcript(vec![seg(3725.7, 3760.0, "x")]);
    assert_eq!(transcript_text(&t), "[01:02:05] x");
}

#[test]
fn empty_transcript_empty_text() {
    assert_eq!(transcript_text(&transcript(vec![])), "");
}

#[test]
fn estimate_rounds_up() {
    let cases = [
        ("", 0),
        ("a", 1),
        ("abc", 1),
        ("abcd", 2),
        ("abcdef", 2),
        ("ñññ", 1),
        ("ñññn", 2),
    ];
    for (s, want) in cases {
        assert_eq!(estimate_tokens(s), want, "{s:?}");
    }
}

#[test]
fn language_instruction_last() {
    let instruction = language_instruction(LANG);
    assert_eq!(
        instruction,
        "Write the entire response in Spanish, including headings and any placeholder \
         text, regardless of the language of the input or of these instructions."
    );
    let rec = Recorder::new();
    let complete = |s: &str, u: &str| {
        rec.record(s, u);
        Ok("out".to_string())
    };
    generate(&request("hello", 1000, 1), &complete, &mut |_| {}).unwrap();
    assert_eq!(rec.calls()[0].0, format!("{BODY}\n\n{instruction}"));
}
