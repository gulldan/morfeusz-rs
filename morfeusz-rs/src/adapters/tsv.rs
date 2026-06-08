use std::fs;
use std::path::Path;

use std::collections::{BTreeSet, HashMap, HashSet};

use crate::{Dictionary, DictionaryEntry, Error, IdResolver, Result};

pub struct TsvLexiconLoader;

impl TsvLexiconLoader {
    pub fn from_paths(
        dictionary_path: impl AsRef<Path>,
        tagset_path: Option<impl AsRef<Path>>,
    ) -> Result<Dictionary> {
        let resolver = match tagset_path {
            Some(path) => Self::tagset_from_path(path)?,
            None => IdResolver::default(),
        };
        let content = fs::read_to_string(dictionary_path)?;
        Self::from_str(&content, resolver)
    }

    pub fn from_paths_with_segmentation(
        dictionary_path: impl AsRef<Path>,
        tagset_path: impl AsRef<Path>,
        segmentation_path: impl AsRef<Path>,
    ) -> Result<Dictionary> {
        let resolver = Self::tagset_from_path(tagset_path)?;
        let content = fs::read_to_string(dictionary_path)?;
        let segmentation = fs::read_to_string(segmentation_path)?;
        let hints = SegmentationHints::parse(&segmentation);
        let dictionary = Self::from_str(&content, resolver)?;
        Ok(apply_segmentation_hints(dictionary, &hints))
    }

    pub fn from_str(content: &str, resolver: IdResolver) -> Result<Dictionary> {
        let mut dictionary = Dictionary::new(resolver);
        let mut dict_id = String::new();
        let mut copyright = String::new();
        let mut in_copyright = false;

        for (line_no, raw_line) in content.lines().enumerate() {
            let line = raw_line.trim_end_matches('\r');
            if in_copyright {
                if line.trim() == "#</COPYRIGHT>" {
                    in_copyright = false;
                } else {
                    copyright.push_str(line);
                    copyright.push('\n');
                }
                continue;
            }
            if let Some(id) = line.strip_prefix("#!DICT-ID") {
                dict_id = id.trim().to_owned();
                continue;
            }
            if line.trim() == "#<COPYRIGHT>" {
                in_copyright = true;
                continue;
            }
            if line.trim().is_empty() || line.trim_start().starts_with('#') {
                continue;
            }
            let fields = line.split('\t').collect::<Vec<_>>();
            if fields.len() < 3 {
                return Err(Error::invalid_dictionary(format!(
                    "dictionary line {} has fewer than 3 tab-separated fields",
                    line_no + 1
                )));
            }

            let tag_id = match dictionary.resolver().tag_id(fields[2]) {
                Ok(id) => id,
                Err(_) => dictionary.resolver_mut().get_or_insert_tag(fields[2]),
            };
            let name_id = dictionary
                .resolver_mut()
                .get_or_insert_name(fields.get(3).copied().unwrap_or("_"));
            let labels_id = dictionary
                .resolver_mut()
                .get_or_insert_labels(fields.get(4).copied().unwrap_or("_"));

            dictionary.insert(DictionaryEntry {
                orth: fields[0].to_owned(),
                lemma: fields[1].to_owned(),
                tag_id,
                name_id,
                labels_id,
            });
        }

        dictionary.set_metadata(dict_id, copyright);
        Ok(dictionary)
    }

    pub fn tagset_from_path(path: impl AsRef<Path>) -> Result<IdResolver> {
        let content = fs::read_to_string(path)?;
        Self::tagset_from_str(&content)
    }

    pub fn tagset_from_str(content: &str) -> Result<IdResolver> {
        let mut resolver = IdResolver::default();
        let mut in_tags = false;

        for raw_line in content.lines() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with('#') && !line.starts_with("#!TAGSET-ID") {
                continue;
            }
            if let Some(tagset_id) = line.strip_prefix("#!TAGSET-ID") {
                resolver.set_tagset_id(tagset_id.trim());
                continue;
            }
            if line.starts_with('[') && line.ends_with(']') {
                in_tags = line == "[TAGS]";
                continue;
            }
            if !in_tags {
                continue;
            }

            let mut fields = line.split_whitespace();
            let id = fields
                .next()
                .ok_or_else(|| Error::invalid_dictionary("missing tag id"))?
                .parse::<i32>()
                .map_err(|_| {
                    Error::invalid_dictionary(format!("invalid tag id in line: {line}"))
                })?;
            let tag = fields
                .next()
                .ok_or_else(|| Error::invalid_dictionary(format!("missing tag in line: {line}")))?;
            resolver.set_tag(id, tag);
        }

        Ok(resolver)
    }
}

#[derive(Debug, Default)]
struct SegmentationHints {
    standalone_labels: BTreeSet<String>,
    prefix_labels: BTreeSet<String>,
    shift_adja_adj: bool,
    prefs_subst: bool,
    prefs_dywiz_subst: bool,
    nie_dywiz_subst: bool,
    subst_dywiz_subst: bool,
}

impl SegmentationHints {
    fn parse(content: &str) -> Self {
        let mut hints = Self::default();
        let mut in_combinations = false;

        for raw_line in content.lines() {
            let line = raw_line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            if line.contains("adja>") && line.contains("adj") {
                hints.shift_adja_adj = true;
            }
            if line.starts_with('[') && line.ends_with(']') {
                in_combinations = line == "[combinations]";
                continue;
            }
            if !in_combinations || line.starts_with("#define") || line.contains('(') {
                continue;
            }

            let parts = line
                .split('>')
                .map(str::trim)
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>();
            match parts.as_slice() {
                ["prefs", "subst"] => {
                    hints.prefs_subst = true;
                }
                ["prefs", "dywiz", "subst"] => {
                    hints.prefs_dywiz_subst = true;
                }
                ["nie", "dywiz", "subst"] => {
                    hints.nie_dywiz_subst = true;
                }
                ["subst", "dywiz", "subst"] => {
                    hints.subst_dywiz_subst = true;
                }
                [standalone] if !is_atomic_segment_type(standalone) => {
                    hints.standalone_labels.insert((*standalone).to_owned());
                }
                [prefix, ..] if !is_atomic_segment_type(prefix) => {
                    hints.prefix_labels.insert((*prefix).to_owned());
                }
                _ => {}
            }
        }

        hints
    }

    fn has_rules(&self) -> bool {
        !self.prefix_labels.is_empty()
            || self.shift_adja_adj
            || self.prefs_subst
            || self.prefs_dywiz_subst
            || self.nie_dywiz_subst
            || self.subst_dywiz_subst
    }
}

fn is_atomic_segment_type(segment_type: &str) -> bool {
    matches!(segment_type, "prefs" | "nie" | "dywiz" | "subst")
}

fn apply_segmentation_hints(dictionary: Dictionary, hints: &SegmentationHints) -> Dictionary {
    if !hints.has_rules() {
        return dictionary;
    }

    let resolver = dictionary.resolver().clone();
    let entries = dictionary.entries().cloned().collect::<Vec<_>>();
    let mut transformed = Dictionary::new(resolver.clone());
    let prefix_entries = entries
        .iter()
        .filter(|entry| {
            resolver
                .tag(entry.tag_id)
                .map(|tag| tag == "prefs")
                .unwrap_or(false)
                && hints.prefix_labels.contains(&entry.orth)
        })
        .cloned()
        .collect::<Vec<_>>();

    for entry in entries
        .iter()
        .filter(|entry| should_keep_standalone(entry, &resolver, hints))
    {
        let labels = ordered_standalone_labels(entry, &resolver, hints);
        let entry = with_labels(entry, &mut transformed, &labels);
        transformed.insert(entry);
    }

    for prefix in &prefix_entries {
        for base in entries
            .iter()
            .filter(|entry| should_combine_with_prefix(entry, prefix, &resolver))
        {
            let labels = ordered_prefixed_labels(base, prefix, &resolver, hints);
            let labels_id = transformed
                .resolver_mut()
                .get_or_insert_labels_in_order(&labels);
            transformed.insert(DictionaryEntry {
                orth: format!("{}{}", prefix.orth, base.orth),
                lemma: format!("{}{}", prefix.orth, base.lemma),
                tag_id: base.tag_id,
                name_id: base.name_id,
                labels_id,
            });
        }
    }

    if hints.shift_adja_adj {
        insert_adja_adj_shift_entries(&entries, &resolver, &mut transformed);
    }

    insert_case_and_compound_entries(&entries, &resolver, hints, &mut transformed);

    transformed
}

fn insert_case_and_compound_entries(
    entries: &[DictionaryEntry],
    resolver: &IdResolver,
    hints: &SegmentationHints,
    dictionary: &mut Dictionary,
) {
    insert_case_lookup_aliases(entries, resolver, dictionary);

    let mut inserted = HashSet::new();
    if hints.prefs_subst {
        insert_prefixed_subst_entries(entries, resolver, dictionary, &mut inserted, "prefs", "");
    }
    if hints.prefs_dywiz_subst {
        insert_prefixed_subst_entries(entries, resolver, dictionary, &mut inserted, "prefs", "-");
    }
    if hints.nie_dywiz_subst {
        insert_prefixed_subst_entries(entries, resolver, dictionary, &mut inserted, "nie", "-");
    }
    if hints.subst_dywiz_subst {
        insert_subst_dywiz_subst_entries(entries, resolver, dictionary, &mut inserted);
    }
}

fn insert_case_lookup_aliases(
    entries: &[DictionaryEntry],
    resolver: &IdResolver,
    dictionary: &mut Dictionary,
) {
    let candidates = entries
        .iter()
        .filter(|entry| is_case_alias_candidate(entry, resolver))
        .collect::<Vec<_>>();
    let mut by_orth: HashMap<String, Vec<&DictionaryEntry>> = HashMap::new();
    for entry in &candidates {
        by_orth.entry(entry.orth.clone()).or_default().push(*entry);
    }

    let mut handled = HashSet::new();
    for entry in candidates {
        let lower = lowercase(&entry.orth);
        if lower == entry.orth || !handled.insert(entry.orth.clone()) {
            continue;
        }

        if let Some(lower_entries) = by_orth.get(&lower) {
            for lower_entry in lower_entries.iter().rev() {
                dictionary.insert_lookup_alias_front(entry.orth.clone(), (*lower_entry).clone());
            }
        } else if let Some(mixed_entries) = by_orth.get(&entry.orth) {
            for mixed_entry in mixed_entries.iter().rev() {
                dictionary.insert_lookup_alias_front(lower.clone(), (*mixed_entry).clone());
            }
        }
    }
}

fn insert_prefixed_subst_entries(
    entries: &[DictionaryEntry],
    resolver: &IdResolver,
    dictionary: &mut Dictionary,
    inserted: &mut HashSet<EntryKey>,
    prefix_tag: &str,
    separator: &str,
) {
    let case_index = CaseIndex::new(entries, resolver);
    let prefixes = entries
        .iter()
        .filter(|entry| resolver.tag(entry.tag_id) == Some(prefix_tag))
        .collect::<Vec<_>>();
    let bases = entries
        .iter()
        .filter(|entry| is_subst_entry(entry, resolver))
        .collect::<Vec<_>>();

    for prefix in prefixes {
        for prefix_variant in prefix_variants(prefix, prefix_tag) {
            for base in &bases {
                for base_variant in case_index.variants_for_entry(base) {
                    let entry = DictionaryEntry {
                        orth: format!("{}{}{}", prefix_variant.orth, separator, base_variant.orth),
                        lemma: format!(
                            "{}{}{}",
                            prefix_variant.lemma, separator, base_variant.lemma
                        ),
                        tag_id: base_variant.entry.tag_id,
                        name_id: base_variant.entry.name_id,
                        labels_id: base_variant.entry.labels_id,
                    };
                    insert_unique(dictionary, inserted, entry);
                }
            }
        }
    }
}

fn insert_subst_dywiz_subst_entries(
    entries: &[DictionaryEntry],
    resolver: &IdResolver,
    dictionary: &mut Dictionary,
    inserted: &mut HashSet<EntryKey>,
) {
    let case_index = CaseIndex::new(entries, resolver);
    let subst_entries = entries
        .iter()
        .filter(|entry| is_subst_entry(entry, resolver))
        .collect::<Vec<_>>();

    for left in &subst_entries {
        for left_variant in case_index.variants_for_entry(left) {
            for right in &subst_entries {
                for right_variant in case_index.variants_for_entry(right) {
                    let entry = DictionaryEntry {
                        orth: format!("{}-{}", left_variant.orth, right_variant.orth),
                        lemma: format!(
                            "{}-{}",
                            lemma_base(&left_variant.lemma),
                            right_variant.lemma
                        ),
                        tag_id: right_variant.entry.tag_id,
                        name_id: right_variant.entry.name_id,
                        labels_id: right_variant.entry.labels_id,
                    };
                    insert_unique(dictionary, inserted, entry);
                }
            }
        }
    }
}

type EntryKey = (String, String, i32, i32, i32);

fn insert_unique(
    dictionary: &mut Dictionary,
    inserted: &mut HashSet<EntryKey>,
    entry: DictionaryEntry,
) {
    let key = (
        entry.orth.clone(),
        entry.lemma.clone(),
        entry.tag_id,
        entry.name_id,
        entry.labels_id,
    );
    if inserted.insert(key) {
        dictionary.insert(entry);
    }
}

#[derive(Debug, Clone)]
struct PieceVariant {
    orth: String,
    lemma: String,
}

#[derive(Debug, Clone)]
struct EntryVariant {
    orth: String,
    lemma: String,
    entry: DictionaryEntry,
}

struct CaseIndex<'a> {
    by_orth: HashMap<String, Vec<&'a DictionaryEntry>>,
    mixed_orths_by_lower: HashMap<String, Vec<String>>,
}

impl<'a> CaseIndex<'a> {
    fn new(entries: &'a [DictionaryEntry], resolver: &IdResolver) -> Self {
        let mut by_orth: HashMap<String, Vec<&DictionaryEntry>> = HashMap::new();
        let mut mixed_orths_by_lower: HashMap<String, Vec<String>> = HashMap::new();

        for entry in entries
            .iter()
            .filter(|entry| is_case_alias_candidate(entry, resolver))
        {
            by_orth.entry(entry.orth.clone()).or_default().push(entry);
            let lower = lowercase(&entry.orth);
            if lower != entry.orth {
                let mixed_orths = mixed_orths_by_lower.entry(lower).or_default();
                if !mixed_orths.contains(&entry.orth) {
                    mixed_orths.push(entry.orth.clone());
                }
            }
        }

        Self {
            by_orth,
            mixed_orths_by_lower,
        }
    }

    fn variants_for_entry(&self, entry: &DictionaryEntry) -> Vec<EntryVariant> {
        let mut variants = vec![EntryVariant {
            orth: entry.orth.clone(),
            lemma: entry.lemma.clone(),
            entry: entry.clone(),
        }];
        let lower = lowercase(&entry.orth);
        if lower == entry.orth {
            if let Some(mixed_orths) = self.mixed_orths_by_lower.get(&lower) {
                variants.extend(mixed_orths.iter().map(|orth| EntryVariant {
                    orth: orth.clone(),
                    lemma: entry.lemma.clone(),
                    entry: entry.clone(),
                }));
            }
        } else if !self.by_orth.contains_key(&lower) {
            variants.push(EntryVariant {
                orth: lower,
                lemma: entry.lemma.clone(),
                entry: entry.clone(),
            });
        }
        variants
    }
}

fn prefix_variants(prefix: &DictionaryEntry, prefix_tag: &str) -> Vec<PieceVariant> {
    let mut variants = vec![PieceVariant {
        orth: prefix.orth.clone(),
        lemma: if prefix.orth == lowercase(&prefix.orth) {
            prefix.lemma.clone()
        } else {
            prefix.orth.clone()
        },
    }];

    if prefix.orth == lowercase(&prefix.orth) {
        if let Some(titlecase) = titlecase_first(&prefix.orth) {
            if titlecase != prefix.orth {
                variants.push(PieceVariant {
                    orth: titlecase,
                    lemma: prefix.lemma.clone(),
                });
            }
        }
    }

    if prefix_tag == "nie" {
        variants.sort_by_key(|variant| usize::from(variant.orth != prefix.orth));
    }

    variants
}

fn is_case_alias_candidate(entry: &DictionaryEntry, resolver: &IdResolver) -> bool {
    !matches!(resolver.tag(entry.tag_id), Some("prefs" | "nie" | "interp"))
}

fn is_subst_entry(entry: &DictionaryEntry, resolver: &IdResolver) -> bool {
    resolver
        .tag(entry.tag_id)
        .map(|tag| tag.starts_with("subst:"))
        .unwrap_or(false)
}

fn lowercase(value: &str) -> String {
    value.to_lowercase()
}

fn titlecase_first(value: &str) -> Option<String> {
    let mut chars = value.chars();
    let first = chars.next()?;
    let mut titlecase = first.to_uppercase().collect::<String>();
    titlecase.push_str(chars.as_str());
    Some(titlecase)
}

fn lemma_base(lemma: &str) -> &str {
    lemma.split_once(':').map(|(base, _)| base).unwrap_or(lemma)
}

fn insert_adja_adj_shift_entries(
    entries: &[DictionaryEntry],
    resolver: &IdResolver,
    dictionary: &mut Dictionary,
) {
    let adja_entries = entries
        .iter()
        .filter(|entry| resolver.tag(entry.tag_id) == Some("adja"))
        .collect::<Vec<_>>();
    let adj_entries = entries
        .iter()
        .filter(|entry| {
            resolver
                .tag(entry.tag_id)
                .map(|tag| tag.starts_with("adj:"))
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();

    for adja in &adja_entries {
        for adj in &adj_entries {
            dictionary.insert(DictionaryEntry {
                orth: format!("{}{}", adja.orth, adj.orth),
                lemma: format!("{}{}", adja.orth, adj.lemma),
                tag_id: adj.tag_id,
                name_id: adj.name_id,
                labels_id: adj.labels_id,
            });
        }
    }
}

fn with_labels(
    entry: &DictionaryEntry,
    dictionary: &mut Dictionary,
    labels: &str,
) -> DictionaryEntry {
    let mut entry = entry.clone();
    entry.labels_id = dictionary
        .resolver_mut()
        .get_or_insert_labels_in_order(labels);
    entry
}

fn ordered_standalone_labels(
    entry: &DictionaryEntry,
    resolver: &IdResolver,
    hints: &SegmentationHints,
) -> String {
    let Some(labels) = resolver.labels(entry.labels_id) else {
        return "_".to_owned();
    };
    let mut ordered = hints
        .standalone_labels
        .iter()
        .filter(|label| labels.contains(*label))
        .cloned()
        .collect::<Vec<_>>();
    let mut rest = labels
        .iter()
        .filter(|label| !hints.standalone_labels.contains(*label))
        .cloned()
        .collect::<Vec<_>>();
    rest.sort();
    ordered.extend(rest);
    join_labels(ordered)
}

fn ordered_prefixed_labels(
    entry: &DictionaryEntry,
    prefix: &DictionaryEntry,
    resolver: &IdResolver,
    hints: &SegmentationHints,
) -> String {
    let Some(labels) = resolver.labels(entry.labels_id) else {
        return "_".to_owned();
    };
    let prefix_label = &prefix.orth;
    if prefix_label == "euro" && labels.len() > 1 {
        let mut ordered = hints
            .standalone_labels
            .iter()
            .filter(|label| labels.contains(*label))
            .cloned()
            .collect::<Vec<_>>();
        let mut middle = labels
            .iter()
            .filter(|label| *label != prefix_label && !hints.standalone_labels.contains(*label))
            .cloned()
            .collect::<Vec<_>>();
        middle.sort();
        ordered.extend(middle);
        ordered.push(prefix_label.clone());
        return join_labels(ordered);
    }

    let mut ordered = Vec::new();
    if labels.contains(prefix_label) {
        ordered.push(prefix_label.clone());
    }
    let mut rest = labels
        .iter()
        .filter(|label| *label != prefix_label)
        .cloned()
        .collect::<Vec<_>>();
    rest.sort();
    ordered.extend(rest);
    join_labels(ordered)
}

fn join_labels(labels: Vec<String>) -> String {
    if labels.is_empty() {
        "_".to_owned()
    } else {
        labels.join("|")
    }
}

fn should_keep_standalone(
    entry: &DictionaryEntry,
    resolver: &IdResolver,
    hints: &SegmentationHints,
) -> bool {
    if resolver
        .tag(entry.tag_id)
        .map(|tag| tag == "prefs")
        .unwrap_or(false)
    {
        return false;
    }
    if hints.prefix_labels.is_empty() {
        return true;
    }
    let Some(labels) = resolver.labels(entry.labels_id) else {
        return false;
    };
    if hints.standalone_labels.is_empty() {
        return true;
    }
    labels
        .iter()
        .any(|label| hints.standalone_labels.contains(label))
        && !labels
            .iter()
            .any(|label| hints.prefix_labels.contains(label))
}

fn should_combine_with_prefix(
    entry: &DictionaryEntry,
    prefix: &DictionaryEntry,
    resolver: &IdResolver,
) -> bool {
    if resolver
        .tag(entry.tag_id)
        .map(|tag| tag == "prefs")
        .unwrap_or(false)
    {
        return false;
    }
    resolver
        .labels(entry.labels_id)
        .map(|labels| labels.contains(&prefix.orth))
        .unwrap_or(false)
}
