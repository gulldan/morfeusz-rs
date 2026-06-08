use std::collections::{BTreeSet, HashMap};

use crate::{Error, Result};

#[derive(Debug, Clone)]
pub struct IdResolver {
    tagset_id: String,
    tags: Vec<String>,
    tag_ids: HashMap<String, i32>,
    names: Vec<String>,
    name_ids: HashMap<String, i32>,
    labels: Vec<String>,
    label_ids: HashMap<String, i32>,
    labels_as_sets: Vec<BTreeSet<String>>,
}

impl Default for IdResolver {
    fn default() -> Self {
        let mut resolver = Self {
            tagset_id: String::new(),
            tags: Vec::new(),
            tag_ids: HashMap::new(),
            names: Vec::new(),
            name_ids: HashMap::new(),
            labels: Vec::new(),
            label_ids: HashMap::new(),
            labels_as_sets: Vec::new(),
        };
        resolver.set_tag(0, "ign");
        resolver.set_tag(1, "sp");
        resolver.get_or_insert_name("_");
        resolver.get_or_insert_labels("_");
        resolver
    }
}

impl IdResolver {
    pub fn tagset_id(&self) -> &str {
        &self.tagset_id
    }

    pub fn tag(&self, tag_id: i32) -> Option<&str> {
        self.tags.get(tag_id as usize).map(String::as_str)
    }

    pub fn tag_id(&self, tag: &str) -> Result<i32> {
        self.tag_ids
            .get(tag)
            .copied()
            .ok_or_else(|| Error::InvalidArgument(format!("Invalid tag: {tag}")))
    }

    pub fn name(&self, name_id: i32) -> Option<&str> {
        self.names.get(name_id as usize).map(String::as_str)
    }

    pub fn name_id(&self, name: &str) -> Result<i32> {
        self.name_ids
            .get(name)
            .copied()
            .ok_or_else(|| Error::InvalidArgument(format!("Invalid name: {name}")))
    }

    pub fn labels_as_string(&self, labels_id: i32) -> Option<&str> {
        self.labels.get(labels_id as usize).map(String::as_str)
    }

    pub fn labels(&self, labels_id: i32) -> Option<&BTreeSet<String>> {
        self.labels_as_sets.get(labels_id as usize)
    }

    pub fn labels_id(&self, labels: &str) -> Result<i32> {
        let labels = normalize_empty_id_value(labels);
        self.label_ids
            .get(labels)
            .copied()
            .ok_or_else(|| Error::InvalidArgument(format!("Invalid labels string: {labels}")))
    }

    pub fn tags_count(&self) -> usize {
        self.tags.len()
    }

    pub fn names_count(&self) -> usize {
        self.names.len()
    }

    pub fn labels_count(&self) -> usize {
        self.labels.len()
    }

    pub fn get_or_insert_tag(&mut self, tag: &str) -> i32 {
        if let Some(id) = self.tag_ids.get(tag) {
            return *id;
        }
        let id = self.tags.len() as i32;
        self.set_tag(id, tag);
        id
    }

    pub fn get_or_insert_name(&mut self, name: &str) -> i32 {
        let normalized = normalize_empty_id_value(name);
        if let Some(id) = self.name_ids.get(normalized) {
            return *id;
        }
        let id = self.names.len() as i32;
        self.names.push(normalized.to_owned());
        self.name_ids.insert(normalized.to_owned(), id);
        id
    }

    pub fn get_or_insert_labels(&mut self, labels: &str) -> i32 {
        let labels = canonicalize_labels(labels);
        self.get_or_insert_labels_in_order(&labels)
    }

    pub(crate) fn get_or_insert_labels_in_order(&mut self, labels: &str) -> i32 {
        let labels = normalize_empty_id_value(labels).to_owned();
        if let Some(id) = self.label_ids.get(&labels) {
            return *id;
        }
        let id = self.labels.len() as i32;
        self.labels.push(labels.clone());
        self.label_ids.insert(labels.clone(), id);
        self.labels_as_sets.push(labels_to_set(&labels));
        id
    }

    pub(crate) fn set_tagset_id(&mut self, tagset_id: impl Into<String>) {
        self.tagset_id = tagset_id.into();
    }

    pub(crate) fn set_tag(&mut self, id: i32, tag: &str) {
        let index = id as usize;
        if self.tags.len() <= index {
            self.tags.resize(index + 1, String::new());
        }
        if !self.tags[index].is_empty() {
            self.tag_ids.remove(&self.tags[index]);
        }
        self.tags[index] = tag.to_owned();
        self.tag_ids.insert(tag.to_owned(), id);
    }

    pub(crate) fn set_name(&mut self, id: i32, name: &str) {
        let index = id as usize;
        let name = normalize_empty_id_value(name);
        if self.names.len() <= index {
            self.names.resize(index + 1, String::new());
        }
        if !self.names[index].is_empty() {
            self.name_ids.remove(&self.names[index]);
        }
        self.names[index] = name.to_owned();
        self.name_ids.insert(name.to_owned(), id);
    }

    pub(crate) fn set_labels_in_order(&mut self, id: i32, labels: &str) {
        let index = id as usize;
        let labels = normalize_empty_id_value(labels);
        if self.labels.len() <= index {
            self.labels.resize(index + 1, String::new());
            self.labels_as_sets.resize(index + 1, BTreeSet::new());
        }
        if !self.labels[index].is_empty() {
            self.label_ids.remove(&self.labels[index]);
        }
        self.labels[index] = labels.to_owned();
        self.label_ids.insert(labels.to_owned(), id);
        self.labels_as_sets[index] = labels_to_set(labels);
    }
}

fn normalize_empty_id_value(value: &str) -> &str {
    if value.trim().is_empty() {
        "_"
    } else {
        value.trim()
    }
}

pub(crate) fn canonicalize_labels(labels: &str) -> String {
    let labels = normalize_empty_id_value(labels);
    if labels == "_" {
        return "_".to_owned();
    }
    let mut parts = labels
        .split('|')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    parts.sort_unstable();
    parts.dedup();
    if parts.is_empty() {
        "_".to_owned()
    } else {
        parts.join("|")
    }
}

fn labels_to_set(labels: &str) -> BTreeSet<String> {
    if labels == "_" {
        BTreeSet::new()
    } else {
        labels.split('|').map(ToOwned::to_owned).collect()
    }
}
