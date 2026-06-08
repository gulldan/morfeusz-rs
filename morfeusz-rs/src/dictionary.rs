use std::collections::HashMap;

use crate::{IdResolver, Lexicon, MorphInterpretation, Result, SegmentationPreset};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DictionaryEntry {
    pub orth: String,
    pub lemma: String,
    pub tag_id: i32,
    pub name_id: i32,
    pub labels_id: i32,
}

#[derive(Debug, Clone)]
pub struct Dictionary {
    id: String,
    copyright: String,
    resolver: IdResolver,
    entries: Vec<DictionaryEntry>,
    by_orth: HashMap<String, Vec<DictionaryEntry>>,
    by_generator_key: HashMap<String, Vec<DictionaryEntry>>,
}

impl Default for Dictionary {
    fn default() -> Self {
        Self::empty()
    }
}

impl Dictionary {
    pub fn empty() -> Self {
        Self {
            id: String::new(),
            copyright: String::new(),
            resolver: IdResolver::default(),
            entries: Vec::new(),
            by_orth: HashMap::new(),
            by_generator_key: HashMap::new(),
        }
    }

    pub fn new(resolver: IdResolver) -> Self {
        Self {
            resolver,
            ..Self::empty()
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn copyright(&self) -> &str {
        &self.copyright
    }

    pub(crate) fn set_metadata(&mut self, id: impl Into<String>, copyright: impl Into<String>) {
        self.id = id.into();
        self.copyright = copyright.into();
    }

    pub fn resolver(&self) -> &IdResolver {
        &self.resolver
    }

    pub fn lookup(&self, orth: &str) -> Option<&[DictionaryEntry]> {
        self.by_orth.get(orth).map(Vec::as_slice)
    }

    pub fn generate(&self, lemma: &str) -> Option<&[DictionaryEntry]> {
        self.by_generator_key.get(lemma).map(Vec::as_slice)
    }

    pub(crate) fn entries(&self) -> impl Iterator<Item = &DictionaryEntry> {
        self.entries.iter()
    }

    pub(crate) fn insert_lookup_alias_front(
        &mut self,
        orth: impl Into<String>,
        entry: DictionaryEntry,
    ) {
        self.by_orth
            .entry(orth.into())
            .or_default()
            .insert(0, entry);
    }

    pub(crate) fn resolver_mut(&mut self) -> &mut IdResolver {
        &mut self.resolver
    }

    pub(crate) fn insert(&mut self, entry: DictionaryEntry) {
        let lemma_base = lemma_base(&entry.lemma);
        self.by_generator_key
            .entry(lemma_base.to_owned())
            .or_default()
            .push(entry.clone());
        if entry.lemma != lemma_base {
            self.by_generator_key
                .entry(entry.lemma.clone())
                .or_default()
                .push(entry.clone());
        }
        self.by_orth
            .entry(entry.orth.clone())
            .or_default()
            .push(entry.clone());
        self.entries.push(entry);
    }
}

fn lemma_base(lemma: &str) -> &str {
    lemma.split_once(':').map(|(base, _)| base).unwrap_or(lemma)
}

impl Lexicon for Dictionary {
    fn id(&self) -> &str {
        self.id()
    }

    fn copyright(&self) -> &str {
        self.copyright()
    }

    fn resolver(&self) -> &IdResolver {
        self.resolver()
    }

    fn lookup(&self, orth: &str) -> Option<&[DictionaryEntry]> {
        self.lookup(orth)
    }

    fn synthesize(&self, lemma: &str) -> Option<&[DictionaryEntry]> {
        self.generate(lemma)
    }

    fn synthesize_interpretations(
        &self,
        lemma: &str,
        _segmentation: &SegmentationPreset,
    ) -> Result<Vec<MorphInterpretation>> {
        if let Some(entries) = self.synthesize(lemma) {
            return Ok(entries
                .iter()
                .map(|entry| entry_to_interpretation(entry, 0, 0, entry.orth.clone()))
                .collect());
        }
        // Synthetic-fixture fallback: the small TSV dictionaries list only the
        // single digits 0-9, but the real segmentation rules collapse a run of
        // `dig` segments into a single number. Reproduce that here so the
        // builder-free TSV path matches the binary FSA path for digit strings.
        // This stays confined to the TSV lexicon; the engine never special-cases
        // digits.
        if let Some(interp) = tsv_digit_interpretation(&self.resolver, lemma, 0, 0, lemma) {
            return Ok(vec![interp]);
        }
        Ok(Vec::new())
    }
}

/// Builds a `dig` interpretation for an all-ASCII-digit token, if the tagset
/// defines the `dig` tag. Used only by the synthetic TSV lexicon path.
pub(crate) fn tsv_digit_interpretation(
    resolver: &IdResolver,
    text: &str,
    start_node: i32,
    end_node: i32,
    orth: &str,
) -> Option<MorphInterpretation> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let tag_id = resolver.tag_id("dig").ok()?;
    Some(MorphInterpretation {
        start_node,
        end_node,
        orth: orth.to_owned(),
        lemma: text.to_owned(),
        tag_id,
        name_id: 0,
        labels_id: 0,
    })
}

pub(crate) fn entry_to_interpretation(
    entry: &DictionaryEntry,
    start_node: i32,
    end_node: i32,
    orth: String,
) -> MorphInterpretation {
    MorphInterpretation {
        start_node,
        end_node,
        orth,
        lemma: entry.lemma.clone(),
        tag_id: entry.tag_id,
        name_id: entry.name_id,
        labels_id: entry.labels_id,
    }
}
