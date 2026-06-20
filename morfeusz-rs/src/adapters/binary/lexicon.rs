use super::*;

#[derive(Debug, Clone)]
pub struct BinaryLexicon {
    analyzer: Option<BinaryAnalyzerLexicon>,
    generator: Option<BinaryGeneratorLexicon>,
    id: String,
    copyright: String,
    resolver: Arc<IdResolver>,
}

impl BinaryLexicon {
    pub fn from_paths(
        analyzer_path: Option<impl AsRef<Path>>,
        generator_path: Option<impl AsRef<Path>>,
    ) -> Result<Self> {
        let analyzer = analyzer_path
            .map(BinaryAnalyzerLexicon::from_path)
            .transpose()?;
        let generator = generator_path
            .map(BinaryGeneratorLexicon::from_path)
            .transpose()?;
        Self::new(analyzer, generator)
    }

    pub fn new(
        analyzer: Option<BinaryAnalyzerLexicon>,
        generator: Option<BinaryGeneratorLexicon>,
    ) -> Result<Self> {
        let Some(primary) = analyzer
            .as_ref()
            .map(|lexicon| {
                (
                    lexicon.id().to_owned(),
                    lexicon.copyright().to_owned(),
                    lexicon.resolver().clone(),
                )
            })
            .or_else(|| {
                generator.as_ref().map(|lexicon| {
                    (
                        lexicon.id().to_owned(),
                        lexicon.copyright().to_owned(),
                        lexicon.resolver().clone(),
                    )
                })
            })
        else {
            return Err(Error::invalid_argument(
                "binary lexicon requires analyzer or generator dictionary",
            ));
        };

        Ok(Self {
            analyzer,
            generator,
            id: primary.0,
            copyright: primary.1,
            resolver: Arc::new(primary.2),
        })
    }
}

impl Lexicon for BinaryLexicon {
    fn try_fork(&self) -> Option<Arc<dyn Lexicon>> {
        let mut forked = self.clone();
        if let Some(analyzer) = &mut forked.analyzer {
            analyzer.analyzer_decode_cache = SharedAnalyzerGroupDecodeCache::default();
            analyzer.word_template_cache = SharedWordTemplateCache::default();
        }
        if let Some(generator) = &mut forked.generator {
            generator.generator_decode_cache = SharedGeneratorGroupDecodeCache::default();
        }
        Some(Arc::new(forked))
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn copyright(&self) -> &str {
        &self.copyright
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
        match (&self.analyzer, &self.generator) {
            (Some(analyzer), _) => analyzer.default_aggl(),
            (None, Some(generator)) => generator.default_aggl(),
            (None, None) => None,
        }
    }

    fn default_praet(&self) -> Option<&str> {
        match (&self.analyzer, &self.generator) {
            (Some(analyzer), _) => analyzer.default_praet(),
            (None, Some(generator)) => generator.default_praet(),
            (None, None) => None,
        }
    }

    fn available_aggl_options(&self) -> Vec<String> {
        match (&self.analyzer, &self.generator) {
            (Some(analyzer), _) => analyzer.available_aggl_options(),
            (None, Some(generator)) => generator.available_aggl_options(),
            (None, None) => Vec::new(),
        }
    }

    fn available_praet_options(&self) -> Vec<String> {
        match (&self.analyzer, &self.generator) {
            (Some(analyzer), _) => analyzer.available_praet_options(),
            (None, Some(generator)) => generator.available_praet_options(),
            (None, None) => Vec::new(),
        }
    }

    fn validate_segmentation(
        &self,
        segmentation: &SegmentationPreset,
        option: &str,
        value: &str,
    ) -> Result<()> {
        if let Some(analyzer) = &self.analyzer {
            analyzer.validate_segmentation(segmentation, option, value)?;
        }
        if let Some(generator) = &self.generator {
            generator.validate_segmentation(segmentation, option, value)?;
        }
        Ok(())
    }

    fn is_native_analyzer(&self) -> bool {
        self.analyzer.is_some()
    }

    fn analyze_native_word(
        &self,
        word: &str,
        start_node: i32,
        case_handling: CaseHandling,
        segmentation: &SegmentationPreset,
    ) -> Result<(Vec<MorphInterpretation>, i32)> {
        match &self.analyzer {
            Some(analyzer) => {
                analyzer.analyze_native_word(word, start_node, case_handling, segmentation)
            }
            None => Ok((Vec::new(), start_node)),
        }
    }

    fn analyze_word_interpretations(
        &self,
        orth: &str,
        start_node: i32,
        case_handling: CaseHandling,
        segmentation: &SegmentationPreset,
    ) -> Result<Option<(Vec<MorphInterpretation>, i32)>> {
        match &self.analyzer {
            Some(analyzer) => {
                analyzer.analyze_word_interpretations(orth, start_node, case_handling, segmentation)
            }
            None => Ok(None),
        }
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
        match &self.analyzer {
            Some(analyzer) => analyzer.lookup_interpretations(
                orth,
                start_node,
                end_node,
                result_orth,
                case_handling,
                segmentation,
            ),
            None => Ok(None),
        }
    }

    fn synthesize_interpretations(
        &self,
        lemma: &str,
        segmentation: &SegmentationPreset,
    ) -> Result<Vec<MorphInterpretation>> {
        match &self.generator {
            Some(generator) => generator.synthesize_interpretations(lemma, segmentation),
            None => Ok(Vec::new()),
        }
    }
}
