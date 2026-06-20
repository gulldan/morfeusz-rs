use std::collections::BTreeSet;

pub const MAX_QUALIFIERS_COMBINATIONS: usize = 2048;
pub const MAX_TAGS: usize = 65_535;
pub const MAX_NAMES: usize = 255;
pub const MAX_SEGMENT_TYPES: usize = 255;
pub const MAX_SEGMENT_RULES_FSA_SIZE: usize = 65_535;
pub const MAGIC_NUMBER: u32 = 0x8fc2_bc1b;
pub const DICTIONARY_VERSION: u8 = 21;

pub(crate) const ORTH_ONLY_LOWER: u8 = 128;
pub(crate) const ORTH_ONLY_TITLE: u8 = 64;
pub(crate) const LEMMA_ONLY_LOWER: u8 = 32;
pub(crate) const LEMMA_ONLY_TITLE: u8 = 16;
pub(crate) const PREFIX_CUT_MASK: u8 = 15;
pub(crate) const CASE_PATTERN_ONLY_LOWER: u8 = 0;
pub(crate) const CASE_PATTERN_UPPER_PREFIX: u8 = 1;
pub(crate) const CASE_PATTERN_MIXED: u8 = 2;

pub type QualifierSet = BTreeSet<String>;
