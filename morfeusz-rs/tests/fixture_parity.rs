use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use morfeusz::{
    Morfeusz, MorphInterpretation, TokenNumbering, TsvLexiconLoader, WhitespaceHandling,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FixtureMode {
    Analyzer,
    Generator,
    Copyright,
    DictCopyright,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
enum FixtureStatus {
    Supported,
    Unsupported(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Fixture {
    path: &'static str,
    mode: FixtureMode,
    status: FixtureStatus,
}

const FIXTURES: &[Fixture] = &[
    Fixture {
        path: "tests/analyzer/test_copyright",
        mode: FixtureMode::Copyright,
        status: FixtureStatus::Supported,
    },
    Fixture {
        path: "tests/analyzer/test_dict_copyright",
        mode: FixtureMode::DictCopyright,
        status: FixtureStatus::Supported,
    },
    Fixture {
        path: "tests/analyzer/test_digits",
        mode: FixtureMode::Analyzer,
        status: FixtureStatus::Supported,
    },
    Fixture {
        path: "tests/analyzer/test_digits_roman",
        mode: FixtureMode::Analyzer,
        status: FixtureStatus::Supported,
    },
    Fixture {
        path: "tests/analyzer/test_inflection_graph_numbers",
        mode: FixtureMode::Analyzer,
        status: FixtureStatus::Supported,
    },
    Fixture {
        path: "tests/analyzer/test_mixed_case",
        mode: FixtureMode::Analyzer,
        status: FixtureStatus::Supported,
    },
    Fixture {
        path: "tests/analyzer/test_multisegments",
        mode: FixtureMode::Analyzer,
        status: FixtureStatus::Supported,
    },
    Fixture {
        path: "tests/analyzer/test_names",
        mode: FixtureMode::Analyzer,
        status: FixtureStatus::Supported,
    },
    Fixture {
        path: "tests/analyzer/test_prefixes_with_uppercase_at_the_beginning",
        mode: FixtureMode::Analyzer,
        status: FixtureStatus::Supported,
    },
    Fixture {
        path: "tests/analyzer/test_prefixes_with_uppercase_in_the_middle",
        mode: FixtureMode::Analyzer,
        status: FixtureStatus::Supported,
    },
    Fixture {
        path: "tests/analyzer/test_qualifiers",
        mode: FixtureMode::Analyzer,
        status: FixtureStatus::Supported,
    },
    Fixture {
        path: "tests/analyzer/test_segtypes",
        mode: FixtureMode::Analyzer,
        status: FixtureStatus::Supported,
    },
    Fixture {
        path: "tests/analyzer/test_segtypes_with_homonyms",
        mode: FixtureMode::Analyzer,
        status: FixtureStatus::Supported,
    },
    Fixture {
        path: "tests/analyzer/test_whitespace_handling_append",
        mode: FixtureMode::Analyzer,
        status: FixtureStatus::Supported,
    },
    Fixture {
        path: "tests/generator/test_additional_atomic_segments",
        mode: FixtureMode::Generator,
        status: FixtureStatus::Supported,
    },
    Fixture {
        path: "tests/generator/test_digits",
        mode: FixtureMode::Generator,
        status: FixtureStatus::Supported,
    },
    Fixture {
        path: "tests/generator/test_names",
        mode: FixtureMode::Generator,
        status: FixtureStatus::Supported,
    },
    Fixture {
        path: "tests/generator/test_qualifiers",
        mode: FixtureMode::Generator,
        status: FixtureStatus::Supported,
    },
    Fixture {
        path: "tests/generator/test_segtypes",
        mode: FixtureMode::Generator,
        status: FixtureStatus::Supported,
    },
];

#[test]
fn every_cpp_fixture_is_registered_in_rust_parity_manifest() {
    let root = repo_root();
    let discovered = ["tests/analyzer", "tests/generator"]
        .into_iter()
        .flat_map(|relative_dir| {
            fs::read_dir(root.join(relative_dir))
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .filter(|path| path.is_dir())
                .map(|path| {
                    path.strip_prefix(&root)
                        .unwrap()
                        .to_string_lossy()
                        .into_owned()
                })
                .collect::<Vec<_>>()
        })
        .collect::<BTreeSet<_>>();
    let registered = FIXTURES
        .iter()
        .map(|fixture| fixture.path.to_owned())
        .collect::<BTreeSet<_>>();

    assert_eq!(discovered, registered);
}

#[test]
fn supported_repository_fixtures_match_expected_outputs() {
    for fixture in FIXTURES
        .iter()
        .filter(|fixture| fixture.status == FixtureStatus::Supported)
    {
        match fixture.mode {
            FixtureMode::Analyzer => assert_analyzer_fixture(fixture.path),
            FixtureMode::Generator => assert_generator_fixture(fixture.path),
            FixtureMode::Copyright => assert_copyright_fixture(fixture.path),
            FixtureMode::DictCopyright => assert_dict_copyright_fixture(fixture.path),
        }
    }
}

#[test]
fn unsupported_repository_fixtures_are_explicitly_tracked() {
    let unsupported = FIXTURES
        .iter()
        .filter_map(|fixture| match fixture.status {
            FixtureStatus::Supported => None,
            FixtureStatus::Unsupported(reason) => Some((fixture.path, reason)),
        })
        .collect::<Vec<_>>();

    assert_eq!(unsupported.len(), 0);
    assert!(unsupported
        .iter()
        .all(|(_, reason)| !reason.trim().is_empty()));
}

fn assert_analyzer_fixture(relative_dir: &str) {
    let fixture = repo_root().join(relative_dir);
    let mut morfeusz = morfeusz_for_fixture(&fixture);
    apply_args(&mut morfeusz, &fixture);

    let input = fs::read_to_string(fixture.join("input.txt")).unwrap();
    let actual = input
        .lines()
        .map(|line| {
            let interps = morfeusz.analyse(line).unwrap();
            format_analyzer_interps(&morfeusz, &interps)
        })
        .collect::<Vec<_>>()
        .join("\n");
    let expected = fs::read_to_string(fixture.join("output.txt")).unwrap();

    assert_eq!(
        actual.trim_end(),
        expected.trim_end(),
        "fixture mismatch: {relative_dir}"
    );
}

fn assert_generator_fixture(relative_dir: &str) {
    let fixture = repo_root().join(relative_dir);
    let morfeusz = morfeusz_for_fixture(&fixture);

    let input = fs::read_to_string(fixture.join("input.txt")).unwrap();
    let actual = input
        .lines()
        .map(|line| {
            let interps = morfeusz.generate(line).unwrap();
            format_generator_interps(&morfeusz, &interps)
        })
        .collect::<Vec<_>>()
        .join("\n");
    let expected = fs::read_to_string(fixture.join("output.txt")).unwrap();

    assert_eq!(
        actual.trim_end(),
        expected.trim_end(),
        "fixture mismatch: {relative_dir}"
    );
}

fn assert_copyright_fixture(relative_dir: &str) {
    let fixture = repo_root().join(relative_dir);
    let actual = Morfeusz::copyright();
    let expected = fs::read_to_string(fixture.join("output.txt")).unwrap();

    assert_eq!(
        actual.trim_end(),
        expected.trim_end(),
        "fixture mismatch: {relative_dir}"
    );
}

fn assert_dict_copyright_fixture(relative_dir: &str) {
    let fixture = repo_root().join(relative_dir);
    let morfeusz = morfeusz_for_fixture(&fixture);
    let actual = morfeusz.dict_copyright();
    let expected = fs::read_to_string(fixture.join("output.txt")).unwrap();

    assert_eq!(
        actual.trim_end(),
        expected.trim_end(),
        "fixture mismatch: {relative_dir}"
    );
}

fn morfeusz_for_fixture(fixture: &Path) -> Morfeusz {
    let dictionary = TsvLexiconLoader::from_paths_with_segmentation(
        fixture.join("dictionary.tab"),
        fixture.join("tagset.dat"),
        fixture.join("segmentation.dat"),
    )
    .unwrap_or_else(|_| {
        let resolver = TsvLexiconLoader::tagset_from_path(fixture.join("tagset.dat")).unwrap();
        TsvLexiconLoader::from_str(
            &fs::read_to_string(fixture.join("dictionary.tab")).unwrap(),
            resolver,
        )
        .unwrap()
    });
    Morfeusz::with_dictionary(dictionary, Default::default())
}

fn apply_args(morfeusz: &mut Morfeusz, fixture: &Path) {
    let args = fs::read_to_string(fixture.join("ARGS")).unwrap_or_default();
    if args.contains("APPEND_WHITESPACES") {
        morfeusz.set_whitespace_handling(WhitespaceHandling::Append);
    } else if args.contains("KEEP_WHITESPACES") {
        morfeusz.set_whitespace_handling(WhitespaceHandling::Keep);
    }
    if args.contains("CONTINUOUS_NUMBERING") {
        morfeusz.set_token_numbering(TokenNumbering::Continuous);
    }
}

fn format_analyzer_interps(morfeusz: &Morfeusz, interps: &[MorphInterpretation]) -> String {
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
                format_analyzer_interp(morfeusz, &group[0])
            ));
        } else {
            for (offset, interp) in group.iter().enumerate() {
                if offset == 0 {
                    rendered.push('[');
                } else {
                    rendered.push(' ');
                }
                rendered.push_str(&format_analyzer_interp(morfeusz, interp));
                if offset + 1 == group.len() {
                    rendered.push(']');
                }
                rendered.push('\n');
            }
        }
    }

    rendered.trim_end_matches('\n').to_owned()
}

fn format_generator_interps(morfeusz: &Morfeusz, interps: &[MorphInterpretation]) -> String {
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
        rendered.push_str(&format_generator_interp(morfeusz, interp));
        if index + 1 == interps.len() {
            rendered.push(']');
        }
        rendered.push('\n');
    }
    rendered.trim_end_matches('\n').to_owned()
}

fn format_analyzer_interp(morfeusz: &Morfeusz, interp: &MorphInterpretation) -> String {
    format!(
        "{},{},{},{},{},{},{}",
        interp.start_node,
        interp.end_node,
        interp.orth,
        interp.lemma,
        interp.tag(morfeusz.id_resolver()).unwrap_or("_"),
        interp.name(morfeusz.id_resolver()).unwrap_or("_"),
        interp
            .labels_as_string(morfeusz.id_resolver())
            .unwrap_or("_")
    )
}

fn format_generator_interp(morfeusz: &Morfeusz, interp: &MorphInterpretation) -> String {
    format!(
        "{},{},{},{},{}",
        interp.orth,
        interp.lemma,
        interp.tag(morfeusz.id_resolver()).unwrap_or("_"),
        interp.name(morfeusz.id_resolver()).unwrap_or("_"),
        interp
            .labels_as_string(morfeusz.id_resolver())
            .unwrap_or("_")
    )
}

fn same_edge(left: &MorphInterpretation, right: &MorphInterpretation) -> bool {
    left.start_node == right.start_node
        && left.end_node == right.end_node
        && left.orth == right.orth
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}
