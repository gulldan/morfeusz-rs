use std::sync::Arc;

use crate::dictionary::entry_to_interpretation;
use crate::{
    CaseHandling, DictionaryEntry, IdResolver, MorphInterpretation, Result, SegmentationPreset,
};

pub trait Lexicon: Send + Sync {
    fn id(&self) -> &str;
    fn copyright(&self) -> &str;
    fn resolver(&self) -> &IdResolver;
    fn lookup(&self, orth: &str) -> Option<&[DictionaryEntry]>;
    fn synthesize(&self, lemma: &str) -> Option<&[DictionaryEntry]>;

    /// Produce an independent copy of this lexicon that shares the immutable
    /// dictionary data (via `Arc`) but starts with its own empty decode caches.
    /// Used to give each worker thread a contention-free analyzer: the shared
    /// per-process decode cache is a `Mutex` hot spot that makes naive
    /// multi-threading scale *negatively*, whereas per-thread caches keep their
    /// locks uncontended. Returns `None` for lexicons with no mutable
    /// per-instance state, where sharing the original `Arc` is equivalent.
    fn try_fork(&self) -> Option<Arc<dyn Lexicon>> {
        None
    }

    /// The dictionary's own default `aggl` option, used when the caller has not
    /// explicitly chosen one. `None` for lexicons without segmentation metadata.
    fn default_aggl(&self) -> Option<&str> {
        None
    }

    /// The dictionary's own default `praet` option, used when the caller has not
    /// explicitly chosen one. `None` for lexicons without segmentation metadata.
    fn default_praet(&self) -> Option<&str> {
        None
    }

    fn available_aggl_options(&self) -> Vec<String> {
        vec![
            "strict".to_owned(),
            "permissive".to_owned(),
            "isolated".to_owned(),
        ]
    }

    fn available_praet_options(&self) -> Vec<String> {
        vec!["split".to_owned(), "composite".to_owned()]
    }

    fn validate_segmentation(
        &self,
        _segmentation: &SegmentationPreset,
        _option: &str,
        _value: &str,
    ) -> Result<()> {
        Ok(())
    }

    /// Whether this lexicon performs faithful native whole-word analysis (the
    /// binary FSA path). When true, the engine routes each whitespace-delimited
    /// word through [`Lexicon::analyze_native_word`] — the real recursive
    /// FSA+segrules algorithm with ign separator splitting — instead of the
    /// heuristic tokenizer used for the synthetic TSV path.
    fn is_native_analyzer(&self) -> bool {
        false
    }

    /// Analyze one whitespace-delimited word, returning its interpretations and
    /// the next free node number. Only meaningful when
    /// [`Lexicon::is_native_analyzer`] is true.
    fn analyze_native_word(
        &self,
        _word: &str,
        start_node: i32,
        _case_handling: CaseHandling,
        _segmentation: &SegmentationPreset,
    ) -> Result<(Vec<MorphInterpretation>, i32)> {
        Ok((Vec::new(), start_node))
    }

    fn analyze_word_interpretations(
        &self,
        _orth: &str,
        _start_node: i32,
        _case_handling: CaseHandling,
        _segmentation: &SegmentationPreset,
    ) -> Result<Option<(Vec<MorphInterpretation>, i32)>> {
        Ok(None)
    }

    fn lookup_interpretations(
        &self,
        orth: &str,
        start_node: i32,
        end_node: i32,
        result_orth: &str,
        _case_handling: CaseHandling,
        _segmentation: &SegmentationPreset,
    ) -> Result<Option<Vec<MorphInterpretation>>> {
        Ok(self.lookup(orth).map(|entries| {
            entries
                .iter()
                .map(|entry| {
                    entry_to_interpretation(entry, start_node, end_node, result_orth.to_owned())
                })
                .collect()
        }))
    }

    fn synthesize_interpretations(
        &self,
        lemma: &str,
        _segmentation: &SegmentationPreset,
    ) -> Result<Vec<MorphInterpretation>> {
        Ok(self
            .synthesize(lemma)
            .unwrap_or(&[])
            .iter()
            .map(|entry| entry_to_interpretation(entry, 0, 0, entry.orth.clone()))
            .collect())
    }
}
