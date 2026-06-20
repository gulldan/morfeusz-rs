use std::collections::{BTreeMap, BTreeSet};

use super::config::SegmentRulesConfigFile;
use super::types::{ParsedSegmentRules, SegmentRulesLookup, SegmentTypeLookup};
use crate::constants::QualifierSet;
use crate::dictionary::parse_qualifiers;
use crate::error::{validate, BuilderError, Result};
use crate::tagset::TagsetRulesLookup;

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

impl SegmentTypeResolver {
    pub(super) fn from_config<T>(
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

    pub(super) fn with_shift_magic(
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
