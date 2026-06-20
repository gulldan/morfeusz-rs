mod automaton;
mod config;
mod parser;
mod preprocess;
mod resolver;
mod rule;
mod serialization;
mod types;

pub use automaton::build_segment_rules_fsa;
pub use config::{
    parse_segmentation_rules_from_str, parse_segmentation_rules_with_tagset_from_str,
};
pub use parser::parse_segment_rule_line;
pub use preprocess::preprocess_segment_rules;
#[cfg(test)]
pub(crate) use preprocess::replace_ascii_word;
pub use resolver::SegmentTypeResolver;
pub use rule::SegmentRule;
pub use serialization::{
    serialize_segment_rules_fsa, serialize_segment_rules_fsa_data,
    serialize_segmentation_rules_data,
};
pub use types::{
    ParsedSegmentRules, SegmentRulesFsa, SegmentRulesFsaLayout, SegmentRulesFsaVariantData,
    SegmentRulesLookup, SegmentRulesState, SegmentRulesTarget, SegmentRulesTransitionLabel,
    SegmentTypeLookup,
};
