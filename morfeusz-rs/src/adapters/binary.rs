use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::hash::{BuildHasherDefault, Hasher};
use std::ops::Range;
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::{
    CaseHandling, DictionaryEntry, Error, IdResolver, Lexicon, MorphInterpretation, Result,
    SegmentationPreset,
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
type InterpsGroupId = (usize, usize);
type AnalyzerDecodeCacheMap = HashMap<
    InterpsGroupId,
    Arc<[BinaryAnalyzerInterpretation]>,
    BuildHasherDefault<FastInterpsGroupHasher>,
>;
type GeneratorDecodeCacheMap = HashMap<
    InterpsGroupId,
    Arc<[EncodedGeneratorInterpretation]>,
    BuildHasherDefault<FastInterpsGroupHasher>,
>;

#[derive(Default)]
struct FastInterpsGroupHasher(u64);

impl FastInterpsGroupHasher {
    fn mix(&mut self, value: u64) {
        const K: u64 = 0x9e37_79b1_85eb_ca87;
        let mut value = value.wrapping_add(K);
        value ^= value >> 33;
        value = value.wrapping_mul(0xff51_afd7_ed55_8ccd);
        value ^= value >> 33;
        self.0 = self.0.rotate_left(5) ^ value;
    }
}

impl Hasher for FastInterpsGroupHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        let mut chunks = bytes.chunks_exact(8);
        for chunk in &mut chunks {
            self.mix(u64::from_ne_bytes(chunk.try_into().expect("8-byte chunk")));
        }
        let remainder = chunks.remainder();
        if !remainder.is_empty() {
            let mut last = [0u8; 8];
            last[..remainder.len()].copy_from_slice(remainder);
            self.mix(u64::from_ne_bytes(last));
        }
    }

    fn write_usize(&mut self, value: usize) {
        self.mix(value as u64);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsaImplementation {
    Simple,
    SimpleWithTransitionData,
    VLength1,
    VLength2,
}

impl FsaImplementation {
    fn from_code(code: u8) -> Result<Self> {
        match code {
            0 => Ok(Self::Simple),
            128 => Ok(Self::SimpleWithTransitionData),
            1 => Ok(Self::VLength1),
            2 => Ok(Self::VLength2),
            _ => Err(Error::invalid_dictionary(format!(
                "unsupported FSA implementation code: {code}"
            ))),
        }
    }
}

#[derive(Debug, Clone)]
pub struct BinaryDictionaryData {
    // `Arc<[u8]>` so a dictionary can be shared across threads (and across
    // per-thread analyzer clones) without copying the multi-megabyte payload.
    bytes: Arc<[u8]>,
    implementation: FsaImplementation,
    fsa_range: Range<usize>,
    epilogue_offset: usize,
    id_resolver_offset: usize,
    segmentation_rules_offset: usize,
    dict_id: String,
    copyright: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentationMetadata {
    pub separators: Vec<u32>,
    pub fsa_variants: Vec<SegmentationFsaVariant>,
    pub default_options: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentationFsaVariant {
    pub options: BTreeMap<String, String>,
    pub fsa: Vec<u8>,
}

impl SegmentationMetadata {
    pub fn default_fsa_variant(&self) -> Option<&SegmentationFsaVariant> {
        self.fsa_variant_for_options(&self.default_options)
    }

    fn fsa_variant_for_preset(
        &self,
        segmentation: &SegmentationPreset,
    ) -> Option<&SegmentationFsaVariant> {
        self.fsa_variants
            .iter()
            .find(|variant| self.variant_matches_preset(variant, segmentation))
    }

    fn variant_matches_preset(
        &self,
        variant: &SegmentationFsaVariant,
        segmentation: &SegmentationPreset,
    ) -> bool {
        if variant.options.len() != self.effective_options_len(segmentation) {
            return false;
        }

        for (key, default_value) in &self.default_options {
            let expected = explicit_segmentation_value(segmentation, key.as_str())
                .unwrap_or(default_value.as_str());
            if variant.options.get(key).map(String::as_str) != Some(expected) {
                return false;
            }
        }

        self.explicit_extra_option_matches(variant, "aggl", segmentation.aggl())
            && self.explicit_extra_option_matches(variant, "praet", segmentation.praet())
    }

    fn effective_options_len(&self, segmentation: &SegmentationPreset) -> usize {
        self.default_options.len()
            + usize::from(
                segmentation.aggl().is_some() && !self.default_options.contains_key("aggl"),
            )
            + usize::from(
                segmentation.praet().is_some() && !self.default_options.contains_key("praet"),
            )
    }

    fn explicit_extra_option_matches(
        &self,
        variant: &SegmentationFsaVariant,
        option: &str,
        value: Option<&str>,
    ) -> bool {
        if self.default_options.contains_key(option) {
            return true;
        }
        match value {
            Some(value) => variant.options.get(option).map(String::as_str) == Some(value),
            None => true,
        }
    }

    pub fn available_options(&self, option: &str) -> BTreeSet<String> {
        self.fsa_variants
            .iter()
            .filter_map(|variant| variant.options.get(option).cloned())
            .collect()
    }

    pub fn fsa_variant_for_options(
        &self,
        options: &BTreeMap<String, String>,
    ) -> Option<&SegmentationFsaVariant> {
        self.fsa_variants
            .iter()
            .find(|variant| &variant.options == options)
    }
}

impl SegmentationFsaVariant {
    pub fn rules_fsa(&self) -> Result<SegmentationRulesFsa<'_>> {
        SegmentationRulesFsa::new(&self.fsa)
    }
}

fn explicit_segmentation_value<'a>(
    segmentation: &'a SegmentationPreset,
    option: &str,
) -> Option<&'a str> {
    match option {
        "aggl" => segmentation.aggl(),
        "praet" => segmentation.praet(),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegmentationState {
    pub offset: usize,
    pub accepting: bool,
    pub weak: bool,
    pub shift_orth_from_previous: bool,
    pub sink: bool,
    pub failed: bool,
}

impl SegmentationState {
    pub const fn initial() -> Self {
        Self {
            offset: 0,
            accepting: false,
            weak: false,
            shift_orth_from_previous: false,
            sink: false,
            failed: false,
        }
    }

    pub const fn failed() -> Self {
        Self {
            offset: 0,
            accepting: false,
            weak: false,
            shift_orth_from_previous: false,
            sink: true,
            failed: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SegmentationRulesFsa<'a> {
    data: &'a [u8],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawFsaMatch<'a> {
    pub state_offset: usize,
    pub value: &'a [u8],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawFsaPrefixMatch<'a> {
    pub input_end: usize,
    pub state_offset: usize,
    pub value: &'a [u8],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawInterpsGroup<'a> {
    pub segment_type: u8,
    pub data: &'a [u8],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedForm {
    pub prefix_to_cut: u8,
    pub suffix_to_cut: u8,
    pub suffix_to_add: String,
    pub case_pattern: Vec<bool>,
    pub prefix_to_add: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedAnalyzerInterpretation {
    pub orth_case_pattern: Vec<bool>,
    pub form: EncodedForm,
    pub tag_id: i32,
    pub name_id: i32,
    pub labels_id: i32,
}

impl EncodedAnalyzerInterpretation {
    pub fn matches_orth_case(&self, orth: &str) -> bool {
        case_pattern_matches_orth(orth, &self.orth_case_pattern)
    }

    pub fn to_morph_interpretation(
        &self,
        orth: &str,
        start_node: i32,
        end_node: i32,
    ) -> Result<MorphInterpretation> {
        Ok(MorphInterpretation {
            start_node,
            end_node,
            orth: orth.to_owned(),
            lemma: decode_analyzer_lemma(orth, &self.form)?,
            tag_id: self.tag_id,
            name_id: self.name_id,
            labels_id: self.labels_id,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BinaryAnalyzerForm {
    prefix_to_cut: u8,
    suffix_to_cut: u8,
    suffix_to_add: String,
    case_pattern: BinaryCasePattern,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BinaryAnalyzerInterpretation {
    orth_case_pattern: BinaryCasePattern,
    form: BinaryAnalyzerForm,
    tag_id: i32,
    name_id: i32,
    labels_id: i32,
}

impl BinaryAnalyzerInterpretation {
    fn matches_orth_case(&self, orth: &str) -> bool {
        self.orth_case_pattern.matches_orth(orth)
    }

    fn to_morph_interpretation_in_context(
        &self,
        context: &AnalyzerOrthContext<'_>,
        start_node: i32,
        end_node: i32,
    ) -> Result<MorphInterpretation> {
        Ok(MorphInterpretation {
            start_node,
            end_node,
            orth: context.orth.to_owned(),
            lemma: decode_analyzer_lemma_for_form_in_context(context, &self.form)?,
            tag_id: self.tag_id,
            name_id: self.name_id,
            labels_id: self.labels_id,
        })
    }

    fn into_public(self) -> EncodedAnalyzerInterpretation {
        EncodedAnalyzerInterpretation {
            orth_case_pattern: self.orth_case_pattern.into_vec(),
            form: EncodedForm {
                prefix_to_cut: self.form.prefix_to_cut,
                suffix_to_cut: self.form.suffix_to_cut,
                suffix_to_add: self.form.suffix_to_add,
                case_pattern: self.form.case_pattern.into_vec(),
                prefix_to_add: String::new(),
            },
            tag_id: self.tag_id,
            name_id: self.name_id,
            labels_id: self.labels_id,
        }
    }
}

trait AnalyzerInterpretationView {
    fn matches_orth_case(&self, orth: &str) -> bool;
}

impl AnalyzerInterpretationView for EncodedAnalyzerInterpretation {
    fn matches_orth_case(&self, orth: &str) -> bool {
        self.matches_orth_case(orth)
    }
}

impl AnalyzerInterpretationView for BinaryAnalyzerInterpretation {
    fn matches_orth_case(&self, orth: &str) -> bool {
        self.matches_orth_case(orth)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum BinaryCasePattern {
    Lower,
    UpperPrefix(usize),
    Mixed(Vec<usize>),
}

impl BinaryCasePattern {
    fn is_empty(&self) -> bool {
        matches!(self, Self::Lower)
    }

    fn is_uppercase_at(&self, index: usize) -> bool {
        match self {
            Self::Lower => false,
            Self::UpperPrefix(len) => index < *len,
            Self::Mixed(indices) => indices.contains(&index),
        }
    }

    fn matches_orth(&self, orth: &str) -> bool {
        match self {
            Self::Lower => true,
            Self::UpperPrefix(len) => {
                let mut chars = orth.chars();
                for _ in 0..*len {
                    let Some(ch) = chars.next() else {
                        return false;
                    };
                    if char_is_lowercase_equivalent(ch) {
                        return false;
                    }
                }
                true
            }
            Self::Mixed(indices) => {
                let mut chars = orth.chars();
                let mut current = 0usize;
                for &raw_index in indices {
                    let wanted = raw_index as usize;
                    while current < wanted {
                        let Some(_) = chars.next() else {
                            return false;
                        };
                        current += 1;
                    }
                    let Some(ch) = chars.next() else {
                        return false;
                    };
                    if char_is_lowercase_equivalent(ch) {
                        return false;
                    }
                    current += 1;
                }
                true
            }
        }
    }

    fn shifted_by_lower_prefix(&self, prefix_len: usize) -> Self {
        if prefix_len == 0 || self.is_empty() {
            return self.clone();
        }
        match self {
            Self::Lower => Self::Lower,
            Self::UpperPrefix(len) => {
                Self::Mixed((0..*len).map(|index| index + prefix_len).collect())
            }
            Self::Mixed(indices) => {
                Self::Mixed(indices.iter().map(|index| index + prefix_len).collect())
            }
        }
    }

    fn into_vec(self) -> Vec<bool> {
        match self {
            Self::Lower => Vec::new(),
            Self::UpperPrefix(len) => vec![true; len],
            Self::Mixed(indices) => {
                let Some(max_index) = indices.iter().copied().max() else {
                    return Vec::new();
                };
                let mut pattern = vec![false; max_index + 1];
                for index in indices {
                    pattern[index] = true;
                }
                pattern
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedAnalyzerInterpsGroup {
    pub segment_type: u8,
    pub interpretations: Vec<EncodedAnalyzerInterpretation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedGeneratorInterpretation {
    pub homonym_id: String,
    pub form: EncodedForm,
    pub tag_id: i32,
    pub name_id: i32,
    pub labels_id: i32,
}

impl EncodedGeneratorInterpretation {
    pub fn to_morph_interpretation(
        &self,
        lemma: &str,
        start_node: i32,
        end_node: i32,
    ) -> Result<MorphInterpretation> {
        let mut orth = self.form.prefix_to_add.clone();
        orth.push_str(drop_suffix_chars(lemma, self.form.suffix_to_cut as usize)?);
        orth.push_str(&self.form.suffix_to_add);

        let lemma = if self.homonym_id.is_empty() {
            lemma.to_owned()
        } else {
            format!("{lemma}:{}", self.homonym_id)
        };

        Ok(MorphInterpretation {
            start_node,
            end_node,
            orth,
            lemma,
            tag_id: self.tag_id,
            name_id: self.name_id,
            labels_id: self.labels_id,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedGeneratorInterpsGroup {
    pub segment_type: u8,
    pub interpretations: Vec<EncodedGeneratorInterpretation>,
}

#[derive(Debug, Clone, Copy)]
pub enum BinaryFsa<'a> {
    Simple(SimpleFsa<'a>),
    VLength1(VLength1Fsa<'a>),
    VLength2(VLength2Fsa<'a>),
}

#[derive(Debug, Clone, Copy)]
pub struct SimpleFsa<'a> {
    data: &'a [u8],
    transition_size: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct VLength1Fsa<'a> {
    data: &'a [u8],
    short_labels: &'a [u8; 256],
}

#[derive(Debug, Clone, Copy)]
pub struct VLength2Fsa<'a> {
    data: &'a [u8],
}

#[derive(Debug, Clone)]
pub struct BinaryAnalyzerLexicon {
    data: BinaryDictionaryData,
    // `Arc` so per-thread forks share the (large, immutable) id tables instead
    // of deep-copying them.
    resolver: Arc<IdResolver>,
    segmentation_metadata: SegmentationMetadata,
    default_segmentation_variant_index: Option<usize>,
    analyzer_decode_cache: SharedAnalyzerGroupDecodeCache,
}

#[derive(Debug, Clone)]
pub struct BinaryGeneratorLexicon {
    data: BinaryDictionaryData,
    resolver: Arc<IdResolver>,
    segmentation_metadata: SegmentationMetadata,
    default_segmentation_variant_index: Option<usize>,
    generator_decode_cache: SharedGeneratorGroupDecodeCache,
}

#[derive(Debug, Clone)]
pub struct BinaryLexicon {
    analyzer: Option<BinaryAnalyzerLexicon>,
    generator: Option<BinaryGeneratorLexicon>,
    id: String,
    copyright: String,
    resolver: Arc<IdResolver>,
}

impl BinaryAnalyzerLexicon {
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self> {
        Self::from_data(BinaryDictionaryData::from_path(path)?)
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self> {
        Self::from_data(BinaryDictionaryData::from_bytes(bytes)?)
    }

    pub fn from_data(data: BinaryDictionaryData) -> Result<Self> {
        let _ = data.fsa()?;
        let resolver = Arc::new(data.id_resolver()?);
        let segmentation_metadata = data.segmentation_metadata()?;
        let default_segmentation_variant_index =
            default_segmentation_fsa_variant_index(&segmentation_metadata);
        Ok(Self {
            data,
            resolver,
            segmentation_metadata,
            default_segmentation_variant_index,
            analyzer_decode_cache: SharedAnalyzerGroupDecodeCache::default(),
        })
    }

    pub fn lookup_encoded_groups(
        &self,
        orth: &str,
    ) -> Result<Option<Vec<EncodedAnalyzerInterpsGroup>>> {
        let lookup: String = orth.chars().map(crate::case_tables::to_lower_char).collect();
        let Some(raw_match) = self
            .data
            .fsa_unchecked()
            .try_recognize_loaded(lookup.as_bytes())?
        else {
            return Ok(None);
        };
        Ok(Some(decode_analyzer_interps_groups(raw_match.value)?))
    }

    pub fn analyze_word_with_segmentation(
        &self,
        orth: &str,
        start_node: i32,
        case_handling: CaseHandling,
        segmentation: &SegmentationPreset,
    ) -> Result<Option<(Vec<MorphInterpretation>, i32)>> {
        let Some(word_paths) = self.word_paths(orth, segmentation)? else {
            return Ok(None);
        };
        let BinaryAnalyzerWordPaths {
            paths,
            decode_cache,
        } = word_paths;
        paths_to_morph_interpretations(paths, &decode_cache, start_node, case_handling)
    }

    /// Collects the raw FSA+segrules segmentation paths for a single word.
    /// Returns `None` when the dictionary has no segmentation FSA for the given
    /// options, `Some(vec![])` when the FSA produced no accepting path (the word
    /// is unknown), and `Some(paths)` otherwise. This separation lets callers
    /// distinguish "unknown word" (drives ign separator splitting) from "graph
    /// produced but case-filtered to nothing" (drives a whole-word ignotium),
    /// matching C++ `processOneWord`.
    fn word_paths<'a>(
        &self,
        orth: &'a str,
        segmentation: &SegmentationPreset,
    ) -> Result<Option<BinaryAnalyzerWordPaths<'a>>> {
        let Some(segmentation_fsa) = self.segmentation_fsa(segmentation)? else {
            return Ok(None);
        };
        // A segmentation FSA with no initial transitions (the `[1, 0]`
        // placeholder used by minimal/hand-built dictionaries) means the
        // dictionary carries no real segmentation rules; signal that to the
        // caller so it falls back to a plain lookup instead of ign splitting.
        if !has_initial_segmentation_transitions(Some(segmentation_fsa)) {
            return Ok(None);
        }
        if orth.is_empty() {
            return Ok(Some(BinaryAnalyzerWordPaths::empty()));
        }

        let rules_fsa = SegmentationRulesFsa::from_data_unchecked(segmentation_fsa);
        let fsa = self.data.fsa_unchecked();
        let normalized = lowercase_with_original_boundaries(orth);
        let path_capacity = normalized.path_capacity_hint();
        let mut paths = Vec::with_capacity(4);
        let mut current_path = Vec::with_capacity(path_capacity);
        let mut decode_cache =
            AnalyzerGroupDecodeCache::with_capacity(self.analyzer_decode_cache.clone(), 8);

        collect_segmented_analyzer_paths(
            fsa,
            &rules_fsa,
            &normalized,
            orth,
            0,
            rules_fsa.initial_state(),
            &mut current_path,
            &mut paths,
            &mut decode_cache,
        )?;

        Ok(Some(BinaryAnalyzerWordPaths {
            paths,
            decode_cache,
        }))
    }

    /// Faithful port of C++ `MorfeuszImpl::processOneWord` for a single
    /// whitespace-delimited word (whitespace handling lives in the engine).
    ///
    /// Returns the interpretations plus the next free node number. Unknown
    /// chunks are split on dictionary-defined separator characters and each part
    /// re-analyzed, exactly like `handleIgnChunk`; a wholly unknown chunk (or a
    /// chunk that produced a graph but no decodable results) becomes a single
    /// `ign` spanning the word.
    pub fn analyze_native_word(
        &self,
        word: &str,
        start_node: i32,
        case_handling: CaseHandling,
        segmentation: &SegmentationPreset,
    ) -> Result<(Vec<MorphInterpretation>, i32)> {
        self.process_one_word(word, start_node, case_handling, segmentation, false)
    }

    fn process_one_word(
        &self,
        word: &str,
        start_node: i32,
        case_handling: CaseHandling,
        segmentation: &SegmentationPreset,
        inside_ign_handler: bool,
    ) -> Result<(Vec<MorphInterpretation>, i32)> {
        if word.is_empty() {
            return Ok((Vec::new(), start_node));
        }

        match self.word_paths(word, segmentation)? {
            // Real builder dictionaries expose a segmentation FSA.
            Some(word_paths) if !word_paths.paths.is_empty() => {
                let BinaryAnalyzerWordPaths {
                    paths,
                    decode_cache,
                } = word_paths;
                if let Some((interps, nodes)) =
                    paths_to_morph_interpretations(paths, &decode_cache, start_node, case_handling)?
                {
                    return Ok((interps, start_node + nodes));
                }
                // Graph existed but decoded to nothing (e.g. case-filtered):
                // C++ appends a single ignotium for the whole word.
                Ok((vec![ignotium(word, start_node)], start_node + 1))
            }
            // Segmentation FSA present but no accepting path: unknown word. C++
            // splits it on dictionary separators and re-analyzes each part,
            // unless we are already inside the ign handler.
            Some(_) if inside_ign_handler => Ok((vec![ignotium(word, start_node)], start_node + 1)),
            Some(_) => self.handle_ign_chunk(word, start_node, case_handling, segmentation),
            // No segmentation FSA at all (minimal / hand-built dictionaries):
            // fall back to a plain case-aware single-edge lookup.
            None => match self.lookup_word_interpretations(word, start_node, case_handling)? {
                Some(interps) => Ok((interps, start_node + 1)),
                None => Ok((vec![ignotium(word, start_node)], start_node + 1)),
            },
        }
    }

    /// Plain single-edge, case-aware lookup of a whole word. Used only when the
    /// dictionary has no segmentation FSA (the real builder always emits one).
    fn lookup_word_interpretations(
        &self,
        word: &str,
        start_node: i32,
        case_handling: CaseHandling,
    ) -> Result<Option<Vec<MorphInterpretation>>> {
        let Some(groups) = self.lookup_encoded_groups(word)? else {
            return Ok(None);
        };
        let mut result = Vec::new();
        for group in groups {
            for_each_case_compatible_interpretation(
                word,
                &group.interpretations,
                case_handling,
                |interp| {
                    result.push(interp.to_morph_interpretation(
                        word,
                        start_node,
                        start_node + 1,
                    )?);
                    Ok(())
                },
            )?;
        }
        Ok((!result.is_empty()).then_some(result))
    }

    /// Port of C++ `handleIgnChunk`: split an unknown chunk into maximal
    /// non-separator runs and individual separator characters, re-analyzing each
    /// part. If the chunk contains no separators at all it stays a single `ign`.
    fn handle_ign_chunk(
        &self,
        word: &str,
        start_node: i32,
        case_handling: CaseHandling,
        segmentation: &SegmentationPreset,
    ) -> Result<(Vec<MorphInterpretation>, i32)> {
        let separators = &self.segmentation_metadata.separators;
        let is_separator = |c: char| separators.binary_search(&(c as u32)).is_ok();

        if !word.chars().any(is_separator) {
            return Ok((vec![ignotium(word, start_node)], start_node + 1));
        }

        let mut results = Vec::new();
        let mut node = start_node;
        let mut run_start = 0usize;
        let mut pending_non_sep: Option<(usize, usize)> = None;

        for (index, ch) in word.char_indices() {
            if is_separator(ch) {
                if let Some((s, e)) = pending_non_sep.take() {
                    let (interps, next) = self.process_one_word(
                        &word[s..e],
                        node,
                        case_handling,
                        segmentation,
                        true,
                    )?;
                    results.extend(interps);
                    node = next;
                }
                let sep_end = index + ch.len_utf8();
                let (interps, next) = self.process_one_word(
                    &word[index..sep_end],
                    node,
                    case_handling,
                    segmentation,
                    true,
                )?;
                results.extend(interps);
                node = next;
                run_start = sep_end;
            } else {
                pending_non_sep = Some((
                    pending_non_sep.map(|(s, _)| s).unwrap_or(run_start),
                    index + ch.len_utf8(),
                ));
            }
        }

        if let Some((s, e)) = pending_non_sep {
            let (interps, next) =
                self.process_one_word(&word[s..e], node, case_handling, segmentation, true)?;
            results.extend(interps);
            node = next;
        }

        Ok((results, node))
    }

    fn segmentation_fsa(&self, segmentation: &SegmentationPreset) -> Result<Option<&[u8]>> {
        if segmentation.aggl().is_none() && segmentation.praet().is_none() {
            if let Some(index) = self.default_segmentation_variant_index {
                return Ok(Some(
                    self.segmentation_metadata.fsa_variants[index]
                        .fsa
                        .as_slice(),
                ));
            }
        }
        segmentation_fsa_for_options(&self.segmentation_metadata, segmentation)
    }

    fn validate_segmentation(
        &self,
        segmentation: &SegmentationPreset,
        option: &str,
        value: &str,
    ) -> Result<()> {
        validate_segmentation_options(&self.segmentation_metadata, segmentation, option, value)
    }

    fn has_segmentation_transitions(&self, segmentation: &SegmentationPreset) -> Result<bool> {
        Ok(has_initial_segmentation_transitions(
            self.segmentation_fsa(segmentation)?,
        ))
    }
}

impl BinaryGeneratorLexicon {
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self> {
        Self::from_data(BinaryDictionaryData::from_path(path)?)
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self> {
        Self::from_data(BinaryDictionaryData::from_bytes(bytes)?)
    }

    pub fn from_data(data: BinaryDictionaryData) -> Result<Self> {
        let _ = data.fsa()?;
        let resolver = Arc::new(data.id_resolver()?);
        let segmentation_metadata = data.segmentation_metadata()?;
        let default_segmentation_variant_index =
            default_segmentation_fsa_variant_index(&segmentation_metadata);
        Ok(Self {
            data,
            resolver,
            segmentation_metadata,
            default_segmentation_variant_index,
            generator_decode_cache: SharedGeneratorGroupDecodeCache::default(),
        })
    }

    pub fn synthesize_encoded_groups(
        &self,
        lemma: &str,
    ) -> Result<Vec<EncodedGeneratorInterpsGroup>> {
        let Some(raw_match) = self
            .data
            .fsa_unchecked()
            .try_recognize_loaded(lemma.as_bytes())?
        else {
            return Ok(Vec::new());
        };
        decode_generator_interps_groups(raw_match.value)
    }

    pub fn synthesize_with_segmentation(
        &self,
        lemma: &str,
        segmentation: &SegmentationPreset,
    ) -> Result<Vec<MorphInterpretation>> {
        let (lookup_lemma, required_homonym_id) = split_generator_lemma(lemma);
        let Some(segmentation_fsa) = self.segmentation_fsa(segmentation)? else {
            return Ok(Vec::new());
        };
        if lookup_lemma.is_empty() {
            return Ok(Vec::new());
        }

        let rules_fsa = SegmentationRulesFsa::from_data_unchecked(segmentation_fsa);
        let fsa = self.data.fsa_unchecked();
        let mut paths = Vec::with_capacity(2);
        let mut current_path = Vec::with_capacity(4);
        let mut decode_cache =
            GeneratorGroupDecodeCache::with_capacity(self.generator_decode_cache.clone(), 4);

        collect_segmented_generator_paths(
            fsa,
            &rules_fsa,
            lookup_lemma,
            0,
            rules_fsa.initial_state(),
            &mut current_path,
            &mut paths,
            &mut decode_cache,
        )?;

        generator_paths_to_morph_interpretations(paths, &decode_cache, required_homonym_id)
    }

    fn segmentation_fsa(&self, segmentation: &SegmentationPreset) -> Result<Option<&[u8]>> {
        if segmentation.aggl().is_none() && segmentation.praet().is_none() {
            if let Some(index) = self.default_segmentation_variant_index {
                return Ok(Some(
                    self.segmentation_metadata.fsa_variants[index]
                        .fsa
                        .as_slice(),
                ));
            }
        }
        segmentation_fsa_for_options(&self.segmentation_metadata, segmentation)
    }

    fn validate_segmentation(
        &self,
        segmentation: &SegmentationPreset,
        option: &str,
        value: &str,
    ) -> Result<()> {
        validate_segmentation_options(&self.segmentation_metadata, segmentation, option, value)
    }

    fn has_segmentation_transitions(&self, segmentation: &SegmentationPreset) -> Result<bool> {
        Ok(has_initial_segmentation_transitions(
            self.segmentation_fsa(segmentation)?,
        ))
    }
}

impl BinaryLexicon {
    pub fn from_paths(
        analyzer_path: Option<impl AsRef<Path>>,
        generator_path: Option<impl AsRef<Path>>,
    ) -> Result<Self> {
        let analyzer = analyzer_path
            .map(BinaryAnalyzerLexicon::from_path)
            .transpose()?;
        let generator = generator_path
            .map(BinaryGeneratorLexicon::from_path)
            .transpose()?;
        Self::new(analyzer, generator)
    }

    pub fn new(
        analyzer: Option<BinaryAnalyzerLexicon>,
        generator: Option<BinaryGeneratorLexicon>,
    ) -> Result<Self> {
        let Some(primary) = analyzer
            .as_ref()
            .map(|lexicon| {
                (
                    lexicon.id().to_owned(),
                    lexicon.copyright().to_owned(),
                    lexicon.resolver().clone(),
                )
            })
            .or_else(|| {
                generator.as_ref().map(|lexicon| {
                    (
                        lexicon.id().to_owned(),
                        lexicon.copyright().to_owned(),
                        lexicon.resolver().clone(),
                    )
                })
            })
        else {
            return Err(Error::invalid_argument(
                "binary lexicon requires analyzer or generator dictionary",
            ));
        };

        Ok(Self {
            analyzer,
            generator,
            id: primary.0,
            copyright: primary.1,
            resolver: Arc::new(primary.2),
        })
    }
}

impl Lexicon for BinaryAnalyzerLexicon {
    fn try_fork(&self) -> Option<Arc<dyn Lexicon>> {
        // Share the immutable dictionary (Arc'd bytes) but start a fresh,
        // uncontended decode cache for this thread's copy.
        let mut forked = self.clone();
        forked.analyzer_decode_cache = SharedAnalyzerGroupDecodeCache::default();
        Some(Arc::new(forked))
    }

    fn id(&self) -> &str {
        self.data.dict_id()
    }

    fn copyright(&self) -> &str {
        self.data.copyright()
    }

    fn resolver(&self) -> &IdResolver {
        &*self.resolver
    }

    fn lookup(&self, _orth: &str) -> Option<&[DictionaryEntry]> {
        None
    }

    fn synthesize(&self, _lemma: &str) -> Option<&[DictionaryEntry]> {
        None
    }

    fn default_aggl(&self) -> Option<&str> {
        self.segmentation_metadata
            .default_options
            .get("aggl")
            .map(String::as_str)
    }

    fn default_praet(&self) -> Option<&str> {
        self.segmentation_metadata
            .default_options
            .get("praet")
            .map(String::as_str)
    }

    fn available_aggl_options(&self) -> Vec<String> {
        available_options_vec(&self.segmentation_metadata, "aggl")
    }

    fn available_praet_options(&self) -> Vec<String> {
        available_options_vec(&self.segmentation_metadata, "praet")
    }

    fn validate_segmentation(
        &self,
        segmentation: &SegmentationPreset,
        option: &str,
        value: &str,
    ) -> Result<()> {
        validate_segmentation_options(&self.segmentation_metadata, segmentation, option, value)
    }

    fn is_native_analyzer(&self) -> bool {
        true
    }

    fn analyze_native_word(
        &self,
        word: &str,
        start_node: i32,
        case_handling: CaseHandling,
        segmentation: &SegmentationPreset,
    ) -> Result<(Vec<MorphInterpretation>, i32)> {
        self.analyze_native_word(word, start_node, case_handling, segmentation)
    }

    fn analyze_word_interpretations(
        &self,
        orth: &str,
        start_node: i32,
        case_handling: CaseHandling,
        segmentation: &SegmentationPreset,
    ) -> Result<Option<(Vec<MorphInterpretation>, i32)>> {
        self.analyze_word_with_segmentation(orth, start_node, case_handling, segmentation)
    }

    fn lookup_interpretations(
        &self,
        orth: &str,
        start_node: i32,
        end_node: i32,
        result_orth: &str,
        case_handling: CaseHandling,
        segmentation: &SegmentationPreset,
    ) -> Result<Option<Vec<MorphInterpretation>>> {
        if orth == result_orth && self.has_segmentation_transitions(segmentation)? {
            return Ok(None);
        }

        let Some(groups) = self.lookup_encoded_groups(orth)? else {
            return Ok(None);
        };
        let mut result = Vec::new();
        for group in groups {
            for_each_case_compatible_interpretation(
                orth,
                &group.interpretations,
                case_handling,
                |interp| {
                    let mut morph = interp.to_morph_interpretation(orth, start_node, end_node)?;
                    morph.orth = result_orth.to_owned();
                    result.push(morph);
                    Ok(())
                },
            )?;
        }
        Ok((!result.is_empty()).then_some(result))
    }
}

impl Lexicon for BinaryLexicon {
    fn try_fork(&self) -> Option<Arc<dyn Lexicon>> {
        let mut forked = self.clone();
        if let Some(analyzer) = &mut forked.analyzer {
            analyzer.analyzer_decode_cache = SharedAnalyzerGroupDecodeCache::default();
        }
        if let Some(generator) = &mut forked.generator {
            generator.generator_decode_cache = SharedGeneratorGroupDecodeCache::default();
        }
        Some(Arc::new(forked))
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn copyright(&self) -> &str {
        &self.copyright
    }

    fn resolver(&self) -> &IdResolver {
        &*self.resolver
    }

    fn lookup(&self, _orth: &str) -> Option<&[DictionaryEntry]> {
        None
    }

    fn synthesize(&self, _lemma: &str) -> Option<&[DictionaryEntry]> {
        None
    }

    fn default_aggl(&self) -> Option<&str> {
        match (&self.analyzer, &self.generator) {
            (Some(analyzer), _) => analyzer.default_aggl(),
            (None, Some(generator)) => generator.default_aggl(),
            (None, None) => None,
        }
    }

    fn default_praet(&self) -> Option<&str> {
        match (&self.analyzer, &self.generator) {
            (Some(analyzer), _) => analyzer.default_praet(),
            (None, Some(generator)) => generator.default_praet(),
            (None, None) => None,
        }
    }

    fn available_aggl_options(&self) -> Vec<String> {
        match (&self.analyzer, &self.generator) {
            (Some(analyzer), _) => analyzer.available_aggl_options(),
            (None, Some(generator)) => generator.available_aggl_options(),
            (None, None) => Vec::new(),
        }
    }

    fn available_praet_options(&self) -> Vec<String> {
        match (&self.analyzer, &self.generator) {
            (Some(analyzer), _) => analyzer.available_praet_options(),
            (None, Some(generator)) => generator.available_praet_options(),
            (None, None) => Vec::new(),
        }
    }

    fn validate_segmentation(
        &self,
        segmentation: &SegmentationPreset,
        option: &str,
        value: &str,
    ) -> Result<()> {
        if let Some(analyzer) = &self.analyzer {
            analyzer.validate_segmentation(segmentation, option, value)?;
        }
        if let Some(generator) = &self.generator {
            generator.validate_segmentation(segmentation, option, value)?;
        }
        Ok(())
    }

    fn is_native_analyzer(&self) -> bool {
        self.analyzer.is_some()
    }

    fn analyze_native_word(
        &self,
        word: &str,
        start_node: i32,
        case_handling: CaseHandling,
        segmentation: &SegmentationPreset,
    ) -> Result<(Vec<MorphInterpretation>, i32)> {
        match &self.analyzer {
            Some(analyzer) => {
                analyzer.analyze_native_word(word, start_node, case_handling, segmentation)
            }
            None => Ok((Vec::new(), start_node)),
        }
    }

    fn analyze_word_interpretations(
        &self,
        orth: &str,
        start_node: i32,
        case_handling: CaseHandling,
        segmentation: &SegmentationPreset,
    ) -> Result<Option<(Vec<MorphInterpretation>, i32)>> {
        match &self.analyzer {
            Some(analyzer) => {
                analyzer.analyze_word_interpretations(orth, start_node, case_handling, segmentation)
            }
            None => Ok(None),
        }
    }

    fn lookup_interpretations(
        &self,
        orth: &str,
        start_node: i32,
        end_node: i32,
        result_orth: &str,
        case_handling: CaseHandling,
        segmentation: &SegmentationPreset,
    ) -> Result<Option<Vec<MorphInterpretation>>> {
        match &self.analyzer {
            Some(analyzer) => analyzer.lookup_interpretations(
                orth,
                start_node,
                end_node,
                result_orth,
                case_handling,
                segmentation,
            ),
            None => Ok(None),
        }
    }

    fn synthesize_interpretations(
        &self,
        lemma: &str,
        segmentation: &SegmentationPreset,
    ) -> Result<Vec<MorphInterpretation>> {
        match &self.generator {
            Some(generator) => generator.synthesize_interpretations(lemma, segmentation),
            None => Ok(Vec::new()),
        }
    }
}

impl Lexicon for BinaryGeneratorLexicon {
    fn try_fork(&self) -> Option<Arc<dyn Lexicon>> {
        let mut forked = self.clone();
        forked.generator_decode_cache = SharedGeneratorGroupDecodeCache::default();
        Some(Arc::new(forked))
    }

    fn id(&self) -> &str {
        self.data.dict_id()
    }

    fn copyright(&self) -> &str {
        self.data.copyright()
    }

    fn resolver(&self) -> &IdResolver {
        &*self.resolver
    }

    fn lookup(&self, _orth: &str) -> Option<&[DictionaryEntry]> {
        None
    }

    fn synthesize(&self, _lemma: &str) -> Option<&[DictionaryEntry]> {
        None
    }

    fn default_aggl(&self) -> Option<&str> {
        self.segmentation_metadata
            .default_options
            .get("aggl")
            .map(String::as_str)
    }

    fn default_praet(&self) -> Option<&str> {
        self.segmentation_metadata
            .default_options
            .get("praet")
            .map(String::as_str)
    }

    fn available_aggl_options(&self) -> Vec<String> {
        available_options_vec(&self.segmentation_metadata, "aggl")
    }

    fn available_praet_options(&self) -> Vec<String> {
        available_options_vec(&self.segmentation_metadata, "praet")
    }

    fn validate_segmentation(
        &self,
        segmentation: &SegmentationPreset,
        option: &str,
        value: &str,
    ) -> Result<()> {
        validate_segmentation_options(&self.segmentation_metadata, segmentation, option, value)
    }

    fn synthesize_interpretations(
        &self,
        lemma: &str,
        segmentation: &SegmentationPreset,
    ) -> Result<Vec<MorphInterpretation>> {
        let segmented = self.synthesize_with_segmentation(lemma, segmentation)?;
        if !segmented.is_empty() {
            return Ok(segmented);
        }
        if self.has_segmentation_transitions(segmentation)? {
            return Ok(Vec::new());
        }

        let (lookup_lemma, required_homonym_id) = split_generator_lemma(lemma);
        let mut result = Vec::new();
        for group in self.synthesize_encoded_groups(lookup_lemma)? {
            for interp in group.interpretations {
                if !generator_homonym_matches(&interp, required_homonym_id) {
                    continue;
                }
                result.push(interp.to_morph_interpretation(lookup_lemma, 0, 0)?);
            }
        }
        Ok(result)
    }
}

fn has_initial_segmentation_transitions(fsa: Option<&[u8]>) -> bool {
    fsa.and_then(|fsa| fsa.get(1)).copied().unwrap_or(0) > 0
}

fn default_segmentation_fsa_variant_index(metadata: &SegmentationMetadata) -> Option<usize> {
    metadata
        .fsa_variants
        .iter()
        .position(|variant| variant.options == metadata.default_options)
}

fn segmentation_fsa_for_options<'a>(
    metadata: &'a SegmentationMetadata,
    segmentation: &SegmentationPreset,
) -> Result<Option<&'a [u8]>> {
    if metadata.fsa_variants.is_empty() {
        return Ok(None);
    }

    // Start from the dictionary's own default options, then apply only the
    // options the caller explicitly set. This mirrors C++: unset `aggl`/`praet`
    // take the dictionary default (e.g. SGJP defaults `aggl=isolated`), so the
    // out-of-the-box behavior matches the reference exactly.
    if let Some(variant) = metadata.fsa_variant_for_preset(segmentation) {
        return Ok(Some(variant.fsa.as_slice()));
    }

    let options = effective_segmentation_options(metadata, segmentation);
    if let Some(variant) = metadata.fsa_variant_for_options(&options) {
        return Ok(Some(variant.fsa.as_slice()));
    }
    Err(Error::InvalidArgument(format!(
        "Invalid segmentation options: {}",
        format_options_map(&options)
    )))
}

fn available_options_vec(metadata: &SegmentationMetadata, option: &str) -> Vec<String> {
    metadata.available_options(option).into_iter().collect()
}

fn validate_segmentation_options(
    metadata: &SegmentationMetadata,
    segmentation: &SegmentationPreset,
    option: &str,
    value: &str,
) -> Result<()> {
    if metadata.fsa_variants.is_empty() {
        return Ok(());
    }
    if !metadata.default_options.contains_key(option) {
        return Err(Error::InvalidArgument(format!(
            "Invalid segmentation option '{option}'"
        )));
    }

    if metadata.fsa_variant_for_preset(segmentation).is_some() {
        return Ok(());
    }

    Err(Error::InvalidArgument(format!(
        "Invalid \"{option}\" option: \"{value}\". Possible values: {}",
        format_available_options(metadata, option)
    )))
}

fn effective_segmentation_options(
    metadata: &SegmentationMetadata,
    segmentation: &SegmentationPreset,
) -> BTreeMap<String, String> {
    let mut options = metadata.default_options.clone();
    if let Some(aggl) = segmentation.aggl() {
        options.insert("aggl".to_owned(), aggl.to_owned());
    }
    if let Some(praet) = segmentation.praet() {
        options.insert("praet".to_owned(), praet.to_owned());
    }
    options
}

fn format_available_options(metadata: &SegmentationMetadata, option: &str) -> String {
    metadata
        .available_options(option)
        .into_iter()
        .map(|value| format!("\"{value}\""))
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_options_map(options: &BTreeMap<String, String>) -> String {
    options
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join(", ")
}

impl BinaryDictionaryData {
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self> {
        Self::from_bytes(fs::read(path)?)
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self> {
        validate_min_len(&bytes, FSA_DATA_OFFSET, "dictionary prologue")?;
        let magic = read_u32_at(&bytes, 0, "magic number")?;
        if magic != MAGIC_NUMBER {
            return Err(Error::invalid_dictionary(format!(
                "invalid dictionary magic: 0x{magic:08x}"
            )));
        }

        let version = bytes[VERSION_NUM_OFFSET];
        if version != VERSION_NUM {
            return Err(Error::invalid_dictionary(format!(
                "unsupported dictionary version: {version}"
            )));
        }

        let implementation = FsaImplementation::from_code(bytes[IMPLEMENTATION_NUM_OFFSET])?;
        let fsa_size = read_u32_at(&bytes, FSA_DATA_SIZE_OFFSET, "FSA size")? as usize;
        let fsa_end = FSA_DATA_OFFSET
            .checked_add(fsa_size)
            .filter(|end| *end <= bytes.len())
            .ok_or_else(|| Error::invalid_dictionary("FSA data exceeds dictionary size"))?;
        let fsa_range = FSA_DATA_OFFSET..fsa_end;
        let epilogue_offset = fsa_end;
        validate_min_len(&bytes, epilogue_offset + 4, "dictionary epilogue offset")?;

        let epilogue = &bytes[epilogue_offset..];
        let segmentation_rules_offset =
            read_u32_at(epilogue, 0, "segmentation rules offset")? as usize;
        let segmentation_rules_start = epilogue_offset
            .checked_add(4)
            .and_then(|offset| offset.checked_add(segmentation_rules_offset))
            .filter(|offset| *offset <= bytes.len())
            .ok_or_else(|| {
                Error::invalid_dictionary("segmentation rules offset exceeds dictionary size")
            })?;

        let metadata_start = epilogue_offset + 4;
        let (dict_id, copyright_start) = read_c_string_at(&bytes, metadata_start, "dictionary id")?;
        let (copyright, id_resolver_offset) =
            read_c_string_at(&bytes, copyright_start, "dictionary copyright")?;

        Ok(Self {
            bytes: bytes.into(),
            implementation,
            fsa_range,
            epilogue_offset,
            id_resolver_offset,
            segmentation_rules_offset: segmentation_rules_start,
            dict_id,
            copyright,
        })
    }

    pub fn version(&self) -> u8 {
        VERSION_NUM
    }

    pub fn implementation(&self) -> FsaImplementation {
        self.implementation
    }

    pub fn dict_id(&self) -> &str {
        &self.dict_id
    }

    pub fn copyright(&self) -> &str {
        &self.copyright
    }

    pub fn fsa_data(&self) -> &[u8] {
        &self.bytes[self.fsa_range.clone()]
    }

    pub fn epilogue(&self) -> &[u8] {
        &self.bytes[self.epilogue_offset..]
    }

    pub fn segmentation_rules_data(&self) -> &[u8] {
        &self.bytes[self.segmentation_rules_offset..]
    }

    pub fn segmentation_metadata(&self) -> Result<SegmentationMetadata> {
        parse_segmentation_metadata(self.segmentation_rules_data())
    }

    pub fn id_resolver(&self) -> Result<IdResolver> {
        let mut cursor = self.id_resolver_offset;
        let limit = self.segmentation_rules_offset;
        let mut resolver = IdResolver::default();

        let (tagset_id, next) = read_c_string_at_limit(&self.bytes, cursor, limit, "tagset id")?;
        resolver.set_tagset_id(tagset_id);
        cursor = next;

        read_id_string_table(&self.bytes, &mut cursor, limit, |id, value| {
            resolver.set_tag(id, value);
        })?;
        read_id_string_table(&self.bytes, &mut cursor, limit, |id, value| {
            resolver.set_name(id, value);
        })?;
        read_id_string_table(&self.bytes, &mut cursor, limit, |id, value| {
            resolver.set_labels_in_order(id, value);
        })?;

        Ok(resolver)
    }

    pub fn fsa(&self) -> Result<BinaryFsa<'_>> {
        match self.implementation {
            FsaImplementation::Simple => {
                Ok(BinaryFsa::Simple(SimpleFsa::new(self.fsa_data(), false)?))
            }
            FsaImplementation::SimpleWithTransitionData => {
                Ok(BinaryFsa::Simple(SimpleFsa::new(self.fsa_data(), true)?))
            }
            FsaImplementation::VLength1 => {
                Ok(BinaryFsa::VLength1(VLength1Fsa::new(self.fsa_data())?))
            }
            FsaImplementation::VLength2 => {
                Ok(BinaryFsa::VLength2(VLength2Fsa::new(self.fsa_data())))
            }
        }
    }

    fn fsa_unchecked(&self) -> BinaryFsa<'_> {
        match self.implementation {
            FsaImplementation::Simple => BinaryFsa::Simple(SimpleFsa {
                data: self.fsa_data(),
                transition_size: SIMPLE_TRANSITION_SIZE,
            }),
            FsaImplementation::SimpleWithTransitionData => BinaryFsa::Simple(SimpleFsa {
                data: self.fsa_data(),
                transition_size: SIMPLE_TRANSDUCER_TRANSITION_SIZE,
            }),
            FsaImplementation::VLength1 => {
                BinaryFsa::VLength1(VLength1Fsa::from_data_unchecked(self.fsa_data()))
            }
            FsaImplementation::VLength2 => BinaryFsa::VLength2(VLength2Fsa::new(self.fsa_data())),
        }
    }

    pub fn vlength2_fsa(&self) -> Result<VLength2Fsa<'_>> {
        match self.fsa()? {
            BinaryFsa::VLength2(fsa) => Ok(fsa),
            _ => Err(Error::invalid_dictionary(
                "dictionary does not use VLength2 FSA encoding",
            )),
        }
    }
}

impl<'a> BinaryFsa<'a> {
    pub fn try_recognize(&self, input: &[u8]) -> Result<Option<RawFsaMatch<'a>>> {
        match self {
            Self::Simple(fsa) => fsa.try_recognize(input),
            Self::VLength1(fsa) => fsa.try_recognize(input),
            Self::VLength2(fsa) => fsa.try_recognize(input),
        }
    }

    fn try_recognize_loaded(&self, input: &[u8]) -> Result<Option<RawFsaMatch<'a>>> {
        match self {
            Self::Simple(fsa) => fsa.try_recognize(input),
            Self::VLength1(fsa) => Ok(fsa.try_recognize_loaded(input)),
            Self::VLength2(fsa) => fsa.try_recognize(input),
        }
    }

    pub fn prefix_matches(&self, input: &[u8]) -> Result<Vec<RawFsaPrefixMatch<'a>>> {
        match self {
            Self::Simple(fsa) => fsa.prefix_matches(input),
            Self::VLength1(fsa) => fsa.prefix_matches(input),
            Self::VLength2(fsa) => fsa.prefix_matches(input),
        }
    }

    fn for_each_prefix_match_loaded<F>(&self, input: &[u8], visit: F) -> Result<()>
    where
        F: FnMut(RawFsaPrefixMatch<'a>) -> Result<()>,
    {
        match self {
            Self::Simple(fsa) => fsa.for_each_prefix_match(input, visit),
            Self::VLength1(fsa) => fsa.for_each_prefix_match_loaded(input, visit),
            Self::VLength2(fsa) => fsa.for_each_prefix_match(input, visit),
        }
    }
}

impl<'a> SimpleFsa<'a> {
    pub fn new(data: &'a [u8], has_transition_data: bool) -> Result<Self> {
        validate_min_len(data, 1, "Simple FSA initial state")?;
        Ok(Self {
            data,
            transition_size: if has_transition_data {
                SIMPLE_TRANSDUCER_TRANSITION_SIZE
            } else {
                SIMPLE_TRANSITION_SIZE
            },
        })
    }

    pub fn try_recognize(&self, input: &[u8]) -> Result<Option<RawFsaMatch<'a>>> {
        let mut state = RawFsaState::initial();
        for &byte in input {
            let Some(next_state) = self.proceed(byte, state)? else {
                return Ok(None);
            };
            state = next_state;
        }

        if state.accepting {
            Ok(Some(RawFsaMatch {
                state_offset: state.offset,
                value: state.value.unwrap_or(&[]),
            }))
        } else {
            Ok(None)
        }
    }

    pub fn prefix_matches(&self, input: &[u8]) -> Result<Vec<RawFsaPrefixMatch<'a>>> {
        let mut matches = Vec::new();
        self.for_each_prefix_match(input, |prefix_match| {
            matches.push(prefix_match);
            Ok(())
        })?;
        Ok(matches)
    }

    fn for_each_prefix_match<F>(&self, input: &[u8], mut visit: F) -> Result<()>
    where
        F: FnMut(RawFsaPrefixMatch<'a>) -> Result<()>,
    {
        let mut state = RawFsaState::initial();
        for (index, &byte) in input.iter().enumerate() {
            let Some(next_state) = self.proceed(byte, state)? else {
                break;
            };
            state = next_state;
            if state.accepting {
                visit(RawFsaPrefixMatch {
                    input_end: index + 1,
                    state_offset: state.offset,
                    value: state.value.unwrap_or(&[]),
                })?;
            }
        }

        Ok(())
    }

    fn proceed(&self, byte: u8, state: RawFsaState<'a>) -> Result<Option<RawFsaState<'a>>> {
        let state_header = read_simple_state_header(self.data, state.offset)?;
        let transitions_size = state_header
            .transitions_num
            .checked_mul(self.transition_size)
            .ok_or_else(|| Error::invalid_dictionary("Simple FSA transition table overflow"))?;
        let table_end = state_header
            .transitions_offset
            .checked_add(transitions_size)
            .ok_or_else(|| Error::invalid_dictionary("Simple FSA transition table overflow"))?;
        validate_min_len(self.data, table_end, "Simple FSA transitions")?;

        let mut cursor = state_header.transitions_offset;
        for _ in 0..state_header.transitions_num {
            let label = self.data[cursor];
            if label == byte {
                let next_offset = read_u24_at(self.data, cursor + 1, "Simple FSA target offset")?;
                return self.state_at(next_offset);
            }
            cursor += self.transition_size;
        }

        Ok(None)
    }

    fn state_at(&self, offset: usize) -> Result<Option<RawFsaState<'a>>> {
        let state_header = read_simple_state_header(self.data, offset)?;
        if state_header.accepting {
            Ok(Some(RawFsaState {
                offset,
                accepting: true,
                value: state_header.value,
                value_record_size: state_header.value_record_size,
                transitions_num: 0,
                transitions_offset: 0,
            }))
        } else {
            Ok(Some(RawFsaState {
                offset,
                accepting: false,
                value: None,
                value_record_size: 0,
                transitions_num: 0,
                transitions_offset: 0,
            }))
        }
    }
}

impl<'a> VLength1Fsa<'a> {
    pub fn new(data: &'a [u8]) -> Result<Self> {
        validate_min_len(data, V1_INITIAL_STATE_OFFSET, "VLength1 FSA prologue")?;
        if data[V1_INITIAL_STATE_OFFSET - 1] != b'^' {
            return Err(Error::invalid_dictionary(
                "VLength1 FSA prologue is missing magic marker",
            ));
        }
        let short_labels = data[..256]
            .try_into()
            .map_err(|_| Error::invalid_dictionary("VLength1 short-label table is truncated"))?;
        validate_reachable_vlength1_states(data)?;
        Ok(Self { data, short_labels })
    }

    fn from_data_unchecked(data: &'a [u8]) -> Self {
        let short_labels = data[..256]
            .try_into()
            .expect("VLength1 FSA was validated when the binary lexicon loaded");
        Self { data, short_labels }
    }

    pub fn try_recognize(&self, input: &[u8]) -> Result<Option<RawFsaMatch<'a>>> {
        let Some(mut state) = self.state_at(V1_INITIAL_STATE_OFFSET)? else {
            return Ok(None);
        };
        for &byte in input {
            let Some(next_state) = self.proceed(byte, state)? else {
                return Ok(None);
            };
            state = next_state;
        }

        if state.accepting {
            Ok(Some(RawFsaMatch {
                state_offset: state.offset,
                value: state.value.unwrap_or(&[]),
            }))
        } else {
            Ok(None)
        }
    }

    #[cfg_attr(not(debug_assertions), inline(always))]
    fn try_recognize_loaded(&self, input: &[u8]) -> Option<RawFsaMatch<'a>> {
        let mut state = self.state_at_loaded(V1_INITIAL_STATE_OFFSET);
        for &byte in input {
            let Some(next_state) = self.proceed_loaded(byte, state) else {
                return None;
            };
            state = next_state;
        }

        if state.accepting {
            Some(RawFsaMatch {
                state_offset: state.offset,
                value: state.value.unwrap_or(&[]),
            })
        } else {
            None
        }
    }

    pub fn prefix_matches(&self, input: &[u8]) -> Result<Vec<RawFsaPrefixMatch<'a>>> {
        let mut matches = Vec::new();
        self.for_each_prefix_match(input, |prefix_match| {
            matches.push(prefix_match);
            Ok(())
        })?;
        Ok(matches)
    }

    fn for_each_prefix_match<F>(&self, input: &[u8], mut visit: F) -> Result<()>
    where
        F: FnMut(RawFsaPrefixMatch<'a>) -> Result<()>,
    {
        let Some(mut state) = self.state_at(V1_INITIAL_STATE_OFFSET)? else {
            return Ok(());
        };
        for (index, &byte) in input.iter().enumerate() {
            let Some(next_state) = self.proceed(byte, state)? else {
                break;
            };
            state = next_state;
            if state.accepting {
                visit(RawFsaPrefixMatch {
                    input_end: index + 1,
                    state_offset: state.offset,
                    value: state.value.unwrap_or(&[]),
                })?;
            }
        }

        Ok(())
    }

    #[cfg_attr(not(debug_assertions), inline(always))]
    fn for_each_prefix_match_loaded<F>(&self, input: &[u8], mut visit: F) -> Result<()>
    where
        F: FnMut(RawFsaPrefixMatch<'a>) -> Result<()>,
    {
        let mut state = self.state_at_loaded(V1_INITIAL_STATE_OFFSET);
        for (index, &byte) in input.iter().enumerate() {
            let Some(next_state) = self.proceed_loaded(byte, state) else {
                break;
            };
            state = next_state;
            if state.accepting {
                visit(RawFsaPrefixMatch {
                    input_end: index + 1,
                    state_offset: state.offset,
                    value: state.value.unwrap_or(&[]),
                })?;
            }
        }

        Ok(())
    }

    fn proceed(&self, byte: u8, state: RawFsaState<'a>) -> Result<Option<RawFsaState<'a>>> {
        let mut cursor = state.transitions_offset;
        let input_short_label = self.short_labels[byte as usize];

        for _ in 0..state.transitions_num {
            validate_min_len(self.data, cursor + 1, "VLength1 transition")?;
            let first = self.data[cursor];
            cursor += 1;
            let offset_size = (first & V1_OFFSET_SIZE_MASK) as usize;
            let transition_short_label = first >> 2;

            let label_matches = if transition_short_label == 0 {
                validate_min_len(self.data, cursor + 1, "VLength1 transition label")?;
                let label = self.data[cursor];
                cursor += 1;
                input_short_label == 0 && label == byte
            } else {
                transition_short_label == input_short_label
            };

            if label_matches {
                let offset_end = cursor + offset_size;
                validate_min_len(self.data, offset_end, "VLength1 offset")?;
                let relative_offset = read_vlength1_offset_at(self.data, cursor, offset_size);
                let next_offset = offset_end
                    .checked_add(relative_offset)
                    .ok_or_else(|| Error::invalid_dictionary("VLength1 transition overflow"))?;
                return self.state_at(next_offset);
            }
            cursor += offset_size;
            validate_min_len(self.data, cursor, "VLength1 offset")?;
        }

        Ok(None)
    }

    #[cfg_attr(not(debug_assertions), inline(always))]
    fn proceed_loaded(&self, byte: u8, state: RawFsaState<'a>) -> Option<RawFsaState<'a>> {
        let data = self.data;
        let mut cursor = state.transitions_offset;
        let input_short_label = self.short_labels[byte as usize];

        for _ in 0..state.transitions_num {
            let first = byte_at_unchecked(data, cursor);
            cursor += 1;
            let offset_size = (first & V1_OFFSET_SIZE_MASK) as usize;
            let transition_short_label = first >> 2;

            let label_matches = if transition_short_label == 0 {
                let label = byte_at_unchecked(data, cursor);
                cursor += 1;
                input_short_label == 0 && label == byte
            } else {
                transition_short_label == input_short_label
            };

            let offset_end = cursor + offset_size;

            if label_matches {
                let relative_offset = read_vlength1_offset_at(data, cursor, offset_size);
                let next_offset = offset_end + relative_offset;
                return Some(self.state_at_loaded(next_offset));
            }
            cursor = offset_end;
        }

        None
    }

    fn state_at(&self, offset: usize) -> Result<Option<RawFsaState<'a>>> {
        validate_min_len(self.data, offset + 1, "VLength1 target state")?;
        let state_header = read_vlength1_state_header(self.data, offset)?;
        let state_offset = offset
            .checked_sub(V1_INITIAL_STATE_OFFSET)
            .ok_or_else(|| Error::invalid_dictionary("VLength1 target precedes initial state"))?;

        if state_header.accepting {
            Ok(Some(RawFsaState {
                offset: state_offset,
                accepting: true,
                value: state_header.value,
                value_record_size: state_header.value_record_size,
                transitions_num: state_header.transitions_num,
                transitions_offset: state_header.transitions_offset,
            }))
        } else {
            Ok(Some(RawFsaState {
                offset: state_offset,
                accepting: false,
                value: None,
                value_record_size: 0,
                transitions_num: state_header.transitions_num,
                transitions_offset: state_header.transitions_offset,
            }))
        }
    }

    #[cfg_attr(not(debug_assertions), inline(always))]
    fn state_at_loaded(&self, offset: usize) -> RawFsaState<'a> {
        debug_assert!(offset >= V1_INITIAL_STATE_OFFSET);
        let state_header = read_vlength1_state_header_loaded(self.data, offset);
        let state_offset = offset - V1_INITIAL_STATE_OFFSET;

        if state_header.accepting {
            RawFsaState {
                offset: state_offset,
                accepting: true,
                value: state_header.value,
                value_record_size: state_header.value_record_size,
                transitions_num: state_header.transitions_num,
                transitions_offset: state_header.transitions_offset,
            }
        } else {
            RawFsaState {
                offset: state_offset,
                accepting: false,
                value: None,
                value_record_size: 0,
                transitions_num: state_header.transitions_num,
                transitions_offset: state_header.transitions_offset,
            }
        }
    }
}

impl<'a> VLength2Fsa<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data }
    }

    pub fn try_recognize(&self, input: &[u8]) -> Result<Option<RawFsaMatch<'a>>> {
        let mut state = RawFsaState::initial();
        for &byte in input {
            let Some(next_state) = self.proceed(byte, state)? else {
                return Ok(None);
            };
            state = next_state;
        }

        if state.accepting {
            Ok(Some(RawFsaMatch {
                state_offset: state.offset,
                value: state.value.unwrap_or(&[]),
            }))
        } else {
            Ok(None)
        }
    }

    pub fn prefix_matches(&self, input: &[u8]) -> Result<Vec<RawFsaPrefixMatch<'a>>> {
        let mut matches = Vec::new();
        self.for_each_prefix_match(input, |prefix_match| {
            matches.push(prefix_match);
            Ok(())
        })?;
        Ok(matches)
    }

    fn for_each_prefix_match<F>(&self, input: &[u8], mut visit: F) -> Result<()>
    where
        F: FnMut(RawFsaPrefixMatch<'a>) -> Result<()>,
    {
        let mut state = RawFsaState::initial();
        for (index, &byte) in input.iter().enumerate() {
            let Some(next_state) = self.proceed(byte, state)? else {
                break;
            };
            state = next_state;
            if state.accepting {
                visit(RawFsaPrefixMatch {
                    input_end: index + 1,
                    state_offset: state.offset,
                    value: state.value.unwrap_or(&[]),
                })?;
            }
        }

        Ok(())
    }

    fn proceed(&self, byte: u8, state: RawFsaState<'a>) -> Result<Option<RawFsaState<'a>>> {
        let mut cursor = state
            .offset
            .checked_add(state.value_record_size)
            .ok_or_else(|| Error::invalid_dictionary("FSA state offset overflow"))?;
        validate_min_len(self.data, cursor + 1, "VLength2 transition table")?;

        loop {
            let label = self.data[cursor];
            cursor += 1;
            if cursor >= self.data.len() && label & V2_LAST_FLAG != 0 {
                return Ok(None);
            }
            validate_min_len(self.data, cursor + 1, "VLength2 transition flags")?;
            let flags_offset = cursor;
            let flags = self.data[flags_offset];
            if label == byte {
                let relative_offset = read_vlength2_offset(self.data, &mut cursor)?;
                let next_offset = cursor
                    .checked_add(relative_offset)
                    .ok_or_else(|| Error::invalid_dictionary("VLength2 transition overflow"))?;
                validate_min_len(self.data, next_offset, "VLength2 target state")?;
                return RawFsaState::from_target(self.data, next_offset, flags);
            }

            if flags & V2_LAST_FLAG != 0 {
                return Ok(None);
            }
            skip_vlength2_offset(self.data, &mut cursor)?;
        }
    }
}

impl<'a> SegmentationRulesFsa<'a> {
    pub fn new(data: &'a [u8]) -> Result<Self> {
        validate_min_len(data, 2, "segmentation FSA initial state")?;
        validate_reachable_segmentation_states(data)?;
        Ok(Self { data })
    }

    fn from_data_unchecked(data: &'a [u8]) -> Self {
        Self { data }
    }

    pub fn initial_state(&self) -> SegmentationState {
        SegmentationState::initial()
    }

    pub fn proceed_to_next(
        &self,
        segnum: u8,
        state: SegmentationState,
        at_end_of_word: bool,
    ) -> Result<Option<SegmentationState>> {
        if state.failed {
            return Err(Error::invalid_argument(
                "cannot proceed from a failed segmentation state",
            ));
        }

        // `state.offset == 0` is the initial state, whose transition table header
        // is at data offset 0 — `find_transition` reads it the same way as any
        // other state.
        let candidate = self.find_transition(segnum, state)?;

        Ok(candidate.filter(|next| Self::allows_transition(*next, at_end_of_word)))
    }

    #[cfg_attr(not(debug_assertions), inline(always))]
    fn proceed_to_next_unchecked(
        &self,
        segnum: u8,
        state: SegmentationState,
        at_end_of_word: bool,
    ) -> Option<SegmentationState> {
        debug_assert!(!state.failed);
        let next = self.find_transition_unchecked(segnum, state)?;
        Self::allows_transition(next, at_end_of_word).then_some(next)
    }

    fn find_transition(
        &self,
        segnum: u8,
        state: SegmentationState,
    ) -> Result<Option<SegmentationState>> {
        validate_min_len(self.data, state.offset + 2, "segmentation FSA state")?;
        let table_start = state.offset + 2;
        let transitions_num = self.data[state.offset + 1] as usize;
        let table_end = checked_transition_table_end(table_start, transitions_num)?;
        validate_min_len(self.data, table_end, "segmentation FSA transitions")?;

        let mut cursor = table_end;
        for _ in 0..transitions_num {
            cursor -= SEGRULES_TRANSITION_SIZE;
            if self.data[cursor] == segnum {
                return Ok(Some(Self::transition_to_state(self.data, cursor)?));
            }
        }
        Ok(None)
    }

    #[cfg_attr(not(debug_assertions), inline(always))]
    fn find_transition_unchecked(
        &self,
        segnum: u8,
        state: SegmentationState,
    ) -> Option<SegmentationState> {
        let data = self.data;
        let transitions_num = byte_at_unchecked(data, state.offset + 1) as usize;
        let mut cursor = state.offset + 2 + transitions_num * SEGRULES_TRANSITION_SIZE;

        for _ in 0..transitions_num {
            cursor -= SEGRULES_TRANSITION_SIZE;
            if byte_at_unchecked(data, cursor) == segnum {
                return Some(Self::transition_to_state_unchecked(data, cursor));
            }
        }
        None
    }

    fn transition_to_state(data: &[u8], transition_offset: usize) -> Result<SegmentationState> {
        validate_min_len(
            data,
            transition_offset + SEGRULES_TRANSITION_SIZE,
            "segmentation FSA transition",
        )?;
        let shift_orth_from_previous = data[transition_offset + 1] != 0;
        let target_offset =
            u16::from_be_bytes([data[transition_offset + 2], data[transition_offset + 3]]) as usize;
        let mut state = Self::state_at(data, target_offset)?;
        state.shift_orth_from_previous = shift_orth_from_previous;
        Ok(state)
    }

    #[cfg_attr(not(debug_assertions), inline(always))]
    fn transition_to_state_unchecked(data: &[u8], transition_offset: usize) -> SegmentationState {
        let shift_orth_from_previous = byte_at_unchecked(data, transition_offset + 1) != 0;
        let target_offset = ((byte_at_unchecked(data, transition_offset + 2) as usize) << 8)
            | byte_at_unchecked(data, transition_offset + 3) as usize;
        let mut state = Self::state_at_unchecked(data, target_offset);
        state.shift_orth_from_previous = shift_orth_from_previous;
        state
    }

    fn state_at(data: &[u8], offset: usize) -> Result<SegmentationState> {
        validate_min_len(data, offset + 2, "segmentation FSA target state")?;
        let transitions_num = data[offset + 1] as usize;
        let table_start = offset
            .checked_add(2)
            .ok_or_else(|| Error::invalid_dictionary("segmentation FSA state offset overflow"))?;
        let table_end = checked_transition_table_end(table_start, transitions_num)?;
        validate_min_len(data, table_end, "segmentation FSA target transitions")?;

        let accepting = data[offset] & SEGRULES_ACCEPTING_FLAG != 0;
        let weak = data[offset] & SEGRULES_WEAK_FLAG != 0;
        let sink = transitions_num == 0;
        Ok(SegmentationState {
            offset,
            accepting,
            weak,
            shift_orth_from_previous: false,
            sink,
            failed: !accepting && sink,
        })
    }

    #[cfg_attr(not(debug_assertions), inline(always))]
    fn state_at_unchecked(data: &[u8], offset: usize) -> SegmentationState {
        let flags = byte_at_unchecked(data, offset);
        let transitions_num = byte_at_unchecked(data, offset + 1) as usize;
        let accepting = flags & SEGRULES_ACCEPTING_FLAG != 0;
        let weak = flags & SEGRULES_WEAK_FLAG != 0;
        let sink = transitions_num == 0;
        SegmentationState {
            offset,
            accepting,
            weak,
            shift_orth_from_previous: false,
            sink,
            failed: !accepting && sink,
        }
    }

    #[cfg_attr(not(debug_assertions), inline(always))]
    fn allows_transition(state: SegmentationState, at_end_of_word: bool) -> bool {
        if at_end_of_word {
            state.accepting
        } else {
            !state.sink
        }
    }
}

pub fn read_raw_interps_groups(payload: &[u8]) -> Result<Vec<RawInterpsGroup<'_>>> {
    let mut groups = Vec::new();
    for_each_raw_interps_group(payload, |_, group| {
        groups.push(group);
        Ok(())
    })?;
    Ok(groups)
}

fn for_each_raw_interps_group<'a, F>(payload: &'a [u8], mut visit: F) -> Result<()>
where
    F: FnMut(usize, RawInterpsGroup<'a>) -> Result<()>,
{
    let mut cursor = 0;
    let mut group_index = 0usize;
    while cursor < payload.len() {
        validate_min_len(payload, cursor + 3, "interpretations group header")?;
        let segment_type = payload[cursor];
        cursor += 1;
        let size = u16::from_be_bytes([payload[cursor], payload[cursor + 1]]) as usize;
        cursor += 2;
        let end = cursor
            .checked_add(size)
            .ok_or_else(|| Error::invalid_dictionary("interpretations group size overflow"))?;
        validate_min_len(payload, end, "interpretations group data")?;
        visit(
            group_index,
            RawInterpsGroup {
                segment_type,
                data: &payload[cursor..end],
            },
        )?;
        group_index += 1;
        cursor = end;
    }

    Ok(())
}

pub fn decode_analyzer_interpretations(
    group: RawInterpsGroup<'_>,
) -> Result<Vec<EncodedAnalyzerInterpretation>> {
    Ok(decode_binary_analyzer_interpretations(group)?
        .into_iter()
        .map(BinaryAnalyzerInterpretation::into_public)
        .collect())
}

fn decode_binary_analyzer_interpretations(
    group: RawInterpsGroup<'_>,
) -> Result<Vec<BinaryAnalyzerInterpretation>> {
    validate_min_len(group.data, 1, "analyzer interpretations compression byte")?;
    let compression_byte = group.data[0];
    let mut cursor = 1;

    if !orth_case_patterns_encoded_in_compression_byte(compression_byte) {
        let patterns_num = read_u8(group.data, &mut cursor, "orth case patterns count")?;
        for _ in 0..patterns_num {
            skip_case_pattern(group.data, &mut cursor)?;
        }
    }

    let mut result = Vec::with_capacity(((group.data.len() - cursor) / 6).max(1));
    while cursor < group.data.len() {
        let orth_case_pattern =
            read_binary_orth_case_pattern(group.data, &mut cursor, compression_byte)?;
        let form = read_binary_analyzer_form(group.data, &mut cursor, compression_byte)?;
        let tag_id = read_u16(group.data, &mut cursor, "analyzer tag id")? as i32;
        let name_id = read_u8(group.data, &mut cursor, "analyzer name id")? as i32;
        let labels_id = read_u16(group.data, &mut cursor, "analyzer labels id")? as i32;

        result.push(BinaryAnalyzerInterpretation {
            orth_case_pattern,
            form,
            tag_id,
            name_id,
            labels_id,
        });
    }

    Ok(result)
}

pub fn decode_analyzer_interps_groups(payload: &[u8]) -> Result<Vec<EncodedAnalyzerInterpsGroup>> {
    read_raw_interps_groups(payload)?
        .into_iter()
        .map(|group| {
            Ok(EncodedAnalyzerInterpsGroup {
                segment_type: group.segment_type,
                interpretations: decode_analyzer_interpretations(group)?,
            })
        })
        .collect()
}

pub fn decode_generator_interpretations(
    group: RawInterpsGroup<'_>,
) -> Result<Vec<EncodedGeneratorInterpretation>> {
    let mut cursor = 0;
    let mut result = Vec::with_capacity((group.data.len() / 8).max(1));

    while cursor < group.data.len() {
        let homonym_id = read_c_string_from_slice(group.data, &mut cursor, "homonym id")?;
        let prefix_to_add = read_c_string_from_slice(group.data, &mut cursor, "prefix to add")?;
        let suffix_to_cut = read_u8(group.data, &mut cursor, "generator suffix cut")?;
        let suffix_to_add = read_c_string_from_slice(group.data, &mut cursor, "suffix to add")?;
        let tag_id = read_u16(group.data, &mut cursor, "generator tag id")? as i32;
        let name_id = read_u8(group.data, &mut cursor, "generator name id")? as i32;
        let labels_id = read_u16(group.data, &mut cursor, "generator labels id")? as i32;

        result.push(EncodedGeneratorInterpretation {
            homonym_id,
            form: EncodedForm {
                prefix_to_cut: 0,
                suffix_to_cut,
                suffix_to_add,
                case_pattern: Vec::new(),
                prefix_to_add,
            },
            tag_id,
            name_id,
            labels_id,
        });
    }

    Ok(result)
}

pub fn decode_generator_interps_groups(
    payload: &[u8],
) -> Result<Vec<EncodedGeneratorInterpsGroup>> {
    read_raw_interps_groups(payload)?
        .into_iter()
        .map(|group| {
            Ok(EncodedGeneratorInterpsGroup {
                segment_type: group.segment_type,
                interpretations: decode_generator_interpretations(group)?,
            })
        })
        .collect()
}

#[derive(Debug, Clone, Copy)]
struct BinaryAnalyzerChunk<'a> {
    orth: &'a str,
    original_start: usize,
    original_end: usize,
    shift_orth: bool,
    segment_type: u8,
    /// Stable identity of the dictionary interpretations group this chunk came
    /// from: `(fsa_state_offset, group_index_within_payload)`. Mirrors C++
    /// `interpsGroupPtr` so identical edges reached through different paths
    /// dedup, while distinct groups that happen to decode identically (e.g. two
    /// `adv:pos` readings of "jednotonowo") are both kept.
    group_id: (usize, usize),
    /// Whether this segment's orthographic case matches the dictionary group's
    /// case patterns (C++ `checkInterpsGroupOrthCasePatterns`). A group matches
    /// when any of its interpretations' orth-case patterns accept the segment's
    /// orth. Drives strict-case rejection and conditional-case weak pruning.
    case_matches: bool,
    /// Index into the per-word decode cache. Keeping only an index makes path
    /// clones and graph arena copies plain scalar copies; decoded String/Vec
    /// payloads stay owned by the cache for the duration of processing one word.
    interpretations: usize,
}

#[derive(Debug, Clone)]
struct BinaryAnalyzerPath<'a> {
    chunks: Vec<BinaryAnalyzerChunk<'a>>,
    weak: bool,
}

#[derive(Debug)]
struct BinaryAnalyzerWordPaths<'a> {
    paths: Vec<BinaryAnalyzerPath<'a>>,
    decode_cache: AnalyzerGroupDecodeCache,
}

impl<'a> BinaryAnalyzerWordPaths<'a> {
    fn empty() -> Self {
        Self {
            paths: Vec::new(),
            decode_cache: AnalyzerGroupDecodeCache::default(),
        }
    }
}

#[derive(Debug, Clone, Default)]
struct SharedAnalyzerGroupDecodeCache {
    groups: Arc<Mutex<AnalyzerDecodeCacheMap>>,
}

impl SharedAnalyzerGroupDecodeCache {
    fn get_or_decode(
        &self,
        group_id: InterpsGroupId,
        raw_group: RawInterpsGroup<'_>,
    ) -> Result<Arc<[BinaryAnalyzerInterpretation]>> {
        if let Some(cached) = self
            .groups
            .lock()
            .map_err(|_| Error::invalid_dictionary("analyzer decode cache is poisoned"))?
            .get(&group_id)
            .cloned()
        {
            return Ok(cached);
        }

        let decoded: Arc<[BinaryAnalyzerInterpretation]> =
            decode_binary_analyzer_interpretations(raw_group)?.into();
        let mut groups = self
            .groups
            .lock()
            .map_err(|_| Error::invalid_dictionary("analyzer decode cache is poisoned"))?;
        if groups.len() >= ANALYZER_DECODE_CACHE_MAX_GROUPS && !groups.contains_key(&group_id) {
            groups.clear();
        }
        Ok(groups
            .entry(group_id)
            .or_insert_with(|| Arc::clone(&decoded))
            .clone())
    }
}

#[derive(Debug)]
struct AnalyzerGroupDecodeCache {
    shared: SharedAnalyzerGroupDecodeCache,
    groups: Vec<(InterpsGroupId, Arc<[BinaryAnalyzerInterpretation]>)>,
}

impl AnalyzerGroupDecodeCache {
    fn with_capacity(shared: SharedAnalyzerGroupDecodeCache, capacity: usize) -> Self {
        Self {
            shared,
            groups: Vec::with_capacity(capacity),
        }
    }

    fn get_or_decode(
        &mut self,
        group_id: InterpsGroupId,
        raw_group: RawInterpsGroup<'_>,
    ) -> Result<usize> {
        if let Some(index) = self
            .groups
            .iter()
            .position(|(cached_id, _)| *cached_id == group_id)
        {
            return Ok(index);
        }

        let interpretations = self.shared.get_or_decode(group_id, raw_group)?;
        self.groups.push((group_id, interpretations));
        Ok(self.groups.len() - 1)
    }

    fn interpretations(&self, index: usize) -> &[BinaryAnalyzerInterpretation] {
        &self.groups[index].1
    }
}

impl Default for AnalyzerGroupDecodeCache {
    fn default() -> Self {
        Self::with_capacity(SharedAnalyzerGroupDecodeCache::default(), 0)
    }
}

#[derive(Debug, Clone, Default)]
struct SharedGeneratorGroupDecodeCache {
    groups: Arc<Mutex<GeneratorDecodeCacheMap>>,
}

impl SharedGeneratorGroupDecodeCache {
    fn get_or_decode(
        &self,
        group_id: InterpsGroupId,
        raw_group: RawInterpsGroup<'_>,
    ) -> Result<Arc<[EncodedGeneratorInterpretation]>> {
        if let Some(cached) = self
            .groups
            .lock()
            .map_err(|_| Error::invalid_dictionary("generator decode cache is poisoned"))?
            .get(&group_id)
            .cloned()
        {
            return Ok(cached);
        }

        let decoded: Arc<[EncodedGeneratorInterpretation]> =
            decode_generator_interpretations(raw_group)?.into();
        let mut groups = self
            .groups
            .lock()
            .map_err(|_| Error::invalid_dictionary("generator decode cache is poisoned"))?;
        if groups.len() >= GENERATOR_DECODE_CACHE_MAX_GROUPS && !groups.contains_key(&group_id) {
            groups.clear();
        }
        Ok(groups
            .entry(group_id)
            .or_insert_with(|| Arc::clone(&decoded))
            .clone())
    }
}

#[derive(Debug)]
struct GeneratorGroupDecodeCache {
    shared: SharedGeneratorGroupDecodeCache,
    groups: Vec<(InterpsGroupId, Arc<[EncodedGeneratorInterpretation]>)>,
}

impl GeneratorGroupDecodeCache {
    fn with_capacity(shared: SharedGeneratorGroupDecodeCache, capacity: usize) -> Self {
        Self {
            shared,
            groups: Vec::with_capacity(capacity),
        }
    }

    fn get_or_decode(
        &mut self,
        group_id: InterpsGroupId,
        raw_group: RawInterpsGroup<'_>,
    ) -> Result<usize> {
        if let Some(index) = self
            .groups
            .iter()
            .position(|(cached_id, _)| *cached_id == group_id)
        {
            return Ok(index);
        }

        let interpretations = self.shared.get_or_decode(group_id, raw_group)?;
        self.groups.push((group_id, interpretations));
        Ok(self.groups.len() - 1)
    }

    fn interpretations(&self, index: usize) -> &[EncodedGeneratorInterpretation] {
        &self.groups[index].1
    }
}

#[derive(Debug, Clone, Copy)]
struct BinaryGeneratorChunk<'a> {
    lemma: &'a str,
    shift_orth: bool,
    interpretations: usize,
}

#[derive(Debug, Clone)]
struct BinaryGeneratorPath<'a> {
    chunks: Vec<BinaryGeneratorChunk<'a>>,
    weak: bool,
}

fn collect_segmented_analyzer_paths<'a>(
    fsa: BinaryFsa<'_>,
    rules_fsa: &SegmentationRulesFsa<'_>,
    normalized: &NormalizedInput<'a>,
    original: &'a str,
    position: usize,
    segmentation_state: SegmentationState,
    current_path: &mut Vec<BinaryAnalyzerChunk<'a>>,
    paths: &mut Vec<BinaryAnalyzerPath<'a>>,
    decode_cache: &mut AnalyzerGroupDecodeCache,
) -> Result<()> {
    let normalized_text = normalized.as_str();
    if position >= normalized_text.len() {
        return Ok(());
    }

    fsa.for_each_prefix_match_loaded(&normalized_text.as_bytes()[position..], |prefix_match| {
        let end = position
            .checked_add(prefix_match.input_end)
            .ok_or_else(|| Error::invalid_dictionary("normalized prefix offset overflow"))?;
        let Some((chunk_orth, original_start, original_end)) =
            normalized.original_span(original, position, end)
        else {
            return Ok(());
        };
        let at_end = end == normalized_text.len();

        for_each_raw_interps_group(prefix_match.value, |group_index, raw_group| {
            // Check the segmentation rules using only the (cheap) segment type and
            // decode the full interpretations lazily — groups rejected by segrules
            // are never decoded, which avoids a large amount of wasted
            // `String`/`Vec` allocation on rich dictionaries.
            let Some(new_state) = rules_fsa.proceed_to_next_unchecked(
                raw_group.segment_type,
                segmentation_state,
                at_end,
            ) else {
                return Ok(());
            };

            let segment_type = raw_group.segment_type;
            let group_id = (prefix_match.state_offset, group_index);
            let interpretations = decode_cache.get_or_decode(group_id, raw_group)?;
            let case_matches = decode_cache
                .interpretations(interpretations)
                .iter()
                .any(|interp| interp.matches_orth_case(chunk_orth));
            current_path.push(BinaryAnalyzerChunk {
                orth: chunk_orth,
                original_start,
                original_end,
                shift_orth: new_state.shift_orth_from_previous,
                segment_type,
                group_id,
                case_matches,
                interpretations,
            });

            if at_end {
                if new_state.accepting {
                    paths.push(BinaryAnalyzerPath {
                        chunks: current_path.clone(),
                        weak: new_state.weak,
                    });
                }
            } else if !new_state.sink {
                collect_segmented_analyzer_paths(
                    fsa,
                    rules_fsa,
                    normalized,
                    original,
                    end,
                    new_state,
                    current_path,
                    paths,
                    decode_cache,
                )?;
            }

            current_path.pop();
            Ok(())
        })?;
        Ok(())
    })?;

    Ok(())
}

fn collect_segmented_generator_paths<'a>(
    fsa: BinaryFsa<'_>,
    rules_fsa: &SegmentationRulesFsa<'_>,
    lemma: &'a str,
    position: usize,
    segmentation_state: SegmentationState,
    current_path: &mut Vec<BinaryGeneratorChunk<'a>>,
    paths: &mut Vec<BinaryGeneratorPath<'a>>,
    decode_cache: &mut GeneratorGroupDecodeCache,
) -> Result<()> {
    if position >= lemma.len() {
        return Ok(());
    }

    fsa.for_each_prefix_match_loaded(&lemma.as_bytes()[position..], |prefix_match| {
        let end = position
            .checked_add(prefix_match.input_end)
            .ok_or_else(|| Error::invalid_dictionary("generator prefix offset overflow"))?;
        let Some(chunk_lemma) = lemma.get(position..end) else {
            return Ok(());
        };
        let at_end = end == lemma.len();

        for_each_raw_interps_group(prefix_match.value, |group_index, raw_group| {
            let segment_type = raw_group.segment_type;
            let Some(new_state) =
                rules_fsa.proceed_to_next_unchecked(segment_type, segmentation_state, at_end)
            else {
                return Ok(());
            };
            let group_id = (prefix_match.state_offset, group_index);
            let interpretations = decode_cache.get_or_decode(group_id, raw_group)?;

            current_path.push(BinaryGeneratorChunk {
                lemma: chunk_lemma,
                shift_orth: new_state.shift_orth_from_previous,
                interpretations,
            });

            if at_end {
                if new_state.accepting {
                    paths.push(BinaryGeneratorPath {
                        chunks: current_path.clone(),
                        weak: new_state.weak,
                    });
                }
            } else if !new_state.sink {
                collect_segmented_generator_paths(
                    fsa,
                    rules_fsa,
                    lemma,
                    end,
                    new_state,
                    current_path,
                    paths,
                    decode_cache,
                )?;
            }

            current_path.pop();
            Ok(())
        })?;
        Ok(())
    })?;

    Ok(())
}

/// A single `ign` interpretation spanning `[start_node, start_node + 1]`.
fn ignotium(word: &str, start_node: i32) -> MorphInterpretation {
    MorphInterpretation::create_ign(start_node, start_node + 1, word, word)
}

fn paths_to_morph_interpretations<'a>(
    mut paths: Vec<BinaryAnalyzerPath<'a>>,
    decode_cache: &AnalyzerGroupDecodeCache,
    start_node: i32,
    case_handling: CaseHandling,
) -> Result<Option<(Vec<MorphInterpretation>, i32)>> {
    if paths.is_empty() {
        return Ok(None);
    }

    // Orthographic-case handling, mirroring C++ `processInterpsGroup`:
    //  * STRICTLY_CASE_SENSITIVE: a segment whose case does not match the
    //    dictionary group is rejected outright, so any path containing one is
    //    dropped before it can enter the graph.
    //  * CONDITIONALLY_CASE_SENSITIVE: such a segment is accepted but makes the
    //    whole path weak (`InflexionGraph::add_path` keeps strong paths over
    //    weak ones), so lowercase "rogalińską" keeps `rogaliński` (case match)
    //    and drops the capitalized proper-noun reading.
    //  * IGNORE_CASE: case is irrelevant.
    if case_handling == CaseHandling::StrictlyCaseSensitive {
        paths.retain(|path| path.chunks.iter().all(|chunk| chunk.case_matches));
        if paths.is_empty() {
            return Ok(None);
        }
    }
    if paths.len() == 1 {
        return single_path_to_morph_interpretations(
            &paths[0],
            decode_cache,
            start_node,
            case_handling,
        );
    }
    if paths.iter().all(path_is_single_graph_edge) {
        return single_edge_paths_to_morph_interpretations(
            &paths,
            decode_cache,
            start_node,
            case_handling,
        );
    }

    let mut graph = InflexionGraph::default();
    for path in &paths {
        let weak = path.weak
            || (case_handling == CaseHandling::ConditionallyCaseSensitive
                && path.chunks.iter().any(|chunk| !chunk.case_matches));
        graph.add_path(path, weak);
    }
    if graph.is_empty() {
        return Ok(None);
    }

    let node_count = graph.finish();
    let mut result = Vec::new();
    for node in 0..graph.node_len() {
        let src = start_node + node as i32;
        for edge_index in 0..graph.edges_at(node) {
            let (group, next_node) = graph.edge(node, edge_index);
            let target = start_node + next_node as i32;
            if group.len() > 1 {
                push_shifted_chunk_interpretations(
                    group,
                    decode_cache,
                    src,
                    target,
                    case_handling,
                    &mut result,
                )?;
            } else {
                push_plain_chunk_interpretations(
                    &group[0],
                    decode_cache,
                    src,
                    target,
                    case_handling,
                    &mut result,
                )?;
            }
        }
    }

    if result.is_empty() {
        Ok(None)
    } else {
        Ok(Some((result, node_count as i32)))
    }
}

fn path_is_single_graph_edge(path: &BinaryAnalyzerPath<'_>) -> bool {
    !path.chunks.is_empty()
        && path
            .chunks
            .iter()
            .take(path.chunks.len().saturating_sub(1))
            .all(|chunk| chunk.shift_orth)
}

fn path_is_effectively_weak(path: &BinaryAnalyzerPath<'_>, case_handling: CaseHandling) -> bool {
    path.weak
        || (case_handling == CaseHandling::ConditionallyCaseSensitive
            && path.chunks.iter().any(|chunk| !chunk.case_matches))
}

fn single_edge_paths_to_morph_interpretations(
    paths: &[BinaryAnalyzerPath<'_>],
    decode_cache: &AnalyzerGroupDecodeCache,
    start_node: i32,
    case_handling: CaseHandling,
) -> Result<Option<(Vec<MorphInterpretation>, i32)>> {
    let has_strong = paths
        .iter()
        .any(|path| !path_is_effectively_weak(path, case_handling));
    let capacity = paths
        .iter()
        .filter(|path| !(has_strong && path_is_effectively_weak(path, case_handling)))
        .flat_map(|path| path.chunks.iter())
        .map(|chunk| decode_cache.interpretations(chunk.interpretations).len())
        .sum();
    let mut result = Vec::with_capacity(capacity);
    for path in paths {
        if has_strong && path_is_effectively_weak(path, case_handling) {
            continue;
        }
        let target = start_node + 1;
        if path.chunks.len() > 1 {
            push_shifted_chunk_interpretations(
                &path.chunks,
                decode_cache,
                start_node,
                target,
                case_handling,
                &mut result,
            )?;
        } else {
            push_plain_chunk_interpretations(
                &path.chunks[0],
                decode_cache,
                start_node,
                target,
                case_handling,
                &mut result,
            )?;
        }
    }

    if result.is_empty() {
        Ok(None)
    } else {
        Ok(Some((result, 1)))
    }
}

fn single_path_to_morph_interpretations(
    path: &BinaryAnalyzerPath<'_>,
    decode_cache: &AnalyzerGroupDecodeCache,
    start_node: i32,
    case_handling: CaseHandling,
) -> Result<Option<(Vec<MorphInterpretation>, i32)>> {
    let capacity = path
        .chunks
        .iter()
        .map(|chunk| decode_cache.interpretations(chunk.interpretations).len())
        .sum();
    let mut result = Vec::with_capacity(capacity);
    let mut index = 0usize;
    let mut node = 0i32;
    while index < path.chunks.len() {
        let mut shifted_end = index;
        while shifted_end + 1 < path.chunks.len() && path.chunks[shifted_end].shift_orth {
            shifted_end += 1;
        }

        let src = start_node + node;
        let target = src + 1;
        if shifted_end > index {
            push_shifted_chunk_interpretations(
                &path.chunks[index..=shifted_end],
                decode_cache,
                src,
                target,
                case_handling,
                &mut result,
            )?;
        } else {
            push_plain_chunk_interpretations(
                &path.chunks[index],
                decode_cache,
                src,
                target,
                case_handling,
                &mut result,
            )?;
        }
        node += 1;
        index = shifted_end + 1;
    }

    if result.is_empty() {
        Ok(None)
    } else {
        Ok(Some((result, node)))
    }
}

/// Sentinel for an edge that leaves the graph (C++ `UINT_MAX`).
const GRAPH_END: usize = usize::MAX;

/// One edge of the inflexion graph. `group` indexes a chunk group (a shifted
/// prefix run plus its main chunk) in the owning graph's arena.
#[derive(Debug, Clone, Copy)]
struct GraphEdge {
    group: usize,
    text_start: usize,
    main_start: usize,
    text_end: usize,
    segment_type: u8,
    group_id: (usize, usize),
    next_node: usize,
}

/// Faithful port of C++ `InflexionGraph`: indexes nodes, drops weak paths when a
/// strong one exists, and minimizes the node count so that alternative
/// segmentations of one token collapse onto the same node span — reproducing the
/// reference edge ordering and node numbering exactly.
#[derive(Debug)]
struct InflexionGraph<'a> {
    graph: Vec<Vec<GraphEdge>>,
    node2start: Vec<usize>,
    arena: Vec<Vec<BinaryAnalyzerChunk<'a>>>,
    only_weak_paths: bool,
}

impl<'a> Default for InflexionGraph<'a> {
    fn default() -> Self {
        // `only_weak_paths` starts true so the first strong path clears any
        // weak-only graph accumulated before it (C++ constructor invariant).
        Self {
            graph: Vec::new(),
            node2start: Vec::new(),
            arena: Vec::new(),
            only_weak_paths: true,
        }
    }
}

impl<'a> InflexionGraph<'a> {
    fn is_empty(&self) -> bool {
        self.graph.is_empty()
    }

    fn node_len(&self) -> usize {
        self.graph.len()
    }

    fn edges_at(&self, node: usize) -> usize {
        self.graph[node].len()
    }

    fn edge(&self, node: usize, index: usize) -> (&[BinaryAnalyzerChunk<'a>], usize) {
        let edge = &self.graph[node][index];
        (&self.arena[edge.group], edge.next_node)
    }

    /// Splits a path into its non-shifted edges (each a shifted prefix run plus
    /// its main chunk) and adds them, replicating C++ `addPath`.
    fn add_path(&mut self, path: &BinaryAnalyzerPath<'a>, weak: bool) {
        if weak && !self.is_empty() && !self.only_weak_paths {
            return;
        } else if self.only_weak_paths && !weak {
            self.graph.clear();
            self.node2start.clear();
            self.arena.clear();
            self.only_weak_paths = false;
        }

        let edges_num = analyzer_path_edge_count(&path.chunks);
        let mut position = 0usize;
        let mut index = 0;
        while index < path.chunks.len() {
            let mut shifted_end = index;
            while shifted_end + 1 < path.chunks.len() && path.chunks[shifted_end].shift_orth {
                shifted_end += 1;
            }
            let group = path.chunks[index..=shifted_end].to_vec();
            let main = &group[group.len() - 1];
            let arena_index = self.arena.len();
            let text_start = group[0].original_start;
            let make = |next_node: usize| GraphEdge {
                group: arena_index,
                text_start,
                main_start: main.original_start,
                text_end: main.original_end,
                segment_type: main.segment_type,
                group_id: main.group_id,
                next_node,
            };
            let is_front = position == 0;
            let is_back = position + 1 == edges_num;

            if is_front && is_back {
                let edge = make(GRAPH_END);
                self.arena.push(group);
                self.add_start_edge(edge);
            } else if is_front {
                let next = if self.graph.is_empty() {
                    1
                } else {
                    self.graph.len()
                };
                let edge = make(next);
                self.arena.push(group);
                self.add_start_edge(edge);
            } else if is_back {
                let start_node = self.graph.len();
                let edge = make(GRAPH_END);
                self.arena.push(group);
                self.add_middle_edge(start_node, edge);
            } else {
                let start_node = self.graph.len();
                let edge = make(start_node + 1);
                self.arena.push(group);
                self.add_middle_edge(start_node, edge);
            }
            position += 1;
            index = shifted_end + 1;
        }
    }

    fn add_start_edge(&mut self, edge: GraphEdge) {
        if self.graph.is_empty() {
            self.graph.push(Vec::new());
            self.node2start.push(edge.text_start);
        }
        self.graph[0].push(edge);
    }

    fn add_middle_edge(&mut self, start_node: usize, edge: GraphEdge) {
        if start_node == self.graph.len() {
            self.graph.push(Vec::new());
            self.node2start.push(edge.text_start);
        }
        self.graph[start_node].push(edge);
    }

    /// Runs minimization, topological renumbering and last-node repair, returning
    /// the node count (the next free node number, C++ `graph.size()`).
    fn finish(&mut self) -> usize {
        self.minimize();
        if self.graph.len() > 2 {
            self.sort_nodes_topologically();
        }
        self.repair_last_node_numbers();
        self.graph.len()
    }

    fn minimize(&mut self) {
        if self.graph.len() > 2 {
            while self.try_to_merge_two_nodes() {}
        }
    }

    fn try_to_merge_two_nodes(&mut self) -> bool {
        for node1 in 0..self.graph.len() {
            for node2 in ((node1 + 1)..self.graph.len()).rev() {
                if self.can_merge_nodes(node1, node2) {
                    self.do_merge_nodes(node1, node2);
                    return true;
                }
            }
        }
        false
    }

    fn can_merge_nodes(&self, node1: usize, node2: usize) -> bool {
        self.node2start[node1] == self.node2start[node2]
            && self.possible_paths(node1) == self.possible_paths(node2)
    }

    fn possible_paths(&self, node: usize) -> BTreeSet<BTreeSet<(usize, u8)>> {
        if node == GRAPH_END || node + 1 == self.graph.len() {
            return BTreeSet::new();
        }
        let mut res = BTreeSet::new();
        for edge in &self.graph[node] {
            let elem = (edge.text_start, edge.segment_type);
            if edge.next_node != self.graph.len() {
                for mut path in self.possible_paths(edge.next_node) {
                    path.insert(elem);
                    res.insert(path);
                }
            }
        }
        res
    }

    fn do_merge_nodes(&mut self, node1: usize, node2: usize) {
        debug_assert!(node1 < node2);
        let incoming = self.graph[node2].clone();
        for edge in incoming {
            if !edge_in(&self.graph[node1], &edge) {
                self.graph[node1].push(edge);
            }
        }
        self.redirect_edges(node2, node1);
        self.do_remove_node(node2);
    }

    fn redirect_edges(&mut self, from_node: usize, to_node: usize) {
        for node in 0..from_node {
            let mut i = 0;
            while i < self.graph[node].len() {
                if self.graph[node][i].next_node == from_node {
                    let mut redirected = self.graph[node][i].clone();
                    redirected.next_node = to_node;
                    if edge_in(&self.graph[node], &redirected) {
                        self.graph[node].remove(i);
                    } else {
                        self.graph[node][i].next_node = to_node;
                        i += 1;
                    }
                } else {
                    i += 1;
                }
            }
        }
    }

    fn do_remove_node(&mut self, node: usize) {
        for i in (node + 1)..self.graph.len() {
            self.redirect_edges(i, i - 1);
            self.graph[i - 1] = self.graph[i].clone();
            self.node2start[i - 1] = self.node2start[i];
        }
        self.graph.pop();
        self.node2start.pop();
    }

    fn repair_last_node_numbers(&mut self) {
        let size = self.graph.len();
        for edges in &mut self.graph {
            for edge in edges {
                if edge.next_node == GRAPH_END {
                    edge.next_node = size;
                }
            }
        }
    }

    fn sort_nodes_topologically(&mut self) {
        let n = self.graph.len();
        let mut sorted: Vec<usize> = (0..n).collect();
        sorted.sort_by(|&i, &j| self.node2start[i].cmp(&self.node2start[j]));
        let mut old_to_new = vec![0usize; n];
        for (new_node, &old_node) in sorted.iter().enumerate() {
            old_to_new[old_node] = new_node;
        }
        for edges in &mut self.graph {
            for edge in edges {
                if edge.next_node < n {
                    edge.next_node = old_to_new[edge.next_node];
                }
            }
        }
        let graph_copy = self.graph.clone();
        let node2start_copy = self.node2start.clone();
        for old_node in 0..n {
            let new_node = old_to_new[old_node];
            self.graph[new_node] = graph_copy[old_node].clone();
            self.node2start[new_node] = node2start_copy[old_node];
        }
    }
}

fn analyzer_path_edge_count(chunks: &[BinaryAnalyzerChunk<'_>]) -> usize {
    let mut count = 0usize;
    let mut index = 0usize;
    while index < chunks.len() {
        let mut shifted_end = index;
        while shifted_end + 1 < chunks.len() && chunks[shifted_end].shift_orth {
            shifted_end += 1;
        }
        count += 1;
        index = shifted_end + 1;
    }
    count
}

/// C++ `containsEqualEdge`: edges are equal by node span, segment identity and
/// target — never by decoded text.
fn edge_in(edges: &[GraphEdge], edge: &GraphEdge) -> bool {
    edges.iter().any(|other| {
        other.text_start == edge.text_start
            && other.main_start == edge.main_start
            && other.text_end == edge.text_end
            && other.segment_type == edge.segment_type
            && other.next_node == edge.next_node
            && other.group_id == edge.group_id
    })
}

fn generator_paths_to_morph_interpretations(
    mut paths: Vec<BinaryGeneratorPath<'_>>,
    decode_cache: &GeneratorGroupDecodeCache,
    required_homonym_id: Option<&str>,
) -> Result<Vec<MorphInterpretation>> {
    if paths.iter().any(|path| !path.weak) {
        paths.retain(|path| !path.weak);
    }

    let capacity = paths
        .iter()
        .flat_map(|path| path.chunks.iter())
        .map(|chunk| decode_cache.interpretations(chunk.interpretations).len())
        .sum();
    let mut result = Vec::with_capacity(capacity);
    for path in paths {
        let mut index = 0;
        while index < path.chunks.len() {
            let mut shifted_end = index;
            while shifted_end + 1 < path.chunks.len() && path.chunks[shifted_end].shift_orth {
                shifted_end += 1;
            }

            if shifted_end > index {
                push_shifted_generator_interpretations(
                    &path.chunks[index..=shifted_end],
                    decode_cache,
                    &mut result,
                    required_homonym_id,
                )?;
            } else {
                push_plain_generator_interpretations(
                    &path.chunks[index],
                    decode_cache,
                    &mut result,
                    required_homonym_id,
                )?;
            }

            index = shifted_end + 1;
        }
    }
    Ok(result)
}

fn push_plain_generator_interpretations(
    chunk: &BinaryGeneratorChunk<'_>,
    decode_cache: &GeneratorGroupDecodeCache,
    result: &mut Vec<MorphInterpretation>,
    required_homonym_id: Option<&str>,
) -> Result<()> {
    for interp in decode_cache.interpretations(chunk.interpretations) {
        if !generator_homonym_matches(interp, required_homonym_id) {
            continue;
        }
        result.push(interp.to_morph_interpretation(&chunk.lemma, 0, 0)?);
    }
    Ok(())
}

fn push_shifted_generator_interpretations(
    chunks: &[BinaryGeneratorChunk<'_>],
    decode_cache: &GeneratorGroupDecodeCache,
    result: &mut Vec<MorphInterpretation>,
    required_homonym_id: Option<&str>,
) -> Result<()> {
    let Some((current, prefixes)) = chunks.split_last() else {
        return Ok(());
    };
    let lemma = chunks.iter().map(|chunk| chunk.lemma).collect::<String>();
    let orth_prefix = prefixes.iter().map(|chunk| chunk.lemma).collect::<String>();

    for interp in decode_cache.interpretations(current.interpretations) {
        if !generator_homonym_matches(interp, required_homonym_id) {
            continue;
        }
        let mut morph = interp.to_morph_interpretation(&current.lemma, 0, 0)?;
        morph.orth = format!("{orth_prefix}{}", morph.orth);
        morph.lemma = if interp.homonym_id.is_empty() {
            lemma.clone()
        } else {
            format!("{lemma}:{}", interp.homonym_id)
        };
        result.push(morph);
    }
    Ok(())
}

fn split_generator_lemma(lemma: &str) -> (&str, Option<&str>) {
    lemma
        .split_once(':')
        .map(|(base, homonym_id)| (base, Some(homonym_id)))
        .unwrap_or((lemma, None))
}

fn generator_homonym_matches(
    interp: &EncodedGeneratorInterpretation,
    required_homonym_id: Option<&str>,
) -> bool {
    required_homonym_id
        .map(|required| interp.homonym_id == required)
        .unwrap_or(true)
}

fn push_plain_chunk_interpretations(
    chunk: &BinaryAnalyzerChunk,
    decode_cache: &AnalyzerGroupDecodeCache,
    start_node: i32,
    end_node: i32,
    case_handling: CaseHandling,
    result: &mut Vec<MorphInterpretation>,
) -> Result<()> {
    let orth_context = AnalyzerOrthContext::new(chunk.orth);
    for_each_case_compatible_interpretation(
        &chunk.orth,
        decode_cache.interpretations(chunk.interpretations),
        case_handling,
        |interp| {
            result.push(interp.to_morph_interpretation_in_context(
                &orth_context,
                start_node,
                end_node,
            )?);
            Ok(())
        },
    )?;
    Ok(())
}

fn push_shifted_chunk_interpretations(
    chunks: &[BinaryAnalyzerChunk],
    decode_cache: &AnalyzerGroupDecodeCache,
    start_node: i32,
    end_node: i32,
    case_handling: CaseHandling,
    result: &mut Vec<MorphInterpretation>,
) -> Result<()> {
    let Some((current, prefixes)) = chunks.split_last() else {
        return Ok(());
    };
    let orth = chunks.iter().map(|chunk| chunk.orth).collect::<String>();
    let mut lemma_prefix = String::new();
    for prefix in prefixes {
        let Some(prefix_interp) = first_case_compatible_interpretation(
            &prefix.orth,
            decode_cache.interpretations(prefix.interpretations),
            case_handling,
        ) else {
            return Ok(());
        };
        lemma_prefix.push_str(&decode_analyzer_prefix_lemma_for_form(
            &prefix.orth,
            &prefix_interp.form,
        )?);
    }

    let current_codepoints = current.orth.chars().count();
    let orth_context = AnalyzerOrthContext::new(&orth);
    let prefix_codepoints = orth_context.original_codepoints_len - current_codepoints;
    for_each_case_compatible_interpretation(
        &current.orth,
        decode_cache.interpretations(current.interpretations),
        case_handling,
        |interp| {
            let mut form = interp.form.clone();
            if !interp.orth_case_pattern.is_empty() && !form.case_pattern.is_empty() {
                form.case_pattern = form.case_pattern.shifted_by_lower_prefix(prefix_codepoints);
            }
            let mut lemma = lemma_prefix.clone();
            lemma.push_str(&decode_analyzer_lemma_with_prefix_context_len(
                &orth,
                current_codepoints,
                orth_context.lowercase_codepoints_len,
                &form,
            )?);
            result.push(MorphInterpretation {
                start_node,
                end_node,
                orth: orth.clone(),
                lemma,
                tag_id: interp.tag_id,
                name_id: interp.name_id,
                labels_id: interp.labels_id,
            });
            Ok(())
        },
    )?;
    Ok(())
}

fn for_each_case_compatible_interpretation<I, F>(
    orth: &str,
    interpretations: &[I],
    case_handling: CaseHandling,
    mut visit: F,
) -> Result<()>
where
    I: AnalyzerInterpretationView,
    F: FnMut(&I) -> Result<()>,
{
    if case_handling == CaseHandling::IgnoreCase {
        for interp in interpretations {
            visit(interp)?;
        }
        return Ok(());
    }

    let mut strict_seen = false;
    for interp in interpretations {
        if interp.matches_orth_case(orth) {
            strict_seen = true;
            visit(interp)?;
        }
    }
    if !strict_seen && case_handling == CaseHandling::ConditionallyCaseSensitive {
        for interp in interpretations {
            visit(interp)?;
        }
    }
    Ok(())
}

fn first_case_compatible_interpretation<'a, I>(
    orth: &str,
    interpretations: &'a [I],
    case_handling: CaseHandling,
) -> Option<&'a I>
where
    I: AnalyzerInterpretationView,
{
    if case_handling == CaseHandling::IgnoreCase {
        return interpretations.first();
    }

    interpretations
        .iter()
        .find(|interp| interp.matches_orth_case(orth))
        .or_else(|| {
            (case_handling == CaseHandling::ConditionallyCaseSensitive)
                .then(|| interpretations.first())
                .flatten()
        })
}

enum NormalizedInput<'a> {
    Borrowed(&'a str),
    OwnedIdentity {
        normalized: String,
    },
    Owned {
        normalized: String,
        boundaries: Vec<(usize, usize)>,
    },
}

impl<'a> NormalizedInput<'a> {
    fn as_str(&self) -> &str {
        match self {
            Self::Borrowed(text) => text,
            Self::OwnedIdentity { normalized } => normalized,
            Self::Owned { normalized, .. } => normalized,
        }
    }

    fn path_capacity_hint(&self) -> usize {
        self.as_str().len().clamp(1, 8)
    }

    fn original_span<'b>(
        &self,
        original: &'b str,
        start: usize,
        end: usize,
    ) -> Option<(&'b str, usize, usize)> {
        match self {
            Self::Borrowed(text) => {
                if text.is_char_boundary(start) && text.is_char_boundary(end) {
                    original.get(start..end).map(|slice| (slice, start, end))
                } else {
                    None
                }
            }
            Self::OwnedIdentity { normalized } => {
                if normalized.is_char_boundary(start) && normalized.is_char_boundary(end) {
                    original.get(start..end).map(|slice| (slice, start, end))
                } else {
                    None
                }
            }
            Self::Owned { boundaries, .. } => {
                original_span_for_normalized_range(original, boundaries, start, end)
            }
        }
    }
}

/// Lowercases `text` for FSA lookup while recording, for each normalized byte
/// offset, the matching original byte offset. If lowercasing leaves the input
/// byte-identical, returns a borrowed identity mapping and avoids per-word
/// `String`/`Vec` allocation.
fn lowercase_with_original_boundaries(text: &str) -> NormalizedInput<'_> {
    if text.is_ascii() {
        if text.bytes().all(|byte| !byte.is_ascii_uppercase()) {
            return NormalizedInput::Borrowed(text);
        }
        return NormalizedInput::OwnedIdentity {
            normalized: text.to_ascii_lowercase(),
        };
    }

    if text.chars().all(char_lowercases_to_self) {
        return NormalizedInput::Borrowed(text);
    }

    let mut normalized = String::new();
    let mut boundaries = Vec::with_capacity(text.len() + 1);
    boundaries.push((0usize, 0usize));

    for (original_start, ch) in text.char_indices() {
        // Morfeusz lowercases one codepoint to exactly one codepoint via its own
        // table (NOT Unicode multi-char folding) — the dictionary FSA keys are
        // built the same way, so e.g. 'İ' must become 'i', not "i\u{0307}".
        normalized.push(crate::case_tables::to_lower_char(ch));
        boundaries.push((normalized.len(), original_start + ch.len_utf8()));
    }

    NormalizedInput::Owned {
        normalized,
        boundaries,
    }
}

fn char_lowercases_to_self(ch: char) -> bool {
    crate::case_tables::to_lower_char(ch) == ch
}

/// Looks up the original byte offset for a normalized byte offset in the sorted
/// boundary table produced by [`lowercase_with_original_boundaries`].
fn boundary_original(boundaries: &[(usize, usize)], normalized_offset: usize) -> Option<usize> {
    boundaries
        .binary_search_by_key(&normalized_offset, |&(norm, _)| norm)
        .ok()
        .map(|index| boundaries[index].1)
}

fn original_span_for_normalized_range<'a>(
    original: &'a str,
    original_boundaries: &[(usize, usize)],
    start: usize,
    end: usize,
) -> Option<(&'a str, usize, usize)> {
    let original_start = boundary_original(original_boundaries, start)?;
    let original_end = boundary_original(original_boundaries, end)?;
    original
        .get(original_start..original_end)
        .map(|slice| (slice, original_start, original_end))
}

fn case_pattern_matches_orth(orth: &str, case_pattern: &[bool]) -> bool {
    if case_pattern.is_empty() {
        return true;
    }

    let mut chars = orth.chars();
    for must_be_uppercase in case_pattern.iter().copied() {
        let Some(ch) = chars.next() else {
            return false;
        };
        if must_be_uppercase && char_is_lowercase_equivalent(ch) {
            return false;
        }
    }
    true
}

fn char_is_lowercase_equivalent(ch: char) -> bool {
    // Matches C++ case-pattern checking: a codepoint counts as lowercase iff the
    // Morfeusz lowercase table maps it to itself.
    crate::case_tables::to_lower_char(ch) == ch
}

fn parse_segmentation_metadata(data: &[u8]) -> Result<SegmentationMetadata> {
    let mut cursor = 0;
    let separators_count = read_u16(data, &mut cursor, "separators count")? as usize;
    let mut separators = Vec::with_capacity(separators_count);
    for _ in 0..separators_count {
        separators.push(read_u32(data, &mut cursor, "separator codepoint")?);
    }

    let variants_count = read_u8(data, &mut cursor, "segmentation FSA variants count")? as usize;
    let mut fsa_variants = Vec::with_capacity(variants_count);
    for _ in 0..variants_count {
        let options = read_options_map(data, &mut cursor)?;
        let fsa_size = read_u32(data, &mut cursor, "segmentation FSA size")? as usize;
        let end = cursor
            .checked_add(fsa_size)
            .ok_or_else(|| Error::invalid_dictionary("segmentation FSA size overflow"))?;
        validate_min_len(data, end, "segmentation FSA data")?;
        SegmentationRulesFsa::new(&data[cursor..end])?;
        fsa_variants.push(SegmentationFsaVariant {
            options,
            fsa: data[cursor..end].to_vec(),
        });
        cursor = end;
    }

    let default_options = read_options_map(data, &mut cursor)?;
    if cursor != data.len() {
        return Err(Error::invalid_dictionary(format!(
            "trailing segmentation metadata bytes: {}",
            data.len() - cursor
        )));
    }

    Ok(SegmentationMetadata {
        separators,
        fsa_variants,
        default_options,
    })
}

fn read_options_map(data: &[u8], cursor: &mut usize) -> Result<BTreeMap<String, String>> {
    let options_count = read_u8(data, cursor, "segmentation options count")? as usize;
    let mut options = BTreeMap::new();
    for _ in 0..options_count {
        let key = read_c_string_from_slice(data, cursor, "segmentation option key")?;
        let value = read_c_string_from_slice(data, cursor, "segmentation option value")?;
        options.insert(key, value);
    }
    Ok(options)
}

fn checked_transition_table_end(cursor: usize, transitions_num: usize) -> Result<usize> {
    transitions_num
        .checked_mul(SEGRULES_TRANSITION_SIZE)
        .and_then(|size| cursor.checked_add(size))
        .ok_or_else(|| Error::invalid_dictionary("segmentation FSA transition table overflow"))
}

fn validate_reachable_segmentation_states(data: &[u8]) -> Result<()> {
    let mut seen = BTreeSet::new();
    let mut stack = vec![0usize];

    while let Some(offset) = stack.pop() {
        if !seen.insert(offset) {
            continue;
        }

        validate_min_len(data, offset + 2, "segmentation FSA state")?;
        let transitions_num = data[offset + 1] as usize;
        let table_start = offset
            .checked_add(2)
            .ok_or_else(|| Error::invalid_dictionary("segmentation FSA state offset overflow"))?;
        let table_end = checked_transition_table_end(table_start, transitions_num)?;
        validate_min_len(data, table_end, "segmentation FSA transitions")?;

        let mut cursor = table_start;
        for _ in 0..transitions_num {
            let target_offset = u16::from_be_bytes([data[cursor + 2], data[cursor + 3]]) as usize;
            validate_min_len(data, target_offset + 2, "segmentation FSA target state")?;
            stack.push(target_offset);
            cursor += SEGRULES_TRANSITION_SIZE;
        }
    }

    Ok(())
}

fn validate_reachable_vlength1_states(data: &[u8]) -> Result<()> {
    let mut seen = OffsetBitSet::new(data.len());
    let mut stack = vec![V1_INITIAL_STATE_OFFSET];

    while let Some(offset) = stack.pop() {
        if !seen.insert(offset) {
            continue;
        }
        if offset < V1_INITIAL_STATE_OFFSET {
            return Err(Error::invalid_dictionary(
                "VLength1 target precedes initial state",
            ));
        }

        let state_header = read_vlength1_state_header(data, offset)?;
        let mut cursor = state_header.transitions_offset;
        for _ in 0..state_header.transitions_num {
            validate_min_len(data, cursor + 1, "VLength1 transition")?;
            let first = data[cursor];
            cursor += 1;
            let offset_size = (first & V1_OFFSET_SIZE_MASK) as usize;
            let transition_short_label = first >> 2;
            if transition_short_label == 0 {
                validate_min_len(data, cursor + 1, "VLength1 transition label")?;
                cursor += 1;
            }

            let offset_end = cursor
                .checked_add(offset_size)
                .ok_or_else(|| Error::invalid_dictionary("VLength1 offset overflow"))?;
            validate_min_len(data, offset_end, "VLength1 offset")?;
            let relative_offset = read_vlength1_offset_at(data, cursor, offset_size);
            let next_offset = offset_end
                .checked_add(relative_offset)
                .ok_or_else(|| Error::invalid_dictionary("VLength1 transition overflow"))?;
            if next_offset < V1_INITIAL_STATE_OFFSET {
                return Err(Error::invalid_dictionary(
                    "VLength1 target precedes initial state",
                ));
            }
            validate_min_len(data, next_offset + 1, "VLength1 target state")?;
            stack.push(next_offset);
            cursor = offset_end;
        }
    }

    Ok(())
}

struct OffsetBitSet {
    words: Vec<u64>,
}

impl OffsetBitSet {
    fn new(max_offset: usize) -> Self {
        Self {
            words: vec![0; (max_offset / u64::BITS as usize) + 1],
        }
    }

    fn insert(&mut self, offset: usize) -> bool {
        let word_index = offset / u64::BITS as usize;
        let mask = 1u64 << (offset % u64::BITS as usize);
        let word = &mut self.words[word_index];
        if *word & mask != 0 {
            return false;
        }
        *word |= mask;
        true
    }
}

#[derive(Debug, Clone, Copy)]
struct SimpleStateHeader<'a> {
    accepting: bool,
    value: Option<&'a [u8]>,
    value_record_size: usize,
    transitions_num: usize,
    transitions_offset: usize,
}

fn read_simple_state_header<'a>(data: &'a [u8], offset: usize) -> Result<SimpleStateHeader<'a>> {
    validate_min_len(data, offset + 1, "Simple FSA state header")?;
    let first = data[offset];
    let accepting = first & SIMPLE_ACCEPTING_FLAG != 0;
    let transitions_num = (first & SIMPLE_TRANSITIONS_NUM_MASK) as usize;
    let mut transitions_offset = offset + 1;
    let mut value = None;
    let mut value_record_size = 0;
    if accepting {
        let (payload, size) = read_morph_payload(data, transitions_offset)?;
        value = Some(payload);
        value_record_size = size;
        transitions_offset += value_record_size;
    }

    Ok(SimpleStateHeader {
        accepting,
        value,
        value_record_size,
        transitions_num,
        transitions_offset,
    })
}

#[derive(Debug, Clone, Copy)]
struct VLength1StateHeader<'a> {
    accepting: bool,
    transitions_num: usize,
    value: Option<&'a [u8]>,
    value_record_size: usize,
    transitions_offset: usize,
}

fn read_vlength1_state_header<'a>(
    data: &'a [u8],
    offset: usize,
) -> Result<VLength1StateHeader<'a>> {
    validate_min_len(data, offset + 1, "VLength1 state header")?;
    let first = data[offset];
    let accepting = first & V1_ACCEPTING_FLAG != 0;
    let mut transitions_num = (first & V1_TRANSITIONS_NUM_MASK) as usize;
    let mut cursor = offset + 1;
    if transitions_num == V1_TRANSITIONS_NUM_MASK as usize {
        validate_min_len(data, cursor + 1, "VLength1 extended transitions count")?;
        transitions_num = data[cursor] as usize;
        cursor += 1;
    }

    let mut value = None;
    let mut value_record_size = 0;
    if accepting {
        let (payload, size) = read_morph_payload(data, cursor)?;
        value = Some(payload);
        value_record_size = size;
        cursor += value_record_size;
    }

    Ok(VLength1StateHeader {
        accepting,
        transitions_num,
        value,
        value_record_size,
        transitions_offset: cursor,
    })
}

#[cfg_attr(not(debug_assertions), inline(always))]
fn read_vlength1_state_header_loaded<'a>(data: &'a [u8], offset: usize) -> VLength1StateHeader<'a> {
    let first = byte_at_unchecked(data, offset);
    let accepting = first & V1_ACCEPTING_FLAG != 0;
    let mut transitions_num = (first & V1_TRANSITIONS_NUM_MASK) as usize;
    let mut cursor = offset + 1;
    if transitions_num == V1_TRANSITIONS_NUM_MASK as usize {
        transitions_num = byte_at_unchecked(data, cursor) as usize;
        cursor += 1;
    }

    let mut value = None;
    let mut value_record_size = 0;
    if accepting {
        let (payload, size) = read_morph_payload_loaded(data, cursor);
        value = Some(payload);
        value_record_size = size;
        cursor += value_record_size;
    }

    VLength1StateHeader {
        accepting,
        transitions_num,
        value,
        value_record_size,
        transitions_offset: cursor,
    }
}

#[derive(Debug, Clone, Copy)]
struct RawFsaState<'a> {
    offset: usize,
    accepting: bool,
    value: Option<&'a [u8]>,
    value_record_size: usize,
    transitions_num: usize,
    transitions_offset: usize,
}

impl<'a> RawFsaState<'a> {
    fn initial() -> Self {
        Self {
            offset: 0,
            accepting: false,
            value: None,
            value_record_size: 0,
            transitions_num: 0,
            transitions_offset: 0,
        }
    }

    fn from_target(data: &'a [u8], offset: usize, flags: u8) -> Result<Option<Self>> {
        let accepting = flags & V2_ACCEPTING_FLAG != 0;
        if !accepting {
            return Ok(Some(Self {
                offset,
                accepting,
                value: None,
                value_record_size: 0,
                transitions_num: 0,
                transitions_offset: 0,
            }));
        }

        let (value, value_record_size) = read_morph_payload(data, offset)?;
        Ok(Some(Self {
            offset,
            accepting,
            value: Some(value),
            value_record_size,
            transitions_num: 0,
            transitions_offset: 0,
        }))
    }
}

fn validate_min_len(bytes: &[u8], required: usize, section: &str) -> Result<()> {
    if bytes.len() < required {
        Err(Error::invalid_dictionary(format!(
            "truncated {section}: need at least {required} bytes, got {}",
            bytes.len()
        )))
    } else {
        Ok(())
    }
}

fn read_u32_at(bytes: &[u8], offset: usize, field: &str) -> Result<u32> {
    validate_min_len(bytes, offset + 4, field)?;
    Ok(u32::from_be_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("validated length"),
    ))
}

fn read_u24_at(bytes: &[u8], offset: usize, field: &str) -> Result<usize> {
    validate_min_len(bytes, offset + 3, field)?;
    Ok(((bytes[offset] as usize) << 16)
        | ((bytes[offset + 1] as usize) << 8)
        | bytes[offset + 2] as usize)
}

fn read_c_string_at(bytes: &[u8], offset: usize, field: &str) -> Result<(String, usize)> {
    validate_min_len(bytes, offset, field)?;
    let relative_end = bytes[offset..]
        .iter()
        .position(|byte| *byte == 0)
        .ok_or_else(|| Error::invalid_dictionary(format!("unterminated {field}")))?;
    let end = offset + relative_end;
    let value = String::from_utf8(bytes[offset..end].to_vec())
        .map_err(|_| Error::invalid_dictionary(format!("{field} is not valid UTF-8")))?;
    Ok((value, end + 1))
}

fn read_c_string_at_limit(
    bytes: &[u8],
    offset: usize,
    limit: usize,
    field: &str,
) -> Result<(String, usize)> {
    if offset > limit || limit > bytes.len() {
        return Err(Error::invalid_dictionary(format!(
            "{field} offset is outside metadata"
        )));
    }
    let relative_end = bytes[offset..limit]
        .iter()
        .position(|byte| *byte == 0)
        .ok_or_else(|| Error::invalid_dictionary(format!("unterminated {field}")))?;
    let end = offset + relative_end;
    let value = String::from_utf8(bytes[offset..end].to_vec())
        .map_err(|_| Error::invalid_dictionary(format!("{field} is not valid UTF-8")))?;
    Ok((value, end + 1))
}

fn read_id_string_table(
    bytes: &[u8],
    cursor: &mut usize,
    limit: usize,
    mut set_value: impl FnMut(i32, &str),
) -> Result<()> {
    validate_limit(bytes, *cursor + 2, limit, "id string table size")?;
    let entries = u16::from_be_bytes([bytes[*cursor], bytes[*cursor + 1]]) as usize;
    *cursor += 2;

    for _ in 0..entries {
        validate_limit(bytes, *cursor + 2, limit, "id string table id")?;
        let id = u16::from_be_bytes([bytes[*cursor], bytes[*cursor + 1]]) as i32;
        *cursor += 2;
        let (value, next) = read_c_string_at_limit(bytes, *cursor, limit, "id string value")?;
        set_value(id, &value);
        *cursor = next;
    }

    Ok(())
}

fn validate_limit(bytes: &[u8], required: usize, limit: usize, section: &str) -> Result<()> {
    if required > limit || required > bytes.len() {
        Err(Error::invalid_dictionary(format!(
            "truncated {section}: need offset {required}, metadata limit is {limit}"
        )))
    } else {
        Ok(())
    }
}

fn read_morph_payload(data: &[u8], offset: usize) -> Result<(&[u8], usize)> {
    validate_min_len(data, offset + 2, "morph payload size")?;
    let size = u16::from_be_bytes([data[offset], data[offset + 1]]) as usize;
    let value_start = offset + 2;
    let value_end = value_start
        .checked_add(size)
        .ok_or_else(|| Error::invalid_dictionary("morph payload size overflow"))?;
    validate_min_len(data, value_end, "morph payload")?;
    Ok((&data[value_start..value_end], size + 2))
}

#[cfg_attr(not(debug_assertions), inline(always))]
fn read_morph_payload_loaded(data: &[u8], offset: usize) -> (&[u8], usize) {
    let size = u16::from_be_bytes([
        byte_at_unchecked(data, offset),
        byte_at_unchecked(data, offset + 1),
    ]) as usize;
    let value_start = offset + 2;
    let value_end = value_start + size;
    debug_assert!(value_end <= data.len());
    let value = unsafe {
        // VLength1 loaded traversal is only constructed after
        // `validate_reachable_vlength1_states`, which validates every reachable
        // accepting payload boundary.
        data.get_unchecked(value_start..value_end)
    };
    (value, size + 2)
}

fn read_binary_analyzer_form(
    data: &[u8],
    cursor: &mut usize,
    compression_byte: u8,
) -> Result<BinaryAnalyzerForm> {
    let prefix_to_cut = if prefix_cut_encoded_in_compression_byte(compression_byte) {
        compression_byte & PREFIX_CUT_MASK
    } else {
        read_u8(data, cursor, "analyzer prefix cut")?
    };
    let suffix_to_cut = read_u8(data, cursor, "analyzer suffix cut")?;
    let suffix_to_add = read_c_string_from_slice(data, cursor, "analyzer suffix to add")?;
    let case_pattern = read_binary_lemma_case_pattern(data, cursor, compression_byte)?;

    Ok(BinaryAnalyzerForm {
        prefix_to_cut,
        suffix_to_cut,
        suffix_to_add,
        case_pattern,
    })
}

fn read_binary_orth_case_pattern(
    data: &[u8],
    cursor: &mut usize,
    compression_byte: u8,
) -> Result<BinaryCasePattern> {
    if compression_byte & ORTH_ONLY_LOWER != 0 {
        Ok(BinaryCasePattern::Lower)
    } else if compression_byte & ORTH_ONLY_TITLE != 0 {
        Ok(BinaryCasePattern::UpperPrefix(1))
    } else {
        read_binary_case_pattern(data, cursor)
    }
}

fn read_binary_lemma_case_pattern(
    data: &[u8],
    cursor: &mut usize,
    compression_byte: u8,
) -> Result<BinaryCasePattern> {
    if compression_byte & LEMMA_ONLY_LOWER != 0 {
        Ok(BinaryCasePattern::Lower)
    } else if compression_byte & LEMMA_ONLY_TITLE != 0 {
        Ok(BinaryCasePattern::UpperPrefix(1))
    } else {
        read_binary_case_pattern(data, cursor)
    }
}

fn read_binary_case_pattern(data: &[u8], cursor: &mut usize) -> Result<BinaryCasePattern> {
    match read_u8(data, cursor, "case pattern kind")? {
        CASE_PATTERN_ONLY_LOWER => Ok(BinaryCasePattern::Lower),
        CASE_PATTERN_UPPER_PREFIX => {
            let len = read_u8(data, cursor, "case pattern upper prefix length")? as usize;
            Ok(BinaryCasePattern::UpperPrefix(len))
        }
        CASE_PATTERN_MIXED => {
            let uppercase_count = read_u8(data, cursor, "case pattern uppercase count")? as usize;
            validate_min_len(
                data,
                *cursor + uppercase_count,
                "case pattern uppercase indices",
            )?;
            let indices = data[*cursor..*cursor + uppercase_count]
                .iter()
                .map(|index| *index as usize)
                .collect();
            *cursor += uppercase_count;
            Ok(BinaryCasePattern::Mixed(indices))
        }
        kind => Err(Error::invalid_dictionary(format!(
            "unsupported case pattern kind: {kind}"
        ))),
    }
}

fn skip_case_pattern(data: &[u8], cursor: &mut usize) -> Result<()> {
    match read_u8(data, cursor, "case pattern kind")? {
        CASE_PATTERN_ONLY_LOWER => Ok(()),
        CASE_PATTERN_UPPER_PREFIX => {
            let _ = read_u8(data, cursor, "case pattern upper prefix length")?;
            Ok(())
        }
        CASE_PATTERN_MIXED => {
            let uppercase_count = read_u8(data, cursor, "case pattern uppercase count")? as usize;
            validate_min_len(
                data,
                *cursor + uppercase_count,
                "case pattern uppercase indices",
            )?;
            *cursor += uppercase_count;
            Ok(())
        }
        kind => Err(Error::invalid_dictionary(format!(
            "unsupported case pattern kind: {kind}"
        ))),
    }
}

fn orth_case_patterns_encoded_in_compression_byte(byte: u8) -> bool {
    byte & (ORTH_ONLY_LOWER | ORTH_ONLY_TITLE) != 0
}

fn prefix_cut_encoded_in_compression_byte(byte: u8) -> bool {
    byte & PREFIX_CUT_MASK != PREFIX_CUT_MASK
}

fn read_u8(data: &[u8], cursor: &mut usize, field: &str) -> Result<u8> {
    validate_min_len(data, *cursor + 1, field)?;
    let value = data[*cursor];
    *cursor += 1;
    Ok(value)
}

fn read_u16(data: &[u8], cursor: &mut usize, field: &str) -> Result<u16> {
    validate_min_len(data, *cursor + 2, field)?;
    let value = u16::from_be_bytes([data[*cursor], data[*cursor + 1]]);
    *cursor += 2;
    Ok(value)
}

fn read_u32(data: &[u8], cursor: &mut usize, field: &str) -> Result<u32> {
    validate_min_len(data, *cursor + 4, field)?;
    let value = u32::from_be_bytes([
        data[*cursor],
        data[*cursor + 1],
        data[*cursor + 2],
        data[*cursor + 3],
    ]);
    *cursor += 4;
    Ok(value)
}

fn read_c_string_from_slice(data: &[u8], cursor: &mut usize, field: &str) -> Result<String> {
    let (value, next) = read_c_string_at_limit(data, *cursor, data.len(), field)?;
    *cursor = next;
    Ok(value)
}

fn drop_suffix_chars(value: &str, chars_to_cut: usize) -> Result<&str> {
    if chars_to_cut == 0 {
        return Ok(value);
    }
    if value.is_ascii() {
        let end = value.len().checked_sub(chars_to_cut).ok_or_else(|| {
            Error::invalid_dictionary(format!(
                "cannot cut {chars_to_cut} codepoints from {}-codepoint form",
                value.len()
            ))
        })?;
        return Ok(&value[..end]);
    }
    let chars_count = value.chars().count();
    if chars_to_cut > chars_count {
        return Err(Error::invalid_dictionary(format!(
            "cannot cut {chars_to_cut} codepoints from {chars_count}-codepoint form"
        )));
    }
    let keep = chars_count - chars_to_cut;
    let end = value
        .char_indices()
        .nth(keep)
        .map(|(index, _)| index)
        .unwrap_or(value.len());
    Ok(&value[..end])
}

trait AnalyzerFormView {
    fn prefix_to_cut(&self) -> u8;
    fn suffix_to_cut(&self) -> u8;
    fn suffix_to_add(&self) -> &str;
    fn case_pattern_is_empty(&self) -> bool;
    fn case_pattern_is_uppercase_at(&self, index: usize) -> bool;
}

impl AnalyzerFormView for EncodedForm {
    fn prefix_to_cut(&self) -> u8 {
        self.prefix_to_cut
    }

    fn suffix_to_cut(&self) -> u8 {
        self.suffix_to_cut
    }

    fn suffix_to_add(&self) -> &str {
        &self.suffix_to_add
    }

    fn case_pattern_is_empty(&self) -> bool {
        self.case_pattern.is_empty()
    }

    fn case_pattern_is_uppercase_at(&self, index: usize) -> bool {
        self.case_pattern.get(index).copied().unwrap_or(false)
    }
}

impl AnalyzerFormView for BinaryAnalyzerForm {
    fn prefix_to_cut(&self) -> u8 {
        self.prefix_to_cut
    }

    fn suffix_to_cut(&self) -> u8 {
        self.suffix_to_cut
    }

    fn suffix_to_add(&self) -> &str {
        &self.suffix_to_add
    }

    fn case_pattern_is_empty(&self) -> bool {
        self.case_pattern.is_empty()
    }

    fn case_pattern_is_uppercase_at(&self, index: usize) -> bool {
        self.case_pattern.is_uppercase_at(index)
    }
}

fn decode_analyzer_lemma(orth: &str, form: &EncodedForm) -> Result<String> {
    decode_analyzer_lemma_for_form(orth, form)
}

fn decode_analyzer_lemma_for_form<F>(orth: &str, form: &F) -> Result<String>
where
    F: AnalyzerFormView,
{
    let context = AnalyzerOrthContext::new(orth);
    decode_analyzer_lemma_for_form_in_context(&context, form)
}

fn decode_analyzer_lemma_for_form_in_context<F>(
    context: &AnalyzerOrthContext<'_>,
    form: &F,
) -> Result<String>
where
    F: AnalyzerFormView,
{
    if form.prefix_to_cut() == 0
        && form.suffix_to_cut() == 0
        && form.suffix_to_add().is_empty()
        && form.case_pattern_is_empty()
        && context.lowercases_to_self
    {
        return Ok(context.orth.to_owned());
    }
    decode_analyzer_lemma_with_prefix_context_len(
        context.orth,
        context.original_codepoints_len,
        context.lowercase_codepoints_len,
        form,
    )
}

fn decode_analyzer_prefix_lemma_for_form<F>(orth: &str, form: &F) -> Result<String>
where
    F: AnalyzerFormView,
{
    let normalized_len = AnalyzerOrthContext::new(orth).lowercase_codepoints_len;
    let start = form.prefix_to_cut() as usize;
    decode_analyzer_lemma_range(orth, start, normalized_len, form, false)
}

fn decode_analyzer_lemma_with_prefix_context_len<F>(
    orth: &str,
    non_prefix_codepoints_num: usize,
    normalized_len: usize,
    form: &F,
) -> Result<String>
where
    F: AnalyzerFormView,
{
    let prefix_codepoints_num = normalized_len
        .checked_sub(non_prefix_codepoints_num)
        .ok_or_else(|| {
            Error::invalid_dictionary(format!(
                "non-prefix codepoints {non_prefix_codepoints_num} exceed {}-codepoint orth",
                normalized_len
            ))
        })?;
    let start = prefix_codepoints_num + form.prefix_to_cut() as usize;
    let end = normalized_len
        .checked_sub(form.suffix_to_cut() as usize)
        .ok_or_else(|| {
            Error::invalid_dictionary(format!(
                "cannot cut {} codepoints from {}-codepoint orth",
                form.suffix_to_cut(),
                normalized_len
            ))
        })?;
    decode_analyzer_lemma_range(orth, start, end, form, true)
}

#[derive(Debug, Clone, Copy)]
struct AnalyzerOrthContext<'a> {
    orth: &'a str,
    original_codepoints_len: usize,
    lowercase_codepoints_len: usize,
    lowercases_to_self: bool,
}

impl<'a> AnalyzerOrthContext<'a> {
    fn new(orth: &'a str) -> Self {
        if orth.is_ascii() {
            return Self {
                orth,
                original_codepoints_len: orth.len(),
                lowercase_codepoints_len: orth.len(),
                lowercases_to_self: orth.bytes().all(|byte| !byte.is_ascii_uppercase()),
            };
        }

        // Morfeusz lowercasing is 1:1 (codepoint -> codepoint) via its own table,
        // so the lowercased form always has the same codepoint count as the orth.
        let mut original_codepoints_len = 0usize;
        let mut lowercases_to_self = true;
        for ch in orth.chars() {
            original_codepoints_len += 1;
            if crate::case_tables::to_lower_char(ch) != ch {
                lowercases_to_self = false;
            }
        }

        Self {
            orth,
            original_codepoints_len,
            lowercase_codepoints_len: original_codepoints_len,
            lowercases_to_self,
        }
    }
}

fn decode_analyzer_lemma_range<F>(
    orth: &str,
    start: usize,
    end: usize,
    form: &F,
    append_suffix: bool,
) -> Result<String>
where
    F: AnalyzerFormView,
{
    if start > end {
        return Err(Error::invalid_dictionary(format!(
            "prefix cut {start} exceeds lemma end {end}"
        )));
    }

    if form.case_pattern_is_empty() && orth.is_ascii() {
        if end > orth.len() {
            return Err(Error::invalid_dictionary(format!(
                "lemma end {end} exceeds normalized {}-codepoint orth",
                orth.len()
            )));
        }
        let mut lemma = String::with_capacity(
            end - start
                + if append_suffix {
                    form.suffix_to_add().len()
                } else {
                    0
                },
        );
        lemma.push_str(&orth[start..end]);
        lemma.make_ascii_lowercase();
        if append_suffix {
            lemma.push_str(form.suffix_to_add());
        }
        return Ok(lemma);
    }

    let mut lemma = String::with_capacity(
        orth.len()
            + if append_suffix {
                form.suffix_to_add().len()
            } else {
                0
            },
    );
    let mut index = 0usize;
    'chars: for ch in orth.chars() {
        if ch.is_ascii() {
            if index >= end {
                break;
            }
            if index < start {
                index += 1;
                continue;
            }

            let lowered = ch.to_ascii_lowercase();
            if form.case_pattern_is_uppercase_at(index) {
                lemma.push(lowered.to_ascii_uppercase());
            } else {
                lemma.push(lowered);
            }
            index += 1;
            continue;
        }

        {
            // 1:1 Morfeusz lowercasing (see `to_lower_char`), so one orth
            // codepoint maps to exactly one lemma codepoint.
            let lowered = crate::case_tables::to_lower_char(ch);
            if index >= end {
                break 'chars;
            }
            if index >= start {
                if form.case_pattern_is_uppercase_at(index) {
                    lemma.extend(lowered.to_uppercase());
                } else {
                    lemma.push(lowered);
                }
            }
            index += 1;
        }
    }
    if index < end {
        return Err(Error::invalid_dictionary(format!(
            "lemma end {end} exceeds normalized {}-codepoint orth",
            index
        )));
    }
    if append_suffix {
        lemma.push_str(form.suffix_to_add());
    }
    Ok(lemma)
}

fn skip_vlength2_offset(data: &[u8], cursor: &mut usize) -> Result<()> {
    read_vlength2_offset(data, cursor).map(|_| ())
}

fn read_vlength2_offset(data: &[u8], cursor: &mut usize) -> Result<usize> {
    validate_min_len(data, *cursor + 1, "VLength2 offset")?;
    let first = data[*cursor];
    *cursor += 1;
    let mut offset = (first & V2_FIRST_BYTE_OFFSET_MASK) as usize;

    if first & V2_HAS_REMAINING_FLAG != 0 {
        loop {
            validate_min_len(data, *cursor + 1, "VLength2 extended offset")?;
            let byte = data[*cursor];
            *cursor += 1;
            offset = offset
                .checked_shl(7)
                .and_then(|value| value.checked_add((byte & V2_OFFSET_MASK) as usize))
                .ok_or_else(|| Error::invalid_dictionary("VLength2 offset overflow"))?;
            if byte & V2_HAS_REMAINING_FLAG == 0 {
                break;
            }
        }
    }

    Ok(offset)
}

#[cfg_attr(not(debug_assertions), inline(always))]
fn read_vlength1_offset_at(data: &[u8], cursor: usize, size: usize) -> usize {
    match size {
        0 => 0,
        1 => byte_at_unchecked(data, cursor) as usize,
        2 => {
            ((byte_at_unchecked(data, cursor) as usize) << 8)
                | byte_at_unchecked(data, cursor + 1) as usize
        }
        3 => {
            ((byte_at_unchecked(data, cursor) as usize) << 16)
                | ((byte_at_unchecked(data, cursor + 1) as usize) << 8)
                | byte_at_unchecked(data, cursor + 2) as usize
        }
        _ => unreachable!("validated VLength1 offset size"),
    }
}

#[cfg_attr(not(debug_assertions), inline(always))]
fn byte_at_unchecked(data: &[u8], index: usize) -> u8 {
    debug_assert!(index < data.len());
    unsafe {
        // Callers validate dictionary byte ranges before entering loaded FSA
        // traversal; debug builds still assert the local precondition.
        *data.get_unchecked(index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Engine;

    #[test]
    fn parses_binary_dictionary_sections() {
        let bytes = minimal_dictionary_bytes();

        let dictionary = BinaryDictionaryData::from_bytes(bytes).unwrap();

        assert_eq!(dictionary.version(), VERSION_NUM);
        assert_eq!(dictionary.implementation(), FsaImplementation::VLength2);
        assert_eq!(dictionary.fsa_data(), [0xaa, 0xbb]);
        assert_eq!(dictionary.dict_id(), "test-dict");
        assert_eq!(dictionary.copyright(), "copyright");
        assert_eq!(
            dictionary.segmentation_rules_data(),
            segmentation_metadata_bytes()
        );
    }

    #[test]
    fn rejects_invalid_magic() {
        let mut bytes = minimal_dictionary_bytes();
        bytes[0] = 0;

        assert!(matches!(
            BinaryDictionaryData::from_bytes(bytes),
            Err(Error::InvalidDictionary(message)) if message.contains("magic")
        ));
    }

    #[test]
    fn rejects_truncated_fsa_data() {
        let mut bytes = minimal_dictionary_bytes();
        bytes[FSA_DATA_SIZE_OFFSET..FSA_DATA_SIZE_OFFSET + 4]
            .copy_from_slice(&999_999_u32.to_be_bytes());

        assert!(matches!(
            BinaryDictionaryData::from_bytes(bytes),
            Err(Error::InvalidDictionary(message)) if message.contains("FSA data")
        ));
    }

    #[test]
    fn rejects_unknown_implementation_code() {
        let mut bytes = minimal_dictionary_bytes();
        bytes[IMPLEMENTATION_NUM_OFFSET] = 9;

        assert!(matches!(
            BinaryDictionaryData::from_bytes(bytes),
            Err(Error::InvalidDictionary(message)) if message.contains("implementation")
        ));
    }

    #[test]
    fn recognizes_raw_vlength2_payload() {
        let fsa = VLength2Fsa::new(&[
            b'a',
            V2_LAST_FLAG | V2_ACCEPTING_FLAG,
            0,
            3,
            0xde,
            0xad,
            0xbe,
            V2_LAST_FLAG,
        ]);

        let matched = fsa.try_recognize(b"a").unwrap().unwrap();

        assert_eq!(matched.state_offset, 2);
        assert_eq!(matched.value, [0xde, 0xad, 0xbe]);
        assert!(fsa.try_recognize(b"b").unwrap().is_none());
    }

    #[test]
    fn recognizes_raw_simple_payload() {
        let fsa = SimpleFsa::new(&[0x01, b'a', 0, 0, 5, 0x80, 0, 1, 0x11], false).unwrap();

        let matched = fsa.try_recognize(b"a").unwrap().unwrap();

        assert_eq!(matched.state_offset, 5);
        assert_eq!(matched.value, [0x11]);
        assert!(fsa.try_recognize(b"b").unwrap().is_none());
    }

    #[test]
    fn skips_accepting_payload_before_following_simple_transitions() {
        let fsa = SimpleFsa::new(
            &[
                0x01, b'a', 0, 0, 5, 0x81, 0, 1, 0x11, b'b', 0, 0, 13, 0x80, 0, 1, 0x22,
            ],
            false,
        )
        .unwrap();

        let prefix = fsa.try_recognize(b"a").unwrap().unwrap();
        let full = fsa.try_recognize(b"ab").unwrap().unwrap();

        assert_eq!(prefix.value, [0x11]);
        assert_eq!(full.value, [0x22]);
        assert!(fsa.try_recognize(b"ac").unwrap().is_none());
    }

    #[test]
    fn recognizes_simple_payload_with_transition_data() {
        let fsa = SimpleFsa::new(&[0x01, b'a', 0, 0, 6, 0xee, 0x80, 0, 1, 0x11], true).unwrap();

        let matched = fsa.try_recognize(b"a").unwrap().unwrap();

        assert_eq!(matched.state_offset, 6);
        assert_eq!(matched.value, [0x11]);
    }

    #[test]
    fn recognizes_raw_vlength1_payload() {
        let fsa_data = vlength1_fsa_data(&[0x01, 0x00, b'a', 0x80, 0, 1, 0x11]);
        let fsa = VLength1Fsa::new(&fsa_data).unwrap();

        let matched = fsa.try_recognize(b"a").unwrap().unwrap();

        assert_eq!(matched.state_offset, 3);
        assert_eq!(matched.value, [0x11]);
        assert!(fsa.try_recognize(b"b").unwrap().is_none());
    }

    #[test]
    fn rejects_truncated_vlength1_transition_at_load() {
        let fsa_data = vlength1_fsa_data(&[0x01, 0x00]);

        assert!(matches!(
            VLength1Fsa::new(&fsa_data),
            Err(Error::InvalidDictionary(message)) if message.contains("transition label")
        ));
    }

    #[test]
    fn rejects_vlength1_target_past_data_at_load() {
        let fsa_data = vlength1_fsa_data(&[0x01, 0x05, 0xff]);

        assert!(matches!(
            VLength1Fsa::new(&fsa_data),
            Err(Error::InvalidDictionary(message)) if message.contains("target state")
        ));
    }

    #[test]
    fn normalized_identity_span_preserves_original_case_offsets() {
        let original = "ABC";
        let normalized = lowercase_with_original_boundaries(original);

        assert_eq!(normalized.as_str(), "abc");
        assert_eq!(normalized.original_span(original, 0, 1), Some(("A", 0, 1)));
        assert_eq!(normalized.original_span(original, 1, 3), Some(("BC", 1, 3)));
    }

    #[test]
    fn normalized_contracting_lowercase_span_uses_original_boundaries() {
        // 'İ' (U+0130, 2 bytes) lowercases to a single 'i' (1 byte) via the
        // Morfeusz table — NOT Unicode's two-codepoint "i\u{0307}". The boundary
        // table must still map back to the original 2-byte span.
        let original = "\u{0130}x";
        let normalized = lowercase_with_original_boundaries(original);

        assert_eq!(normalized.as_str(), "ix");
        assert_eq!(
            normalized.original_span(original, 0, 1),
            Some(("\u{0130}", 0, 2))
        );
        assert_eq!(normalized.original_span(original, 1, 2), Some(("x", 2, 3)));
    }

    #[test]
    fn skips_accepting_payload_before_following_vlength1_transitions() {
        let fsa_data = vlength1_fsa_data(&[
            0x01, 0x00, b'a', 0x81, 0, 1, 0x11, 0x00, b'b', 0x80, 0, 1, 0x22,
        ]);
        let fsa = VLength1Fsa::new(&fsa_data).unwrap();

        let prefix = fsa.try_recognize(b"a").unwrap().unwrap();
        let full = fsa.try_recognize(b"ab").unwrap().unwrap();

        assert_eq!(prefix.value, [0x11]);
        assert_eq!(full.value, [0x22]);
        assert!(fsa.try_recognize(b"ac").unwrap().is_none());
    }

    #[test]
    fn collects_vlength1_prefix_matches() {
        let fsa_data = vlength1_fsa_data(&[
            0x01, 0x00, b'a', 0x81, 0, 1, 0x11, 0x00, b'b', 0x80, 0, 1, 0x22,
        ]);
        let fsa = VLength1Fsa::new(&fsa_data).unwrap();

        let matches = fsa.prefix_matches(b"ab").unwrap();

        assert_eq!(
            matches,
            [
                RawFsaPrefixMatch {
                    input_end: 1,
                    state_offset: 3,
                    value: &[0x11],
                },
                RawFsaPrefixMatch {
                    input_end: 2,
                    state_offset: 9,
                    value: &[0x22],
                }
            ]
        );
    }

    #[test]
    fn skips_accepting_payload_before_following_vlength2_transitions() {
        let fsa = VLength2Fsa::new(&[
            b'a',
            V2_LAST_FLAG | V2_ACCEPTING_FLAG,
            0,
            1,
            0x11,
            b'b',
            V2_LAST_FLAG | V2_ACCEPTING_FLAG,
            0,
            1,
            0x22,
            V2_LAST_FLAG,
        ]);

        let prefix = fsa.try_recognize(b"a").unwrap().unwrap();
        let full = fsa.try_recognize(b"ab").unwrap().unwrap();

        assert_eq!(prefix.value, [0x11]);
        assert_eq!(full.value, [0x22]);
        assert!(fsa.try_recognize(b"ac").unwrap().is_none());
    }

    #[test]
    fn collects_vlength2_prefix_matches() {
        let fsa = VLength2Fsa::new(&[
            b'a',
            V2_LAST_FLAG | V2_ACCEPTING_FLAG,
            0,
            1,
            0x11,
            b'b',
            V2_LAST_FLAG | V2_ACCEPTING_FLAG,
            0,
            1,
            0x22,
            0,
            V2_LAST_FLAG,
        ]);

        let matches = fsa.prefix_matches(b"ab").unwrap();

        assert_eq!(
            matches,
            [
                RawFsaPrefixMatch {
                    input_end: 1,
                    state_offset: 2,
                    value: &[0x11],
                },
                RawFsaPrefixMatch {
                    input_end: 2,
                    state_offset: 7,
                    value: &[0x22],
                }
            ]
        );
    }

    #[test]
    fn reads_raw_interpretation_groups() {
        let payload = [7, 0, 2, 0xaa, 0xbb, 9, 0, 1, 0xcc];

        let groups = read_raw_interps_groups(&payload).unwrap();

        assert_eq!(
            groups,
            [
                RawInterpsGroup {
                    segment_type: 7,
                    data: &[0xaa, 0xbb]
                },
                RawInterpsGroup {
                    segment_type: 9,
                    data: &[0xcc]
                }
            ]
        );
    }

    #[test]
    fn rejects_truncated_interpretation_group() {
        let payload = [7, 0, 3, 0xaa];

        assert!(matches!(
            read_raw_interps_groups(&payload),
            Err(Error::InvalidDictionary(message)) if message.contains("group data")
        ));
    }

    #[test]
    fn decodes_compressed_analyzer_interpretation_record() {
        let group = RawInterpsGroup {
            segment_type: 4,
            data: &[ORTH_ONLY_LOWER | LEMMA_ONLY_LOWER, 0, 0, 0, 42, 7, 0, 9],
        };

        let decoded = decode_analyzer_interpretations(group).unwrap();

        assert_eq!(
            decoded,
            [EncodedAnalyzerInterpretation {
                orth_case_pattern: Vec::new(),
                form: EncodedForm {
                    prefix_to_cut: 0,
                    suffix_to_cut: 0,
                    suffix_to_add: String::new(),
                    case_pattern: Vec::new(),
                    prefix_to_add: String::new(),
                },
                tag_id: 42,
                name_id: 7,
                labels_id: 9,
            }]
        );
    }

    #[test]
    fn decodes_analyzer_interps_groups_with_segment_types() {
        let payload = analyzer_groups_payload(&[4, 8]);

        let groups = decode_analyzer_interps_groups(&payload).unwrap();

        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].segment_type, 4);
        assert_eq!(groups[0].interpretations[0].tag_id, 42);
        assert_eq!(groups[1].segment_type, 8);
        assert_eq!(groups[1].interpretations[0].name_id, 7);
    }

    #[test]
    fn analyzer_decode_cache_reuses_groups_across_words() {
        let shared = SharedAnalyzerGroupDecodeCache::default();
        let group = RawInterpsGroup {
            segment_type: 4,
            data: &[ORTH_ONLY_LOWER | LEMMA_ONLY_LOWER, 0, 0, 0, 42, 7, 0, 9],
        };
        let mut first_word = AnalyzerGroupDecodeCache::with_capacity(shared.clone(), 1);

        let first_index = first_word.get_or_decode((123, 0), group).unwrap();

        assert_eq!(first_word.interpretations(first_index)[0].tag_id, 42);

        let invalid_if_decoded = RawInterpsGroup {
            segment_type: 4,
            data: &[],
        };
        let mut second_word = AnalyzerGroupDecodeCache::with_capacity(shared, 1);
        let second_index = second_word
            .get_or_decode((123, 0), invalid_if_decoded)
            .unwrap();

        assert_eq!(second_word.interpretations(second_index)[0].name_id, 7);
    }

    #[test]
    fn decodes_explicit_analyzer_case_patterns_and_prefix_cut() {
        let group = RawInterpsGroup {
            segment_type: 4,
            data: &[
                PREFIX_CUT_MASK,
                0,
                CASE_PATTERN_MIXED,
                1,
                1,
                3,
                1,
                b'x',
                0,
                CASE_PATTERN_UPPER_PREFIX,
                2,
                0,
                42,
                7,
                0,
                9,
            ],
        };

        let decoded = decode_analyzer_interpretations(group).unwrap();

        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].orth_case_pattern, [false, true]);
        assert_eq!(decoded[0].form.prefix_to_cut, 3);
        assert_eq!(decoded[0].form.suffix_to_cut, 1);
        assert_eq!(decoded[0].form.suffix_to_add, "x");
        assert_eq!(decoded[0].form.case_pattern, [true, true]);
        assert_eq!(decoded[0].tag_id, 42);
        assert_eq!(decoded[0].name_id, 7);
        assert_eq!(decoded[0].labels_id, 9);
    }

    #[test]
    fn decodes_generator_interpretation_record() {
        let group = RawInterpsGroup {
            segment_type: 4,
            data: &[
                b's', b'1', 0, b'p', b'r', b'e', 0, 2, b's', b'u', b'f', 0, 0, 42, 7, 0, 9,
            ],
        };

        let decoded = decode_generator_interpretations(group).unwrap();

        assert_eq!(
            decoded,
            [EncodedGeneratorInterpretation {
                homonym_id: "s1".to_owned(),
                form: EncodedForm {
                    prefix_to_cut: 0,
                    suffix_to_cut: 2,
                    suffix_to_add: "suf".to_owned(),
                    case_pattern: Vec::new(),
                    prefix_to_add: "pre".to_owned(),
                },
                tag_id: 42,
                name_id: 7,
                labels_id: 9,
            }]
        );
    }

    #[test]
    fn decodes_generator_interps_groups_with_segment_types() {
        let payload = generator_groups_payload(&[4, 8]);

        let groups = decode_generator_interps_groups(&payload).unwrap();

        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].segment_type, 4);
        assert_eq!(groups[0].interpretations[0].form.suffix_to_cut, 0);
        assert_eq!(groups[1].segment_type, 8);
        assert_eq!(groups[1].interpretations[0].tag_id, 42);
    }

    #[test]
    fn generator_decode_cache_reuses_groups_across_lemmas() {
        let shared = SharedGeneratorGroupDecodeCache::default();
        let group = RawInterpsGroup {
            segment_type: 4,
            data: &[
                b's', b'1', 0, b'p', b'r', b'e', 0, 2, b's', b'u', b'f', 0, 0, 42, 7, 0, 9,
            ],
        };
        let mut first_lemma = GeneratorGroupDecodeCache::with_capacity(shared.clone(), 1);

        let first_index = first_lemma.get_or_decode((456, 0), group).unwrap();

        assert_eq!(
            first_lemma.interpretations(first_index)[0]
                .form
                .prefix_to_add,
            "pre"
        );

        let invalid_if_decoded = RawInterpsGroup {
            segment_type: 4,
            data: &[],
        };
        let mut second_lemma = GeneratorGroupDecodeCache::with_capacity(shared, 1);
        let second_index = second_lemma
            .get_or_decode((456, 0), invalid_if_decoded)
            .unwrap();

        assert_eq!(second_lemma.interpretations(second_index)[0].tag_id, 42);
    }

    #[test]
    fn applies_generator_form_with_unicode_codepoint_suffix_cut() {
        let interp = EncodedGeneratorInterpretation {
            homonym_id: "s1".to_owned(),
            form: EncodedForm {
                prefix_to_cut: 0,
                suffix_to_cut: 1,
                suffix_to_add: "ego".to_owned(),
                case_pattern: Vec::new(),
                prefix_to_add: "naj".to_owned(),
            },
            tag_id: 42,
            name_id: 7,
            labels_id: 9,
        };

        let decoded = interp.to_morph_interpretation("żółty", 2, 3).unwrap();

        assert_eq!(decoded.start_node, 2);
        assert_eq!(decoded.end_node, 3);
        assert_eq!(decoded.orth, "najżółtego");
        assert_eq!(decoded.lemma, "żółty:s1");
        assert_eq!(decoded.tag_id, 42);
        assert_eq!(decoded.name_id, 7);
        assert_eq!(decoded.labels_id, 9);
    }

    #[test]
    fn applies_generator_form_without_suffix_cut_to_unicode_lemma() {
        let interp = EncodedGeneratorInterpretation {
            homonym_id: String::new(),
            form: EncodedForm {
                prefix_to_cut: 0,
                suffix_to_cut: 0,
                suffix_to_add: String::new(),
                case_pattern: Vec::new(),
                prefix_to_add: String::new(),
            },
            tag_id: 42,
            name_id: 7,
            labels_id: 9,
        };

        let decoded = interp.to_morph_interpretation("żółty", 2, 3).unwrap();

        assert_eq!(decoded.orth, "żółty");
        assert_eq!(decoded.lemma, "żółty");
    }

    #[test]
    fn applies_generator_form_with_ascii_suffix_cut() {
        let interp = EncodedGeneratorInterpretation {
            homonym_id: String::new(),
            form: EncodedForm {
                prefix_to_cut: 0,
                suffix_to_cut: 2,
                suffix_to_add: "ed".to_owned(),
                case_pattern: Vec::new(),
                prefix_to_add: String::new(),
            },
            tag_id: 42,
            name_id: 7,
            labels_id: 9,
        };

        let decoded = interp.to_morph_interpretation("testxx", 2, 3).unwrap();

        assert_eq!(decoded.orth, "tested");
        assert_eq!(decoded.lemma, "testxx");
    }

    #[test]
    fn rejects_generator_suffix_cut_longer_than_lemma() {
        let interp = EncodedGeneratorInterpretation {
            homonym_id: String::new(),
            form: EncodedForm {
                prefix_to_cut: 0,
                suffix_to_cut: 2,
                suffix_to_add: String::new(),
                case_pattern: Vec::new(),
                prefix_to_add: String::new(),
            },
            tag_id: 0,
            name_id: 0,
            labels_id: 0,
        };

        assert!(matches!(
            interp.to_morph_interpretation("a", 0, 0),
            Err(Error::InvalidDictionary(message)) if message.contains("cannot cut")
        ));
    }

    #[test]
    fn applies_analyzer_form_with_unicode_case_pattern() {
        let interp = EncodedAnalyzerInterpretation {
            orth_case_pattern: Vec::new(),
            form: EncodedForm {
                prefix_to_cut: 0,
                suffix_to_cut: 1,
                suffix_to_add: "ego".to_owned(),
                case_pattern: vec![true],
                prefix_to_add: String::new(),
            },
            tag_id: 42,
            name_id: 7,
            labels_id: 9,
        };

        let decoded = interp.to_morph_interpretation("ŻÓŁTY", 2, 3).unwrap();

        assert_eq!(decoded.start_node, 2);
        assert_eq!(decoded.end_node, 3);
        assert_eq!(decoded.orth, "ŻÓŁTY");
        assert_eq!(decoded.lemma, "Żółtego");
        assert_eq!(decoded.tag_id, 42);
        assert_eq!(decoded.name_id, 7);
        assert_eq!(decoded.labels_id, 9);
    }

    #[test]
    fn applies_analyzer_identity_form_to_unicode_lowercase_orth() {
        let interp = EncodedAnalyzerInterpretation {
            orth_case_pattern: Vec::new(),
            form: EncodedForm {
                prefix_to_cut: 0,
                suffix_to_cut: 0,
                suffix_to_add: String::new(),
                case_pattern: Vec::new(),
                prefix_to_add: String::new(),
            },
            tag_id: 42,
            name_id: 7,
            labels_id: 9,
        };

        let decoded = interp.to_morph_interpretation("żółty", 2, 3).unwrap();

        assert_eq!(decoded.orth, "żółty");
        assert_eq!(decoded.lemma, "żółty");
    }

    #[test]
    fn analyzer_orth_context_reuses_unicode_lengths_for_binary_forms() {
        let context = AnalyzerOrthContext::new("ŻÓŁTY");
        let form = BinaryAnalyzerForm {
            prefix_to_cut: 0,
            suffix_to_cut: 1,
            suffix_to_add: "ego".to_owned(),
            case_pattern: BinaryCasePattern::UpperPrefix(1),
        };

        assert_eq!(context.original_codepoints_len, 5);
        assert_eq!(context.lowercase_codepoints_len, 5);
        assert!(!context.lowercases_to_self);
        assert_eq!(
            decode_analyzer_lemma_for_form_in_context(&context, &form).unwrap(),
            "Żółtego"
        );
    }

    #[test]
    fn matches_analyzer_orth_case_pattern() {
        let interp = EncodedAnalyzerInterpretation {
            orth_case_pattern: vec![true, false, true],
            form: EncodedForm {
                prefix_to_cut: 0,
                suffix_to_cut: 0,
                suffix_to_add: String::new(),
                case_pattern: Vec::new(),
                prefix_to_add: String::new(),
            },
            tag_id: 0,
            name_id: 0,
            labels_id: 0,
        };

        assert!(interp.matches_orth_case("AbC"));
        assert!(interp.matches_orth_case("ABC"));
        assert!(interp.matches_orth_case("İxC"));
        assert!(!interp.matches_orth_case("abc"));
        assert!(!interp.matches_orth_case("Ab"));
        assert!(!interp.matches_orth_case("ßxC"));
    }

    #[test]
    fn applies_analyzer_prefix_and_suffix_cuts() {
        let interp = EncodedAnalyzerInterpretation {
            orth_case_pattern: Vec::new(),
            form: EncodedForm {
                prefix_to_cut: 1,
                suffix_to_cut: 1,
                suffix_to_add: "ny".to_owned(),
                case_pattern: Vec::new(),
                prefix_to_add: String::new(),
            },
            tag_id: 1,
            name_id: 2,
            labels_id: 3,
        };

        let decoded = interp.to_morph_interpretation("ABCDE", 0, 1).unwrap();

        assert_eq!(decoded.lemma, "bcdny");
    }

    #[test]
    fn rejects_analyzer_cuts_outside_orth() {
        let interp = EncodedAnalyzerInterpretation {
            orth_case_pattern: Vec::new(),
            form: EncodedForm {
                prefix_to_cut: 2,
                suffix_to_cut: 2,
                suffix_to_add: String::new(),
                case_pattern: Vec::new(),
                prefix_to_add: String::new(),
            },
            tag_id: 0,
            name_id: 0,
            labels_id: 0,
        };

        assert!(matches!(
            interp.to_morph_interpretation("a", 0, 1),
            Err(Error::InvalidDictionary(message)) if message.contains("cannot cut")
                || message.contains("prefix cut")
        ));
    }

    #[test]
    fn reads_binary_id_resolver_tables() {
        let dictionary =
            BinaryDictionaryData::from_bytes(dictionary_bytes_with_id_resolver()).unwrap();

        let resolver = dictionary.id_resolver().unwrap();

        assert_eq!(resolver.tagset_id(), "test-tagset");
        assert_eq!(resolver.tag(42), Some("subst:sg:nom:m1"));
        assert_eq!(resolver.tag_id("subst:sg:nom:m1").unwrap(), 42);
        assert_eq!(resolver.name(7), Some("wlasna"));
        assert_eq!(resolver.name_id("wlasna").unwrap(), 7);
        assert_eq!(resolver.labels_as_string(9), Some("a|b"));
        assert_eq!(resolver.labels_id("a|b").unwrap(), 9);
        assert!(resolver.labels_id("b|a").is_err());
        assert!(resolver.labels(9).unwrap().contains("a"));
    }

    #[test]
    fn reads_binary_segmentation_metadata() {
        let dictionary =
            BinaryDictionaryData::from_bytes(dictionary_bytes_with_id_resolver()).unwrap();

        let metadata = dictionary.segmentation_metadata().unwrap();

        assert_eq!(metadata.separators, [44, 46]);
        assert_eq!(metadata.fsa_variants.len(), 1);
        assert_eq!(
            metadata
                .available_options("aggl")
                .into_iter()
                .collect::<Vec<_>>(),
            ["permissive"]
        );
        assert_eq!(
            metadata
                .available_options("praet")
                .into_iter()
                .collect::<Vec<_>>(),
            ["split"]
        );
        assert_eq!(
            metadata.fsa_variants[0]
                .options
                .get("aggl")
                .map(String::as_str),
            Some("permissive")
        );
        assert_eq!(
            metadata.fsa_variants[0]
                .options
                .get("praet")
                .map(String::as_str),
            Some("split")
        );
        assert_eq!(metadata.fsa_variants[0].fsa, [1, 0]);
        assert_eq!(
            metadata.default_options.get("aggl").map(String::as_str),
            Some("permissive")
        );
        assert_eq!(
            metadata.default_options.get("praet").map(String::as_str),
            Some("split")
        );
        let default_variant = metadata.default_fsa_variant().unwrap();
        let rules_fsa = default_variant.rules_fsa().unwrap();
        assert_eq!(rules_fsa.initial_state(), SegmentationState::initial());
    }

    #[test]
    fn selects_segmentation_fsa_variant_from_runtime_options() {
        let metadata = SegmentationMetadata {
            separators: Vec::new(),
            fsa_variants: vec![
                SegmentationFsaVariant {
                    options: options_map(&[("aggl", "strict"), ("praet", "split")]),
                    fsa: vec![0, 0],
                },
                SegmentationFsaVariant {
                    options: options_map(&[("aggl", "permissive"), ("praet", "split")]),
                    fsa: vec![0, 1, 4, 0, 0, 6, 1, 0],
                },
            ],
            default_options: options_map(&[("aggl", "strict"), ("praet", "split")]),
        };
        let dictionary_default = SegmentationPreset::default();
        let strict = SegmentationPreset::new("strict", "split").unwrap();
        let permissive = SegmentationPreset::new("permissive", "split").unwrap();
        let invalid_combo = SegmentationPreset::new("permissive", "composite").unwrap();

        assert_eq!(
            metadata
                .available_options("aggl")
                .into_iter()
                .collect::<Vec<_>>(),
            ["permissive", "strict"]
        );
        assert_eq!(
            metadata
                .available_options("praet")
                .into_iter()
                .collect::<Vec<_>>(),
            ["split"]
        );
        assert_eq!(
            segmentation_fsa_for_options(&metadata, &dictionary_default).unwrap(),
            Some([0, 0].as_slice())
        );
        assert_eq!(default_segmentation_fsa_variant_index(&metadata), Some(0));
        assert_eq!(
            segmentation_fsa_for_options(&metadata, &strict).unwrap(),
            Some([0, 0].as_slice())
        );
        assert_eq!(
            segmentation_fsa_for_options(&metadata, &permissive).unwrap(),
            Some([0, 1, 4, 0, 0, 6, 1, 0].as_slice())
        );
        assert!(matches!(
            segmentation_fsa_for_options(&metadata, &invalid_combo),
            Err(Error::InvalidArgument(message))
                if message.contains("aggl=permissive") && message.contains("praet=composite")
        ));
        assert!(matches!(
            validate_segmentation_options(&metadata, &invalid_combo, "praet", "composite"),
            Err(Error::InvalidArgument(message))
                if message.contains("Invalid \"praet\" option")
                    && message.contains("\"split\"")
        ));

        for preset in [&dictionary_default, &strict, &permissive] {
            assert_eq!(
                metadata
                    .fsa_variant_for_preset(preset)
                    .map(|variant| variant.fsa.as_slice()),
                metadata
                    .fsa_variant_for_options(&effective_segmentation_options(&metadata, preset))
                    .map(|variant| variant.fsa.as_slice())
            );
        }
    }

    #[test]
    fn rejects_trailing_segmentation_metadata_bytes() {
        let mut bytes = dictionary_bytes_with_id_resolver();
        bytes.push(0xff);
        let dictionary = BinaryDictionaryData::from_bytes(bytes).unwrap();

        assert!(matches!(
            dictionary.segmentation_metadata(),
            Err(Error::InvalidDictionary(message)) if message.contains("trailing")
        ));
    }

    #[test]
    fn traverses_segmentation_rules_fsa() {
        let bytes = segmentation_rules_fsa_bytes();
        let fsa = SegmentationRulesFsa::new(&bytes).unwrap();
        let initial = fsa.initial_state();

        let terminal = fsa.proceed_to_next(4, initial, true).unwrap().unwrap();
        assert_eq!(
            terminal,
            SegmentationState {
                offset: 10,
                accepting: true,
                weak: false,
                shift_orth_from_previous: false,
                sink: true,
                failed: false,
            }
        );
        assert!(fsa.proceed_to_next(4, initial, false).unwrap().is_none());

        let mid = fsa.proceed_to_next(5, initial, false).unwrap().unwrap();
        assert_eq!(
            mid,
            SegmentationState {
                offset: 12,
                accepting: false,
                weak: false,
                shift_orth_from_previous: true,
                sink: false,
                failed: false,
            }
        );
        assert!(fsa.proceed_to_next(5, initial, true).unwrap().is_none());

        let final_state = fsa.proceed_to_next(6, mid, true).unwrap().unwrap();
        assert_eq!(
            final_state,
            SegmentationState {
                offset: 18,
                accepting: true,
                weak: true,
                shift_orth_from_previous: false,
                sink: true,
                failed: false,
            }
        );
        assert!(fsa.proceed_to_next(6, mid, false).unwrap().is_none());
        assert!(fsa.proceed_to_next(99, initial, false).unwrap().is_none());
    }

    #[test]
    fn rejects_failed_segmentation_state_transition() {
        let bytes = segmentation_rules_fsa_bytes();
        let fsa = SegmentationRulesFsa::new(&bytes).unwrap();

        assert!(matches!(
            fsa.proceed_to_next(4, SegmentationState::failed(), true),
            Err(Error::InvalidArgument(message)) if message.contains("failed")
        ));
    }

    #[test]
    fn rejects_segmentation_fsa_transition_past_data() {
        let bytes = [0, 1, 4, 0, 0, 10];

        assert!(matches!(
            SegmentationRulesFsa::new(&bytes),
            Err(Error::InvalidDictionary(message)) if message.contains("target state")
        ));
    }

    #[test]
    fn binary_generator_lexicon_integrates_with_engine_generate() {
        let lexicon =
            BinaryGeneratorLexicon::from_bytes(binary_generator_dictionary_bytes()).unwrap();
        let engine = Engine::builder().lexicon(lexicon).build();

        let generated = engine.generate("kot").unwrap();
        let unknown = engine.generate("pies").unwrap();

        assert_eq!(generated.len(), 1);
        assert_eq!(generated[0].orth, "kota");
        assert_eq!(generated[0].lemma, "kot");
        assert_eq!(generated[0].tag(engine.resolver()), Some("subst:sg:nom:m1"));
        assert_eq!(generated[0].name(engine.resolver()), Some("wlasna"));
        assert_eq!(
            generated[0].labels_as_string(engine.resolver()),
            Some("a|b")
        );
        assert_eq!(unknown[0].tag(engine.resolver()), Some("ign"));
    }

    #[test]
    fn binary_generator_filters_requested_homonym_id() {
        let lexicon =
            BinaryGeneratorLexicon::from_bytes(binary_generator_dictionary_with_homonyms_bytes())
                .unwrap();
        let engine = Engine::builder().lexicon(lexicon).build();

        let all = engine.generate("kot").unwrap();
        let s1 = engine.generate("kot:s1").unwrap();
        let unknown = engine.generate("kot:s3").unwrap();

        assert_eq!(all.len(), 2);
        assert_eq!(all[0].orth, "kota");
        assert_eq!(all[0].lemma, "kot:s1");
        assert_eq!(all[1].orth, "kotu");
        assert_eq!(all[1].lemma, "kot:s2");
        assert_eq!(s1.len(), 1);
        assert_eq!(s1[0].orth, "kota");
        assert_eq!(s1[0].lemma, "kot:s1");
        assert_eq!(unknown.len(), 1);
        assert_eq!(unknown[0].orth, "kot:s3");
        assert_eq!(unknown[0].tag(engine.resolver()), Some("ign"));
    }

    #[test]
    fn binary_analyzer_lexicon_integrates_with_engine_analyze() {
        let lexicon =
            BinaryAnalyzerLexicon::from_bytes(binary_analyzer_dictionary_bytes()).unwrap();
        let engine = Engine::builder().lexicon(lexicon).build();

        let analyzed = engine.analyze("Kot pies").unwrap();

        assert_eq!(analyzed.len(), 2);
        assert_eq!(analyzed[0].orth, "Kot");
        assert_eq!(analyzed[0].lemma, "kot");
        assert_eq!(analyzed[0].tag(engine.resolver()), Some("subst:sg:nom:m1"));
        assert_eq!(analyzed[0].name(engine.resolver()), Some("wlasna"));
        assert_eq!(analyzed[0].labels_as_string(engine.resolver()), Some("a|b"));
        assert_eq!(analyzed[1].orth, "pies");
        assert_eq!(analyzed[1].tag(engine.resolver()), Some("ign"));
    }

    #[test]
    fn binary_analyzer_uses_conditional_case_pattern_fallback() {
        let lexicon =
            BinaryAnalyzerLexicon::from_bytes(binary_titlecase_analyzer_dictionary_bytes())
                .unwrap();
        let engine = Engine::builder().lexicon(lexicon).build();

        let title = engine.analyze("Kot").unwrap();
        let lower = engine.analyze("kot").unwrap();

        assert_eq!(title.len(), 1);
        assert_eq!(title[0].tag(engine.resolver()), Some("subst:sg:nom:m1"));
        assert_eq!(lower.len(), 1);
        assert_eq!(lower[0].tag(engine.resolver()), Some("subst:sg:nom:m1"));
    }

    #[test]
    fn binary_lexicons_expose_encoded_groups() {
        let analyzer =
            BinaryAnalyzerLexicon::from_bytes(binary_analyzer_dictionary_bytes()).unwrap();
        let generator =
            BinaryGeneratorLexicon::from_bytes(binary_generator_dictionary_bytes()).unwrap();

        let analyzer_groups = analyzer.lookup_encoded_groups("Kot").unwrap().unwrap();
        let generator_groups = generator.synthesize_encoded_groups("kot").unwrap();

        assert_eq!(analyzer_groups[0].segment_type, 4);
        assert_eq!(analyzer_groups[0].interpretations[0].tag_id, 42);
        assert_eq!(generator_groups[0].segment_type, 4);
        assert_eq!(generator_groups[0].interpretations[0].tag_id, 42);
    }

    #[test]
    fn binary_analyzer_uses_default_segmentation_rules_for_word_graph() {
        let lexicon =
            BinaryAnalyzerLexicon::from_bytes(binary_segmented_analyzer_dictionary_bytes())
                .unwrap();
        let engine = Engine::builder().lexicon(lexicon).build();

        let analyzed = engine.analyze("ab").unwrap();

        assert_eq!(analyzed.len(), 2);
        assert_eq!(analyzed[0].start_node, 0);
        assert_eq!(analyzed[0].end_node, 1);
        assert_eq!(analyzed[0].orth, "a");
        assert_eq!(analyzed[0].lemma, "a");
        assert_eq!(analyzed[1].start_node, 1);
        assert_eq!(analyzed[1].end_node, 2);
        assert_eq!(analyzed[1].orth, "b");
        assert_eq!(analyzed[1].lemma, "b");
        assert_eq!(analyzed[1].tag(engine.resolver()), Some("subst:sg:nom:m1"));
    }

    #[test]
    fn binary_analyzer_applies_shift_orth_segmentation() {
        let lexicon =
            BinaryAnalyzerLexicon::from_bytes(binary_shifted_analyzer_dictionary_bytes()).unwrap();
        let engine = Engine::builder().lexicon(lexicon).build();

        let analyzed = engine.analyze("ab").unwrap();

        assert_eq!(analyzed.len(), 1);
        assert_eq!(analyzed[0].start_node, 0);
        assert_eq!(analyzed[0].end_node, 1);
        assert_eq!(analyzed[0].orth, "ab");
        assert_eq!(analyzed[0].lemma, "ab");
        assert_eq!(analyzed[0].tag(engine.resolver()), Some("subst:sg:nom:m1"));
    }

    fn minimal_dictionary_bytes() -> Vec<u8> {
        let fsa = [0xaa, 0xbb];
        let mut metadata = Vec::new();
        metadata.extend_from_slice(b"test-dict\0copyright\0");

        let mut bytes = Vec::new();
        bytes.extend_from_slice(&MAGIC_NUMBER.to_be_bytes());
        bytes.push(VERSION_NUM);
        bytes.push(2);
        bytes.extend_from_slice(&(fsa.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&fsa);
        bytes.extend_from_slice(&(metadata.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&metadata);
        bytes.extend_from_slice(&segmentation_metadata_bytes());
        bytes
    }

    fn dictionary_bytes_with_id_resolver() -> Vec<u8> {
        let fsa = [0xaa, 0xbb];
        dictionary_bytes_with_fsa(&fsa)
    }

    fn binary_generator_dictionary_bytes() -> Vec<u8> {
        let payload = generator_morph_payload(&[4]);
        let mut fsa = Vec::new();
        fsa.extend_from_slice(&[b'k', V2_LAST_FLAG]);
        fsa.extend_from_slice(&[b'o', V2_LAST_FLAG]);
        fsa.extend_from_slice(&[b't', V2_LAST_FLAG | V2_ACCEPTING_FLAG]);
        fsa.extend_from_slice(&payload);

        dictionary_bytes_with_fsa(&fsa)
    }

    fn binary_generator_dictionary_with_homonyms_bytes() -> Vec<u8> {
        let payload = generator_morph_payload_with_homonyms(&[4]);
        let mut fsa = Vec::new();
        fsa.extend_from_slice(&[b'k', V2_LAST_FLAG]);
        fsa.extend_from_slice(&[b'o', V2_LAST_FLAG]);
        fsa.extend_from_slice(&[b't', V2_LAST_FLAG | V2_ACCEPTING_FLAG]);
        fsa.extend_from_slice(&payload);

        dictionary_bytes_with_fsa(&fsa)
    }

    fn binary_analyzer_dictionary_bytes() -> Vec<u8> {
        let payload = analyzer_morph_payload(&[4]);
        let mut fsa = Vec::new();
        fsa.extend_from_slice(&[b'k', V2_LAST_FLAG]);
        fsa.extend_from_slice(&[b'o', V2_LAST_FLAG]);
        fsa.extend_from_slice(&[b't', V2_LAST_FLAG | V2_ACCEPTING_FLAG]);
        fsa.extend_from_slice(&payload);

        dictionary_bytes_with_fsa(&fsa)
    }

    fn binary_titlecase_analyzer_dictionary_bytes() -> Vec<u8> {
        let payload = analyzer_morph_payload_with_compression(ORTH_ONLY_TITLE | LEMMA_ONLY_LOWER);
        let mut fsa = Vec::new();
        fsa.extend_from_slice(&[b'k', V2_LAST_FLAG]);
        fsa.extend_from_slice(&[b'o', V2_LAST_FLAG]);
        fsa.extend_from_slice(&[b't', V2_LAST_FLAG | V2_ACCEPTING_FLAG]);
        fsa.extend_from_slice(&payload);

        dictionary_bytes_with_fsa(&fsa)
    }

    fn binary_segmented_analyzer_dictionary_bytes() -> Vec<u8> {
        let fsa = one_byte_accepting_fsa(&[
            (b'a', analyzer_morph_payload(&[4])),
            (b'b', analyzer_morph_payload(&[4])),
        ]);
        dictionary_bytes_with_fsa_and_segmentation(&fsa, &two_segment_rules_fsa())
    }

    fn binary_shifted_analyzer_dictionary_bytes() -> Vec<u8> {
        let fsa = one_byte_accepting_fsa(&[
            (b'a', analyzer_morph_payload(&[4])),
            (b'b', analyzer_morph_payload(&[4])),
        ]);
        dictionary_bytes_with_fsa_and_segmentation(&fsa, &shifted_two_segment_rules_fsa())
    }

    fn analyzer_groups_payload(segment_types: &[u8]) -> Vec<u8> {
        let encoded_interp = vec![ORTH_ONLY_LOWER | LEMMA_ONLY_LOWER, 0, 0, 0, 42, 7, 0, 9];
        interps_groups_bytes(segment_types, &encoded_interp)
    }

    fn generator_groups_payload(segment_types: &[u8]) -> Vec<u8> {
        let mut encoded_interp = Vec::new();
        encoded_interp.push(0);
        encoded_interp.push(0);
        encoded_interp.push(0);
        encoded_interp.extend_from_slice(b"a\0");
        encoded_interp.extend_from_slice(&42_u16.to_be_bytes());
        encoded_interp.push(7);
        encoded_interp.extend_from_slice(&9_u16.to_be_bytes());
        interps_groups_bytes(segment_types, &encoded_interp)
    }

    fn analyzer_morph_payload(segment_types: &[u8]) -> Vec<u8> {
        let encoded_interp = analyzer_interp_with_compression(ORTH_ONLY_LOWER | LEMMA_ONLY_LOWER);
        morph_payload(segment_types, &encoded_interp)
    }

    fn analyzer_morph_payload_with_compression(compression: u8) -> Vec<u8> {
        let encoded_interp = analyzer_interp_with_compression(compression);
        morph_payload(&[4], &encoded_interp)
    }

    fn analyzer_interp_with_compression(compression: u8) -> Vec<u8> {
        vec![compression, 0, 0, 0, 42, 7, 0, 9]
    }

    fn generator_morph_payload(segment_types: &[u8]) -> Vec<u8> {
        let mut encoded_interp = Vec::new();
        encoded_interp.push(0);
        encoded_interp.push(0);
        encoded_interp.push(0);
        encoded_interp.extend_from_slice(b"a\0");
        encoded_interp.extend_from_slice(&42_u16.to_be_bytes());
        encoded_interp.push(7);
        encoded_interp.extend_from_slice(&9_u16.to_be_bytes());
        morph_payload(segment_types, &encoded_interp)
    }

    fn generator_morph_payload_with_homonyms(segment_types: &[u8]) -> Vec<u8> {
        let mut encoded_interps = Vec::new();
        encoded_interps.extend_from_slice(generator_interp_record("s1", "a", 42).as_slice());
        encoded_interps.extend_from_slice(generator_interp_record("s2", "u", 42).as_slice());
        morph_payload(segment_types, &encoded_interps)
    }

    fn generator_interp_record(homonym_id: &str, suffix_to_add: &str, tag_id: u16) -> Vec<u8> {
        let mut encoded_interp = Vec::new();
        encoded_interp.extend_from_slice(homonym_id.as_bytes());
        encoded_interp.push(0);
        encoded_interp.push(0);
        encoded_interp.push(0);
        encoded_interp.extend_from_slice(suffix_to_add.as_bytes());
        encoded_interp.push(0);
        encoded_interp.extend_from_slice(&tag_id.to_be_bytes());
        encoded_interp.push(7);
        encoded_interp.extend_from_slice(&9_u16.to_be_bytes());
        encoded_interp
    }

    fn morph_payload(segment_types: &[u8], encoded_interp: &[u8]) -> Vec<u8> {
        let groups = interps_groups_bytes(segment_types, encoded_interp);
        let mut payload = Vec::new();
        payload.extend_from_slice(&(groups.len() as u16).to_be_bytes());
        payload.extend_from_slice(&groups);
        payload
    }

    fn interps_groups_bytes(segment_types: &[u8], encoded_interp: &[u8]) -> Vec<u8> {
        let mut payload = Vec::new();
        for segment_type in segment_types {
            payload.push(*segment_type);
            payload.extend_from_slice(&(encoded_interp.len() as u16).to_be_bytes());
            payload.extend_from_slice(encoded_interp);
        }
        payload
    }

    fn dictionary_bytes_with_fsa(fsa: &[u8]) -> Vec<u8> {
        dictionary_bytes_with_fsa_and_segmentation(fsa, &default_rules_fsa())
    }

    fn dictionary_bytes_with_fsa_and_segmentation(fsa: &[u8], rules_fsa: &[u8]) -> Vec<u8> {
        let mut metadata = Vec::new();
        metadata.extend_from_slice(b"test-dict\0copyright\0");
        metadata.extend_from_slice(b"test-tagset\0");
        push_id_string_table(&mut metadata, &[(0, "ign"), (42, "subst:sg:nom:m1")]);
        push_id_string_table(&mut metadata, &[(0, "_"), (7, "wlasna")]);
        push_id_string_table(&mut metadata, &[(0, "_"), (9, "a|b")]);

        let mut bytes = Vec::new();
        bytes.extend_from_slice(&MAGIC_NUMBER.to_be_bytes());
        bytes.push(VERSION_NUM);
        bytes.push(2);
        bytes.extend_from_slice(&(fsa.len() as u32).to_be_bytes());
        bytes.extend_from_slice(fsa);
        bytes.extend_from_slice(&(metadata.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&metadata);
        bytes.extend_from_slice(&segmentation_metadata_bytes_with_fsa(rules_fsa));
        bytes
    }

    fn segmentation_metadata_bytes() -> Vec<u8> {
        segmentation_metadata_bytes_with_fsa(&default_rules_fsa())
    }

    fn segmentation_metadata_bytes_with_fsa(rules_fsa: &[u8]) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&2_u16.to_be_bytes());
        data.extend_from_slice(&44_u32.to_be_bytes());
        data.extend_from_slice(&46_u32.to_be_bytes());
        data.push(1);
        push_options_map(&mut data, &[("aggl", "permissive"), ("praet", "split")]);
        data.extend_from_slice(&(rules_fsa.len() as u32).to_be_bytes());
        data.extend_from_slice(rules_fsa);
        push_options_map(&mut data, &[("aggl", "permissive"), ("praet", "split")]);
        data
    }

    fn default_rules_fsa() -> Vec<u8> {
        vec![1, 0]
    }

    fn segmentation_rules_fsa_bytes() -> Vec<u8> {
        vec![
            0, 2, 4, 0, 0, 10, 5, 1, 0, 12, 1, 0, 0, 1, 6, 0, 0, 18, 3, 0,
        ]
    }

    fn two_segment_rules_fsa() -> Vec<u8> {
        vec![0, 1, 4, 0, 0, 6, 0, 1, 4, 0, 0, 12, 1, 0]
    }

    fn shifted_two_segment_rules_fsa() -> Vec<u8> {
        vec![0, 1, 4, 1, 0, 6, 0, 1, 4, 0, 0, 12, 1, 0]
    }

    fn one_byte_accepting_fsa(entries: &[(u8, Vec<u8>)]) -> Vec<u8> {
        let transitions_len = entries.len() * 2;
        let mut target_offsets = Vec::with_capacity(entries.len());
        let mut next_target_offset = transitions_len;
        for (_, payload) in entries {
            target_offsets.push(next_target_offset);
            next_target_offset += payload.len() + 2;
        }

        let mut fsa = Vec::new();
        for (index, (label, _)) in entries.iter().enumerate() {
            let transition_offset = index * 2;
            let relative_offset = target_offsets[index] - (transition_offset + 2);
            assert!(relative_offset <= V2_FIRST_BYTE_OFFSET_MASK as usize);
            let mut flags = V2_ACCEPTING_FLAG | relative_offset as u8;
            if index + 1 == entries.len() {
                flags |= V2_LAST_FLAG;
            }
            fsa.push(*label);
            fsa.push(flags);
        }

        for (_, payload) in entries {
            fsa.extend_from_slice(payload);
            fsa.push(0);
            fsa.push(V2_LAST_FLAG);
        }

        fsa
    }

    fn vlength1_fsa_data(states: &[u8]) -> Vec<u8> {
        let mut data = vec![0; V1_INITIAL_STATE_OFFSET];
        data[V1_INITIAL_STATE_OFFSET - 1] = b'^';
        data.extend_from_slice(states);
        data
    }

    fn push_options_map(out: &mut Vec<u8>, options: &[(&str, &str)]) {
        out.push(options.len() as u8);
        for (key, value) in options {
            out.extend_from_slice(key.as_bytes());
            out.push(0);
            out.extend_from_slice(value.as_bytes());
            out.push(0);
        }
    }

    fn options_map(options: &[(&str, &str)]) -> BTreeMap<String, String> {
        options
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect()
    }

    fn push_id_string_table(out: &mut Vec<u8>, entries: &[(u16, &str)]) {
        out.extend_from_slice(&(entries.len() as u16).to_be_bytes());
        for (id, value) in entries {
            out.extend_from_slice(&id.to_be_bytes());
            out.extend_from_slice(value.as_bytes());
            out.push(0);
        }
    }
}
