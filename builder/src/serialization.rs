use std::collections::{BTreeMap, BTreeSet};

use crate::constants::{
    QualifierSet, CASE_PATTERN_MIXED, CASE_PATTERN_ONLY_LOWER, CASE_PATTERN_UPPER_PREFIX,
    DICTIONARY_VERSION, LEMMA_ONLY_LOWER, LEMMA_ONLY_TITLE, MAGIC_NUMBER, MAX_NAMES,
    MAX_SEGMENT_TYPES, MAX_TAGS, ORTH_ONLY_LOWER, ORTH_ONLY_TITLE, PREFIX_CUT_MASK,
};
use crate::dictionary::{
    AnalyzerEntry, AnalyzerInterpretation, GeneratorEntry, GeneratorInterpretation,
};
use crate::encoding::WordBytesEncoder;
use crate::error::{validate, BuilderError, Result};
use crate::simple_fsa::{serialize_simple_fsa_data, simple_implementation_code, SimpleFsaGraph};
use crate::tagset::Tagset;

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
