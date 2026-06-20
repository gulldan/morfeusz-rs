use super::*;

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

    pub(super) fn fsa_variant_for_preset(
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

pub(super) fn explicit_segmentation_value<'a>(
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

pub(super) fn has_initial_segmentation_transitions(fsa: Option<&[u8]>) -> bool {
    fsa.and_then(|fsa| fsa.get(1)).copied().unwrap_or(0) > 0
}

pub(super) fn default_segmentation_fsa_variant_index(
    metadata: &SegmentationMetadata,
) -> Option<usize> {
    metadata
        .fsa_variants
        .iter()
        .position(|variant| variant.options == metadata.default_options)
}

pub(super) fn segmentation_fsa_for_options<'a>(
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

pub(super) fn available_options_vec(metadata: &SegmentationMetadata, option: &str) -> Vec<String> {
    metadata.available_options(option).into_iter().collect()
}

pub(super) fn validate_segmentation_options(
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

pub(super) fn effective_segmentation_options(
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

pub(super) fn format_available_options(metadata: &SegmentationMetadata, option: &str) -> String {
    metadata
        .available_options(option)
        .into_iter()
        .map(|value| format!("\"{value}\""))
        .collect::<Vec<_>>()
        .join(", ")
}

pub(super) fn format_options_map(options: &BTreeMap<String, String>) -> String {
    options
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join(", ")
}

impl<'a> SegmentationRulesFsa<'a> {
    pub fn new(data: &'a [u8]) -> Result<Self> {
        validate_min_len(data, 2, "segmentation FSA initial state")?;
        validate_reachable_segmentation_states(data)?;
        Ok(Self { data })
    }

    pub(super) fn from_data_unchecked(data: &'a [u8]) -> Self {
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
    pub(super) fn proceed_to_next_unchecked(
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

pub(super) fn parse_segmentation_metadata(data: &[u8]) -> Result<SegmentationMetadata> {
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

pub(super) fn read_options_map(
    data: &[u8],
    cursor: &mut usize,
) -> Result<BTreeMap<String, String>> {
    let options_count = read_u8(data, cursor, "segmentation options count")? as usize;
    let mut options = BTreeMap::new();
    for _ in 0..options_count {
        let key = read_c_string_from_slice(data, cursor, "segmentation option key")?;
        let value = read_c_string_from_slice(data, cursor, "segmentation option value")?;
        options.insert(key, value);
    }
    Ok(options)
}

pub(super) fn checked_transition_table_end(cursor: usize, transitions_num: usize) -> Result<usize> {
    transitions_num
        .checked_mul(SEGRULES_TRANSITION_SIZE)
        .and_then(|size| cursor.checked_add(size))
        .ok_or_else(|| Error::invalid_dictionary("segmentation FSA transition table overflow"))
}

pub(super) fn validate_reachable_segmentation_states(data: &[u8]) -> Result<()> {
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
