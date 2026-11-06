use super::*;

#[test]
fn plain_mode_prints_no_bar_and_same_lines() {
    let server = server();
    let dest = Dest::new();
    let mut c = case(names(&["alpha", "beta"]));
    c.yes = true;
    c.mode = Mode::Plain;
    let out = c.run(&server, &dest.dir());
    out.result.unwrap();
    assert_eq!(out.stderr, "alpha.md  written\nbeta.md   written\n");
    assert_eq!(
        out.stdout.lines().count(),
        2,
        "stdout carries only the paths"
    );
}

#[test]
fn live_mode_fetch_failure_reports_the_error_and_writes_nothing() {
    // beta is in the catalog but the server does not have its file.
    let mut routes = vec![Route::ok("/templates/index.toml", catalog_text(&ENTRIES))];
    routes.push(Route::ok("/templates/alpha.md", template_text("alpha")));
    routes.push(Route::status("/templates/beta.md", 404, "missing"));
    let server = FakeHttp::start(routes);
    let dest = Dest::new();
    let mut c = case(names(&["alpha", "beta"]));
    c.yes = true;
    c.mode = Mode::Live;
    let out = c.run(&server, &dest.dir());
    assert!(out.result.is_err());
    assert!(!out.stderr.contains('\r') && !out.stderr.contains('\x1b'));
    assert!(out.stdout.is_empty());
}
