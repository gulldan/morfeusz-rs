use super::*;

pub(super) type InterpsGroupId = (usize, usize);
pub(super) type AnalyzerDecodeCacheMap = HashMap<
    InterpsGroupId,
    Arc<[BinaryAnalyzerInterpretation]>,
    BuildHasherDefault<FastInterpsGroupHasher>,
>;
pub(super) type GeneratorDecodeCacheMap = HashMap<
    InterpsGroupId,
    Arc<[EncodedGeneratorInterpretation]>,
    BuildHasherDefault<FastInterpsGroupHasher>,
>;

#[derive(Default)]
pub(super) struct FastInterpsGroupHasher(u64);

impl FastInterpsGroupHasher {
    fn mix(&mut self, value: u64) {
        const K: u64 = 0x9e37_79b1_85eb_ca87;
        let mut value = value.wrapping_add(K);
        value ^= value >> 33;
        value = value.wrapping_mul(0xff51_afd7_ed55_8ccd);
        value ^= value >> 33;
        self.0 = self.0.rotate_left(5) ^ value;
    }
}

impl Hasher for FastInterpsGroupHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        let mut chunks = bytes.chunks_exact(8);
        for chunk in &mut chunks {
            self.mix(u64::from_ne_bytes(chunk.try_into().expect("8-byte chunk")));
        }
        let remainder = chunks.remainder();
        if !remainder.is_empty() {
            let mut last = [0u8; 8];
            last[..remainder.len()].copy_from_slice(remainder);
            self.mix(u64::from_ne_bytes(last));
        }
    }

    fn write_usize(&mut self, value: usize) {
        self.mix(value as u64);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct WordTemplateCacheKey {
    pub(super) hash: u64,
    pub(super) config_key: u16,
}

#[derive(Debug, Clone)]
pub(super) enum OrthTemplate {
    InputWord,
    Owned(Box<str>),
}

#[derive(Debug, Clone)]
pub(super) struct MorphInterpretationTemplate {
    rel_start_node: i32,
    rel_end_node: i32,
    orth: OrthTemplate,
    lemma: Box<str>,
    tag_id: i32,
    name_id: i32,
    labels_id: i32,
}

impl MorphInterpretationTemplate {
    fn from_interpretation(
        interp: &MorphInterpretation,
        input_word: &str,
        start_node: i32,
    ) -> Self {
        let orth = if interp.orth == input_word {
            OrthTemplate::InputWord
        } else {
            OrthTemplate::Owned(interp.orth.clone().into_boxed_str())
        };
        Self {
            rel_start_node: interp.start_node - start_node,
            rel_end_node: interp.end_node - start_node,
            orth,
            lemma: interp.lemma.clone().into_boxed_str(),
            tag_id: interp.tag_id,
            name_id: interp.name_id,
            labels_id: interp.labels_id,
        }
    }

    fn instantiate(&self, input_word: &str, start_node: i32) -> MorphInterpretation {
        MorphInterpretation {
            start_node: start_node + self.rel_start_node,
            end_node: start_node + self.rel_end_node,
            orth: match &self.orth {
                OrthTemplate::InputWord => input_word.to_owned(),
                OrthTemplate::Owned(orth) => orth.to_string(),
            },
            lemma: self.lemma.to_string(),
            tag_id: self.tag_id,
            name_id: self.name_id,
            labels_id: self.labels_id,
        }
    }
}

#[derive(Debug)]
pub(super) struct WordAnalysisTemplate {
    hash: u64,
    config_key: u16,
    word: Box<str>,
    next_node_delta: i32,
    interps: Box<[MorphInterpretationTemplate]>,
}

impl WordAnalysisTemplate {
    fn from_result(
        word: &str,
        key: WordTemplateCacheKey,
        interps: &[MorphInterpretation],
        start_node: i32,
        next_node: i32,
    ) -> Option<Self> {
        Some(Self {
            hash: key.hash,
            config_key: key.config_key,
            word: word.to_owned().into_boxed_str(),
            next_node_delta: next_node.checked_sub(start_node)?,
            interps: interps
                .iter()
                .map(|interp| {
                    MorphInterpretationTemplate::from_interpretation(interp, word, start_node)
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        })
    }

    fn instantiate(&self, word: &str, start_node: i32) -> (Vec<MorphInterpretation>, i32) {
        (
            self.interps
                .iter()
                .map(|interp| interp.instantiate(word, start_node))
                .collect(),
            start_node + self.next_node_delta,
        )
    }
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct WordTemplateCacheStats {
    pub(super) lookups: u64,
    pub(super) hits: u64,
    pub(super) first_seen: u64,
    pub(super) second_seen: u64,
    pub(super) inserts: u64,
    pub(super) reject_len: u64,
    pub(super) reject_template_count: u64,
    pub(super) reject_admission: u64,
    pub(super) reject_full: u64,
}

#[derive(Debug)]
pub(super) struct WordTemplateCache {
    buckets: HashMap<u64, Vec<WordAnalysisTemplate>>,
    seen_once: Box<[u64]>,
    entries: usize,
    #[cfg(test)]
    stats: WordTemplateCacheStats,
}

impl Default for WordTemplateCache {
    fn default() -> Self {
        Self {
            buckets: HashMap::with_capacity(WORD_TEMPLATE_CACHE_MAX_WORDS),
            seen_once: vec![0; WORD_TEMPLATE_SEEN_ONCE_SLOTS].into_boxed_slice(),
            entries: 0,
            #[cfg(test)]
            stats: WordTemplateCacheStats::default(),
        }
    }
}

impl WordTemplateCache {
    fn get(
        &mut self,
        word: &str,
        key: WordTemplateCacheKey,
        start_node: i32,
    ) -> Option<(Vec<MorphInterpretation>, i32)> {
        #[cfg(test)]
        {
            self.stats.lookups += 1;
        }
        let bucket = self.buckets.get_mut(&key.hash)?;
        for entry in bucket {
            if entry.hash == key.hash && entry.config_key == key.config_key && &*entry.word == word
            {
                #[cfg(test)]
                {
                    self.stats.hits += 1;
                }
                return Some(entry.instantiate(word, start_node));
            }
        }
        None
    }

    fn insert_if_admitted(
        &mut self,
        word: &str,
        key: WordTemplateCacheKey,
        interps: &[MorphInterpretation],
        start_node: i32,
        next_node: i32,
    ) {
        if word.len() > WORD_TEMPLATE_MAX_WORD_BYTES {
            #[cfg(test)]
            {
                self.stats.reject_len += 1;
            }
            return;
        }
        if interps.len() > WORD_TEMPLATE_MAX_INTERPRETATIONS {
            #[cfg(test)]
            {
                self.stats.reject_template_count += 1;
            }
            return;
        }
        if !interps.iter().any(|interp| !interp.is_ign()) {
            #[cfg(test)]
            {
                self.stats.reject_admission += 1;
            }
            return;
        }
        if !self.seen_again(key.hash) {
            return;
        }
        if self.entries >= WORD_TEMPLATE_CACHE_MAX_WORDS {
            #[cfg(test)]
            {
                self.stats.reject_full += 1;
            }
            return;
        }
        if self.buckets.get(&key.hash).is_some_and(|bucket| {
            bucket
                .iter()
                .any(|entry| entry.config_key == key.config_key && &*entry.word == word)
        }) {
            return;
        }
        let Some(template) =
            WordAnalysisTemplate::from_result(word, key, interps, start_node, next_node)
        else {
            return;
        };
        self.buckets.entry(key.hash).or_default().push(template);
        self.entries += 1;
        #[cfg(test)]
        {
            self.stats.inserts += 1;
        }
    }

    fn seen_again(&mut self, fingerprint: u64) -> bool {
        debug_assert!(self.seen_once.len().is_power_of_two());
        let index = fingerprint as usize & (self.seen_once.len() - 1);
        if self.seen_once[index] == fingerprint {
            #[cfg(test)]
            {
                self.stats.second_seen += 1;
            }
            true
        } else {
            self.seen_once[index] = fingerprint;
            #[cfg(test)]
            {
                self.stats.first_seen += 1;
            }
            false
        }
    }

    #[cfg(test)]
    fn stats(&self) -> WordTemplateCacheStats {
        self.stats
    }
}

#[derive(Debug, Clone, Default)]
pub(super) struct SharedWordTemplateCache {
    cache: Arc<Mutex<WordTemplateCache>>,
}

impl SharedWordTemplateCache {
    pub(super) fn get(
        &self,
        word: &str,
        key: WordTemplateCacheKey,
        start_node: i32,
    ) -> Result<Option<(Vec<MorphInterpretation>, i32)>> {
        Ok(self
            .cache
            .lock()
            .map_err(|_| Error::invalid_dictionary("word template cache is poisoned"))?
            .get(word, key, start_node))
    }

    pub(super) fn insert_if_admitted(
        &self,
        word: &str,
        key: WordTemplateCacheKey,
        interps: &[MorphInterpretation],
        start_node: i32,
        next_node: i32,
    ) -> Result<()> {
        self.cache
            .lock()
            .map_err(|_| Error::invalid_dictionary("word template cache is poisoned"))?
            .insert_if_admitted(word, key, interps, start_node, next_node);
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn stats(&self) -> Result<WordTemplateCacheStats> {
        Ok(self
            .cache
            .lock()
            .map_err(|_| Error::invalid_dictionary("word template cache is poisoned"))?
            .stats())
    }
}

#[derive(Debug, Clone, Default)]
pub(super) struct SharedAnalyzerGroupDecodeCache {
    groups: Arc<Mutex<AnalyzerDecodeCacheMap>>,
}

impl SharedAnalyzerGroupDecodeCache {
    fn get_or_decode(
        &self,
        group_id: InterpsGroupId,
        raw_group: RawInterpsGroup<'_>,
    ) -> Result<Arc<[BinaryAnalyzerInterpretation]>> {
        if let Some(cached) = self
            .groups
            .lock()
            .map_err(|_| Error::invalid_dictionary("analyzer decode cache is poisoned"))?
            .get(&group_id)
            .cloned()
        {
            return Ok(cached);
        }

        let decoded: Arc<[BinaryAnalyzerInterpretation]> =
            decode_binary_analyzer_interpretations(raw_group)?.into();
        let mut groups = self
            .groups
            .lock()
            .map_err(|_| Error::invalid_dictionary("analyzer decode cache is poisoned"))?;
        if groups.len() >= ANALYZER_DECODE_CACHE_MAX_GROUPS && !groups.contains_key(&group_id) {
            groups.clear();
        }
        Ok(groups
            .entry(group_id)
            .or_insert_with(|| Arc::clone(&decoded))
            .clone())
    }
}

#[derive(Debug)]
pub(super) struct AnalyzerGroupDecodeCache {
    shared: SharedAnalyzerGroupDecodeCache,
    groups: Vec<(InterpsGroupId, Arc<[BinaryAnalyzerInterpretation]>)>,
}

impl AnalyzerGroupDecodeCache {
    pub(super) fn with_capacity(shared: SharedAnalyzerGroupDecodeCache, capacity: usize) -> Self {
        Self {
            shared,
            groups: Vec::with_capacity(capacity),
        }
    }

    pub(super) fn get_or_decode(
        &mut self,
        group_id: InterpsGroupId,
        raw_group: RawInterpsGroup<'_>,
    ) -> Result<usize> {
        if let Some(index) = self
            .groups
            .iter()
            .position(|(cached_id, _)| *cached_id == group_id)
        {
            return Ok(index);
        }

        let interpretations = self.shared.get_or_decode(group_id, raw_group)?;
        self.groups.push((group_id, interpretations));
        Ok(self.groups.len() - 1)
    }

    pub(super) fn interpretations(&self, index: usize) -> &[BinaryAnalyzerInterpretation] {
        &self.groups[index].1
    }
}

impl Default for AnalyzerGroupDecodeCache {
    fn default() -> Self {
        Self::with_capacity(SharedAnalyzerGroupDecodeCache::default(), 0)
    }
}

#[derive(Debug, Clone, Default)]
pub(super) struct SharedGeneratorGroupDecodeCache {
    groups: Arc<Mutex<GeneratorDecodeCacheMap>>,
}

impl SharedGeneratorGroupDecodeCache {
    fn get_or_decode(
        &self,
        group_id: InterpsGroupId,
        raw_group: RawInterpsGroup<'_>,
    ) -> Result<Arc<[EncodedGeneratorInterpretation]>> {
        if let Some(cached) = self
            .groups
            .lock()
            .map_err(|_| Error::invalid_dictionary("generator decode cache is poisoned"))?
            .get(&group_id)
            .cloned()
        {
            return Ok(cached);
        }

        let decoded: Arc<[EncodedGeneratorInterpretation]> =
            decode_generator_interpretations(raw_group)?.into();
        let mut groups = self
            .groups
            .lock()
            .map_err(|_| Error::invalid_dictionary("generator decode cache is poisoned"))?;
        if groups.len() >= GENERATOR_DECODE_CACHE_MAX_GROUPS && !groups.contains_key(&group_id) {
            groups.clear();
        }
        Ok(groups
            .entry(group_id)
            .or_insert_with(|| Arc::clone(&decoded))
            .clone())
    }
}

#[derive(Debug)]
pub(super) struct GeneratorGroupDecodeCache {
    shared: SharedGeneratorGroupDecodeCache,
    groups: Vec<(InterpsGroupId, Arc<[EncodedGeneratorInterpretation]>)>,
}

impl GeneratorGroupDecodeCache {
    pub(super) fn with_capacity(shared: SharedGeneratorGroupDecodeCache, capacity: usize) -> Self {
        Self {
            shared,
            groups: Vec::with_capacity(capacity),
        }
    }

    pub(super) fn get_or_decode(
        &mut self,
        group_id: InterpsGroupId,
        raw_group: RawInterpsGroup<'_>,
    ) -> Result<usize> {
        if let Some(index) = self
            .groups
            .iter()
            .position(|(cached_id, _)| *cached_id == group_id)
        {
            return Ok(index);
        }

        let interpretations = self.shared.get_or_decode(group_id, raw_group)?;
        self.groups.push((group_id, interpretations));
        Ok(self.groups.len() - 1)
    }

    pub(super) fn interpretations(&self, index: usize) -> &[EncodedGeneratorInterpretation] {
        &self.groups[index].1
    }
}

pub(super) fn option_code(value: Option<&str>, known: &[(&str, u8)]) -> Option<u8> {
    match value {
        None => Some(0),
        Some(value) => known
            .iter()
            .find_map(|(candidate, code)| (*candidate == value).then_some(*code)),
    }
}

pub(super) fn word_template_hash(word: &str, config_key: u16) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in word.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    for byte in config_key.to_le_bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    if hash == 0 {
        1
    } else {
        hash
    }
}
