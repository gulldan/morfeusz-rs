mod constants;
mod dictionary;
mod encoding;
mod error;
mod segment_rules;
mod serialization;
mod simple_fsa;
mod tagset;

pub use constants::{
    QualifierSet, DICTIONARY_VERSION, MAGIC_NUMBER, MAX_NAMES, MAX_QUALIFIERS_COMBINATIONS,
    MAX_SEGMENT_RULES_FSA_SIZE, MAX_SEGMENT_TYPES, MAX_TAGS,
};
pub use dictionary::{
    build_analyzer_simple_dictionary_from_entries, build_analyzer_simple_dictionary_from_paths,
    build_analyzer_simple_dictionary_from_sources, build_analyzer_simple_dictionary_from_str,
    build_analyzer_simple_fsa_from_entries, build_generator_simple_dictionary_from_entries,
    build_generator_simple_dictionary_from_paths, build_generator_simple_dictionary_from_sources,
    build_generator_simple_dictionary_from_str, build_generator_simple_fsa_from_entries,
    convert_polimorf_for_analyzer, convert_polimorf_for_generator, encode_analyzer_form,
    encode_form_without_prefix, encode_generator_form, merge_metadata, merge_names_and_qualifiers,
    parse_qualifiers, read_metadata_from_str, read_names_and_qualifiers_from_str,
    split_generator_homonym, AnalyzerEntry, AnalyzerInterpretation, AnalyzerInterpretationSortKey,
    DictionaryMetadata, DictionarySource, EncodedAnalyzerForm, EncodedFormWithoutPrefix,
    EncodedGeneratorForm, GeneratorEntry, GeneratorInterpretation, GeneratorInterpretationSortKey,
    NamesAndQualifiers,
};
pub use encoding::{
    IdentityEncoder, SortEncoder, Utf8AnalyzerEncoder, Utf8GeneratorEncoder, Utf8WordEncoder,
    WordBytesEncoder,
};
pub use error::{BuilderError, Result};
#[cfg(test)]
pub(crate) use segment_rules::replace_ascii_word;
pub use segment_rules::{
    build_segment_rules_fsa, parse_segment_rule_line, parse_segmentation_rules_from_str,
    parse_segmentation_rules_with_tagset_from_str, preprocess_segment_rules,
    serialize_segment_rules_fsa, serialize_segment_rules_fsa_data,
    serialize_segmentation_rules_data, ParsedSegmentRules, SegmentRule, SegmentRulesFsa,
    SegmentRulesFsaLayout, SegmentRulesFsaVariantData, SegmentRulesLookup, SegmentRulesState,
    SegmentRulesTarget, SegmentRulesTransitionLabel, SegmentTypeLookup, SegmentTypeResolver,
};
pub use serialization::{
    analyzer_entries_to_sorted_fsa_entries, generator_entries_to_sorted_fsa_entries,
    serialize_analyzer_entry_payload, serialize_epilogue, serialize_generator_entry_payload,
    serialize_legacy_string, serialize_prologue, serialize_qualifiers_map,
    serialize_simple_dictionary, serialize_tags_map, serialize_tagset_data, serialize_u16_be,
    serialize_u32_be,
};
pub use simple_fsa::{
    build_simple_fsa_from_sorted_entries, calculate_simple_graph_layout, serialize_simple_fsa_data,
    serialize_simple_state, serialize_simple_state_data, serialize_simple_transitions,
    simple_implementation_code, simple_state_size, SimpleFsaGraph, SimpleGraphLayout,
    SimpleGraphState, SimpleGraphTransition, SimpleState, SimpleTransition,
};
pub use tagset::{Tagset, TagsetLookup, TagsetRulesLookup};

#[cfg(test)]
mod tests;
