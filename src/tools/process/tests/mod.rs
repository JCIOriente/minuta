use super::*;

#[cfg(unix)]
mod run;

/// Hands out one chunk per `read`, like a pipe whose writer wrote them separately.
struct Chunks(std::collections::VecDeque<&'static [u8]>);

impl Read for Chunks {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let Some(chunk) = self.0.pop_front() else {
            return Ok(0);
        };
        buf[..chunk.len()].copy_from_slice(chunk);
        Ok(chunk.len())
    }
}

/// The stderr tail that `drain` would keep for `chunks`.
fn tail_of(chunks: &[&'static [u8]]) -> Vec<String> {
    let (tx, rx) = mpsc::channel();
    pump(Chunks(chunks.iter().copied().collect()), true, tx)
        .join()
        .unwrap();
    let mut tail = Tail::default();
    for msg in rx {
        match msg {
            (_, Ok(Out::Piece(Piece { text, update }))) => tail.push(text, update),
            (_, Ok(Out::LastWasLine)) => tail.settle(),
            (_, Err(e)) => panic!("read error {e}"),
        }
    }
    tail.lines
}

#[test]
fn crlf_split_across_reads_stays_in_tail() {
    assert_eq!(
        tail_of(&[b"Error opening output\r", b"\nframe=1\r", b"frame=2\r"]),
        ["Error opening output", "frame=2"]
    );
}

/// Real `ffmpeg -stats` stderr of a re-encoding join (ffmpeg 9.0.2), trimmed.
const FFMPEG_JOIN_STATS: &[u8] =
    include_bytes!("../../../../tests/fixtures/ffmpeg/join-stats.stderr");

#[test]
fn ffmpeg_stats_kept_out_of_tail() {
    assert_eq!(
        tail_of(&[FFMPEG_JOIN_STATS, b"Conversion failed!\n"]),
        [
            "      encoder         : Lavc63.1.102 aac",
            "[out#0/mp4 @ 0x79794106c0] video:558KiB audio:507KiB subtitle:0KiB other \
             streams:0KiB global headers:0KiB muxing overhead: 4.741262%",
            "[libx264 @ 0x797945df80] frame I:6     Avg QP:10.53  size:  8473",
            "Conversion failed!",
        ]
    );
}

#[test]
fn try_wait_error_kills_child() {
    use std::os::unix::process::ExitStatusExt;
    let program = OsStr::new("/bin/sleep");
    let mut cmd = Command::new(program);
    cmd.arg("30");
    let mut child = spawn(program, &mut cmd, &RunOpts::default()).unwrap();
    let err = wait_status_with(&mut child, &RunOpts::default(), |_| {
        Err(std::io::Error::other("injected"))
    })
    .unwrap_err();
    match err {
        Error::Io(e) => assert_eq!(e.to_string(), "injected"),
        other => panic!("expected Io error, got {other:?}"),
    }
    // Reaped: the cached status says SIGKILL.
    let status = child.try_wait().unwrap();
    assert_eq!(status.and_then(|s| s.signal()), Some(9));
}
