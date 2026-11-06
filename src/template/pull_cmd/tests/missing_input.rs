use super::*;

fn chain_server() -> FakeHttp {
    server_with(&[
        ("alpha", None, "Alpha post", Some("gamma -> transcript")),
        ("gamma", None, "Gamma", None),
    ])
}

const NOTE: &str =
    "note: alpha needs template gamma, which is not available (pull it too or add it to your templates)\n";

#[test]
fn a_missing_input_template_is_noted() {
    let d = Dest::new();
    let s = chain_server();
    let mut c = case(names(&["alpha"]));
    c.yes = true;
    let out = c.run(&s, &d.dir());
    out.result.unwrap();
    assert_eq!(out.stderr, format!("alpha.md  written\n{NOTE}"));
}

#[test]
fn no_note_when_minuta_already_has_the_input_template() {
    let d = Dest::new();
    let s = chain_server();
    let mut c = case(names(&["alpha"]));
    c.yes = true;
    c.known = Some(vec!["gamma".to_string()]);
    let out = c.run(&s, &d.dir());
    assert_eq!(out.stderr, "alpha.md  written\n");
}

#[test]
fn no_note_when_the_input_template_is_pulled_too() {
    let d = Dest::new();
    let s = chain_server();
    let mut c = case(names(&["alpha", "gamma"]));
    c.yes = true;
    let out = c.run(&s, &d.dir());
    assert_eq!(out.stderr, "alpha.md  written\ngamma.md  written\n");
}

#[test]
fn transcript_is_never_a_missing_input() {
    let d = Dest::new();
    let s = server_with(&[("alpha", None, "Alpha", Some("transcript"))]);
    let mut c = case(names(&["alpha"]));
    c.yes = true;
    assert_eq!(c.run(&s, &d.dir()).stderr, "alpha.md  written\n");
}

#[test]
fn the_note_escapes_control_characters_from_the_catalog() {
    let d = Dest::new();
    let s = server_with(&[("alpha", None, "Alpha", Some("bad\\u001b[2J -> transcript"))]);
    let mut c = case(names(&["alpha"]));
    c.yes = true;
    let out = c.run(&s, &d.dir());
    assert_eq!(
        out.stderr,
        "alpha.md  written\nnote: alpha needs template bad?[2J, which is not available (pull it too or add it to your templates)\n"
    );
}

fn input_server() -> FakeHttp {
    server_with(&[
        ("alpha", None, "Alpha", Some("gamma")),
        ("beta", None, "Beta", Some("delta -> transcript")),
        ("chain", None, "Chain", Some("x -> y -> transcript")),
    ])
}

fn note(name: &str, dep: &str) -> String {
    format!("note: {name} needs template {dep}, which is not available (pull it too or add it to your templates)\n")
}

#[test]
fn every_written_template_with_a_missing_input_gets_its_own_note() {
    let d = Dest::new();
    let s = input_server();
    let mut c = case(names(&["alpha", "beta"]));
    c.yes = true;
    let out = c.run(&s, &d.dir());
    assert_eq!(
        out.stderr,
        format!(
            "alpha.md  written\nbeta.md   written\n{}{}",
            note("alpha", "gamma"),
            note("beta", "delta")
        )
    );
}

#[test]
fn every_missing_template_of_a_chain_gets_a_note() {
    let d = Dest::new();
    let s = input_server();
    let mut c = case(names(&["chain"]));
    c.yes = true;
    let out = c.run(&s, &d.dir());
    assert_eq!(
        out.stderr,
        format!(
            "chain.md  written\n{}{}",
            note("chain", "x"),
            note("chain", "y")
        )
    );
}

#[test]
fn a_file_that_is_up_to_date_gets_no_note() {
    let d = Dest::new();
    d.pulled("alpha", &template_text("alpha"));
    let s = input_server();
    let mut c = case(names(&["alpha"]));
    c.yes = true;
    let out = c.run(&s, &d.dir());
    assert_eq!(out.stderr, "alpha.md  up to date\nnothing to download\n");
}

#[test]
fn a_modified_file_that_is_skipped_gets_no_note() {
    let d = Dest::new();
    d.write("alpha.md", "my alpha\n");
    let s = input_server();
    let mut c = case(names(&["alpha"]));
    c.yes = true;
    let out = c.run(&s, &d.dir());
    assert_eq!(
        out.stderr,
        "alpha.md  skipped (modified; use --force to overwrite)\nnothing to download\n"
    );
}

#[test]
fn an_input_template_already_in_the_destination_is_available() {
    let d = Dest::new();
    d.write("gamma.md", "a gamma pulled earlier\n");
    let s = input_server();
    let mut c = case(names(&["alpha"]));
    c.yes = true;
    let out = c.run(&s, &d.dir());
    assert_eq!(out.stderr, "alpha.md  written\n");
}

#[test]
fn no_note_when_minuta_could_not_find_out_what_it_has() {
    let d = Dest::new();
    let s = input_server();
    let mut c = case(names(&["alpha"]));
    c.yes = true;
    c.known = None;
    let out = c.run(&s, &d.dir());
    assert_eq!(out.stderr, "alpha.md  written\n");
}

#[test]
fn a_destination_lookup_that_fails_is_reported_and_makes_no_note() {
    let d = Dest::new();
    // A name this long is a valid template name, but no file system can look it up.
    let long = "a".repeat(300);
    let input = format!("{long} -> transcript");
    let s = server_with(&[("alpha", None, "Alpha", Some(input.as_str()))]);
    let mut c = case(names(&["alpha"]));
    c.yes = true;
    let out = c.run(&s, &d.dir());
    out.result.unwrap();
    let path = d.dir().join(format!("{long}.md"));
    assert!(
        out.stderr.starts_with(&format!(
            "alpha.md  written\nwarning: cannot check {}: ",
            path.display()
        )),
        "{}",
        out.stderr
    );
    assert!(!out.stderr.contains("note:"), "{}", out.stderr);
}

#[test]
fn an_input_name_that_is_not_a_template_name_is_never_looked_up_on_disk() {
    let d = Dest::new();
    // `<dest>/../gamma.md` exists: a lookup of "../gamma" would wrongly find it.
    fs::write(d.tmp.path().join("gamma.md"), "outside the destination\n").unwrap();
    let s = server_with(&[("alpha", None, "Alpha", Some("../gamma"))]);
    let mut c = case(names(&["alpha"]));
    c.yes = true;
    let out = c.run(&s, &d.dir());
    assert_eq!(
        out.stderr,
        format!("alpha.md  written\n{}", note("alpha", "../gamma"))
    );
}
