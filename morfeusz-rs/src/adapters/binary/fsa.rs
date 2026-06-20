use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawFsaMatch<'a> {
    pub state_offset: usize,
    pub value: &'a [u8],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawFsaPrefixMatch<'a> {
    pub input_end: usize,
    pub state_offset: usize,
    pub value: &'a [u8],
}

#[derive(Debug, Clone, Copy)]
pub enum BinaryFsa<'a> {
    Simple(SimpleFsa<'a>),
    VLength1(VLength1Fsa<'a>),
    VLength2(VLength2Fsa<'a>),
}

#[derive(Debug, Clone, Copy)]
pub struct SimpleFsa<'a> {
    pub(super) data: &'a [u8],
    pub(super) transition_size: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct VLength1Fsa<'a> {
    data: &'a [u8],
    short_labels: &'a [u8; 256],
}

#[derive(Debug, Clone, Copy)]
pub struct VLength2Fsa<'a> {
    data: &'a [u8],
}

impl<'a> BinaryFsa<'a> {
    pub fn try_recognize(&self, input: &[u8]) -> Result<Option<RawFsaMatch<'a>>> {
        match self {
            Self::Simple(fsa) => fsa.try_recognize(input),
            Self::VLength1(fsa) => fsa.try_recognize(input),
            Self::VLength2(fsa) => fsa.try_recognize(input),
        }
    }

    pub(super) fn try_recognize_loaded(&self, input: &[u8]) -> Result<Option<RawFsaMatch<'a>>> {
        match self {
            Self::Simple(fsa) => fsa.try_recognize(input),
            Self::VLength1(fsa) => Ok(fsa.try_recognize_loaded(input)),
            Self::VLength2(fsa) => fsa.try_recognize(input),
        }
    }

    pub fn prefix_matches(&self, input: &[u8]) -> Result<Vec<RawFsaPrefixMatch<'a>>> {
        match self {
            Self::Simple(fsa) => fsa.prefix_matches(input),
            Self::VLength1(fsa) => fsa.prefix_matches(input),
            Self::VLength2(fsa) => fsa.prefix_matches(input),
        }
    }

    pub(super) fn for_each_prefix_match_loaded<F>(&self, input: &[u8], visit: F) -> Result<()>
    where
        F: FnMut(RawFsaPrefixMatch<'a>) -> Result<()>,
    {
        match self {
            Self::Simple(fsa) => fsa.for_each_prefix_match(input, visit),
            Self::VLength1(fsa) => fsa.for_each_prefix_match_loaded(input, visit),
            Self::VLength2(fsa) => fsa.for_each_prefix_match(input, visit),
        }
    }
}

impl<'a> SimpleFsa<'a> {
    pub fn new(data: &'a [u8], has_transition_data: bool) -> Result<Self> {
        validate_min_len(data, 1, "Simple FSA initial state")?;
        Ok(Self {
            data,
            transition_size: if has_transition_data {
                SIMPLE_TRANSDUCER_TRANSITION_SIZE
            } else {
                SIMPLE_TRANSITION_SIZE
            },
        })
    }

    pub fn try_recognize(&self, input: &[u8]) -> Result<Option<RawFsaMatch<'a>>> {
        let mut state = RawFsaState::initial();
        for &byte in input {
            let Some(next_state) = self.proceed(byte, state)? else {
                return Ok(None);
            };
            state = next_state;
        }

        if state.accepting {
            Ok(Some(RawFsaMatch {
                state_offset: state.offset,
                value: state.value.unwrap_or(&[]),
            }))
        } else {
            Ok(None)
        }
    }

    pub fn prefix_matches(&self, input: &[u8]) -> Result<Vec<RawFsaPrefixMatch<'a>>> {
        let mut matches = Vec::new();
        self.for_each_prefix_match(input, |prefix_match| {
            matches.push(prefix_match);
            Ok(())
        })?;
        Ok(matches)
    }

    fn for_each_prefix_match<F>(&self, input: &[u8], mut visit: F) -> Result<()>
    where
        F: FnMut(RawFsaPrefixMatch<'a>) -> Result<()>,
    {
        let mut state = RawFsaState::initial();
        for (index, &byte) in input.iter().enumerate() {
            let Some(next_state) = self.proceed(byte, state)? else {
                break;
            };
            state = next_state;
            if state.accepting {
                visit(RawFsaPrefixMatch {
                    input_end: index + 1,
                    state_offset: state.offset,
                    value: state.value.unwrap_or(&[]),
                })?;
            }
        }

        Ok(())
    }

    fn proceed(&self, byte: u8, state: RawFsaState<'a>) -> Result<Option<RawFsaState<'a>>> {
        let state_header = read_simple_state_header(self.data, state.offset)?;
        let transitions_size = state_header
            .transitions_num
            .checked_mul(self.transition_size)
            .ok_or_else(|| Error::invalid_dictionary("Simple FSA transition table overflow"))?;
        let table_end = state_header
            .transitions_offset
            .checked_add(transitions_size)
            .ok_or_else(|| Error::invalid_dictionary("Simple FSA transition table overflow"))?;
        validate_min_len(self.data, table_end, "Simple FSA transitions")?;

        let mut cursor = state_header.transitions_offset;
        for _ in 0..state_header.transitions_num {
            let label = self.data[cursor];
            if label == byte {
                let next_offset = read_u24_at(self.data, cursor + 1, "Simple FSA target offset")?;
                return self.state_at(next_offset);
            }
            cursor += self.transition_size;
        }

        Ok(None)
    }

    fn state_at(&self, offset: usize) -> Result<Option<RawFsaState<'a>>> {
        let state_header = read_simple_state_header(self.data, offset)?;
        if state_header.accepting {
            Ok(Some(RawFsaState {
                offset,
                accepting: true,
                value: state_header.value,
                value_record_size: state_header.value_record_size,
                transitions_num: 0,
                transitions_offset: 0,
            }))
        } else {
            Ok(Some(RawFsaState {
                offset,
                accepting: false,
                value: None,
                value_record_size: 0,
                transitions_num: 0,
                transitions_offset: 0,
            }))
        }
    }
}

impl<'a> VLength1Fsa<'a> {
    pub fn new(data: &'a [u8]) -> Result<Self> {
        validate_min_len(data, V1_INITIAL_STATE_OFFSET, "VLength1 FSA prologue")?;
        if data[V1_INITIAL_STATE_OFFSET - 1] != b'^' {
            return Err(Error::invalid_dictionary(
                "VLength1 FSA prologue is missing magic marker",
            ));
        }
        let short_labels = data[..256]
            .try_into()
            .map_err(|_| Error::invalid_dictionary("VLength1 short-label table is truncated"))?;
        validate_reachable_vlength1_states(data)?;
        Ok(Self { data, short_labels })
    }

    pub(super) fn from_data_unchecked(data: &'a [u8]) -> Self {
        let short_labels = data[..256]
            .try_into()
            .expect("VLength1 FSA was validated when the binary lexicon loaded");
        Self { data, short_labels }
    }

    pub fn try_recognize(&self, input: &[u8]) -> Result<Option<RawFsaMatch<'a>>> {
        let Some(mut state) = self.state_at(V1_INITIAL_STATE_OFFSET)? else {
            return Ok(None);
        };
        for &byte in input {
            let Some(next_state) = self.proceed(byte, state)? else {
                return Ok(None);
            };
            state = next_state;
        }

        if state.accepting {
            Ok(Some(RawFsaMatch {
                state_offset: state.offset,
                value: state.value.unwrap_or(&[]),
            }))
        } else {
            Ok(None)
        }
    }

    #[cfg_attr(not(debug_assertions), inline(always))]
    fn try_recognize_loaded(&self, input: &[u8]) -> Option<RawFsaMatch<'a>> {
        let mut state = self.state_at_loaded(V1_INITIAL_STATE_OFFSET);
        for &byte in input {
            let next_state = self.proceed_loaded(byte, state)?;
            state = next_state;
        }

        if state.accepting {
            Some(RawFsaMatch {
                state_offset: state.offset,
                value: state.value.unwrap_or(&[]),
            })
        } else {
            None
        }
    }

    pub fn prefix_matches(&self, input: &[u8]) -> Result<Vec<RawFsaPrefixMatch<'a>>> {
        let mut matches = Vec::new();
        self.for_each_prefix_match(input, |prefix_match| {
            matches.push(prefix_match);
            Ok(())
        })?;
        Ok(matches)
    }

    fn for_each_prefix_match<F>(&self, input: &[u8], mut visit: F) -> Result<()>
    where
        F: FnMut(RawFsaPrefixMatch<'a>) -> Result<()>,
    {
        let Some(mut state) = self.state_at(V1_INITIAL_STATE_OFFSET)? else {
            return Ok(());
        };
        for (index, &byte) in input.iter().enumerate() {
            let Some(next_state) = self.proceed(byte, state)? else {
                break;
            };
            state = next_state;
            if state.accepting {
                visit(RawFsaPrefixMatch {
                    input_end: index + 1,
                    state_offset: state.offset,
                    value: state.value.unwrap_or(&[]),
                })?;
            }
        }

        Ok(())
    }

    #[cfg_attr(not(debug_assertions), inline(always))]
    fn for_each_prefix_match_loaded<F>(&self, input: &[u8], mut visit: F) -> Result<()>
    where
        F: FnMut(RawFsaPrefixMatch<'a>) -> Result<()>,
    {
        let mut state = self.state_at_loaded(V1_INITIAL_STATE_OFFSET);
        for (index, &byte) in input.iter().enumerate() {
            let Some(next_state) = self.proceed_loaded(byte, state) else {
                break;
            };
            state = next_state;
            if state.accepting {
                visit(RawFsaPrefixMatch {
                    input_end: index + 1,
                    state_offset: state.offset,
                    value: state.value.unwrap_or(&[]),
                })?;
            }
        }

        Ok(())
    }

    fn proceed(&self, byte: u8, state: RawFsaState<'a>) -> Result<Option<RawFsaState<'a>>> {
        let mut cursor = state.transitions_offset;
        let input_short_label = self.short_labels[byte as usize];

        for _ in 0..state.transitions_num {
            validate_min_len(self.data, cursor + 1, "VLength1 transition")?;
            let first = self.data[cursor];
            cursor += 1;
            let offset_size = (first & V1_OFFSET_SIZE_MASK) as usize;
            let transition_short_label = first >> 2;

            let label_matches = if transition_short_label == 0 {
                validate_min_len(self.data, cursor + 1, "VLength1 transition label")?;
                let label = self.data[cursor];
                cursor += 1;
                input_short_label == 0 && label == byte
            } else {
                transition_short_label == input_short_label
            };

            if label_matches {
                let offset_end = cursor + offset_size;
                validate_min_len(self.data, offset_end, "VLength1 offset")?;
                let relative_offset = read_vlength1_offset_at(self.data, cursor, offset_size);
                let next_offset = offset_end
                    .checked_add(relative_offset)
                    .ok_or_else(|| Error::invalid_dictionary("VLength1 transition overflow"))?;
                return self.state_at(next_offset);
            }
            cursor += offset_size;
            validate_min_len(self.data, cursor, "VLength1 offset")?;
        }

        Ok(None)
    }

    #[cfg_attr(not(debug_assertions), inline(always))]
    fn proceed_loaded(&self, byte: u8, state: RawFsaState<'a>) -> Option<RawFsaState<'a>> {
        let data = self.data;
        let mut cursor = state.transitions_offset;
        let input_short_label = self.short_labels[byte as usize];

        for _ in 0..state.transitions_num {
            let first = byte_at_unchecked(data, cursor);
            cursor += 1;
            let offset_size = (first & V1_OFFSET_SIZE_MASK) as usize;
            let transition_short_label = first >> 2;

            let label_matches = if transition_short_label == 0 {
                let label = byte_at_unchecked(data, cursor);
                cursor += 1;
                input_short_label == 0 && label == byte
            } else {
                transition_short_label == input_short_label
            };

            let offset_end = cursor + offset_size;

            if label_matches {
                let relative_offset = read_vlength1_offset_at(data, cursor, offset_size);
                let next_offset = offset_end + relative_offset;
                return Some(self.state_at_loaded(next_offset));
            }
            cursor = offset_end;
        }

        None
    }

    fn state_at(&self, offset: usize) -> Result<Option<RawFsaState<'a>>> {
        validate_min_len(self.data, offset + 1, "VLength1 target state")?;
        let state_header = read_vlength1_state_header(self.data, offset)?;
        let state_offset = offset
            .checked_sub(V1_INITIAL_STATE_OFFSET)
            .ok_or_else(|| Error::invalid_dictionary("VLength1 target precedes initial state"))?;

        if state_header.accepting {
            Ok(Some(RawFsaState {
                offset: state_offset,
                accepting: true,
                value: state_header.value,
                value_record_size: state_header.value_record_size,
                transitions_num: state_header.transitions_num,
                transitions_offset: state_header.transitions_offset,
            }))
        } else {
            Ok(Some(RawFsaState {
                offset: state_offset,
                accepting: false,
                value: None,
                value_record_size: 0,
                transitions_num: state_header.transitions_num,
                transitions_offset: state_header.transitions_offset,
            }))
        }
    }

    #[cfg_attr(not(debug_assertions), inline(always))]
    fn state_at_loaded(&self, offset: usize) -> RawFsaState<'a> {
        debug_assert!(offset >= V1_INITIAL_STATE_OFFSET);
        let state_header = read_vlength1_state_header_loaded(self.data, offset);
        let state_offset = offset - V1_INITIAL_STATE_OFFSET;

        if state_header.accepting {
            RawFsaState {
                offset: state_offset,
                accepting: true,
                value: state_header.value,
                value_record_size: state_header.value_record_size,
                transitions_num: state_header.transitions_num,
                transitions_offset: state_header.transitions_offset,
            }
        } else {
            RawFsaState {
                offset: state_offset,
                accepting: false,
                value: None,
                value_record_size: 0,
                transitions_num: state_header.transitions_num,
                transitions_offset: state_header.transitions_offset,
            }
        }
    }
}

impl<'a> VLength2Fsa<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data }
    }

    pub fn try_recognize(&self, input: &[u8]) -> Result<Option<RawFsaMatch<'a>>> {
        let mut state = RawFsaState::initial();
        for &byte in input {
            let Some(next_state) = self.proceed(byte, state)? else {
                return Ok(None);
            };
            state = next_state;
        }

        if state.accepting {
            Ok(Some(RawFsaMatch {
                state_offset: state.offset,
                value: state.value.unwrap_or(&[]),
            }))
        } else {
            Ok(None)
        }
    }

    pub fn prefix_matches(&self, input: &[u8]) -> Result<Vec<RawFsaPrefixMatch<'a>>> {
        let mut matches = Vec::new();
        self.for_each_prefix_match(input, |prefix_match| {
            matches.push(prefix_match);
            Ok(())
        })?;
        Ok(matches)
    }

    fn for_each_prefix_match<F>(&self, input: &[u8], mut visit: F) -> Result<()>
    where
        F: FnMut(RawFsaPrefixMatch<'a>) -> Result<()>,
    {
        let mut state = RawFsaState::initial();
        for (index, &byte) in input.iter().enumerate() {
            let Some(next_state) = self.proceed(byte, state)? else {
                break;
            };
            state = next_state;
            if state.accepting {
                visit(RawFsaPrefixMatch {
                    input_end: index + 1,
                    state_offset: state.offset,
                    value: state.value.unwrap_or(&[]),
                })?;
            }
        }

        Ok(())
    }

    fn proceed(&self, byte: u8, state: RawFsaState<'a>) -> Result<Option<RawFsaState<'a>>> {
        let mut cursor = state
            .offset
            .checked_add(state.value_record_size)
            .ok_or_else(|| Error::invalid_dictionary("FSA state offset overflow"))?;
        validate_min_len(self.data, cursor + 1, "VLength2 transition table")?;

        loop {
            let label = self.data[cursor];
            cursor += 1;
            if cursor >= self.data.len() && label & V2_LAST_FLAG != 0 {
                return Ok(None);
            }
            validate_min_len(self.data, cursor + 1, "VLength2 transition flags")?;
            let flags_offset = cursor;
            let flags = self.data[flags_offset];
            if label == byte {
                let relative_offset = read_vlength2_offset(self.data, &mut cursor)?;
                let next_offset = cursor
                    .checked_add(relative_offset)
                    .ok_or_else(|| Error::invalid_dictionary("VLength2 transition overflow"))?;
                validate_min_len(self.data, next_offset, "VLength2 target state")?;
                return RawFsaState::from_target(self.data, next_offset, flags);
            }

            if flags & V2_LAST_FLAG != 0 {
                return Ok(None);
            }
            skip_vlength2_offset(self.data, &mut cursor)?;
        }
    }
}

pub(super) fn validate_reachable_vlength1_states(data: &[u8]) -> Result<()> {
    let mut seen = OffsetBitSet::new(data.len());
    let mut stack = vec![V1_INITIAL_STATE_OFFSET];

    while let Some(offset) = stack.pop() {
        if !seen.insert(offset) {
            continue;
        }
        if offset < V1_INITIAL_STATE_OFFSET {
            return Err(Error::invalid_dictionary(
                "VLength1 target precedes initial state",
            ));
        }

        let state_header = read_vlength1_state_header(data, offset)?;
        let mut cursor = state_header.transitions_offset;
        for _ in 0..state_header.transitions_num {
            validate_min_len(data, cursor + 1, "VLength1 transition")?;
            let first = data[cursor];
            cursor += 1;
            let offset_size = (first & V1_OFFSET_SIZE_MASK) as usize;
            let transition_short_label = first >> 2;
            if transition_short_label == 0 {
                validate_min_len(data, cursor + 1, "VLength1 transition label")?;
                cursor += 1;
            }

            let offset_end = cursor
                .checked_add(offset_size)
                .ok_or_else(|| Error::invalid_dictionary("VLength1 offset overflow"))?;
            validate_min_len(data, offset_end, "VLength1 offset")?;
            let relative_offset = read_vlength1_offset_at(data, cursor, offset_size);
            let next_offset = offset_end
                .checked_add(relative_offset)
                .ok_or_else(|| Error::invalid_dictionary("VLength1 transition overflow"))?;
            if next_offset < V1_INITIAL_STATE_OFFSET {
                return Err(Error::invalid_dictionary(
                    "VLength1 target precedes initial state",
                ));
            }
            validate_min_len(data, next_offset + 1, "VLength1 target state")?;
            stack.push(next_offset);
            cursor = offset_end;
        }
    }

    Ok(())
}

pub(super) struct OffsetBitSet {
    words: Vec<u64>,
}

impl OffsetBitSet {
    fn new(max_offset: usize) -> Self {
        Self {
            words: vec![0; (max_offset / u64::BITS as usize) + 1],
        }
    }

    fn insert(&mut self, offset: usize) -> bool {
        let word_index = offset / u64::BITS as usize;
        let mask = 1u64 << (offset % u64::BITS as usize);
        let word = &mut self.words[word_index];
        if *word & mask != 0 {
            return false;
        }
        *word |= mask;
        true
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct SimpleStateHeader<'a> {
    accepting: bool,
    value: Option<&'a [u8]>,
    value_record_size: usize,
    transitions_num: usize,
    transitions_offset: usize,
}

pub(super) fn read_simple_state_header<'a>(
    data: &'a [u8],
    offset: usize,
) -> Result<SimpleStateHeader<'a>> {
    validate_min_len(data, offset + 1, "Simple FSA state header")?;
    let first = data[offset];
    let accepting = first & SIMPLE_ACCEPTING_FLAG != 0;
    let transitions_num = (first & SIMPLE_TRANSITIONS_NUM_MASK) as usize;
    let mut transitions_offset = offset + 1;
    let mut value = None;
    let mut value_record_size = 0;
    if accepting {
        let (payload, size) = read_morph_payload(data, transitions_offset)?;
        value = Some(payload);
        value_record_size = size;
        transitions_offset += value_record_size;
    }

    Ok(SimpleStateHeader {
        accepting,
        value,
        value_record_size,
        transitions_num,
        transitions_offset,
    })
}

#[derive(Debug, Clone, Copy)]
pub(super) struct VLength1StateHeader<'a> {
    accepting: bool,
    transitions_num: usize,
    value: Option<&'a [u8]>,
    value_record_size: usize,
    transitions_offset: usize,
}

pub(super) fn read_vlength1_state_header<'a>(
    data: &'a [u8],
    offset: usize,
) -> Result<VLength1StateHeader<'a>> {
    validate_min_len(data, offset + 1, "VLength1 state header")?;
    let first = data[offset];
    let accepting = first & V1_ACCEPTING_FLAG != 0;
    let mut transitions_num = (first & V1_TRANSITIONS_NUM_MASK) as usize;
    let mut cursor = offset + 1;
    if transitions_num == V1_TRANSITIONS_NUM_MASK as usize {
        validate_min_len(data, cursor + 1, "VLength1 extended transitions count")?;
        transitions_num = data[cursor] as usize;
        cursor += 1;
    }

    let mut value = None;
    let mut value_record_size = 0;
    if accepting {
        let (payload, size) = read_morph_payload(data, cursor)?;
        value = Some(payload);
        value_record_size = size;
        cursor += value_record_size;
    }

    Ok(VLength1StateHeader {
        accepting,
        transitions_num,
        value,
        value_record_size,
        transitions_offset: cursor,
    })
}

#[cfg_attr(not(debug_assertions), inline(always))]
pub(super) fn read_vlength1_state_header_loaded<'a>(
    data: &'a [u8],
    offset: usize,
) -> VLength1StateHeader<'a> {
    let first = byte_at_unchecked(data, offset);
    let accepting = first & V1_ACCEPTING_FLAG != 0;
    let mut transitions_num = (first & V1_TRANSITIONS_NUM_MASK) as usize;
    let mut cursor = offset + 1;
    if transitions_num == V1_TRANSITIONS_NUM_MASK as usize {
        transitions_num = byte_at_unchecked(data, cursor) as usize;
        cursor += 1;
    }

    let mut value = None;
    let mut value_record_size = 0;
    if accepting {
        let (payload, size) = read_morph_payload_loaded(data, cursor);
        value = Some(payload);
        value_record_size = size;
        cursor += value_record_size;
    }

    VLength1StateHeader {
        accepting,
        transitions_num,
        value,
        value_record_size,
        transitions_offset: cursor,
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct RawFsaState<'a> {
    offset: usize,
    accepting: bool,
    value: Option<&'a [u8]>,
    value_record_size: usize,
    transitions_num: usize,
    transitions_offset: usize,
}

impl<'a> RawFsaState<'a> {
    fn initial() -> Self {
        Self {
            offset: 0,
            accepting: false,
            value: None,
            value_record_size: 0,
            transitions_num: 0,
            transitions_offset: 0,
        }
    }

    fn from_target(data: &'a [u8], offset: usize, flags: u8) -> Result<Option<Self>> {
        let accepting = flags & V2_ACCEPTING_FLAG != 0;
        if !accepting {
            return Ok(Some(Self {
                offset,
                accepting,
                value: None,
                value_record_size: 0,
                transitions_num: 0,
                transitions_offset: 0,
            }));
        }

        let (value, value_record_size) = read_morph_payload(data, offset)?;
        Ok(Some(Self {
            offset,
            accepting,
            value: Some(value),
            value_record_size,
            transitions_num: 0,
            transitions_offset: 0,
        }))
    }
}
