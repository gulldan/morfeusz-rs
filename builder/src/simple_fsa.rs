use std::collections::BTreeMap;

use crate::error::{validate, BuilderError, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimpleTransition {
    pub label: u8,
    pub target_offset: usize,
    pub transition_data: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SimpleState {
    pub encoded_data: Option<Vec<u8>>,
    pub transitions: Vec<SimpleTransition>,
    pub label_frequencies: BTreeMap<u8, usize>,
}

impl SimpleState {
    pub fn accepting(encoded_data: impl Into<Vec<u8>>) -> Self {
        Self {
            encoded_data: Some(encoded_data.into()),
            transitions: Vec::new(),
            label_frequencies: BTreeMap::new(),
        }
    }

    pub fn non_accepting() -> Self {
        Self::default()
    }

    pub fn with_transition(
        mut self,
        label: u8,
        target_offset: usize,
        transition_data: Option<u8>,
    ) -> Self {
        self.transitions.push(SimpleTransition {
            label,
            target_offset,
            transition_data,
        });
        self
    }

    pub fn with_label_frequency(mut self, label: u8, frequency: usize) -> Self {
        self.label_frequencies.insert(label, frequency);
        self
    }

    pub fn is_accepting(&self) -> bool {
        self.encoded_data.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimpleGraphTransition {
    pub label: u8,
    pub target: usize,
    pub transition_data: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SimpleGraphState {
    pub encoded_data: Option<Vec<u8>>,
    pub transitions: Vec<SimpleGraphTransition>,
    pub frequency: usize,
    pub label_frequencies: BTreeMap<u8, usize>,
}

impl SimpleGraphState {
    pub fn accepting(encoded_data: impl Into<Vec<u8>>) -> Self {
        Self {
            encoded_data: Some(encoded_data.into()),
            transitions: Vec::new(),
            frequency: 0,
            label_frequencies: BTreeMap::new(),
        }
    }

    pub fn non_accepting() -> Self {
        Self::default()
    }

    pub fn with_frequency(mut self, frequency: usize) -> Self {
        self.frequency = frequency;
        self
    }

    pub fn with_transition(
        mut self,
        label: u8,
        target: usize,
        transition_data: Option<u8>,
    ) -> Self {
        self.transitions.push(SimpleGraphTransition {
            label,
            target,
            transition_data,
        });
        self
    }

    pub fn with_label_frequency(mut self, label: u8, frequency: usize) -> Self {
        self.label_frequencies.insert(label, frequency);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimpleFsaGraph {
    pub states: Vec<SimpleGraphState>,
    pub initial_state: usize,
    pub global_label_frequencies: BTreeMap<u8, usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimpleGraphLayout {
    pub dfs_order: Vec<usize>,
    pub offsets: Vec<usize>,
    pub reverse_offsets: Vec<usize>,
    pub total_size: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SimpleBuildState {
    encoded_data: Option<Vec<u8>>,
    transitions: Vec<(u8, usize)>,
}

impl SimpleBuildState {
    fn new() -> Self {
        Self {
            encoded_data: None,
            transitions: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SimpleBuildStateKey {
    transitions: BTreeMap<u8, usize>,
    encoded_data: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
struct SortedSimpleFsaBuilder {
    states: Vec<SimpleBuildState>,
    register: BTreeMap<SimpleBuildStateKey, usize>,
    initial_state: usize,
    previous_word: Option<Vec<u8>>,
    entries_num: usize,
    global_label_frequencies: BTreeMap<u8, usize>,
}

pub fn simple_implementation_code(serialize_transition_data: bool) -> u8 {
    if serialize_transition_data {
        128
    } else {
        0
    }
}

pub fn simple_state_size(state: &SimpleState, serialize_transition_data: bool) -> Result<usize> {
    Ok(
        1 + if serialize_transition_data { 5 } else { 4 } * state.transitions.len()
            + if state.is_accepting() {
                state
                    .encoded_data
                    .as_ref()
                    .map(Vec::len)
                    .unwrap_or_default()
            } else {
                0
            },
    )
}

pub fn serialize_simple_state_data(state: &SimpleState) -> Result<Vec<u8>> {
    validate(
        state.transitions.len() <= 127,
        format!(
            "simple state has too many transitions: {}",
            state.transitions.len()
        ),
    )?;

    let mut first_byte = state.transitions.len();
    if state.is_accepting() {
        first_byte |= 128;
    }
    validate(
        first_byte > 0 && first_byte < 256,
        "simple state must be accepting or have transitions",
    )?;

    let mut out = vec![first_byte as u8];
    if let Some(encoded_data) = &state.encoded_data {
        out.extend(encoded_data);
    }
    Ok(out)
}

pub fn serialize_simple_transitions(
    state: &SimpleState,
    serialize_transition_data: bool,
    global_label_frequencies: &BTreeMap<u8, usize>,
) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    for transition in sorted_simple_transitions(state, global_label_frequencies) {
        validate(
            transition.target_offset < 256 * 256 * 256,
            format!(
                "simple transition offset {} exceeds 24-bit limit",
                transition.target_offset
            ),
        )?;
        out.push(transition.label);
        out.push(((transition.target_offset & 0xFF0000) >> 16) as u8);
        out.push(((transition.target_offset & 0x00FF00) >> 8) as u8);
        out.push((transition.target_offset & 0x0000FF) as u8);
        if serialize_transition_data {
            out.push(transition.transition_data.ok_or_else(|| {
                BuilderError::new(format!(
                    "missing transition data for label {}",
                    transition.label
                ))
            })?);
        }
    }
    Ok(out)
}

pub fn serialize_simple_state(
    state: &SimpleState,
    serialize_transition_data: bool,
    global_label_frequencies: &BTreeMap<u8, usize>,
) -> Result<Vec<u8>> {
    let mut out = serialize_simple_state_data(state)?;
    out.extend(serialize_simple_transitions(
        state,
        serialize_transition_data,
        global_label_frequencies,
    )?);
    Ok(out)
}

pub fn calculate_simple_graph_layout(
    graph: &SimpleFsaGraph,
    serialize_transition_data: bool,
) -> Result<SimpleGraphLayout> {
    validate_simple_graph(graph)?;
    let dfs_order = simple_graph_dfs_order(graph)?;
    let mut reverse_offsets = vec![0; graph.states.len()];
    let mut current_reverse_offset = 0usize;

    for &state_index in &dfs_order {
        current_reverse_offset +=
            simple_graph_state_size(&graph.states[state_index], serialize_transition_data)?;
        reverse_offsets[state_index] = current_reverse_offset;
    }

    let total_size = current_reverse_offset;
    let mut offsets = vec![0; graph.states.len()];
    for &state_index in &dfs_order {
        offsets[state_index] = total_size - reverse_offsets[state_index];
    }

    Ok(SimpleGraphLayout {
        dfs_order,
        offsets,
        reverse_offsets,
        total_size,
    })
}

pub fn serialize_simple_fsa_data(
    graph: &SimpleFsaGraph,
    serialize_transition_data: bool,
) -> Result<Vec<u8>> {
    let layout = calculate_simple_graph_layout(graph, serialize_transition_data)?;
    let mut ordered_states = layout.dfs_order.clone();
    ordered_states.sort_by_key(|state_index| layout.offsets[*state_index]);

    let mut out = Vec::with_capacity(layout.total_size);
    for state_index in ordered_states {
        let state = simple_state_for_graph_state(graph, &layout, state_index);
        out.extend(serialize_simple_state(
            &state,
            serialize_transition_data,
            &graph.global_label_frequencies,
        )?);
    }
    Ok(out)
}

pub fn build_simple_fsa_from_sorted_entries<I, W, D>(entries: I) -> Result<SimpleFsaGraph>
where
    I: IntoIterator<Item = (W, D)>,
    W: AsRef<[u8]>,
    D: AsRef<[u8]>,
{
    let mut builder = SortedSimpleFsaBuilder::new();
    for (word, data) in entries {
        builder.add_entry(word.as_ref(), data.as_ref())?;
    }
    builder.close()
}

fn sorted_simple_transitions<'a>(
    state: &'a SimpleState,
    global_label_frequencies: &BTreeMap<u8, usize>,
) -> Vec<&'a SimpleTransition> {
    let mut indexed: Vec<(usize, &SimpleTransition)> =
        state.transitions.iter().enumerate().collect();
    indexed.sort_by(|(left_index, left), (right_index, right)| {
        let left_local = state
            .label_frequencies
            .get(&left.label)
            .copied()
            .unwrap_or_default();
        let right_local = state
            .label_frequencies
            .get(&right.label)
            .copied()
            .unwrap_or_default();
        let left_global = global_label_frequencies
            .get(&left.label)
            .copied()
            .unwrap_or_default();
        let right_global = global_label_frequencies
            .get(&right.label)
            .copied()
            .unwrap_or_default();

        right_local
            .cmp(&left_local)
            .then(right_global.cmp(&left_global))
            .then(left_index.cmp(right_index))
    });
    indexed
        .into_iter()
        .map(|(_index, transition)| transition)
        .collect()
}

fn validate_simple_graph(graph: &SimpleFsaGraph) -> Result<()> {
    validate(
        graph.initial_state < graph.states.len(),
        format!("invalid initial state index: {}", graph.initial_state),
    )?;
    for (state_index, state) in graph.states.iter().enumerate() {
        for transition in &state.transitions {
            validate(
                transition.target < graph.states.len(),
                format!(
                    "state {state_index} transition {} targets invalid state {}",
                    transition.label, transition.target
                ),
            )?;
        }
    }
    Ok(())
}

fn simple_graph_dfs_order(graph: &SimpleFsaGraph) -> Result<Vec<usize>> {
    let mut visited = vec![false; graph.states.len()];
    let mut order = Vec::new();
    simple_graph_dfs_visit(graph, graph.initial_state, &mut visited, &mut order)?;
    Ok(order)
}

fn simple_graph_dfs_visit(
    graph: &SimpleFsaGraph,
    state_index: usize,
    visited: &mut [bool],
    order: &mut Vec<usize>,
) -> Result<()> {
    if visited[state_index] {
        return Ok(());
    }
    visited[state_index] = true;

    let mut transitions: Vec<(usize, &SimpleGraphTransition)> = graph.states[state_index]
        .transitions
        .iter()
        .enumerate()
        .collect();
    transitions.sort_by(|(left_index, left), (right_index, right)| {
        graph.states[right.target]
            .frequency
            .cmp(&graph.states[left.target].frequency)
            .then(left_index.cmp(right_index))
    });

    for (_index, transition) in transitions {
        validate(
            transition.target < graph.states.len(),
            format!(
                "state {state_index} transition {} targets invalid state {}",
                transition.label, transition.target
            ),
        )?;
        simple_graph_dfs_visit(graph, transition.target, visited, order)?;
    }

    order.push(state_index);
    Ok(())
}

fn simple_graph_state_size(
    state: &SimpleGraphState,
    serialize_transition_data: bool,
) -> Result<usize> {
    let simple_state = SimpleState {
        encoded_data: state.encoded_data.clone(),
        transitions: state
            .transitions
            .iter()
            .map(|transition| SimpleTransition {
                label: transition.label,
                target_offset: 0,
                transition_data: transition.transition_data,
            })
            .collect(),
        label_frequencies: state.label_frequencies.clone(),
    };
    simple_state_size(&simple_state, serialize_transition_data)
}

fn simple_state_for_graph_state(
    graph: &SimpleFsaGraph,
    layout: &SimpleGraphLayout,
    state_index: usize,
) -> SimpleState {
    let graph_state = &graph.states[state_index];
    SimpleState {
        encoded_data: graph_state.encoded_data.clone(),
        transitions: graph_state
            .transitions
            .iter()
            .map(|transition| SimpleTransition {
                label: transition.label,
                target_offset: layout.offsets[transition.target],
                transition_data: transition.transition_data,
            })
            .collect(),
        label_frequencies: graph_state.label_frequencies.clone(),
    }
}

impl SortedSimpleFsaBuilder {
    fn new() -> Self {
        Self {
            states: vec![SimpleBuildState::new()],
            register: BTreeMap::new(),
            initial_state: 0,
            previous_word: None,
            entries_num: 0,
            global_label_frequencies: BTreeMap::new(),
        }
    }

    fn add_entry(&mut self, word: &[u8], data: &[u8]) -> Result<()> {
        validate(!word.is_empty(), "entry word must not be empty")?;
        if let Some(previous_word) = &self.previous_word {
            validate(
                previous_word.as_slice() < word,
                "input entries must be strictly sorted by encoded word",
            )?;
        }

        let mut state_index = self.initial_state;
        let mut prefix_len = 0usize;
        while prefix_len < word.len() {
            let Some(next_index) = self.transition_target(state_index, word[prefix_len]) else {
                break;
            };
            state_index = next_index;
            prefix_len += 1;
        }

        if let Some(previous_word) = self.previous_word.clone() {
            if prefix_len < previous_word.len() {
                let label = previous_word[prefix_len];
                let next_state = self.transition_target(state_index, label).ok_or_else(|| {
                    BuilderError::new(format!(
                        "missing previous-word transition {} at prefix length {prefix_len}",
                        label
                    ))
                })?;
                let replacement =
                    self.replace_or_register(next_state, &previous_word[prefix_len + 1..])?;
                self.set_transition(state_index, label, replacement);
            }
        }

        while prefix_len < word.len() {
            let next_state = self.add_state();
            self.set_transition(state_index, word[prefix_len], next_state);
            state_index = next_state;
            prefix_len += 1;
        }

        validate(
            self.states[state_index].encoded_data.is_none(),
            "duplicate encoded word",
        )?;
        self.states[state_index].encoded_data = Some(data.to_vec());
        self.previous_word = Some(word.to_vec());
        self.entries_num += 1;
        for &label in word {
            *self.global_label_frequencies.entry(label).or_default() += 1;
        }
        Ok(())
    }

    fn close(mut self) -> Result<SimpleFsaGraph> {
        validate(self.entries_num > 0, "empty input")?;
        let previous_word = self
            .previous_word
            .clone()
            .ok_or_else(|| BuilderError::new("empty input"))?;
        self.initial_state = self.replace_or_register(self.initial_state, &previous_word)?;
        Ok(self.into_graph())
    }

    fn add_state(&mut self) -> usize {
        let index = self.states.len();
        self.states.push(SimpleBuildState::new());
        index
    }

    fn transition_target(&self, state_index: usize, label: u8) -> Option<usize> {
        self.states[state_index]
            .transitions
            .iter()
            .find_map(|(transition_label, target)| (*transition_label == label).then_some(*target))
    }

    fn set_transition(&mut self, state_index: usize, label: u8, target: usize) {
        if let Some((_transition_label, transition_target)) = self.states[state_index]
            .transitions
            .iter_mut()
            .find(|(transition_label, _target)| *transition_label == label)
        {
            *transition_target = target;
        } else {
            self.states[state_index].transitions.push((label, target));
        }
    }

    fn replace_or_register(&mut self, state_index: usize, encoded_word: &[u8]) -> Result<usize> {
        if let Some((&label, suffix)) = encoded_word.split_first() {
            let next_state = self.transition_target(state_index, label).ok_or_else(|| {
                BuilderError::new(format!(
                    "missing transition {label} during state minimization"
                ))
            })?;
            let replacement = self.replace_or_register(next_state, suffix)?;
            self.set_transition(state_index, label, replacement);
        }

        let key = self.register_key(state_index);
        if let Some(equivalent_state) = self.register.get(&key) {
            Ok(*equivalent_state)
        } else {
            self.register.insert(key, state_index);
            Ok(state_index)
        }
    }

    fn register_key(&self, state_index: usize) -> SimpleBuildStateKey {
        SimpleBuildStateKey {
            transitions: self.states[state_index]
                .transitions
                .iter()
                .copied()
                .collect(),
            encoded_data: self.states[state_index].encoded_data.clone(),
        }
    }

    fn into_graph(self) -> SimpleFsaGraph {
        let mut old_to_new = BTreeMap::new();
        let mut states = Vec::new();
        self.copy_reachable_state(self.initial_state, &mut old_to_new, &mut states);
        SimpleFsaGraph {
            states,
            initial_state: old_to_new[&self.initial_state],
            global_label_frequencies: self.global_label_frequencies,
        }
    }

    fn copy_reachable_state(
        &self,
        old_index: usize,
        old_to_new: &mut BTreeMap<usize, usize>,
        states: &mut Vec<SimpleGraphState>,
    ) -> usize {
        if let Some(new_index) = old_to_new.get(&old_index) {
            return *new_index;
        }

        let new_index = states.len();
        old_to_new.insert(old_index, new_index);
        states.push(SimpleGraphState {
            encoded_data: self.states[old_index].encoded_data.clone(),
            transitions: Vec::new(),
            frequency: 0,
            label_frequencies: BTreeMap::new(),
        });

        let transitions = self.states[old_index].transitions.clone();
        for (label, old_target) in transitions {
            let new_target = self.copy_reachable_state(old_target, old_to_new, states);
            states[new_index].transitions.push(SimpleGraphTransition {
                label,
                target: new_target,
                transition_data: None,
            });
        }

        new_index
    }
}
