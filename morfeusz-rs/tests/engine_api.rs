use morfeusz::{
    Config, Engine, NumberingScope, SegmentationPreset, TsvLexiconLoader, WhitespaceHandling,
};

#[test]
fn engine_uses_builder_config_without_legacy_setters() {
    let engine = Engine::builder()
        .config(
            Config::default()
                .with_whitespace(WhitespaceHandling::Keep)
                .with_segmentation(SegmentationPreset::new("permissive", "split").unwrap()),
        )
        .build();

    let result = engine.analyze("Aaaa  żżżż").unwrap();

    assert_eq!(
        result
            .iter()
            .map(|interp| interp.orth.as_str())
            .collect::<Vec<_>>(),
        ["Aaaa", "  ", "żżżż"]
    );
}

#[test]
fn stateless_analysis_keeps_numbering_separate_by_default() {
    let engine = Engine::builder().build();

    let first = engine.analyze("aaaa bbbb").unwrap();
    let second = engine.analyze("cccc").unwrap();

    assert_eq!((first[0].start_node, first[1].end_node), (0, 2));
    assert_eq!((second[0].start_node, second[0].end_node), (0, 1));
}

#[test]
fn session_owns_continuous_numbering_state() {
    let engine = Engine::builder()
        .config(Config::default().with_numbering(NumberingScope::Continuous))
        .build();
    let mut session = engine.session();

    let first = session.analyze("aaaa bbbb").unwrap();
    let second = session.analyze("cccc").unwrap();

    assert_eq!((first[0].start_node, first[1].end_node), (0, 2));
    assert_eq!((second[0].start_node, second[0].end_node), (2, 3));
}

#[test]
fn engine_loads_tsv_lexicon_via_builder() {
    let resolver =
        TsvLexiconLoader::tagset_from_path("../tests/analyzer/test_qualifiers/tagset.dat")
            .unwrap();
    let dictionary = TsvLexiconLoader::from_str(
        include_str!("../../tests/analyzer/test_qualifiers/dictionary.tab"),
        resolver,
    )
    .unwrap();
    let engine = Engine::builder().lexicon(dictionary).build();

    let result = engine.analyze("czerwony").unwrap();

    assert_eq!(result.len(), 4);
    assert_eq!(result[0].lemma, "czerwony:a1");
    assert_eq!(result[0].tag(engine.resolver()), Some("adj:sg:acc:m3:pos"));
}
