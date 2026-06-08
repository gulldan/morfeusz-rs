use std::collections::BTreeSet;

use morfeusz::{
    CaseHandling, Error, Morfeusz, TokenNumbering, TsvLexiconLoader, WhitespaceHandling,
};

#[test]
fn default_options_match_cpp_api() {
    let morfeusz = Morfeusz::new();

    assert_eq!(morfeusz.whitespace_handling(), WhitespaceHandling::Skip);
    assert_eq!(
        morfeusz.case_handling(),
        CaseHandling::ConditionallyCaseSensitive
    );
    assert_eq!(morfeusz.token_numbering(), TokenNumbering::Separate);
}

#[test]
fn analyses_unknown_tokens_and_splits_basic_punctuation() {
    let mut morfeusz = Morfeusz::new();
    let result = morfeusz.analyse("AAaaBBbbCCcc DDDD.").unwrap();

    assert_eq!(result.len(), 3);
    assert_eq!(result[0].orth, "AAaaBBbbCCcc");
    assert_eq!(result[0].lemma, "AAaaBBbbCCcc");
    assert_eq!(result[0].tag_id, 0);
    assert_eq!(result[1].orth, "DDDD");
    assert_eq!(result[2].orth, ".");
}

#[test]
fn iterator_peek_and_exhaustion_match_legacy_shape() {
    let mut morfeusz = Morfeusz::new();
    let mut iterator = morfeusz.analyse_iter("AAAAbbbbCCCC").unwrap();

    assert!(iterator.has_next());
    assert_eq!(iterator.peek().unwrap().orth, "AAAAbbbbCCCC");
    assert_eq!(iterator.next().unwrap().orth, "AAAAbbbbCCCC");
    assert!(!iterator.has_next());
    assert!(iterator.peek().is_none());
    assert!(iterator.next().is_none());
    assert!(matches!(iterator.peek_result(), Err(Error::OutOfRange(_))));
    assert!(matches!(iterator.next_result(), Err(Error::OutOfRange(_))));
}

#[test]
fn iterator_keeps_whitespace_interpretations() {
    let mut morfeusz = Morfeusz::new();
    morfeusz.set_whitespace_handling(WhitespaceHandling::Keep);

    let result = morfeusz
        .analyse_iter(" AAAAbbbbCCCC  DDDDeeee.\t")
        .unwrap()
        .map(|item| item.orth)
        .collect::<Vec<_>>();

    assert_eq!(result, [" ", "AAAAbbbbCCCC", "  ", "DDDDeeee", ".", "\t"]);
}

#[test]
fn iterator_appends_whitespace_to_neighboring_token_orth() {
    let mut morfeusz = Morfeusz::new();
    morfeusz.set_whitespace_handling(WhitespaceHandling::Append);

    let result = morfeusz
        .analyse_iter(" AAAAbbbbCCCC  DDDDeeee.\t")
        .unwrap()
        .map(|item| item.orth)
        .collect::<Vec<_>>();

    assert_eq!(result, [" AAAAbbbbCCCC  ", "DDDDeeee", ".\t"]);
}

#[test]
fn keeps_whitespace_as_interpretations() {
    let mut morfeusz = Morfeusz::new();
    morfeusz.set_whitespace_handling(WhitespaceHandling::Keep);

    let result = morfeusz.analyse(" AAAAbbbbCCCC  DDDDeeee.\t").unwrap();

    assert_eq!(
        result
            .iter()
            .map(|interp| interp.orth.as_str())
            .collect::<Vec<_>>(),
        [" ", "AAAAbbbbCCCC", "  ", "DDDDeeee", ".", "\t"]
    );
    assert!(result[0].is_whitespace());
}

#[test]
fn keeps_whitespace_interpretation_shape_from_cpp_unit() {
    let mut morfeusz = Morfeusz::new();
    morfeusz.set_whitespace_handling(WhitespaceHandling::Keep);

    let result = morfeusz.analyse("  AAAAbbbbCCCC DDDDeeee\t").unwrap();

    assert_eq!(result.len(), 5);
    assert_eq!(
        result
            .iter()
            .map(|interp| (interp.orth.as_str(), interp.lemma.as_str(), interp.tag_id))
            .collect::<Vec<_>>(),
        [
            ("  ", "  ", 1),
            ("AAAAbbbbCCCC", "AAAAbbbbCCCC", 0),
            (" ", " ", 1),
            ("DDDDeeee", "DDDDeeee", 0),
            ("\t", "\t", 1)
        ]
    );
}

#[test]
fn treats_cpp_specific_unicode_whitespace_like_legacy_runtime() {
    let mut morfeusz = Morfeusz::new();
    morfeusz.set_whitespace_handling(WhitespaceHandling::Keep);

    let result = morfeusz
        .analyse("aa\u{200B}bb\u{2060}cc\u{180E}dd")
        .unwrap();

    assert_eq!(
        result
            .iter()
            .map(|interp| (interp.orth.as_str(), interp.is_whitespace()))
            .collect::<Vec<_>>(),
        [
            ("aa", false),
            ("\u{200B}", true),
            ("bb", false),
            ("\u{2060}", true),
            ("cc", false),
            ("\u{180E}", true),
            ("dd", false),
        ]
    );

    morfeusz.set_whitespace_handling(WhitespaceHandling::Skip);
    let skipped = morfeusz
        .analyse("aa\u{200B}bb\u{2060}cc\u{180E}dd")
        .unwrap();
    assert_eq!(
        skipped
            .iter()
            .map(|interp| interp.orth.as_str())
            .collect::<Vec<_>>(),
        ["aa", "bb", "cc", "dd"]
    );
}

#[test]
fn appends_whitespace_to_neighboring_token_orth() {
    let mut morfeusz = Morfeusz::new();
    morfeusz.set_whitespace_handling(WhitespaceHandling::Append);

    let result = morfeusz.analyse("  AAAAbbbbCCCC DDDDeeee\t").unwrap();

    assert_eq!(result.len(), 2);
    assert_eq!(result[0].orth, "  AAAAbbbbCCCC ");
    assert_eq!(result[0].lemma, "AAAAbbbbCCCC");
    assert_eq!(result[0].tag_id, 0);
    assert_eq!(result[1].orth, "DDDDeeee\t");
    assert_eq!(result[1].lemma, "DDDDeeee");
    assert_eq!(result[1].tag_id, 0);
}

#[test]
fn supports_continuous_token_numbering() {
    let mut morfeusz = Morfeusz::new();
    morfeusz.set_token_numbering(TokenNumbering::Continuous);

    let first = morfeusz.analyse("aaaabbbb bbbbcccc.").unwrap();
    let second = morfeusz.analyse("ccccdddd").unwrap();

    assert_eq!((first[0].start_node, first[2].end_node), (0, 3));
    assert_eq!((second[0].start_node, second[0].end_node), (3, 4));
}

#[test]
fn loads_plain_dictionary_fixture_with_names_and_labels() {
    let resolver =
        TsvLexiconLoader::tagset_from_path("../tests/analyzer/test_qualifiers/tagset.dat")
            .unwrap();
    let dictionary = TsvLexiconLoader::from_str(
        include_str!("../../tests/analyzer/test_qualifiers/dictionary.tab"),
        resolver,
    )
    .unwrap();
    let mut morfeusz = Morfeusz::with_dictionary(dictionary, Default::default());

    let result = morfeusz.analyse("czerwony").unwrap();

    assert_eq!(result.len(), 4);
    assert_eq!(result[0].lemma, "czerwony:a1");
    assert_eq!(
        result[0].tag(morfeusz.id_resolver()),
        Some("adj:sg:acc:m3:pos")
    );
    assert_eq!(result[0].name(morfeusz.id_resolver()), Some("pospolita"));
    assert_eq!(
        result[2].labels_as_string(morfeusz.id_resolver()),
        Some("żółty1|żółty2")
    );
    assert_eq!(
        result[2].labels(morfeusz.id_resolver()).unwrap(),
        &BTreeSet::from(["żółty1".to_owned(), "żółty2".to_owned()])
    );
    assert_eq!(
        result[2]
            .labels(morfeusz.id_resolver())
            .unwrap()
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["żółty1", "żółty2"]
    );
}

#[test]
fn plain_dictionary_in_word_graph_handles_utf8_boundaries_without_panicking() {
    let resolver = TsvLexiconLoader::tagset_from_str(
        "\
#!TAGSET-ID test
[TAGS]
0 ign
1 sp
2 praet:sg:m1:perf
3 aglt:sg:pri:imperf:wok
",
    )
    .unwrap();
    let dictionary = TsvLexiconLoader::from_str(
        "został\tzostać\tpraet:sg:m1:perf\nem\tbyć\taglt:sg:pri:imperf:wok\n",
        resolver,
    )
    .unwrap();
    let mut morfeusz = Morfeusz::with_dictionary(dictionary, Default::default());

    let result = morfeusz.analyse("zostałem").unwrap();

    assert_eq!(
        result
            .iter()
            .map(|interp| {
                (
                    interp.start_node,
                    interp.end_node,
                    interp.orth.as_str(),
                    interp.lemma.as_str(),
                    interp.tag(morfeusz.id_resolver()),
                )
            })
            .collect::<Vec<_>>(),
        [
            (0, 1, "został", "zostać", Some("praet:sg:m1:perf")),
            (1, 2, "em", "być", Some("aglt:sg:pri:imperf:wok")),
        ]
    );
}

#[test]
fn generates_from_plain_dictionary_fixture() {
    let resolver =
        TsvLexiconLoader::tagset_from_path("../tests/generator/test_names/tagset.dat").unwrap();
    let dictionary = TsvLexiconLoader::from_str(
        include_str!("../../tests/generator/test_names/dictionary.tab"),
        resolver,
    )
    .unwrap();
    let morfeusz = Morfeusz::with_dictionary(dictionary, Default::default());

    let result = morfeusz.generate("czerwony").unwrap();

    assert_eq!(result.len(), 3);
    assert_eq!(result[0].orth, "czerwony");
    assert_eq!(result[0].lemma, "czerwony:a1");
}
