use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::fs;
use std::path::Path;

pub const MAX_QUALIFIERS_COMBINATIONS: usize = 2048;
pub const MAX_TAGS: usize = 65_535;
pub const MAX_NAMES: usize = 255;
pub const MAX_SEGMENT_TYPES: usize = 255;
pub const MAX_SEGMENT_RULES_FSA_SIZE: usize = 65_535;
pub const MAGIC_NUMBER: u32 = 0x8fc2_bc1b;
pub const DICTIONARY_VERSION: u8 = 21;

const ORTH_ONLY_LOWER: u8 = 128;
const ORTH_ONLY_TITLE: u8 = 64;
const LEMMA_ONLY_LOWER: u8 = 32;
const LEMMA_ONLY_TITLE: u8 = 16;
const PREFIX_CUT_MASK: u8 = 15;
const CASE_PATTERN_ONLY_LOWER: u8 = 0;
const CASE_PATTERN_UPPER_PREFIX: u8 = 1;
const CASE_PATTERN_MIXED: u8 = 2;

pub type QualifierSet = BTreeSet<String>;

pub trait TagsetLookup {
    fn tag_num(&self, tag: &str) -> Result<usize>;
}

pub trait TagsetRulesLookup: TagsetLookup {
    fn all_tags(&self) -> &[String];
}

pub trait SortEncoder {
    type Key: Ord;

    fn word_sort_key(&self, word: &str) -> Self::Key;
}

pub trait WordBytesEncoder {
    fn encode_word_bytes(&self, word: &str) -> Vec<u8>;
}

pub trait SegmentRulesLookup {
    fn lexeme_to_segment_type_num(
        &self,
        base: &str,
        tag_num: usize,
        name_num: usize,
        qualifiers_num: usize,
    ) -> Result<usize>;

    fn should_replace_lemma_with_orth(&self, _segment_type_num: usize) -> bool {
        false
    }

    fn new_segment_type_for_shift_orth(&self, _segment_type_num: usize) -> Option<usize> {
        None
    }
}

pub trait SegmentTypeLookup {
    fn segment_type_num(&self, segment_type: &str) -> Result<usize>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct IdentityEncoder;

impl SortEncoder for IdentityEncoder {
    type Key = String;

    fn word_sort_key(&self, word: &str) -> Self::Key {
        word.to_owned()
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Utf8WordEncoder;

impl WordBytesEncoder for Utf8WordEncoder {
    fn encode_word_bytes(&self, word: &str) -> Vec<u8> {
        word.as_bytes().to_vec()
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Utf8AnalyzerEncoder;

impl SortEncoder for Utf8AnalyzerEncoder {
    type Key = String;

    fn word_sort_key(&self, word: &str) -> Self::Key {
        word.to_lowercase()
    }
}

impl WordBytesEncoder for Utf8AnalyzerEncoder {
    fn encode_word_bytes(&self, word: &str) -> Vec<u8> {
        word.as_bytes().to_vec()
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Utf8GeneratorEncoder;

impl SortEncoder for Utf8GeneratorEncoder {
    type Key = String;

    fn word_sort_key(&self, word: &str) -> Self::Key {
        word.to_owned()
    }
}

impl WordBytesEncoder for Utf8GeneratorEncoder {
    fn encode_word_bytes(&self, word: &str) -> Vec<u8> {
        word.as_bytes().to_vec()
    }
}

impl TagsetLookup for BTreeMap<String, usize> {
    fn tag_num(&self, tag: &str) -> Result<usize> {
        self.get(tag)
            .copied()
            .ok_or_else(|| BuilderError::new(format!("unknown tag: {tag}")))
    }
}

impl SegmentTypeLookup for BTreeMap<String, usize> {
    fn segment_type_num(&self, segment_type: &str) -> Result<usize> {
        self.get(segment_type)
            .copied()
            .ok_or_else(|| BuilderError::new(format!("unknown segment type: {segment_type}")))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tagset {
    pub tagset_id: Option<String>,
    pub tag_to_num: BTreeMap<String, usize>,
    num_to_tag: BTreeMap<usize, String>,
    tags_in_order: Vec<String>,
}

impl Tagset {
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self> {
        let contents = fs::read_to_string(path.as_ref()).map_err(|err| {
            BuilderError::new(format!(
                "failed to read tagset file {}: {err}",
                path.as_ref().display()
            ))
        })?;
        Self::from_str(path.as_ref().display().to_string(), &contents)
    }

    pub fn from_str(input_name: impl AsRef<str>, input: &str) -> Result<Self> {
        let mut tagset_id = None;
        let mut tag_to_num = BTreeMap::new();
        let mut tags_in_order = Vec::new();
        let mut inside_tags = false;

        for (line_index, raw_line) in python_file_lines(input).enumerate() {
            let line_number = line_index + 1;
            if line_number == 1 {
                let Some(id) = parse_tagset_id_line(&raw_line) else {
                    return Err(BuilderError::new(
                        "missing TAGSET-ID in first line of tagset file",
                    ));
                };
                tagset_id = Some(id);
            } else if raw_line == "[TAGS]" {
                inside_tags = true;
            } else if !raw_line.is_empty() && !raw_line.starts_with('#') {
                validate(
                    inside_tags,
                    format!(
                        "\"{}\" - text outside [TAGS] section in tagset file line {line_number}",
                        raw_line
                    ),
                )?;
                let fields: Vec<&str> = raw_line.split('\t').collect();
                validate(
                    fields.len() == 2,
                    format!("\"{}\" - invalid line {line_number}", raw_line),
                )?;
                let tag_num = fields[0].parse::<usize>().map_err(|err| {
                    BuilderError::new(format!(
                        "{}:{} - invalid tag id \"{}\": {err}",
                        input_name.as_ref(),
                        line_number,
                        fields[0]
                    ))
                })?;
                let tag = fields[1];

                validate(
                    !tag_to_num.contains_key(tag),
                    format!("duplicate tag: \"{tag}\""),
                )?;
                validate(
                    !tag_to_num.values().any(|existing| *existing == tag_num),
                    format!(
                        "line {line_number}: tagId {tag_num} assigned for tag \"{tag}\" already appeared somewhere else."
                    ),
                )?;

                tag_to_num.insert(tag.to_owned(), tag_num);
                tags_in_order.push(tag.to_owned());
            }
        }

        let num_to_tag = tag_to_num
            .iter()
            .map(|(tag, tag_num)| (*tag_num, tag.clone()))
            .collect();

        Ok(Self {
            tagset_id,
            tag_to_num,
            num_to_tag,
            tags_in_order,
        })
    }

    pub fn all_tags(&self) -> &[String] {
        &self.tags_in_order
    }

    pub fn tag_num_for_tag(&self, tag: &str) -> Result<usize> {
        self.tag_to_num
            .get(tag)
            .copied()
            .ok_or_else(|| BuilderError::new(format!("invalid tag: \"{tag}\"")))
    }

    pub fn tag_for_tag_num(&self, tag_num: usize) -> Result<&str> {
        self.num_to_tag
            .get(&tag_num)
            .map(String::as_str)
            .ok_or_else(|| BuilderError::new(format!("invalid tag id: {tag_num}")))
    }
}

impl TagsetLookup for Tagset {
    fn tag_num(&self, tag: &str) -> Result<usize> {
        self.tag_num_for_tag(tag)
    }
}

impl TagsetRulesLookup for Tagset {
    fn all_tags(&self) -> &[String] {
        self.all_tags()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DictionaryMetadata {
    pub dict_id: String,
    pub copyright: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamesAndQualifiers {
    pub names: BTreeMap<String, usize>,
    pub qualifiers: BTreeMap<QualifierSet, usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DictionarySource<'a> {
    pub name: &'a str,
    pub input: &'a str,
}

impl<'a> DictionarySource<'a> {
    pub fn new(name: &'a str, input: &'a str) -> Self {
        Self { name, input }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedFormWithoutPrefix {
    pub cut_length: usize,
    pub suffix_to_add: String,
    pub case_pattern: Vec<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedAnalyzerForm {
    pub prefix_cut_length: usize,
    pub cut_length: usize,
    pub suffix_to_add: String,
    pub case_pattern: Vec<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedGeneratorForm {
    pub cut_length: usize,
    pub suffix_to_add: String,
    pub prefix_to_add: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalyzerInterpretation {
    pub encoded_form: EncodedAnalyzerForm,
    pub orth_case_pattern: Vec<bool>,
    pub tag_num: usize,
    pub name_num: usize,
    pub type_num: usize,
    pub qualifiers: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct AnalyzerInterpretationSortKey {
    pub cut_length: usize,
    pub prefix_cut_length: usize,
    pub suffix_to_add: Vec<char>,
    pub case_pattern: Vec<bool>,
    pub orth_case_pattern: Vec<bool>,
    pub tag_num: usize,
    pub name_num: usize,
    pub type_num: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalyzerEntry {
    pub key: String,
    pub interpretations: Vec<AnalyzerInterpretation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratorInterpretation {
    pub lemma: String,
    pub encoded_form: EncodedGeneratorForm,
    pub tag_num: usize,
    pub name_num: usize,
    pub type_num: usize,
    pub homonym_id: String,
    pub qualifiers: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct GeneratorInterpretationSortKey {
    pub homonym_id: String,
    pub tag_num: usize,
    pub cut_length: usize,
    pub suffix_to_add: Vec<char>,
    pub name_num: usize,
    pub type_num: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratorEntry {
    pub key: String,
    pub interpretations: Vec<GeneratorInterpretation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimpleTransition {
    pub label: u8,
    pub target_offset: usize,
    pub transition_data: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SimpleState {
    pub encoded_data: Option<Vec<u8>>,
    pub transitions: Vec<SimpleTransition>,
    pub label_frequencies: BTreeMap<u8, usize>,
}

impl SimpleState {
    pub fn accepting(encoded_data: impl Into<Vec<u8>>) -> Self {
        Self {
            encoded_data: Some(encoded_data.into()),
            transitions: Vec::new(),
            label_frequencies: BTreeMap::new(),
        }
    }

    pub fn non_accepting() -> Self {
        Self::default()
    }

    pub fn with_transition(
        mut self,
        label: u8,
        target_offset: usize,
        transition_data: Option<u8>,
    ) -> Self {
        self.transitions.push(SimpleTransition {
            label,
            target_offset,
            transition_data,
        });
        self
    }

    pub fn with_label_frequency(mut self, label: u8, frequency: usize) -> Self {
        self.label_frequencies.insert(label, frequency);
        self
    }

    pub fn is_accepting(&self) -> bool {
        self.encoded_data.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimpleGraphTransition {
    pub label: u8,
    pub target: usize,
    pub transition_data: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SimpleGraphState {
    pub encoded_data: Option<Vec<u8>>,
    pub transitions: Vec<SimpleGraphTransition>,
    pub frequency: usize,
    pub label_frequencies: BTreeMap<u8, usize>,
}

impl SimpleGraphState {
    pub fn accepting(encoded_data: impl Into<Vec<u8>>) -> Self {
        Self {
            encoded_data: Some(encoded_data.into()),
            transitions: Vec::new(),
            frequency: 0,
            label_frequencies: BTreeMap::new(),
        }
    }

    pub fn non_accepting() -> Self {
        Self::default()
    }

    pub fn with_frequency(mut self, frequency: usize) -> Self {
        self.frequency = frequency;
        self
    }

    pub fn with_transition(
        mut self,
        label: u8,
        target: usize,
        transition_data: Option<u8>,
    ) -> Self {
        self.transitions.push(SimpleGraphTransition {
            label,
            target,
            transition_data,
        });
        self
    }

    pub fn with_label_frequency(mut self, label: u8, frequency: usize) -> Self {
        self.label_frequencies.insert(label, frequency);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimpleFsaGraph {
    pub states: Vec<SimpleGraphState>,
    pub initial_state: usize,
    pub global_label_frequencies: BTreeMap<u8, usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentRule {
    kind: SegmentRuleKind,
    line_number: usize,
    weak: bool,
    autogenerated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SegmentRuleKind {
    Tag {
        segment_type_num: usize,
        shift_orth: bool,
        segment_type: String,
    },
    Concat(Vec<SegmentRule>),
    Or(Vec<SegmentRule>),
    ZeroOrMore(Box<SegmentRule>),
    Optional(Box<SegmentRule>),
    Sink,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SegmentRulesTransitionLabel {
    pub segment_type_num: u8,
    pub shift_orth: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentRulesState {
    pub accepting: bool,
    pub weak: bool,
    pub transitions: BTreeMap<SegmentRulesTransitionLabel, usize>,
    transition_order: Vec<SegmentRulesTransitionLabel>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentRulesFsa {
    pub states: Vec<SegmentRulesState>,
    pub initial_state: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentRulesFsaVariantData {
    pub options: BTreeMap<String, String>,
    pub fsa: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentRulesTarget {
    Analyzer,
    Generator,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedSegmentRules {
    pub segment_types: Vec<String>,
    pub segment_type_resolver: Option<SegmentTypeResolver>,
    pub separators: Vec<u32>,
    pub variants: Vec<SegmentRulesFsaVariantData>,
    pub default_options: BTreeMap<String, String>,
    pub segmentation_rules_data: Vec<u8>,
    pub replace_lemma_with_orth: BTreeSet<usize>,
    pub shift_orth_extra_segment_types: BTreeMap<usize, usize>,
    pub additional_segment_type_names: BTreeMap<usize, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentTypeResolver {
    pub segment_types: Vec<String>,
    segment_type_to_num: BTreeMap<String, usize>,
    segment_nums: BTreeMap<(Option<String>, usize), Vec<SegmentTypeAssignment>>,
    replace_lemma_with_orth: BTreeSet<usize>,
    shift_orth_extra_segment_types: BTreeMap<usize, usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SegmentTypeAssignment {
    homonym: Option<String>,
    name_num: Option<usize>,
    labels_num: usize,
    segment_type_num: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SegmentTypePattern {
    lemma: Option<String>,
    homonym: Option<String>,
    pattern: String,
    name: String,
    labels: QualifierSet,
    segment_type_num: usize,
}

impl SegmentRule {
    fn tag(
        segment_type_num: usize,
        shift_orth: bool,
        segment_type: impl Into<String>,
        line_number: usize,
    ) -> Self {
        Self {
            kind: SegmentRuleKind::Tag {
                segment_type_num,
                shift_orth,
                segment_type: segment_type.into(),
            },
            line_number,
            weak: false,
            autogenerated: false,
        }
    }

    fn concat(children: Vec<Self>, line_number: usize) -> Self {
        debug_assert!(!children.is_empty());
        Self {
            kind: SegmentRuleKind::Concat(children),
            line_number,
            weak: false,
            autogenerated: false,
        }
    }

    fn or(children: Vec<Self>, line_number: usize) -> Self {
        debug_assert!(!children.is_empty());
        Self {
            kind: SegmentRuleKind::Or(children),
            line_number,
            weak: false,
            autogenerated: false,
        }
    }

    fn zero_or_more(child: Self, line_number: usize) -> Self {
        Self {
            kind: SegmentRuleKind::ZeroOrMore(Box::new(child)),
            line_number,
            weak: false,
            autogenerated: false,
        }
    }

    fn optional(child: Self, line_number: usize) -> Self {
        Self {
            kind: SegmentRuleKind::Optional(Box::new(child)),
            line_number,
            weak: false,
            autogenerated: false,
        }
    }

    fn sink() -> Self {
        Self {
            kind: SegmentRuleKind::Sink,
            line_number: 0,
            weak: false,
            autogenerated: false,
        }
    }

    pub fn is_weak(&self) -> bool {
        self.weak
    }

    pub fn set_weak(mut self, weak: bool) -> Self {
        self.weak = weak;
        self
    }

    pub fn line_number(&self) -> usize {
        self.line_number
    }

    pub fn is_sink_rule(&self) -> bool {
        matches!(self.kind, SegmentRuleKind::Sink)
    }

    pub fn allows_empty_sequence(&self) -> bool {
        match &self.kind {
            SegmentRuleKind::Tag { .. } => false,
            SegmentRuleKind::Concat(children) => children.iter().all(Self::allows_empty_sequence),
            SegmentRuleKind::Or(children) => children.iter().any(Self::allows_empty_sequence),
            SegmentRuleKind::ZeroOrMore(_) | SegmentRuleKind::Optional(_) => true,
            SegmentRuleKind::Sink => false,
        }
    }

    pub fn is_shift_orth_rule(&self) -> bool {
        match &self.kind {
            SegmentRuleKind::Tag { shift_orth, .. } => *shift_orth,
            SegmentRuleKind::Concat(children) | SegmentRuleKind::Or(children) => {
                children.iter().all(Self::is_shift_orth_rule)
            }
            SegmentRuleKind::ZeroOrMore(child) | SegmentRuleKind::Optional(child) => {
                child.is_shift_orth_rule()
            }
            SegmentRuleKind::Sink => false,
        }
    }

    pub fn make_shift_orth_rule(&mut self) {
        match &mut self.kind {
            SegmentRuleKind::Tag { shift_orth, .. } => *shift_orth = true,
            SegmentRuleKind::Concat(children) | SegmentRuleKind::Or(children) => {
                for child in children {
                    child.make_shift_orth_rule();
                }
            }
            SegmentRuleKind::ZeroOrMore(child) | SegmentRuleKind::Optional(child) => {
                child.make_shift_orth_rule();
            }
            SegmentRuleKind::Sink => {}
        }
    }

    pub fn atomic_rules(&self) -> Vec<&SegmentRule> {
        let mut result = Vec::new();
        self.push_atomic_rules(&mut result);
        result
    }

    fn push_atomic_rules<'a>(&'a self, result: &mut Vec<&'a SegmentRule>) {
        match &self.kind {
            SegmentRuleKind::Tag { .. } => result.push(self),
            SegmentRuleKind::Concat(children) | SegmentRuleKind::Or(children) => {
                for child in children {
                    child.push_atomic_rules(result);
                }
            }
            SegmentRuleKind::ZeroOrMore(child) | SegmentRuleKind::Optional(child) => {
                child.push_atomic_rules(result);
            }
            SegmentRuleKind::Sink => {}
        }
    }

    pub fn transform_to_generator_version(&self) -> Self {
        match &self.kind {
            SegmentRuleKind::Tag { .. } => self.clone(),
            SegmentRuleKind::ZeroOrMore(_) => {
                if self.is_shift_orth_rule() {
                    self.clone()
                } else {
                    Self::sink()
                }
            }
            SegmentRuleKind::Optional(child) => {
                if self.is_shift_orth_rule() {
                    self.clone()
                } else {
                    child.transform_to_generator_version()
                }
            }
            SegmentRuleKind::Or(children) => {
                let new_children: Vec<Self> = children
                    .iter()
                    .filter(|child| !child.allows_empty_sequence() || child.is_shift_orth_rule())
                    .map(Self::transform_to_generator_version)
                    .filter(|child| !child.is_sink_rule())
                    .collect();
                if new_children.is_empty() {
                    Self::sink()
                } else {
                    Self::or(new_children, self.line_number).set_weak(self.weak)
                }
            }
            SegmentRuleKind::Concat(children) => {
                let new_children: Vec<Self> = children
                    .iter()
                    .filter(|child| !child.allows_empty_sequence() || child.is_shift_orth_rule())
                    .map(Self::transform_to_generator_version)
                    .collect();
                if new_children.is_empty() {
                    return Self::sink();
                }
                let mut has_non_optional_non_shifting_rule = false;
                for child in &new_children {
                    if child.is_sink_rule() || has_non_optional_non_shifting_rule {
                        return Self::sink();
                    } else if !child.is_shift_orth_rule() {
                        has_non_optional_non_shifting_rule = true;
                    }
                }
                Self::concat(new_children, self.line_number).set_weak(self.weak)
            }
            SegmentRuleKind::Sink => Self::sink(),
        }
    }

    pub fn additional_atomic_rules_for_generator(&self) -> Vec<Self> {
        match &self.kind {
            SegmentRuleKind::Tag { .. } => {
                let mut rule = self.clone();
                rule.autogenerated = true;
                vec![rule]
            }
            SegmentRuleKind::ZeroOrMore(child) | SegmentRuleKind::Optional(child) => {
                child.additional_atomic_rules_for_generator()
            }
            SegmentRuleKind::Or(children) => children
                .iter()
                .flat_map(Self::additional_atomic_rules_for_generator)
                .collect(),
            SegmentRuleKind::Concat(children) => {
                let mut result = Vec::new();
                let mut current_shift_orth_rule: Option<Self> = None;
                for child in children {
                    if child.is_shift_orth_rule() {
                        current_shift_orth_rule =
                            Some(if let Some(current) = current_shift_orth_rule {
                                Self::concat(vec![current, child.clone()], child.line_number)
                            } else {
                                child.clone()
                            });
                    } else {
                        for atomic_rule in child.additional_atomic_rules_for_generator() {
                            if let Some(current) = &current_shift_orth_rule {
                                result.push(Self::concat(
                                    vec![current.clone(), atomic_rule],
                                    child.line_number,
                                ));
                            } else {
                                result.push(atomic_rule);
                            }
                        }
                        current_shift_orth_rule = None;
                    }
                }
                for rule in &mut result {
                    rule.autogenerated = true;
                }
                result
            }
            SegmentRuleKind::Sink => Vec::new(),
        }
    }

    pub fn validate_segment_rule(&self, input_name: &str) -> Result<()> {
        match &self.kind {
            SegmentRuleKind::Concat(children) => {
                for child in children {
                    child.validate_segment_rule(input_name)?;
                }
                if children.last().is_some_and(Self::is_shift_orth_rule)
                    && !children.iter().all(Self::is_shift_orth_rule)
                {
                    return Err(BuilderError::new(format!(
                        "{input_name}:{} - If the rightmost subrule of concatenation \"{}\" is with \">\", than all subrules must be with \">\"",
                        self.line_number, self
                    )));
                }
                Ok(())
            }
            SegmentRuleKind::Or(children) => {
                for child in children {
                    child.validate_segment_rule(input_name)?;
                }
                let all_shift = children.iter().all(Self::is_shift_orth_rule);
                let any_shift = children.iter().any(Self::is_shift_orth_rule);
                if !(all_shift || !any_shift) {
                    return Err(BuilderError::new(format!(
                        "{input_name}:{} - All subrules of alternative \"{}\" must be either with or without \">\"",
                        self.line_number, self
                    )));
                }
                Ok(())
            }
            SegmentRuleKind::ZeroOrMore(child) | SegmentRuleKind::Optional(child) => {
                child.validate_segment_rule(input_name)
            }
            SegmentRuleKind::Tag { .. } | SegmentRuleKind::Sink => Ok(()),
        }
    }

    fn add_to_segment_rules_nfa(&self, nfa: &mut SegmentRulesNfa) -> Result<()> {
        if self.is_sink_rule() {
            return Ok(());
        }
        let end_state = nfa.add_state(SegmentRulesNfaState::final_for_rule(self));
        self.add_between_segment_rules_nfa_states(nfa, nfa.initial_state, end_state)
    }

    fn add_between_segment_rules_nfa_states(
        &self,
        nfa: &mut SegmentRulesNfa,
        start_state: usize,
        end_state: usize,
    ) -> Result<()> {
        match &self.kind {
            SegmentRuleKind::Tag {
                segment_type_num,
                shift_orth,
                ..
            } => {
                let segment_type_num = u8::try_from(*segment_type_num).map_err(|_| {
                    BuilderError::new(format!(
                        "segment type number {segment_type_num} does not fit into uint8"
                    ))
                })?;
                nfa.add_transition(
                    start_state,
                    Some(SegmentRulesTransitionLabel {
                        segment_type_num,
                        shift_orth: *shift_orth,
                    }),
                    end_state,
                );
                Ok(())
            }
            SegmentRuleKind::Concat(children) => {
                let mut current_start = start_state;
                for child in &children[..children.len() - 1] {
                    let current_end = nfa.add_state(SegmentRulesNfaState::for_rule(self));
                    child.add_between_segment_rules_nfa_states(nfa, current_start, current_end)?;
                    let next_start = nfa.add_state(SegmentRulesNfaState::for_rule(self));
                    nfa.add_transition(current_end, None, next_start);
                    current_start = next_start;
                }
                children
                    .last()
                    .expect("concat is never empty")
                    .add_between_segment_rules_nfa_states(nfa, current_start, end_state)
            }
            SegmentRuleKind::Or(children) => {
                for child in children {
                    let intermediate_start = nfa.add_state(SegmentRulesNfaState::for_rule(self));
                    let intermediate_end = nfa.add_state(SegmentRulesNfaState::for_rule(self));
                    nfa.add_transition(start_state, None, intermediate_start);
                    child.add_between_segment_rules_nfa_states(
                        nfa,
                        intermediate_start,
                        intermediate_end,
                    )?;
                    nfa.add_transition(intermediate_end, None, end_state);
                }
                Ok(())
            }
            SegmentRuleKind::ZeroOrMore(child) => {
                let intermediate_start = nfa.add_state(SegmentRulesNfaState::for_rule(self));
                let intermediate_end = nfa.add_state(SegmentRulesNfaState::for_rule(self));
                nfa.add_transition(start_state, None, intermediate_start);
                nfa.add_transition(start_state, None, end_state);
                child.add_between_segment_rules_nfa_states(
                    nfa,
                    intermediate_start,
                    intermediate_end,
                )?;
                nfa.add_transition(intermediate_end, None, end_state);
                nfa.add_transition(intermediate_end, None, intermediate_start);
                Ok(())
            }
            SegmentRuleKind::Optional(child) => {
                let intermediate_start = nfa.add_state(SegmentRulesNfaState::for_rule(self));
                let intermediate_end = nfa.add_state(SegmentRulesNfaState::for_rule(self));
                nfa.add_transition(start_state, None, intermediate_start);
                nfa.add_transition(start_state, None, end_state);
                child.add_between_segment_rules_nfa_states(
                    nfa,
                    intermediate_start,
                    intermediate_end,
                )?;
                nfa.add_transition(intermediate_end, None, end_state);
                Ok(())
            }
            SegmentRuleKind::Sink => Ok(()),
        }
    }

    fn collect_atomic_rule_info(&self, atoms: &mut Vec<SegmentRuleAtom>) {
        match &self.kind {
            SegmentRuleKind::Tag {
                segment_type_num,
                shift_orth,
                segment_type,
            } => atoms.push(SegmentRuleAtom {
                segment_type_num: *segment_type_num,
                shift_orth: *shift_orth,
                segment_type: segment_type.clone(),
            }),
            SegmentRuleKind::Concat(children) | SegmentRuleKind::Or(children) => {
                for child in children {
                    child.collect_atomic_rule_info(atoms);
                }
            }
            SegmentRuleKind::ZeroOrMore(child) | SegmentRuleKind::Optional(child) => {
                child.collect_atomic_rule_info(atoms);
            }
            SegmentRuleKind::Sink => {}
        }
    }

    fn remap_shift_orth_segment_types(&mut self, shift_orth_map: &BTreeMap<usize, usize>) {
        match &mut self.kind {
            SegmentRuleKind::Tag {
                segment_type_num,
                shift_orth,
                ..
            } => {
                if *shift_orth {
                    if let Some(new_segment_type_num) = shift_orth_map.get(segment_type_num) {
                        *segment_type_num = *new_segment_type_num;
                    }
                }
            }
            SegmentRuleKind::Concat(children) | SegmentRuleKind::Or(children) => {
                for child in children {
                    child.remap_shift_orth_segment_types(shift_orth_map);
                }
            }
            SegmentRuleKind::ZeroOrMore(child) | SegmentRuleKind::Optional(child) => {
                child.remap_shift_orth_segment_types(shift_orth_map);
            }
            SegmentRuleKind::Sink => {}
        }
    }
}

impl fmt::Display for SegmentRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            SegmentRuleKind::Tag {
                segment_type,
                shift_orth,
                ..
            } => {
                f.write_str(segment_type)?;
                if *shift_orth {
                    f.write_str(">")?;
                }
                Ok(())
            }
            SegmentRuleKind::Concat(children) => {
                for (index, child) in children.iter().enumerate() {
                    if index > 0 {
                        f.write_str(" ")?;
                    }
                    write!(f, "{child}")?;
                }
                Ok(())
            }
            SegmentRuleKind::Or(children) => {
                for (index, child) in children.iter().enumerate() {
                    if index > 0 {
                        f.write_str(" | ")?;
                    }
                    write!(f, "{child}")?;
                }
                Ok(())
            }
            SegmentRuleKind::ZeroOrMore(child) => write!(f, "({child})*"),
            SegmentRuleKind::Optional(child) => write!(f, "({child})?"),
            SegmentRuleKind::Sink => f.write_str("<<REMOVED>>"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimpleGraphLayout {
    pub dfs_order: Vec<usize>,
    pub offsets: Vec<usize>,
    pub reverse_offsets: Vec<usize>,
    pub total_size: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SimpleBuildState {
    encoded_data: Option<Vec<u8>>,
    transitions: Vec<(u8, usize)>,
}

impl SimpleBuildState {
    fn new() -> Self {
        Self {
            encoded_data: None,
            transitions: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SimpleBuildStateKey {
    transitions: BTreeMap<u8, usize>,
    encoded_data: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
struct SortedSimpleFsaBuilder {
    states: Vec<SimpleBuildState>,
    register: BTreeMap<SimpleBuildStateKey, usize>,
    initial_state: usize,
    previous_word: Option<Vec<u8>>,
    entries_num: usize,
    global_label_frequencies: BTreeMap<u8, usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuilderError {
    message: String,
}

impl BuilderError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for BuilderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for BuilderError {}

pub type Result<T> = std::result::Result<T, BuilderError>;

pub fn read_metadata_from_str(input_name: &str, input: &str) -> Result<DictionaryMetadata> {
    merge_metadata([(input_name, input)])
}

pub fn read_names_and_qualifiers_from_str(
    input_name: &str,
    input: &str,
) -> Result<NamesAndQualifiers> {
    merge_names_and_qualifiers([(input_name, input)])
}

pub fn merge_metadata<I, N, T>(inputs: I) -> Result<DictionaryMetadata>
where
    I: IntoIterator<Item = (N, T)>,
    N: AsRef<str>,
    T: AsRef<str>,
{
    let mut dict_id = None;
    let mut copyright = None;

    for (input_name, input) in inputs {
        let input_name = input_name.as_ref();
        let mut in_copyright = false;

        for (line_index, line) in input.as_ref().split_inclusive('\n').enumerate() {
            let line_number = line_index + 1;

            if dict_id.is_none() && line.starts_with("#!DICT-ID") {
                let stripped = line.trim();
                let (dict_id_tag, value) = partition_once_space(stripped);
                let space_separated_parts = line.split(' ').count();
                validate(
                    dict_id_tag == "#!DICT-ID",
                    "Dictionary ID tag must be followed by a space character and dictionary ID string",
                )?;
                validate(
                    space_separated_parts > 1,
                    format!("{input_name}:{line_number}: Must provide DICT-ID"),
                )?;
                validate(
                    space_separated_parts == 2,
                    format!("{input_name}:{line_number}: DICT-ID must not contain spaces"),
                )?;
                dict_id = Some(value.to_owned());
            } else if copyright.is_none() && line.starts_with("#<COPYRIGHT>") {
                validate(
                    line.trim() == "#<COPYRIGHT>",
                    format!(
                        "{input_name}:{line_number}: Copyright start tag must be the only one in the line"
                    ),
                )?;
                in_copyright = true;
                copyright = Some(String::new());
            } else if line.starts_with("#</COPYRIGHT>") {
                validate(
                    in_copyright,
                    format!(
                        "{input_name}:{line_number}: Copyright end tag must be preceded by copyright start tag"
                    ),
                )?;
                validate(
                    line.trim() == "#</COPYRIGHT>",
                    format!(
                        "{input_name}:{line_number}: Copyright end tag must be the only one in the line"
                    ),
                )?;
                in_copyright = false;
            } else if in_copyright {
                if let Some(copyright) = copyright.as_mut() {
                    copyright.push_str(line);
                }
            }
        }
    }

    Ok(DictionaryMetadata {
        dict_id: dict_id.unwrap_or_default(),
        copyright: copyright.unwrap_or_default(),
    })
}

pub fn merge_names_and_qualifiers<I, N, T>(inputs: I) -> Result<NamesAndQualifiers>
where
    I: IntoIterator<Item = (N, T)>,
    N: AsRef<str>,
    T: AsRef<str>,
{
    let mut names = BTreeSet::from([String::new()]);
    let mut qualifiers = BTreeSet::from([QualifierSet::new()]);
    let mut line_parser = LineParser::new();

    for (_input_name, input) in inputs {
        for raw_line in input.as_ref().lines() {
            let line = raw_line.trim();
            if !line_parser.ignore_line(line) {
                let parsed = line_parser.parse_line(line)?;
                names.insert(parsed.name);
                qualifiers.insert(parse_qualifiers(&parsed.qualifier));
            }
        }
    }

    validate(
        qualifiers.len() <= MAX_QUALIFIERS_COMBINATIONS,
        format!("Too many qualifiers combinations. The limit is {MAX_QUALIFIERS_COMBINATIONS}"),
    )?;

    Ok(NamesAndQualifiers {
        names: index_btree_set(names),
        qualifiers: index_btree_set(qualifiers),
    })
}

pub fn parse_qualifiers(input: &str) -> QualifierSet {
    if input.is_empty() {
        QualifierSet::new()
    } else {
        input.split('|').map(str::to_owned).collect()
    }
}

pub fn preprocess_segment_rules<I, L, D>(
    input_lines: I,
    active_definitions: D,
    input_name: &str,
) -> Result<Vec<(usize, String)>>
where
    I: IntoIterator<Item = (usize, L)>,
    L: AsRef<str>,
    D: IntoIterator,
    D::Item: AsRef<str>,
{
    let mut defines = BTreeMap::new();
    let active_definitions: BTreeSet<String> = active_definitions
        .into_iter()
        .map(|definition| definition.as_ref().to_owned())
        .collect();
    let mut ifdefs_stack: Vec<(String, bool)> = Vec::new();
    let mut output = Vec::new();

    for (line_number, raw_line) in input_lines {
        let line = raw_line.as_ref();
        if line.starts_with("#define") {
            let parsed = parse_segment_rule_define(line, line_number, input_name)?;
            match parsed {
                SegmentRuleDefine::WithoutArg { name, value } => {
                    defines.insert(name.clone(), SegmentRuleDefineValue::WithoutArg(value));
                }
                SegmentRuleDefine::WithArg { name, arg, value } => {
                    defines.insert(name, SegmentRuleDefineValue::WithArg { arg, value });
                }
            }
        } else if line.starts_with("#ifdef") {
            let name = parse_segment_rule_ifdef(line, line_number, input_name)?;
            ifdefs_stack.push((name, true));
        } else if line.starts_with("#else") {
            let Some((name, is_active)) = ifdefs_stack.last_mut() else {
                return Err(BuilderError::new(format!(
                    "{input_name}:{line_number}: #else without #ifdef"
                )));
            };
            validate(
                *is_active,
                format!("{input_name}:{line_number}: repeated #else for #ifdef {name}"),
            )?;
            *is_active = false;
        } else if line.starts_with("#endif") {
            if ifdefs_stack.pop().is_none() {
                return Err(BuilderError::new(format!(
                    "{input_name}:{line_number}: #endif without #ifdef"
                )));
            }
        } else if line.starts_with('#') {
            output.push((line_number, line.to_owned()));
        } else if segment_rule_ifdefs_active(&ifdefs_stack, &active_definitions) {
            output.push((
                line_number,
                process_segment_rule_line(line_number, line, &defines, input_name)?,
            ));
        }
    }

    validate(
        ifdefs_stack.is_empty(),
        format!("{input_name}: unterminated #ifdef in segmentation rules"),
    )?;
    Ok(output)
}

pub fn parse_segment_rule_line<T>(
    line_number: usize,
    line: &str,
    segment_types: &T,
    input_name: &str,
) -> Result<SegmentRule>
where
    T: SegmentTypeLookup,
{
    SegmentRuleParser::new(line_number, line, segment_types, input_name).parse_complete_rule()
}

pub fn build_segment_rules_fsa<'a, I>(rules: I, input_name: &str) -> Result<SegmentRulesFsa>
where
    I: IntoIterator<Item = &'a SegmentRule>,
{
    let mut nfa = SegmentRulesNfa::new();
    for rule in rules {
        rule.add_to_segment_rules_nfa(&mut nfa)?;
    }
    nfa.convert_to_dfa(input_name)
}

pub fn serialize_segment_rules_fsa<'a, I>(rules: I, input_name: &str) -> Result<Vec<u8>>
where
    I: IntoIterator<Item = &'a SegmentRule>,
{
    let fsa = build_segment_rules_fsa(rules, input_name)?;
    serialize_segment_rules_fsa_data(&fsa)
}

pub fn serialize_u16_be(value: usize) -> Result<[u8; 2]> {
    validate_u16(value)?;
    Ok((value as u16).to_be_bytes())
}

pub fn serialize_u32_be(value: usize) -> Result<[u8; 4]> {
    validate_u32(value)?;
    Ok((value as u32).to_be_bytes())
}

pub fn serialize_legacy_string(value: &str) -> Vec<u8> {
    let mut out = value.as_bytes().to_vec();
    out.push(0);
    out
}

pub fn serialize_prologue(implementation_code: u8) -> Vec<u8> {
    let mut out = Vec::with_capacity(6);
    out.extend(MAGIC_NUMBER.to_be_bytes());
    out.push(DICTIONARY_VERSION);
    out.push(implementation_code);
    out
}

pub fn serialize_tags_map<E>(tags_map: &BTreeMap<String, usize>, encoder: &E) -> Result<Vec<u8>>
where
    E: WordBytesEncoder,
{
    let mut records: Vec<(&str, usize)> = tags_map
        .iter()
        .map(|(tag, tag_num)| (tag.as_str(), *tag_num))
        .collect();
    records.sort_by_key(|(_tag, tag_num)| *tag_num);
    serialize_tag_records(records, encoder)
}

pub fn serialize_qualifiers_map<E>(
    qualifiers_map: &BTreeMap<QualifierSet, usize>,
    encoder: &E,
) -> Result<Vec<u8>>
where
    E: WordBytesEncoder,
{
    let mut records: Vec<(String, usize)> = qualifiers_map
        .iter()
        .map(|(qualifiers, id)| {
            (
                qualifiers
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>()
                    .join("|"),
                *id,
            )
        })
        .collect();
    records.sort_by_key(|(_label, id)| *id);
    serialize_owned_tag_records(records, encoder)
}

pub fn serialize_tagset_data<E>(
    tagset: &Tagset,
    names_map: &BTreeMap<String, usize>,
    encoder: &E,
) -> Result<Vec<u8>>
where
    E: WordBytesEncoder,
{
    let tagset_id = tagset
        .tagset_id
        .as_deref()
        .ok_or_else(|| BuilderError::new("missing tagset id"))?;
    let mut out = serialize_legacy_string(tagset_id);
    out.extend(serialize_tags_map(&tagset.tag_to_num, encoder)?);
    out.extend(serialize_tags_map(names_map, encoder)?);
    Ok(out)
}

pub fn serialize_epilogue(
    dict_id: &str,
    copyright: &str,
    tagset_data: &[u8],
    qualifiers_data: &[u8],
    segmentation_rules_data: &[u8],
) -> Result<Vec<u8>> {
    let mut id_and_copyright = serialize_legacy_string(dict_id);
    id_and_copyright.extend(serialize_legacy_string(copyright));
    let segrules_offset = tagset_data.len() + qualifiers_data.len() + id_and_copyright.len();

    let mut out = Vec::new();
    out.extend(serialize_u32_be(segrules_offset)?);
    out.extend(id_and_copyright);
    out.extend(tagset_data);
    out.extend(qualifiers_data);
    out.extend(segmentation_rules_data);
    Ok(out)
}

pub fn simple_implementation_code(serialize_transition_data: bool) -> u8 {
    if serialize_transition_data {
        128
    } else {
        0
    }
}

pub fn simple_state_size(state: &SimpleState, serialize_transition_data: bool) -> Result<usize> {
    Ok(
        1 + if serialize_transition_data { 5 } else { 4 } * state.transitions.len()
            + if state.is_accepting() {
                state
                    .encoded_data
                    .as_ref()
                    .map(Vec::len)
                    .unwrap_or_default()
            } else {
                0
            },
    )
}

pub fn serialize_simple_state_data(state: &SimpleState) -> Result<Vec<u8>> {
    validate(
        state.transitions.len() <= 127,
        format!(
            "simple state has too many transitions: {}",
            state.transitions.len()
        ),
    )?;

    let mut first_byte = state.transitions.len();
    if state.is_accepting() {
        first_byte |= 128;
    }
    validate(
        first_byte > 0 && first_byte < 256,
        "simple state must be accepting or have transitions",
    )?;

    let mut out = vec![first_byte as u8];
    if let Some(encoded_data) = &state.encoded_data {
        out.extend(encoded_data);
    }
    Ok(out)
}

pub fn serialize_simple_transitions(
    state: &SimpleState,
    serialize_transition_data: bool,
    global_label_frequencies: &BTreeMap<u8, usize>,
) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    for transition in sorted_simple_transitions(state, global_label_frequencies) {
        validate(
            transition.target_offset < 256 * 256 * 256,
            format!(
                "simple transition offset {} exceeds 24-bit limit",
                transition.target_offset
            ),
        )?;
        out.push(transition.label);
        out.push(((transition.target_offset & 0xFF0000) >> 16) as u8);
        out.push(((transition.target_offset & 0x00FF00) >> 8) as u8);
        out.push((transition.target_offset & 0x0000FF) as u8);
        if serialize_transition_data {
            out.push(transition.transition_data.ok_or_else(|| {
                BuilderError::new(format!(
                    "missing transition data for label {}",
                    transition.label
                ))
            })?);
        }
    }
    Ok(out)
}

pub fn serialize_simple_state(
    state: &SimpleState,
    serialize_transition_data: bool,
    global_label_frequencies: &BTreeMap<u8, usize>,
) -> Result<Vec<u8>> {
    let mut out = serialize_simple_state_data(state)?;
    out.extend(serialize_simple_transitions(
        state,
        serialize_transition_data,
        global_label_frequencies,
    )?);
    Ok(out)
}

pub fn calculate_simple_graph_layout(
    graph: &SimpleFsaGraph,
    serialize_transition_data: bool,
) -> Result<SimpleGraphLayout> {
    validate_simple_graph(graph)?;
    let dfs_order = simple_graph_dfs_order(graph)?;
    let mut reverse_offsets = vec![0; graph.states.len()];
    let mut current_reverse_offset = 0usize;

    for &state_index in &dfs_order {
        current_reverse_offset +=
            simple_graph_state_size(&graph.states[state_index], serialize_transition_data)?;
        reverse_offsets[state_index] = current_reverse_offset;
    }

    let total_size = current_reverse_offset;
    let mut offsets = vec![0; graph.states.len()];
    for &state_index in &dfs_order {
        offsets[state_index] = total_size - reverse_offsets[state_index];
    }

    Ok(SimpleGraphLayout {
        dfs_order,
        offsets,
        reverse_offsets,
        total_size,
    })
}

pub fn serialize_simple_fsa_data(
    graph: &SimpleFsaGraph,
    serialize_transition_data: bool,
) -> Result<Vec<u8>> {
    let layout = calculate_simple_graph_layout(graph, serialize_transition_data)?;
    let mut ordered_states = layout.dfs_order.clone();
    ordered_states.sort_by_key(|state_index| layout.offsets[*state_index]);

    let mut out = Vec::with_capacity(layout.total_size);
    for state_index in ordered_states {
        let state = simple_state_for_graph_state(graph, &layout, state_index);
        out.extend(serialize_simple_state(
            &state,
            serialize_transition_data,
            &graph.global_label_frequencies,
        )?);
    }
    Ok(out)
}

pub fn calculate_segment_rules_fsa_layout(fsa: &SegmentRulesFsa) -> Result<SegmentRulesFsaLayout> {
    validate_segment_rules_fsa(fsa)?;
    let dfs_order = segment_rules_fsa_dfs_order(fsa)?;
    let mut reverse_offsets = vec![0; fsa.states.len()];
    let mut current_reverse_offset = 0usize;

    for &state_index in &dfs_order {
        current_reverse_offset += segment_rules_state_size(&fsa.states[state_index])?;
        reverse_offsets[state_index] = current_reverse_offset;
    }

    let total_size = current_reverse_offset;
    let mut offsets = vec![0; fsa.states.len()];
    for &state_index in &dfs_order {
        offsets[state_index] = total_size - reverse_offsets[state_index];
    }

    Ok(SegmentRulesFsaLayout {
        dfs_order,
        offsets,
        reverse_offsets,
        total_size,
    })
}

pub fn serialize_segment_rules_fsa_data(fsa: &SegmentRulesFsa) -> Result<Vec<u8>> {
    let layout = calculate_segment_rules_fsa_layout(fsa)?;
    validate(
        layout.total_size <= MAX_SEGMENT_RULES_FSA_SIZE,
        format!(
            "Segmentation rules are too big and complicated- the resulting automaton would exceed its max size which is {}",
            MAX_SEGMENT_RULES_FSA_SIZE
        ),
    )?;

    let mut ordered_states = layout.dfs_order.clone();
    ordered_states.sort_by_key(|state_index| layout.offsets[*state_index]);

    let mut out = Vec::with_capacity(layout.total_size);
    for state_index in ordered_states {
        out.extend(serialize_segment_rules_state(
            &fsa.states[state_index],
            &layout,
        )?);
    }
    Ok(out)
}

pub fn serialize_segmentation_rules_data<I>(
    separators: I,
    variants: &[SegmentRulesFsaVariantData],
    default_options: &BTreeMap<String, String>,
) -> Result<Vec<u8>>
where
    I: IntoIterator<Item = u32>,
{
    validate(
        !variants.is_empty() && variants.len() < 256,
        "Too many segmentation rules variants",
    )?;

    let mut out = Vec::new();
    let mut separators: Vec<u32> = separators.into_iter().collect();
    separators.sort_unstable();
    out.extend(serialize_u16_be(separators.len())?);
    for separator in separators {
        out.extend(serialize_u32_be(separator as usize)?);
    }

    out.push(variants.len() as u8);
    for variant in variants {
        out.extend(serialize_segment_rules_options_map(&variant.options)?);
        out.extend(serialize_u32_be(variant.fsa.len())?);
        out.extend(&variant.fsa);
    }
    out.extend(serialize_segment_rules_options_map(default_options)?);
    Ok(out)
}

pub fn parse_segmentation_rules_from_str(
    input_name: &str,
    input: &str,
    target: SegmentRulesTarget,
) -> Result<ParsedSegmentRules> {
    let config = SegmentRulesConfigFile::parse(
        input_name,
        input,
        [
            "options",
            "combinations",
            "tags",
            "lexemes",
            "segment types",
            "separator chars",
        ],
    )?;
    let segment_types = parse_segment_types_section(&config)?;
    parse_segmentation_rules_config(input_name, &config, target, segment_types, None)
}

pub fn parse_segmentation_rules_with_tagset_from_str<T>(
    input_name: &str,
    input: &str,
    target: SegmentRulesTarget,
    tagset: &T,
    names_map: &BTreeMap<String, usize>,
    labels_map: &BTreeMap<QualifierSet, usize>,
) -> Result<ParsedSegmentRules>
where
    T: TagsetRulesLookup,
{
    let config = SegmentRulesConfigFile::parse(
        input_name,
        input,
        [
            "options",
            "combinations",
            "tags",
            "lexemes",
            "segment types",
            "separator chars",
        ],
    )?;
    let segment_types = parse_segment_types_section(&config)?;
    let segment_type_resolver = SegmentTypeResolver::from_config(
        &config,
        segment_types.clone(),
        tagset,
        names_map,
        labels_map,
    )?;
    parse_segmentation_rules_config(
        input_name,
        &config,
        target,
        segment_types,
        Some(segment_type_resolver),
    )
}

fn parse_segmentation_rules_config(
    input_name: &str,
    config: &SegmentRulesConfigFile,
    target: SegmentRulesTarget,
    segment_types: Vec<String>,
    segment_type_resolver: Option<SegmentTypeResolver>,
) -> Result<ParsedSegmentRules> {
    let option_definitions = parse_segment_rules_options(config)?;
    let segment_type_nums: BTreeMap<String, usize> = segment_types
        .iter()
        .enumerate()
        .map(|(index, segment_type)| (segment_type.clone(), index))
        .collect();
    let separators = if target == SegmentRulesTarget::Analyzer {
        parse_separator_chars_section(&config)?
    } else {
        Vec::new()
    };

    let definitions_to_option_keys = definitions_to_option_keys(&option_definitions);
    let option_combinations = segment_rules_option_combinations(&option_definitions);
    let combination_lines = config.lines_in_section("combinations", false)?;
    let mut rules_by_options = Vec::new();

    for active_definitions in option_combinations {
        let options =
            active_definitions_to_options(&active_definitions, &definitions_to_option_keys)?;
        let preprocessed = preprocess_segment_rules(
            combination_lines.iter().cloned(),
            &active_definitions,
            input_name,
        )?;
        let mut rules = Vec::new();
        for (line_number, line) in preprocessed {
            if line.starts_with('#') {
                continue;
            }
            let mut rule =
                parse_segment_rule_line(line_number, &line, &segment_type_nums, input_name)?;
            if rule.allows_empty_sequence() {
                return Err(BuilderError::new(format!(
                    "{input_name}:{line_number} - This rule allows empty segments sequence to be accepted"
                )));
            }
            rule.validate_segment_rule(input_name)?;
            if target == SegmentRulesTarget::Generator {
                let mut additional_rules = rule.additional_atomic_rules_for_generator();
                for additional_rule in &mut additional_rules {
                    additional_rule.autogenerated = true;
                }
                rules.extend(additional_rules);
                rule = rule.transform_to_generator_version();
            }
            if !rule.is_sink_rule() {
                rules.push(rule);
            }
        }
        rules_by_options.push((options, rules));
    }

    let shift_magic = apply_shift_orth_magic(&segment_types, &mut rules_by_options);
    let mut variants = Vec::with_capacity(rules_by_options.len());
    for (options, rules) in &rules_by_options {
        variants.push(SegmentRulesFsaVariantData {
            options: options.clone(),
            fsa: serialize_segment_rules_fsa(rules.iter(), input_name)?,
        });
    }
    let default_options = variants
        .first()
        .map(|variant| variant.options.clone())
        .ok_or_else(|| BuilderError::new("Too many segmentation rules variants"))?;
    let segmentation_rules_data =
        serialize_segmentation_rules_data(separators.iter().copied(), &variants, &default_options)?;

    Ok(ParsedSegmentRules {
        segment_types,
        segment_type_resolver: segment_type_resolver.map(|resolver| {
            resolver.with_shift_magic(
                shift_magic.replace_lemma_with_orth.clone(),
                shift_magic.shift_orth_extra_segment_types.clone(),
            )
        }),
        separators,
        variants,
        default_options,
        segmentation_rules_data,
        replace_lemma_with_orth: shift_magic.replace_lemma_with_orth,
        shift_orth_extra_segment_types: shift_magic.shift_orth_extra_segment_types,
        additional_segment_type_names: shift_magic.additional_segment_type_names,
    })
}

pub fn serialize_simple_dictionary<E>(
    graph: &SimpleFsaGraph,
    serialize_transition_data: bool,
    dict_id: &str,
    copyright: &str,
    tagset: &Tagset,
    names_map: &BTreeMap<String, usize>,
    qualifiers_map: &BTreeMap<QualifierSet, usize>,
    segmentation_rules_data: &[u8],
    encoder: &E,
) -> Result<Vec<u8>>
where
    E: WordBytesEncoder,
{
    let tagset_data = serialize_tagset_data(tagset, names_map, encoder)?;
    let qualifiers_data = serialize_qualifiers_map(qualifiers_map, encoder)?;
    let fsa_data = serialize_simple_fsa_data(graph, serialize_transition_data)?;

    let mut out = serialize_prologue(simple_implementation_code(serialize_transition_data));
    out.extend(serialize_u32_be(fsa_data.len())?);
    out.extend(fsa_data);
    out.extend(serialize_epilogue(
        dict_id,
        copyright,
        &tagset_data,
        &qualifiers_data,
        segmentation_rules_data,
    )?);
    Ok(out)
}

pub fn build_simple_fsa_from_sorted_entries<I, W, D>(entries: I) -> Result<SimpleFsaGraph>
where
    I: IntoIterator<Item = (W, D)>,
    W: AsRef<[u8]>,
    D: AsRef<[u8]>,
{
    let mut builder = SortedSimpleFsaBuilder::new();
    for (word, data) in entries {
        builder.add_entry(word.as_ref(), data.as_ref())?;
    }
    builder.close()
}

pub fn serialize_analyzer_entry_payload(entry: &AnalyzerEntry) -> Result<Vec<u8>> {
    validate(
        !entry.interpretations.is_empty(),
        "analyzer entry must have interpretations",
    )?;

    let mut groups: BTreeMap<usize, Vec<&AnalyzerInterpretation>> = BTreeMap::new();
    for interpretation in &entry.interpretations {
        groups
            .entry(interpretation.type_num)
            .or_default()
            .push(interpretation);
    }

    let mut groups_payload = Vec::new();
    for (type_num, interpretations) in groups {
        groups_payload.extend(serialize_analyzer_interpretations_group(
            type_num,
            &interpretations,
        )?);
    }

    let mut out = Vec::new();
    out.extend(serialize_u16_be(groups_payload.len())?);
    out.extend(groups_payload);
    Ok(out)
}

pub fn serialize_generator_entry_payload(entry: &GeneratorEntry) -> Result<Vec<u8>> {
    validate(
        !entry.interpretations.is_empty(),
        "generator entry must have interpretations",
    )?;

    let mut groups: BTreeMap<usize, Vec<&GeneratorInterpretation>> = BTreeMap::new();
    for interpretation in &entry.interpretations {
        groups
            .entry(interpretation.type_num)
            .or_default()
            .push(interpretation);
    }

    let mut groups_payload = Vec::new();
    for (type_num, interpretations) in groups {
        groups_payload.extend(serialize_generator_interpretations_group(
            type_num,
            &interpretations,
        )?);
    }

    let mut out = Vec::new();
    out.extend(serialize_u16_be(groups_payload.len())?);
    out.extend(groups_payload);
    Ok(out)
}

pub fn analyzer_entries_to_sorted_fsa_entries<E>(
    entries: &[AnalyzerEntry],
    encoder: &E,
) -> Result<Vec<(Vec<u8>, Vec<u8>)>>
where
    E: WordBytesEncoder,
{
    entries
        .iter()
        .map(|entry| {
            Ok((
                encoder.encode_word_bytes(&entry.key),
                serialize_analyzer_entry_payload(entry)?,
            ))
        })
        .collect()
}

pub fn generator_entries_to_sorted_fsa_entries<E>(
    entries: &[GeneratorEntry],
    encoder: &E,
) -> Result<Vec<(Vec<u8>, Vec<u8>)>>
where
    E: WordBytesEncoder,
{
    entries
        .iter()
        .map(|entry| {
            Ok((
                encoder.encode_word_bytes(&entry.key),
                serialize_generator_entry_payload(entry)?,
            ))
        })
        .collect()
}

pub fn build_analyzer_simple_fsa_from_entries<E>(
    entries: &[AnalyzerEntry],
    encoder: &E,
) -> Result<SimpleFsaGraph>
where
    E: WordBytesEncoder,
{
    build_simple_fsa_from_sorted_entries(analyzer_entries_to_sorted_fsa_entries(entries, encoder)?)
}

pub fn build_generator_simple_fsa_from_entries<E>(
    entries: &[GeneratorEntry],
    encoder: &E,
) -> Result<SimpleFsaGraph>
where
    E: WordBytesEncoder,
{
    build_simple_fsa_from_sorted_entries(generator_entries_to_sorted_fsa_entries(entries, encoder)?)
}

pub fn build_analyzer_simple_dictionary_from_entries<E>(
    entries: &[AnalyzerEntry],
    dict_id: &str,
    copyright: &str,
    tagset: &Tagset,
    names_map: &BTreeMap<String, usize>,
    qualifiers_map: &BTreeMap<QualifierSet, usize>,
    segmentation_rules_data: &[u8],
    encoder: &E,
) -> Result<Vec<u8>>
where
    E: WordBytesEncoder,
{
    let graph = build_analyzer_simple_fsa_from_entries(entries, encoder)?;
    serialize_simple_dictionary(
        &graph,
        false,
        dict_id,
        copyright,
        tagset,
        names_map,
        qualifiers_map,
        segmentation_rules_data,
        encoder,
    )
}

pub fn build_generator_simple_dictionary_from_entries<E>(
    entries: &[GeneratorEntry],
    dict_id: &str,
    copyright: &str,
    tagset: &Tagset,
    names_map: &BTreeMap<String, usize>,
    qualifiers_map: &BTreeMap<QualifierSet, usize>,
    segmentation_rules_data: &[u8],
    encoder: &E,
) -> Result<Vec<u8>>
where
    E: WordBytesEncoder,
{
    let graph = build_generator_simple_fsa_from_entries(entries, encoder)?;
    serialize_simple_dictionary(
        &graph,
        false,
        dict_id,
        copyright,
        tagset,
        names_map,
        qualifiers_map,
        segmentation_rules_data,
        encoder,
    )
}

pub fn build_analyzer_simple_dictionary_from_str(
    input_name: &str,
    input: &str,
    tagset_name: &str,
    tagset_input: &str,
    segment_rules_name: &str,
    segment_rules_input: &str,
) -> Result<Vec<u8>> {
    build_analyzer_simple_dictionary_from_sources(
        [DictionarySource::new(input_name, input)],
        tagset_name,
        tagset_input,
        segment_rules_name,
        segment_rules_input,
    )
}

pub fn build_generator_simple_dictionary_from_str(
    input_name: &str,
    input: &str,
    tagset_name: &str,
    tagset_input: &str,
    segment_rules_name: &str,
    segment_rules_input: &str,
) -> Result<Vec<u8>> {
    build_generator_simple_dictionary_from_sources(
        [DictionarySource::new(input_name, input)],
        tagset_name,
        tagset_input,
        segment_rules_name,
        segment_rules_input,
    )
}

pub fn build_analyzer_simple_dictionary_from_sources<'a, I>(
    dictionary_sources: I,
    tagset_name: &str,
    tagset_input: &str,
    segment_rules_name: &str,
    segment_rules_input: &str,
) -> Result<Vec<u8>>
where
    I: IntoIterator<Item = DictionarySource<'a>>,
{
    let sources = collect_dictionary_sources(dictionary_sources)?;
    let (metadata, names_and_qualifiers, tagset, parsed_rules) = parse_source_dictionary_context(
        &sources,
        tagset_name,
        tagset_input,
        segment_rules_name,
        segment_rules_input,
        SegmentRulesTarget::Analyzer,
    )?;
    let encoder = Utf8AnalyzerEncoder;
    let entries = convert_polimorf_for_analyzer(
        source_lines(&sources),
        &tagset,
        &names_and_qualifiers.names,
        &names_and_qualifiers.qualifiers,
        &encoder,
        &parsed_rules,
    )?;

    build_analyzer_simple_dictionary_from_entries(
        &entries,
        &metadata.dict_id,
        &metadata.copyright,
        &tagset,
        &names_and_qualifiers.names,
        &names_and_qualifiers.qualifiers,
        &parsed_rules.segmentation_rules_data,
        &encoder,
    )
}

pub fn build_generator_simple_dictionary_from_sources<'a, I>(
    dictionary_sources: I,
    tagset_name: &str,
    tagset_input: &str,
    segment_rules_name: &str,
    segment_rules_input: &str,
) -> Result<Vec<u8>>
where
    I: IntoIterator<Item = DictionarySource<'a>>,
{
    let sources = collect_dictionary_sources(dictionary_sources)?;
    let (metadata, names_and_qualifiers, tagset, parsed_rules) = parse_source_dictionary_context(
        &sources,
        tagset_name,
        tagset_input,
        segment_rules_name,
        segment_rules_input,
        SegmentRulesTarget::Generator,
    )?;
    let encoder = Utf8GeneratorEncoder;
    let entries = convert_polimorf_for_generator(
        source_lines(&sources),
        &tagset,
        &names_and_qualifiers.names,
        &names_and_qualifiers.qualifiers,
        &encoder,
        &parsed_rules,
    )?;

    build_generator_simple_dictionary_from_entries(
        &entries,
        &metadata.dict_id,
        &metadata.copyright,
        &tagset,
        &names_and_qualifiers.names,
        &names_and_qualifiers.qualifiers,
        &parsed_rules.segmentation_rules_data,
        &encoder,
    )
}

pub fn build_analyzer_simple_dictionary_from_paths<I, P, Q, R>(
    dictionary_paths: I,
    tagset_path: Q,
    segment_rules_path: R,
) -> Result<Vec<u8>>
where
    I: IntoIterator<Item = P>,
    P: AsRef<Path>,
    Q: AsRef<Path>,
    R: AsRef<Path>,
{
    let dictionary_files = read_named_files(dictionary_paths)?;
    let tagset_path = tagset_path.as_ref();
    let segment_rules_path = segment_rules_path.as_ref();
    let tagset_input = read_named_file_to_string(tagset_path)?;
    let segment_rules_input = read_named_file_to_string(segment_rules_path)?;
    let dictionary_sources = dictionary_files
        .iter()
        .map(|(name, input)| DictionarySource::new(name.as_str(), input.as_str()));

    build_analyzer_simple_dictionary_from_sources(
        dictionary_sources,
        &tagset_path.display().to_string(),
        &tagset_input,
        &segment_rules_path.display().to_string(),
        &segment_rules_input,
    )
}

pub fn build_generator_simple_dictionary_from_paths<I, P, Q, R>(
    dictionary_paths: I,
    tagset_path: Q,
    segment_rules_path: R,
) -> Result<Vec<u8>>
where
    I: IntoIterator<Item = P>,
    P: AsRef<Path>,
    Q: AsRef<Path>,
    R: AsRef<Path>,
{
    let dictionary_files = read_named_files(dictionary_paths)?;
    let tagset_path = tagset_path.as_ref();
    let segment_rules_path = segment_rules_path.as_ref();
    let tagset_input = read_named_file_to_string(tagset_path)?;
    let segment_rules_input = read_named_file_to_string(segment_rules_path)?;
    let dictionary_sources = dictionary_files
        .iter()
        .map(|(name, input)| DictionarySource::new(name.as_str(), input.as_str()));

    build_generator_simple_dictionary_from_sources(
        dictionary_sources,
        &tagset_path.display().to_string(),
        &tagset_input,
        &segment_rules_path.display().to_string(),
        &segment_rules_input,
    )
}

pub fn convert_polimorf_for_analyzer<I, L, T, E, S>(
    input_lines: I,
    tagset: &T,
    names_map: &BTreeMap<String, usize>,
    qualifiers_map: &BTreeMap<QualifierSet, usize>,
    encoder: &E,
    segment_rules: &S,
) -> Result<Vec<AnalyzerEntry>>
where
    I: IntoIterator<Item = L>,
    L: AsRef<str>,
    T: TagsetLookup,
    E: SortEncoder,
    S: SegmentRulesLookup,
{
    let mut parser = LineParser::new();
    let mut partial_lines = Vec::new();

    for (index, raw_line) in input_lines.into_iter().enumerate() {
        let line = strip_python_newline(raw_line.as_ref());
        if parser.ignore_line(line.as_ref()) {
            continue;
        }

        let parsed = parser.parse_line(line.as_ref())?;
        let tag_num = tagset.tag_num(&parsed.tag)?;
        let name_num = lookup_name(names_map, &parsed.name)?;
        let qualifiers = parse_qualifiers(&parsed.qualifier);
        let qualifiers_num = lookup_qualifiers(qualifiers_map, &qualifiers)?;
        let segment_type_num = segment_rules.lexeme_to_segment_type_num(
            &parsed.base,
            tag_num,
            name_num,
            qualifiers_num,
        )?;

        validate(
            !(segment_rules.should_replace_lemma_with_orth(segment_type_num)
                && segment_rules
                    .new_segment_type_for_shift_orth(segment_type_num)
                    .is_some()),
            "shift-orth replacement and extra segment cannot both be active",
        )?;

        let base = if segment_rules.should_replace_lemma_with_orth(segment_type_num) {
            parsed.orth.clone()
        } else {
            parsed.base.clone()
        };
        partial_lines.push(PartialAnalyzerLine {
            sort_key: encoder.word_sort_key(&parsed.orth),
            index,
            orth: parsed.orth.clone(),
            base,
            tag_num,
            name_num,
            segment_type_num,
            qualifiers_num,
        });

        if let Some(segment_type_num) =
            segment_rules.new_segment_type_for_shift_orth(segment_type_num)
        {
            partial_lines.push(PartialAnalyzerLine {
                sort_key: encoder.word_sort_key(&parsed.orth),
                index,
                orth: parsed.orth.clone(),
                base: parsed.orth.clone(),
                tag_num,
                name_num,
                segment_type_num,
                qualifiers_num,
            });
        }
    }

    partial_lines.sort_by(|left, right| {
        left.sort_key
            .cmp(&right.sort_key)
            .then(left.index.cmp(&right.index))
    });

    let keyed_interpretations = partial_lines
        .into_iter()
        .map(|line| {
            Ok((
                line.orth.clone(),
                AnalyzerInterpretation::new(
                    &line.orth,
                    &line.base,
                    line.tag_num,
                    line.name_num,
                    line.segment_type_num,
                    line.qualifiers_num,
                )?,
            ))
        })
        .collect::<Result<Vec<_>>>()?;

    merge_analyzer_entries(keyed_interpretations, true)
}

pub fn convert_polimorf_for_generator<I, L, T, E, S>(
    input_lines: I,
    tagset: &T,
    names_map: &BTreeMap<String, usize>,
    qualifiers_map: &BTreeMap<QualifierSet, usize>,
    encoder: &E,
    segment_rules: &S,
) -> Result<Vec<GeneratorEntry>>
where
    I: IntoIterator<Item = L>,
    L: AsRef<str>,
    T: TagsetLookup,
    E: SortEncoder,
    S: SegmentRulesLookup,
{
    let mut parser = LineParser::new();
    let mut partial_lines = Vec::new();

    for raw_line in input_lines {
        let line = strip_python_newline(raw_line.as_ref());
        if parser.ignore_line(line.as_ref()) {
            continue;
        }

        let parsed = parser.parse_line(line.as_ref())?;
        if parsed.base.is_empty() {
            continue;
        }

        let (base, homonym_id) = split_generator_homonym(&parsed.base);
        let tag_num = tagset.tag_num(&parsed.tag)?;
        let name_num = lookup_name(names_map, &parsed.name)?;
        let qualifiers = parse_qualifiers(&parsed.qualifier);
        let qualifiers_num = lookup_qualifiers(qualifiers_map, &qualifiers)?;
        let segment_type_num =
            segment_rules.lexeme_to_segment_type_num(&base, tag_num, name_num, qualifiers_num)?;

        let base = if segment_rules.should_replace_lemma_with_orth(segment_type_num) {
            parsed.orth.clone()
        } else {
            base
        };
        partial_lines.push(PartialGeneratorLine::new(
            &parsed.orth,
            &base,
            tag_num,
            name_num,
            segment_type_num,
            &homonym_id,
            qualifiers_num,
            encoder.word_sort_key(&base),
        ));

        if let Some(segment_type_num) =
            segment_rules.new_segment_type_for_shift_orth(segment_type_num)
        {
            partial_lines.push(PartialGeneratorLine::new(
                &parsed.orth,
                &parsed.orth,
                tag_num,
                name_num,
                segment_type_num,
                &homonym_id,
                qualifiers_num,
                encoder.word_sort_key(&parsed.orth),
            ));
        }
    }

    partial_lines.sort_by(|left, right| {
        left.sort_key
            .cmp(&right.sort_key)
            .then(left.legacy_line.cmp(&right.legacy_line))
    });

    let mut prev_legacy_line = None;
    let mut keyed_interpretations = Vec::new();
    for line in partial_lines {
        if prev_legacy_line.as_deref() == Some(line.legacy_line.as_str()) {
            continue;
        }
        prev_legacy_line = Some(line.legacy_line.clone());
        keyed_interpretations.push((
            line.base.clone(),
            GeneratorInterpretation::new(
                &line.orth,
                &line.base,
                line.tag_num,
                line.name_num,
                line.segment_type_num,
                &line.homonym_id,
                line.qualifiers_num,
            )?,
        ));
    }

    merge_generator_entries(keyed_interpretations, false)
}

pub fn split_generator_homonym(base: &str) -> (String, String) {
    match base.split_once(':') {
        Some((assumed_base, assumed_homonym_id))
            if !assumed_base.is_empty() && !assumed_homonym_id.is_empty() =>
        {
            (assumed_base.to_owned(), assumed_homonym_id.to_owned())
        }
        _ => (base.to_owned(), String::new()),
    }
}

pub fn encode_form_without_prefix(
    from_word: &str,
    target_word: &str,
    lowercase: bool,
) -> EncodedFormWithoutPrefix {
    let from_chars: Vec<char> = from_word.chars().collect();
    let target_chars: Vec<char> = target_word.chars().collect();
    let root_len = from_chars
        .iter()
        .zip(target_chars.iter())
        .take_while(|(from, target)| chars_match(**from, **target, lowercase))
        .count();
    let root = &target_chars[..root_len];
    let suffix_to_add = target_chars[root_len..].iter().collect();

    EncodedFormWithoutPrefix {
        cut_length: from_chars.len() - root_len,
        suffix_to_add,
        case_pattern: root.iter().copied().map(is_case_pattern_upper).collect(),
    }
}

pub fn encode_analyzer_form(from_word: &str, target_word: &str) -> Result<EncodedAnalyzerForm> {
    let from_chars: Vec<char> = from_word.chars().collect();
    let mut best_form = None;
    let mut best_prefix_cut_length = 0usize;

    for prefix_cut_length in 0..from_chars.len().min(5) {
        let suffix_from_word: String = from_chars[prefix_cut_length..].iter().collect();
        let encoded_form = encode_form_without_prefix(&suffix_from_word, target_word, true);
        let is_better = best_form
            .as_ref()
            .map(|best: &EncodedFormWithoutPrefix| {
                encoded_form.suffix_to_add.chars().count() + prefix_cut_length
                    < best.suffix_to_add.chars().count()
            })
            .unwrap_or(true);

        if is_better {
            best_form = Some(encoded_form);
            best_prefix_cut_length = prefix_cut_length;
        }
    }

    let best_form = best_form
        .ok_or_else(|| BuilderError::new("cannot encode analyzer form for an empty source word"))?;

    Ok(EncodedAnalyzerForm {
        prefix_cut_length: best_prefix_cut_length,
        cut_length: best_form.cut_length,
        suffix_to_add: best_form.suffix_to_add,
        case_pattern: best_form.case_pattern,
    })
}

pub fn encode_generator_form(from_word: &str, target_word: &str) -> Result<EncodedGeneratorForm> {
    let target_chars: Vec<char> = target_word.chars().collect();
    let mut best_form = None;
    let mut best_prefix_length = 0usize;

    for prefix_length in 0..target_chars.len().min(5) {
        let suffix_target_word: String = target_chars[prefix_length..].iter().collect();
        let encoded_form = encode_form_without_prefix(from_word, &suffix_target_word, false);
        let is_better = best_form
            .as_ref()
            .map(|best: &EncodedFormWithoutPrefix| {
                encoded_form.suffix_to_add.chars().count() + prefix_length
                    < best.suffix_to_add.chars().count() + best_prefix_length
            })
            .unwrap_or(true);

        if is_better {
            best_form = Some(encoded_form);
            best_prefix_length = prefix_length;
        }
    }

    let best_form = best_form.ok_or_else(|| {
        BuilderError::new("cannot encode generator form for an empty target word")
    })?;

    Ok(EncodedGeneratorForm {
        cut_length: best_form.cut_length,
        suffix_to_add: best_form.suffix_to_add,
        prefix_to_add: target_chars[..best_prefix_length].iter().collect(),
    })
}

impl AnalyzerInterpretation {
    pub fn new(
        orth: &str,
        base: &str,
        tag_num: usize,
        name_num: usize,
        type_num: usize,
        qualifiers: usize,
    ) -> Result<Self> {
        let encoded_form = encode_analyzer_form(orth, base)?;
        let orth_case_pattern_len = orth.chars().count() - encoded_form.cut_length;

        Ok(Self {
            encoded_form,
            orth_case_pattern: orth
                .chars()
                .take(orth_case_pattern_len)
                .map(is_case_pattern_upper)
                .collect(),
            tag_num,
            name_num,
            type_num,
            qualifiers,
        })
    }

    pub fn sort_key(&self) -> AnalyzerInterpretationSortKey {
        AnalyzerInterpretationSortKey {
            cut_length: self.encoded_form.cut_length,
            prefix_cut_length: self.encoded_form.prefix_cut_length,
            suffix_to_add: self.encoded_form.suffix_to_add.chars().collect(),
            case_pattern: self.encoded_form.case_pattern.clone(),
            orth_case_pattern: self.orth_case_pattern.clone(),
            tag_num: self.tag_num,
            name_num: self.name_num,
            type_num: self.type_num,
        }
    }
}

impl GeneratorInterpretation {
    pub fn new(
        orth: &str,
        base: &str,
        tag_num: usize,
        name_num: usize,
        type_num: usize,
        homonym_id: &str,
        qualifiers: usize,
    ) -> Result<Self> {
        Ok(Self {
            lemma: base.to_owned(),
            encoded_form: encode_generator_form(base, orth)?,
            tag_num,
            name_num,
            type_num,
            homonym_id: homonym_id.to_owned(),
            qualifiers,
        })
    }

    pub fn sort_key(&self) -> GeneratorInterpretationSortKey {
        GeneratorInterpretationSortKey {
            homonym_id: self.homonym_id.clone(),
            tag_num: self.tag_num,
            cut_length: self.encoded_form.cut_length,
            suffix_to_add: self.encoded_form.suffix_to_add.chars().collect(),
            name_num: self.name_num,
            type_num: self.type_num,
        }
    }
}

fn partition_once_space(line: &str) -> (&str, &str) {
    match line.split_once(' ') {
        Some((tag, value)) => (tag, value),
        None => (line, ""),
    }
}

fn parse_tagset_id_line(line: &str) -> Option<String> {
    let rest = line.strip_prefix("#!TAGSET-ID")?;
    if !rest.chars().next().is_some_and(char::is_whitespace) {
        return None;
    }
    Some(rest.trim_start_matches(char::is_whitespace).to_owned())
}

fn python_file_lines(input: &str) -> impl Iterator<Item = &str> {
    input
        .split_inclusive('\n')
        .map(|line| line.trim_matches(['\n', '\r']))
}

fn chars_match(left: char, right: char, lowercase: bool) -> bool {
    if lowercase {
        left.to_lowercase().to_string() == right.to_lowercase().to_string()
    } else {
        left == right
    }
}

fn is_case_pattern_upper(ch: char) -> bool {
    let original = ch.to_string();
    original == ch.to_uppercase().to_string() && original != ch.to_lowercase().to_string()
}

fn lookup_name(names_map: &BTreeMap<String, usize>, name: &str) -> Result<usize> {
    names_map
        .get(name)
        .copied()
        .ok_or_else(|| BuilderError::new(format!("unknown name: {name}")))
}

fn lookup_qualifiers(
    qualifiers_map: &BTreeMap<QualifierSet, usize>,
    qualifiers: &QualifierSet,
) -> Result<usize> {
    qualifiers_map
        .get(qualifiers)
        .copied()
        .ok_or_else(|| BuilderError::new(format!("unknown qualifiers: {qualifiers:?}")))
}

fn collect_dictionary_sources<'a, I>(dictionary_sources: I) -> Result<Vec<DictionarySource<'a>>>
where
    I: IntoIterator<Item = DictionarySource<'a>>,
{
    let sources = dictionary_sources.into_iter().collect::<Vec<_>>();
    validate(!sources.is_empty(), "dictionary sources must not be empty")?;
    Ok(sources)
}

fn parse_source_dictionary_context(
    sources: &[DictionarySource<'_>],
    tagset_name: &str,
    tagset_input: &str,
    segment_rules_name: &str,
    segment_rules_input: &str,
    target: SegmentRulesTarget,
) -> Result<(
    DictionaryMetadata,
    NamesAndQualifiers,
    Tagset,
    ParsedSegmentRules,
)> {
    let metadata = merge_metadata(sources.iter().map(|source| (source.name, source.input)))?;
    let names_and_qualifiers =
        merge_names_and_qualifiers(sources.iter().map(|source| (source.name, source.input)))?;
    let tagset = Tagset::from_str(tagset_name, tagset_input)?;
    let parsed_rules = parse_segmentation_rules_with_tagset_from_str(
        segment_rules_name,
        segment_rules_input,
        target,
        &tagset,
        &names_and_qualifiers.names,
        &names_and_qualifiers.qualifiers,
    )?;

    Ok((metadata, names_and_qualifiers, tagset, parsed_rules))
}

fn source_lines<'a>(sources: &'a [DictionarySource<'a>]) -> impl Iterator<Item = &'a str> + 'a {
    sources.iter().flat_map(|source| source.input.lines())
}

fn read_named_files<I, P>(paths: I) -> Result<Vec<(String, String)>>
where
    I: IntoIterator<Item = P>,
    P: AsRef<Path>,
{
    paths
        .into_iter()
        .map(|path| {
            let path = path.as_ref();
            Ok((path.display().to_string(), read_named_file_to_string(path)?))
        })
        .collect()
}

fn read_named_file_to_string(path: &Path) -> Result<String> {
    fs::read_to_string(path).map_err(|err| {
        BuilderError::new(format!(
            "failed to read input file {}: {err}",
            path.display()
        ))
    })
}

fn serialize_analyzer_interpretations_group(
    type_num: usize,
    interpretations: &[&AnalyzerInterpretation],
) -> Result<Vec<u8>> {
    validate_type_num(type_num)?;

    let mut sorted = interpretations.to_vec();
    sorted.sort_by_key(|interpretation| interpretation.sort_key());

    let orth_case_patterns = sorted
        .iter()
        .map(|interpretation| interpretation.orth_case_pattern.as_slice())
        .collect::<Vec<_>>();
    let lemma_case_patterns = sorted
        .iter()
        .map(|interpretation| interpretation.encoded_form.case_pattern.as_slice())
        .collect::<Vec<_>>();
    let prefix_cuts = sorted
        .iter()
        .map(|interpretation| interpretation.encoded_form.prefix_cut_length)
        .collect::<BTreeSet<_>>();

    let compression_byte =
        encode_analyzer_compression_byte(&orth_case_patterns, &lemma_case_patterns, &prefix_cuts);
    let orth_patterns_in_compression_byte =
        case_patterns_are_encoded_in_compression_byte(&orth_case_patterns);
    let lemma_patterns_in_compression_byte =
        case_patterns_are_encoded_in_compression_byte(&lemma_case_patterns);
    let prefix_cuts_in_compression_byte = prefix_cuts_are_encoded_in_compression_byte(&prefix_cuts);

    let mut encoded = Vec::new();
    encoded.push(compression_byte);

    if !orth_patterns_in_compression_byte {
        let min_patterns = min_orth_case_patterns(&orth_case_patterns);
        validate(
            min_patterns.len() <= u8::MAX as usize,
            "too many orth case patterns",
        )?;
        encoded.push(min_patterns.len() as u8);
        for pattern in min_patterns {
            encode_case_pattern(pattern, &mut encoded)?;
        }
    }

    for interpretation in sorted {
        if !orth_patterns_in_compression_byte {
            encode_case_pattern(&interpretation.orth_case_pattern, &mut encoded)?;
        }
        if !prefix_cuts_in_compression_byte {
            validate_u8(
                interpretation.encoded_form.prefix_cut_length,
                "analyzer prefix cut",
            )?;
            encoded.push(interpretation.encoded_form.prefix_cut_length as u8);
        }
        validate_u8(
            interpretation.encoded_form.cut_length,
            "analyzer suffix cut",
        )?;
        encoded.push(interpretation.encoded_form.cut_length as u8);
        encoded.extend(serialize_legacy_string(
            &interpretation.encoded_form.suffix_to_add,
        ));
        if !lemma_patterns_in_compression_byte {
            encode_case_pattern(&interpretation.encoded_form.case_pattern, &mut encoded)?;
        }
        encoded.extend(serialize_tag_num(interpretation.tag_num)?);
        encoded.push(serialize_name_num(interpretation.name_num)?);
        encoded.extend(serialize_u16_be(interpretation.qualifiers)?);
    }

    serialize_interpretations_group(type_num, encoded)
}

fn serialize_generator_interpretations_group(
    type_num: usize,
    interpretations: &[&GeneratorInterpretation],
) -> Result<Vec<u8>> {
    validate_type_num(type_num)?;

    let mut sorted = interpretations.to_vec();
    sorted.sort_by_key(|interpretation| interpretation.sort_key());

    let mut encoded = Vec::new();
    for interpretation in sorted {
        encoded.extend(serialize_legacy_string(&interpretation.homonym_id));
        encoded.extend(serialize_legacy_string(
            &interpretation.encoded_form.prefix_to_add,
        ));
        validate_u8(
            interpretation.encoded_form.cut_length,
            "generator suffix cut",
        )?;
        encoded.push(interpretation.encoded_form.cut_length as u8);
        encoded.extend(serialize_legacy_string(
            &interpretation.encoded_form.suffix_to_add,
        ));
        encoded.extend(serialize_tag_num(interpretation.tag_num)?);
        encoded.push(serialize_name_num(interpretation.name_num)?);
        encoded.extend(serialize_u16_be(interpretation.qualifiers)?);
    }

    serialize_interpretations_group(type_num, encoded)
}

fn serialize_interpretations_group(type_num: usize, encoded: Vec<u8>) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    out.push(type_num as u8);
    out.extend(serialize_u16_be(encoded.len())?);
    out.extend(encoded);
    Ok(out)
}

fn encode_analyzer_compression_byte(
    orth_case_patterns: &[&[bool]],
    lemma_case_patterns: &[&[bool]],
    prefix_cuts: &BTreeSet<usize>,
) -> u8 {
    let mut byte = 0;
    if case_patterns_have_only_lowercase(orth_case_patterns) {
        byte |= ORTH_ONLY_LOWER;
    } else if case_patterns_are_only_titles(orth_case_patterns) {
        byte |= ORTH_ONLY_TITLE;
    }
    if case_patterns_have_only_lowercase(lemma_case_patterns) {
        byte |= LEMMA_ONLY_LOWER;
    } else if case_patterns_are_only_titles(lemma_case_patterns) {
        byte |= LEMMA_ONLY_TITLE;
    }

    if prefix_cuts_are_encoded_in_compression_byte(prefix_cuts) {
        byte |= *prefix_cuts.iter().next().unwrap_or(&0) as u8;
    } else {
        byte |= PREFIX_CUT_MASK;
    }
    byte
}

fn min_orth_case_patterns<'a>(patterns: &[&'a [bool]]) -> Vec<&'a [bool]> {
    let mut result = Vec::new();
    for pattern in patterns {
        if !pattern.iter().any(|is_upper| *is_upper) {
            return Vec::new();
        }
        result.push(*pattern);
    }
    result
}

fn case_patterns_are_encoded_in_compression_byte(patterns: &[&[bool]]) -> bool {
    case_patterns_have_only_lowercase(patterns) || case_patterns_are_only_titles(patterns)
}

fn case_patterns_have_only_lowercase(patterns: &[&[bool]]) -> bool {
    !patterns
        .iter()
        .any(|pattern| pattern.iter().any(|is_upper| *is_upper))
}

fn case_patterns_are_only_titles(patterns: &[&[bool]]) -> bool {
    patterns.iter().all(|pattern| {
        pattern
            .split_first()
            .map(|(first, rest)| *first && !rest.iter().any(|is_upper| *is_upper))
            .unwrap_or(false)
    })
}

fn prefix_cuts_are_encoded_in_compression_byte(prefix_cuts: &BTreeSet<usize>) -> bool {
    prefix_cuts
        .iter()
        .next()
        .is_some_and(|prefix_cut| prefix_cuts.len() == 1 && *prefix_cut < PREFIX_CUT_MASK as usize)
}

fn encode_case_pattern(pattern: &[bool], out: &mut Vec<u8>) -> Result<()> {
    if !pattern.iter().any(|is_upper| *is_upper) {
        out.push(CASE_PATTERN_ONLY_LOWER);
    } else if has_upper_prefix(pattern) {
        out.push(CASE_PATTERN_UPPER_PREFIX);
        out.push(upper_prefix_length(pattern) as u8);
    } else {
        validate(
            pattern.len() <= u8::MAX as usize,
            "case pattern length does not fit into uint8",
        )?;
        let upper_indices = pattern
            .iter()
            .enumerate()
            .filter_map(|(index, is_upper)| is_upper.then_some(index))
            .collect::<Vec<_>>();
        validate(
            upper_indices.len() <= u8::MAX as usize,
            "case pattern uppercase count does not fit into uint8",
        )?;
        out.push(CASE_PATTERN_MIXED);
        out.push(upper_indices.len() as u8);
        for index in upper_indices {
            out.push(index as u8);
        }
    }
    Ok(())
}

fn has_upper_prefix(pattern: &[bool]) -> bool {
    (0..=pattern.len()).any(|index| {
        pattern[..index].iter().all(|is_upper| *is_upper)
            && !pattern[index..].iter().any(|is_upper| *is_upper)
    })
}

fn upper_prefix_length(pattern: &[bool]) -> usize {
    pattern
        .iter()
        .position(|is_upper| !*is_upper)
        .unwrap_or(pattern.len())
}

fn serialize_tag_num(tag_num: usize) -> Result<[u8; 2]> {
    validate(
        tag_num <= MAX_TAGS,
        format!("Too many tags. The limit is {MAX_TAGS}"),
    )?;
    serialize_u16_be(tag_num)
}

fn serialize_name_num(name_num: usize) -> Result<u8> {
    validate(
        name_num <= MAX_NAMES,
        format!("Too many named entity types. The limit is {MAX_NAMES}"),
    )?;
    Ok(name_num as u8)
}

fn validate_type_num(type_num: usize) -> Result<()> {
    validate(
        type_num <= MAX_SEGMENT_TYPES,
        format!("Too many segment types. The limit is {MAX_SEGMENT_TYPES}"),
    )
}

fn validate_u8(value: usize, label: &str) -> Result<()> {
    validate(
        value <= u8::MAX as usize,
        format!("{label} value {value} does not fit into uint8"),
    )
}

fn serialize_tag_records<'a, I, E>(records: I, encoder: &E) -> Result<Vec<u8>>
where
    I: IntoIterator<Item = (&'a str, usize)>,
    E: WordBytesEncoder,
{
    let records: Vec<(&str, usize)> = records.into_iter().collect();
    let mut out = Vec::new();
    push_u16(&mut out, records.len())?;
    for (tag, tag_num) in records {
        push_u16(&mut out, tag_num)?;
        out.extend(encoder.encode_word_bytes(tag));
        out.push(0);
    }
    Ok(out)
}

fn serialize_owned_tag_records<E>(records: Vec<(String, usize)>, encoder: &E) -> Result<Vec<u8>>
where
    E: WordBytesEncoder,
{
    let borrowed = records
        .iter()
        .map(|(tag, tag_num)| (tag.as_str(), *tag_num));
    serialize_tag_records(borrowed, encoder)
}

fn sorted_simple_transitions<'a>(
    state: &'a SimpleState,
    global_label_frequencies: &BTreeMap<u8, usize>,
) -> Vec<&'a SimpleTransition> {
    let mut indexed: Vec<(usize, &SimpleTransition)> =
        state.transitions.iter().enumerate().collect();
    indexed.sort_by(|(left_index, left), (right_index, right)| {
        let left_local = state
            .label_frequencies
            .get(&left.label)
            .copied()
            .unwrap_or_default();
        let right_local = state
            .label_frequencies
            .get(&right.label)
            .copied()
            .unwrap_or_default();
        let left_global = global_label_frequencies
            .get(&left.label)
            .copied()
            .unwrap_or_default();
        let right_global = global_label_frequencies
            .get(&right.label)
            .copied()
            .unwrap_or_default();

        right_local
            .cmp(&left_local)
            .then(right_global.cmp(&left_global))
            .then(left_index.cmp(right_index))
    });
    indexed
        .into_iter()
        .map(|(_index, transition)| transition)
        .collect()
}

fn validate_simple_graph(graph: &SimpleFsaGraph) -> Result<()> {
    validate(
        graph.initial_state < graph.states.len(),
        format!("invalid initial state index: {}", graph.initial_state),
    )?;
    for (state_index, state) in graph.states.iter().enumerate() {
        for transition in &state.transitions {
            validate(
                transition.target < graph.states.len(),
                format!(
                    "state {state_index} transition {} targets invalid state {}",
                    transition.label, transition.target
                ),
            )?;
        }
    }
    Ok(())
}

fn simple_graph_dfs_order(graph: &SimpleFsaGraph) -> Result<Vec<usize>> {
    let mut visited = vec![false; graph.states.len()];
    let mut order = Vec::new();
    simple_graph_dfs_visit(graph, graph.initial_state, &mut visited, &mut order)?;
    Ok(order)
}

fn simple_graph_dfs_visit(
    graph: &SimpleFsaGraph,
    state_index: usize,
    visited: &mut [bool],
    order: &mut Vec<usize>,
) -> Result<()> {
    if visited[state_index] {
        return Ok(());
    }
    visited[state_index] = true;

    let mut transitions: Vec<(usize, &SimpleGraphTransition)> = graph.states[state_index]
        .transitions
        .iter()
        .enumerate()
        .collect();
    transitions.sort_by(|(left_index, left), (right_index, right)| {
        graph.states[right.target]
            .frequency
            .cmp(&graph.states[left.target].frequency)
            .then(left_index.cmp(right_index))
    });

    for (_index, transition) in transitions {
        validate(
            transition.target < graph.states.len(),
            format!(
                "state {state_index} transition {} targets invalid state {}",
                transition.label, transition.target
            ),
        )?;
        simple_graph_dfs_visit(graph, transition.target, visited, order)?;
    }

    order.push(state_index);
    Ok(())
}

fn validate_segment_rules_fsa(fsa: &SegmentRulesFsa) -> Result<()> {
    validate(!fsa.states.is_empty(), "empty segmentation rules FSA")?;
    validate(
        fsa.initial_state < fsa.states.len(),
        format!(
            "invalid segmentation rules initial state index: {}",
            fsa.initial_state
        ),
    )?;
    for (state_index, state) in fsa.states.iter().enumerate() {
        validate(
            state.transitions.len() <= u8::MAX as usize,
            format!(
                "segmentation rules state {state_index} has too many transitions: {}",
                state.transitions.len()
            ),
        )?;
        validate(
            state.transition_order.len() == state.transitions.len(),
            format!("segmentation rules state {state_index} transition order is inconsistent"),
        )?;
        for label in &state.transition_order {
            validate(
                state.transitions.contains_key(label),
                format!("segmentation rules state {state_index} transition order has stale label"),
            )?;
        }
        for target in state.transitions.values() {
            validate(
                *target < fsa.states.len(),
                format!("segmentation rules state {state_index} targets invalid state {target}"),
            )?;
        }
    }
    Ok(())
}

fn segment_rules_fsa_dfs_order(fsa: &SegmentRulesFsa) -> Result<Vec<usize>> {
    let mut visited = vec![false; fsa.states.len()];
    let mut order = Vec::new();
    segment_rules_fsa_dfs_visit(fsa, fsa.initial_state, &mut visited, &mut order)?;
    Ok(order)
}

fn segment_rules_fsa_dfs_visit(
    fsa: &SegmentRulesFsa,
    state_index: usize,
    visited: &mut [bool],
    order: &mut Vec<usize>,
) -> Result<()> {
    if visited[state_index] {
        return Ok(());
    }
    visited[state_index] = true;

    for label in &fsa.states[state_index].transition_order {
        let target = fsa.states[state_index]
            .transitions
            .get(label)
            .ok_or_else(|| {
                BuilderError::new(format!(
                    "segmentation rules state {state_index} transition order has stale label"
                ))
            })?;
        validate(
            *target < fsa.states.len(),
            format!("segmentation rules state {state_index} targets invalid state {target}"),
        )?;
        segment_rules_fsa_dfs_visit(fsa, *target, visited, order)?;
    }

    order.push(state_index);
    Ok(())
}

fn segment_rules_state_size(state: &SegmentRulesState) -> Result<usize> {
    validate(
        state.transitions.len() <= u8::MAX as usize,
        format!(
            "segmentation rules state has too many transitions: {}",
            state.transitions.len()
        ),
    )?;
    Ok(2 + 4 * state.transitions.len())
}

fn serialize_segment_rules_state(
    state: &SegmentRulesState,
    layout: &SegmentRulesFsaLayout,
) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(segment_rules_state_size(state)?);
    let mut flags = 0u8;
    if state.accepting {
        flags |= 1;
    }
    if state.weak {
        flags |= 2;
    }
    out.push(flags);
    out.push(state.transitions.len() as u8);
    for (label, target) in &state.transitions {
        let target_offset = layout.offsets[*target];
        validate(
            target_offset <= MAX_SEGMENT_RULES_FSA_SIZE,
            format!(
                "Segmentation rules are too big and complicated- the resulting automaton would exceed its max size which is {}",
                MAX_SEGMENT_RULES_FSA_SIZE
            ),
        )?;
        out.push(label.segment_type_num);
        out.push(u8::from(label.shift_orth));
        out.extend(serialize_u16_be(target_offset)?);
    }
    Ok(out)
}

fn serialize_segment_rules_options_map(options: &BTreeMap<String, String>) -> Result<Vec<u8>> {
    let aggl = options
        .get("aggl")
        .ok_or_else(|| BuilderError::new("segmentation options missing aggl"))?;
    let praet = options
        .get("praet")
        .ok_or_else(|| BuilderError::new("segmentation options missing praet"))?;
    let mut out = Vec::new();
    out.push(2);
    out.extend(serialize_legacy_string("aggl"));
    out.extend(serialize_legacy_string(aggl));
    out.extend(serialize_legacy_string("praet"));
    out.extend(serialize_legacy_string(praet));
    Ok(out)
}

fn simple_graph_state_size(
    state: &SimpleGraphState,
    serialize_transition_data: bool,
) -> Result<usize> {
    let simple_state = SimpleState {
        encoded_data: state.encoded_data.clone(),
        transitions: state
            .transitions
            .iter()
            .map(|transition| SimpleTransition {
                label: transition.label,
                target_offset: 0,
                transition_data: transition.transition_data,
            })
            .collect(),
        label_frequencies: state.label_frequencies.clone(),
    };
    simple_state_size(&simple_state, serialize_transition_data)
}

fn simple_state_for_graph_state(
    graph: &SimpleFsaGraph,
    layout: &SimpleGraphLayout,
    state_index: usize,
) -> SimpleState {
    let graph_state = &graph.states[state_index];
    SimpleState {
        encoded_data: graph_state.encoded_data.clone(),
        transitions: graph_state
            .transitions
            .iter()
            .map(|transition| SimpleTransition {
                label: transition.label,
                target_offset: layout.offsets[transition.target],
                transition_data: transition.transition_data,
            })
            .collect(),
        label_frequencies: graph_state.label_frequencies.clone(),
    }
}

impl SortedSimpleFsaBuilder {
    fn new() -> Self {
        Self {
            states: vec![SimpleBuildState::new()],
            register: BTreeMap::new(),
            initial_state: 0,
            previous_word: None,
            entries_num: 0,
            global_label_frequencies: BTreeMap::new(),
        }
    }

    fn add_entry(&mut self, word: &[u8], data: &[u8]) -> Result<()> {
        validate(!word.is_empty(), "entry word must not be empty")?;
        if let Some(previous_word) = &self.previous_word {
            validate(
                previous_word.as_slice() < word,
                "input entries must be strictly sorted by encoded word",
            )?;
        }

        let mut state_index = self.initial_state;
        let mut prefix_len = 0usize;
        while prefix_len < word.len() {
            let Some(next_index) = self.transition_target(state_index, word[prefix_len]) else {
                break;
            };
            state_index = next_index;
            prefix_len += 1;
        }

        if let Some(previous_word) = self.previous_word.clone() {
            if prefix_len < previous_word.len() {
                let label = previous_word[prefix_len];
                let next_state = self.transition_target(state_index, label).ok_or_else(|| {
                    BuilderError::new(format!(
                        "missing previous-word transition {} at prefix length {prefix_len}",
                        label
                    ))
                })?;
                let replacement =
                    self.replace_or_register(next_state, &previous_word[prefix_len + 1..])?;
                self.set_transition(state_index, label, replacement);
            }
        }

        while prefix_len < word.len() {
            let next_state = self.add_state();
            self.set_transition(state_index, word[prefix_len], next_state);
            state_index = next_state;
            prefix_len += 1;
        }

        validate(
            self.states[state_index].encoded_data.is_none(),
            "duplicate encoded word",
        )?;
        self.states[state_index].encoded_data = Some(data.to_vec());
        self.previous_word = Some(word.to_vec());
        self.entries_num += 1;
        for &label in word {
            *self.global_label_frequencies.entry(label).or_default() += 1;
        }
        Ok(())
    }

    fn close(mut self) -> Result<SimpleFsaGraph> {
        validate(self.entries_num > 0, "empty input")?;
        let previous_word = self
            .previous_word
            .clone()
            .ok_or_else(|| BuilderError::new("empty input"))?;
        self.initial_state = self.replace_or_register(self.initial_state, &previous_word)?;
        Ok(self.into_graph())
    }

    fn add_state(&mut self) -> usize {
        let index = self.states.len();
        self.states.push(SimpleBuildState::new());
        index
    }

    fn transition_target(&self, state_index: usize, label: u8) -> Option<usize> {
        self.states[state_index]
            .transitions
            .iter()
            .find_map(|(transition_label, target)| (*transition_label == label).then_some(*target))
    }

    fn set_transition(&mut self, state_index: usize, label: u8, target: usize) {
        if let Some((_transition_label, transition_target)) = self.states[state_index]
            .transitions
            .iter_mut()
            .find(|(transition_label, _target)| *transition_label == label)
        {
            *transition_target = target;
        } else {
            self.states[state_index].transitions.push((label, target));
        }
    }

    fn replace_or_register(&mut self, state_index: usize, encoded_word: &[u8]) -> Result<usize> {
        if let Some((&label, suffix)) = encoded_word.split_first() {
            let next_state = self.transition_target(state_index, label).ok_or_else(|| {
                BuilderError::new(format!(
                    "missing transition {label} during state minimization"
                ))
            })?;
            let replacement = self.replace_or_register(next_state, suffix)?;
            self.set_transition(state_index, label, replacement);
        }

        let key = self.register_key(state_index);
        if let Some(equivalent_state) = self.register.get(&key) {
            Ok(*equivalent_state)
        } else {
            self.register.insert(key, state_index);
            Ok(state_index)
        }
    }

    fn register_key(&self, state_index: usize) -> SimpleBuildStateKey {
        SimpleBuildStateKey {
            transitions: self.states[state_index]
                .transitions
                .iter()
                .copied()
                .collect(),
            encoded_data: self.states[state_index].encoded_data.clone(),
        }
    }

    fn into_graph(self) -> SimpleFsaGraph {
        let mut old_to_new = BTreeMap::new();
        let mut states = Vec::new();
        self.copy_reachable_state(self.initial_state, &mut old_to_new, &mut states);
        SimpleFsaGraph {
            states,
            initial_state: old_to_new[&self.initial_state],
            global_label_frequencies: self.global_label_frequencies,
        }
    }

    fn copy_reachable_state(
        &self,
        old_index: usize,
        old_to_new: &mut BTreeMap<usize, usize>,
        states: &mut Vec<SimpleGraphState>,
    ) -> usize {
        if let Some(new_index) = old_to_new.get(&old_index) {
            return *new_index;
        }

        let new_index = states.len();
        old_to_new.insert(old_index, new_index);
        states.push(SimpleGraphState {
            encoded_data: self.states[old_index].encoded_data.clone(),
            transitions: Vec::new(),
            frequency: 0,
            label_frequencies: BTreeMap::new(),
        });

        let transitions = self.states[old_index].transitions.clone();
        for (label, old_target) in transitions {
            let new_target = self.copy_reachable_state(old_target, old_to_new, states);
            states[new_index].transitions.push(SimpleGraphTransition {
                label,
                target: new_target,
                transition_data: None,
            });
        }

        new_index
    }
}

fn push_u16(out: &mut Vec<u8>, value: usize) -> Result<()> {
    out.extend(serialize_u16_be(value)?);
    Ok(())
}

fn validate_u16(value: usize) -> Result<()> {
    validate(
        value <= u16::MAX as usize,
        format!("value {value} does not fit into uint16"),
    )
}

fn validate_u32(value: usize) -> Result<()> {
    validate(
        value <= u32::MAX as usize,
        format!("value {value} does not fit into uint32"),
    )
}

fn index_btree_set<T>(values: BTreeSet<T>) -> BTreeMap<T, usize>
where
    T: Ord,
{
    values
        .into_iter()
        .enumerate()
        .map(|(index, value)| (value, index))
        .collect()
}

fn validate(condition: bool, message: impl Into<String>) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(BuilderError::new(message))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SegmentRulesConfigFile {
    input_name: String,
    section_names: BTreeSet<String>,
    section_lines: BTreeMap<String, Vec<(usize, String)>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SegmentRuleAtom {
    segment_type_num: usize,
    shift_orth: bool,
    segment_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct ShiftOrthMagicData {
    replace_lemma_with_orth: BTreeSet<usize>,
    shift_orth_extra_segment_types: BTreeMap<usize, usize>,
    additional_segment_type_names: BTreeMap<usize, String>,
}

impl SegmentRulesConfigFile {
    fn parse<I, S>(input_name: &str, input: &str, section_names: I) -> Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let section_names: BTreeSet<String> = section_names
            .into_iter()
            .map(|section_name| section_name.as_ref().to_owned())
            .collect();
        let mut section_lines = BTreeMap::new();
        let mut current_section: Option<String> = None;

        for (line_index, raw_line) in input.lines().enumerate() {
            let line_number = line_index + 1;
            if let Some(section_name) = segment_rules_header_value(raw_line) {
                validate(
                    section_names.contains(&section_name),
                    format!("{input_name}:{line_number} - Invalid section: {section_name}"),
                )?;
                validate(
                    !section_lines.contains_key(&section_name),
                    format!("{input_name}:{line_number} - Duplicate section: {section_name}"),
                )?;
                section_lines.insert(section_name.clone(), Vec::new());
                current_section = Some(section_name);
            } else {
                let line = raw_line.trim();
                if line.is_empty() {
                    continue;
                }
                let Some(section_name) = current_section.as_ref() else {
                    if line.starts_with('#') {
                        continue;
                    }
                    return Err(BuilderError::new(format!(
                        "{input_name}:{line_number} - Text outside of any section"
                    )));
                };
                section_lines
                    .get_mut(section_name)
                    .expect("current section was inserted")
                    .push((line_number, line.to_owned()));
            }
        }

        Ok(Self {
            input_name: input_name.to_owned(),
            section_names,
            section_lines,
        })
    }

    fn lines_in_section(
        &self,
        section_name: &str,
        ignore_comments: bool,
    ) -> Result<Vec<(usize, String)>> {
        validate(
            self.section_names.contains(section_name),
            format!("invalid known section request: {section_name}"),
        )?;
        let lines = self.section_lines.get(section_name).ok_or_else(|| {
            BuilderError::new(format!(
                "{} - Missing section: \"{}\"",
                self.input_name, section_name
            ))
        })?;
        Ok(lines
            .iter()
            .filter(|(_line_number, line)| !ignore_comments || !line.starts_with('#'))
            .cloned()
            .collect())
    }
}

fn segment_rules_header_value(line: &str) -> Option<String> {
    let line = line.trim_start();
    let rest = line.strip_prefix('[')?;
    let end = rest.find(']')?;
    Some(rest[..end].to_owned())
}

fn parse_segment_rules_options(
    config: &SegmentRulesConfigFile,
) -> Result<Vec<(String, Vec<String>)>> {
    let mut options = Vec::new();
    for (line_number, line) in config.lines_in_section("options", true)? {
        let (key, values) = line.split_once('=').ok_or_else(|| {
            BuilderError::new(format!(
                "{}:{line_number} - Error in [options] section: missing '='",
                config.input_name
            ))
        })?;
        let key = key.trim();
        validate(
            is_ascii_word(key),
            format!(
                "{}:{line_number} - Error in [options] section: invalid option key: {key}",
                config.input_name
            ),
        )?;
        let values: Vec<String> = values.split_whitespace().map(str::to_owned).collect();
        validate(
            !values.is_empty() && values.iter().all(|value| is_ascii_word(value)),
            format!(
                "{}:{line_number} - Error in [options] section: invalid option values",
                config.input_name
            ),
        )?;
        options.push((key.to_owned(), values));
    }
    Ok(options)
}

fn parse_segment_types_section(config: &SegmentRulesConfigFile) -> Result<Vec<String>> {
    let mut segment_types = Vec::new();
    for (line_number, line) in config.lines_in_section("segment types", true)? {
        validate(
            is_ascii_word(&line),
            format!(
                "{}:{line_number} - Segment type must be a single word",
                config.input_name
            ),
        )?;
        validate(
            !segment_types.contains(&line),
            format!(
                "{}:{line_number} - Segment type already defined: \"{}\"",
                config.input_name, line
            ),
        )?;
        segment_types.push(line);
    }
    Ok(segment_types)
}

fn parse_separator_chars_section(config: &SegmentRulesConfigFile) -> Result<Vec<u32>> {
    let mut separators = Vec::new();
    for (line_number, line) in config.lines_in_section("separator chars", true)? {
        let separator = line.parse::<u32>().map_err(|err| {
            BuilderError::new(format!("{}:{line_number} - {err}", config.input_name))
        })?;
        separators.push(separator);
    }
    Ok(separators)
}

fn definitions_to_option_keys(
    option_definitions: &[(String, Vec<String>)],
) -> BTreeMap<String, String> {
    let mut definitions_to_option_keys = BTreeMap::new();
    for (key, definitions) in option_definitions {
        for definition in definitions {
            definitions_to_option_keys.insert(definition.clone(), key.clone());
        }
    }
    definitions_to_option_keys
}

fn segment_rules_option_combinations(
    option_definitions: &[(String, Vec<String>)],
) -> Vec<Vec<String>> {
    let mut combinations = vec![Vec::new()];
    for (_key, definitions) in option_definitions {
        let mut next = Vec::new();
        for combination in &combinations {
            for definition in definitions {
                let mut combination = combination.clone();
                combination.push(definition.clone());
                next.push(combination);
            }
        }
        combinations = next;
    }
    combinations
}

fn active_definitions_to_options(
    active_definitions: &[String],
    definitions_to_option_keys: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>> {
    let mut options = BTreeMap::new();
    for definition in active_definitions {
        let key = definitions_to_option_keys.get(definition).ok_or_else(|| {
            BuilderError::new(format!("missing option key for definition {definition}"))
        })?;
        options.insert(key.clone(), definition.clone());
    }
    Ok(options)
}

fn apply_shift_orth_magic(
    segment_types: &[String],
    rules_by_options: &mut [(BTreeMap<String, String>, Vec<SegmentRule>)],
) -> ShiftOrthMagicData {
    let mut shift_orth_segment_types = BTreeSet::new();
    let mut non_shift_orth_segment_types = BTreeSet::new();

    for (_options, rules) in rules_by_options.iter() {
        for rule in rules {
            let mut atoms = Vec::new();
            rule.collect_atomic_rule_info(&mut atoms);
            for atom in atoms {
                if atom.shift_orth {
                    shift_orth_segment_types.insert(atom.segment_type);
                } else {
                    non_shift_orth_segment_types.insert(atom.segment_type);
                }
            }
        }
    }

    let mut result = ShiftOrthMagicData::default();
    let mut next_new_segment_type_num = segment_types.len();
    for (segment_type_num, segment_type) in segment_types.iter().enumerate() {
        if shift_orth_segment_types.contains(segment_type)
            && non_shift_orth_segment_types.contains(segment_type)
        {
            result
                .shift_orth_extra_segment_types
                .insert(segment_type_num, next_new_segment_type_num);
            result
                .additional_segment_type_names
                .insert(next_new_segment_type_num, format!("{segment_type}>"));
            next_new_segment_type_num += 1;
        } else if shift_orth_segment_types.contains(segment_type) {
            result.replace_lemma_with_orth.insert(segment_type_num);
        }
    }

    for (_options, rules) in rules_by_options {
        for rule in rules {
            rule.remap_shift_orth_segment_types(&result.shift_orth_extra_segment_types);
        }
    }

    result
}

fn is_ascii_word(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

impl SegmentTypeResolver {
    fn from_config<T>(
        config: &SegmentRulesConfigFile,
        segment_types: Vec<String>,
        tagset: &T,
        names_map: &BTreeMap<String, usize>,
        labels_map: &BTreeMap<QualifierSet, usize>,
    ) -> Result<Self>
    where
        T: TagsetRulesLookup,
    {
        let segment_type_to_num: BTreeMap<String, usize> = segment_types
            .iter()
            .enumerate()
            .map(|(index, segment_type)| (segment_type.clone(), index))
            .collect();
        let mut patterns = Vec::new();
        for (line_number, line) in config.lines_in_section("lexemes", true)? {
            patterns.push(parse_segment_type_pattern(
                config,
                line_number,
                &line,
                true,
                &segment_type_to_num,
                tagset,
            )?);
        }

        let mut got_wildcard_pattern = false;
        let mut last_tag_line_number = None;
        for (line_number, line) in config.lines_in_section("tags", true)? {
            patterns.push(parse_segment_type_pattern(
                config,
                line_number,
                &line,
                false,
                &segment_type_to_num,
                tagset,
            )?);
            validate(
                !got_wildcard_pattern,
                format!(
                    "{}:{} - Pattern that matches everything must be the last one",
                    config.input_name,
                    line_number.saturating_sub(1)
                ),
            )?;
            got_wildcard_pattern = patterns
                .last()
                .expect("just pushed tag pattern")
                .is_wildcard_pattern();
            last_tag_line_number = Some(line_number);
        }
        let line_number = last_tag_line_number.unwrap_or_default();
        validate(
            patterns.last().is_some_and(SegmentTypePattern::is_wildcard_pattern),
            format!(
                "{}:{line_number} - There must be a pattern that matches everything at the end of [tags] section",
                config.input_name
            ),
        )?;

        let mut resolver = Self {
            segment_types,
            segment_type_to_num,
            segment_nums: BTreeMap::new(),
            replace_lemma_with_orth: BTreeSet::new(),
            shift_orth_extra_segment_types: BTreeMap::new(),
        };
        resolver.index_patterns(patterns, tagset, names_map, labels_map)?;
        Ok(resolver)
    }

    fn with_shift_magic(
        mut self,
        replace_lemma_with_orth: BTreeSet<usize>,
        shift_orth_extra_segment_types: BTreeMap<usize, usize>,
    ) -> Self {
        self.replace_lemma_with_orth = replace_lemma_with_orth;
        self.shift_orth_extra_segment_types = shift_orth_extra_segment_types;
        self
    }

    fn index_patterns<T>(
        &mut self,
        patterns: Vec<SegmentTypePattern>,
        tagset: &T,
        names_map: &BTreeMap<String, usize>,
        labels_map: &BTreeMap<QualifierSet, usize>,
    ) -> Result<()>
    where
        T: TagsetRulesLookup,
    {
        for pattern in patterns {
            for tag in tagset.all_tags() {
                if pattern.matches_tag(tag) {
                    let tag_num = tagset.tag_num(tag)?;
                    let name_num = names_map.get(&pattern.name).copied();
                    for labels_num in existing_labels_num_combinations(&pattern.labels, labels_map)
                    {
                        self.segment_nums
                            .entry((pattern.lemma.clone(), tag_num))
                            .or_default()
                            .push(SegmentTypeAssignment {
                                homonym: pattern.homonym.clone(),
                                name_num,
                                labels_num,
                                segment_type_num: pattern.segment_type_num,
                            });
                    }
                }
            }
        }
        Ok(())
    }

    fn lookup_segment_type_num(
        &self,
        lemma: Option<&str>,
        tag_num: usize,
        name_num: usize,
        labels_num: usize,
    ) -> Option<usize> {
        let (lemma, homonym) = split_lemma_homonym(lemma);
        if let Some(assignments) = self.segment_nums.get(&(lemma.clone(), tag_num)) {
            for assignment in assignments {
                if assignment.homonym == homonym
                    && segment_type_assignment_matches(assignment, name_num, labels_num)
                {
                    return Some(assignment.segment_type_num);
                }
            }
        }

        if homonym.is_some() {
            self.lookup_segment_type_num(lemma.as_deref(), tag_num, name_num, labels_num)
        } else if lemma.is_some() {
            self.lookup_segment_type_num(None, tag_num, name_num, labels_num)
        } else {
            None
        }
    }
}

impl SegmentTypeLookup for SegmentTypeResolver {
    fn segment_type_num(&self, segment_type: &str) -> Result<usize> {
        self.segment_type_to_num
            .get(segment_type)
            .copied()
            .ok_or_else(|| BuilderError::new(format!("unknown segment type: {segment_type}")))
    }
}

impl SegmentRulesLookup for SegmentTypeResolver {
    fn lexeme_to_segment_type_num(
        &self,
        base: &str,
        tag_num: usize,
        name_num: usize,
        qualifiers_num: usize,
    ) -> Result<usize> {
        self.lookup_segment_type_num(Some(base), tag_num, name_num, qualifiers_num)
            .ok_or_else(|| {
                BuilderError::new(format!(
                    "missing segment type for {base}/{tag_num}/{name_num}/{qualifiers_num}"
                ))
            })
    }

    fn should_replace_lemma_with_orth(&self, segment_type_num: usize) -> bool {
        self.replace_lemma_with_orth.contains(&segment_type_num)
    }

    fn new_segment_type_for_shift_orth(&self, segment_type_num: usize) -> Option<usize> {
        self.shift_orth_extra_segment_types
            .get(&segment_type_num)
            .copied()
    }
}

impl SegmentRulesLookup for ParsedSegmentRules {
    fn lexeme_to_segment_type_num(
        &self,
        base: &str,
        tag_num: usize,
        name_num: usize,
        qualifiers_num: usize,
    ) -> Result<usize> {
        self.segment_type_resolver
            .as_ref()
            .ok_or_else(|| BuilderError::new("segmentation rules were parsed without tagset data"))?
            .lexeme_to_segment_type_num(base, tag_num, name_num, qualifiers_num)
    }

    fn should_replace_lemma_with_orth(&self, segment_type_num: usize) -> bool {
        self.segment_type_resolver
            .as_ref()
            .is_some_and(|resolver| resolver.should_replace_lemma_with_orth(segment_type_num))
    }

    fn new_segment_type_for_shift_orth(&self, segment_type_num: usize) -> Option<usize> {
        self.segment_type_resolver
            .as_ref()
            .and_then(|resolver| resolver.new_segment_type_for_shift_orth(segment_type_num))
    }
}

impl SegmentTypePattern {
    fn matches_tag(&self, tag: &str) -> bool {
        segment_type_pattern_matches_tag(&self.pattern, tag)
    }

    fn is_wildcard_pattern(&self) -> bool {
        self.lemma.is_none()
            && self.pattern == "%"
            && self.name.is_empty()
            && self.labels.is_empty()
    }
}

fn parse_segment_type_pattern<T>(
    config: &SegmentRulesConfigFile,
    line_number: usize,
    line: &str,
    with_lemma: bool,
    segment_type_to_num: &BTreeMap<String, usize>,
    tagset: &T,
) -> Result<SegmentTypePattern>
where
    T: TagsetRulesLookup,
{
    let fields: Vec<&str> = line.split_whitespace().collect();
    let (segment_type, lemma, pattern, constraint_fields) = if with_lemma {
        validate(
            (3..=5).contains(&fields.len()),
            format!(
                "{}:{line_number} - Line in [lexemes] section must contain 3 to 5 fields - segment type, lemma, tag pattern and optional constraints on name and labels",
                config.input_name
            ),
        )?;
        (fields[0], Some(fields[1]), fields[2], &fields[3..])
    } else {
        validate(
            (2..=4).contains(&fields.len()),
            format!(
                "{}:{line_number} - Line in [tags] section must contain 2 to 4 fields - segment type, tag pattern and optional constraints on name and labels",
                config.input_name
            ),
        )?;
        (fields[0], None, fields[1], &fields[2..])
    };

    let segment_type_num = segment_type_to_num
        .get(segment_type)
        .copied()
        .ok_or_else(|| {
            BuilderError::new(format!(
                "{}:{line_number} - Undeclared segment type: \"{}\"",
                config.input_name, segment_type
            ))
        })?;

    validate(
        is_legacy_segment_type_pattern(pattern),
        format!(
            "{}:{line_number} - Pattern must contain only \":\", \"%\", \".\" and lowercase alphanumeric letters",
            config.input_name
        ),
    )?;

    let constraints = parse_segment_type_constraints(config, line_number, constraint_fields)?;
    let (lemma, homonym) = split_lemma_homonym(lemma);
    let segment_type_pattern = SegmentTypePattern {
        lemma,
        homonym,
        pattern: pattern.to_owned(),
        name: constraints.name.unwrap_or_default(),
        labels: constraints.labels.unwrap_or_default(),
        segment_type_num,
    };
    validate(
        tagset
            .all_tags()
            .iter()
            .any(|tag| segment_type_pattern.matches_tag(tag)),
        format!(
            "{}:{line_number} - There is no tag that matches pattern \"{}\".",
            config.input_name, pattern
        ),
    )?;
    Ok(segment_type_pattern)
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct SegmentTypeConstraints {
    name: Option<String>,
    labels: Option<QualifierSet>,
}

fn parse_segment_type_constraints(
    config: &SegmentRulesConfigFile,
    line_number: usize,
    fields: &[&str],
) -> Result<SegmentTypeConstraints> {
    let mut constraints = SegmentTypeConstraints::default();
    for field in fields {
        let (key, value) = field.split_once('=').ok_or_else(|| {
            BuilderError::new(format!(
                "{}:{line_number} - invalid name or labels constraint: \"{}\"",
                config.input_name, field
            ))
        })?;
        validate(
            matches!(key, "name" | "labels")
                && !value.is_empty()
                && !value.chars().any(char::is_whitespace),
            format!(
                "{}:{line_number} - invalid name or labels constraint: \"{}\"",
                config.input_name, field
            ),
        )?;
        match key {
            "name" => {
                validate(
                    constraints.name.is_none(),
                    format!(
                        "{}:{line_number} - name already specified",
                        config.input_name
                    ),
                )?;
                constraints.name = Some(value.to_owned());
            }
            "labels" => {
                validate(
                    constraints.labels.is_none(),
                    format!(
                        "{}:{line_number} - labels already specified",
                        config.input_name
                    ),
                )?;
                constraints.labels = Some(parse_qualifiers(value));
            }
            _ => unreachable!("key validated"),
        }
    }
    Ok(constraints)
}

fn existing_labels_num_combinations(
    labels: &QualifierSet,
    labels_map: &BTreeMap<QualifierSet, usize>,
) -> Vec<usize> {
    if labels.is_empty() {
        vec![0]
    } else {
        labels_map
            .iter()
            .filter_map(|(labels_combination, labels_num)| {
                labels.is_subset(labels_combination).then_some(*labels_num)
            })
            .collect()
    }
}

fn segment_type_assignment_matches(
    assignment: &SegmentTypeAssignment,
    name_num: usize,
    labels_num: usize,
) -> bool {
    matches!(
        (assignment.name_num, assignment.labels_num),
        (Some(n), l)
            if (n, l) == (name_num, labels_num)
                || (n, l) == (0, 0)
                || (n == 0 && l == labels_num)
                || (l == 0 && n == name_num)
    )
}

fn split_lemma_homonym(lemma: Option<&str>) -> (Option<String>, Option<String>) {
    match lemma {
        None => (None, None),
        Some(lemma) if lemma.contains(':') && !lemma.replace(':', "").is_empty() => {
            let (lemma, homonym) = lemma.split_once(':').expect("contains ':' checked");
            (Some(lemma.to_owned()), Some(homonym.to_owned()))
        }
        Some(lemma) => (Some(lemma.to_owned()), None),
    }
}

fn is_legacy_segment_type_pattern(pattern: &str) -> bool {
    pattern
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_lowercase() || matches!(ch, '_' | '.' | ':' | '%'))
}

fn segment_type_pattern_matches_tag(pattern: &str, tag: &str) -> bool {
    wildcard_pattern_matches(pattern, tag)
        || pattern
            .strip_suffix(":%")
            .is_some_and(|stripped| wildcard_pattern_matches(stripped, tag))
}

fn wildcard_pattern_matches(pattern: &str, tag: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let tag: Vec<char> = tag.chars().collect();
    let mut memo = BTreeMap::new();
    wildcard_pattern_matches_from(&pattern, &tag, 0, 0, &mut memo)
}

fn wildcard_pattern_matches_from(
    pattern: &[char],
    tag: &[char],
    pattern_index: usize,
    tag_index: usize,
    memo: &mut BTreeMap<(usize, usize), bool>,
) -> bool {
    if let Some(result) = memo.get(&(pattern_index, tag_index)) {
        return *result;
    }
    let result = if pattern_index == pattern.len() {
        tag_index == tag.len()
    } else if pattern[pattern_index] == '%' {
        (tag_index..=tag.len()).any(|next_tag_index| {
            wildcard_pattern_matches_from(pattern, tag, pattern_index + 1, next_tag_index, memo)
        })
    } else if tag_index < tag.len()
        && (pattern[pattern_index] == '.' || pattern[pattern_index] == tag[tag_index])
    {
        wildcard_pattern_matches_from(pattern, tag, pattern_index + 1, tag_index + 1, memo)
    } else {
        false
    };
    memo.insert((pattern_index, tag_index), result);
    result
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SegmentRuleDefine {
    WithoutArg {
        name: String,
        value: String,
    },
    WithArg {
        name: String,
        arg: String,
        value: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SegmentRuleDefineValue {
    WithoutArg(String),
    WithArg { arg: String, value: String },
}

fn parse_segment_rule_define(
    line: &str,
    line_number: usize,
    input_name: &str,
) -> Result<SegmentRuleDefine> {
    let rest = line
        .strip_prefix("#define")
        .ok_or_else(|| BuilderError::new(format!("{input_name}:{line_number}: invalid #define")))?;
    let rest = rest.trim_start();
    let (name, mut cursor) = read_segment_rule_identifier(rest).ok_or_else(|| {
        BuilderError::new(format!(
            "{input_name}:{line_number}: #define must be followed by identifier"
        ))
    })?;

    if rest[cursor..].starts_with('(') {
        cursor += 1;
        let (arg, arg_len) = read_segment_rule_identifier(&rest[cursor..]).ok_or_else(|| {
            BuilderError::new(format!(
                "{input_name}:{line_number}: #define argument must be an identifier"
            ))
        })?;
        cursor += arg_len;
        let after_arg = rest[cursor..].trim_start();
        validate(
            after_arg.starts_with(')'),
            format!("{input_name}:{line_number}: #define argument list must end with ')'"),
        )?;
        let close_offset = rest.len() - after_arg.len();
        let value = rest[close_offset + 1..].to_owned();
        Ok(SegmentRuleDefine::WithArg { name, arg, value })
    } else {
        Ok(SegmentRuleDefine::WithoutArg {
            name,
            value: rest[cursor..].trim_start().to_owned(),
        })
    }
}

fn parse_segment_rule_ifdef(line: &str, line_number: usize, input_name: &str) -> Result<String> {
    let rest = line
        .strip_prefix("#ifdef")
        .ok_or_else(|| BuilderError::new(format!("{input_name}:{line_number}: invalid #ifdef")))?;
    let rest = rest.trim();
    validate(
        is_segment_rule_identifier(rest),
        format!("{input_name}:{line_number}: #ifdef must be followed by one identifier"),
    )?;
    Ok(rest.to_owned())
}

fn segment_rule_ifdefs_active(
    ifdefs_stack: &[(String, bool)],
    active_definitions: &BTreeSet<String>,
) -> bool {
    ifdefs_stack.iter().all(|(name, is_active)| {
        (active_definitions.contains(name) && *is_active)
            || (!active_definitions.contains(name) && !*is_active)
    })
}

fn process_segment_rule_line(
    line_number: usize,
    line: &str,
    defines: &BTreeMap<String, SegmentRuleDefineValue>,
    input_name: &str,
) -> Result<String> {
    if line.trim().is_empty() {
        return Ok(line.to_owned());
    }

    let mut current = line.to_owned();
    for _ in 0..128 {
        let processed = SegmentRuleLineProcessor::new(&current, line_number, defines, input_name)
            .parse_complete_rule()?;
        if processed.trim() == current.trim() {
            return Ok(current);
        }
        current = processed;
    }
    Err(BuilderError::new(format!(
        "{input_name}:{line_number}: recursive segmentation-rule define expansion did not stabilize"
    )))
}

struct SegmentRuleParser<'a, T> {
    line_number: usize,
    line: &'a str,
    cursor: usize,
    segment_types: &'a T,
    input_name: &'a str,
}

impl<'a, T> SegmentRuleParser<'a, T>
where
    T: SegmentTypeLookup,
{
    fn new(line_number: usize, line: &'a str, segment_types: &'a T, input_name: &'a str) -> Self {
        Self {
            line_number,
            line,
            cursor: 0,
            segment_types,
            input_name,
        }
    }

    fn parse_complete_rule(mut self) -> Result<SegmentRule> {
        let mut rule = self.parse_concat_rule(false)?;
        self.skip_whitespace();
        if self.remaining_starts_with_ignore_ascii_case("!weak") {
            self.cursor += "!weak".len();
            rule = rule.set_weak(true);
            self.skip_whitespace();
        }
        validate(
            self.cursor == self.line.len(),
            format!(
                "{}:{}: unexpected token in segmentation rule near {:?}",
                self.input_name,
                self.line_number,
                &self.line[self.cursor..]
            ),
        )?;
        Ok(rule)
    }

    fn parse_concat_rule(&mut self, stop_at_paren: bool) -> Result<SegmentRule> {
        let mut children = Vec::new();
        loop {
            self.skip_whitespace();
            if self.cursor == self.line.len()
                || (stop_at_paren && self.peek_char() == Some(')'))
                || self.remaining_starts_with_ignore_ascii_case("!weak")
            {
                break;
            }
            children.push(self.parse_one_of_rule()?);
        }
        validate(
            !children.is_empty(),
            format!(
                "{}:{}: empty segmentation rule",
                self.input_name, self.line_number
            ),
        )?;
        Ok(if children.len() == 1 {
            children.remove(0)
        } else {
            SegmentRule::concat(children, self.line_number)
        })
    }

    fn parse_one_of_rule(&mut self) -> Result<SegmentRule> {
        let mut children = vec![self.parse_unary_rule()?];
        loop {
            self.skip_whitespace();
            if self.peek_char() != Some('|') {
                break;
            }
            self.cursor += 1;
            children.push(self.parse_unary_rule()?);
        }
        Ok(if children.len() == 1 {
            children.remove(0)
        } else {
            SegmentRule::or(children, self.line_number)
        })
    }

    fn parse_unary_rule(&mut self) -> Result<SegmentRule> {
        let child = self.parse_atomic_rule()?;
        self.skip_whitespace();
        match self.peek_char() {
            Some('*') => {
                self.cursor += 1;
                Ok(SegmentRule::zero_or_more(child, self.line_number))
            }
            Some('+') => {
                self.cursor += 1;
                Ok(SegmentRule::concat(
                    vec![
                        child.clone(),
                        SegmentRule::zero_or_more(child, self.line_number),
                    ],
                    self.line_number,
                ))
            }
            Some('?') => {
                self.cursor += 1;
                Ok(SegmentRule::optional(child, self.line_number))
            }
            Some('{') => self.parse_quantified_rule(child),
            _ => Ok(child),
        }
    }

    fn parse_quantified_rule(&mut self, child: SegmentRule) -> Result<SegmentRule> {
        self.cursor += 1;
        self.skip_whitespace();
        let left = self.read_usize("quantity")?;
        self.skip_whitespace();
        match self.peek_char() {
            Some('}') => {
                self.cursor += 1;
                self.create_quant_rule_exact(child, left)
            }
            Some(',') => {
                self.cursor += 1;
                self.skip_whitespace();
                if self.peek_char() == Some('}') {
                    self.cursor += 1;
                    self.create_quant_rule_open(child, left)
                } else {
                    let right = self.read_usize("right quantity")?;
                    self.skip_whitespace();
                    validate(
                        self.peek_char() == Some('}'),
                        format!(
                            "{}:{}: quantity range must end with '}}'",
                            self.input_name, self.line_number
                        ),
                    )?;
                    self.cursor += 1;
                    self.create_quant_rule_range(child, left, right)
                }
            }
            _ => Err(BuilderError::new(format!(
                "{}:{}: invalid quantity expression",
                self.input_name, self.line_number
            ))),
        }
    }

    fn parse_atomic_rule(&mut self) -> Result<SegmentRule> {
        self.skip_whitespace();
        match self.peek_char() {
            Some('(') => {
                self.cursor += 1;
                let mut rule = self.parse_concat_rule(true)?;
                validate(
                    self.peek_char() == Some(')'),
                    format!(
                        "{}:{}: parenthesized rule must end with ')'",
                        self.input_name, self.line_number
                    ),
                )?;
                self.cursor += 1;
                self.skip_whitespace();
                if self.peek_char() == Some('>') {
                    self.cursor += 1;
                    rule.make_shift_orth_rule();
                }
                Ok(rule)
            }
            Some(ch) if is_rule_tag_start(ch) => {
                let segment_type = self.read_rule_tag().expect("tag start checked");
                self.skip_whitespace();
                let shift_orth = if self.peek_char() == Some('>') {
                    self.cursor += 1;
                    true
                } else {
                    false
                };
                let segment_type_num =
                    self.segment_types
                        .segment_type_num(&segment_type)
                        .map_err(|err| {
                            BuilderError::new(format!(
                                "{}:{}: {}",
                                self.input_name, self.line_number, err
                            ))
                        })?;
                Ok(SegmentRule::tag(
                    segment_type_num,
                    shift_orth,
                    segment_type,
                    self.line_number,
                ))
            }
            Some(ch) => Err(BuilderError::new(format!(
                "{}:{}: unexpected character {:?} in segmentation rule",
                self.input_name, self.line_number, ch
            ))),
            None => Err(BuilderError::new(format!(
                "{}:{}: unexpected end of segmentation rule",
                self.input_name, self.line_number
            ))),
        }
    }

    fn create_quant_rule_exact(&self, child: SegmentRule, quantity: usize) -> Result<SegmentRule> {
        validate(
            quantity > 0,
            format!(
                "{}:{}: {} - invalid quantity: {}",
                self.input_name, self.line_number, self.line, quantity
            ),
        )?;
        Ok(SegmentRule::concat(vec![child; quantity], self.line_number))
    }

    fn create_quant_rule_range(
        &self,
        child: SegmentRule,
        left: usize,
        right: usize,
    ) -> Result<SegmentRule> {
        validate(
            left <= right && (left, right) != (0, 0),
            format!(
                "{}:{}: {} - invalid quantities: {} {}",
                self.input_name, self.line_number, self.line, left, right
            ),
        )?;
        let mut children = Vec::new();
        if left == 0 {
            children.push(SegmentRule::optional(child.clone(), self.line_number));
            for quantity in 2..=right {
                children.push(self.create_quant_rule_exact(child.clone(), quantity)?);
            }
        } else {
            for quantity in left..=right {
                children.push(self.create_quant_rule_exact(child.clone(), quantity)?);
            }
        }
        Ok(SegmentRule::or(children, self.line_number))
    }

    fn create_quant_rule_open(&self, child: SegmentRule, quantity: usize) -> Result<SegmentRule> {
        validate(
            quantity > 0,
            format!(
                "{}:{}: {} - invalid quantity: {}",
                self.input_name, self.line_number, self.line, quantity
            ),
        )?;
        Ok(SegmentRule::concat(
            vec![
                self.create_quant_rule_exact(child.clone(), quantity)?,
                SegmentRule::zero_or_more(child, self.line_number),
            ],
            self.line_number,
        ))
    }

    fn skip_whitespace(&mut self) {
        while let Some(ch) = self.peek_char() {
            if ch.is_whitespace() {
                self.cursor += ch.len_utf8();
            } else {
                break;
            }
        }
    }

    fn read_usize(&mut self, field: &str) -> Result<usize> {
        let start = self.cursor;
        while let Some(ch) = self.peek_char() {
            if ch.is_ascii_digit() {
                self.cursor += ch.len_utf8();
            } else {
                break;
            }
        }
        validate(
            self.cursor > start,
            format!(
                "{}:{}: missing {field} in segmentation rule",
                self.input_name, self.line_number
            ),
        )?;
        self.line[start..self.cursor]
            .parse::<usize>()
            .map_err(|err| {
                BuilderError::new(format!(
                    "{}:{}: invalid {field}: {err}",
                    self.input_name, self.line_number
                ))
            })
    }

    fn peek_char(&self) -> Option<char> {
        self.line[self.cursor..].chars().next()
    }

    fn remaining_starts_with_ignore_ascii_case(&self, needle: &str) -> bool {
        self.line[self.cursor..]
            .get(..needle.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(needle))
    }

    fn read_rule_tag(&mut self) -> Option<String> {
        let mut chars = self.line[self.cursor..].char_indices();
        let (_, first) = chars.next()?;
        if !is_rule_tag_start(first) {
            return None;
        }
        let mut end = first.len_utf8();
        for (index, ch) in chars {
            if is_rule_tag_body(ch) {
                end = index + ch.len_utf8();
            } else {
                break;
            }
        }
        let tag = self.line[self.cursor..self.cursor + end].to_owned();
        self.cursor += end;
        Some(tag)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SegmentRulesNfaState {
    transitions: BTreeMap<Option<SegmentRulesTransitionLabel>, BTreeSet<usize>>,
    final_state: bool,
    weak: bool,
    autogenerated: bool,
    rule_line_number: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SegmentRulesNfa {
    states: Vec<SegmentRulesNfaState>,
    initial_state: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentRulesFsaLayout {
    pub dfs_order: Vec<usize>,
    pub offsets: Vec<usize>,
    pub reverse_offsets: Vec<usize>,
    pub total_size: usize,
}

impl SegmentRulesNfaState {
    fn initial() -> Self {
        Self {
            transitions: BTreeMap::new(),
            final_state: false,
            weak: false,
            autogenerated: false,
            rule_line_number: None,
        }
    }

    fn for_rule(rule: &SegmentRule) -> Self {
        Self {
            transitions: BTreeMap::new(),
            final_state: false,
            weak: false,
            autogenerated: rule.autogenerated,
            rule_line_number: Some(rule.line_number),
        }
    }

    fn final_for_rule(rule: &SegmentRule) -> Self {
        Self {
            transitions: BTreeMap::new(),
            final_state: true,
            weak: rule.weak,
            autogenerated: rule.autogenerated,
            rule_line_number: Some(rule.line_number),
        }
    }
}

impl SegmentRulesNfa {
    fn new() -> Self {
        Self {
            states: vec![SegmentRulesNfaState::initial()],
            initial_state: 0,
        }
    }

    fn add_state(&mut self, state: SegmentRulesNfaState) -> usize {
        let index = self.states.len();
        self.states.push(state);
        index
    }

    fn add_transition(
        &mut self,
        state_index: usize,
        label: Option<SegmentRulesTransitionLabel>,
        target: usize,
    ) {
        self.states[state_index]
            .transitions
            .entry(label)
            .or_default()
            .insert(target);
    }

    fn closure_from(&self, state_index: usize) -> BTreeSet<usize> {
        let mut visited = BTreeSet::new();
        self.add_closure(state_index, &mut visited);
        visited
    }

    fn add_closure(&self, state_index: usize, visited: &mut BTreeSet<usize>) {
        if !visited.insert(state_index) {
            return;
        }
        if let Some(next_states) = self.states[state_index].transitions.get(&None) {
            for &next_state in next_states {
                self.add_closure(next_state, visited);
            }
        }
    }

    fn grouped_output_by_labels(
        &self,
        nfa_states: &BTreeSet<usize>,
    ) -> Vec<(SegmentRulesTransitionLabel, BTreeSet<usize>)> {
        let mut ordered_states: Vec<usize> = nfa_states.iter().copied().collect();
        ordered_states.sort_by(|left, right| {
            self.states[*right]
                .rule_line_number
                .unwrap_or_default()
                .cmp(&self.states[*left].rule_line_number.unwrap_or_default())
                .then(left.cmp(right))
        });

        let mut grouped: Vec<(SegmentRulesTransitionLabel, BTreeSet<usize>)> = Vec::new();
        for state_index in ordered_states {
            for (label, next_states) in &self.states[state_index].transitions {
                let Some(label) = *label else {
                    continue;
                };
                let position = grouped
                    .iter()
                    .position(|(existing_label, _targets)| *existing_label == label);
                let targets = if let Some(position) = position {
                    &mut grouped[position].1
                } else {
                    grouped.push((label, BTreeSet::new()));
                    &mut grouped.last_mut().expect("just pushed label group").1
                };
                for &next_state in next_states {
                    targets.extend(self.closure_from(next_state));
                }
            }
        }
        grouped
    }

    fn convert_to_dfa(&self, input_name: &str) -> Result<SegmentRulesFsa> {
        let start_states = self.closure_from(self.initial_state);
        validate(
            !start_states
                .iter()
                .any(|state_index| self.states[*state_index].final_state),
            "initial segmentation-rule NFA closure must not be accepting",
        )?;

        let mut dfa = SegmentRulesFsa {
            states: Vec::new(),
            initial_state: 0,
        };
        let mut nfa_subset_to_dfa_state = BTreeMap::new();
        let initial_state = self.convert_subset_to_dfa_state(
            &start_states,
            &mut dfa,
            &mut nfa_subset_to_dfa_state,
            input_name,
        )?;
        dfa.initial_state = initial_state;
        Ok(dfa)
    }

    fn convert_subset_to_dfa_state(
        &self,
        nfa_states: &BTreeSet<usize>,
        dfa: &mut SegmentRulesFsa,
        nfa_subset_to_dfa_state: &mut BTreeMap<BTreeSet<usize>, usize>,
        input_name: &str,
    ) -> Result<usize> {
        if let Some(state_index) = nfa_subset_to_dfa_state.get(nfa_states) {
            return Ok(*state_index);
        }

        let (accepting, weak) = self.dfa_state_acceptance(nfa_states, input_name)?;
        let state_index = dfa.states.len();
        nfa_subset_to_dfa_state.insert(nfa_states.clone(), state_index);
        dfa.states.push(SegmentRulesState {
            accepting,
            weak,
            transitions: BTreeMap::new(),
            transition_order: Vec::new(),
        });

        for (label, next_nfa_states) in self.grouped_output_by_labels(nfa_states) {
            let next_dfa_state = self.convert_subset_to_dfa_state(
                &next_nfa_states,
                dfa,
                nfa_subset_to_dfa_state,
                input_name,
            )?;
            if !dfa.states[state_index].transitions.contains_key(&label) {
                dfa.states[state_index].transition_order.push(label);
            }
            dfa.states[state_index]
                .transitions
                .insert(label, next_dfa_state);
        }
        Ok(state_index)
    }

    fn dfa_state_acceptance(
        &self,
        nfa_states: &BTreeSet<usize>,
        input_name: &str,
    ) -> Result<(bool, bool)> {
        let weak_hits: Vec<(bool, usize)> = nfa_states
            .iter()
            .filter_map(|state_index| {
                let state = &self.states[*state_index];
                (state.final_state && !state.autogenerated).then(|| {
                    (
                        state.weak,
                        state
                            .rule_line_number
                            .expect("final rule state has source line"),
                    )
                })
            })
            .collect();

        if weak_hits.iter().any(|(weak, _line)| *weak)
            && !weak_hits.iter().all(|(weak, _line)| *weak)
        {
            let weak_line = weak_hits
                .iter()
                .find_map(|(weak, line)| (*weak).then_some(*line))
                .expect("weak hit exists");
            let non_weak_line = weak_hits
                .iter()
                .find_map(|(weak, line)| (!*weak).then_some(*line))
                .expect("non-weak hit exists");
            return Err(BuilderError::new(format!(
                "{input_name}:{weak_line} - conflicts with rule at line {non_weak_line}. Segmentation for some chunks can be both weak and non-weak which is illegal."
            )));
        }

        let accepting = nfa_states
            .iter()
            .any(|state_index| self.states[*state_index].final_state);
        let weak = nfa_states.iter().any(|state_index| {
            let state = &self.states[*state_index];
            state.final_state && state.weak && !state.autogenerated
        });
        Ok((accepting, weak))
    }
}

struct SegmentRuleLineProcessor<'a> {
    line: &'a str,
    cursor: usize,
    line_number: usize,
    defines: &'a BTreeMap<String, SegmentRuleDefineValue>,
    input_name: &'a str,
}

impl<'a> SegmentRuleLineProcessor<'a> {
    fn new(
        line: &'a str,
        line_number: usize,
        defines: &'a BTreeMap<String, SegmentRuleDefineValue>,
        input_name: &'a str,
    ) -> Self {
        Self {
            line,
            cursor: 0,
            line_number,
            defines,
            input_name,
        }
    }

    fn parse_complete_rule(mut self) -> Result<String> {
        let rule = self.parse_rule(false)?;
        self.skip_whitespace();
        validate(
            self.cursor == self.line.len(),
            format!(
                "{}:{}: unexpected token in segmentation rule near {:?}",
                self.input_name,
                self.line_number,
                &self.line[self.cursor..]
            ),
        )?;
        Ok(rule)
    }

    fn parse_rule(&mut self, stop_at_paren: bool) -> Result<String> {
        let mut tokens = Vec::new();
        loop {
            self.skip_whitespace();
            if self.cursor == self.line.len() || (stop_at_paren && self.peek_char() == Some(')')) {
                break;
            }
            tokens.push(self.parse_token()?);
        }
        Ok(tokens.join(" "))
    }

    fn parse_token(&mut self) -> Result<String> {
        if self.line[self.cursor..]
            .to_ascii_lowercase()
            .starts_with("!weak")
        {
            self.cursor += "!weak".len();
            return Ok("!weak".to_owned());
        }

        match self.peek_char() {
            Some('(') => {
                self.cursor += 1;
                let inner = self.parse_rule(true)?;
                validate(
                    self.peek_char() == Some(')'),
                    format!(
                        "{}:{}: unterminated parenthesized segmentation rule",
                        self.input_name, self.line_number
                    ),
                )?;
                self.cursor += 1;
                Ok(format!("( {inner} )"))
            }
            Some(ch) if is_segment_rule_operator(ch) => Ok(self.read_operator_word()),
            Some(ch) if is_segment_rule_identifier_start(ch) => {
                let name = self.read_identifier().expect("identifier start checked");
                self.skip_whitespace();
                if self.peek_char() == Some('(') {
                    self.cursor += 1;
                    let substitute_value = self.parse_rule(true)?;
                    validate(
                        self.peek_char() == Some(')'),
                        format!(
                            "{}:{}: unterminated define invocation",
                            self.input_name, self.line_number
                        ),
                    )?;
                    self.cursor += 1;
                    Ok(self.substitute_arg_define(&name, &substitute_value))
                } else {
                    Ok(self.substitute_non_arg_define(&name))
                }
            }
            Some(ch) => Err(BuilderError::new(format!(
                "{}:{}: unexpected character {:?} in segmentation rule",
                self.input_name, self.line_number, ch
            ))),
            None => Err(BuilderError::new(format!(
                "{}:{}: unexpected end of segmentation rule",
                self.input_name, self.line_number
            ))),
        }
    }

    fn substitute_arg_define(&self, name: &str, substitute_value: &str) -> String {
        match self.defines.get(name) {
            Some(SegmentRuleDefineValue::WithArg { arg, value }) => {
                replace_ascii_word(value, arg, substitute_value)
            }
            Some(SegmentRuleDefineValue::WithoutArg(value)) => {
                format!("{value} ( {substitute_value} )")
            }
            None => format!("{name} ( {substitute_value} )"),
        }
    }

    fn substitute_non_arg_define(&self, name: &str) -> String {
        match self.defines.get(name) {
            Some(SegmentRuleDefineValue::WithoutArg(value)) => value.clone(),
            _ => name.to_owned(),
        }
    }

    fn skip_whitespace(&mut self) {
        while let Some(ch) = self.peek_char() {
            if ch.is_whitespace() {
                self.cursor += ch.len_utf8();
            } else {
                break;
            }
        }
    }

    fn peek_char(&self) -> Option<char> {
        self.line[self.cursor..].chars().next()
    }

    fn read_identifier(&mut self) -> Option<String> {
        let (identifier, len) = read_segment_rule_identifier(&self.line[self.cursor..])?;
        self.cursor += len;
        Some(identifier)
    }

    fn read_operator_word(&mut self) -> String {
        let start = self.cursor;
        while let Some(ch) = self.peek_char() {
            if is_segment_rule_operator(ch) {
                self.cursor += ch.len_utf8();
            } else {
                break;
            }
        }
        self.line[start..self.cursor].to_owned()
    }
}

fn read_segment_rule_identifier(input: &str) -> Option<(String, usize)> {
    let mut chars = input.char_indices();
    let (_, first) = chars.next()?;
    if !is_segment_rule_identifier_start(first) {
        return None;
    }
    let mut end = first.len_utf8();
    for (index, ch) in chars {
        if is_segment_rule_identifier_body(ch) {
            end = index + ch.len_utf8();
        } else {
            break;
        }
    }
    Some((input[..end].to_owned(), end))
}

fn is_segment_rule_identifier(input: &str) -> bool {
    read_segment_rule_identifier(input)
        .map(|(_identifier, len)| len == input.len())
        .unwrap_or(false)
}

fn is_segment_rule_identifier_start(ch: char) -> bool {
    ch.is_ascii_alphabetic()
}

fn is_segment_rule_identifier_body(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '>' | '*' | '+' | '{' | '}' | ',')
}

fn is_segment_rule_operator(ch: char) -> bool {
    matches!(ch, '*' | '|' | '+' | '?' | '>')
}

fn is_rule_tag_start(ch: char) -> bool {
    is_rule_tag_body(ch)
}

fn is_rule_tag_body(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

fn replace_ascii_word(input: &str, word: &str, replacement: &str) -> String {
    if word.is_empty() {
        return input.to_owned();
    }
    let mut out = String::with_capacity(input.len());
    let mut cursor = 0usize;
    while let Some(relative) = input[cursor..].find(word) {
        let start = cursor + relative;
        let end = start + word.len();
        if is_ascii_word_start_boundary(input, start) && is_ascii_word_end_boundary(input, end) {
            out.push_str(&input[cursor..start]);
            out.push_str(replacement);
            cursor = end;
        } else {
            out.push_str(&input[cursor..end]);
            cursor = end;
        }
    }
    out.push_str(&input[cursor..]);
    out
}

fn is_ascii_word_start_boundary(input: &str, index: usize) -> bool {
    if index == 0 {
        true
    } else {
        !is_ascii_regex_word_byte(input.as_bytes()[index - 1])
    }
}

fn is_ascii_word_end_boundary(input: &str, index: usize) -> bool {
    if index == input.len() {
        true
    } else {
        !is_ascii_regex_word_byte(input.as_bytes()[index])
    }
}

fn is_ascii_regex_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedInputLine {
    orth: String,
    base: String,
    tag: String,
    name: String,
    qualifier: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PartialAnalyzerLine<K> {
    sort_key: K,
    index: usize,
    orth: String,
    base: String,
    tag_num: usize,
    name_num: usize,
    segment_type_num: usize,
    qualifiers_num: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PartialGeneratorLine<K> {
    sort_key: K,
    legacy_line: String,
    orth: String,
    base: String,
    tag_num: usize,
    name_num: usize,
    segment_type_num: usize,
    homonym_id: String,
    qualifiers_num: usize,
}

impl<K> PartialGeneratorLine<K> {
    fn new(
        orth: &str,
        base: &str,
        tag_num: usize,
        name_num: usize,
        segment_type_num: usize,
        homonym_id: &str,
        qualifiers_num: usize,
        sort_key: K,
    ) -> Self {
        Self {
            sort_key,
            legacy_line: format!(
                "{orth}\t{base}\t{tag_num}\t{name_num}\t{segment_type_num}\t{homonym_id}\t{qualifiers_num}"
            ),
            orth: orth.to_owned(),
            base: base.to_owned(),
            tag_num,
            name_num,
            segment_type_num,
            homonym_id: homonym_id.to_owned(),
            qualifiers_num,
        }
    }
}

#[derive(Debug, Clone, Default)]
struct LineParser {
    in_copyright: bool,
}

impl LineParser {
    fn new() -> Self {
        Self::default()
    }

    fn ignore_line(&mut self, line: &str) -> bool {
        if line.is_empty() {
            true
        } else if line.trim() == "#<COPYRIGHT>" {
            self.in_copyright = true;
            true
        } else if line.trim() == "#</COPYRIGHT>" {
            self.in_copyright = false;
            true
        } else if self.in_copyright {
            true
        } else if !first_two_tab_fields(line).contains(' ') {
            false
        } else {
            true
        }
    }

    fn parse_line(&self, line: &str) -> Result<ParsedInputLine> {
        let fields: Vec<&str> = line.trim().split('\t').collect();
        match fields.as_slice() {
            [_orth, _base, _tag] => Ok(ParsedInputLine {
                orth: (*_orth).to_owned(),
                base: (*_base).to_owned(),
                tag: (*_tag).to_owned(),
                name: String::new(),
                qualifier: String::new(),
            }),
            [_orth, _base, _tag, name] => Ok(ParsedInputLine {
                orth: (*_orth).to_owned(),
                base: (*_base).to_owned(),
                tag: (*_tag).to_owned(),
                name: (*name).to_owned(),
                qualifier: String::new(),
            }),
            [_orth, _base, _tag, name, qualifier] => Ok(ParsedInputLine {
                orth: (*_orth).to_owned(),
                base: (*_base).to_owned(),
                tag: (*_tag).to_owned(),
                name: (*name).to_owned(),
                qualifier: (*qualifier).to_owned(),
            }),
            _ => Err(BuilderError::new(format!(
                "input line \"{line}\" does not have 3, 4 or 5 tab-separated fields"
            ))),
        }
    }
}

fn first_two_tab_fields(line: &str) -> String {
    line.split('\t').take(2).collect()
}

fn strip_python_newline(line: &str) -> String {
    line.trim_matches('\n').to_owned()
}

fn merge_analyzer_entries(
    keyed_interpretations: Vec<(String, AnalyzerInterpretation)>,
    lowercase: bool,
) -> Result<Vec<AnalyzerEntry>> {
    let merged = merge_entries(
        keyed_interpretations,
        lowercase,
        dedup_analyzer_interpretations,
    )?;
    Ok(merged
        .into_iter()
        .map(|(key, interpretations)| AnalyzerEntry {
            key,
            interpretations,
        })
        .collect())
}

fn merge_generator_entries(
    keyed_interpretations: Vec<(String, GeneratorInterpretation)>,
    lowercase: bool,
) -> Result<Vec<GeneratorEntry>> {
    let merged = merge_entries(
        keyed_interpretations,
        lowercase,
        dedup_generator_interpretations,
    )?;
    Ok(merged
        .into_iter()
        .map(|(key, interpretations)| GeneratorEntry {
            key,
            interpretations,
        })
        .collect())
}

fn merge_entries<T, F>(
    keyed_interpretations: Vec<(String, T)>,
    lowercase: bool,
    dedup: F,
) -> Result<Vec<(String, Vec<T>)>>
where
    F: Fn(Vec<T>) -> Vec<T>,
{
    let mut result = Vec::new();
    let mut prev_key = None;
    let mut prev_interpretations = Vec::new();

    for (raw_key, interpretation) in keyed_interpretations {
        let key = if lowercase {
            raw_key.to_lowercase()
        } else {
            raw_key
        };
        validate(!key.is_empty(), "entry key must not be empty")?;

        if prev_key.as_deref() == Some(key.as_str()) {
            prev_interpretations.push(interpretation);
        } else {
            if let Some(prev_key) = prev_key.replace(key) {
                result.push((prev_key, dedup(prev_interpretations)));
                prev_interpretations = Vec::new();
            }
            prev_interpretations.push(interpretation);
        }
    }

    if let Some(prev_key) = prev_key {
        result.push((prev_key, dedup(prev_interpretations)));
    }

    Ok(result)
}

fn dedup_analyzer_interpretations(
    interpretations: Vec<AnalyzerInterpretation>,
) -> Vec<AnalyzerInterpretation> {
    let mut by_sort_key = BTreeMap::new();
    for interpretation in interpretations {
        by_sort_key
            .entry(interpretation.sort_key())
            .or_insert(interpretation);
    }
    by_sort_key.into_values().collect()
}

fn dedup_generator_interpretations(
    interpretations: Vec<GeneratorInterpretation>,
) -> Vec<GeneratorInterpretation> {
    let mut by_sort_key = BTreeMap::new();
    for interpretation in interpretations {
        by_sort_key
            .entry(interpretation.sort_key())
            .or_insert(interpretation);
    }
    by_sort_key.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_dict_id_and_copyright() {
        let metadata = read_metadata_from_str(
            "dict.tab",
            "#!DICT-ID sgjp\n#<COPYRIGHT>\nCopyright line 1\nCopyright line 2\n#</COPYRIGHT>\n",
        )
        .unwrap();

        assert_eq!(
            metadata,
            DictionaryMetadata {
                dict_id: "sgjp".to_owned(),
                copyright: "Copyright line 1\nCopyright line 2\n".to_owned(),
            }
        );
    }

    #[test]
    fn defaults_missing_metadata_to_empty_strings() {
        let metadata = read_metadata_from_str("dict.tab", "kot\tkot\tsubst\n").unwrap();

        assert_eq!(metadata.dict_id, "");
        assert_eq!(metadata.copyright, "");
    }

    #[test]
    fn keeps_first_dict_id_across_inputs() {
        let metadata = merge_metadata([
            ("first.tab", "#!DICT-ID first\n"),
            ("second.tab", "#!DICT-ID second\n"),
        ])
        .unwrap();

        assert_eq!(metadata.dict_id, "first");
    }

    #[test]
    fn rejects_dict_id_without_value() {
        let error = read_metadata_from_str("dict.tab", "#!DICT-ID\n").unwrap_err();

        assert_eq!(error.to_string(), "dict.tab:1: Must provide DICT-ID");
    }

    #[test]
    fn rejects_dict_id_tag_without_space_separator() {
        let error = read_metadata_from_str("dict.tab", "#!DICT-ID\tmain\n").unwrap_err();

        assert_eq!(
            error.to_string(),
            "Dictionary ID tag must be followed by a space character and dictionary ID string"
        );
    }

    #[test]
    fn accepts_legacy_empty_dict_id_after_space() {
        let metadata = read_metadata_from_str("dict.tab", "#!DICT-ID \n").unwrap();

        assert_eq!(metadata.dict_id, "");
    }

    #[test]
    fn rejects_dict_id_containing_spaces() {
        let error = read_metadata_from_str("dict.tab", "#!DICT-ID sgjp main\n").unwrap_err();

        assert_eq!(
            error.to_string(),
            "dict.tab:1: DICT-ID must not contain spaces"
        );
    }

    #[test]
    fn rejects_copyright_start_with_extra_text() {
        let error = read_metadata_from_str("dict.tab", "#<COPYRIGHT> extra\n").unwrap_err();

        assert_eq!(
            error.to_string(),
            "dict.tab:1: Copyright start tag must be the only one in the line"
        );
    }

    #[test]
    fn rejects_copyright_end_without_start() {
        let error = read_metadata_from_str("dict.tab", "#</COPYRIGHT>\n").unwrap_err();

        assert_eq!(
            error.to_string(),
            "dict.tab:1: Copyright end tag must be preceded by copyright start tag"
        );
    }

    #[test]
    fn rejects_copyright_end_with_extra_text() {
        let error = read_metadata_from_str("dict.tab", "#<COPYRIGHT>\ntext\n#</COPYRIGHT> extra\n")
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "dict.tab:3: Copyright end tag must be the only one in the line"
        );
    }

    #[test]
    fn parses_tagset_with_legacy_tag_indexes_and_insertion_order() {
        let tagset = Tagset::from_str(
            "sample.tagset",
            "#!TAGSET-ID   sgjp\n# comment\n\n[TAGS]\n2\tsubst\n0\tadj\n",
        )
        .unwrap();

        assert_eq!(tagset.tagset_id.as_deref(), Some("sgjp"));
        assert_eq!(tagset.all_tags(), &["subst".to_owned(), "adj".to_owned()]);
        assert_eq!(tagset.tag_num_for_tag("subst").unwrap(), 2);
        assert_eq!(tagset.tag_for_tag_num(0).unwrap(), "adj");
        assert_eq!(TagsetLookup::tag_num(&tagset, "adj").unwrap(), 0);
    }

    #[test]
    fn parses_empty_tagset_id_like_python_regex() {
        let tagset = Tagset::from_str("sample.tagset", "#!TAGSET-ID   \n[TAGS]\n").unwrap();

        assert_eq!(tagset.tagset_id.as_deref(), Some(""));
    }

    #[test]
    fn rejects_missing_tagset_id_in_first_line() {
        let error = Tagset::from_str("sample.tagset", "#!MORFEUSZ-TAGSET 0.1\n[TAGS]\n0\ttag\n")
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "missing TAGSET-ID in first line of tagset file"
        );
    }

    #[test]
    fn rejects_text_outside_tags_section() {
        let error = Tagset::from_str("sample.tagset", "#!TAGSET-ID x\n0\ttag\n").unwrap_err();

        assert_eq!(
            error.to_string(),
            "\"0\ttag\" - text outside [TAGS] section in tagset file line 2"
        );
    }

    #[test]
    fn rejects_invalid_tagset_line_shape() {
        let error = Tagset::from_str("sample.tagset", "#!TAGSET-ID x\n[TAGS]\n0\ttag\textra\n")
            .unwrap_err();

        assert_eq!(error.to_string(), "\"0\ttag\textra\" - invalid line 3");
    }

    #[test]
    fn rejects_duplicate_tag() {
        let error = Tagset::from_str("sample.tagset", "#!TAGSET-ID x\n[TAGS]\n0\ttag\n1\ttag\n")
            .unwrap_err();

        assert_eq!(error.to_string(), "duplicate tag: \"tag\"");
    }

    #[test]
    fn rejects_duplicate_tag_id() {
        let error = Tagset::from_str("sample.tagset", "#!TAGSET-ID x\n[TAGS]\n0\ttag\n0\tother\n")
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "line 4: tagId 0 assigned for tag \"other\" already appeared somewhere else."
        );
    }

    #[test]
    fn rejects_invalid_tag_lookup() {
        let tagset = Tagset::from_str("sample.tagset", "#!TAGSET-ID x\n[TAGS]\n0\ttag\n").unwrap();

        assert_eq!(
            tagset.tag_num_for_tag("missing").unwrap_err().to_string(),
            "invalid tag: \"missing\""
        );
        assert_eq!(
            tagset.tag_for_tag_num(9).unwrap_err().to_string(),
            "invalid tag id: 9"
        );
    }

    #[test]
    fn serializes_legacy_numbers_strings_and_prologue() {
        assert_eq!(serialize_u16_be(0x1234).unwrap(), [0x12, 0x34]);
        assert_eq!(
            serialize_u32_be(0x1234_5678).unwrap(),
            [0x12, 0x34, 0x56, 0x78]
        );
        assert_eq!(serialize_legacy_string("zazolc"), b"zazolc\0");
        assert_eq!(serialize_prologue(2), vec![0x8f, 0xc2, 0xbc, 0x1b, 21, 2]);
    }

    #[test]
    fn rejects_legacy_number_overflow() {
        assert_eq!(
            serialize_u16_be(65_536).unwrap_err().to_string(),
            "value 65536 does not fit into uint16"
        );
        assert_eq!(
            serialize_u32_be(u32::MAX as usize + 1)
                .unwrap_err()
                .to_string(),
            "value 4294967296 does not fit into uint32"
        );
    }

    #[test]
    fn serializes_tags_map_like_legacy_serializer() {
        let tags = BTreeMap::from([("b".to_owned(), 2), ("a".to_owned(), 1)]);

        assert_eq!(
            hex(&serialize_tags_map(&tags, &Utf8WordEncoder).unwrap()),
            "00020001610000026200"
        );
    }

    #[test]
    fn serializes_qualifiers_map_like_legacy_serializer() {
        let qualifiers_map = BTreeMap::from([
            (qualifiers([]), 0),
            (qualifiers(["x"]), 1),
            (qualifiers(["arch", "rare"]), 2),
        ]);

        assert_eq!(
            hex(&serialize_qualifiers_map(&qualifiers_map, &Utf8WordEncoder).unwrap()),
            "0003000000000178000002617263687c7261726500"
        );
    }

    #[test]
    fn serializes_tagset_data_like_legacy_serializer() {
        let tagset =
            Tagset::from_str("sample.tagset", "#!TAGSET-ID tid\n[TAGS]\n2\tb\n1\ta\n").unwrap();
        let names = BTreeMap::from([(String::new(), 0), ("name".to_owned(), 1)]);

        assert_eq!(
            hex(&serialize_tagset_data(&tagset, &names, &Utf8WordEncoder).unwrap()),
            "7469640000020001610000026200000200000000016e616d6500"
        );
    }

    #[test]
    fn serializes_epilogue_like_legacy_serializer() {
        let tagset =
            Tagset::from_str("sample.tagset", "#!TAGSET-ID tid\n[TAGS]\n2\tb\n1\ta\n").unwrap();
        let names = BTreeMap::from([(String::new(), 0), ("name".to_owned(), 1)]);
        let tagset_data = serialize_tagset_data(&tagset, &names, &Utf8WordEncoder).unwrap();
        let qualifiers_map = BTreeMap::from([
            (qualifiers([]), 0),
            (qualifiers(["x"]), 1),
            (qualifiers(["arch", "rare"]), 2),
        ]);
        let qualifiers_data = serialize_qualifiers_map(&qualifiers_map, &Utf8WordEncoder).unwrap();

        assert_eq!(
            hex(&serialize_epilogue(
                "dict",
                "copy",
                &tagset_data,
                &qualifiers_data,
                &[1, 2, 3],
            )
            .unwrap()),
            "000000396469637400636f7079007469640000020001610000026200000200000000016e616d65000003000000000178000002617263687c7261726500010203"
        );
    }

    #[test]
    fn serializes_simple_state_like_legacy_serializer() {
        let state = simple_oracle_state();
        let global = simple_global_frequencies();

        assert_eq!(simple_implementation_code(false), 0);
        assert_eq!(simple_state_size(&state, false).unwrap(), 15);
        assert_eq!(hex(&serialize_simple_state_data(&state).unwrap()), "83aabb");
        assert_eq!(
            hex(&serialize_simple_transitions(&state, false, &global).unwrap()),
            "620001026101020363000001"
        );
        assert_eq!(
            hex(&serialize_simple_state(&state, false, &global).unwrap()),
            "83aabb620001026101020363000001"
        );
    }

    #[test]
    fn serializes_simple_state_with_transition_data_like_legacy_serializer() {
        let state = simple_oracle_state();
        let global = simple_global_frequencies();

        assert_eq!(simple_implementation_code(true), 128);
        assert_eq!(simple_state_size(&state, true).unwrap(), 18);
        assert_eq!(
            hex(&serialize_simple_transitions(&state, true, &global).unwrap()),
            "620001020861010203096300000107"
        );
        assert_eq!(
            hex(&serialize_simple_state(&state, true, &global).unwrap()),
            "83aabb620001020861010203096300000107"
        );
    }

    #[test]
    fn serializes_non_accepting_simple_state_with_transition() {
        let state = SimpleState::non_accepting().with_transition(b'x', 0x00000a, None);

        assert_eq!(
            hex(&serialize_simple_state(&state, false, &BTreeMap::new()).unwrap()),
            "017800000a"
        );
    }

    #[test]
    fn rejects_invalid_simple_states_and_offsets() {
        assert_eq!(
            serialize_simple_state_data(&SimpleState::non_accepting())
                .unwrap_err()
                .to_string(),
            "simple state must be accepting or have transitions"
        );

        let too_many = (0..128).fold(SimpleState::non_accepting(), |state, index| {
            state.with_transition(index as u8, 0, None)
        });
        assert_eq!(
            serialize_simple_state_data(&too_many)
                .unwrap_err()
                .to_string(),
            "simple state has too many transitions: 128"
        );

        let too_large_offset =
            SimpleState::non_accepting().with_transition(b'a', 256 * 256 * 256, None);
        assert_eq!(
            serialize_simple_transitions(&too_large_offset, false, &BTreeMap::new())
                .unwrap_err()
                .to_string(),
            "simple transition offset 16777216 exceeds 24-bit limit"
        );

        let missing_data = SimpleState::non_accepting().with_transition(b'a', 1, None);
        assert_eq!(
            serialize_simple_transitions(&missing_data, true, &BTreeMap::new())
                .unwrap_err()
                .to_string(),
            "missing transition data for label 97"
        );
    }

    #[test]
    fn calculates_simple_graph_offsets_like_legacy_state_dfs() {
        let graph = simple_oracle_graph(false);
        let layout = calculate_simple_graph_layout(&graph, false).unwrap();

        assert_eq!(layout.dfs_order, vec![3, 2, 1, 0]);
        assert_eq!(layout.offsets, vec![0, 9, 14, 19]);
        assert_eq!(layout.reverse_offsets, vec![22, 13, 8, 3]);
        assert_eq!(layout.total_size, 22);
    }

    #[test]
    fn serializes_simple_fsa_data_like_legacy_serializer() {
        let graph = simple_oracle_graph(false);

        assert_eq!(
            hex(&serialize_simple_fsa_data(&graph, false).unwrap()),
            "02610000096200000e0178000013016300001380dead"
        );
    }

    #[test]
    fn serializes_simple_fsa_data_with_transition_data_like_legacy_serializer() {
        let graph = simple_oracle_graph(true);
        let layout = calculate_simple_graph_layout(&graph, true).unwrap();

        assert_eq!(layout.dfs_order, vec![3, 2, 1, 0]);
        assert_eq!(layout.offsets, vec![0, 11, 17, 23]);
        assert_eq!(layout.reverse_offsets, vec![26, 15, 9, 3]);
        assert_eq!(layout.total_size, 26);
        assert_eq!(
            hex(&serialize_simple_fsa_data(&graph, true).unwrap()),
            "026100000b09620000110801780000170301630000170480dead"
        );
    }

    #[test]
    fn serializes_full_simple_dictionary_like_legacy_serializer() {
        let (tagset, names, qualifiers) = simple_dictionary_metadata();

        assert_eq!(
            hex(&serialize_simple_dictionary(
                &simple_oracle_graph(false),
                false,
                "dict",
                "copy",
                &tagset,
                &names,
                &qualifiers,
                &[1, 2, 3],
                &Utf8WordEncoder,
            )
            .unwrap()),
            "8fc2bc1b15000000001602610000096200000e0178000013016300001380dead000000396469637400636f7079007469640000020001610000026200000200000000016e616d65000003000000000178000002617263687c7261726500010203"
        );
    }

    #[test]
    fn serializes_full_simple_dictionary_with_transition_data_like_legacy_serializer() {
        let (tagset, names, qualifiers) = simple_dictionary_metadata();

        assert_eq!(
            hex(&serialize_simple_dictionary(
                &simple_oracle_graph(true),
                true,
                "dict",
                "copy",
                &tagset,
                &names,
                &qualifiers,
                &[1, 2, 3],
                &Utf8WordEncoder,
            )
            .unwrap()),
            "8fc2bc1b15800000001a026100000b09620000110801780000170301630000170480dead000000396469637400636f7079007469640000020001610000026200000200000000016e616d65000003000000000178000002617263687c7261726500010203"
        );
    }

    #[test]
    fn builds_minimized_simple_fsa_from_sorted_entries_like_legacy_builder() {
        let graph = build_simple_fsa_from_sorted_entries(constructed_simple_entries()).unwrap();

        assert_eq!(
            graph.global_label_frequencies,
            BTreeMap::from([(b'a', 2), (b'b', 4)])
        );
        assert_eq!(
            hex(&serialize_simple_fsa_data(&graph, false).unwrap()),
            "02620000096100000f8103620000158101620000158002"
        );
    }

    #[test]
    fn builds_full_simple_dictionary_from_sorted_entries_like_legacy_builder() {
        let graph = build_simple_fsa_from_sorted_entries(constructed_simple_entries()).unwrap();
        let tagset =
            Tagset::from_str("sample.tagset", "#!TAGSET-ID tid\n[TAGS]\n0\ttag\n").unwrap();
        let names = BTreeMap::from([(String::new(), 0)]);
        let qualifiers = BTreeMap::from([(qualifiers([]), 0)]);

        assert_eq!(
            hex(&serialize_simple_dictionary(
                &graph,
                false,
                "dict",
                "copy",
                &tagset,
                &names,
                &qualifiers,
                &[],
                &Utf8WordEncoder,
            )
            .unwrap()),
            "8fc2bc1b15000000001702620000096100000f8103620000158101620000158002000000206469637400636f70790074696400000100007461670000010000000001000000"
        );
    }

    #[test]
    fn rejects_invalid_sorted_fsa_inputs_like_legacy_assertions() {
        assert_eq!(
            build_simple_fsa_from_sorted_entries(Vec::<(Vec<u8>, Vec<u8>)>::new())
                .unwrap_err()
                .to_string(),
            "empty input"
        );
        assert_eq!(
            build_simple_fsa_from_sorted_entries(vec![(Vec::new(), vec![1])])
                .unwrap_err()
                .to_string(),
            "entry word must not be empty"
        );
        assert_eq!(
            build_simple_fsa_from_sorted_entries(vec![
                (b"b".to_vec(), vec![1]),
                (b"a".to_vec(), vec![2]),
            ])
            .unwrap_err()
            .to_string(),
            "input entries must be strictly sorted by encoded word"
        );
        assert_eq!(
            build_simple_fsa_from_sorted_entries(vec![
                (b"a".to_vec(), vec![1]),
                (b"a".to_vec(), vec![2]),
            ])
            .unwrap_err()
            .to_string(),
            "input entries must be strictly sorted by encoded word"
        );
    }

    #[test]
    fn serializes_analyzer_entry_payload_like_legacy_morph_encoder() {
        let entry = AnalyzerEntry {
            key: "kot".to_owned(),
            interpretations: vec![
                AnalyzerInterpretation::new("Kot", "kot", 11, 1, 1, 4).unwrap(),
                AnalyzerInterpretation::new("Kot", "Kot", 10, 1, 2, 3).unwrap(),
            ],
        };

        assert_eq!(
            hex(&serialize_analyzer_entry_payload(&entry).unwrap()),
            "0016010008600000000b010004020008500000000a010003"
        );
    }

    #[test]
    fn serializes_analyzer_mixed_case_payload_like_legacy_morph_encoder() {
        let entry = AnalyzerEntry {
            key: "abcde".to_owned(),
            interpretations: vec![
                AnalyzerInterpretation::new("AbCde", "Xy", 513, 2, 7, 9).unwrap(),
                AnalyzerInterpretation::new("AbCde", "ABcxy", 514, 3, 7, 10).unwrap(),
            ],
        };

        assert_eq!(
            hex(&serialize_analyzer_entry_payload(&entry).unwrap()),
            "001f07001c000002020002027879000102020203000a0005587900000201020009"
        );
    }

    #[test]
    fn serializes_generator_entry_payload_like_legacy_generator_encoder() {
        let entry = GeneratorEntry {
            key: "kot".to_owned(),
            interpretations: vec![
                GeneratorInterpretation::new("przedkotami", "kot", 513, 2, 7, "h", 9).unwrap(),
                GeneratorInterpretation::new("koty", "kot", 514, 3, 7, "", 10).unwrap(),
            ],
        };

        assert_eq!(
            hex(&serialize_generator_entry_payload(&entry).unwrap()),
            "002207001f0000007900020203000a6800000370727a65646b6f74616d69000201020009"
        );
    }

    #[test]
    fn converts_analyzer_entries_to_sorted_simple_fsa_entries() {
        let entries = vec![
            AnalyzerEntry {
                key: "a".to_owned(),
                interpretations: vec![AnalyzerInterpretation::new("a", "a", 1, 0, 1, 0).unwrap()],
            },
            AnalyzerEntry {
                key: "b".to_owned(),
                interpretations: vec![AnalyzerInterpretation::new("b", "b", 2, 0, 1, 0).unwrap()],
            },
        ];

        let fsa_entries =
            analyzer_entries_to_sorted_fsa_entries(&entries, &Utf8WordEncoder).unwrap();
        assert_eq!(fsa_entries[0].0, b"a");
        assert_eq!(hex(&fsa_entries[0].1), "000b010008a000000001000000");
        assert_eq!(fsa_entries[1].0, b"b");

        let graph = build_analyzer_simple_fsa_from_entries(&entries, &Utf8WordEncoder).unwrap();
        assert_eq!(
            graph.global_label_frequencies,
            BTreeMap::from([(b'a', 1), (b'b', 1)])
        );
    }

    #[test]
    fn builds_simple_dictionary_from_analyzer_entries() {
        let entries = vec![AnalyzerEntry {
            key: "a".to_owned(),
            interpretations: vec![AnalyzerInterpretation::new("a", "a", 1, 0, 1, 0).unwrap()],
        }];
        let tagset =
            Tagset::from_str("sample.tagset", "#!TAGSET-ID tid\n[TAGS]\n1\ttag\n").unwrap();
        let names = BTreeMap::from([(String::new(), 0)]);
        let qualifiers = BTreeMap::from([(qualifiers([]), 0)]);

        let bytes = build_analyzer_simple_dictionary_from_entries(
            &entries,
            "dict",
            "copy",
            &tagset,
            &names,
            &qualifiers,
            &[1, 2, 3],
            &Utf8WordEncoder,
        )
        .unwrap();

        assert!(bytes.starts_with(&[0x8f, 0xc2, 0xbc, 0x1b, DICTIONARY_VERSION, 0x00]));
        assert!(hex(&bytes).contains("6469637400636f70790074696400"));
    }

    #[test]
    fn builds_simple_dictionaries_from_source_strings() {
        let dictionary =
            "#!DICT-ID dict\n#<COPYRIGHT>\ncopy\n#</COPYRIGHT>\nKot\tkot\ttag\tname\tq\n";
        let tagset = "#!TAGSET-ID tid\n[TAGS]\n0\tign\n1\tsp\n10\ttag\n";
        let segmentation = "[options]\n\
aggl = isolated\n\
praet = split\n\
[combinations]\n\
A\n\
[tags]\n\
A %\n\
[lexemes]\n\
[segment types]\n\
A\n\
[separator chars]\n";

        let analyzer = build_analyzer_simple_dictionary_from_str(
            "dict.tab",
            dictionary,
            "tagset.dat",
            tagset,
            "segmenty.dat",
            segmentation,
        )
        .unwrap();
        let generator = build_generator_simple_dictionary_from_str(
            "dict.tab",
            dictionary,
            "tagset.dat",
            tagset,
            "segmenty.dat",
            segmentation,
        )
        .unwrap();

        assert!(analyzer.starts_with(&[0x8f, 0xc2, 0xbc, 0x1b, DICTIONARY_VERSION, 0x00]));
        assert!(generator.starts_with(&[0x8f, 0xc2, 0xbc, 0x1b, DICTIONARY_VERSION, 0x00]));
        assert!(hex(&analyzer).contains("6469637400636f70790a0074696400"));
        assert!(hex(&generator).contains("6469637400636f70790a0074696400"));
    }

    #[test]
    fn rejects_empty_source_dictionary_builds() {
        let error = build_analyzer_simple_dictionary_from_sources(
            std::iter::empty(),
            "tagset.dat",
            "",
            "segmenty.dat",
            "",
        )
        .unwrap_err();

        assert_eq!(error.to_string(), "dictionary sources must not be empty");
    }

    #[test]
    fn rejects_invalid_simple_graph_references() {
        let invalid_initial = SimpleFsaGraph {
            states: vec![SimpleGraphState::accepting([1])],
            initial_state: 3,
            global_label_frequencies: BTreeMap::new(),
        };
        assert_eq!(
            calculate_simple_graph_layout(&invalid_initial, false)
                .unwrap_err()
                .to_string(),
            "invalid initial state index: 3"
        );

        let invalid_target = SimpleFsaGraph {
            states: vec![SimpleGraphState::non_accepting().with_transition(b'a', 7, None)],
            initial_state: 0,
            global_label_frequencies: BTreeMap::new(),
        };
        assert_eq!(
            calculate_simple_graph_layout(&invalid_target, false)
                .unwrap_err()
                .to_string(),
            "state 0 transition 97 targets invalid state 7"
        );
    }

    #[test]
    fn reads_names_and_qualifiers_with_legacy_indexes() {
        let result = read_names_and_qualifiers_from_str(
            "dict.tab",
            "pies\tpies\tsubst\nkot\tkot\tsubst\tpospolita\nala\tala\tsubst\twlasna\trare|archaic\n",
        )
        .unwrap();

        assert_eq!(result.names.get(""), Some(&0));
        assert_eq!(result.names.get("pospolita"), Some(&1));
        assert_eq!(result.names.get("wlasna"), Some(&2));

        assert_eq!(result.qualifiers.get(&qualifiers([])), Some(&0));
        assert_eq!(
            result.qualifiers.get(&qualifiers(["archaic", "rare"])),
            Some(&1)
        );
    }

    #[test]
    fn ignores_metadata_copyright_and_space_containing_forms() {
        let result = read_names_and_qualifiers_from_str(
            "dict.tab",
            "#!DICT-ID sgjp\n#<COPYRIGHT>\ninside\tinside\ttag\tignored\tq1\n#</COPYRIGHT>\nbad orth\tbad\ttag\tignored\tq2\ngood\tbad lemma\ttag\tignored\tq3\ngood\tgood\ttag\tkept\tq4\n",
        )
        .unwrap();

        assert_eq!(result.names.len(), 2);
        assert_eq!(result.names.get(""), Some(&0));
        assert_eq!(result.names.get("kept"), Some(&1));
        assert_eq!(result.qualifiers.len(), 2);
        assert_eq!(result.qualifiers.get(&qualifiers([])), Some(&0));
        assert_eq!(result.qualifiers.get(&qualifiers(["q4"])), Some(&1));
    }

    #[test]
    fn parses_three_four_and_five_field_lines() {
        let result = read_names_and_qualifiers_from_str(
            "dict.tab",
            "a\ta\ttag\nb\tb\ttag\tname\nc\tc\ttag\tname2\tq\n",
        )
        .unwrap();

        assert_eq!(result.names.get(""), Some(&0));
        assert_eq!(result.names.get("name"), Some(&1));
        assert_eq!(result.names.get("name2"), Some(&2));
        assert_eq!(result.qualifiers.get(&qualifiers([])), Some(&0));
        assert_eq!(result.qualifiers.get(&qualifiers(["q"])), Some(&1));
    }

    #[test]
    fn rejects_invalid_tab_field_count() {
        let error = read_names_and_qualifiers_from_str("dict.tab", "a\tb\n").unwrap_err();

        assert_eq!(
            error.to_string(),
            "input line \"a\tb\" does not have 3, 4 or 5 tab-separated fields"
        );
    }

    #[test]
    fn malformed_dict_id_without_space_flows_to_line_parser_like_legacy() {
        let error = read_names_and_qualifiers_from_str("dict.tab", "#!DICT-ID\n").unwrap_err();

        assert_eq!(
            error.to_string(),
            "input line \"#!DICT-ID\" does not have 3, 4 or 5 tab-separated fields"
        );
    }

    #[test]
    fn parse_qualifiers_keeps_empty_members() {
        assert_eq!(parse_qualifiers("rare|"), qualifiers(["", "rare"]));
    }

    #[test]
    fn preprocesses_segment_rule_defines_like_legacy_preprocessor() {
        assert_eq!(
            replace_ascii_word(" left x right", "x", "A B"),
            " left A B right"
        );

        let lines = [
            (1, "#define A x"),
            (2, "A B"),
            (3, "#define WRAP(x) left x right"),
            (4, "WRAP(A B)"),
            (5, "#define B(y) A y"),
            (6, "B(C)"),
        ];

        assert_eq!(
            preprocess_segment_rules(lines, std::iter::empty::<&str>(), "segmenty.dat").unwrap(),
            vec![
                (2, "x B".to_owned()),
                (4, " left x B right".to_owned()),
                (6, "x C".to_owned()),
            ]
        );
    }

    #[test]
    fn preprocesses_segment_rule_conditionals_like_legacy_preprocessor() {
        let lines = [
            (1, "#ifdef extra"),
            (2, "A"),
            (3, "#else"),
            (4, "B"),
            (5, "#endif"),
        ];

        assert_eq!(
            preprocess_segment_rules(lines, ["extra"], "segmenty.dat").unwrap(),
            vec![(2, "A".to_owned())]
        );
        assert_eq!(
            preprocess_segment_rules(lines, std::iter::empty::<&str>(), "segmenty.dat").unwrap(),
            vec![(4, "B".to_owned())]
        );
    }

    #[test]
    fn preprocesses_segment_rule_calls_comments_and_operators_like_legacy_preprocessor() {
        let lines = [
            (1, "#define A x y"),
            (2, "A(B)"),
            (3, "UNKNOWN(B C)"),
            (4, "# comment"),
            (5, "#define SHIFT x>"),
            (6, "(SHIFT | B)+ !weak"),
        ];

        assert_eq!(
            preprocess_segment_rules(lines, std::iter::empty::<&str>(), "segmenty.dat").unwrap(),
            vec![
                (2, "x y ( B )".to_owned()),
                (3, "UNKNOWN ( B C )".to_owned()),
                (4, "# comment".to_owned()),
                (6, "( x> | B ) + !weak".to_owned()),
            ]
        );
    }

    #[test]
    fn parses_segment_rule_lines_like_legacy_parser() {
        for (line, expected, weak, empty, shift_orth) in [
            ("A", "A", false, false, false),
            ("A>", "A>", false, false, true),
            ("(A)>", "A>", false, false, true),
            ("A B", "A B", false, false, false),
            ("A | B", "A | B", false, false, false),
            ("A|B C", "A | B C", false, false, false),
            ("A B|C", "A B | C", false, false, false),
            ("(A | B) C", "A | B C", false, false, false),
            ("A*", "(A)*", false, true, false),
            ("A+", "A (A)*", false, false, false),
            ("A?", "(A)?", false, true, false),
            ("A{2}", "A A", false, false, false),
            ("A{0,2}", "(A)? | A A", false, true, false),
            ("A{2,4}", "A A | A A A | A A A A", false, false, false),
            ("A{2,}", "A A (A)*", false, false, false),
            ("A> B>", "A> B>", false, false, true),
            ("A !weak", "A", true, false, false),
            ("(A B)>", "A> B>", false, false, true),
        ] {
            let rule = parsed_segment_rule(line);
            assert_eq!(rule.to_string(), expected, "{line}");
            assert_eq!(rule.is_weak(), weak, "{line}");
            assert_eq!(rule.allows_empty_sequence(), empty, "{line}");
            assert_eq!(rule.is_shift_orth_rule(), shift_orth, "{line}");
            rule.validate_segment_rule("<case>").unwrap_or_else(|err| {
                panic!("{line} should validate like legacy parser, got {err}")
            });
        }
    }

    #[test]
    fn transforms_segment_rules_to_generator_like_legacy_parser() {
        for (line, expected_generator, expected_additional) in [
            ("A", "A", vec!["A"]),
            ("A>", "A>", vec!["A>"]),
            ("(A)>", "A>", vec!["A>"]),
            ("A B", "<<REMOVED>>", vec!["A", "B"]),
            ("A | B", "A | B", vec!["A", "B"]),
            ("A|B C", "<<REMOVED>>", vec!["A", "B", "C"]),
            ("A B|C", "<<REMOVED>>", vec!["A", "B", "C"]),
            ("(A | B) C", "<<REMOVED>>", vec!["A", "B", "C"]),
            ("A*", "<<REMOVED>>", vec!["A"]),
            ("A+", "A", vec!["A", "A"]),
            ("A?", "A", vec!["A"]),
            ("A{2}", "<<REMOVED>>", vec!["A", "A"]),
            ("A{0,2}", "<<REMOVED>>", vec!["A", "A", "A"]),
            (
                "A{2,4}",
                "<<REMOVED>>",
                vec!["A", "A", "A", "A", "A", "A", "A", "A", "A"],
            ),
            ("A{2,}", "<<REMOVED>>", vec!["A", "A", "A"]),
            ("A> B>", "A> B>", vec![]),
            ("A B>", "<<REMOVED>>", vec!["A"]),
            ("A> | B>", "A> | B>", vec!["A>", "B>"]),
            ("A> | B", "A> | B", vec!["A>", "B"]),
            ("A !weak", "A", vec!["A"]),
            ("(A B)>", "A> B>", vec![]),
        ] {
            let rule = parsed_segment_rule(line);
            assert_eq!(
                rule.transform_to_generator_version().to_string(),
                expected_generator,
                "{line}"
            );
            assert_eq!(
                segment_rule_strings(rule.additional_atomic_rules_for_generator()),
                expected_additional,
                "{line}"
            );
        }
    }

    #[test]
    fn validates_segment_rule_shift_orth_constraints_like_legacy() {
        let error = parsed_segment_rule("A B>")
            .validate_segment_rule("<case>")
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "<case>:7 - If the rightmost subrule of concatenation \"A B>\" is with \">\", than all subrules must be with \">\""
        );

        let error = parsed_segment_rule("A> | B")
            .validate_segment_rule("<case>")
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "<case>:7 - All subrules of alternative \"A> | B\" must be either with or without \">\""
        );

        parsed_segment_rule("A> B>")
            .validate_segment_rule("<case>")
            .unwrap();
    }

    #[test]
    fn rejects_invalid_segment_rule_quantities_and_unknown_segment_types() {
        assert_eq!(
            parse_segment_rule_line(7, "A{0}", &segment_types(), "<case>")
                .unwrap_err()
                .to_string(),
            "<case>:7: A{0} - invalid quantity: 0"
        );
        assert_eq!(
            parse_segment_rule_line(7, "A{3,2}", &segment_types(), "<case>")
                .unwrap_err()
                .to_string(),
            "<case>:7: A{3,2} - invalid quantities: 3 2"
        );
        assert_eq!(
            parse_segment_rule_line(7, "NOPE", &segment_types(), "<case>")
                .unwrap_err()
                .to_string(),
            "<case>:7: unknown segment type: NOPE"
        );
    }

    #[test]
    fn serializes_segment_rules_fsa_like_legacy_python_builder() {
        for (lines, expected_bytes) in [
            (vec!["A"], vec![0, 1, 1, 0, 0, 6, 1, 0]),
            (vec!["A B"], vec![0, 1, 1, 0, 0, 6, 0, 1, 2, 0, 0, 12, 1, 0]),
            (
                vec!["A | B"],
                vec![0, 2, 1, 0, 0, 12, 2, 0, 0, 10, 1, 0, 1, 0],
            ),
            (
                vec!["A? B"],
                vec![0, 2, 1, 0, 0, 10, 2, 0, 0, 16, 0, 1, 2, 0, 0, 16, 1, 0],
            ),
            (
                vec!["A* B"],
                vec![
                    0, 2, 1, 0, 0, 10, 2, 0, 0, 20, 0, 2, 1, 0, 0, 10, 2, 0, 0, 20, 1, 0,
                ],
            ),
            (vec!["A>"], vec![0, 1, 1, 1, 0, 6, 1, 0]),
            (vec!["A !weak"], vec![0, 1, 1, 0, 0, 6, 3, 0]),
            (
                vec!["A", "B"],
                vec![0, 2, 1, 0, 0, 12, 2, 0, 0, 10, 1, 0, 1, 0],
            ),
            (
                vec!["A B", "A C"],
                vec![0, 1, 1, 0, 0, 6, 0, 2, 2, 0, 0, 16, 3, 0, 0, 18, 1, 0, 1, 0],
            ),
        ] {
            let rules = parsed_segment_rules(&lines);
            assert_eq!(
                serialize_segment_rules_fsa(rules.iter(), "<case>").unwrap(),
                expected_bytes,
                "{lines:?}"
            );
        }
    }

    #[test]
    fn rejects_segment_rules_fsa_weakness_conflicts_like_legacy_builder() {
        let rules = vec![
            parsed_segment_rule_at(7, "A B"),
            parsed_segment_rule_at(8, "A B !weak"),
        ];
        let error = serialize_segment_rules_fsa(rules.iter(), "<case>").unwrap_err();

        assert_eq!(
            error.to_string(),
            "<case>:8 - conflicts with rule at line 7. Segmentation for some chunks can be both weak and non-weak which is illegal."
        );
    }

    #[test]
    fn serializes_segmentation_rules_metadata_like_legacy_rules_manager() {
        let variants = vec![
            SegmentRulesFsaVariantData {
                options: segment_rule_options("strict", "split"),
                fsa: vec![0, 1, 2, 3],
            },
            SegmentRulesFsaVariantData {
                options: segment_rule_options("permissive", "composite"),
                fsa: vec![4, 5],
            },
        ];

        assert_eq!(
            serialize_segmentation_rules_data(
                [32, 9],
                &variants,
                &segment_rule_options("strict", "split"),
            )
            .unwrap(),
            vec![
                0, 2, 0, 0, 0, 9, 0, 0, 0, 32, 2, 2, 97, 103, 103, 108, 0, 115, 116, 114, 105, 99,
                116, 0, 112, 114, 97, 101, 116, 0, 115, 112, 108, 105, 116, 0, 0, 0, 0, 4, 0, 1, 2,
                3, 2, 97, 103, 103, 108, 0, 112, 101, 114, 109, 105, 115, 115, 105, 118, 101, 0,
                112, 114, 97, 101, 116, 0, 99, 111, 109, 112, 111, 115, 105, 116, 101, 0, 0, 0, 0,
                2, 4, 5, 2, 97, 103, 103, 108, 0, 115, 116, 114, 105, 99, 116, 0, 112, 114, 97,
                101, 116, 0, 115, 112, 108, 105, 116, 0,
            ]
        );
    }

    #[test]
    fn rejects_invalid_segmentation_rules_metadata_options() {
        assert_eq!(
            serialize_segmentation_rules_data(
                [],
                &[SegmentRulesFsaVariantData {
                    options: BTreeMap::new(),
                    fsa: vec![0, 0],
                }],
                &segment_rule_options("strict", "split"),
            )
            .unwrap_err()
            .to_string(),
            "segmentation options missing aggl"
        );
        assert_eq!(
            serialize_segmentation_rules_data([], &[], &segment_rule_options("strict", "split"),)
                .unwrap_err()
                .to_string(),
            "Too many segmentation rules variants"
        );
    }

    #[test]
    fn parses_analyzer_segmentation_rules_config_like_legacy_pipeline() {
        let parsed = parse_segmentation_rules_from_str(
            "segmenty.dat",
            sample_segment_rules_config(),
            SegmentRulesTarget::Analyzer,
        )
        .unwrap();

        assert_eq!(
            parsed.segmentation_rules_data,
            vec![
                0, 2, 0, 0, 0, 9, 0, 0, 0, 32, 2, 2, 97, 103, 103, 108, 0, 115, 116, 114, 105, 99,
                116, 0, 112, 114, 97, 101, 116, 0, 115, 112, 108, 105, 116, 0, 0, 0, 0, 8, 0, 1, 0,
                0, 0, 6, 1, 0, 2, 97, 103, 103, 108, 0, 112, 101, 114, 109, 105, 115, 115, 105,
                118, 101, 0, 112, 114, 97, 101, 116, 0, 115, 112, 108, 105, 116, 0, 0, 0, 0, 14, 0,
                1, 0, 0, 0, 6, 0, 1, 1, 0, 0, 12, 1, 0, 2, 97, 103, 103, 108, 0, 115, 116, 114,
                105, 99, 116, 0, 112, 114, 97, 101, 116, 0, 115, 112, 108, 105, 116, 0,
            ]
        );
        assert_eq!(parsed.separators, vec![32, 9]);
        assert_eq!(
            parsed.default_options,
            segment_rule_options("strict", "split")
        );
    }

    #[test]
    fn parses_generator_segmentation_rules_config_like_legacy_pipeline() {
        let parsed = parse_segmentation_rules_from_str(
            "segmenty.dat",
            sample_segment_rules_config(),
            SegmentRulesTarget::Generator,
        )
        .unwrap();

        assert_eq!(
            parsed.segmentation_rules_data,
            vec![
                0, 0, 2, 2, 97, 103, 103, 108, 0, 115, 116, 114, 105, 99, 116, 0, 112, 114, 97,
                101, 116, 0, 115, 112, 108, 105, 116, 0, 0, 0, 0, 8, 0, 1, 0, 0, 0, 6, 1, 0, 2, 97,
                103, 103, 108, 0, 112, 101, 114, 109, 105, 115, 115, 105, 118, 101, 0, 112, 114,
                97, 101, 116, 0, 115, 112, 108, 105, 116, 0, 0, 0, 0, 14, 0, 2, 0, 0, 0, 12, 1, 0,
                0, 10, 1, 0, 1, 0, 2, 97, 103, 103, 108, 0, 115, 116, 114, 105, 99, 116, 0, 112,
                114, 97, 101, 116, 0, 115, 112, 108, 105, 116, 0,
            ]
        );
        assert!(parsed.separators.is_empty());
    }

    #[test]
    fn applies_shift_orth_magic_like_legacy_pipeline() {
        let parsed = parse_segmentation_rules_from_str(
            "segmenty.dat",
            "[options]\naggl = strict\npraet = split\n[combinations]\nA\nA>\n[tags]\nA %\n[lexemes]\n[segment types]\nA\n[separator chars]\n",
            SegmentRulesTarget::Analyzer,
        )
        .unwrap();

        assert_eq!(
            parsed.segmentation_rules_data,
            vec![
                0, 0, 1, 2, 97, 103, 103, 108, 0, 115, 116, 114, 105, 99, 116, 0, 112, 114, 97,
                101, 116, 0, 115, 112, 108, 105, 116, 0, 0, 0, 0, 14, 0, 2, 0, 0, 0, 12, 1, 1, 0,
                10, 1, 0, 1, 0, 2, 97, 103, 103, 108, 0, 115, 116, 114, 105, 99, 116, 0, 112, 114,
                97, 101, 116, 0, 115, 112, 108, 105, 116, 0,
            ]
        );
        assert_eq!(
            parsed.shift_orth_extra_segment_types,
            BTreeMap::from([(0, 1)])
        );
        assert_eq!(parsed.replace_lemma_with_orth, BTreeSet::new());
        assert_eq!(
            parsed.additional_segment_type_names,
            BTreeMap::from([(1, "A>".to_owned())])
        );

        let only_shift = parse_segmentation_rules_from_str(
            "segmenty.dat",
            "[options]\naggl = strict\npraet = split\n[combinations]\nA>\n[tags]\nA %\n[lexemes]\n[segment types]\nA\n[separator chars]\n",
            SegmentRulesTarget::Analyzer,
        )
        .unwrap();
        assert_eq!(only_shift.shift_orth_extra_segment_types, BTreeMap::new());
        assert_eq!(only_shift.replace_lemma_with_orth, BTreeSet::from([0]));
    }

    #[test]
    fn indexes_segment_types_like_legacy_segtypes_helper() {
        let parsed = parse_segmentation_rules_with_tagset_from_str(
            "segmenty.dat",
            sample_segment_type_resolver_config(),
            SegmentRulesTarget::Analyzer,
            &sample_segment_type_tagset(),
            &segment_type_names(),
            &segment_type_labels(),
        )
        .unwrap();
        let resolver = parsed.segment_type_resolver.as_ref().unwrap();

        for (base, tag_num, name_num, labels_num, expected_segment_type_num) in [
            ("kot", 10, 0, 0, 0),
            ("kot:1", 10, 0, 0, 1),
            ("kot:2", 10, 0, 0, 0),
            ("missing", 10, 0, 0, 4),
            ("missing", 11, 0, 0, 4),
            ("missing", 12, 0, 0, 5),
            ("named", 10, 1, 0, 2),
            ("named", 10, 2, 0, 4),
            ("labeled", 10, 0, 1, 3),
            ("labeled", 10, 0, 2, 3),
            ("labeled", 10, 0, 3, 4),
        ] {
            assert_eq!(
                resolver
                    .lexeme_to_segment_type_num(base, tag_num, name_num, labels_num)
                    .unwrap(),
                expected_segment_type_num,
                "{base}/{tag_num}/{name_num}/{labels_num}"
            );
            assert_eq!(
                parsed
                    .lexeme_to_segment_type_num(base, tag_num, name_num, labels_num)
                    .unwrap(),
                expected_segment_type_num,
                "parsed wrapper {base}/{tag_num}/{name_num}/{labels_num}"
            );
        }
    }

    #[test]
    fn parsed_segment_rules_lookup_exposes_shift_orth_magic() {
        let parsed = parse_segmentation_rules_with_tagset_from_str(
            "segmenty.dat",
            "[options]\naggl = strict\npraet = split\n[combinations]\nA\nA>\n[tags]\nA %\n[lexemes]\n[segment types]\nA\n[separator chars]\n",
            SegmentRulesTarget::Analyzer,
            &Tagset::from_str("tagset", "#!TAGSET-ID tid\n[TAGS]\n10\tsubst\n").unwrap(),
            &BTreeMap::from([(String::new(), 0)]),
            &BTreeMap::from([(QualifierSet::new(), 0)]),
        )
        .unwrap();

        assert_eq!(
            parsed
                .lexeme_to_segment_type_num("anything", 10, 0, 0)
                .unwrap(),
            0
        );
        assert_eq!(parsed.new_segment_type_for_shift_orth(0), Some(1));
        assert!(!parsed.should_replace_lemma_with_orth(0));

        let only_shift = parse_segmentation_rules_with_tagset_from_str(
            "segmenty.dat",
            "[options]\naggl = strict\npraet = split\n[combinations]\nA>\n[tags]\nA %\n[lexemes]\n[segment types]\nA\n[separator chars]\n",
            SegmentRulesTarget::Analyzer,
            &Tagset::from_str("tagset", "#!TAGSET-ID tid\n[TAGS]\n10\tsubst\n").unwrap(),
            &BTreeMap::from([(String::new(), 0)]),
            &BTreeMap::from([(QualifierSet::new(), 0)]),
        )
        .unwrap();
        assert!(only_shift.should_replace_lemma_with_orth(0));
        assert_eq!(only_shift.new_segment_type_for_shift_orth(0), None);
    }

    #[test]
    fn rejects_too_many_qualifier_combinations() {
        let mut input = String::new();
        for index in 0..MAX_QUALIFIERS_COMBINATIONS {
            input.push_str(&format!("w{index}\tb{index}\ttag\t\tq{index}\n"));
        }

        let error = read_names_and_qualifiers_from_str("dict.tab", &input).unwrap_err();

        assert_eq!(
            error.to_string(),
            "Too many qualifiers combinations. The limit is 2048"
        );
    }

    #[test]
    fn encodes_analyzer_forms_like_legacy_python_builder() {
        let cases = [
            (
                "kot",
                "kota",
                EncodedAnalyzerForm {
                    prefix_cut_length: 0,
                    cut_length: 0,
                    suffix_to_add: "a".to_owned(),
                    case_pattern: vec![false, false, false],
                },
            ),
            (
                "odkot",
                "kot",
                EncodedAnalyzerForm {
                    prefix_cut_length: 2,
                    cut_length: 0,
                    suffix_to_add: String::new(),
                    case_pattern: vec![false, false, false],
                },
            ),
            (
                "ABCd",
                "abcd",
                EncodedAnalyzerForm {
                    prefix_cut_length: 0,
                    cut_length: 0,
                    suffix_to_add: String::new(),
                    case_pattern: vec![false, false, false, false],
                },
            ),
            (
                "Lodz",
                "lodzi",
                EncodedAnalyzerForm {
                    prefix_cut_length: 0,
                    cut_length: 0,
                    suffix_to_add: "i".to_owned(),
                    case_pattern: vec![false, false, false, false],
                },
            ),
            (
                "abcdef",
                "zabcdef",
                EncodedAnalyzerForm {
                    prefix_cut_length: 0,
                    cut_length: 6,
                    suffix_to_add: "zabcdef".to_owned(),
                    case_pattern: vec![],
                },
            ),
            (
                "abcdef",
                "abcxyz",
                EncodedAnalyzerForm {
                    prefix_cut_length: 0,
                    cut_length: 3,
                    suffix_to_add: "xyz".to_owned(),
                    case_pattern: vec![false, false, false],
                },
            ),
        ];

        for (from, target, expected) in cases {
            assert_eq!(encode_analyzer_form(from, target).unwrap(), expected);
        }
    }

    #[test]
    fn encodes_unicode_analyzer_form_like_legacy_python_builder() {
        assert_eq!(
            encode_analyzer_form("Łódź", "łodzi").unwrap(),
            EncodedAnalyzerForm {
                prefix_cut_length: 0,
                cut_length: 3,
                suffix_to_add: "odzi".to_owned(),
                case_pattern: vec![false],
            }
        );
    }

    #[test]
    fn encodes_generator_forms_like_legacy_python_builder() {
        let cases = [
            (
                "kot",
                "kota",
                EncodedGeneratorForm {
                    cut_length: 0,
                    suffix_to_add: "a".to_owned(),
                    prefix_to_add: String::new(),
                },
            ),
            (
                "odkot",
                "kot",
                EncodedGeneratorForm {
                    cut_length: 4,
                    suffix_to_add: "t".to_owned(),
                    prefix_to_add: "k".to_owned(),
                },
            ),
            (
                "ABCd",
                "abcd",
                EncodedGeneratorForm {
                    cut_length: 4,
                    suffix_to_add: "abcd".to_owned(),
                    prefix_to_add: String::new(),
                },
            ),
            (
                "abcdef",
                "zabcdef",
                EncodedGeneratorForm {
                    cut_length: 0,
                    suffix_to_add: String::new(),
                    prefix_to_add: "z".to_owned(),
                },
            ),
            (
                "abcdef",
                "abcxyz",
                EncodedGeneratorForm {
                    cut_length: 3,
                    suffix_to_add: "xyz".to_owned(),
                    prefix_to_add: String::new(),
                },
            ),
        ];

        for (from, target, expected) in cases {
            assert_eq!(encode_generator_form(from, target).unwrap(), expected);
        }
    }

    #[test]
    fn encodes_unicode_generator_form_like_legacy_python_builder() {
        assert_eq!(
            encode_generator_form("Łódź", "łodzi").unwrap(),
            EncodedGeneratorForm {
                cut_length: 4,
                suffix_to_add: "łodzi".to_owned(),
                prefix_to_add: String::new(),
            }
        );
    }

    #[test]
    fn analyzer_interpretation_sort_key_matches_legacy_python_builder() {
        let interpretation = AnalyzerInterpretation::new("Łódź", "łodzi", 7, 2, 3, 4).unwrap();

        assert_eq!(interpretation.orth_case_pattern, vec![true]);
        assert_eq!(interpretation.qualifiers, 4);
        assert_eq!(
            interpretation.sort_key(),
            AnalyzerInterpretationSortKey {
                cut_length: 3,
                prefix_cut_length: 0,
                suffix_to_add: vec!['o', 'd', 'z', 'i'],
                case_pattern: vec![false],
                orth_case_pattern: vec![true],
                tag_num: 7,
                name_num: 2,
                type_num: 3,
            }
        );
    }

    #[test]
    fn generator_interpretation_sort_key_matches_legacy_python_builder() {
        let interpretation =
            GeneratorInterpretation::new("koty", "kot:1", 7, 2, 3, "1", 4).unwrap();

        assert_eq!(interpretation.lemma, "kot:1");
        assert_eq!(interpretation.qualifiers, 4);
        assert_eq!(
            interpretation.sort_key(),
            GeneratorInterpretationSortKey {
                homonym_id: "1".to_owned(),
                tag_num: 7,
                cut_length: 2,
                suffix_to_add: vec!['y'],
                name_num: 2,
                type_num: 3,
            }
        );
    }

    #[test]
    fn rejects_empty_words_that_legacy_builder_asserts_on() {
        assert_eq!(
            encode_analyzer_form("", "lemma").unwrap_err().to_string(),
            "cannot encode analyzer form for an empty source word"
        );
        assert_eq!(
            encode_generator_form("lemma", "").unwrap_err().to_string(),
            "cannot encode generator form for an empty target word"
        );
    }

    #[test]
    fn analyzer_converter_matches_legacy_grouping_shift_and_dedup_semantics() {
        let tagset = tagset();
        let names = names();
        let qualifiers_map = qualifiers_map();
        let rules = FakeRules::new()
            .with_type("kot", 10, 1, 1, 5)
            .with_type("kot", 10, 1, 2, 5)
            .with_type("base", 10, 1, 1, 6)
            .with_shift(6, 8)
            .with_type("lemma", 10, 0, 0, 9)
            .with_replace(9);

        let entries = convert_polimorf_for_analyzer(
            [
                "shift\tbase\ttag\tname\tq\n",
                "Kot\tkot\ttag\tname\tq\n",
                "kot\tkot\ttag\tname\tq\n",
                "kot\tkot\ttag\tname\tq2\n",
                "replace\tlemma\ttag\n",
            ],
            &tagset,
            &names,
            &qualifiers_map,
            &IdentityEncoder,
            &rules,
        )
        .unwrap();

        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.key.as_str())
                .collect::<Vec<_>>(),
            vec!["kot", "replace", "shift"]
        );

        let kot = entry(&entries, "kot");
        assert_eq!(kot.interpretations.len(), 2);
        assert!(kot
            .interpretations
            .iter()
            .all(|interpretation| interpretation.qualifiers != 2));

        let replace = entry(&entries, "replace");
        assert_eq!(replace.interpretations.len(), 1);
        assert_eq!(replace.interpretations[0].type_num, 9);
        assert_eq!(replace.interpretations[0].encoded_form.cut_length, 0);
        assert_eq!(replace.interpretations[0].encoded_form.suffix_to_add, "");

        let shift_types = entry(&entries, "shift")
            .interpretations
            .iter()
            .map(|interpretation| interpretation.type_num)
            .collect::<BTreeSet<_>>();
        assert_eq!(shift_types, BTreeSet::from([6, 8]));
    }

    #[test]
    fn analyzer_converter_rejects_replace_and_shift_conflict_like_legacy_assertion() {
        let tagset = tagset();
        let names = names();
        let qualifiers_map = qualifiers_map();
        let rules = FakeRules::new()
            .with_type("lemma", 10, 0, 0, 9)
            .with_replace(9)
            .with_shift(9, 8);

        let error = convert_polimorf_for_analyzer(
            ["orth\tlemma\ttag\n"],
            &tagset,
            &names,
            &qualifiers_map,
            &IdentityEncoder,
            &rules,
        )
        .unwrap_err();

        assert_eq!(
            error.to_string(),
            "shift-orth replacement and extra segment cannot both be active"
        );
    }

    #[test]
    fn generator_converter_matches_legacy_homonym_shift_skip_and_dedup_semantics() {
        let tagset = tagset();
        let names = names();
        let qualifiers_map = qualifiers_map();
        let rules = FakeRules::new()
            .with_type("lemma", 10, 1, 1, 5)
            .with_type("lemma", 10, 1, 2, 5)
            .with_type("base", 10, 1, 1, 6)
            .with_shift(6, 8)
            .with_type("lemma", 10, 0, 0, 9)
            .with_replace(9);

        let entries = convert_polimorf_for_generator(
            [
                "form\tlemma:hid\ttag\tname\tq\n",
                "form\tlemma:hid\ttag\tname\tq\n",
                "form\tlemma:hid\ttag\tname\tq2\n",
                "empty\t\ttag\tname\tq\n",
                "shifted\tbase\ttag\tname\tq\n",
                "replace\tlemma\ttag\n",
            ],
            &tagset,
            &names,
            &qualifiers_map,
            &IdentityEncoder,
            &rules,
        )
        .unwrap();

        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.key.as_str())
                .collect::<Vec<_>>(),
            vec!["base", "lemma", "replace", "shifted"]
        );

        let lemma = generator_entry(&entries, "lemma");
        assert_eq!(lemma.interpretations.len(), 1);
        assert_eq!(lemma.interpretations[0].homonym_id, "hid");
        assert_eq!(lemma.interpretations[0].qualifiers, 1);

        let base = generator_entry(&entries, "base");
        assert_eq!(base.interpretations[0].type_num, 6);

        let shifted = generator_entry(&entries, "shifted");
        assert_eq!(shifted.interpretations[0].type_num, 8);
        assert_eq!(shifted.interpretations[0].lemma, "shifted");

        let replace = generator_entry(&entries, "replace");
        assert_eq!(replace.interpretations[0].type_num, 9);
        assert_eq!(replace.interpretations[0].lemma, "replace");
    }

    #[test]
    fn split_generator_homonym_matches_legacy_rules() {
        assert_eq!(
            split_generator_homonym("kot:1"),
            ("kot".to_owned(), "1".to_owned())
        );
        assert_eq!(
            split_generator_homonym(":1"),
            (":1".to_owned(), String::new())
        );
        assert_eq!(
            split_generator_homonym("kot:"),
            ("kot:".to_owned(), String::new())
        );
        assert_eq!(
            split_generator_homonym("kot:1:2"),
            ("kot".to_owned(), "1:2".to_owned())
        );
    }

    fn segment_types() -> BTreeMap<String, usize> {
        ["A", "B", "C", "D", "X", "Y", "Z", "SHIFT", "N"]
            .into_iter()
            .enumerate()
            .map(|(index, segment_type)| (segment_type.to_owned(), index + 1))
            .collect()
    }

    fn parsed_segment_rule(line: &str) -> SegmentRule {
        parsed_segment_rule_at(7, line)
    }

    fn parsed_segment_rule_at(line_number: usize, line: &str) -> SegmentRule {
        parse_segment_rule_line(line_number, line, &segment_types(), "<case>").unwrap()
    }

    fn parsed_segment_rules(lines: &[&str]) -> Vec<SegmentRule> {
        lines
            .iter()
            .enumerate()
            .map(|(index, line)| parsed_segment_rule_at(index + 7, line))
            .collect()
    }

    fn segment_rule_strings(rules: Vec<SegmentRule>) -> Vec<String> {
        rules.into_iter().map(|rule| rule.to_string()).collect()
    }

    fn segment_rule_options(aggl: &str, praet: &str) -> BTreeMap<String, String> {
        BTreeMap::from([
            ("aggl".to_owned(), aggl.to_owned()),
            ("praet".to_owned(), praet.to_owned()),
        ])
    }

    fn sample_segment_rules_config() -> &'static str {
        "[options]\n\
aggl = strict permissive\n\
praet = split\n\
\n\
[combinations]\n\
#ifdef strict\n\
A\n\
#endif\n\
#ifdef permissive\n\
A B\n\
#endif\n\
\n\
[tags]\n\
A %\n\
\n\
[lexemes]\n\
\n\
[segment types]\n\
A\n\
B\n\
\n\
[separator chars]\n\
32\n\
9\n"
    }

    fn sample_segment_type_resolver_config() -> &'static str {
        "[options]\n\
aggl = strict\n\
praet = split\n\
[combinations]\n\
LEX\n\
[tags]\n\
TAG_SUB subst:%\n\
TAG_ANY %\n\
[lexemes]\n\
LEX kot subst\n\
HOM kot:1 subst\n\
NAME named subst name=n1\n\
LABEL labeled subst labels=q\n\
[segment types]\n\
LEX\n\
HOM\n\
NAME\n\
LABEL\n\
TAG_SUB\n\
TAG_ANY\n\
[separator chars]\n"
    }

    fn sample_segment_type_tagset() -> Tagset {
        Tagset::from_str(
            "tagset",
            "#!TAGSET-ID tid\n[TAGS]\n10\tsubst\n11\tsubst:sg\n12\tadj\n",
        )
        .unwrap()
    }

    fn segment_type_names() -> BTreeMap<String, usize> {
        BTreeMap::from([
            (String::new(), 0),
            ("n1".to_owned(), 1),
            ("n2".to_owned(), 2),
        ])
    }

    fn segment_type_labels() -> BTreeMap<QualifierSet, usize> {
        BTreeMap::from([
            (qualifiers([]), 0),
            (qualifiers(["q"]), 1),
            (qualifiers(["q", "r"]), 2),
            (qualifiers(["r"]), 3),
        ])
    }

    fn qualifiers<const N: usize>(values: [&str; N]) -> QualifierSet {
        values.into_iter().map(str::to_owned).collect()
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    fn simple_oracle_state() -> SimpleState {
        SimpleState::accepting([0xaa, 0xbb])
            .with_transition(b'a', 0x010203, Some(9))
            .with_transition(b'b', 0x000102, Some(8))
            .with_transition(b'c', 0x000001, Some(7))
            .with_label_frequency(b'a', 5)
            .with_label_frequency(b'b', 5)
            .with_label_frequency(b'c', 1)
    }

    fn simple_global_frequencies() -> BTreeMap<u8, usize> {
        BTreeMap::from([(b'a', 3), (b'b', 7), (b'c', 1)])
    }

    fn simple_dictionary_metadata() -> (
        Tagset,
        BTreeMap<String, usize>,
        BTreeMap<QualifierSet, usize>,
    ) {
        (
            Tagset::from_str("sample.tagset", "#!TAGSET-ID tid\n[TAGS]\n2\tb\n1\ta\n").unwrap(),
            BTreeMap::from([(String::new(), 0), ("name".to_owned(), 1)]),
            BTreeMap::from([
                (qualifiers([]), 0),
                (qualifiers(["x"]), 1),
                (qualifiers(["arch", "rare"]), 2),
            ]),
        )
    }

    fn constructed_simple_entries() -> Vec<(Vec<u8>, Vec<u8>)> {
        vec![
            (b"a".to_vec(), vec![1]),
            (b"ab".to_vec(), vec![2]),
            (b"b".to_vec(), vec![3]),
            (b"bb".to_vec(), vec![2]),
        ]
    }

    fn simple_oracle_graph(with_transition_data: bool) -> SimpleFsaGraph {
        let transition_data = |value| {
            if with_transition_data {
                Some(value)
            } else {
                None
            }
        };
        SimpleFsaGraph {
            states: vec![
                SimpleGraphState::non_accepting()
                    .with_frequency(0)
                    .with_transition(b'a', 1, transition_data(9))
                    .with_transition(b'b', 2, transition_data(8))
                    .with_label_frequency(b'a', 10)
                    .with_label_frequency(b'b', 1),
                SimpleGraphState::non_accepting()
                    .with_frequency(1)
                    .with_transition(b'x', 3, transition_data(3))
                    .with_label_frequency(b'x', 1),
                SimpleGraphState::non_accepting()
                    .with_frequency(5)
                    .with_transition(b'c', 3, transition_data(4))
                    .with_label_frequency(b'c', 1),
                SimpleGraphState::accepting([0xde, 0xad]).with_frequency(0),
            ],
            initial_state: 0,
            global_label_frequencies: BTreeMap::from([(b'a', 5), (b'b', 2), (b'c', 7), (b'x', 1)]),
        }
    }

    fn tagset() -> BTreeMap<String, usize> {
        BTreeMap::from([("tag".to_owned(), 10)])
    }

    fn names() -> BTreeMap<String, usize> {
        BTreeMap::from([(String::new(), 0), ("name".to_owned(), 1)])
    }

    fn qualifiers_map() -> BTreeMap<QualifierSet, usize> {
        BTreeMap::from([
            (qualifiers([]), 0),
            (qualifiers(["q"]), 1),
            (qualifiers(["q2"]), 2),
        ])
    }

    fn entry<'a>(entries: &'a [AnalyzerEntry], key: &str) -> &'a AnalyzerEntry {
        entries
            .iter()
            .find(|entry| entry.key == key)
            .unwrap_or_else(|| panic!("missing analyzer entry {key}"))
    }

    fn generator_entry<'a>(entries: &'a [GeneratorEntry], key: &str) -> &'a GeneratorEntry {
        entries
            .iter()
            .find(|entry| entry.key == key)
            .unwrap_or_else(|| panic!("missing generator entry {key}"))
    }

    #[derive(Debug, Clone, Default)]
    struct FakeRules {
        types: BTreeMap<(String, usize, usize, usize), usize>,
        replacements: BTreeSet<usize>,
        shifts: BTreeMap<usize, usize>,
    }

    impl FakeRules {
        fn new() -> Self {
            Self::default()
        }

        fn with_type(
            mut self,
            base: &str,
            tag_num: usize,
            name_num: usize,
            qualifiers_num: usize,
            segment_type_num: usize,
        ) -> Self {
            self.types.insert(
                (base.to_owned(), tag_num, name_num, qualifiers_num),
                segment_type_num,
            );
            self
        }

        fn with_replace(mut self, segment_type_num: usize) -> Self {
            self.replacements.insert(segment_type_num);
            self
        }

        fn with_shift(mut self, segment_type_num: usize, new_segment_type_num: usize) -> Self {
            self.shifts.insert(segment_type_num, new_segment_type_num);
            self
        }
    }

    impl SegmentRulesLookup for FakeRules {
        fn lexeme_to_segment_type_num(
            &self,
            base: &str,
            tag_num: usize,
            name_num: usize,
            qualifiers_num: usize,
        ) -> Result<usize> {
            self.types
                .get(&(base.to_owned(), tag_num, name_num, qualifiers_num))
                .copied()
                .ok_or_else(|| {
                    BuilderError::new(format!(
                        "missing fake segment type for {base}/{tag_num}/{name_num}/{qualifiers_num}"
                    ))
                })
        }

        fn should_replace_lemma_with_orth(&self, segment_type_num: usize) -> bool {
            self.replacements.contains(&segment_type_num)
        }

        fn new_segment_type_for_shift_orth(&self, segment_type_num: usize) -> Option<usize> {
            self.shifts.get(&segment_type_num).copied()
        }
    }
}
