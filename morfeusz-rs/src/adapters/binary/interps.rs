use super::*;

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
pub(super) struct BinaryAnalyzerForm {
    pub(super) prefix_to_cut: u8,
    pub(super) suffix_to_cut: u8,
    pub(super) suffix_to_add: String,
    pub(super) case_pattern: BinaryCasePattern,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct BinaryAnalyzerInterpretation {
    pub(super) orth_case_pattern: BinaryCasePattern,
    pub(super) form: BinaryAnalyzerForm,
    pub(super) tag_id: i32,
    pub(super) name_id: i32,
    pub(super) labels_id: i32,
}

impl BinaryAnalyzerInterpretation {
    pub(super) fn matches_orth_case(&self, orth: &str) -> bool {
        self.orth_case_pattern.matches_orth(orth)
    }

    pub(super) fn to_morph_interpretation_in_context(
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

pub(super) trait AnalyzerInterpretationView {
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
pub(super) enum BinaryCasePattern {
    Lower,
    UpperPrefix(usize),
    Mixed(Vec<usize>),
}

impl BinaryCasePattern {
    pub(super) fn is_empty(&self) -> bool {
        matches!(self, Self::Lower)
    }

    pub(super) fn is_uppercase_at(&self, index: usize) -> bool {
        match self {
            Self::Lower => false,
            Self::UpperPrefix(len) => index < *len,
            Self::Mixed(indices) => indices.contains(&index),
        }
    }

    pub(super) fn matches_orth(&self, orth: &str) -> bool {
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
                    let wanted = raw_index;
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

    pub(super) fn shifted_by_lower_prefix(&self, prefix_len: usize) -> Self {
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

    pub(super) fn into_vec(self) -> Vec<bool> {
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

pub fn read_raw_interps_groups(payload: &[u8]) -> Result<Vec<RawInterpsGroup<'_>>> {
    let mut groups = Vec::new();
    for_each_raw_interps_group(payload, |_, group| {
        groups.push(group);
        Ok(())
    })?;
    Ok(groups)
}

pub(super) fn for_each_raw_interps_group<'a, F>(payload: &'a [u8], mut visit: F) -> Result<()>
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

pub(super) fn decode_binary_analyzer_interpretations(
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

pub(super) fn read_binary_analyzer_form(
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

pub(super) fn read_binary_orth_case_pattern(
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

pub(super) fn read_binary_lemma_case_pattern(
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

pub(super) fn read_binary_case_pattern(
    data: &[u8],
    cursor: &mut usize,
) -> Result<BinaryCasePattern> {
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

pub(super) fn skip_case_pattern(data: &[u8], cursor: &mut usize) -> Result<()> {
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

pub(super) fn orth_case_patterns_encoded_in_compression_byte(byte: u8) -> bool {
    byte & (ORTH_ONLY_LOWER | ORTH_ONLY_TITLE) != 0
}

pub(super) fn prefix_cut_encoded_in_compression_byte(byte: u8) -> bool {
    byte & PREFIX_CUT_MASK != PREFIX_CUT_MASK
}
