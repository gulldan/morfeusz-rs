use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use morfeusz::{
    BinaryDictionaryRepository, CaseHandling, Charset, Morfeusz, MorfeuszUsage, TokenNumbering,
    WhitespaceHandling, ANALYSE_ONLY, APPEND_WHITESPACES, BOTH_ANALYSE_AND_GENERATE,
    CONDITIONALLY_CASE_SENSITIVE, CONTINUOUS_NUMBERING, CP1250, CP852, GENERATE_ONLY, IGNORE_CASE,
    ISO8859_2, KEEP_WHITESPACES, SEPARATE_NUMBERING, SKIP_WHITESPACES, STRICTLY_CASE_SENSITIVE,
    UTF8,
};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[test]
fn legacy_static_metadata_matches_cpp_api_contract() {
    assert_eq!(Morfeusz::version(), "1.99.15");
    assert_eq!(env!("CARGO_PKG_VERSION"), Morfeusz::version());
    assert_eq!(Morfeusz::default_dict_name(), "sgjp");
    assert!(Morfeusz::copyright().starts_with("Copyright © 2014–2021"));
}

#[test]
fn cxx_named_constants_match_legacy_header_values() {
    assert_eq!(UTF8, Charset::Utf8);
    assert_eq!(UTF8 as i32, 11);
    assert_eq!(ISO8859_2 as i32, 12);
    assert_eq!(CP1250 as i32, 13);
    assert_eq!(CP852 as i32, 14);

    assert_eq!(
        CONDITIONALLY_CASE_SENSITIVE,
        CaseHandling::ConditionallyCaseSensitive
    );
    assert_eq!(CONDITIONALLY_CASE_SENSITIVE as i32, 100);
    assert_eq!(STRICTLY_CASE_SENSITIVE as i32, 101);
    assert_eq!(IGNORE_CASE as i32, 102);

    assert_eq!(SEPARATE_NUMBERING, TokenNumbering::Separate);
    assert_eq!(SEPARATE_NUMBERING as i32, 201);
    assert_eq!(CONTINUOUS_NUMBERING as i32, 202);

    assert_eq!(SKIP_WHITESPACES, WhitespaceHandling::Skip);
    assert_eq!(SKIP_WHITESPACES as i32, 301);
    assert_eq!(APPEND_WHITESPACES as i32, 302);
    assert_eq!(KEEP_WHITESPACES as i32, 303);

    assert_eq!(ANALYSE_ONLY, MorfeuszUsage::AnalyseOnly);
    assert_eq!(ANALYSE_ONLY as i32, 401);
    assert_eq!(GENERATE_ONLY as i32, 402);
    assert_eq!(BOTH_ANALYSE_AND_GENERATE as i32, 403);
}

#[test]
fn create_instance_named_with_repository_loads_analyzer_and_generator() {
    let temp_dir = dictionary_pair("named");
    let repo = BinaryDictionaryRepository::new([temp_dir.clone()]);

    let mut morfeusz =
        Morfeusz::create_instance_named_with_repository(&repo, "named", MorfeuszUsage::default())
            .unwrap();

    let analyzed = morfeusz.analyse("7").unwrap();
    assert!(analyzed
        .iter()
        .any(|item| item.orth == "7" && item.tag(morfeusz.id_resolver()) == Some("dig")));

    let generated = morfeusz.generate("123").unwrap();
    assert!(generated
        .iter()
        .any(|item| item.orth == "123" && item.tag(morfeusz.id_resolver()) == Some("dig")));

    fs::remove_dir_all(temp_dir).unwrap();
}

#[test]
fn set_dictionary_named_with_repository_preserves_runtime_options_and_resets_segmentation() {
    let temp_dir = dictionary_pair("switch");
    let repo = BinaryDictionaryRepository::new([temp_dir.clone()]);
    let fresh_default =
        Morfeusz::create_instance_named_with_repository(&repo, "switch", MorfeuszUsage::default())
            .unwrap()
            .aggl()
            .to_owned();
    let explicit_aggl = ["strict", "permissive", "isolated"]
        .into_iter()
        .find(|option| *option != fresh_default)
        .unwrap();
    let mut morfeusz = Morfeusz::new();
    morfeusz.set_whitespace_handling(WhitespaceHandling::Keep);
    morfeusz.set_case_handling(CaseHandling::IgnoreCase);
    morfeusz.set_token_numbering(TokenNumbering::Continuous);
    morfeusz.set_aggl(explicit_aggl).unwrap();

    morfeusz
        .set_dictionary_named_with_repository(&repo, "switch")
        .unwrap();

    assert_eq!(morfeusz.whitespace_handling(), WhitespaceHandling::Keep);
    assert_eq!(morfeusz.case_handling(), CaseHandling::IgnoreCase);
    assert_eq!(morfeusz.token_numbering(), TokenNumbering::Continuous);
    assert_eq!(morfeusz.aggl(), fresh_default);
    assert_eq!(morfeusz.praet(), "split");
    assert!(morfeusz
        .analyse("7")
        .unwrap()
        .iter()
        .any(|item| item.tag(morfeusz.id_resolver()) == Some("dig")));

    fs::remove_dir_all(temp_dir).unwrap();
}

#[test]
fn set_dictionary_path_preserves_runtime_options_and_resets_segmentation() {
    let dictionary = fixture("test-dict-copyright-v1-a.dict");
    let fresh_default = BinaryDictionaryRepository::default()
        .load_path(&dictionary, MorfeuszUsage::default())
        .unwrap()
        .aggl()
        .to_owned();
    let explicit_aggl = ["strict", "permissive", "isolated"]
        .into_iter()
        .find(|option| *option != fresh_default)
        .unwrap();
    let mut morfeusz = Morfeusz::new();
    morfeusz.set_whitespace_handling(WhitespaceHandling::Keep);
    morfeusz.set_case_handling(CaseHandling::IgnoreCase);
    morfeusz.set_token_numbering(TokenNumbering::Continuous);
    morfeusz.set_aggl(explicit_aggl).unwrap();

    morfeusz.set_dictionary_path(&dictionary).unwrap();

    assert_eq!(morfeusz.usage(), MorfeuszUsage::AnalyseOnly);
    assert_eq!(morfeusz.whitespace_handling(), WhitespaceHandling::Keep);
    assert_eq!(morfeusz.case_handling(), CaseHandling::IgnoreCase);
    assert_eq!(morfeusz.token_numbering(), TokenNumbering::Continuous);
    assert_eq!(morfeusz.aggl(), fresh_default);
    assert_eq!(morfeusz.praet(), "split");
    assert!(morfeusz
        .analyse("7")
        .unwrap()
        .iter()
        .any(|item| item.tag(morfeusz.id_resolver()) == Some("dig")));
}

fn dictionary_pair(name: &str) -> PathBuf {
    let temp_dir = unique_temp_dir();
    fs::create_dir_all(&temp_dir).unwrap();
    fs::copy(
        fixture("test-dict-copyright-v1-a.dict"),
        temp_dir.join(format!("{name}-a.dict")),
    )
    .unwrap();
    fs::copy(
        fixture("test-digits-v1-s.dict"),
        temp_dir.join(format!("{name}-s.dict")),
    )
    .unwrap();
    temp_dir
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/binary")
        .join(name)
}

fn unique_temp_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "morfeusz-core-api-{}-{nanos}-{counter}",
        std::process::id()
    ))
}
