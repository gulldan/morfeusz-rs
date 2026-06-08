use std::sync::Arc;

use crate::dictionary::entry_to_interpretation;
use crate::{
    Config, Dictionary, Error, IdResolver, Lexicon, MorphInterpretation, NumberingScope, Result,
    WhitespaceHandling,
};

#[derive(Clone)]
pub struct Engine {
    lexicon: Arc<dyn Lexicon>,
    config: Config,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine")
            .field("lexicon_id", &self.lexicon.id())
            .field("config", &self.config)
            .finish()
    }
}

impl Default for Engine {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl Engine {
    pub fn builder() -> EngineBuilder {
        EngineBuilder::default()
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn resolver(&self) -> &IdResolver {
        self.lexicon.resolver()
    }

    pub fn lexicon_id(&self) -> &str {
        self.lexicon.id()
    }

    pub fn lexicon_copyright(&self) -> &str {
        self.lexicon.copyright()
    }

    /// The loaded dictionary's default `aggl` option, if it defines one.
    pub fn default_aggl(&self) -> Option<&str> {
        self.lexicon.default_aggl()
    }

    /// The loaded dictionary's default `praet` option, if it defines one.
    pub fn default_praet(&self) -> Option<&str> {
        self.lexicon.default_praet()
    }

    pub fn available_aggl_options(&self) -> Vec<String> {
        self.lexicon.available_aggl_options()
    }

    pub fn available_praet_options(&self) -> Vec<String> {
        self.lexicon.available_praet_options()
    }

    pub fn validate_segmentation(
        &self,
        segmentation: &crate::SegmentationPreset,
        option: &str,
        value: &str,
    ) -> Result<()> {
        self.lexicon
            .validate_segmentation(segmentation, option, value)
    }

    pub fn with_config(&self, config: Config) -> Self {
        Self {
            lexicon: Arc::clone(&self.lexicon),
            config,
        }
    }

    /// An independent engine for a dedicated thread: it shares the immutable
    /// dictionary but gets its own decode caches, so concurrent analysis does
    /// not contend on a shared cache lock. Falls back to sharing the lexicon
    /// `Arc` for lexicons that keep no per-instance state.
    pub fn fork(&self) -> Self {
        Self {
            lexicon: self
                .lexicon
                .try_fork()
                .unwrap_or_else(|| Arc::clone(&self.lexicon)),
            config: self.config.clone(),
        }
    }

    pub fn analyze(&self, text: &str) -> Result<Vec<MorphInterpretation>> {
        self.analyze_from_node(text, 0).map(|(result, _)| result)
    }

    pub fn session(&self) -> Session {
        Session {
            engine: self.clone(),
            next_node: 0,
        }
    }

    pub fn generate(&self, lemma: &str) -> Result<Vec<MorphInterpretation>> {
        ensure_valid_generate_input(lemma)?;
        // Synthesis is entirely the lexicon's responsibility: the binary
        // generator derives `dig`/`romandig` (and everything else) from its FSA
        // and segmentation rules, and the synthetic TSV lexicon applies its own
        // localized heuristics. The engine must NOT special-case digits here,
        // or it would emit `dig` for dictionaries that have no digit entries
        // (C++ returns `ign` in that case).
        let lexical = self
            .lexicon
            .synthesize_interpretations(lemma, self.config.segmentation())?;
        if !lexical.is_empty() {
            return Ok(lexical);
        }
        Ok(vec![MorphInterpretation::create_ign(0, 0, lemma, lemma)])
    }

    pub fn generate_by_tag_id(&self, lemma: &str, tag_id: i32) -> Result<Vec<MorphInterpretation>> {
        self.resolver()
            .tag(tag_id)
            .ok_or_else(|| Error::InvalidArgument(format!("Invalid tag id: {tag_id}")))?;
        Ok(self
            .generate(lemma)?
            .into_iter()
            .filter(|interp| interp.tag_id == tag_id)
            .collect())
    }

    pub(crate) fn analyze_from_node(
        &self,
        text: &str,
        start_node: i32,
    ) -> Result<(Vec<MorphInterpretation>, i32)> {
        if self.lexicon.is_native_analyzer() {
            return self.analyze_native(text, start_node);
        }
        let segments = tokenize(text);
        match self.config.whitespace() {
            WhitespaceHandling::Skip => self.analyze_skip_whitespace(&segments, start_node),
            WhitespaceHandling::Keep => self.analyze_keep_whitespace(&segments, start_node),
            WhitespaceHandling::Append => self.analyze_append_whitespace(&segments, start_node),
        }
    }

    /// Native analysis path for binary FSA lexicons: split on whitespace only
    /// and route each word through the faithful `processOneWord` port. Mirrors
    /// C++ `MorfeuszImpl::processOneWord`'s whitespace handling.
    fn analyze_native(
        &self,
        text: &str,
        start_node: i32,
    ) -> Result<(Vec<MorphInterpretation>, i32)> {
        let case = self.config.case_handling();
        let seg = self.config.segmentation();
        let whitespace = self.config.whitespace();

        if whitespace == WhitespaceHandling::Skip && !contains_morfeusz_whitespace(text) {
            return self
                .lexicon
                .analyze_native_word(text, start_node, case, seg);
        }

        let runs = whitespace_runs(text);
        let mut result = Vec::new();
        let mut node = start_node;
        let mut index = 0;
        let mut seen_word = false;
        while index < runs.len() {
            let run = &runs[index];
            if run.is_whitespace {
                match whitespace {
                    WhitespaceHandling::Keep => {
                        result.push(MorphInterpretation::create_whitespace(
                            node,
                            node + 1,
                            run.text.clone(),
                        ));
                        node += 1;
                    }
                    WhitespaceHandling::Skip | WhitespaceHandling::Append => {}
                }
                index += 1;
                continue;
            }

            // Non-whitespace word. In APPEND mode the surrounding whitespace is
            // glued to the word's orth: leading whitespace only onto the very
            // first word (inter-word whitespace is consumed as the *preceding*
            // word's trailing), trailing whitespace onto every word. Matches
            // C++ chunk-bounds handling.
            let leading = if whitespace == WhitespaceHandling::Append
                && !seen_word
                && index > 0
                && runs[index - 1].is_whitespace
            {
                Some(runs[index - 1].text.clone())
            } else {
                None
            };
            let trailing = if whitespace == WhitespaceHandling::Append {
                runs.get(index + 1)
                    .filter(|next| next.is_whitespace)
                    .map(|next| next.text.clone())
            } else {
                None
            };
            seen_word = true;

            let (mut interps, next) = self
                .lexicon
                .analyze_native_word(&run.text, node, case, seg)?;
            if leading.is_some() || trailing.is_some() {
                append_whitespace_to_word(&mut interps, leading.as_deref(), trailing.as_deref());
            }
            result.extend(interps);
            node = next;
            index += 1;
        }

        Ok((result, node))
    }

    fn analyze_skip_whitespace(
        &self,
        segments: &[Segment],
        start_node: i32,
    ) -> Result<(Vec<MorphInterpretation>, i32)> {
        let mut result = Vec::new();
        let mut node = start_node;

        let mut index = 0;
        while index < segments.len() {
            if segments[index].is_whitespace {
                index += 1;
                continue;
            }
            let run_end = non_whitespace_run_end(segments, index);
            node =
                self.push_segment_run_interpretations(&mut result, segments, index, run_end, node)?;
            index = run_end;
        }
        Ok((result, node))
    }

    fn analyze_keep_whitespace(
        &self,
        segments: &[Segment],
        start_node: i32,
    ) -> Result<(Vec<MorphInterpretation>, i32)> {
        let mut result = Vec::new();
        let mut node = start_node;
        let mut index = 0;
        while index < segments.len() {
            let segment = &segments[index];
            if segment.is_whitespace {
                result.push(MorphInterpretation::create_whitespace(
                    node,
                    node + 1,
                    segment.text.clone(),
                ));
                node += 1;
                index += 1;
            } else {
                let run_end = non_whitespace_run_end(segments, index);
                node = self.push_segment_run_interpretations(
                    &mut result,
                    segments,
                    index,
                    run_end,
                    node,
                )?;
                index = run_end;
            }
        }
        Ok((result, node))
    }

    fn analyze_append_whitespace(
        &self,
        segments: &[Segment],
        start_node: i32,
    ) -> Result<(Vec<MorphInterpretation>, i32)> {
        let mut result = Vec::new();
        let mut node = start_node;
        let mut index = 0;

        while index < segments.len() {
            let mut leading = String::new();
            while let Some(segment) = segments.get(index) {
                if !segment.is_whitespace {
                    break;
                }
                leading.push_str(&segment.text);
                index += 1;
            }

            let Some(token) = segments.get(index) else {
                break;
            };
            index += 1;

            let mut trailing = String::new();
            while let Some(segment) = segments.get(index) {
                if !segment.is_whitespace {
                    break;
                }
                trailing.push_str(&segment.text);
                index += 1;
            }

            let orth = format!("{leading}{}{trailing}", token.text);
            node +=
                self.push_segment_interpretations(&mut result, token.text.as_str(), orth, node)?;
        }

        Ok((result, node))
    }

    fn push_token_interpretations(
        &self,
        result: &mut Vec<MorphInterpretation>,
        lookup_orth: &str,
        result_orth: String,
        node: i32,
    ) -> Result<i32> {
        if let Some(digit_interp) =
            self.digit_interpretation(lookup_orth, node, node + 1, result_orth.clone())
        {
            result.push(digit_interp);
            return Ok(1);
        }
        if let Some(roman_interp) =
            self.roman_interpretation(lookup_orth, node, node + 1, result_orth.clone())
        {
            result.push(roman_interp);
            return Ok(1);
        }
        if result_orth == lookup_orth {
            if let Some((interps, nodes)) = self.lexicon.analyze_word_interpretations(
                lookup_orth,
                node,
                self.config.case_handling(),
                self.config.segmentation(),
            )? {
                result.extend(interps);
                return Ok(nodes);
            }
        }
        if let Some(interps) = self.lexicon.lookup_interpretations(
            lookup_orth,
            node,
            node + 1,
            &result_orth,
            self.config.case_handling(),
            self.config.segmentation(),
        )? {
            result.extend(interps);
        } else {
            result.push(MorphInterpretation::create_ign(
                node,
                node + 1,
                result_orth,
                lookup_orth,
            ));
        }
        Ok(1)
    }

    fn push_segment_interpretations(
        &self,
        result: &mut Vec<MorphInterpretation>,
        lookup_orth: &str,
        result_orth: String,
        node: i32,
    ) -> Result<i32> {
        if result_orth == lookup_orth {
            if let Some((interps, nodes)) = self.lexicon.analyze_word_interpretations(
                lookup_orth,
                node,
                self.config.case_handling(),
                self.config.segmentation(),
            )? {
                result.extend(interps);
                return Ok(nodes);
            }
            if let Some((interps, nodes)) = self.analyze_in_word_graph(lookup_orth, node) {
                result.extend(interps);
                return Ok(nodes);
            }
        }
        self.push_token_interpretations(result, lookup_orth, result_orth, node)
    }

    fn push_segment_run_interpretations(
        &self,
        result: &mut Vec<MorphInterpretation>,
        segments: &[Segment],
        start: usize,
        end: usize,
        mut node: i32,
    ) -> Result<i32> {
        let mut index = start;
        while index < end {
            if let Some((next_index, orth)) = self.longest_known_join(segments, index, end) {
                node +=
                    self.push_segment_interpretations(result, orth.as_str(), orth.clone(), node)?;
                index = next_index;
            } else if let Some(next_index) = self.unknown_hyphenated_span_end(segments, index, end)
            {
                let orth = join_segments(segments, index, next_index);
                node +=
                    self.push_token_interpretations(result, orth.as_str(), orth.clone(), node)?;
                index = next_index;
            } else {
                let segment = &segments[index];
                node += self.push_segment_interpretations(
                    result,
                    segment.text.as_str(),
                    segment.text.clone(),
                    node,
                )?;
                index += 1;
            }
        }
        Ok(node)
    }

    fn longest_known_join(
        &self,
        segments: &[Segment],
        start: usize,
        end: usize,
    ) -> Option<(usize, String)> {
        let mut joined = String::new();
        let mut best = None;
        for (offset, segment) in segments[start..end].iter().enumerate() {
            joined.push_str(&segment.text);
            let next_index = start + offset + 1;
            if offset > 0 && self.lexicon.lookup(&joined).is_some() {
                best = Some((next_index, joined.clone()));
            }
        }
        best
    }

    fn unknown_hyphenated_span_end(
        &self,
        segments: &[Segment],
        start: usize,
        end: usize,
    ) -> Option<usize> {
        let span_end = hyphenated_word_span_end(segments, start, end)?;
        if self.can_decompose_hyphenated_span(segments, start, span_end) {
            None
        } else {
            Some(span_end)
        }
    }

    fn can_decompose_hyphenated_span(
        &self,
        segments: &[Segment],
        start: usize,
        end: usize,
    ) -> bool {
        if end - start != 3 || segments[start + 1].text != "-" {
            return false;
        }
        let Some(left_entries) = self.lexicon.lookup(&segments[start].text) else {
            return false;
        };
        let Some(right_entries) = self.lexicon.lookup(&segments[start + 2].text) else {
            return false;
        };
        left_entries
            .iter()
            .any(|entry| self.resolver().tag(entry.tag_id) == Some("adja"))
            && right_entries.iter().any(|entry| {
                self.resolver()
                    .tag(entry.tag_id)
                    .map(|tag| tag.starts_with("adj:"))
                    .unwrap_or(false)
            })
    }

    fn analyze_in_word_graph(
        &self,
        token: &str,
        start_node: i32,
    ) -> Option<(Vec<MorphInterpretation>, i32)> {
        let boundaries = token
            .char_indices()
            .map(|(index, _)| index)
            .chain(std::iter::once(token.len()))
            .collect::<Vec<_>>();
        let mut matches = Vec::new();
        let mut graph_boundaries = vec![0, token.len()];
        let mut has_aglt_suffix = false;

        for (start_pos, start) in boundaries.iter().copied().enumerate() {
            for end in boundaries.iter().copied().skip(start_pos + 1) {
                let slice = &token[start..end];
                let Some(entries) = self.lexicon.lookup(slice) else {
                    continue;
                };
                if start > 0
                    && entries.iter().any(|entry| {
                        self.resolver()
                            .tag(entry.tag_id)
                            .map(|tag| tag.starts_with("aglt:"))
                            .unwrap_or(false)
                    })
                {
                    has_aglt_suffix = true;
                }
                graph_boundaries.push(start);
                graph_boundaries.push(end);
                matches.push((start, end, slice.to_owned(), entries.to_vec()));
            }
        }

        if !has_aglt_suffix {
            return None;
        }

        graph_boundaries.sort_unstable();
        graph_boundaries.dedup();
        matches.sort_by_key(|(start, end, _, _)| {
            (
                graph_boundary_index(&graph_boundaries, *start).unwrap_or(usize::MAX),
                graph_boundary_index(&graph_boundaries, *end).unwrap_or(usize::MAX),
            )
        });

        let mut result = Vec::new();
        for (start, end, orth, entries) in matches {
            let Some(local_start) = graph_boundary_index(&graph_boundaries, start) else {
                continue;
            };
            let Some(local_end) = graph_boundary_index(&graph_boundaries, end) else {
                continue;
            };
            let local_start = local_start as i32;
            let local_end = local_end as i32;
            result.extend(entries.iter().map(|entry| {
                entry_to_interpretation(
                    entry,
                    start_node + local_start,
                    start_node + local_end,
                    orth.clone(),
                )
            }));
        }

        Some((result, graph_boundaries.len() as i32 - 1))
    }

    fn digit_interpretation(
        &self,
        text: &str,
        start_node: i32,
        end_node: i32,
        orth: String,
    ) -> Option<MorphInterpretation> {
        if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        let tag_id = self.resolver().tag_id("dig").ok()?;
        Some(MorphInterpretation {
            start_node,
            end_node,
            orth,
            lemma: text.to_owned(),
            tag_id,
            name_id: 0,
            labels_id: 0,
        })
    }

    fn roman_interpretation(
        &self,
        text: &str,
        start_node: i32,
        end_node: i32,
        orth: String,
    ) -> Option<MorphInterpretation> {
        let normalized = text.to_ascii_uppercase();
        if !is_valid_roman_numeral(&normalized) {
            return None;
        }
        let tag_id = self.resolver().tag_id("romandig").ok()?;
        Some(MorphInterpretation {
            start_node,
            end_node,
            orth,
            lemma: normalized,
            tag_id,
            name_id: 0,
            labels_id: 0,
        })
    }
}

fn graph_boundary_index(boundaries: &[usize], value: usize) -> Option<usize> {
    boundaries.binary_search(&value).ok()
}

#[derive(Debug, Clone)]
pub struct Session {
    engine: Engine,
    next_node: i32,
}

impl Session {
    pub fn analyze(&mut self, text: &str) -> Result<Vec<MorphInterpretation>> {
        let start_node = match self.engine.config().numbering() {
            NumberingScope::Separate => 0,
            NumberingScope::Continuous => self.next_node,
        };
        let (result, next_node) = self.engine.analyze_from_node(text, start_node)?;
        if self.engine.config().numbering() == NumberingScope::Continuous {
            self.next_node = next_node;
        }
        Ok(result)
    }

    pub fn reset_numbering(&mut self) {
        self.next_node = 0;
    }
}

#[derive(Clone)]
pub struct EngineBuilder {
    lexicon: Arc<dyn Lexicon>,
    config: Config,
}

impl Default for EngineBuilder {
    fn default() -> Self {
        Self {
            lexicon: Arc::new(Dictionary::empty()),
            config: Config::default(),
        }
    }
}

impl EngineBuilder {
    pub fn config(mut self, config: Config) -> Self {
        self.config = config;
        self
    }

    pub fn lexicon(mut self, lexicon: impl Lexicon + 'static) -> Self {
        self.lexicon = Arc::new(lexicon);
        self
    }

    pub fn build(self) -> Engine {
        Engine {
            lexicon: self.lexicon,
            config: self.config,
        }
    }
}

fn ensure_valid_generate_input(lemma: &str) -> Result<()> {
    if contains_morfeusz_whitespace(lemma) {
        Err(Error::InvalidArgument(
            "Lemma parameter contains whitespace.".to_owned(),
        ))
    } else {
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Segment {
    text: String,
    is_whitespace: bool,
}

/// Splits text into maximal runs of whitespace and non-whitespace, used by the
/// native (binary FSA) analysis path. Unlike [`tokenize`], non-whitespace runs
/// are kept whole — word-internal segmentation is the FSA's job.
fn whitespace_runs(text: &str) -> Vec<Segment> {
    let mut runs = Vec::new();
    let mut iter = text.char_indices().peekable();
    while let Some((start, ch)) = iter.next() {
        let is_whitespace = is_morfeusz_whitespace(ch);
        let mut end = start + ch.len_utf8();
        while let Some((next_index, next_ch)) = iter.peek().copied() {
            if is_morfeusz_whitespace(next_ch) != is_whitespace {
                break;
            }
            iter.next();
            end = next_index + next_ch.len_utf8();
        }
        runs.push(Segment {
            text: text[start..end].to_owned(),
            is_whitespace,
        });
    }
    runs
}

/// Glues whitespace onto a word's interpretations for APPEND mode: leading onto
/// every interpretation on the first edge, trailing onto every interpretation on
/// the last edge (matching C++ where chunk bounds extend the first/last
/// segment's orth).
fn append_whitespace_to_word(
    interps: &mut [MorphInterpretation],
    leading: Option<&str>,
    trailing: Option<&str>,
) {
    if interps.is_empty() {
        return;
    }
    if let Some(leading) = leading {
        let min_start = interps.iter().map(|i| i.start_node).min().unwrap();
        for interp in interps.iter_mut().filter(|i| i.start_node == min_start) {
            interp.orth = format!("{leading}{}", interp.orth);
        }
    }
    if let Some(trailing) = trailing {
        let max_end = interps.iter().map(|i| i.end_node).max().unwrap();
        for interp in interps.iter_mut().filter(|i| i.end_node == max_end) {
            interp.orth = format!("{}{trailing}", interp.orth);
        }
    }
}

fn tokenize(text: &str) -> Vec<Segment> {
    let mut segments = Vec::new();
    let mut iter = text.char_indices().peekable();

    while let Some((start, ch)) = iter.next() {
        if is_morfeusz_whitespace(ch) {
            let mut end = start + ch.len_utf8();
            while let Some((next_index, next_ch)) = iter.peek().copied() {
                if !is_morfeusz_whitespace(next_ch) {
                    break;
                }
                iter.next();
                end = next_index + next_ch.len_utf8();
            }
            segments.push(Segment {
                text: text[start..end].to_owned(),
                is_whitespace: true,
            });
            continue;
        }

        if is_word_char(ch) {
            let mut end = start + ch.len_utf8();
            while let Some((next_index, next_ch)) = iter.peek().copied() {
                if !is_word_char(next_ch) {
                    break;
                }
                iter.next();
                end = next_index + next_ch.len_utf8();
            }
            segments.push(Segment {
                text: text[start..end].to_owned(),
                is_whitespace: false,
            });
        } else {
            segments.push(Segment {
                text: ch.to_string(),
                is_whitespace: false,
            });
        }
    }

    segments
}

fn non_whitespace_run_end(segments: &[Segment], start: usize) -> usize {
    segments[start..]
        .iter()
        .position(|segment| segment.is_whitespace)
        .map(|offset| start + offset)
        .unwrap_or(segments.len())
}

fn hyphenated_word_span_end(segments: &[Segment], start: usize, end: usize) -> Option<usize> {
    if !is_word_segment(&segments[start]) {
        return None;
    }

    let mut index = start + 1;
    let mut saw_hyphen = false;
    while index + 1 < end && segments[index].text == "-" && is_word_segment(&segments[index + 1]) {
        saw_hyphen = true;
        index += 2;
    }

    saw_hyphen.then_some(index)
}

fn is_word_segment(segment: &Segment) -> bool {
    segment.text.chars().all(is_word_char)
}

fn join_segments(segments: &[Segment], start: usize, end: usize) -> String {
    segments[start..end]
        .iter()
        .map(|segment| segment.text.as_str())
        .collect()
}

fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

fn contains_morfeusz_whitespace(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if is_morfeusz_ascii_whitespace(byte) {
            return true;
        }
        if byte < 0x80 {
            index += 1;
            continue;
        }
        match byte {
            0xC2 if matches!(bytes.get(index + 1), Some(0x85 | 0xA0)) => return true,
            0xE1 if matches!(
                (bytes.get(index + 1), bytes.get(index + 2)),
                (Some(0x9A), Some(0x80)) | (Some(0xA0), Some(0x8E))
            ) =>
            {
                return true;
            }
            0xE2 if matches!(
                (bytes.get(index + 1), bytes.get(index + 2)),
                (Some(0x80), Some(0x80..=0x8B | 0xA8 | 0xA9 | 0xAF))
                    | (Some(0x81), Some(0x9F | 0xA0))
            ) =>
            {
                return true;
            }
            0xE3 if matches!(
                (bytes.get(index + 1), bytes.get(index + 2)),
                (Some(0x80), Some(0x80))
            ) =>
            {
                return true;
            }
            _ => {}
        }
        index += utf8_char_width(byte);
    }
    false
}

fn is_morfeusz_whitespace(ch: char) -> bool {
    matches!(
        ch as u32,
        0x0000
            | 0x0009..=0x000D
            | 0x001C..=0x0020
            | 0x0085
            | 0x00A0
            | 0x1680
            | 0x180E
            | 0x2000..=0x200B
            | 0x2028
            | 0x2029
            | 0x202F
            | 0x205F
            | 0x2060
            | 0x3000
    )
}

fn is_morfeusz_ascii_whitespace(byte: u8) -> bool {
    matches!(byte, 0x00 | 0x09..=0x0D | 0x1C..=0x20)
}

fn utf8_char_width(byte: u8) -> usize {
    match byte {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        _ => 4,
    }
}

fn is_valid_roman_numeral(value: &str) -> bool {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| matches!(byte, b'I' | b'V' | b'X' | b'L' | b'C' | b'D' | b'M'))
    {
        return false;
    }

    let bytes = value.as_bytes();
    let mut index = consume_repeated(bytes, 0, b'M', 3);
    index = consume_roman_rank(bytes, index, b'C', b'D', b'M');
    index = consume_roman_rank(bytes, index, b'X', b'L', b'C');
    index = consume_roman_rank(bytes, index, b'I', b'V', b'X');
    index == bytes.len()
}

fn consume_roman_rank(bytes: &[u8], index: usize, one: u8, five: u8, ten: u8) -> usize {
    if bytes.get(index) == Some(&one) && bytes.get(index + 1) == Some(&ten) {
        return index + 2;
    }
    if bytes.get(index) == Some(&one) && bytes.get(index + 1) == Some(&five) {
        return index + 2;
    }
    let mut index = index;
    if bytes.get(index) == Some(&five) {
        index += 1;
    }
    consume_repeated(bytes, index, one, 3)
}

fn consume_repeated(bytes: &[u8], mut index: usize, byte: u8, max: usize) -> usize {
    let mut consumed = 0;
    while consumed < max && bytes.get(index) == Some(&byte) {
        index += 1;
        consumed += 1;
    }
    index
}

#[cfg(test)]
mod tests {
    use super::{contains_morfeusz_whitespace, is_morfeusz_whitespace};

    #[test]
    fn byte_whitespace_scan_matches_cpp_table_predicate() {
        assert!(!contains_morfeusz_whitespace("zażółć-gęślą-jaźń"));
        assert!(!contains_morfeusz_whitespace("漢字"));

        let whitespace_codepoints = [
            0x0000, 0x0009, 0x000A, 0x000B, 0x000C, 0x000D, 0x001C, 0x001D, 0x001E, 0x001F, 0x0020,
            0x0085, 0x00A0, 0x1680, 0x180E, 0x2000, 0x2001, 0x2002, 0x2003, 0x2004, 0x2005, 0x2006,
            0x2007, 0x2008, 0x2009, 0x200A, 0x200B, 0x2028, 0x2029, 0x202F, 0x205F, 0x2060, 0x3000,
        ];

        for codepoint in whitespace_codepoints {
            let ch = char::from_u32(codepoint).unwrap();
            let value = format!("a{ch}b");
            assert!(
                is_morfeusz_whitespace(ch),
                "predicate missed U+{codepoint:04X}"
            );
            assert!(
                contains_morfeusz_whitespace(&value),
                "byte scan missed U+{codepoint:04X}"
            );
        }
    }
}
