//! `scripts/worktrees-clean.sh` against a scratch repo and a stub `gh`.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(dir)
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

struct Fixture {
    _tmp: TempDir,
    repo: std::path::PathBuf,
    stub: std::path::PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let tmp = TempDir::new().unwrap();
        // git prints resolved paths (macOS /var -> /private/var).
        let root = tmp.path().canonicalize().unwrap();
        let repo = root.join("repo");
        let stub = root.join("stub");
        fs::create_dir_all(&repo).unwrap();
        fs::create_dir_all(&stub).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["commit", "-q", "--allow-empty", "-m", "init"]);
        // The stub prints what the script's `--jq` would print for the branch
        // after `--head`: `<STATE> <headRefOid>`, read from `stub/<branch>`.
        // It fails without `--state all`, as real gh then lists only open PRs.
        let gh = stub.join("gh");
        fs::write(
            &gh,
            "#!/bin/sh\ncase \" $* \" in *\" --state all \"*) ;; *) exit 1;; esac\nwhile [ \"$1\" != --head ]; do shift; done\ncat \"$(dirname \"$0\")/$2\" 2>/dev/null || echo NONE\n",
        )
        .unwrap();
        fs::set_permissions(&gh, fs::Permissions::from_mode(0o755)).unwrap();
        Fixture {
            _tmp: tmp,
            repo,
            stub,
        }
    }

    fn worktree(&self, name: &str) -> std::path::PathBuf {
        let path = self.repo.join(".worktrees").join(name);
        git(
            &self.repo,
            &["worktree", "add", "-q", path.to_str().unwrap(), "-b", name],
        );
        path
    }

    /// Record the PR `state` for `branch`, with the head the PR had at `oid`.
    fn pr(&self, branch: &str, state: &str, oid: &str) {
        fs::write(self.stub.join(branch), format!("{state} {oid}\n")).unwrap();
    }

    fn tip(&self, branch: &str) -> String {
        git(&self.repo, &["rev-parse", branch])
    }

    fn run(&self) -> String {
        self.run_in(&self.repo)
    }

    fn run_in(&self, cwd: &Path) -> String {
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/worktrees-clean.sh");
        let path = format!("{}:{}", self.stub.display(), std::env::var("PATH").unwrap());
        let out = Command::new(script)
            .current_dir(cwd)
            .env("PATH", path)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }

    fn has_branch(&self, branch: &str) -> bool {
        !git(&self.repo, &["branch", "--list", branch]).is_empty()
    }
}

#[test]
fn removes_worktree_and_branch_when_pr_merged_or_closed() {
    for state in ["MERGED", "CLOSED"] {
        let f = Fixture::new();
        let wt = f.worktree("done");
        f.pr("done", state, &f.tip("done"));
        let out = f.run();
        assert!(!wt.exists(), "{state}: {out}");
        assert!(!f.has_branch("done"), "{state}: {out}");
        assert!(
            out.contains(&format!("remove {}", wt.display())),
            "{state}: {out}"
        );
    }
}

#[test]
fn keeps_worktree_with_open_pr() {
    let f = Fixture::new();
    let wt = f.worktree("open");
    f.pr("open", "OPEN", &f.tip("open"));
    let out = f.run();
    assert!(wt.exists() && f.has_branch("open"), "{out}");
    assert!(
        out.contains(&format!("keep   {} (PR OPEN)", wt.display())),
        "{out}"
    );
}

#[test]
fn keeps_worktree_without_pr() {
    let f = Fixture::new();
    let wt = f.worktree("nopr");
    let out = f.run();
    assert!(wt.exists() && f.has_branch("nopr"), "{out}");
    assert!(
        out.contains(&format!("keep   {} (PR NONE)", wt.display())),
        "{out}"
    );
}

#[test]
fn keeps_worktree_with_uncommitted_changes() {
    let f = Fixture::new();
    let wt = f.worktree("dirty");
    f.pr("dirty", "MERGED", &f.tip("dirty"));
    fs::write(wt.join("untracked"), "x").unwrap();
    let out = f.run();
    assert!(wt.exists() && f.has_branch("dirty"), "{out}");
    assert!(
        out.contains(&format!("keep   {} (uncommitted changes)", wt.display())),
        "{out}"
    );
}

#[test]
fn keeps_locked_worktree() {
    let f = Fixture::new();
    let wt = f.worktree("locked");
    f.pr("locked", "MERGED", &f.tip("locked"));
    git(&f.repo, &["worktree", "lock", wt.to_str().unwrap()]);
    let out = f.run();
    assert!(wt.exists() && f.has_branch("locked"), "{out}");
    assert!(
        out.contains(&format!("keep   {} (locked)", wt.display())),
        "{out}"
    );
}

#[test]
fn keeps_detached_worktree() {
    let f = Fixture::new();
    let wt = f.repo.join(".worktrees/detached");
    git(
        &f.repo,
        &["worktree", "add", "-q", "--detach", wt.to_str().unwrap()],
    );
    let out = f.run();
    assert!(wt.exists(), "{out}");
    assert!(
        out.contains(&format!("keep   {} (detached HEAD)", wt.display())),
        "{out}"
    );
}

#[test]
fn keeps_branch_with_commits_the_pr_never_had() {
    let f = Fixture::new();
    let wt = f.worktree("reused");
    f.pr("reused", "MERGED", &f.tip("reused"));
    git(
        &wt,
        &["commit", "-q", "--allow-empty", "-m", "after the PR"],
    );
    let out = f.run();
    assert!(wt.exists() && f.has_branch("reused"), "{out}");
    assert!(
        out.contains(&format!(
            "keep   {} (local commits not in PR)",
            wt.display()
        )),
        "{out}"
    );
}

#[test]
fn removes_the_worktree_it_was_started_from() {
    let f = Fixture::new();
    let wt = f.worktree("current");
    f.pr("current", "MERGED", &f.tip("current"));
    let out = f.run_in(&wt);
    assert!(!wt.exists() && !f.has_branch("current"), "{out}");
}

#[test]
fn never_removes_the_main_worktree() {
    let f = Fixture::new();
    f.pr("main", "MERGED", &f.tip("main"));
    let out = f.run();
    assert!(f.repo.exists() && f.has_branch("main"), "{out}");
    assert_eq!(out, "");
}

#[test]
fn prunes_worktree_whose_directory_is_gone() {
    let f = Fixture::new();
    let wt = f.worktree("gone");
    fs::remove_dir_all(&wt).unwrap();
    f.run();
    assert!(!git(&f.repo, &["worktree", "list"]).contains("gone"));
}
