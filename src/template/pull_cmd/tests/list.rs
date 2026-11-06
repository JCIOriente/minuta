use super::*;

#[test]
fn list_prints_the_table_and_downloads_nothing() {
    let d = Dest::new();
    let s = server();
    let mut c = case(Choice::Ask);
    c.list = true;
    let out = c.run(&s, &d.dir());
    out.result.unwrap();
    assert_eq!(
        out.stderr,
        format!(
            "templates from {}/\n\
NAME   PLATFORM  STATE  DESCRIPTION\n\
alpha  social    new    Alpha post\n\
beta   -         new    Beta notes\n\
gamma  audio     new    Gamma audio\n",
            s.base()
        )
    );
    assert_eq!(out.stdout, "");
    assert_eq!(s.requests(), vec!["/templates/index.toml"]);
    assert!(!d.dir().exists());
}

#[test]
fn list_shows_the_state_of_each_template() {
    let d = Dest::new();
    d.pulled("alpha", &template_text("alpha"));
    d.pulled("beta", "an older beta\n");
    d.write("gamma.md", "my own gamma\n");
    let s = server();
    let mut c = case(Choice::Ask);
    c.list = true;
    let out = c.run(&s, &d.dir());
    out.result.unwrap();
    assert_eq!(
        out.stderr,
        format!(
            "templates from {}/\n\
NAME   PLATFORM  STATE             DESCRIPTION\n\
alpha  social    up to date        Alpha post\n\
beta   -         update available  Beta notes\n\
gamma  audio     modified          Gamma audio\n",
            s.base()
        )
    );
}

#[test]
fn list_needs_no_terminal_and_no_yes() {
    let d = Dest::new();
    let s = server();
    let mut c = case(Choice::Ask);
    c.list = true;
    c.interactive = false;
    c.yes = false;
    c.run(&s, &d.dir()).result.unwrap();
}

#[test]
fn list_escapes_control_characters_from_the_catalog() {
    let d = Dest::new();
    let s = server_with(&[("alpha", Some("so\\u001bcial"), "bad\\u001b[2Jtext", None)]);
    let mut c = case(Choice::Ask);
    c.list = true;
    let out = c.run(&s, &d.dir());
    out.result.unwrap();
    assert_eq!(
        out.stderr,
        format!(
            "templates from {}/\n\
NAME   PLATFORM  STATE  DESCRIPTION\n\
alpha  so?cial   new    bad?[2Jtext\n",
            s.base()
        )
    );
}
