use super::*;

#[test]
fn the_prompt_names_the_source_the_files_and_the_destination() {
    let d = Dest::new();
    let s = server();
    let mut c = case(names(&["alpha", "beta"]));
    c.interactive = true;
    c.input = "y\n";
    let out = c.run(&s, &d.dir());
    out.result.unwrap();
    assert_eq!(
        out.stderr,
        format!(
            "minuta will download from:\n  {}/\n  alpha.md  (new)\n  beta.md   (new)\ninto {}\nContinue? [y/N] alpha.md  written\nbeta.md   written\n",
            s.base(),
            d.dir().display()
        )
    );
    assert_eq!(
        out.stdout,
        format!(
            "{}\n{}\n",
            d.dir().join("alpha.md").display(),
            d.dir().join("beta.md").display()
        )
    );
}

#[test]
fn yes_and_yes_in_words_both_confirm() {
    for answer in ["y\n", "Y\n", "yes\n", "YES\n", "  yes  \n"] {
        let d = Dest::new();
        let s = server();
        let mut c = case(names(&["alpha"]));
        c.interactive = true;
        c.input = answer;
        c.run(&s, &d.dir()).result.unwrap();
        assert!(d.dir().join("alpha.md").is_file(), "answer {answer:?}");
    }
}

#[test]
fn any_other_answer_cancels_and_downloads_no_file() {
    for answer in ["n\n", "\n", "no\n", "yy\n", "ok\n"] {
        let d = Dest::new();
        let s = server();
        let mut c = case(names(&["alpha"]));
        c.interactive = true;
        c.input = answer;
        let out = c.run(&s, &d.dir());
        out.result.unwrap();
        assert_eq!(
            out.stderr,
            format!(
                "minuta will download from:\n  {}/\n  alpha.md  (new)\ninto {}\nContinue? [y/N] cancelled\n",
                s.base(),
                d.dir().display()
            ),
            "answer {answer:?}"
        );
        assert_eq!(out.stdout, "", "answer {answer:?}");
        assert_eq!(s.requests(), vec!["/templates/index.toml"], "{answer:?}");
        assert!(!d.dir().exists(), "answer {answer:?}");
    }
}

#[test]
fn yes_skips_the_prompt_even_on_a_terminal() {
    let d = Dest::new();
    let s = server();
    let mut c = case(names(&["alpha"]));
    c.interactive = true;
    c.yes = true;
    c.input = "n\n"; // would cancel if it were read
    let out = c.run(&s, &d.dir());
    out.result.unwrap();
    assert_eq!(out.stderr, "alpha.md  written\n");
    assert!(d.dir().join("alpha.md").is_file());
}

#[test]
fn when_there_is_nothing_to_download_it_does_not_ask() {
    let d = Dest::new();
    d.pulled("alpha", &template_text("alpha"));
    let s = server();
    let mut c = case(names(&["alpha"]));
    c.interactive = true;
    c.input = "n\n";
    let out = c.run(&s, &d.dir());
    out.result.unwrap();
    assert_eq!(out.stderr, "alpha.md  up to date\nnothing to download\n");
    assert_eq!(out.stdout, "");
    assert_eq!(s.requests(), vec!["/templates/index.toml"]);
}

#[test]
fn a_modified_file_is_listed_as_skipped_and_kept_without_force() {
    let d = Dest::new();
    d.pulled("beta", "an older beta\n");
    d.write("beta.md", "my edited beta\n");
    let s = server();
    let mut c = case(names(&["alpha", "beta"]));
    c.interactive = true;
    c.input = "y\n";
    let out = c.run(&s, &d.dir());
    out.result.unwrap();
    assert_eq!(
        out.stderr,
        format!(
            "minuta will download from:\n  {}/\n  alpha.md  (new)\n  beta.md   (modified: skipped, use --force to overwrite)\ninto {}\nContinue? [y/N] alpha.md  written\nbeta.md   skipped (modified; use --force to overwrite)\n",
            s.base(),
            d.dir().display()
        )
    );
    assert_eq!(
        out.stdout,
        format!("{}\n", d.dir().join("alpha.md").display())
    );
    assert_eq!(d.read("beta.md"), "my edited beta\n");
    assert!(!d.dir().join("beta.md.bak").exists());
}

#[test]
fn an_update_is_offered_without_force() {
    let d = Dest::new();
    d.pulled("beta", "an older beta\n");
    let s = server();
    let mut c = case(names(&["beta"]));
    c.interactive = true;
    c.input = "y\n";
    let out = c.run(&s, &d.dir());
    out.result.unwrap();
    assert_eq!(
        out.stderr,
        format!(
            "minuta will download from:\n  {}/\n  beta.md  (update available)\ninto {}\nContinue? [y/N] beta.md  updated\n",
            s.base(),
            d.dir().display()
        )
    );
    assert_eq!(d.read("beta.md"), template_text("beta"));
}

#[test]
fn force_replaces_a_modified_file_and_says_where_the_backup_is() {
    let d = Dest::new();
    d.pulled("beta", "an older beta\n");
    d.write("beta.md", "my edited beta\n");
    let s = server();
    let mut c = case(names(&["beta"]));
    c.force = true;
    c.interactive = true;
    c.input = "y\n";
    let out = c.run(&s, &d.dir());
    out.result.unwrap();
    assert_eq!(
        out.stderr,
        format!(
            "minuta will download from:\n  {}/\n  beta.md  (modified: replaced, old file saved as beta.md.bak)\ninto {}\nContinue? [y/N] beta.md  overwritten (backup: beta.md.bak)\n",
            s.base(),
            d.dir().display()
        )
    );
    assert_eq!(d.read("beta.md"), template_text("beta"));
    assert_eq!(d.read("beta.md.bak"), "my edited beta\n");
    assert_eq!(
        out.stdout,
        format!("{}\n", d.dir().join("beta.md").display())
    );
}

#[test]
fn the_manifest_records_what_was_written() {
    let d = Dest::new();
    let s = server();
    let mut c = case(names(&["alpha"]));
    c.yes = true;
    c.run(&s, &d.dir()).result.unwrap();
    let (manifest, warning) = Manifest::parse(&d.read(".pulled.toml"));
    assert_eq!(warning, None);
    assert_eq!(manifest.get("alpha"), Some(sha("alpha").as_str()));
}

#[test]
fn a_manifest_that_cannot_be_trusted_is_reported_and_nothing_is_overwritten() {
    let d = Dest::new();
    d.write("alpha.md", "my alpha\n");
    d.write(".pulled.toml", "version = [");
    let s = server();
    let mut c = case(names(&["alpha"]));
    c.yes = true;
    let out = c.run(&s, &d.dir());
    out.result.unwrap();
    assert!(
        out.stderr
            .starts_with("warning: invalid manifest, treating every file as modified: "),
        "{}",
        out.stderr
    );
    assert!(
        out.stderr.ends_with(
            "alpha.md  skipped (modified; use --force to overwrite)\nnothing to download\n"
        ),
        "{}",
        out.stderr
    );
    assert_eq!(d.read("alpha.md"), "my alpha\n");
}

#[test]
fn the_destination_in_the_prompt_is_made_safe_to_print() {
    let tmp = tempfile::tempdir().unwrap();
    let dest = tmp.path().join("a\u{1b}[2Jb").join("templates");
    let s = server();
    let mut c = case(names(&["alpha"]));
    c.interactive = true;
    c.input = "n\n";
    let out = c.run(&s, &dest);
    out.result.unwrap();
    assert!(
        out.stderr
            .contains(&format!("into {}/a?[2Jb/templates\n", tmp.path().display())),
        "{:?}",
        out.stderr
    );
    assert!(!out.stderr.contains('\u{1b}'), "{:?}", out.stderr);
}

#[test]
fn end_of_input_at_the_confirmation_ends_the_prompt_line_and_cancels() {
    let d = Dest::new();
    let s = server();
    let mut c = case(names(&["alpha"]));
    c.interactive = true;
    c.input = "";
    let out = c.run(&s, &d.dir());
    out.result.unwrap();
    assert_eq!(
        out.stderr,
        format!(
            "minuta will download from:\n  {}/\n  alpha.md  (new)\ninto {}\nContinue? [y/N] \ncancelled\n",
            s.base(),
            d.dir().display()
        )
    );
    assert!(!d.dir().exists());
}
