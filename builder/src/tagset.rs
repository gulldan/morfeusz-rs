use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::error::{validate, BuilderError, Result};

pub trait TagsetLookup {
    fn tag_num(&self, tag: &str) -> Result<usize>;
}

pub trait TagsetRulesLookup: TagsetLookup {
    fn all_tags(&self) -> &[String];
}

impl TagsetLookup for BTreeMap<String, usize> {
    fn tag_num(&self, tag: &str) -> Result<usize> {
        self.get(tag)
            .copied()
            .ok_or_else(|| BuilderError::new(format!("unknown tag: {tag}")))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tagset {
    pub tagset_id: Option<String>,
    pub tag_to_num: BTreeMap<String, usize>,
    num_to_tag: BTreeMap<usize, String>,
    tags_in_order: Vec<String>,
}

impl Tagset {
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self> {
        let contents = fs::read_to_string(path.as_ref()).map_err(|err| {
            BuilderError::new(format!(
                "failed to read tagset file {}: {err}",
                path.as_ref().display()
            ))
        })?;
        Self::from_str(path.as_ref().display().to_string(), &contents)
    }

    pub fn from_str(input_name: impl AsRef<str>, input: &str) -> Result<Self> {
        let mut tagset_id = None;
        let mut tag_to_num = BTreeMap::new();
        let mut tags_in_order = Vec::new();
        let mut inside_tags = false;

        for (line_index, raw_line) in python_file_lines(input).enumerate() {
            let line_number = line_index + 1;
            if line_number == 1 {
                let Some(id) = parse_tagset_id_line(raw_line) else {
                    return Err(BuilderError::new(
                        "missing TAGSET-ID in first line of tagset file",
                    ));
                };
                tagset_id = Some(id);
            } else if raw_line == "[TAGS]" {
                inside_tags = true;
            } else if !raw_line.is_empty() && !raw_line.starts_with('#') {
                validate(
                    inside_tags,
                    format!(
                        "\"{}\" - text outside [TAGS] section in tagset file line {line_number}",
                        raw_line
                    ),
                )?;
                let fields: Vec<&str> = raw_line.split('\t').collect();
                validate(
                    fields.len() == 2,
                    format!("\"{}\" - invalid line {line_number}", raw_line),
                )?;
                let tag_num = fields[0].parse::<usize>().map_err(|err| {
                    BuilderError::new(format!(
                        "{}:{} - invalid tag id \"{}\": {err}",
                        input_name.as_ref(),
                        line_number,
                        fields[0]
                    ))
                })?;
                let tag = fields[1];

                validate(
                    !tag_to_num.contains_key(tag),
                    format!("duplicate tag: \"{tag}\""),
                )?;
                validate(
                    !tag_to_num.values().any(|existing| *existing == tag_num),
                    format!(
                        "line {line_number}: tagId {tag_num} assigned for tag \"{tag}\" already appeared somewhere else."
                    ),
                )?;

                tag_to_num.insert(tag.to_owned(), tag_num);
                tags_in_order.push(tag.to_owned());
            }
        }

        let num_to_tag = tag_to_num
            .iter()
            .map(|(tag, tag_num)| (*tag_num, tag.clone()))
            .collect();

        Ok(Self {
            tagset_id,
            tag_to_num,
            num_to_tag,
            tags_in_order,
        })
    }

    pub fn all_tags(&self) -> &[String] {
        &self.tags_in_order
    }

    pub fn tag_num_for_tag(&self, tag: &str) -> Result<usize> {
        self.tag_to_num
            .get(tag)
            .copied()
            .ok_or_else(|| BuilderError::new(format!("invalid tag: \"{tag}\"")))
    }

    pub fn tag_for_tag_num(&self, tag_num: usize) -> Result<&str> {
        self.num_to_tag
            .get(&tag_num)
            .map(String::as_str)
            .ok_or_else(|| BuilderError::new(format!("invalid tag id: {tag_num}")))
    }
}

impl TagsetLookup for Tagset {
    fn tag_num(&self, tag: &str) -> Result<usize> {
        self.tag_num_for_tag(tag)
    }
}

impl TagsetRulesLookup for Tagset {
    fn all_tags(&self) -> &[String] {
        self.all_tags()
    }
}

fn parse_tagset_id_line(line: &str) -> Option<String> {
    let rest = line.strip_prefix("#!TAGSET-ID")?;
    if !rest.chars().next().is_some_and(char::is_whitespace) {
        return None;
    }
    Some(rest.trim_start_matches(char::is_whitespace).to_owned())
}

fn python_file_lines(input: &str) -> impl Iterator<Item = &str> {
    input
        .split_inclusive('\n')
        .map(|line| line.trim_matches(['\n', '\r']))
}
