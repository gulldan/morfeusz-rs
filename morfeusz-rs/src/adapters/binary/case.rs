use super::*;

pub(super) enum NormalizedInput<'a> {
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
    pub(super) fn as_str(&self) -> &str {
        match self {
            Self::Borrowed(text) => text,
            Self::OwnedIdentity { normalized } => normalized,
            Self::Owned { normalized, .. } => normalized,
        }
    }

    pub(super) fn path_capacity_hint(&self) -> usize {
        self.as_str().len().clamp(1, 8)
    }

    pub(super) fn original_span<'b>(
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
pub(super) fn lowercase_with_original_boundaries(text: &str) -> NormalizedInput<'_> {
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

    let mut normalized = String::with_capacity(text.len());
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

pub(super) fn char_lowercases_to_self(ch: char) -> bool {
    crate::case_tables::to_lower_char(ch) == ch
}

/// Looks up the original byte offset for a normalized byte offset in the sorted
/// boundary table produced by [`lowercase_with_original_boundaries`].
pub(super) fn boundary_original(
    boundaries: &[(usize, usize)],
    normalized_offset: usize,
) -> Option<usize> {
    boundaries
        .binary_search_by_key(&normalized_offset, |&(norm, _)| norm)
        .ok()
        .map(|index| boundaries[index].1)
}

pub(super) fn original_span_for_normalized_range<'a>(
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

pub(super) fn case_pattern_matches_orth(orth: &str, case_pattern: &[bool]) -> bool {
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

pub(super) fn char_is_lowercase_equivalent(ch: char) -> bool {
    // Matches C++ case-pattern checking: a codepoint counts as lowercase iff the
    // Morfeusz lowercase table maps it to itself.
    crate::case_tables::to_lower_char(ch) == ch
}

pub(super) fn drop_suffix_chars(value: &str, chars_to_cut: usize) -> Result<&str> {
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

pub(super) trait AnalyzerFormView {
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

pub(super) fn decode_analyzer_lemma(orth: &str, form: &EncodedForm) -> Result<String> {
    decode_analyzer_lemma_for_form(orth, form)
}

pub(super) fn decode_analyzer_lemma_for_form<F>(orth: &str, form: &F) -> Result<String>
where
    F: AnalyzerFormView,
{
    let context = AnalyzerOrthContext::new(orth);
    decode_analyzer_lemma_for_form_in_context(&context, form)
}

pub(super) fn decode_analyzer_lemma_for_form_in_context<F>(
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

pub(super) fn decode_analyzer_prefix_lemma_for_form<F>(orth: &str, form: &F) -> Result<String>
where
    F: AnalyzerFormView,
{
    let normalized_len = AnalyzerOrthContext::new(orth).lowercase_codepoints_len;
    let start = form.prefix_to_cut() as usize;
    decode_analyzer_lemma_range(orth, start, normalized_len, form, false)
}

pub(super) fn decode_analyzer_lemma_with_prefix_context_len<F>(
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
pub(super) struct AnalyzerOrthContext<'a> {
    pub(super) orth: &'a str,
    pub(super) original_codepoints_len: usize,
    pub(super) lowercase_codepoints_len: usize,
    pub(super) lowercases_to_self: bool,
}

impl<'a> AnalyzerOrthContext<'a> {
    pub(super) fn new(orth: &'a str) -> Self {
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

pub(super) fn decode_analyzer_lemma_range<F>(
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
