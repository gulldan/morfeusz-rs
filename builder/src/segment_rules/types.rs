use std::collections::{BTreeMap, BTreeSet};

use super::resolver::SegmentTypeResolver;
use crate::error::{BuilderError, Result};

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

impl SegmentTypeLookup for BTreeMap<String, usize> {
    fn segment_type_num(&self, segment_type: &str) -> Result<usize> {
        self.get(segment_type)
            .copied()
            .ok_or_else(|| BuilderError::new(format!("unknown segment type: {segment_type}")))
    }
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
    pub(super) transition_order: Vec<SegmentRulesTransitionLabel>,
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
pub struct SegmentRulesFsaLayout {
    pub dfs_order: Vec<usize>,
    pub offsets: Vec<usize>,
    pub reverse_offsets: Vec<usize>,
    pub total_size: usize,
}
