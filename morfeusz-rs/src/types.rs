use std::collections::BTreeSet;

use crate::IdResolver;

#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Charset {
    Utf8 = 11,
    Iso8859_2 = 12,
    Cp1250 = 13,
    Cp852 = 14,
}

pub const UTF8: Charset = Charset::Utf8;
pub const ISO8859_2: Charset = Charset::Iso8859_2;
pub const CP1250: Charset = Charset::Cp1250;
pub const CP852: Charset = Charset::Cp852;

impl Default for Charset {
    fn default() -> Self {
        Self::Utf8
    }
}

#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaseHandling {
    ConditionallyCaseSensitive = 100,
    StrictlyCaseSensitive = 101,
    IgnoreCase = 102,
}

pub const CONDITIONALLY_CASE_SENSITIVE: CaseHandling = CaseHandling::ConditionallyCaseSensitive;
pub const STRICTLY_CASE_SENSITIVE: CaseHandling = CaseHandling::StrictlyCaseSensitive;
pub const IGNORE_CASE: CaseHandling = CaseHandling::IgnoreCase;

impl Default for CaseHandling {
    fn default() -> Self {
        Self::ConditionallyCaseSensitive
    }
}

#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenNumbering {
    Separate = 201,
    Continuous = 202,
}

pub const SEPARATE_NUMBERING: TokenNumbering = TokenNumbering::Separate;
pub const CONTINUOUS_NUMBERING: TokenNumbering = TokenNumbering::Continuous;

impl Default for TokenNumbering {
    fn default() -> Self {
        Self::Separate
    }
}

#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhitespaceHandling {
    Skip = 301,
    Append = 302,
    Keep = 303,
}

pub const SKIP_WHITESPACES: WhitespaceHandling = WhitespaceHandling::Skip;
pub const APPEND_WHITESPACES: WhitespaceHandling = WhitespaceHandling::Append;
pub const KEEP_WHITESPACES: WhitespaceHandling = WhitespaceHandling::Keep;

impl Default for WhitespaceHandling {
    fn default() -> Self {
        Self::Skip
    }
}

#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MorfeuszUsage {
    AnalyseOnly = 401,
    GenerateOnly = 402,
    BothAnalyseAndGenerate = 403,
}

pub const ANALYSE_ONLY: MorfeuszUsage = MorfeuszUsage::AnalyseOnly;
pub const GENERATE_ONLY: MorfeuszUsage = MorfeuszUsage::GenerateOnly;
pub const BOTH_ANALYSE_AND_GENERATE: MorfeuszUsage = MorfeuszUsage::BothAnalyseAndGenerate;

impl Default for MorfeuszUsage {
    fn default() -> Self {
        Self::BothAnalyseAndGenerate
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MorphInterpretation {
    pub start_node: i32,
    pub end_node: i32,
    pub orth: String,
    pub lemma: String,
    pub tag_id: i32,
    pub name_id: i32,
    pub labels_id: i32,
}

impl Default for MorphInterpretation {
    fn default() -> Self {
        Self {
            start_node: 0,
            end_node: 0,
            orth: String::new(),
            lemma: String::new(),
            tag_id: 0,
            name_id: 0,
            labels_id: 0,
        }
    }
}

impl MorphInterpretation {
    pub fn create_ign(
        start_node: i32,
        end_node: i32,
        orth: impl Into<String>,
        lemma: impl Into<String>,
    ) -> Self {
        Self {
            start_node,
            end_node,
            orth: orth.into(),
            lemma: lemma.into(),
            tag_id: 0,
            name_id: 0,
            labels_id: 0,
        }
    }

    pub fn create_whitespace(start_node: i32, end_node: i32, orth: impl Into<String>) -> Self {
        let orth = orth.into();
        Self {
            start_node,
            end_node,
            lemma: orth.clone(),
            orth,
            tag_id: 1,
            name_id: 0,
            labels_id: 0,
        }
    }

    pub fn is_ign(&self) -> bool {
        self.tag_id == 0
    }

    pub fn is_whitespace(&self) -> bool {
        self.tag_id == 1
    }

    pub fn tag<'a>(&self, resolver: &'a IdResolver) -> Option<&'a str> {
        resolver.tag(self.tag_id)
    }

    pub fn name<'a>(&self, resolver: &'a IdResolver) -> Option<&'a str> {
        resolver.name(self.name_id)
    }

    pub fn labels_as_string<'a>(&self, resolver: &'a IdResolver) -> Option<&'a str> {
        resolver.labels_as_string(self.labels_id)
    }

    pub fn labels<'a>(&self, resolver: &'a IdResolver) -> Option<&'a BTreeSet<String>> {
        resolver.labels(self.labels_id)
    }
}
