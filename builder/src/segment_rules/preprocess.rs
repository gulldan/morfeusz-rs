use std::collections::{BTreeMap, BTreeSet};

use crate::error::{validate, BuilderError, Result};

pub fn preprocess_segment_rules<I, L, D>(
    input_lines: I,
    active_definitions: D,
    input_name: &str,
) -> Result<Vec<(usize, String)>>
where
    I: IntoIterator<Item = (usize, L)>,
    L: AsRef<str>,
    D: IntoIterator,
    D::Item: AsRef<str>,
{
    let mut defines = BTreeMap::new();
    let active_definitions: BTreeSet<String> = active_definitions
        .into_iter()
        .map(|definition| definition.as_ref().to_owned())
        .collect();
    let mut ifdefs_stack: Vec<(String, bool)> = Vec::new();
    let mut output = Vec::new();

    for (line_number, raw_line) in input_lines {
        let line = raw_line.as_ref();
        if line.starts_with("#define") {
            let parsed = parse_segment_rule_define(line, line_number, input_name)?;
            match parsed {
                SegmentRuleDefine::WithoutArg { name, value } => {
                    defines.insert(name.clone(), SegmentRuleDefineValue::WithoutArg(value));
                }
                SegmentRuleDefine::WithArg { name, arg, value } => {
                    defines.insert(name, SegmentRuleDefineValue::WithArg { arg, value });
                }
            }
        } else if line.starts_with("#ifdef") {
            let name = parse_segment_rule_ifdef(line, line_number, input_name)?;
            ifdefs_stack.push((name, true));
        } else if line.starts_with("#else") {
            let Some((name, is_active)) = ifdefs_stack.last_mut() else {
                return Err(BuilderError::new(format!(
                    "{input_name}:{line_number}: #else without #ifdef"
                )));
            };
            validate(
                *is_active,
                format!("{input_name}:{line_number}: repeated #else for #ifdef {name}"),
            )?;
            *is_active = false;
        } else if line.starts_with("#endif") {
            if ifdefs_stack.pop().is_none() {
                return Err(BuilderError::new(format!(
                    "{input_name}:{line_number}: #endif without #ifdef"
                )));
            }
        } else if line.starts_with('#') {
            output.push((line_number, line.to_owned()));
        } else if segment_rule_ifdefs_active(&ifdefs_stack, &active_definitions) {
            output.push((
                line_number,
                process_segment_rule_line(line_number, line, &defines, input_name)?,
            ));
        }
    }

    validate(
        ifdefs_stack.is_empty(),
        format!("{input_name}: unterminated #ifdef in segmentation rules"),
    )?;
    Ok(output)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SegmentRuleDefine {
    WithoutArg {
        name: String,
        value: String,
    },
    WithArg {
        name: String,
        arg: String,
        value: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SegmentRuleDefineValue {
    WithoutArg(String),
    WithArg { arg: String, value: String },
}

fn parse_segment_rule_define(
    line: &str,
    line_number: usize,
    input_name: &str,
) -> Result<SegmentRuleDefine> {
    let rest = line
        .strip_prefix("#define")
        .ok_or_else(|| BuilderError::new(format!("{input_name}:{line_number}: invalid #define")))?;
    let rest = rest.trim_start();
    let (name, mut cursor) = read_segment_rule_identifier(rest).ok_or_else(|| {
        BuilderError::new(format!(
            "{input_name}:{line_number}: #define must be followed by identifier"
        ))
    })?;

    if rest[cursor..].starts_with('(') {
        cursor += 1;
        let (arg, arg_len) = read_segment_rule_identifier(&rest[cursor..]).ok_or_else(|| {
            BuilderError::new(format!(
                "{input_name}:{line_number}: #define argument must be an identifier"
            ))
        })?;
        cursor += arg_len;
        let after_arg = rest[cursor..].trim_start();
        validate(
            after_arg.starts_with(')'),
            format!("{input_name}:{line_number}: #define argument list must end with ')'"),
        )?;
        let close_offset = rest.len() - after_arg.len();
        let value = rest[close_offset + 1..].to_owned();
        Ok(SegmentRuleDefine::WithArg { name, arg, value })
    } else {
        Ok(SegmentRuleDefine::WithoutArg {
            name,
            value: rest[cursor..].trim_start().to_owned(),
        })
    }
}

fn parse_segment_rule_ifdef(line: &str, line_number: usize, input_name: &str) -> Result<String> {
    let rest = line
        .strip_prefix("#ifdef")
        .ok_or_else(|| BuilderError::new(format!("{input_name}:{line_number}: invalid #ifdef")))?;
    let rest = rest.trim();
    validate(
        is_segment_rule_identifier(rest),
        format!("{input_name}:{line_number}: #ifdef must be followed by one identifier"),
    )?;
    Ok(rest.to_owned())
}

fn segment_rule_ifdefs_active(
    ifdefs_stack: &[(String, bool)],
    active_definitions: &BTreeSet<String>,
) -> bool {
    ifdefs_stack.iter().all(|(name, is_active)| {
        (active_definitions.contains(name) && *is_active)
            || (!active_definitions.contains(name) && !*is_active)
    })
}

fn process_segment_rule_line(
    line_number: usize,
    line: &str,
    defines: &BTreeMap<String, SegmentRuleDefineValue>,
    input_name: &str,
) -> Result<String> {
    if line.trim().is_empty() {
        return Ok(line.to_owned());
    }

    let mut current = line.to_owned();
    for _ in 0..128 {
        let processed = SegmentRuleLineProcessor::new(&current, line_number, defines, input_name)
            .parse_complete_rule()?;
        if processed.trim() == current.trim() {
            return Ok(current);
        }
        current = processed;
    }
    Err(BuilderError::new(format!(
        "{input_name}:{line_number}: recursive segmentation-rule define expansion did not stabilize"
    )))
}

struct SegmentRuleLineProcessor<'a> {
    line: &'a str,
    cursor: usize,
    line_number: usize,
    defines: &'a BTreeMap<String, SegmentRuleDefineValue>,
    input_name: &'a str,
}

impl<'a> SegmentRuleLineProcessor<'a> {
    fn new(
        line: &'a str,
        line_number: usize,
        defines: &'a BTreeMap<String, SegmentRuleDefineValue>,
        input_name: &'a str,
    ) -> Self {
        Self {
            line,
            cursor: 0,
            line_number,
            defines,
            input_name,
        }
    }

    fn parse_complete_rule(mut self) -> Result<String> {
        let rule = self.parse_rule(false)?;
        self.skip_whitespace();
        validate(
            self.cursor == self.line.len(),
            format!(
                "{}:{}: unexpected token in segmentation rule near {:?}",
                self.input_name,
                self.line_number,
                &self.line[self.cursor..]
            ),
        )?;
        Ok(rule)
    }

    fn parse_rule(&mut self, stop_at_paren: bool) -> Result<String> {
        let mut tokens = Vec::new();
        loop {
            self.skip_whitespace();
            if self.cursor == self.line.len() || (stop_at_paren && self.peek_char() == Some(')')) {
                break;
            }
            tokens.push(self.parse_token()?);
        }
        Ok(tokens.join(" "))
    }

    fn parse_token(&mut self) -> Result<String> {
        if self.line[self.cursor..]
            .to_ascii_lowercase()
            .starts_with("!weak")
        {
            self.cursor += "!weak".len();
            return Ok("!weak".to_owned());
        }

        match self.peek_char() {
            Some('(') => {
                self.cursor += 1;
                let inner = self.parse_rule(true)?;
                validate(
                    self.peek_char() == Some(')'),
                    format!(
                        "{}:{}: unterminated parenthesized segmentation rule",
                        self.input_name, self.line_number
                    ),
                )?;
                self.cursor += 1;
                Ok(format!("( {inner} )"))
            }
            Some(ch) if is_segment_rule_operator(ch) => Ok(self.read_operator_word()),
            Some(ch) if is_segment_rule_identifier_start(ch) => {
                let name = self.read_identifier().expect("identifier start checked");
                self.skip_whitespace();
                if self.peek_char() == Some('(') {
                    self.cursor += 1;
                    let substitute_value = self.parse_rule(true)?;
                    validate(
                        self.peek_char() == Some(')'),
                        format!(
                            "{}:{}: unterminated define invocation",
                            self.input_name, self.line_number
                        ),
                    )?;
                    self.cursor += 1;
                    Ok(self.substitute_arg_define(&name, &substitute_value))
                } else {
                    Ok(self.substitute_non_arg_define(&name))
                }
            }
            Some(ch) => Err(BuilderError::new(format!(
                "{}:{}: unexpected character {:?} in segmentation rule",
                self.input_name, self.line_number, ch
            ))),
            None => Err(BuilderError::new(format!(
                "{}:{}: unexpected end of segmentation rule",
                self.input_name, self.line_number
            ))),
        }
    }

    fn substitute_arg_define(&self, name: &str, substitute_value: &str) -> String {
        match self.defines.get(name) {
            Some(SegmentRuleDefineValue::WithArg { arg, value }) => {
                replace_ascii_word(value, arg, substitute_value)
            }
            Some(SegmentRuleDefineValue::WithoutArg(value)) => {
                format!("{value} ( {substitute_value} )")
            }
            None => format!("{name} ( {substitute_value} )"),
        }
    }

    fn substitute_non_arg_define(&self, name: &str) -> String {
        match self.defines.get(name) {
            Some(SegmentRuleDefineValue::WithoutArg(value)) => value.clone(),
            _ => name.to_owned(),
        }
    }

    fn skip_whitespace(&mut self) {
        while let Some(ch) = self.peek_char() {
            if ch.is_whitespace() {
                self.cursor += ch.len_utf8();
            } else {
                break;
            }
        }
    }

    fn peek_char(&self) -> Option<char> {
        self.line[self.cursor..].chars().next()
    }

    fn read_identifier(&mut self) -> Option<String> {
        let (identifier, len) = read_segment_rule_identifier(&self.line[self.cursor..])?;
        self.cursor += len;
        Some(identifier)
    }

    fn read_operator_word(&mut self) -> String {
        let start = self.cursor;
        while let Some(ch) = self.peek_char() {
            if is_segment_rule_operator(ch) {
                self.cursor += ch.len_utf8();
            } else {
                break;
            }
        }
        self.line[start..self.cursor].to_owned()
    }
}

fn read_segment_rule_identifier(input: &str) -> Option<(String, usize)> {
    let mut chars = input.char_indices();
    let (_, first) = chars.next()?;
    if !is_segment_rule_identifier_start(first) {
        return None;
    }
    let mut end = first.len_utf8();
    for (index, ch) in chars {
        if is_segment_rule_identifier_body(ch) {
            end = index + ch.len_utf8();
        } else {
            break;
        }
    }
    Some((input[..end].to_owned(), end))
}

fn is_segment_rule_identifier(input: &str) -> bool {
    read_segment_rule_identifier(input)
        .map(|(_identifier, len)| len == input.len())
        .unwrap_or(false)
}

fn is_segment_rule_identifier_start(ch: char) -> bool {
    ch.is_ascii_alphabetic()
}

fn is_segment_rule_identifier_body(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '>' | '*' | '+' | '{' | '}' | ',')
}

fn is_segment_rule_operator(ch: char) -> bool {
    matches!(ch, '*' | '|' | '+' | '?' | '>')
}

pub(crate) fn replace_ascii_word(input: &str, word: &str, replacement: &str) -> String {
    if word.is_empty() {
        return input.to_owned();
    }
    let mut out = String::with_capacity(input.len());
    let mut cursor = 0usize;
    while let Some(relative) = input[cursor..].find(word) {
        let start = cursor + relative;
        let end = start + word.len();
        if is_ascii_word_start_boundary(input, start) && is_ascii_word_end_boundary(input, end) {
            out.push_str(&input[cursor..start]);
            out.push_str(replacement);
            cursor = end;
        } else {
            out.push_str(&input[cursor..end]);
            cursor = end;
        }
    }
    out.push_str(&input[cursor..]);
    out
}

fn is_ascii_word_start_boundary(input: &str, index: usize) -> bool {
    if index == 0 {
        true
    } else {
        !is_ascii_regex_word_byte(input.as_bytes()[index - 1])
    }
}

fn is_ascii_word_end_boundary(input: &str, index: usize) -> bool {
    if index == input.len() {
        true
    } else {
        !is_ascii_regex_word_byte(input.as_bytes()[index])
    }
}

fn is_ascii_regex_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

pub(super) fn is_ascii_word(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}
