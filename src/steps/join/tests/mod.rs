mod with_ffmpeg;
mod without_ffmpeg;

use super::*;
use crate::steps::probe::{AudioStream, VideoStream};

#[test]
fn concat_list_escapes_single_quotes() {
    let list = concat_list(&[PathBuf::from("/a/it's.mp4"), PathBuf::from("/b.mp4")]).unwrap();
    assert_eq!(list, "file '/a/it'\\''s.mp4'\nfile '/b.mp4'\n");
}

#[test]
fn concat_list_rejects_line_breaks_and_non_utf8() {
    for bad in ["/a\nb.mp4", "/a\rb.mp4"] {
        let err = concat_list(&[PathBuf::from(bad)]).unwrap_err();
        assert!(matches!(err, Error::Usage(_)), "{bad:?}");
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        let bytes = OsString::from_vec(b"/a\xff.mp4".to_vec());
        let err = concat_list(&[PathBuf::from(bytes)]).unwrap_err();
        assert!(matches!(err, Error::Usage(_)));
    }
}

#[test]
fn muxer_follows_the_extension() {
    let muxer = |name: &str| muxer_for(Path::new(name));
    assert_eq!(muxer("joined.mp4"), "mp4");
    assert_eq!(muxer("joined.mov"), "mov");
    assert_eq!(muxer("joined.mkv"), "matroska");
    assert_eq!(muxer("joined.webm"), "webm");
    assert_eq!(muxer("joined.m4a"), "ipod");
    assert_eq!(muxer("joined.wav"), "wav");
    assert_eq!(muxer("joined.xyz"), "matroska");
}

#[test]
fn audio_codec_fits_the_container() {
    let codec = |name: &str| audio_codec_for(Path::new(name));
    assert_eq!(codec("joined.wav"), "pcm_s16le");
    assert_eq!(codec("joined.flac"), "flac");
    assert_eq!(codec("joined.mp3"), "libmp3lame");
    assert_eq!(codec("joined.m4a"), "aac");
}

/// A 320x240 25 fps mpeg4 source with aac audio at `sample_rate` Hz and `channels`.
fn source(sample_rate: u32, channels: u32) -> SourceProbe {
    SourceProbe {
        path: PathBuf::from("/a.mp4"),
        duration: 1.0,
        video: Some(VideoStream {
            codec: "mpeg4".into(),
            width: 320,
            height: 240,
            fps: "25/1".into(),
        }),
        audio: AudioStream {
            codec: "aac".into(),
            sample_rate,
            channels,
        },
    }
}

#[test]
fn plan_join_has_nothing_to_join_for_fewer_than_two_sources() {
    assert_eq!(plan_join(&[]), None);
    assert_eq!(plan_join(&[source(48000, 2)]), None);
}

#[test]
fn plan_join_matching_streams_stream_copy() {
    assert_eq!(
        plan_join(&[source(48000, 2), source(48000, 2)]),
        Some(Mode::StreamCopy)
    );
}

#[test]
fn plan_join_differing_sample_rate_reencodes() {
    assert_eq!(
        plan_join(&[source(48000, 2), source(44100, 2)]),
        Some(Mode::Reencode)
    );
}

#[test]
fn plan_join_differing_channels_reencodes() {
    assert_eq!(
        plan_join(&[source(48000, 2), source(48000, 1)]),
        Some(Mode::Reencode)
    );
}

#[test]
fn plan_join_differing_audio_codec_reencodes() {
    let mut mp3 = source(48000, 2);
    mp3.audio.codec = "mp3".into();
    assert_eq!(plan_join(&[source(48000, 2), mp3]), Some(Mode::Reencode));
}
