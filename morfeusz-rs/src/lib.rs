pub mod adapters;
pub(crate) mod case_tables;
pub mod charset;
mod config;
mod dictionary;
mod engine;
mod error;
mod id_resolver;
mod morfeusz;
mod ports;
mod repository;
mod types;

pub use adapters::binary::{
    decode_analyzer_interpretations, decode_analyzer_interps_groups,
    decode_generator_interpretations, decode_generator_interps_groups, read_raw_interps_groups,
    BinaryAnalyzerLexicon, BinaryDictionaryData, BinaryFsa, BinaryGeneratorLexicon, BinaryLexicon,
    EncodedAnalyzerInterpretation, EncodedAnalyzerInterpsGroup, EncodedForm,
    EncodedGeneratorInterpretation, EncodedGeneratorInterpsGroup, FsaImplementation, RawFsaMatch,
    RawFsaPrefixMatch, RawInterpsGroup, SegmentationFsaVariant, SegmentationMetadata,
    SegmentationRulesFsa, SegmentationState, SimpleFsa, VLength1Fsa, VLength2Fsa,
};
pub use adapters::tsv::TsvLexiconLoader;
pub use config::{Config, NumberingScope, SegmentationPreset};
pub use dictionary::{Dictionary, DictionaryEntry};
pub use engine::{Engine, EngineBuilder, Session};
pub use error::{Error, Result};
pub use id_resolver::IdResolver;
pub use morfeusz::{Morfeusz, ResultsIterator};
pub use ports::Lexicon;
pub use repository::{BinaryDictionaryRepository, NamedDictionaryPaths};
pub use types::{
    CaseHandling, Charset, MorfeuszUsage, MorphInterpretation, TokenNumbering, WhitespaceHandling,
    ANALYSE_ONLY, APPEND_WHITESPACES, BOTH_ANALYSE_AND_GENERATE, CONDITIONALLY_CASE_SENSITIVE,
    CONTINUOUS_NUMBERING, CP1250, CP852, GENERATE_ONLY, IGNORE_CASE, ISO8859_2, KEEP_WHITESPACES,
    SEPARATE_NUMBERING, SKIP_WHITESPACES, STRICTLY_CASE_SENSITIVE, UTF8,
};
