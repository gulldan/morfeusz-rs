use std::fs;
use std::path::{Path, PathBuf};

use morfeusz::{
    BinaryAnalyzerLexicon, BinaryDictionaryData, BinaryGeneratorLexicon, CaseHandling, Config,
    Engine, FsaImplementation, IdResolver, MorphInterpretation, NumberingScope,
};

fn builder_v2_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/binary/test-dict-copyright-a.dict")
}

fn builder_v1_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/binary/test-dict-copyright-v1-a.dict")
}

fn builder_simple_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/binary/test-dict-copyright-simple-a.dict")
}

fn builder_v2_digits_roman_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/binary/test-digits-roman-a.dict")
}

fn builder_v2_generator_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/binary/test-digits-s.dict")
}

fn builder_v1_generator_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/binary/test-digits-v1-s.dict")
}

fn builder_simple_generator_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/binary/test-digits-simple-s.dict")
}

fn builder_v2_additional_atomic_generator_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/binary/test-additional-atomic-s.dict")
}

fn builder_v2_mixed_case_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/binary/test-mixed-case-a.dict")
}

fn builder_v2_inflection_graph_numbers_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/binary/test-inflection-graph-numbers-a.dict")
}

fn builder_v2_names_analyzer_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/binary/test-names-a.dict")
}

fn builder_v2_qualifiers_analyzer_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/binary/test-qualifiers-a.dict")
}

fn builder_v2_names_generator_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/binary/test-names-s.dict")
}

fn builder_v2_qualifiers_generator_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/binary/test-qualifiers-s.dict")
}

fn builder_v2_prefix_strict_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/binary/test-prefixes-uppercase-beginning-a.dict")
}

fn builder_v2_prefix_middle_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/binary/test-prefixes-uppercase-middle-a.dict")
}

fn builder_v2_segtypes_analyzer_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/binary/test-segtypes-a.dict")
}

fn builder_v2_segtypes_homonyms_analyzer_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/binary/test-segtypes-homonyms-a.dict")
}

fn builder_v2_segtypes_generator_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/binary/test-segtypes-s.dict")
}

#[test]
fn parses_v2_dictionary_generated_by_builder() {
    let dictionary = BinaryDictionaryData::from_path(builder_v2_fixture()).unwrap();

    assert_eq!(dictionary.implementation(), FsaImplementation::VLength2);
    assert_eq!(dictionary.dict_id(), "identyfikator_słownika");
    assert!(dictionary.copyright().contains("copyrightowa"));

    let resolver = dictionary.id_resolver().unwrap();
    assert_eq!(resolver.tagset_id(), "pl.sgjp.morfeusz-0.5.1");
    assert_eq!(resolver.tag(0), Some("ign"));
    assert_eq!(resolver.tag(151), Some("dig"));
}

#[test]
fn parses_v1_dictionary_generated_by_builder() {
    let dictionary = BinaryDictionaryData::from_path(builder_v1_fixture()).unwrap();

    assert_eq!(dictionary.implementation(), FsaImplementation::VLength1);
    assert_eq!(dictionary.dict_id(), "identyfikator_słownika");
    assert!(dictionary.copyright().contains("copyrightowa"));

    let resolver = dictionary.id_resolver().unwrap();
    assert_eq!(resolver.tagset_id(), "pl.sgjp.morfeusz-0.5.1");
    assert_eq!(resolver.tag(0), Some("ign"));
    assert_eq!(resolver.tag(151), Some("dig"));
}

#[test]
fn parses_simple_dictionary_generated_by_builder() {
    let dictionary = BinaryDictionaryData::from_path(builder_simple_fixture()).unwrap();

    assert_eq!(dictionary.implementation(), FsaImplementation::Simple);
    assert_eq!(dictionary.dict_id(), "identyfikator_słownika");
    assert!(dictionary.copyright().contains("copyrightowa"));

    let resolver = dictionary.id_resolver().unwrap();
    assert_eq!(resolver.tagset_id(), "pl.sgjp.morfeusz-0.5.1");
    assert_eq!(resolver.tag(0), Some("ign"));
    assert_eq!(resolver.tag(151), Some("dig"));
}

#[test]
fn analyzes_digit_from_builder_v2_dictionary() {
    let lexicon = BinaryAnalyzerLexicon::from_path(builder_v2_fixture()).unwrap();
    let engine = Engine::builder().lexicon(lexicon).build();

    let analyzed = engine.analyze("7").unwrap();

    assert!(analyzed.iter().any(|interp| {
        interp.orth == "7" && interp.lemma == "7" && interp.tag(engine.resolver()) == Some("dig")
    }));
}

#[test]
fn analyzes_digit_from_builder_simple_dictionary() {
    let lexicon = BinaryAnalyzerLexicon::from_path(builder_simple_fixture()).unwrap();
    let engine = Engine::builder().lexicon(lexicon).build();

    let analyzed = engine.analyze("7").unwrap();

    assert!(analyzed.iter().any(|interp| {
        interp.orth == "7" && interp.lemma == "7" && interp.tag(engine.resolver()) == Some("dig")
    }));
}

#[test]
fn analyzes_digit_from_builder_v1_dictionary() {
    let lexicon = BinaryAnalyzerLexicon::from_path(builder_v1_fixture()).unwrap();
    let engine = Engine::builder().lexicon(lexicon).build();

    let analyzed = engine.analyze("7").unwrap();

    assert!(analyzed.iter().any(|interp| {
        interp.orth == "7" && interp.lemma == "7" && interp.tag(engine.resolver()) == Some("dig")
    }));
}

#[test]
fn analyzes_roman_digit_fixture_from_builder_v2_dictionary() {
    let lexicon = BinaryAnalyzerLexicon::from_path(builder_v2_digits_roman_fixture()).unwrap();
    let engine = Engine::builder().lexicon(lexicon).build();
    let source_fixture = repo_root().join("tests/analyzer/test_digits_roman");

    let input = fs::read_to_string(source_fixture.join("input.txt")).unwrap();
    let actual = input
        .lines()
        .map(|line| {
            let interps = engine.analyze(line).unwrap();
            format_analyzer_interps(engine.resolver(), &interps)
        })
        .collect::<Vec<_>>()
        .join("\n");
    let expected = read_expected_builder_binary_output(&source_fixture);

    assert_eq!(actual.trim_end(), expected.trim_end());
}

#[test]
fn generates_digit_from_builder_v2_dictionary() {
    let lexicon = BinaryGeneratorLexicon::from_path(builder_v2_generator_fixture()).unwrap();
    let engine = Engine::builder().lexicon(lexicon).build();

    let generated = engine.generate("7").unwrap();

    assert!(generated.iter().any(|interp| {
        interp.orth == "7" && interp.lemma == "7" && interp.tag(engine.resolver()) == Some("dig")
    }));
}

#[test]
fn generates_digits_fixture_from_builder_v1_dictionary() {
    assert_generator_fixture_matches(
        builder_v1_generator_fixture(),
        repo_root().join("tests/generator/test_digits"),
    );
}

#[test]
fn generates_digits_fixture_from_builder_simple_dictionary() {
    assert_generator_fixture_matches(
        builder_simple_generator_fixture(),
        repo_root().join("tests/generator/test_digits"),
    );
}

#[test]
fn generates_additional_atomic_fixture_from_builder_v2_dictionary() {
    let lexicon =
        BinaryGeneratorLexicon::from_path(builder_v2_additional_atomic_generator_fixture())
            .unwrap();
    let engine = Engine::builder().lexicon(lexicon).build();
    let source_fixture = repo_root().join("tests/generator/test_additional_atomic_segments");

    let input = fs::read_to_string(source_fixture.join("input.txt")).unwrap();
    let actual = input
        .lines()
        .map(|line| {
            let interps = engine.generate(line).unwrap();
            format_generator_interps(engine.resolver(), &interps)
        })
        .collect::<Vec<_>>()
        .join("\n");
    let expected = read_expected_builder_binary_output(&source_fixture);

    assert_eq!(actual.trim_end(), expected.trim_end());
}

#[test]
fn analyzes_mixed_case_fixture_from_builder_v2_dictionary() {
    let lexicon = BinaryAnalyzerLexicon::from_path(builder_v2_mixed_case_fixture()).unwrap();
    let engine = Engine::builder()
        .config(Config::default().with_numbering(NumberingScope::Continuous))
        .lexicon(lexicon)
        .build();
    let mut session = engine.session();
    let source_fixture = repo_root().join("tests/analyzer/test_mixed_case");

    let input = fs::read_to_string(source_fixture.join("input.txt")).unwrap();
    let actual = input
        .lines()
        .map(|line| {
            let interps = session.analyze(line).unwrap();
            format_analyzer_interps(engine.resolver(), &interps)
        })
        .collect::<Vec<_>>()
        .join("\n");
    let expected = read_expected_builder_binary_output(&source_fixture);

    assert_eq!(actual.trim_end(), expected.trim_end());
}

#[test]
fn analyzes_names_fixture_from_builder_v2_dictionary() {
    let lexicon = BinaryAnalyzerLexicon::from_path(builder_v2_names_analyzer_fixture()).unwrap();
    let engine = Engine::builder().lexicon(lexicon).build();
    let source_fixture = repo_root().join("tests/analyzer/test_names");

    let input = fs::read_to_string(source_fixture.join("input.txt")).unwrap();
    let actual = input
        .lines()
        .map(|line| {
            let interps = engine.analyze(line).unwrap();
            format_analyzer_interps(engine.resolver(), &interps)
        })
        .collect::<Vec<_>>()
        .join("\n");
    let expected = read_expected_builder_binary_output(&source_fixture);

    assert_eq!(actual.trim_end(), expected.trim_end());
}

#[test]
fn analyzes_qualifiers_fixture_from_builder_v2_dictionary() {
    let lexicon =
        BinaryAnalyzerLexicon::from_path(builder_v2_qualifiers_analyzer_fixture()).unwrap();
    let engine = Engine::builder().lexicon(lexicon).build();
    let source_fixture = repo_root().join("tests/analyzer/test_qualifiers");

    let input = fs::read_to_string(source_fixture.join("input.txt")).unwrap();
    let actual = input
        .lines()
        .map(|line| {
            let interps = engine.analyze(line).unwrap();
            format_analyzer_interps(engine.resolver(), &interps)
        })
        .collect::<Vec<_>>()
        .join("\n");
    let expected = read_expected_builder_binary_output(&source_fixture);

    assert_eq!(actual.trim_end(), expected.trim_end());
}

#[test]
fn generates_names_fixture_from_builder_v2_dictionary() {
    let lexicon = BinaryGeneratorLexicon::from_path(builder_v2_names_generator_fixture()).unwrap();
    let engine = Engine::builder().lexicon(lexicon).build();
    let source_fixture = repo_root().join("tests/generator/test_names");

    let input = fs::read_to_string(source_fixture.join("input.txt")).unwrap();
    let actual = input
        .lines()
        .map(|line| {
            let interps = engine.generate(line).unwrap();
            format_generator_interps(engine.resolver(), &interps)
        })
        .collect::<Vec<_>>()
        .join("\n");
    let expected = read_expected_builder_binary_output(&source_fixture);

    assert_eq!(actual.trim_end(), expected.trim_end());
}

#[test]
fn generates_qualifiers_fixture_from_builder_v2_dictionary() {
    let lexicon =
        BinaryGeneratorLexicon::from_path(builder_v2_qualifiers_generator_fixture()).unwrap();
    let engine = Engine::builder().lexicon(lexicon).build();
    let source_fixture = repo_root().join("tests/generator/test_qualifiers");

    let input = fs::read_to_string(source_fixture.join("input.txt")).unwrap();
    let actual = input
        .lines()
        .map(|line| {
            let interps = engine.generate(line).unwrap();
            format_generator_interps(engine.resolver(), &interps)
        })
        .collect::<Vec<_>>()
        .join("\n");
    let expected = read_expected_builder_binary_output(&source_fixture);

    assert_eq!(actual.trim_end(), expected.trim_end());
}

#[test]
fn analyzes_inflection_graph_numbers_fixture_from_builder_v2_dictionary() {
    let lexicon =
        BinaryAnalyzerLexicon::from_path(builder_v2_inflection_graph_numbers_fixture()).unwrap();
    let engine = Engine::builder()
        .config(Config::default())
        .lexicon(lexicon)
        .build();
    let source_fixture = repo_root().join("tests/analyzer/test_inflection_graph_numbers");

    let input = fs::read_to_string(source_fixture.join("input.txt")).unwrap();
    let actual = input
        .lines()
        .map(|line| {
            let interps = engine.analyze(line).unwrap();
            format_analyzer_interps(engine.resolver(), &interps)
        })
        .collect::<Vec<_>>()
        .join("\n");
    let expected = read_expected_builder_binary_output(&source_fixture);

    assert_eq!(actual.trim_end(), expected.trim_end());
}

#[test]
fn analyzes_strict_prefix_fixture_from_builder_v2_dictionary() {
    let lexicon = BinaryAnalyzerLexicon::from_path(builder_v2_prefix_strict_fixture()).unwrap();
    let engine = Engine::builder()
        .config(Config::default().with_case_handling(CaseHandling::StrictlyCaseSensitive))
        .lexicon(lexicon)
        .build();
    let source_fixture =
        repo_root().join("tests/analyzer/test_prefixes_with_uppercase_at_the_beginning");

    let input = fs::read_to_string(source_fixture.join("input.txt")).unwrap();
    let actual = input
        .lines()
        .map(|line| {
            let interps = engine.analyze(line).unwrap();
            format_analyzer_interps(engine.resolver(), &interps)
        })
        .collect::<Vec<_>>()
        .join("\n");
    let expected = read_expected_builder_binary_output(&source_fixture);

    assert_eq!(actual.trim_end(), expected.trim_end());
}

#[test]
fn analyzes_middle_prefix_fixture_from_builder_v2_dictionary() {
    let lexicon = BinaryAnalyzerLexicon::from_path(builder_v2_prefix_middle_fixture()).unwrap();
    let engine = Engine::builder().lexicon(lexicon).build();
    let source_fixture =
        repo_root().join("tests/analyzer/test_prefixes_with_uppercase_in_the_middle");

    let input = fs::read_to_string(source_fixture.join("input.txt")).unwrap();
    let actual = input
        .lines()
        .map(|line| {
            let interps = engine.analyze(line).unwrap();
            format_analyzer_interps(engine.resolver(), &interps)
        })
        .collect::<Vec<_>>()
        .join("\n");
    let expected = read_expected_builder_binary_output(&source_fixture);

    assert_eq!(actual.trim_end(), expected.trim_end());
}

#[test]
fn analyzes_segtypes_fixture_from_builder_v2_dictionary() {
    let lexicon = BinaryAnalyzerLexicon::from_path(builder_v2_segtypes_analyzer_fixture()).unwrap();
    let engine = Engine::builder().lexicon(lexicon).build();
    let source_fixture = repo_root().join("tests/analyzer/test_segtypes");

    let input = fs::read_to_string(source_fixture.join("input.txt")).unwrap();
    let actual = input
        .lines()
        .map(|line| {
            let interps = engine.analyze(line).unwrap();
            format_analyzer_interps(engine.resolver(), &interps)
        })
        .collect::<Vec<_>>()
        .join("\n");
    let expected = read_expected_builder_binary_output(&source_fixture);

    assert_eq!(actual.trim_end(), expected.trim_end());
}

#[test]
fn analyzes_segtypes_homonyms_fixture_from_builder_v2_dictionary() {
    let lexicon =
        BinaryAnalyzerLexicon::from_path(builder_v2_segtypes_homonyms_analyzer_fixture()).unwrap();
    let engine = Engine::builder().lexicon(lexicon).build();
    let source_fixture = repo_root().join("tests/analyzer/test_segtypes_with_homonyms");

    let input = fs::read_to_string(source_fixture.join("input.txt")).unwrap();
    let actual = input
        .lines()
        .map(|line| {
            let interps = engine.analyze(line).unwrap();
            format_analyzer_interps(engine.resolver(), &interps)
        })
        .collect::<Vec<_>>()
        .join("\n");
    let expected = read_expected_builder_binary_output(&source_fixture);

    assert_eq!(actual.trim_end(), expected.trim_end());
}

#[test]
fn generates_segtypes_fixture_from_builder_v2_dictionary() {
    let lexicon =
        BinaryGeneratorLexicon::from_path(builder_v2_segtypes_generator_fixture()).unwrap();
    let engine = Engine::builder().lexicon(lexicon).build();
    let source_fixture = repo_root().join("tests/generator/test_segtypes");

    let input = fs::read_to_string(source_fixture.join("input.txt")).unwrap();
    let actual = input
        .lines()
        .map(|line| {
            let interps = engine.generate(line).unwrap();
            format_generator_interps(engine.resolver(), &interps)
        })
        .collect::<Vec<_>>()
        .join("\n");
    let expected = read_expected_builder_binary_output(&source_fixture);

    assert_eq!(actual.trim_end(), expected.trim_end());
}

fn format_analyzer_interps(resolver: &IdResolver, interps: &[MorphInterpretation]) -> String {
    if interps.is_empty() {
        return "[]".to_owned();
    }
    let mut rendered = String::new();
    let mut index = 0;

    while index < interps.len() {
        let group_start = index;
        index += 1;
        while index < interps.len() && same_edge(&interps[group_start], &interps[index]) {
            index += 1;
        }

        let group = &interps[group_start..index];
        if group.len() == 1 {
            rendered.push_str(&format!(
                "[{}]\n",
                format_analyzer_interp(resolver, &group[0])
            ));
        } else {
            for (offset, interp) in group.iter().enumerate() {
                if offset == 0 {
                    rendered.push('[');
                } else {
                    rendered.push(' ');
                }
                rendered.push_str(&format_analyzer_interp(resolver, interp));
                if offset + 1 == group.len() {
                    rendered.push(']');
                }
                rendered.push('\n');
            }
        }
    }

    rendered.trim_end_matches('\n').to_owned()
}

fn read_expected_builder_binary_output(source_fixture: &Path) -> String {
    fs::read_to_string(source_fixture.join("output.txt"))
        .unwrap()
        .lines()
        .map(canonicalize_result_labels_for_builder_binary)
        .collect::<Vec<_>>()
        .join("\n")
}

fn canonicalize_result_labels_for_builder_binary(line: &str) -> String {
    if line == "[]" {
        return line.to_owned();
    }

    let (prefix, line) = line
        .strip_prefix('[')
        .map(|rest| ("[", rest))
        .or_else(|| line.strip_prefix(' ').map(|rest| (" ", rest)))
        .unwrap_or(("", line));
    let (line, suffix) = line
        .strip_suffix(']')
        .map_or((line, ""), |body| (body, "]"));

    let Some((body, labels)) = line.rsplit_once(',') else {
        return format!("{prefix}{line}{suffix}");
    };

    format!(
        "{prefix}{body},{}{suffix}",
        canonicalize_builder_binary_labels(labels)
    )
}

fn canonicalize_builder_binary_labels(labels: &str) -> String {
    if labels == "_" {
        return "_".to_owned();
    }
    let mut labels = labels
        .split('|')
        .filter(|label| !label.is_empty())
        .collect::<Vec<_>>();
    labels.sort_unstable();
    labels.dedup();
    if labels.is_empty() {
        "_".to_owned()
    } else {
        labels.join("|")
    }
}

fn format_analyzer_interp(resolver: &IdResolver, interp: &MorphInterpretation) -> String {
    format!(
        "{},{},{},{},{},{},{}",
        interp.start_node,
        interp.end_node,
        interp.orth,
        interp.lemma,
        interp.tag(resolver).unwrap_or("_"),
        interp.name(resolver).unwrap_or("_"),
        interp.labels_as_string(resolver).unwrap_or("_")
    )
}

fn format_generator_interps(resolver: &IdResolver, interps: &[MorphInterpretation]) -> String {
    if interps.is_empty() {
        return "[]".to_owned();
    }
    let mut rendered = String::new();
    for (index, interp) in interps.iter().enumerate() {
        if index == 0 {
            rendered.push('[');
        } else {
            rendered.push(' ');
        }
        rendered.push_str(&format_generator_interp(resolver, interp));
        if index + 1 == interps.len() {
            rendered.push(']');
        }
        rendered.push('\n');
    }
    rendered.trim_end_matches('\n').to_owned()
}

fn assert_generator_fixture_matches(dictionary_path: PathBuf, source_fixture: PathBuf) {
    let lexicon = BinaryGeneratorLexicon::from_path(dictionary_path).unwrap();
    let engine = Engine::builder().lexicon(lexicon).build();

    let input = fs::read_to_string(source_fixture.join("input.txt")).unwrap();
    let actual = input
        .lines()
        .map(|line| {
            let interps = engine.generate(line).unwrap();
            format_generator_interps(engine.resolver(), &interps)
        })
        .collect::<Vec<_>>()
        .join("\n");
    let expected = read_expected_builder_binary_output(&source_fixture);

    assert_eq!(actual.trim_end(), expected.trim_end());
}

fn format_generator_interp(resolver: &IdResolver, interp: &MorphInterpretation) -> String {
    format!(
        "{},{},{},{},{}",
        interp.orth,
        interp.lemma,
        interp.tag(resolver).unwrap_or("_"),
        interp.name(resolver).unwrap_or("_"),
        interp.labels_as_string(resolver).unwrap_or("_")
    )
}

fn same_edge(left: &MorphInterpretation, right: &MorphInterpretation) -> bool {
    left.start_node == right.start_node
        && left.end_node == right.end_node
        && left.orth == right.orth
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}
