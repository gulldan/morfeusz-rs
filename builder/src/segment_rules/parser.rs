use super::rule::SegmentRule;
use super::types::SegmentTypeLookup;
use crate::error::{validate, BuilderError, Result};

pub fn parse_segment_rule_line<T>(
    line_number: usize,
    line: &str,
    segment_types: &T,
    input_name: &str,
) -> Result<SegmentRule>
where
    T: SegmentTypeLookup,
{
    SegmentRuleParser::new(line_number, line, segment_types, input_name).parse_complete_rule()
}

struct SegmentRuleParser<'a, T> {
    line_number: usize,
    line: &'a str,
    cursor: usize,
    segment_types: &'a T,
    input_name: &'a str,
}

impl<'a, T> SegmentRuleParser<'a, T>
where
    T: SegmentTypeLookup,
{
    fn new(line_number: usize, line: &'a str, segment_types: &'a T, input_name: &'a str) -> Self {
        Self {
            line_number,
            line,
            cursor: 0,
            segment_types,
            input_name,
        }
    }

    fn parse_complete_rule(mut self) -> Result<SegmentRule> {
        let mut rule = self.parse_concat_rule(false)?;
        self.skip_whitespace();
        if self.remaining_starts_with_ignore_ascii_case("!weak") {
            self.cursor += "!weak".len();
            rule = rule.set_weak(true);
            self.skip_whitespace();
        }
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

    fn parse_concat_rule(&mut self, stop_at_paren: bool) -> Result<SegmentRule> {
        let mut children = Vec::new();
        loop {
            self.skip_whitespace();
            if self.cursor == self.line.len()
                || (stop_at_paren && self.peek_char() == Some(')'))
                || self.remaining_starts_with_ignore_ascii_case("!weak")
            {
                break;
            }
            children.push(self.parse_one_of_rule()?);
        }
        validate(
            !children.is_empty(),
            format!(
                "{}:{}: empty segmentation rule",
                self.input_name, self.line_number
            ),
        )?;
        Ok(if children.len() == 1 {
            children.remove(0)
        } else {
            SegmentRule::concat(children, self.line_number)
        })
    }

    fn parse_one_of_rule(&mut self) -> Result<SegmentRule> {
        let mut children = vec![self.parse_unary_rule()?];
        loop {
            self.skip_whitespace();
            if self.peek_char() != Some('|') {
                break;
            }
            self.cursor += 1;
            children.push(self.parse_unary_rule()?);
        }
        Ok(if children.len() == 1 {
            children.remove(0)
        } else {
            SegmentRule::or(children, self.line_number)
        })
    }

    fn parse_unary_rule(&mut self) -> Result<SegmentRule> {
        let child = self.parse_atomic_rule()?;
        self.skip_whitespace();
        match self.peek_char() {
            Some('*') => {
                self.cursor += 1;
                Ok(SegmentRule::zero_or_more(child, self.line_number))
            }
            Some('+') => {
                self.cursor += 1;
                Ok(SegmentRule::concat(
                    vec![
                        child.clone(),
                        SegmentRule::zero_or_more(child, self.line_number),
                    ],
                    self.line_number,
                ))
            }
            Some('?') => {
                self.cursor += 1;
                Ok(SegmentRule::optional(child, self.line_number))
            }
            Some('{') => self.parse_quantified_rule(child),
            _ => Ok(child),
        }
    }

    fn parse_quantified_rule(&mut self, child: SegmentRule) -> Result<SegmentRule> {
        self.cursor += 1;
        self.skip_whitespace();
        let left = self.read_usize("quantity")?;
        self.skip_whitespace();
        match self.peek_char() {
            Some('}') => {
                self.cursor += 1;
                self.create_quant_rule_exact(child, left)
            }
            Some(',') => {
                self.cursor += 1;
                self.skip_whitespace();
                if self.peek_char() == Some('}') {
                    self.cursor += 1;
                    self.create_quant_rule_open(child, left)
                } else {
                    let right = self.read_usize("right quantity")?;
                    self.skip_whitespace();
                    validate(
                        self.peek_char() == Some('}'),
                        format!(
                            "{}:{}: quantity range must end with '}}'",
                            self.input_name, self.line_number
                        ),
                    )?;
                    self.cursor += 1;
                    self.create_quant_rule_range(child, left, right)
                }
            }
            _ => Err(BuilderError::new(format!(
                "{}:{}: invalid quantity expression",
                self.input_name, self.line_number
            ))),
        }
    }

    fn parse_atomic_rule(&mut self) -> Result<SegmentRule> {
        self.skip_whitespace();
        match self.peek_char() {
            Some('(') => {
                self.cursor += 1;
                let mut rule = self.parse_concat_rule(true)?;
                validate(
                    self.peek_char() == Some(')'),
                    format!(
                        "{}:{}: parenthesized rule must end with ')'",
                        self.input_name, self.line_number
                    ),
                )?;
                self.cursor += 1;
                self.skip_whitespace();
                if self.peek_char() == Some('>') {
                    self.cursor += 1;
                    rule.make_shift_orth_rule();
                }
                Ok(rule)
            }
            Some(ch) if is_rule_tag_start(ch) => {
                let segment_type = self.read_rule_tag().expect("tag start checked");
                self.skip_whitespace();
                let shift_orth = if self.peek_char() == Some('>') {
                    self.cursor += 1;
                    true
                } else {
                    false
                };
                let segment_type_num =
                    self.segment_types
                        .segment_type_num(&segment_type)
                        .map_err(|err| {
                            BuilderError::new(format!(
                                "{}:{}: {}",
                                self.input_name, self.line_number, err
                            ))
                        })?;
                Ok(SegmentRule::tag(
                    segment_type_num,
                    shift_orth,
                    segment_type,
                    self.line_number,
                ))
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

    fn create_quant_rule_exact(&self, child: SegmentRule, quantity: usize) -> Result<SegmentRule> {
        validate(
            quantity > 0,
            format!(
                "{}:{}: {} - invalid quantity: {}",
                self.input_name, self.line_number, self.line, quantity
            ),
        )?;
        Ok(SegmentRule::concat(vec![child; quantity], self.line_number))
    }

    fn create_quant_rule_range(
        &self,
        child: SegmentRule,
        left: usize,
        right: usize,
    ) -> Result<SegmentRule> {
        validate(
            left <= right && (left, right) != (0, 0),
            format!(
                "{}:{}: {} - invalid quantities: {} {}",
                self.input_name, self.line_number, self.line, left, right
            ),
        )?;
        let mut children = Vec::new();
        if left == 0 {
            children.push(SegmentRule::optional(child.clone(), self.line_number));
            for quantity in 2..=right {
                children.push(self.create_quant_rule_exact(child.clone(), quantity)?);
            }
        } else {
            for quantity in left..=right {
                children.push(self.create_quant_rule_exact(child.clone(), quantity)?);
            }
        }
        Ok(SegmentRule::or(children, self.line_number))
    }

    fn create_quant_rule_open(&self, child: SegmentRule, quantity: usize) -> Result<SegmentRule> {
        validate(
            quantity > 0,
            format!(
                "{}:{}: {} - invalid quantity: {}",
                self.input_name, self.line_number, self.line, quantity
            ),
        )?;
        Ok(SegmentRule::concat(
            vec![
                self.create_quant_rule_exact(child.clone(), quantity)?,
                SegmentRule::zero_or_more(child, self.line_number),
            ],
            self.line_number,
        ))
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

    fn read_usize(&mut self, field: &str) -> Result<usize> {
        let start = self.cursor;
        while let Some(ch) = self.peek_char() {
            if ch.is_ascii_digit() {
                self.cursor += ch.len_utf8();
            } else {
                break;
            }
        }
        validate(
            self.cursor > start,
            format!(
                "{}:{}: missing {field} in segmentation rule",
                self.input_name, self.line_number
            ),
        )?;
        self.line[start..self.cursor]
            .parse::<usize>()
            .map_err(|err| {
                BuilderError::new(format!(
                    "{}:{}: invalid {field}: {err}",
                    self.input_name, self.line_number
                ))
            })
    }

    fn peek_char(&self) -> Option<char> {
        self.line[self.cursor..].chars().next()
    }

    fn remaining_starts_with_ignore_ascii_case(&self, needle: &str) -> bool {
        self.line[self.cursor..]
            .get(..needle.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(needle))
    }

    fn read_rule_tag(&mut self) -> Option<String> {
        let mut chars = self.line[self.cursor..].char_indices();
        let (_, first) = chars.next()?;
        if !is_rule_tag_start(first) {
            return None;
        }
        let mut end = first.len_utf8();
        for (index, ch) in chars {
            if is_rule_tag_body(ch) {
                end = index + ch.len_utf8();
            } else {
                break;
            }
        }
        let tag = self.line[self.cursor..self.cursor + end].to_owned();
        self.cursor += end;
        Some(tag)
    }
}

fn is_rule_tag_start(ch: char) -> bool {
    is_rule_tag_body(ch)
}

fn is_rule_tag_body(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}
