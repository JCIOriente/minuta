use super::*;

#[test]
fn by_name_with_yes_writes_the_files_and_prints_only_the_paths() {
    let d = Dest::new();
    let s = server();
    let mut c = case(names(&["beta", "alpha"]));
    c.yes = true;
    let out = c.run(&s, &d.dir());
    out.result.unwrap();
    assert_eq!(
        out.stdout,
        format!(
            "{}\n{}\n",
            d.dir().join("beta.md").display(),
            d.dir().join("alpha.md").display()
        )
    );
    assert_eq!(out.stderr, "beta.md   written\nalpha.md  written\n");
    assert_eq!(d.read("alpha.md"), template_text("alpha"));
    assert_eq!(d.read("beta.md"), template_text("beta"));
    assert!(!d.dir().join("gamma.md").exists());
    assert_eq!(
        s.requests(),
        vec![
            "/templates/index.toml",
            "/templates/beta.md",
            "/templates/alpha.md"
        ]
    );
}

#[test]
fn all_with_yes_writes_every_template() {
    let d = Dest::new();
    let s = server();
    let mut c = case(Choice::All);
    c.yes = true;
    c.run(&s, &d.dir()).result.unwrap();
    for name in ["alpha", "beta", "gamma"] {
        assert_eq!(d.read(&format!("{name}.md")), template_text(name), "{name}");
    }
}

#[test]
fn filter_with_yes_writes_the_matching_templates() {
    let d = Dest::new();
    let s = server();
    let mut c = case(Choice::Filter("AUDIO".into()));
    c.yes = true;
    c.run(&s, &d.dir()).result.unwrap();
    assert!(d.dir().join("gamma.md").is_file());
    assert!(!d.dir().join("alpha.md").exists());
    assert!(!d.dir().join("beta.md").exists());
}

#[test]
fn filter_without_a_match_is_a_usage_error_and_downloads_no_file() {
    let d = Dest::new();
    let s = server();
    let mut c = case(Choice::Filter("zzz".into()));
    c.yes = true;
    let out = c.run(&s, &d.dir());
    assert_eq!(usage(&out.result), "no template matches filter \"zzz\"");
    assert_eq!(s.requests(), vec!["/templates/index.toml"]);
    assert!(!d.dir().exists());
}

#[test]
fn an_unknown_name_lists_the_available_ones() {
    let d = Dest::new();
    let s = server();
    let mut c = case(names(&["alpha", "nope"]));
    c.yes = true;
    let out = c.run(&s, &d.dir());
    assert_eq!(
        usage(&out.result),
        "unknown template \"nope\" (available: alpha, beta, gamma)"
    );
    assert!(!d.dir().exists());
}

#[test]
fn without_a_terminal_and_without_yes_it_refuses_before_any_request() {
    let d = Dest::new();
    let s = server();
    let c = case(names(&["alpha"]));
    let out = c.run(&s, &d.dir());
    assert_eq!(
        usage(&out.result),
        "minuta asks before it downloads: pass --yes, or run it from a terminal"
    );
    assert_eq!(s.requests(), Vec::<String>::new());
    assert!(!d.dir().exists());
    assert_eq!((out.stdout.as_str(), out.stderr.as_str()), ("", ""));
}

#[test]
fn without_a_terminal_it_cannot_open_the_list() {
    let d = Dest::new();
    let s = server();
    let mut c = case(Choice::Ask);
    c.yes = true;
    let out = c.run(&s, &d.dir());
    assert_eq!(
        usage(&out.result),
        "nothing selected: name the templates to download, or use --all or --filter TEXT (a list opens only on a terminal)"
    );
    assert_eq!(s.requests(), Vec::<String>::new());
}

#[test]
fn nothing_chosen_and_no_confirmation_without_a_terminal_reports_nothing_selected_first() {
    let d = Dest::new();
    let s = server();
    let out = case(Choice::Ask).run(&s, &d.dir());
    assert_eq!(
        usage(&out.result),
        "nothing selected: name the templates to download, or use --all or --filter TEXT (a list opens only on a terminal)"
    );
    assert_eq!(s.requests(), Vec::<String>::new());
}

#[test]
fn an_empty_catalog_says_so_for_all() {
    let d = Dest::new();
    let s = server_with(&[]);
    let mut c = case(Choice::All);
    c.yes = true;
    let out = c.run(&s, &d.dir());
    out.result.unwrap();
    assert_eq!(out.stderr, "the catalog has no templates\n");
    assert_eq!(out.stdout, "");
}

#[test]
fn an_empty_catalog_says_so_instead_of_opening_the_list() {
    let d = Dest::new();
    let s = server_with(&[]);
    let mut c = case(Choice::Ask);
    c.interactive = true;
    c.input = "1\n";
    let out = c.run(&s, &d.dir());
    out.result.unwrap();
    assert_eq!(out.stderr, "the catalog has no templates\n");
}

#[test]
fn an_empty_catalog_with_a_name_is_an_unknown_template() {
    let d = Dest::new();
    let s = server_with(&[]);
    let mut c = case(names(&["alpha"]));
    c.yes = true;
    let out = c.run(&s, &d.dir());
    assert!(
        usage(&out.result).starts_with("unknown template \"alpha\""),
        "{:?}",
        out.result
    );
}
