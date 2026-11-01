//! Helpers shared by the backends that run a CLI as a subprocess (`claude`, `codex`).

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::error::Result;
use crate::tools::{self, RunOpts};

const WATCH_INTERVAL: Duration = Duration::from_millis(10);

/// A fresh, empty, private directory, removed on drop. Cleanup is best effort on every
/// path: a failure to remove it never discards a successful reply and never masks the
/// primary error.
pub(super) struct ScratchDir(PathBuf);

impl ScratchDir {
    pub(super) fn create(prefix: &str) -> std::io::Result<ScratchDir> {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let name = format!(
            "{prefix}-{}-{nanos}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        );
        let path = std::env::temp_dir().join(name);
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&path)?;
        Ok(ScratchDir(path))
    }

    pub(super) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Sets the flag on drop, so the watcher stops even if the call panics.
struct SetOnDrop<'a>(&'a AtomicBool);

impl Drop for SetOnDrop<'_> {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

/// `tools` wants an owned flag, the trait hands over a borrowed one: a watcher thread
/// mirrors the caller's flag for the duration of the call.
pub(super) fn run_interruptible(
    argv: &[OsString],
    stdin: &[u8],
    cwd: &Path,
    interrupt: &AtomicBool,
) -> Result<String> {
    let mirror = Arc::new(AtomicBool::new(false));
    let done = AtomicBool::new(false);
    let opts = RunOpts {
        cwd: Some(cwd.to_path_buf()),
        interrupt: Some(Arc::clone(&mirror)),
    };
    std::thread::scope(|scope| {
        scope.spawn(|| {
            while !done.load(Ordering::SeqCst) {
                if interrupt.load(Ordering::SeqCst) {
                    mirror.store(true, Ordering::SeqCst);
                    return;
                }
                std::thread::sleep(WATCH_INTERVAL);
            }
        });
        let _stop = SetOnDrop(&done);
        tools::run_capture(argv, stdin, opts, &mut |_| {})
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scratch_dir_is_private_and_removed_on_drop() {
        let scratch = ScratchDir::create("minuta-test").unwrap();
        let path = scratch.path().to_path_buf();
        assert!(path.is_dir());
        assert_eq!(std::fs::read_dir(&path).unwrap().count(), 0);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o700);
        }
        drop(scratch);
        assert!(!path.exists());
    }

    #[test]
    fn scratch_dir_name_starts_with_prefix_and_is_unique() {
        let a = ScratchDir::create("minuta-codex").unwrap();
        let b = ScratchDir::create("minuta-codex").unwrap();
        let name = |s: &ScratchDir| s.path().file_name().unwrap().to_string_lossy().into_owned();
        assert!(name(&a).starts_with("minuta-codex-"), "{}", name(&a));
        assert_ne!(a.path(), b.path());
    }
}
