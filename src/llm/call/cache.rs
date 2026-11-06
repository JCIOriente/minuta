//! Per-job cache of cleaned LLM outputs: keys, entry reads, atomic writes and temp cleanup.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::{Error, Result};
use crate::job::{fingerprint, layout, reject_symlinked_tier};
use crate::llm::Backend;

/// Makes temp names unique across threads of this process.
static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

fn with_path(path: &Path, e: io::Error) -> Error {
    Error::Io(io::Error::new(e.kind(), format!("{}: {e}", path.display())))
}

/// Per-job cache of cleaned LLM outputs in `<job>/.minuta/llm-cache`.
pub struct CallCache {
    dir: PathBuf,
}

impl CallCache {
    pub fn new(job_dir: &Path) -> CallCache {
        CallCache {
            dir: layout::call_cache_dir(job_dir),
        }
    }

    /// The folder holding the cached entries.
    #[cfg(test)]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn entry(&self, key: &str) -> PathBuf {
        self.dir.join(format!("{key}.txt"))
    }

    /// An empty entry counts as a miss: only non-empty text is ever stored.
    pub(super) fn read(&self, key: &str) -> Result<Option<String>> {
        let path = self.entry(key);
        match fs::read_to_string(&path) {
            Ok(text) if text.is_empty() => Ok(None),
            Ok(text) => Ok(Some(text)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(with_path(&path, e)),
        }
    }

    /// Writes to a temp file unique to this call, syncs it and renames it into place, so
    /// concurrent writers of one key never share a temp file.
    pub(super) fn write(&self, key: &str, text: &str) -> Result<()> {
        for dir in self.dir.ancestors().take(2) {
            reject_symlinked_tier(dir)?;
        }
        fs::create_dir_all(&self.dir).map_err(|e| with_path(&self.dir, e))?;
        let path = self.entry(key);
        let tmp = self.dir.join(format!(
            "{key}.{}-{}.tmp",
            std::process::id(),
            TMP_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let result = (|| {
            let mut f = fs::File::create(&tmp).map_err(|e| with_path(&tmp, e))?;
            f.write_all(text.as_bytes())
                .and_then(|()| f.sync_all())
                .map_err(|e| with_path(&tmp, e))?;
            fs::rename(&tmp, &path).map_err(|e| with_path(&path, e))
        })();
        if result.is_ok() {
            // Best effort: persist the rename. Some platforms cannot sync a directory.
            let _ = fs::File::open(&self.dir).and_then(|d| d.sync_all());
        }
        if result.is_err() {
            // Best effort: `clean_tmp` removes whatever survives.
            let _ = fs::remove_file(&tmp);
        }
        result
    }

    /// Removes leftover `*.tmp` files in `llm-cache`; call only when no calls are in flight
    /// (at the start of a run). A missing cache dir is fine.
    pub fn clean_tmp(&self) -> Result<()> {
        for dir in self.dir.ancestors().take(2) {
            reject_symlinked_tier(dir)?;
        }
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(with_path(&self.dir, e)),
        };
        for entry in entries {
            let path = entry.map_err(|e| with_path(&self.dir, e))?.path();
            if path.extension().is_some_and(|ext| ext == "tmp") {
                fs::remove_file(&path).map_err(|e| with_path(&path, e))?;
            }
        }
        Ok(())
    }
}

pub(super) fn cache_key(backend: &dyn Backend, system: &str, user: &str) -> String {
    key(backend, system, user, &[])
}

/// Fingerprint of (backend id, context tokens, system, user) followed by `extra` parts.
fn key(backend: &dyn Backend, system: &str, user: &str, extra: &[&[u8]]) -> String {
    let context = backend.context_tokens().to_string();
    let mut parts: Vec<&[u8]> = vec![
        backend.id().as_bytes(),
        context.as_bytes(),
        system.as_bytes(),
        user.as_bytes(),
    ];
    parts.extend_from_slice(extra);
    fingerprint(&parts)
}

/// Cache key of a `complete_json` call: the `complete` parts plus the compact schema JSON, so
/// it never collides with a `complete` key and changes with the schema.
pub(super) fn json_cache_key(
    backend: &dyn Backend,
    system: &str,
    user: &str,
    schema: &serde_json::Value,
) -> String {
    key(backend, system, user, &[schema.to_string().as_bytes()])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::call::test_support::{backend, policy, run, schema};
    use crate::llm::fake::ScriptedBackend;

    #[test]
    fn cache_key_sensitive_to_each_part() {
        let base = ScriptedBackend::new("fake:x", 1000, vec![]);
        let k = cache_key(&base, "sys", "usr");
        let other_id = ScriptedBackend::new("fake:y", 1000, vec![]);
        let other_ctx = ScriptedBackend::new("fake:x", 2000, vec![]);
        assert_ne!(k, cache_key(&other_id, "sys", "usr"), "backend id");
        assert_ne!(k, cache_key(&other_ctx, "sys", "usr"), "context tokens");
        assert_ne!(k, cache_key(&base, "sys2", "usr"), "system");
        assert_ne!(k, cache_key(&base, "sys", "usr2"), "user");
        assert_eq!(k, cache_key(&base, "sys", "usr"), "stable");
    }

    #[test]
    fn complete_cache_key_unchanged() {
        // Computed on main before `call_json` existed: existing job folders keep their cache.
        let b = ScriptedBackend::new("fake:x", 1000, vec![]);
        assert_eq!(
            cache_key(&b, "s", "u"),
            "67d3590845e650cb4c38fd859fea4e204e5ab8ec39b79402422d9057f2b31226"
        );
    }

    #[test]
    fn cache_read_error_is_error() {
        let dir = tempfile::tempdir().unwrap();
        let cache = CallCache::new(dir.path());
        let b = backend(vec![Ok("x".into())]);
        let key = cache_key(&b, "sys", "usr");
        // A directory in place of the entry: reading fails with something other than NotFound.
        fs::create_dir_all(layout::call_cache_dir(dir.path()).join(format!("{key}.txt"))).unwrap();
        let err = run(&b, &cache, &policy(1), false).unwrap_err();
        assert!(matches!(err, Error::Io(_)), "{err}");
        assert!(b.calls().is_empty());
    }

    #[test]
    fn empty_cache_entry_is_a_miss() {
        let dir = tempfile::tempdir().unwrap();
        let cache = CallCache::new(dir.path());
        let b = backend(vec![Ok("fresh".into())]);
        let key = cache_key(&b, "sys", "usr");
        let cache_dir = layout::call_cache_dir(dir.path());
        fs::create_dir_all(&cache_dir).unwrap();
        fs::write(cache_dir.join(format!("{key}.txt")), "").unwrap();
        assert_eq!(run(&b, &cache, &policy(1), false).unwrap(), "fresh");
        assert_eq!(b.calls().len(), 1);
    }

    #[test]
    fn write_error_names_path() {
        let dir = tempfile::tempdir().unwrap();
        // A file where the cache dir should be.
        fs::create_dir(dir.path().join(".minuta")).unwrap();
        fs::write(layout::call_cache_dir(dir.path()), "x").unwrap();
        let cache = CallCache::new(dir.path());
        let err = cache.write("k", "text").unwrap_err();
        assert!(err.to_string().contains("llm-cache"), "{err}");
    }

    #[test]
    fn rename_error_names_entry_path() {
        let dir = tempfile::tempdir().unwrap();
        let cache = CallCache::new(dir.path());
        let entry = layout::call_cache_dir(dir.path()).join("k.txt");
        fs::create_dir_all(entry.join("child")).unwrap();
        let err = cache.write("k", "text").unwrap_err();
        assert!(err.to_string().contains("k.txt"), "{err}");
        let tmp_left = fs::read_dir(layout::call_cache_dir(dir.path()))
            .unwrap()
            .any(|e| e.unwrap().path().extension().is_some_and(|x| x == "tmp"));
        assert!(!tmp_left);
    }

    #[test]
    fn clean_tmp_refuses_a_symlinked_state_folder() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let victim = outside.path().join("llm-cache");
        fs::create_dir(&victim).unwrap();
        fs::write(victim.join("x.tmp"), "keep").unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join(".minuta")).unwrap();
        let err = CallCache::new(dir.path()).clean_tmp().unwrap_err();
        assert!(matches!(err, Error::Job(_)), "{err}");
        assert!(victim.join("x.tmp").exists());
    }

    #[test]
    fn clean_tmp_removes_only_tmp_files() {
        let dir = tempfile::tempdir().unwrap();
        let cache = CallCache::new(dir.path());
        cache.clean_tmp().unwrap(); // missing dir is fine
        cache.write("k", "text").unwrap();
        let cache_dir = layout::call_cache_dir(dir.path());
        fs::write(cache_dir.join("k.1-2.tmp"), "partial").unwrap();
        cache.clean_tmp().unwrap();
        assert!(!cache_dir.join("k.1-2.tmp").exists());
        assert_eq!(fs::read_to_string(cache_dir.join("k.txt")).unwrap(), "text");
    }

    #[test]
    fn write_leaves_no_tmp_files() {
        let dir = tempfile::tempdir().unwrap();
        let cache = CallCache::new(dir.path());
        cache.write("k", "a").unwrap();
        cache.write("k", "b").unwrap();
        let names: Vec<_> = fs::read_dir(layout::call_cache_dir(dir.path()))
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from("k.txt")]);
    }

    #[test]
    fn json_key_includes_schema() {
        let base = ScriptedBackend::new("fake:x", 1000, vec![]);
        let s = schema();
        let k = json_cache_key(&base, "sys", "usr", &s);
        let other_id = ScriptedBackend::new("fake:y", 1000, vec![]);
        let other_ctx = ScriptedBackend::new("fake:x", 2000, vec![]);
        let other_schema = serde_json::json!({"type": "object", "required": ["body"]});
        assert_ne!(k, cache_key(&base, "sys", "usr"), "complete key");
        assert_ne!(
            k,
            json_cache_key(&base, "sys", "usr", &other_schema),
            "schema"
        );
        assert_ne!(k, json_cache_key(&other_id, "sys", "usr", &s), "backend id");
        assert_ne!(k, json_cache_key(&other_ctx, "sys", "usr", &s), "context");
        assert_ne!(k, json_cache_key(&base, "sys2", "usr", &s), "system");
        assert_ne!(k, json_cache_key(&base, "sys", "usr2", &s), "user");
        assert_eq!(k, json_cache_key(&base, "sys", "usr", &s), "stable");
    }
}
