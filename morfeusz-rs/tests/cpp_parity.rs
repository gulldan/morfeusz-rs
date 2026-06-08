//! Behavioral parity regressions derived from the live C++ reference.
//!
//! Each case here encodes an observed `morfeusz_analyzer`/`morfeusz_generator`
//! output captured from the untouched C++ implementation (see
//! `build-cpp-ref.sh` and `tests/diff_corpus/`). They guard against the Rust
//! port re-introducing heuristics that diverge from the dictionary-driven C++
//! algorithm.

use std::path::PathBuf;

use morfeusz::{
    BinaryAnalyzerLexicon, BinaryGeneratorLexicon, Engine, IdResolver, MorphInterpretation,
};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/binary")
        .join(name)
}

fn analyzer(name: &str) -> Engine {
    let lexicon = BinaryAnalyzerLexicon::from_path(fixture(name)).unwrap();
    Engine::builder().lexicon(lexicon).build()
}

fn generator(name: &str) -> Engine {
    let lexicon = BinaryGeneratorLexicon::from_path(fixture(name)).unwrap();
    Engine::builder().lexicon(lexicon).build()
}

fn tags(engine: &Engine, interps: &[MorphInterpretation]) -> Vec<String> {
    interps
        .iter()
        .map(|i| i.tag(engine.resolver()).unwrap_or("?").to_owned())
        .collect()
}

fn render(resolver: &IdResolver, interps: &[MorphInterpretation]) -> Vec<String> {
    interps
        .iter()
        .map(|i| {
            format!(
                "{},{},{},{},{}",
                i.start_node,
                i.end_node,
                i.orth,
                i.lemma,
                i.tag(resolver).unwrap_or("?")
            )
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Generator: digits must come from the dictionary FSA, never a global heuristic.
// ---------------------------------------------------------------------------

#[test]
fn generator_digit_dictionary_synthesizes_multi_digit_numbers() {
    // C++: `123` -> [123,123,dig,_,_] with the digits generator dictionary.
    let engine = generator("test-digits-s.dict");
    for input in ["1", "12", "123", "1234", "012341"] {
        let out = engine.generate(input).unwrap();
        assert_eq!(
            out.len(),
            1,
            "input {input:?}: {:?}",
            render(engine.resolver(), &out)
        );
        assert_eq!(
            out[0].tag(engine.resolver()),
            Some("dig"),
            "input {input:?}"
        );
        assert_eq!(out[0].lemma, input);
        assert_eq!(out[0].orth, input);
    }
}

#[test]
fn generator_without_digit_entries_returns_ign_for_digits() {
    // C++: `123` -> [123,123,ign,_,_] with dictionaries lacking digit entries.
    for dict in [
        "test-names-s.dict",
        "test-qualifiers-s.dict",
        "test-segtypes-s.dict",
    ] {
        let engine = generator(dict);
        let out = engine.generate("123").unwrap();
        assert_eq!(
            tags(&engine, &out),
            vec!["ign".to_owned()],
            "dict {dict}: digits must not be special-cased"
        );
        assert_eq!(out[0].lemma, "123");
    }
}

// ---------------------------------------------------------------------------
// Analyzer: roman/digit interpretations must come from the dictionary FSA.
// ---------------------------------------------------------------------------

#[test]
fn analyzer_without_roman_entries_returns_ign_for_roman_letters() {
    // C++: with test-segtypes (no romandig/dig), `c`,`V`,`VII`,`123` are all ign.
    let engine = analyzer("test-segtypes-a.dict");
    for input in ["c", "V", "VII", "123", "42"] {
        let out = engine.analyze(input).unwrap();
        assert_eq!(
            tags(&engine, &out),
            vec!["ign".to_owned()],
            "input {input:?}: must be ign with a non-digit/roman dictionary, got {:?}",
            render(engine.resolver(), &out)
        );
        assert_eq!(out[0].lemma, input);
    }
}

#[test]
fn analyzer_roman_dictionary_recognizes_roman_and_digits() {
    // C++ with test-digits-roman: c->romandig(C), V->romandig, VII->romandig, 123->dig.
    let engine = analyzer("test-digits-roman-a.dict");
    let cases = [
        ("c", "C", "romandig"),
        ("V", "V", "romandig"),
        ("VII", "VII", "romandig"),
        ("123", "123", "dig"),
    ];
    for (input, lemma, tag) in cases {
        let out = engine.analyze(input).unwrap();
        let rendered = render(engine.resolver(), &out);
        assert!(
            out.iter()
                .any(|i| i.lemma == lemma && i.tag(engine.resolver()) == Some(tag)),
            "input {input:?}: expected {lemma}/{tag}, got {rendered:?}"
        );
        // Single merged edge (0,1) for the whole token.
        assert!(
            out.iter().all(|i| i.start_node == 0 && i.end_node == 1),
            "input {input:?}: {rendered:?}"
        );
    }
}
