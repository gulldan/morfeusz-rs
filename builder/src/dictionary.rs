use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use crate::constants::{QualifierSet, MAX_QUALIFIERS_COMBINATIONS};
use crate::encoding::{SortEncoder, Utf8AnalyzerEncoder, Utf8GeneratorEncoder, WordBytesEncoder};
use crate::error::{validate, BuilderError, Result};
use crate::segment_rules::{
    parse_segmentation_rules_with_tagset_from_str, ParsedSegmentRules, SegmentRulesLookup,
    SegmentRulesTarget,
};
use crate::serialization::{
    analyzer_entries_to_sorted_fsa_entries, generator_entries_to_sorted_fsa_entries,
    serialize_simple_dictionary,
};
use crate::simple_fsa::{build_simple_fsa_from_sorted_entries, SimpleFsaGraph};
use crate::tagset::{Tagset, TagsetLookup};

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
        } else {
            first_two_tab_fields(line).contains(' ')
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
