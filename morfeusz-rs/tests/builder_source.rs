use std::path::PathBuf;

use morfeusz::{BinaryAnalyzerLexicon, BinaryGeneratorLexicon, Engine};
use morfeusz_builder::{
    build_analyzer_simple_dictionary_from_paths, build_generator_simple_dictionary_from_paths,
};

fn repo_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(relative)
}

#[test]
fn rust_builder_source_pipeline_produces_loadable_analyzer_dictionary() {
    let dictionary = repo_path("tests/analyzer/test_digits/dictionary.tab");
    let tagset = repo_path("tests/analyzer/test_digits/tagset.dat");
    let segmentation = repo_path("tests/analyzer/test_digits/segmentation.dat");

    let bytes =
        build_analyzer_simple_dictionary_from_paths([dictionary], tagset, segmentation).unwrap();
    let engine = Engine::builder()
        .lexicon(BinaryAnalyzerLexicon::from_bytes(bytes).unwrap())
        .build();

    let result = engine.analyze("123").unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].orth, "123");
    assert_eq!(result[0].lemma, "123");
    assert_eq!(engine.resolver().tag(result[0].tag_id), Some("dig"));
}

#[test]
fn rust_builder_source_pipeline_produces_loadable_generator_dictionary() {
    let dictionary = repo_path("tests/generator/test_digits/dictionary.tab");
    let tagset = repo_path("tests/generator/test_digits/tagset.dat");
    let segmentation = repo_path("tests/generator/test_digits/segmentation.dat");

    let bytes =
        build_generator_simple_dictionary_from_paths([dictionary], tagset, segmentation).unwrap();
    let engine = Engine::builder()
        .lexicon(BinaryGeneratorLexicon::from_bytes(bytes).unwrap())
        .build();

    let result = engine.generate("123").unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].orth, "123");
    assert_eq!(result[0].lemma, "123");
    assert_eq!(engine.resolver().tag(result[0].tag_id), Some("dig"));
}
