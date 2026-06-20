use std::collections::BTreeMap;

use super::automaton::build_segment_rules_fsa;
use super::rule::SegmentRule;
use super::types::{
    SegmentRulesFsa, SegmentRulesFsaLayout, SegmentRulesFsaVariantData, SegmentRulesState,
};
use crate::constants::MAX_SEGMENT_RULES_FSA_SIZE;
use crate::error::{validate, BuilderError, Result};
use crate::serialization::{serialize_legacy_string, serialize_u16_be, serialize_u32_be};

pub fn serialize_segment_rules_fsa<'a, I>(rules: I, input_name: &str) -> Result<Vec<u8>>
where
    I: IntoIterator<Item = &'a SegmentRule>,
{
    let fsa = build_segment_rules_fsa(rules, input_name)?;
    serialize_segment_rules_fsa_data(&fsa)
}

pub fn calculate_segment_rules_fsa_layout(fsa: &SegmentRulesFsa) -> Result<SegmentRulesFsaLayout> {
    validate_segment_rules_fsa(fsa)?;
    let dfs_order = segment_rules_fsa_dfs_order(fsa)?;
    let mut reverse_offsets = vec![0; fsa.states.len()];
    let mut current_reverse_offset = 0usize;

    for &state_index in &dfs_order {
        current_reverse_offset += segment_rules_state_size(&fsa.states[state_index])?;
        reverse_offsets[state_index] = current_reverse_offset;
    }

    let total_size = current_reverse_offset;
    let mut offsets = vec![0; fsa.states.len()];
    for &state_index in &dfs_order {
        offsets[state_index] = total_size - reverse_offsets[state_index];
    }

    Ok(SegmentRulesFsaLayout {
        dfs_order,
        offsets,
        reverse_offsets,
        total_size,
    })
}

pub fn serialize_segment_rules_fsa_data(fsa: &SegmentRulesFsa) -> Result<Vec<u8>> {
    let layout = calculate_segment_rules_fsa_layout(fsa)?;
    validate(
        layout.total_size <= MAX_SEGMENT_RULES_FSA_SIZE,
        format!(
            "Segmentation rules are too big and complicated- the resulting automaton would exceed its max size which is {}",
            MAX_SEGMENT_RULES_FSA_SIZE
        ),
    )?;

    let mut ordered_states = layout.dfs_order.clone();
    ordered_states.sort_by_key(|state_index| layout.offsets[*state_index]);

    let mut out = Vec::with_capacity(layout.total_size);
    for state_index in ordered_states {
        out.extend(serialize_segment_rules_state(
            &fsa.states[state_index],
            &layout,
        )?);
    }
    Ok(out)
}

pub fn serialize_segmentation_rules_data<I>(
    separators: I,
    variants: &[SegmentRulesFsaVariantData],
    default_options: &BTreeMap<String, String>,
) -> Result<Vec<u8>>
where
    I: IntoIterator<Item = u32>,
{
    validate(
        !variants.is_empty() && variants.len() < 256,
        "Too many segmentation rules variants",
    )?;

    let mut out = Vec::new();
    let mut separators: Vec<u32> = separators.into_iter().collect();
    separators.sort_unstable();
    out.extend(serialize_u16_be(separators.len())?);
    for separator in separators {
        out.extend(serialize_u32_be(separator as usize)?);
    }

    out.push(variants.len() as u8);
    for variant in variants {
        out.extend(serialize_segment_rules_options_map(&variant.options)?);
        out.extend(serialize_u32_be(variant.fsa.len())?);
        out.extend(&variant.fsa);
    }
    out.extend(serialize_segment_rules_options_map(default_options)?);
    Ok(out)
}

fn validate_segment_rules_fsa(fsa: &SegmentRulesFsa) -> Result<()> {
    validate(!fsa.states.is_empty(), "empty segmentation rules FSA")?;
    validate(
        fsa.initial_state < fsa.states.len(),
        format!(
            "invalid segmentation rules initial state index: {}",
            fsa.initial_state
        ),
    )?;
    for (state_index, state) in fsa.states.iter().enumerate() {
        validate(
            state.transitions.len() <= u8::MAX as usize,
            format!(
                "segmentation rules state {state_index} has too many transitions: {}",
                state.transitions.len()
            ),
        )?;
        validate(
            state.transition_order.len() == state.transitions.len(),
            format!("segmentation rules state {state_index} transition order is inconsistent"),
        )?;
        for label in &state.transition_order {
            validate(
                state.transitions.contains_key(label),
                format!("segmentation rules state {state_index} transition order has stale label"),
            )?;
        }
        for target in state.transitions.values() {
            validate(
                *target < fsa.states.len(),
                format!("segmentation rules state {state_index} targets invalid state {target}"),
            )?;
        }
    }
    Ok(())
}

fn segment_rules_fsa_dfs_order(fsa: &SegmentRulesFsa) -> Result<Vec<usize>> {
    let mut visited = vec![false; fsa.states.len()];
    let mut order = Vec::new();
    segment_rules_fsa_dfs_visit(fsa, fsa.initial_state, &mut visited, &mut order)?;
    Ok(order)
}

fn segment_rules_fsa_dfs_visit(
    fsa: &SegmentRulesFsa,
    state_index: usize,
    visited: &mut [bool],
    order: &mut Vec<usize>,
) -> Result<()> {
    if visited[state_index] {
        return Ok(());
    }
    visited[state_index] = true;

    for label in &fsa.states[state_index].transition_order {
        let target = fsa.states[state_index]
            .transitions
            .get(label)
            .ok_or_else(|| {
                BuilderError::new(format!(
                    "segmentation rules state {state_index} transition order has stale label"
                ))
            })?;
        validate(
            *target < fsa.states.len(),
            format!("segmentation rules state {state_index} targets invalid state {target}"),
        )?;
        segment_rules_fsa_dfs_visit(fsa, *target, visited, order)?;
    }

    order.push(state_index);
    Ok(())
}

fn segment_rules_state_size(state: &SegmentRulesState) -> Result<usize> {
    validate(
        state.transitions.len() <= u8::MAX as usize,
        format!(
            "segmentation rules state has too many transitions: {}",
            state.transitions.len()
        ),
    )?;
    Ok(2 + 4 * state.transitions.len())
}

fn serialize_segment_rules_state(
    state: &SegmentRulesState,
    layout: &SegmentRulesFsaLayout,
) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(segment_rules_state_size(state)?);
    let mut flags = 0u8;
    if state.accepting {
        flags |= 1;
    }
    if state.weak {
        flags |= 2;
    }
    out.push(flags);
    out.push(state.transitions.len() as u8);
    for (label, target) in &state.transitions {
        let target_offset = layout.offsets[*target];
        validate(
            target_offset <= MAX_SEGMENT_RULES_FSA_SIZE,
            format!(
                "Segmentation rules are too big and complicated- the resulting automaton would exceed its max size which is {}",
                MAX_SEGMENT_RULES_FSA_SIZE
            ),
        )?;
        out.push(label.segment_type_num);
        out.push(u8::from(label.shift_orth));
        out.extend(serialize_u16_be(target_offset)?);
    }
    Ok(out)
}

fn serialize_segment_rules_options_map(options: &BTreeMap<String, String>) -> Result<Vec<u8>> {
    let aggl = options
        .get("aggl")
        .ok_or_else(|| BuilderError::new("segmentation options missing aggl"))?;
    let praet = options
        .get("praet")
        .ok_or_else(|| BuilderError::new("segmentation options missing praet"))?;
    let mut out = Vec::new();
    out.push(2);
    out.extend(serialize_legacy_string("aggl"));
    out.extend(serialize_legacy_string(aggl));
    out.extend(serialize_legacy_string("praet"));
    out.extend(serialize_legacy_string(praet));
    Ok(out)
}
