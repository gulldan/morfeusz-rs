//! Golden behavioral-parity tests against committed C++ reference output.
//!
//! `tests/parity/<dict>/input.txt` holds a deterministic adversarial corpus and
//! `<flagkey>.expected` holds the exact stdout the untouched C++
//! `morfeusz_analyzer` produced for that corpus under the corresponding option
//! set (see `tests/diff_corpus/gen_golden.py`). Each case replays the corpus
//! through the Rust analyzer — formatted byte-for-byte like the CLI's
//! `printMorphResults` — and asserts equality. This locks in parity in CI
//! without needing the C++ build at test time.

use std::fs;
use std::path::PathBuf;

use morfeusz::{
    BinaryAnalyzerLexicon, CaseHandling, IdResolver, Morfeusz, MorfeuszUsage, MorphInterpretation,
    TokenNumbering, WhitespaceHandling,
};

fn parity_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/parity")
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/binary")
}

#[test]
fn rust_analyzer_matches_committed_cpp_golden_output() {
    let manifest = fs::read_to_string(parity_dir().join("manifest.tsv"))
        .expect("parity manifest must exist (run tests/diff_corpus/gen_golden.py)");

    let mut checked = 0;
    for line in manifest.lines().filter(|l| !l.trim().is_empty()) {
        let mut cols = line.split('\t');
        let dict_name = cols.next().unwrap();
        let dict_file = cols.next().unwrap();
        let flagkey = cols.next().unwrap();

        let lexicon = BinaryAnalyzerLexicon::from_path(fixtures_dir().join(dict_file))
            .unwrap_or_else(|e| panic!("load {dict_file}: {e:?}"));
        let mut morfeusz = Morfeusz::with_lexicon(lexicon, MorfeuszUsage::AnalyseOnly);
        apply_flagkey(&mut morfeusz, flagkey);

        let input = fs::read_to_string(parity_dir().join(dict_name).join("input.txt")).unwrap();
        let expected = fs::read_to_string(
            parity_dir()
                .join(dict_name)
                .join(format!("{flagkey}.expected")),
        )
        .unwrap();

        let actual = run_like_cli(&mut morfeusz, &input);

        assert_eq!(
            actual, expected,
            "parity mismatch for dict={dict_name} flags={flagkey}"
        );
        checked += 1;
    }
    assert!(checked > 0, "no golden parity cases were checked");
    eprintln!("verified {checked} golden parity cases");
}

fn apply_flagkey(morfeusz: &mut Morfeusz, flagkey: &str) {
    match flagkey {
        "default" => {}
        "keep" => morfeusz.set_whitespace_handling(WhitespaceHandling::Keep),
        "append" => morfeusz.set_whitespace_handling(WhitespaceHandling::Append),
        "strict" => morfeusz.set_case_handling(CaseHandling::StrictlyCaseSensitive),
        "ignore" => morfeusz.set_case_handling(CaseHandling::IgnoreCase),
        "continuous" => morfeusz.set_token_numbering(TokenNumbering::Continuous),
        other => panic!("unknown flagkey {other}"),
    }
}

/// Replays the corpus exactly as `morfeusz-cli`'s `process_stdin` would: one
/// `analyse` per input line (the final empty line after a trailing newline is
/// not processed), each result block formatted like `printMorphResults`, then a
/// trailing newline.
fn run_like_cli(morfeusz: &mut Morfeusz, input: &str) -> String {
    let mut lines: Vec<&str> = input.split('\n').collect();
    if input.ends_with('\n') {
        lines.pop();
    }
    let mut out = String::new();
    for line in lines {
        let line = line.strip_suffix('\r').unwrap_or(line);
        let results = morfeusz.analyse(line).unwrap();
        out.push_str(&format_results(morfeusz.id_resolver(), &results));
    }
    out.push('\n');
    out
}

/// Byte-for-byte port of `morfeusz-cli`'s `format_results` (print_node_numbers
/// = true), which mirrors C++ `printMorphResults`.
fn format_results(resolver: &IdResolver, results: &[MorphInterpretation]) -> String {
    let mut out = String::from("[");
    let mut prev: Option<(i32, i32)> = None;
    for item in results {
        let current = (item.start_node, item.end_node);
        match prev {
            Some(previous) if previous != current => out.push_str("]\n["),
            Some(_) => out.push_str("\n "),
            None => {}
        }
        out.push_str(&format!("{},{},", item.start_node, item.end_node));
        let tag = item.tag(resolver).unwrap_or("ign");
        let name = if item.name_id == 0 {
            "_"
        } else {
            item.name(resolver).unwrap_or("_")
        };
        let labels = if item.labels_id == 0 {
            "_"
        } else {
            item.labels_as_string(resolver).unwrap_or("_")
        };
        out.push_str(&format!(
            "{},{},{},{},{}",
            item.orth, item.lemma, tag, name, labels
        ));
        prev = Some(current);
    }
    out.push_str("]\n");
    out
}
