use super::*;

#[test]
fn pull_end_to_end_against_a_server() {
    let d = Dest::new();
    let ta = template_text("a");
    let tb = template_text("b");
    let a = entry("alpha", &ta);
    let b = entry("beta", &tb);
    let server = FakeHttp::start(vec![
        Route::ok("/templates/index.toml", catalog_text(&[&a, &b])),
        Route::ok("/templates/alpha.md", ta.clone()),
        Route::ok("/templates/beta.md", tb.clone()),
    ]);
    let origin = Origin::new(&server.base()).unwrap();
    let catalog: Catalog = fetch_catalog(&origin).unwrap();
    let selection: Vec<&Entry> = catalog.entries.iter().collect();
    let (mut manifest, warning) = read_manifest(&d.dir()).unwrap();
    assert_eq!(warning, None);
    let items = plan(&d.dir(), &selection, &manifest).unwrap();
    let done = apply(
        &d.dir(),
        &items,
        false,
        &mut |e| fetch_file(&origin, e),
        &mut manifest,
    )
    .unwrap();
    let outcomes: Vec<Outcome> = done.iter().map(|x| x.outcome).collect();
    assert_eq!(outcomes, [Outcome::Created, Outcome::Created]);
    assert_eq!(fs::read_to_string(d.dir().join("alpha.md")).unwrap(), ta);
    assert_eq!(fs::read_to_string(d.dir().join("beta.md")).unwrap(), tb);
    assert_eq!(
        server.requests(),
        vec![
            "/templates/index.toml",
            "/templates/alpha.md",
            "/templates/beta.md"
        ]
    );

    // A second pull finds everything up to date and fetches no file.
    let (mut manifest, _) = read_manifest(&d.dir()).unwrap();
    let items = plan(&d.dir(), &selection, &manifest).unwrap();
    let done = apply(
        &d.dir(),
        &items,
        false,
        &mut |e| fetch_file(&origin, e),
        &mut manifest,
    )
    .unwrap();
    let outcomes: Vec<Outcome> = done.iter().map(|x| x.outcome).collect();
    assert_eq!(outcomes, [Outcome::KeptUpToDate, Outcome::KeptUpToDate]);
    assert_eq!(server.requests().len(), 3);
}
