use super::*;

#[derive(Debug, Clone)]
pub struct BinaryGeneratorLexicon {
    data: BinaryDictionaryData,
    resolver: Arc<IdResolver>,
    segmentation_metadata: SegmentationMetadata,
    default_segmentation_variant_index: Option<usize>,
    pub(super) generator_decode_cache: SharedGeneratorGroupDecodeCache,
}

impl BinaryGeneratorLexicon {
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self> {
        Self::from_data(BinaryDictionaryData::from_path(path)?)
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self> {
        Self::from_data(BinaryDictionaryData::from_bytes(bytes)?)
    }

    pub fn from_data(data: BinaryDictionaryData) -> Result<Self> {
        let _ = data.fsa()?;
        let resolver = Arc::new(data.id_resolver()?);
        let segmentation_metadata = data.segmentation_metadata()?;
        let default_segmentation_variant_index =
            default_segmentation_fsa_variant_index(&segmentation_metadata);
        Ok(Self {
            data,
            resolver,
            segmentation_metadata,
            default_segmentation_variant_index,
            generator_decode_cache: SharedGeneratorGroupDecodeCache::default(),
        })
    }

    pub fn synthesize_encoded_groups(
        &self,
        lemma: &str,
    ) -> Result<Vec<EncodedGeneratorInterpsGroup>> {
        let Some(raw_match) = self
            .data
            .fsa_unchecked()
            .try_recognize_loaded(lemma.as_bytes())?
        else {
            return Ok(Vec::new());
        };
        decode_generator_interps_groups(raw_match.value)
    }

    pub fn synthesize_with_segmentation(
        &self,
        lemma: &str,
        segmentation: &SegmentationPreset,
    ) -> Result<Vec<MorphInterpretation>> {
        let (lookup_lemma, required_homonym_id) = split_generator_lemma(lemma);
        let Some(segmentation_fsa) = self.segmentation_fsa(segmentation)? else {
            return Ok(Vec::new());
        };
        if lookup_lemma.is_empty() {
            return Ok(Vec::new());
        }

        let rules_fsa = SegmentationRulesFsa::from_data_unchecked(segmentation_fsa);
        let fsa = self.data.fsa_unchecked();
        let mut paths = Vec::with_capacity(2);
        let mut current_path = Vec::with_capacity(4);
        let mut decode_cache =
            GeneratorGroupDecodeCache::with_capacity(self.generator_decode_cache.clone(), 4);

        collect_segmented_generator_paths(
            fsa,
            &rules_fsa,
            lookup_lemma,
            0,
            rules_fsa.initial_state(),
            &mut current_path,
            &mut paths,
            &mut decode_cache,
        )?;

        generator_paths_to_morph_interpretations(paths, &decode_cache, required_homonym_id)
    }

    fn segmentation_fsa(&self, segmentation: &SegmentationPreset) -> Result<Option<&[u8]>> {
        if segmentation.aggl().is_none() && segmentation.praet().is_none() {
            if let Some(index) = self.default_segmentation_variant_index {
                return Ok(Some(
                    self.segmentation_metadata.fsa_variants[index]
                        .fsa
                        .as_slice(),
                ));
            }
        }
        segmentation_fsa_for_options(&self.segmentation_metadata, segmentation)
    }

    pub(super) fn validate_segmentation(
        &self,
        segmentation: &SegmentationPreset,
        option: &str,
        value: &str,
    ) -> Result<()> {
        validate_segmentation_options(&self.segmentation_metadata, segmentation, option, value)
    }

    fn has_segmentation_transitions(&self, segmentation: &SegmentationPreset) -> Result<bool> {
        Ok(has_initial_segmentation_transitions(
            self.segmentation_fsa(segmentation)?,
        ))
    }
}

impl Lexicon for BinaryGeneratorLexicon {
    fn try_fork(&self) -> Option<Arc<dyn Lexicon>> {
        let mut forked = self.clone();
        forked.generator_decode_cache = SharedGeneratorGroupDecodeCache::default();
        Some(Arc::new(forked))
    }

    fn id(&self) -> &str {
        self.data.dict_id()
    }

    fn copyright(&self) -> &str {
        self.data.copyright()
    }

    fn resolver(&self) -> &IdResolver {
        &self.resolver
    }

    fn lookup(&self, _orth: &str) -> Option<&[DictionaryEntry]> {
        None
    }

    fn synthesize(&self, _lemma: &str) -> Option<&[DictionaryEntry]> {
        None
    }

    fn default_aggl(&self) -> Option<&str> {
        self.segmentation_metadata
            .default_options
            .get("aggl")
            .map(String::as_str)
    }

    fn default_praet(&self) -> Option<&str> {
        self.segmentation_metadata
            .default_options
            .get("praet")
            .map(String::as_str)
    }

    fn available_aggl_options(&self) -> Vec<String> {
        available_options_vec(&self.segmentation_metadata, "aggl")
    }

    fn available_praet_options(&self) -> Vec<String> {
        available_options_vec(&self.segmentation_metadata, "praet")
    }

    fn validate_segmentation(
        &self,
        segmentation: &SegmentationPreset,
        option: &str,
        value: &str,
    ) -> Result<()> {
        validate_segmentation_options(&self.segmentation_metadata, segmentation, option, value)
    }

    fn synthesize_interpretations(
        &self,
        lemma: &str,
        segmentation: &SegmentationPreset,
    ) -> Result<Vec<MorphInterpretation>> {
        let segmented = self.synthesize_with_segmentation(lemma, segmentation)?;
        if !segmented.is_empty() {
            return Ok(segmented);
        }
        if self.has_segmentation_transitions(segmentation)? {
            return Ok(Vec::new());
        }

        let (lookup_lemma, required_homonym_id) = split_generator_lemma(lemma);
        let mut result = Vec::new();
        for group in self.synthesize_encoded_groups(lookup_lemma)? {
            for interp in group.interpretations {
                if !generator_homonym_matches(&interp, required_homonym_id) {
                    continue;
                }
                result.push(interp.to_morph_interpretation(lookup_lemma, 0, 0)?);
            }
        }
        Ok(result)
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct BinaryGeneratorChunk<'a> {
    lemma: &'a str,
    shift_orth: bool,
    interpretations: usize,
}

#[derive(Debug, Clone)]
pub(super) struct BinaryGeneratorPath<'a> {
    chunks: Vec<BinaryGeneratorChunk<'a>>,
    weak: bool,
}

pub(super) fn collect_segmented_generator_paths<'a>(
    fsa: BinaryFsa<'_>,
    rules_fsa: &SegmentationRulesFsa<'_>,
    lemma: &'a str,
    position: usize,
    segmentation_state: SegmentationState,
    current_path: &mut Vec<BinaryGeneratorChunk<'a>>,
    paths: &mut Vec<BinaryGeneratorPath<'a>>,
    decode_cache: &mut GeneratorGroupDecodeCache,
) -> Result<()> {
    if position >= lemma.len() {
        return Ok(());
    }

    fsa.for_each_prefix_match_loaded(&lemma.as_bytes()[position..], |prefix_match| {
        let end = position
            .checked_add(prefix_match.input_end)
            .ok_or_else(|| Error::invalid_dictionary("generator prefix offset overflow"))?;
        let Some(chunk_lemma) = lemma.get(position..end) else {
            return Ok(());
        };
        let at_end = end == lemma.len();

        for_each_raw_interps_group(prefix_match.value, |group_index, raw_group| {
            let segment_type = raw_group.segment_type;
            let Some(new_state) =
                rules_fsa.proceed_to_next_unchecked(segment_type, segmentation_state, at_end)
            else {
                return Ok(());
            };
            let group_id = (prefix_match.state_offset, group_index);
            let interpretations = decode_cache.get_or_decode(group_id, raw_group)?;

            current_path.push(BinaryGeneratorChunk {
                lemma: chunk_lemma,
                shift_orth: new_state.shift_orth_from_previous,
                interpretations,
            });

            if at_end {
                if new_state.accepting {
                    paths.push(BinaryGeneratorPath {
                        chunks: current_path.clone(),
                        weak: new_state.weak,
                    });
                }
            } else if !new_state.sink {
                collect_segmented_generator_paths(
                    fsa,
                    rules_fsa,
                    lemma,
                    end,
                    new_state,
                    current_path,
                    paths,
                    decode_cache,
                )?;
            }

            current_path.pop();
            Ok(())
        })?;
        Ok(())
    })?;

    Ok(())
}

pub(super) fn generator_paths_to_morph_interpretations(
    mut paths: Vec<BinaryGeneratorPath<'_>>,
    decode_cache: &GeneratorGroupDecodeCache,
    required_homonym_id: Option<&str>,
) -> Result<Vec<MorphInterpretation>> {
    if paths.iter().any(|path| !path.weak) {
        paths.retain(|path| !path.weak);
    }

    let capacity = paths
        .iter()
        .flat_map(|path| path.chunks.iter())
        .map(|chunk| decode_cache.interpretations(chunk.interpretations).len())
        .sum();
    let mut result = Vec::with_capacity(capacity);
    for path in paths {
        let mut index = 0;
        while index < path.chunks.len() {
            let mut shifted_end = index;
            while shifted_end + 1 < path.chunks.len() && path.chunks[shifted_end].shift_orth {
                shifted_end += 1;
            }

            if shifted_end > index {
                push_shifted_generator_interpretations(
                    &path.chunks[index..=shifted_end],
                    decode_cache,
                    &mut result,
                    required_homonym_id,
                )?;
            } else {
                push_plain_generator_interpretations(
                    &path.chunks[index],
                    decode_cache,
                    &mut result,
                    required_homonym_id,
                )?;
            }

            index = shifted_end + 1;
        }
    }
    Ok(result)
}

pub(super) fn push_plain_generator_interpretations(
    chunk: &BinaryGeneratorChunk<'_>,
    decode_cache: &GeneratorGroupDecodeCache,
    result: &mut Vec<MorphInterpretation>,
    required_homonym_id: Option<&str>,
) -> Result<()> {
    for interp in decode_cache.interpretations(chunk.interpretations) {
        if !generator_homonym_matches(interp, required_homonym_id) {
            continue;
        }
        result.push(interp.to_morph_interpretation(chunk.lemma, 0, 0)?);
    }
    Ok(())
}

pub(super) fn push_shifted_generator_interpretations(
    chunks: &[BinaryGeneratorChunk<'_>],
    decode_cache: &GeneratorGroupDecodeCache,
    result: &mut Vec<MorphInterpretation>,
    required_homonym_id: Option<&str>,
) -> Result<()> {
    let Some((current, prefixes)) = chunks.split_last() else {
        return Ok(());
    };
    let mut lemma =
        String::with_capacity(chunks.iter().map(|chunk| chunk.lemma.len()).sum::<usize>());
    for chunk in chunks {
        lemma.push_str(chunk.lemma);
    }
    let mut orth_prefix = String::with_capacity(
        prefixes
            .iter()
            .map(|chunk| chunk.lemma.len())
            .sum::<usize>(),
    );
    for chunk in prefixes {
        orth_prefix.push_str(chunk.lemma);
    }

    for interp in decode_cache.interpretations(current.interpretations) {
        if !generator_homonym_matches(interp, required_homonym_id) {
            continue;
        }
        let mut morph = interp.to_morph_interpretation(current.lemma, 0, 0)?;
        let mut orth = String::with_capacity(orth_prefix.len() + morph.orth.len());
        orth.push_str(&orth_prefix);
        orth.push_str(&morph.orth);
        morph.orth = orth;
        morph.lemma = if interp.homonym_id.is_empty() {
            lemma.clone()
        } else {
            let mut with_homonym = String::with_capacity(lemma.len() + 1 + interp.homonym_id.len());
            with_homonym.push_str(&lemma);
            with_homonym.push(':');
            with_homonym.push_str(&interp.homonym_id);
            with_homonym
        };
        result.push(morph);
    }
    Ok(())
}

pub(super) fn split_generator_lemma(lemma: &str) -> (&str, Option<&str>) {
    lemma
        .split_once(':')
        .map(|(base, homonym_id)| (base, Some(homonym_id)))
        .unwrap_or((lemma, None))
}

pub(super) fn generator_homonym_matches(
    interp: &EncodedGeneratorInterpretation,
    required_homonym_id: Option<&str>,
) -> bool {
    required_homonym_id
        .map(|required| interp.homonym_id == required)
        .unwrap_or(true)
}
