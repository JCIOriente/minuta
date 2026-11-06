use super::*;

#[test]
fn the_list_selects_by_number() {
    let d = Dest::new();
    let s = server();
    let mut c = case(Choice::Ask);
    c.interactive = true;
    c.input = "1,3\ny\n";
    let out = c.run(&s, &d.dir());
    out.result.unwrap();
    assert_eq!(
        out.stderr,
        format!(
            "{LIST}{ASK}minuta will download from:\n  {}/\n  alpha.md  (new)\n  gamma.md  (new)\ninto {}\nContinue? [y/N] alpha.md  written\ngamma.md  written\n",
            s.base(),
            d.dir().display()
        )
    );
    assert!(d.dir().join("alpha.md").is_file());
    assert!(!d.dir().join("beta.md").exists());
    assert!(d.dir().join("gamma.md").is_file());
}

#[test]
fn the_list_selects_everything_with_a() {
    let d = Dest::new();
    let s = server();
    let mut c = case(Choice::Ask);
    c.interactive = true;
    c.input = "a\nyes\n";
    c.run(&s, &d.dir()).result.unwrap();
    for name in ["alpha", "beta", "gamma"] {
        assert!(d.dir().join(format!("{name}.md")).is_file(), "{name}");
    }
}

#[test]
fn an_empty_answer_to_the_list_cancels() {
    let d = Dest::new();
    let s = server();
    let mut c = case(Choice::Ask);
    c.interactive = true;
    c.input = "\n";
    let out = c.run(&s, &d.dir());
    out.result.unwrap();
    assert_eq!(out.stderr, format!("{LIST}{ASK}nothing selected\n"));
    assert_eq!(s.requests(), vec!["/templates/index.toml"]);
    assert!(!d.dir().exists());
}

#[test]
fn an_invalid_answer_to_the_list_is_a_usage_error() {
    let d = Dest::new();
    let s = server();
    let mut c = case(Choice::Ask);
    c.interactive = true;
    c.input = "x\n";
    let out = c.run(&s, &d.dir());
    assert_eq!(
        usage(&out.result),
        "invalid choice \"x\" (enter numbers from 1 to 3, 'a' for all, or nothing to cancel)"
    );
    assert_eq!(s.requests(), vec!["/templates/index.toml"]);
}

#[test]
fn the_list_shows_the_state_of_each_template() {
    let d = Dest::new();
    d.pulled("alpha", &template_text("alpha"));
    d.pulled("beta", "an older beta\n");
    let s = server();
    let mut c = case(Choice::Ask);
    c.interactive = true;
    c.input = "\n";
    let out = c.run(&s, &d.dir());
    out.result.unwrap();
    assert_eq!(
        out.stderr,
        format!(
            " 1) alpha  up to date        Alpha post\n 2) beta   update available  Beta notes\n 3) gamma  new               Gamma audio\n{ASK}nothing selected\n"
        )
    );
}

#[test]
fn the_numbered_list_escapes_control_characters_from_the_catalog() {
    let d = Dest::new();
    let s = server_with(&[("alpha", None, "bad\\u001b[2Jtext", None)]);
    let mut c = case(Choice::Ask);
    c.interactive = true;
    c.input = "\n";
    let out = c.run(&s, &d.dir());
    out.result.unwrap();
    assert_eq!(
        out.stderr,
        format!(" 1) alpha  new  bad?[2Jtext\n{ASK}nothing selected\n")
    );
}

#[test]
fn end_of_input_at_the_list_ends_the_prompt_line() {
    let d = Dest::new();
    let s = server();
    let mut c = case(Choice::Ask);
    c.interactive = true;
    c.input = "";
    let out = c.run(&s, &d.dir());
    out.result.unwrap();
    assert_eq!(out.stderr, format!("{LIST}{ASK}\nnothing selected\n"));
}
