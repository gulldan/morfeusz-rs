use super::*;

#[derive(Debug, Clone)]
pub struct BinaryAnalyzerLexicon {
    data: BinaryDictionaryData,
    // `Arc` so per-thread forks share the (large, immutable) id tables instead
    // of deep-copying them.
    resolver: Arc<IdResolver>,
    segmentation_metadata: SegmentationMetadata,
    default_segmentation_variant_index: Option<usize>,
    default_aggl_cache_code: Option<u8>,
    default_praet_cache_code: Option<u8>,
    pub(super) analyzer_decode_cache: SharedAnalyzerGroupDecodeCache,
    pub(super) word_template_cache: SharedWordTemplateCache,
}

impl BinaryAnalyzerLexicon {
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
        let default_aggl_cache_code = option_code(
            segmentation_metadata
                .default_options
                .get("aggl")
                .map(String::as_str),
            &[("strict", 1), ("permissive", 2), ("isolated", 3)],
        );
        let default_praet_cache_code = option_code(
            segmentation_metadata
                .default_options
                .get("praet")
                .map(String::as_str),
            &[("split", 1), ("composite", 2)],
        );
        Ok(Self {
            data,
            resolver,
            segmentation_metadata,
            default_segmentation_variant_index,
            default_aggl_cache_code,
            default_praet_cache_code,
            analyzer_decode_cache: SharedAnalyzerGroupDecodeCache::default(),
            word_template_cache: SharedWordTemplateCache::default(),
        })
    }

    pub fn lookup_encoded_groups(
        &self,
        orth: &str,
    ) -> Result<Option<Vec<EncodedAnalyzerInterpsGroup>>> {
        let lookup = lowercase_with_original_boundaries(orth);
        let Some(raw_match) = self
            .data
            .fsa_unchecked()
            .try_recognize_loaded(lookup.as_str().as_bytes())?
        else {
            return Ok(None);
        };
        Ok(Some(decode_analyzer_interps_groups(raw_match.value)?))
    }

    pub fn analyze_word_with_segmentation(
        &self,
        orth: &str,
        start_node: i32,
        case_handling: CaseHandling,
        segmentation: &SegmentationPreset,
    ) -> Result<Option<(Vec<MorphInterpretation>, i32)>> {
        let Some(word_paths) = self.word_paths(orth, segmentation)? else {
            return Ok(None);
        };
        let BinaryAnalyzerWordPaths {
            paths,
            decode_cache,
        } = word_paths;
        paths_to_morph_interpretations(paths, &decode_cache, start_node, case_handling)
    }

    /// Collects the raw FSA+segrules segmentation paths for a single word.
    /// Returns `None` when the dictionary has no segmentation FSA for the given
    /// options, `Some(vec![])` when the FSA produced no accepting path (the word
    /// is unknown), and `Some(paths)` otherwise. This separation lets callers
    /// distinguish "unknown word" (drives ign separator splitting) from "graph
    /// produced but case-filtered to nothing" (drives a whole-word ignotium),
    /// matching C++ `processOneWord`.
    fn word_paths<'a>(
        &self,
        orth: &'a str,
        segmentation: &SegmentationPreset,
    ) -> Result<Option<BinaryAnalyzerWordPaths<'a>>> {
        let Some(segmentation_fsa) = self.segmentation_fsa(segmentation)? else {
            return Ok(None);
        };
        // A segmentation FSA with no initial transitions (the `[1, 0]`
        // placeholder used by minimal/hand-built dictionaries) means the
        // dictionary carries no real segmentation rules; signal that to the
        // caller so it falls back to a plain lookup instead of ign splitting.
        if !has_initial_segmentation_transitions(Some(segmentation_fsa)) {
            return Ok(None);
        }
        if orth.is_empty() {
            return Ok(Some(BinaryAnalyzerWordPaths::empty()));
        }

        let rules_fsa = SegmentationRulesFsa::from_data_unchecked(segmentation_fsa);
        let fsa = self.data.fsa_unchecked();
        let normalized = lowercase_with_original_boundaries(orth);
        let path_capacity = normalized.path_capacity_hint();
        let mut paths = Vec::with_capacity(4);
        let mut current_path = Vec::with_capacity(path_capacity);
        let mut decode_cache =
            AnalyzerGroupDecodeCache::with_capacity(self.analyzer_decode_cache.clone(), 8);

        collect_segmented_analyzer_paths(
            fsa,
            &rules_fsa,
            &normalized,
            orth,
            0,
            rules_fsa.initial_state(),
            &mut current_path,
            &mut paths,
            &mut decode_cache,
        )?;

        Ok(Some(BinaryAnalyzerWordPaths {
            paths,
            decode_cache,
        }))
    }

    /// Faithful port of C++ `MorfeuszImpl::processOneWord` for a single
    /// whitespace-delimited word (whitespace handling lives in the engine).
    ///
    /// Returns the interpretations plus the next free node number. Unknown
    /// chunks are split on dictionary-defined separator characters and each part
    /// re-analyzed, exactly like `handleIgnChunk`; a wholly unknown chunk (or a
    /// chunk that produced a graph but no decodable results) becomes a single
    /// `ign` spanning the word.
    pub fn analyze_native_word(
        &self,
        word: &str,
        start_node: i32,
        case_handling: CaseHandling,
        segmentation: &SegmentationPreset,
    ) -> Result<(Vec<MorphInterpretation>, i32)> {
        self.process_one_word(word, start_node, case_handling, segmentation, false)
    }

    fn process_one_word(
        &self,
        word: &str,
        start_node: i32,
        case_handling: CaseHandling,
        segmentation: &SegmentationPreset,
        inside_ign_handler: bool,
    ) -> Result<(Vec<MorphInterpretation>, i32)> {
        if word.is_empty() {
            return Ok((Vec::new(), start_node));
        }

        let cache_key = (!inside_ign_handler)
            .then(|| self.word_template_cache_key(word, case_handling, segmentation))
            .flatten();
        if let Some(key) = cache_key {
            if let Some(cached) = self.word_template_cache.get(word, key, start_node)? {
                return Ok(cached);
            }
        }

        let result = match self.word_paths(word, segmentation)? {
            // Real builder dictionaries expose a segmentation FSA.
            Some(word_paths) if !word_paths.paths.is_empty() => {
                let BinaryAnalyzerWordPaths {
                    paths,
                    decode_cache,
                } = word_paths;
                match paths_to_morph_interpretations(
                    paths,
                    &decode_cache,
                    start_node,
                    case_handling,
                )? {
                    Some((interps, nodes)) => Ok((interps, start_node + nodes)),
                    // Graph existed but decoded to nothing (e.g. case-filtered):
                    // C++ appends a single ignotium for the whole word.
                    None => Ok((vec![ignotium(word, start_node)], start_node + 1)),
                }
            }
            // Segmentation FSA present but no accepting path: unknown word. C++
            // splits it on dictionary separators and re-analyzes each part,
            // unless we are already inside the ign handler.
            Some(_) if inside_ign_handler => Ok((vec![ignotium(word, start_node)], start_node + 1)),
            Some(_) => self.handle_ign_chunk(word, start_node, case_handling, segmentation),
            // No segmentation FSA at all (minimal / hand-built dictionaries):
            // fall back to a plain case-aware single-edge lookup.
            None => match self.lookup_word_interpretations(word, start_node, case_handling)? {
                Some(interps) => Ok((interps, start_node + 1)),
                None => Ok((vec![ignotium(word, start_node)], start_node + 1)),
            },
        };

        if let (Some(key), Ok((interps, next_node))) = (cache_key, result.as_ref()) {
            self.word_template_cache
                .insert_if_admitted(word, key, interps, start_node, *next_node)?;
        }

        result
    }

    fn word_template_cache_key(
        &self,
        word: &str,
        case_handling: CaseHandling,
        segmentation: &SegmentationPreset,
    ) -> Option<WordTemplateCacheKey> {
        let case_code = match case_handling {
            CaseHandling::ConditionallyCaseSensitive => 0_u16,
            CaseHandling::StrictlyCaseSensitive => 1,
            CaseHandling::IgnoreCase => 2,
        };
        let aggl_code = match segmentation.aggl() {
            Some(aggl) => option_code(
                Some(aggl),
                &[("strict", 1), ("permissive", 2), ("isolated", 3)],
            )?,
            None => self.default_aggl_cache_code?,
        };
        let praet_code = match segmentation.praet() {
            Some(praet) => option_code(Some(praet), &[("split", 1), ("composite", 2)])?,
            None => self.default_praet_cache_code?,
        };
        let config_key = case_code | ((aggl_code as u16) << 2) | ((praet_code as u16) << 5);
        Some(WordTemplateCacheKey {
            hash: word_template_hash(word, config_key),
            config_key,
        })
    }

    /// Plain single-edge, case-aware lookup of a whole word. Used only when the
    /// dictionary has no segmentation FSA (the real builder always emits one).
    fn lookup_word_interpretations(
        &self,
        word: &str,
        start_node: i32,
        case_handling: CaseHandling,
    ) -> Result<Option<Vec<MorphInterpretation>>> {
        let Some(groups) = self.lookup_encoded_groups(word)? else {
            return Ok(None);
        };
        let mut result = Vec::new();
        for group in groups {
            for_each_case_compatible_interpretation(
                word,
                &group.interpretations,
                case_handling,
                |interp| {
                    result.push(interp.to_morph_interpretation(
                        word,
                        start_node,
                        start_node + 1,
                    )?);
                    Ok(())
                },
            )?;
        }
        Ok((!result.is_empty()).then_some(result))
    }

    /// Port of C++ `handleIgnChunk`: split an unknown chunk into maximal
    /// non-separator runs and individual separator characters, re-analyzing each
    /// part. If the chunk contains no separators at all it stays a single `ign`.
    fn handle_ign_chunk(
        &self,
        word: &str,
        start_node: i32,
        case_handling: CaseHandling,
        segmentation: &SegmentationPreset,
    ) -> Result<(Vec<MorphInterpretation>, i32)> {
        let separators = &self.segmentation_metadata.separators;
        let is_separator = |c: char| separators.binary_search(&(c as u32)).is_ok();

        if !word.chars().any(is_separator) {
            return Ok((vec![ignotium(word, start_node)], start_node + 1));
        }

        let mut results = Vec::new();
        let mut node = start_node;
        let mut run_start = 0usize;
        let mut pending_non_sep: Option<(usize, usize)> = None;

        for (index, ch) in word.char_indices() {
            if is_separator(ch) {
                if let Some((s, e)) = pending_non_sep.take() {
                    let (interps, next) = self.process_one_word(
                        &word[s..e],
                        node,
                        case_handling,
                        segmentation,
                        true,
                    )?;
                    results.extend(interps);
                    node = next;
                }
                let sep_end = index + ch.len_utf8();
                let (interps, next) = self.process_one_word(
                    &word[index..sep_end],
                    node,
                    case_handling,
                    segmentation,
                    true,
                )?;
                results.extend(interps);
                node = next;
                run_start = sep_end;
            } else {
                pending_non_sep = Some((
                    pending_non_sep.map(|(s, _)| s).unwrap_or(run_start),
                    index + ch.len_utf8(),
                ));
            }
        }

        if let Some((s, e)) = pending_non_sep {
            let (interps, next) =
                self.process_one_word(&word[s..e], node, case_handling, segmentation, true)?;
            results.extend(interps);
            node = next;
        }

        Ok((results, node))
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

impl Lexicon for BinaryAnalyzerLexicon {
    fn try_fork(&self) -> Option<Arc<dyn Lexicon>> {
        // Share the immutable dictionary (Arc'd bytes) but start a fresh,
        // uncontended decode cache for this thread's copy.
        let mut forked = self.clone();
        forked.analyzer_decode_cache = SharedAnalyzerGroupDecodeCache::default();
        forked.word_template_cache = SharedWordTemplateCache::default();
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

    fn is_native_analyzer(&self) -> bool {
        true
    }

    fn analyze_native_word(
        &self,
        word: &str,
        start_node: i32,
        case_handling: CaseHandling,
        segmentation: &SegmentationPreset,
    ) -> Result<(Vec<MorphInterpretation>, i32)> {
        self.analyze_native_word(word, start_node, case_handling, segmentation)
    }

    fn analyze_word_interpretations(
        &self,
        orth: &str,
        start_node: i32,
        case_handling: CaseHandling,
        segmentation: &SegmentationPreset,
    ) -> Result<Option<(Vec<MorphInterpretation>, i32)>> {
        self.analyze_word_with_segmentation(orth, start_node, case_handling, segmentation)
    }

    fn lookup_interpretations(
        &self,
        orth: &str,
        start_node: i32,
        end_node: i32,
        result_orth: &str,
        case_handling: CaseHandling,
        segmentation: &SegmentationPreset,
    ) -> Result<Option<Vec<MorphInterpretation>>> {
        if orth == result_orth && self.has_segmentation_transitions(segmentation)? {
            return Ok(None);
        }

        let Some(groups) = self.lookup_encoded_groups(orth)? else {
            return Ok(None);
        };
        let mut result = Vec::new();
        for group in groups {
            for_each_case_compatible_interpretation(
                orth,
                &group.interpretations,
                case_handling,
                |interp| {
                    let mut morph = interp.to_morph_interpretation(orth, start_node, end_node)?;
                    morph.orth = result_orth.to_owned();
                    result.push(morph);
                    Ok(())
                },
            )?;
        }
        Ok((!result.is_empty()).then_some(result))
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct BinaryAnalyzerChunk<'a> {
    pub(super) orth: &'a str,
    pub(super) original_start: usize,
    pub(super) original_end: usize,
    pub(super) shift_orth: bool,
    pub(super) segment_type: u8,
    /// Stable identity of the dictionary interpretations group this chunk came
    /// from: `(fsa_state_offset, group_index_within_payload)`. Mirrors C++
    /// `interpsGroupPtr` so identical edges reached through different paths
    /// dedup, while distinct groups that happen to decode identically (e.g. two
    /// `adv:pos` readings of "jednotonowo") are both kept.
    pub(super) group_id: (usize, usize),
    /// Whether this segment's orthographic case matches the dictionary group's
    /// case patterns (C++ `checkInterpsGroupOrthCasePatterns`). A group matches
    /// when any of its interpretations' orth-case patterns accept the segment's
    /// orth. Drives strict-case rejection and conditional-case weak pruning.
    pub(super) case_matches: bool,
    /// Index into the per-word decode cache. Keeping only an index makes path
    /// clones and graph arena copies plain scalar copies; decoded String/Vec
    /// payloads stay owned by the cache for the duration of processing one word.
    pub(super) interpretations: usize,
}

#[derive(Debug, Clone)]
pub(super) struct BinaryAnalyzerPath<'a> {
    pub(super) chunks: Vec<BinaryAnalyzerChunk<'a>>,
    pub(super) weak: bool,
}

#[derive(Debug)]
pub(super) struct BinaryAnalyzerWordPaths<'a> {
    paths: Vec<BinaryAnalyzerPath<'a>>,
    decode_cache: AnalyzerGroupDecodeCache,
}

impl<'a> BinaryAnalyzerWordPaths<'a> {
    fn empty() -> Self {
        Self {
            paths: Vec::new(),
            decode_cache: AnalyzerGroupDecodeCache::default(),
        }
    }
}

pub(super) fn collect_segmented_analyzer_paths<'a>(
    fsa: BinaryFsa<'_>,
    rules_fsa: &SegmentationRulesFsa<'_>,
    normalized: &NormalizedInput<'a>,
    original: &'a str,
    position: usize,
    segmentation_state: SegmentationState,
    current_path: &mut Vec<BinaryAnalyzerChunk<'a>>,
    paths: &mut Vec<BinaryAnalyzerPath<'a>>,
    decode_cache: &mut AnalyzerGroupDecodeCache,
) -> Result<()> {
    let normalized_text = normalized.as_str();
    if position >= normalized_text.len() {
        return Ok(());
    }

    fsa.for_each_prefix_match_loaded(&normalized_text.as_bytes()[position..], |prefix_match| {
        let end = position
            .checked_add(prefix_match.input_end)
            .ok_or_else(|| Error::invalid_dictionary("normalized prefix offset overflow"))?;
        let Some((chunk_orth, original_start, original_end)) =
            normalized.original_span(original, position, end)
        else {
            return Ok(());
        };
        let at_end = end == normalized_text.len();

        for_each_raw_interps_group(prefix_match.value, |group_index, raw_group| {
            // Check the segmentation rules using only the (cheap) segment type and
            // decode the full interpretations lazily — groups rejected by segrules
            // are never decoded, which avoids a large amount of wasted
            // `String`/`Vec` allocation on rich dictionaries.
            let Some(new_state) = rules_fsa.proceed_to_next_unchecked(
                raw_group.segment_type,
                segmentation_state,
                at_end,
            ) else {
                return Ok(());
            };

            let segment_type = raw_group.segment_type;
            let group_id = (prefix_match.state_offset, group_index);
            let interpretations = decode_cache.get_or_decode(group_id, raw_group)?;
            let case_matches = decode_cache
                .interpretations(interpretations)
                .iter()
                .any(|interp| interp.matches_orth_case(chunk_orth));
            current_path.push(BinaryAnalyzerChunk {
                orth: chunk_orth,
                original_start,
                original_end,
                shift_orth: new_state.shift_orth_from_previous,
                segment_type,
                group_id,
                case_matches,
                interpretations,
            });

            if at_end {
                if new_state.accepting {
                    paths.push(BinaryAnalyzerPath {
                        chunks: current_path.clone(),
                        weak: new_state.weak,
                    });
                }
            } else if !new_state.sink {
                collect_segmented_analyzer_paths(
                    fsa,
                    rules_fsa,
                    normalized,
                    original,
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

/// A single `ign` interpretation spanning `[start_node, start_node + 1]`.
pub(super) fn ignotium(word: &str, start_node: i32) -> MorphInterpretation {
    MorphInterpretation::create_ign(start_node, start_node + 1, word, word)
}

pub(super) fn paths_to_morph_interpretations<'a>(
    mut paths: Vec<BinaryAnalyzerPath<'a>>,
    decode_cache: &AnalyzerGroupDecodeCache,
    start_node: i32,
    case_handling: CaseHandling,
) -> Result<Option<(Vec<MorphInterpretation>, i32)>> {
    if paths.is_empty() {
        return Ok(None);
    }

    // Orthographic-case handling, mirroring C++ `processInterpsGroup`:
    //  * STRICTLY_CASE_SENSITIVE: a segment whose case does not match the
    //    dictionary group is rejected outright, so any path containing one is
    //    dropped before it can enter the graph.
    //  * CONDITIONALLY_CASE_SENSITIVE: such a segment is accepted but makes the
    //    whole path weak (`InflexionGraph::add_path` keeps strong paths over
    //    weak ones), so lowercase "rogalińską" keeps `rogaliński` (case match)
    //    and drops the capitalized proper-noun reading.
    //  * IGNORE_CASE: case is irrelevant.
    if case_handling == CaseHandling::StrictlyCaseSensitive {
        paths.retain(|path| path.chunks.iter().all(|chunk| chunk.case_matches));
        if paths.is_empty() {
            return Ok(None);
        }
    }
    if paths.len() == 1 {
        return single_path_to_morph_interpretations(
            &paths[0],
            decode_cache,
            start_node,
            case_handling,
        );
    }
    if paths.iter().all(path_is_single_graph_edge) {
        return single_edge_paths_to_morph_interpretations(
            &paths,
            decode_cache,
            start_node,
            case_handling,
        );
    }

    let mut graph = InflexionGraph::default();
    for path in &paths {
        let weak = path.weak
            || (case_handling == CaseHandling::ConditionallyCaseSensitive
                && path.chunks.iter().any(|chunk| !chunk.case_matches));
        graph.add_path(path, weak);
    }
    if graph.is_empty() {
        return Ok(None);
    }

    let node_count = graph.finish();
    let mut result = Vec::new();
    for node in 0..graph.node_len() {
        let src = start_node + node as i32;
        for edge_index in 0..graph.edges_at(node) {
            let (group, next_node) = graph.edge(node, edge_index);
            let target = start_node + next_node as i32;
            if group.len() > 1 {
                push_shifted_chunk_interpretations(
                    group,
                    decode_cache,
                    src,
                    target,
                    case_handling,
                    &mut result,
                )?;
            } else {
                push_plain_chunk_interpretations(
                    &group[0],
                    decode_cache,
                    src,
                    target,
                    case_handling,
                    &mut result,
                )?;
            }
        }
    }

    if result.is_empty() {
        Ok(None)
    } else {
        Ok(Some((result, node_count as i32)))
    }
}

pub(super) fn path_is_single_graph_edge(path: &BinaryAnalyzerPath<'_>) -> bool {
    !path.chunks.is_empty()
        && path
            .chunks
            .iter()
            .take(path.chunks.len().saturating_sub(1))
            .all(|chunk| chunk.shift_orth)
}

pub(super) fn path_is_effectively_weak(
    path: &BinaryAnalyzerPath<'_>,
    case_handling: CaseHandling,
) -> bool {
    path.weak
        || (case_handling == CaseHandling::ConditionallyCaseSensitive
            && path.chunks.iter().any(|chunk| !chunk.case_matches))
}

pub(super) fn single_edge_paths_to_morph_interpretations(
    paths: &[BinaryAnalyzerPath<'_>],
    decode_cache: &AnalyzerGroupDecodeCache,
    start_node: i32,
    case_handling: CaseHandling,
) -> Result<Option<(Vec<MorphInterpretation>, i32)>> {
    let has_strong = paths
        .iter()
        .any(|path| !path_is_effectively_weak(path, case_handling));
    let capacity = paths
        .iter()
        .filter(|path| !(has_strong && path_is_effectively_weak(path, case_handling)))
        .flat_map(|path| path.chunks.iter())
        .map(|chunk| decode_cache.interpretations(chunk.interpretations).len())
        .sum();
    let mut result = Vec::with_capacity(capacity);
    for path in paths {
        if has_strong && path_is_effectively_weak(path, case_handling) {
            continue;
        }
        let target = start_node + 1;
        if path.chunks.len() > 1 {
            push_shifted_chunk_interpretations(
                &path.chunks,
                decode_cache,
                start_node,
                target,
                case_handling,
                &mut result,
            )?;
        } else {
            push_plain_chunk_interpretations(
                &path.chunks[0],
                decode_cache,
                start_node,
                target,
                case_handling,
                &mut result,
            )?;
        }
    }

    if result.is_empty() {
        Ok(None)
    } else {
        Ok(Some((result, 1)))
    }
}

pub(super) fn single_path_to_morph_interpretations(
    path: &BinaryAnalyzerPath<'_>,
    decode_cache: &AnalyzerGroupDecodeCache,
    start_node: i32,
    case_handling: CaseHandling,
) -> Result<Option<(Vec<MorphInterpretation>, i32)>> {
    let capacity = path
        .chunks
        .iter()
        .map(|chunk| decode_cache.interpretations(chunk.interpretations).len())
        .sum();
    let mut result = Vec::with_capacity(capacity);
    let mut index = 0usize;
    let mut node = 0i32;
    while index < path.chunks.len() {
        let mut shifted_end = index;
        while shifted_end + 1 < path.chunks.len() && path.chunks[shifted_end].shift_orth {
            shifted_end += 1;
        }

        let src = start_node + node;
        let target = src + 1;
        if shifted_end > index {
            push_shifted_chunk_interpretations(
                &path.chunks[index..=shifted_end],
                decode_cache,
                src,
                target,
                case_handling,
                &mut result,
            )?;
        } else {
            push_plain_chunk_interpretations(
                &path.chunks[index],
                decode_cache,
                src,
                target,
                case_handling,
                &mut result,
            )?;
        }
        node += 1;
        index = shifted_end + 1;
    }

    if result.is_empty() {
        Ok(None)
    } else {
        Ok(Some((result, node)))
    }
}

pub(super) fn push_plain_chunk_interpretations(
    chunk: &BinaryAnalyzerChunk,
    decode_cache: &AnalyzerGroupDecodeCache,
    start_node: i32,
    end_node: i32,
    case_handling: CaseHandling,
    result: &mut Vec<MorphInterpretation>,
) -> Result<()> {
    let orth_context = AnalyzerOrthContext::new(chunk.orth);
    for_each_case_compatible_interpretation(
        chunk.orth,
        decode_cache.interpretations(chunk.interpretations),
        case_handling,
        |interp| {
            result.push(interp.to_morph_interpretation_in_context(
                &orth_context,
                start_node,
                end_node,
            )?);
            Ok(())
        },
    )?;
    Ok(())
}

pub(super) fn push_shifted_chunk_interpretations(
    chunks: &[BinaryAnalyzerChunk],
    decode_cache: &AnalyzerGroupDecodeCache,
    start_node: i32,
    end_node: i32,
    case_handling: CaseHandling,
    result: &mut Vec<MorphInterpretation>,
) -> Result<()> {
    let Some((current, prefixes)) = chunks.split_last() else {
        return Ok(());
    };
    let orth = chunks.iter().map(|chunk| chunk.orth).collect::<String>();
    let mut lemma_prefix = String::new();
    for prefix in prefixes {
        let Some(prefix_interp) = first_case_compatible_interpretation(
            prefix.orth,
            decode_cache.interpretations(prefix.interpretations),
            case_handling,
        ) else {
            return Ok(());
        };
        lemma_prefix.push_str(&decode_analyzer_prefix_lemma_for_form(
            prefix.orth,
            &prefix_interp.form,
        )?);
    }

    let current_codepoints = current.orth.chars().count();
    let orth_context = AnalyzerOrthContext::new(&orth);
    let prefix_codepoints = orth_context.original_codepoints_len - current_codepoints;
    for_each_case_compatible_interpretation(
        current.orth,
        decode_cache.interpretations(current.interpretations),
        case_handling,
        |interp| {
            let mut form = interp.form.clone();
            if !interp.orth_case_pattern.is_empty() && !form.case_pattern.is_empty() {
                form.case_pattern = form.case_pattern.shifted_by_lower_prefix(prefix_codepoints);
            }
            let mut lemma = lemma_prefix.clone();
            lemma.push_str(&decode_analyzer_lemma_with_prefix_context_len(
                &orth,
                current_codepoints,
                orth_context.lowercase_codepoints_len,
                &form,
            )?);
            result.push(MorphInterpretation {
                start_node,
                end_node,
                orth: orth.clone(),
                lemma,
                tag_id: interp.tag_id,
                name_id: interp.name_id,
                labels_id: interp.labels_id,
            });
            Ok(())
        },
    )?;
    Ok(())
}

pub(super) fn for_each_case_compatible_interpretation<I, F>(
    orth: &str,
    interpretations: &[I],
    case_handling: CaseHandling,
    mut visit: F,
) -> Result<()>
where
    I: AnalyzerInterpretationView,
    F: FnMut(&I) -> Result<()>,
{
    if case_handling == CaseHandling::IgnoreCase {
        for interp in interpretations {
            visit(interp)?;
        }
        return Ok(());
    }

    let mut strict_seen = false;
    for interp in interpretations {
        if interp.matches_orth_case(orth) {
            strict_seen = true;
            visit(interp)?;
        }
    }
    if !strict_seen && case_handling == CaseHandling::ConditionallyCaseSensitive {
        for interp in interpretations {
            visit(interp)?;
        }
    }
    Ok(())
}

pub(super) fn first_case_compatible_interpretation<'a, I>(
    orth: &str,
    interpretations: &'a [I],
    case_handling: CaseHandling,
) -> Option<&'a I>
where
    I: AnalyzerInterpretationView,
{
    if case_handling == CaseHandling::IgnoreCase {
        return interpretations.first();
    }

    interpretations
        .iter()
        .find(|interp| interp.matches_orth_case(orth))
        .or_else(|| {
            (case_handling == CaseHandling::ConditionallyCaseSensitive)
                .then(|| interpretations.first())
                .flatten()
        })
}
