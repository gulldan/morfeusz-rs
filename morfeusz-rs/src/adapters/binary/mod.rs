use std::collections::hash_map::RandomState;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::hash::{BuildHasher, BuildHasherDefault, Hasher};
use std::ops::Range;
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::{
    CaseHandling, DictionaryEntry, Error, IdResolver, Lexicon, MorphInterpretation, Result,
    SegmentationPreset,
};

mod analyzer;
mod cache;
mod case;
mod data;
mod fsa;
mod generator;
mod graph;
mod interps;
mod lexicon;
mod read;
mod segmentation;
#[cfg(test)]
mod tests;

use analyzer::*;
use cache::*;
use case::*;
use graph::*;
use interps::*;
use read::*;
use segmentation::*;

pub use analyzer::BinaryAnalyzerLexicon;
pub use data::{BinaryDictionaryData, FsaImplementation};
pub use fsa::{BinaryFsa, RawFsaMatch, RawFsaPrefixMatch, SimpleFsa, VLength1Fsa, VLength2Fsa};
pub use generator::BinaryGeneratorLexicon;
pub use interps::{
    decode_analyzer_interpretations, decode_analyzer_interps_groups,
    decode_generator_interpretations, decode_generator_interps_groups, read_raw_interps_groups,
    EncodedAnalyzerInterpretation, EncodedAnalyzerInterpsGroup, EncodedForm,
    EncodedGeneratorInterpretation, EncodedGeneratorInterpsGroup, RawInterpsGroup,
};
pub use lexicon::BinaryLexicon;
pub use segmentation::{
    SegmentationFsaVariant, SegmentationMetadata, SegmentationRulesFsa, SegmentationState,
};

const MAGIC_NUMBER: u32 = 0x8fc2bc1b;
const VERSION_NUM: u8 = 21;
const VERSION_NUM_OFFSET: usize = 4;
const IMPLEMENTATION_NUM_OFFSET: usize = 5;
const FSA_DATA_SIZE_OFFSET: usize = 6;
const FSA_DATA_OFFSET: usize = 10;
const SIMPLE_ACCEPTING_FLAG: u8 = 128;
const SIMPLE_TRANSITIONS_NUM_MASK: u8 = 127;
const SIMPLE_TRANSITION_SIZE: usize = 4;
const SIMPLE_TRANSDUCER_TRANSITION_SIZE: usize = 5;
const V1_INITIAL_STATE_OFFSET: usize = 257;
const V1_ACCEPTING_FLAG: u8 = 128;
const V1_TRANSITIONS_NUM_MASK: u8 = 127;
const V1_OFFSET_SIZE_MASK: u8 = 3;
const V2_HAS_REMAINING_FLAG: u8 = 128;
const V2_ACCEPTING_FLAG: u8 = 64;
const V2_LAST_FLAG: u8 = 32;
const V2_OFFSET_MASK: u8 = 0x7f;
const V2_FIRST_BYTE_OFFSET_MASK: u8 = 0x1f;
const ORTH_ONLY_LOWER: u8 = 128;
const ORTH_ONLY_TITLE: u8 = 64;
const LEMMA_ONLY_LOWER: u8 = 32;
const LEMMA_ONLY_TITLE: u8 = 16;
const PREFIX_CUT_MASK: u8 = 15;
const CASE_PATTERN_ONLY_LOWER: u8 = 0;
const CASE_PATTERN_UPPER_PREFIX: u8 = 1;
const CASE_PATTERN_MIXED: u8 = 2;
const SEGRULES_ACCEPTING_FLAG: u8 = 1;
const SEGRULES_WEAK_FLAG: u8 = 2;
const SEGRULES_TRANSITION_SIZE: usize = 4;
const ANALYZER_DECODE_CACHE_MAX_GROUPS: usize = 32 * 1024;
const GENERATOR_DECODE_CACHE_MAX_GROUPS: usize = 4 * 1024;
const WORD_TEMPLATE_CACHE_MAX_WORDS: usize = 20 * 1024;
const WORD_TEMPLATE_SEEN_ONCE_SLOTS: usize = 64 * 1024;
const WORD_TEMPLATE_MAX_WORD_BYTES: usize = 64;
const WORD_TEMPLATE_MAX_INTERPRETATIONS: usize = 32;
