use std::fs;

use crate::job::Step;
use crate::steps::outputs::Outputs;
use crate::steps::test_support::{names_in, run_step, setup, step, FIXTURES};
use crate::transcript::{self, Segment, Transcript};

#[test]
fn outputs_derive_from_transcript_json() {
    let s = setup(None, 0);
    let t = Transcript {
        language: "en".into(),
        segments: vec![Segment {
            start: 0.0,
            end: 1.5,
            text: "only in transcript json".into(),
        }],
    };
    fs::write(
        s.ctx.dir.join("assets/transcript.json"),
        serde_json::to_string(&t).unwrap(),
    )
    .unwrap();
    // A raw whisper file with different content must be ignored.
    fs::create_dir_all(s.ctx.dir.join(".minuta")).unwrap();
    fs::copy(
        format!("{FIXTURES}/sample.json"),
        s.ctx.dir.join(".minuta/whisper-raw.json"),
    )
    .unwrap();

    assert_eq!(
        Outputs.outputs(&s.ctx),
        [
            s.ctx.dir.join("transcript.txt"),
            s.ctx.dir.join("captions.srt")
        ]
    );
    Outputs.run(&s.ctx).unwrap();
    assert_eq!(
        fs::read_to_string(s.ctx.dir.join("transcript.txt")).unwrap(),
        transcript::to_txt(&t)
    );
    assert_eq!(
        fs::read_to_string(s.ctx.dir.join("captions.srt")).unwrap(),
        transcript::to_srt(&t)
    );
}

#[test]
fn transcript_txt_and_srt_stay_at_root() {
    let s = setup(Some("sample.json"), 0);
    run_step(&step(&s, "auto"), &s.ctx, &mut |_| {}).unwrap();
    Outputs.run(&s.ctx).unwrap();

    assert_eq!(
        names_in(&s.ctx.dir),
        [".minuta", "assets", "captions.srt", "transcript.txt"]
    );
    assert_eq!(
        names_in(&s.ctx.dir.join("assets")),
        ["audio.wav", "transcript.json"]
    );
}

#[test]
fn outputs_fail_without_transcript_json() {
    let s = setup(None, 0);
    assert!(Outputs.run(&s.ctx).is_err());
    assert!(!s.ctx.dir.join("transcript.txt").exists());
    assert!(!s.ctx.dir.join("captions.srt").exists());
}

#[cfg(unix)]
#[test]
fn outputs_do_not_follow_a_planted_tmp_symlink() {
    let s = setup(None, 0);
    let t = Transcript {
        language: "en".into(),
        segments: vec![Segment {
            start: 0.0,
            end: 1.5,
            text: "new text".into(),
        }],
    };
    fs::write(
        s.ctx.dir.join("assets/transcript.json"),
        serde_json::to_string(&t).unwrap(),
    )
    .unwrap();
    let victim = s._tmp.path().join("victim.txt");
    fs::write(&victim, "precious").unwrap();
    for name in ["transcript.txt.tmp", "captions.srt.tmp"] {
        std::os::unix::fs::symlink(&victim, s.ctx.dir.join(name)).unwrap();
    }

    Outputs.run(&s.ctx).unwrap();

    assert_eq!(fs::read_to_string(&victim).unwrap(), "precious");
    assert_eq!(
        fs::read_to_string(s.ctx.dir.join("transcript.txt")).unwrap(),
        transcript::to_txt(&t)
    );
    assert_eq!(
        fs::read_to_string(s.ctx.dir.join("captions.srt")).unwrap(),
        transcript::to_srt(&t)
    );
}
