use super::*;

pub(super) fn validate_min_len(bytes: &[u8], required: usize, section: &str) -> Result<()> {
    if bytes.len() < required {
        Err(Error::invalid_dictionary(format!(
            "truncated {section}: need at least {required} bytes, got {}",
            bytes.len()
        )))
    } else {
        Ok(())
    }
}

pub(super) fn read_u32_at(bytes: &[u8], offset: usize, field: &str) -> Result<u32> {
    validate_min_len(bytes, offset + 4, field)?;
    Ok(u32::from_be_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("validated length"),
    ))
}

pub(super) fn read_u24_at(bytes: &[u8], offset: usize, field: &str) -> Result<usize> {
    validate_min_len(bytes, offset + 3, field)?;
    Ok(((bytes[offset] as usize) << 16)
        | ((bytes[offset + 1] as usize) << 8)
        | bytes[offset + 2] as usize)
}

pub(super) fn read_c_string_at(
    bytes: &[u8],
    offset: usize,
    field: &str,
) -> Result<(String, usize)> {
    validate_min_len(bytes, offset, field)?;
    let relative_end = bytes[offset..]
        .iter()
        .position(|byte| *byte == 0)
        .ok_or_else(|| Error::invalid_dictionary(format!("unterminated {field}")))?;
    let end = offset + relative_end;
    let value = String::from_utf8(bytes[offset..end].to_vec())
        .map_err(|_| Error::invalid_dictionary(format!("{field} is not valid UTF-8")))?;
    Ok((value, end + 1))
}

pub(super) fn read_c_string_at_limit(
    bytes: &[u8],
    offset: usize,
    limit: usize,
    field: &str,
) -> Result<(String, usize)> {
    if offset > limit || limit > bytes.len() {
        return Err(Error::invalid_dictionary(format!(
            "{field} offset is outside metadata"
        )));
    }
    let relative_end = bytes[offset..limit]
        .iter()
        .position(|byte| *byte == 0)
        .ok_or_else(|| Error::invalid_dictionary(format!("unterminated {field}")))?;
    let end = offset + relative_end;
    let value = String::from_utf8(bytes[offset..end].to_vec())
        .map_err(|_| Error::invalid_dictionary(format!("{field} is not valid UTF-8")))?;
    Ok((value, end + 1))
}

pub(super) fn read_id_string_table(
    bytes: &[u8],
    cursor: &mut usize,
    limit: usize,
    mut set_value: impl FnMut(i32, &str),
) -> Result<()> {
    validate_limit(bytes, *cursor + 2, limit, "id string table size")?;
    let entries = u16::from_be_bytes([bytes[*cursor], bytes[*cursor + 1]]) as usize;
    *cursor += 2;

    for _ in 0..entries {
        validate_limit(bytes, *cursor + 2, limit, "id string table id")?;
        let id = u16::from_be_bytes([bytes[*cursor], bytes[*cursor + 1]]) as i32;
        *cursor += 2;
        let (value, next) = read_c_string_at_limit(bytes, *cursor, limit, "id string value")?;
        set_value(id, &value);
        *cursor = next;
    }

    Ok(())
}

pub(super) fn validate_limit(
    bytes: &[u8],
    required: usize,
    limit: usize,
    section: &str,
) -> Result<()> {
    if required > limit || required > bytes.len() {
        Err(Error::invalid_dictionary(format!(
            "truncated {section}: need offset {required}, metadata limit is {limit}"
        )))
    } else {
        Ok(())
    }
}

pub(super) fn read_morph_payload(data: &[u8], offset: usize) -> Result<(&[u8], usize)> {
    validate_min_len(data, offset + 2, "morph payload size")?;
    let size = u16::from_be_bytes([data[offset], data[offset + 1]]) as usize;
    let value_start = offset + 2;
    let value_end = value_start
        .checked_add(size)
        .ok_or_else(|| Error::invalid_dictionary("morph payload size overflow"))?;
    validate_min_len(data, value_end, "morph payload")?;
    Ok((&data[value_start..value_end], size + 2))
}

#[cfg_attr(not(debug_assertions), inline(always))]
pub(super) fn read_morph_payload_loaded(data: &[u8], offset: usize) -> (&[u8], usize) {
    let size = u16::from_be_bytes([
        byte_at_unchecked(data, offset),
        byte_at_unchecked(data, offset + 1),
    ]) as usize;
    let value_start = offset + 2;
    let value_end = value_start + size;
    debug_assert!(value_end <= data.len());
    let value = unsafe {
        // VLength1 loaded traversal is only constructed after
        // `validate_reachable_vlength1_states`, which validates every reachable
        // accepting payload boundary.
        data.get_unchecked(value_start..value_end)
    };
    (value, size + 2)
}

pub(super) fn read_u8(data: &[u8], cursor: &mut usize, field: &str) -> Result<u8> {
    validate_min_len(data, *cursor + 1, field)?;
    let value = data[*cursor];
    *cursor += 1;
    Ok(value)
}

pub(super) fn read_u16(data: &[u8], cursor: &mut usize, field: &str) -> Result<u16> {
    validate_min_len(data, *cursor + 2, field)?;
    let value = u16::from_be_bytes([data[*cursor], data[*cursor + 1]]);
    *cursor += 2;
    Ok(value)
}

pub(super) fn read_u32(data: &[u8], cursor: &mut usize, field: &str) -> Result<u32> {
    validate_min_len(data, *cursor + 4, field)?;
    let value = u32::from_be_bytes([
        data[*cursor],
        data[*cursor + 1],
        data[*cursor + 2],
        data[*cursor + 3],
    ]);
    *cursor += 4;
    Ok(value)
}

pub(super) fn read_c_string_from_slice(
    data: &[u8],
    cursor: &mut usize,
    field: &str,
) -> Result<String> {
    let (value, next) = read_c_string_at_limit(data, *cursor, data.len(), field)?;
    *cursor = next;
    Ok(value)
}

pub(super) fn skip_vlength2_offset(data: &[u8], cursor: &mut usize) -> Result<()> {
    read_vlength2_offset(data, cursor).map(|_| ())
}

pub(super) fn read_vlength2_offset(data: &[u8], cursor: &mut usize) -> Result<usize> {
    validate_min_len(data, *cursor + 1, "VLength2 offset")?;
    let first = data[*cursor];
    *cursor += 1;
    let mut offset = (first & V2_FIRST_BYTE_OFFSET_MASK) as usize;

    if first & V2_HAS_REMAINING_FLAG != 0 {
        loop {
            validate_min_len(data, *cursor + 1, "VLength2 extended offset")?;
            let byte = data[*cursor];
            *cursor += 1;
            offset = offset
                .checked_shl(7)
                .and_then(|value| value.checked_add((byte & V2_OFFSET_MASK) as usize))
                .ok_or_else(|| Error::invalid_dictionary("VLength2 offset overflow"))?;
            if byte & V2_HAS_REMAINING_FLAG == 0 {
                break;
            }
        }
    }

    Ok(offset)
}

#[cfg_attr(not(debug_assertions), inline(always))]
pub(super) fn read_vlength1_offset_at(data: &[u8], cursor: usize, size: usize) -> usize {
    match size {
        0 => 0,
        1 => byte_at_unchecked(data, cursor) as usize,
        2 => {
            ((byte_at_unchecked(data, cursor) as usize) << 8)
                | byte_at_unchecked(data, cursor + 1) as usize
        }
        3 => {
            ((byte_at_unchecked(data, cursor) as usize) << 16)
                | ((byte_at_unchecked(data, cursor + 1) as usize) << 8)
                | byte_at_unchecked(data, cursor + 2) as usize
        }
        _ => unreachable!("validated VLength1 offset size"),
    }
}

#[cfg_attr(not(debug_assertions), inline(always))]
pub(super) fn byte_at_unchecked(data: &[u8], index: usize) -> u8 {
    debug_assert!(index < data.len());
    unsafe {
        // Callers validate dictionary byte ranges before entering loaded FSA
        // traversal; debug builds still assert the local precondition.
        *data.get_unchecked(index)
    }
}
