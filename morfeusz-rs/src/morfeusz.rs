use std::path::Path;

use crate::{
    BinaryDictionaryRepository, CaseHandling, Charset, Config, Dictionary, Engine, Error,
    IdResolver, MorfeuszUsage, MorphInterpretation, NumberingScope, Result, SegmentationPreset,
    Session, TokenNumbering, WhitespaceHandling,
};

const VERSION: &str = "1.99.15";
const COPYRIGHT: &str = "Copyright © 2014–2021 by Institute of Computer Science, Polish Academy of
Science

All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are
met:

Redistributions of source code must retain the above copyright notice,
this list of conditions and the following disclaimer.
Redistributions in binary form must reproduce the above copyright
notice, this list of conditions and the following disclaimer in the
documentation and/or other materials provided with the distribution.

THIS SOFTWARE IS PROVIDED BY COPYRIGHT HOLDERS “AS IS” AND ANY EXPRESS
OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
DISCLAIMED. IN NO EVENT SHALL COPYRIGHT HOLDERS OR CONTRIBUTORS BE
LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF
THE POSSIBILITY OF SUCH DAMAGE.
";

#[derive(Debug, Clone)]
pub struct Morfeusz {
    engine: Engine,
    session: Session,
    usage: MorfeuszUsage,
}

impl Default for Morfeusz {
    fn default() -> Self {
        Self::new()
    }
}

impl Morfeusz {
    pub fn new() -> Self {
        Self::with_dictionary(Dictionary::empty(), MorfeuszUsage::default())
    }

    pub fn with_dictionary(dictionary: Dictionary, usage: MorfeuszUsage) -> Self {
        Self::with_lexicon(dictionary, usage)
    }

    pub fn with_lexicon(lexicon: impl crate::Lexicon + 'static, usage: MorfeuszUsage) -> Self {
        let engine = Engine::builder().lexicon(lexicon).build();
        let session = engine.session();
        Self {
            engine,
            session,
            usage,
        }
    }

    pub fn create_instance(usage: MorfeuszUsage) -> Result<Self> {
        Self::create_instance_named(Self::default_dict_name(), usage)
    }

    pub fn create_instance_named(dict_name: &str, usage: MorfeuszUsage) -> Result<Self> {
        BinaryDictionaryRepository::default().load_named(dict_name, usage)
    }

    pub fn create_instance_with_repository(
        repository: &BinaryDictionaryRepository,
        usage: MorfeuszUsage,
    ) -> Result<Self> {
        Self::create_instance_named_with_repository(repository, Self::default_dict_name(), usage)
    }

    pub fn create_instance_named_with_repository(
        repository: &BinaryDictionaryRepository,
        dict_name: &str,
        usage: MorfeuszUsage,
    ) -> Result<Self> {
        repository.load_named(dict_name, usage)
    }

    pub fn version() -> &'static str {
        VERSION
    }

    pub fn default_dict_name() -> &'static str {
        "sgjp"
    }

    pub fn copyright() -> &'static str {
        COPYRIGHT
    }

    pub fn dict_id(&self) -> &str {
        self.engine.lexicon_id()
    }

    pub fn dict_copyright(&self) -> &str {
        self.engine.lexicon_copyright()
    }

    pub fn id_resolver(&self) -> &IdResolver {
        self.engine.resolver()
    }

    pub fn usage(&self) -> MorfeuszUsage {
        self.usage
    }

    pub fn analyse(&mut self, text: &str) -> Result<Vec<MorphInterpretation>> {
        self.ensure_can_analyse()?;
        self.session.analyze(text)
    }

    pub fn analyse_iter(&mut self, text: &str) -> Result<ResultsIterator> {
        Ok(ResultsIterator::new(self.analyse(text)?))
    }

    /// Stateless analysis from an explicit `start_node`, returning the results
    /// and the next free node number. Unlike [`analyse`](Self::analyse) this
    /// takes `&self` and carries no numbering state, so a single instance can be
    /// shared across threads and driven by a caller that manages node numbering
    /// itself (e.g. a parallel CLI). With `start_node = 0` the node numbers are
    /// exactly those of `SEPARATE` numbering for that line.
    pub fn analyse_from(
        &self,
        text: &str,
        start_node: i32,
    ) -> Result<(Vec<MorphInterpretation>, i32)> {
        self.ensure_can_analyse()?;
        self.engine.analyze_from_node(text, start_node)
    }

    /// An independent instance for a dedicated thread, sharing the immutable
    /// dictionary (no multi-megabyte copy) but with its own decode caches so
    /// parallel workers never contend on a shared cache lock. Combine with
    /// [`analyse_from`](Self::analyse_from) / [`generate`](Self::generate) to
    /// analyse on many threads at near-linear scaling.
    pub fn fork(&self) -> Self {
        let engine = self.engine.fork();
        let session = engine.session();
        Self {
            engine,
            session,
            usage: self.usage,
        }
    }

    pub fn generate(&self, lemma: &str) -> Result<Vec<MorphInterpretation>> {
        self.ensure_can_generate()?;
        self.engine.generate(lemma)
    }

    pub fn generate_by_tag_id(&self, lemma: &str, tag_id: i32) -> Result<Vec<MorphInterpretation>> {
        self.ensure_can_generate()?;
        self.engine.generate_by_tag_id(lemma, tag_id)
    }

    pub fn set_charset(&mut self, charset: Charset) {
        self.update_config(|config| config.with_charset(charset));
    }

    pub fn charset(&self) -> Charset {
        self.engine.config().charset()
    }

    pub fn set_aggl(&mut self, aggl: &str) -> Result<()> {
        let segmentation = self
            .engine
            .config()
            .segmentation()
            .clone()
            .with_aggl(aggl)?;
        self.engine
            .validate_segmentation(&segmentation, "aggl", aggl)?;
        self.update_config(|config| config.with_segmentation(segmentation));
        Ok(())
    }

    pub fn aggl(&self) -> &str {
        // Effective value: explicit override, else the dictionary's own default,
        // else the conventional fallback for dictionary-less instances.
        self.engine
            .config()
            .segmentation()
            .aggl()
            .or_else(|| self.engine.default_aggl())
            .unwrap_or("permissive")
    }

    pub fn set_praet(&mut self, praet: &str) -> Result<()> {
        let segmentation = self
            .engine
            .config()
            .segmentation()
            .clone()
            .with_praet(praet)?;
        self.engine
            .validate_segmentation(&segmentation, "praet", praet)?;
        self.update_config(|config| config.with_segmentation(segmentation));
        Ok(())
    }

    pub fn praet(&self) -> &str {
        self.engine
            .config()
            .segmentation()
            .praet()
            .or_else(|| self.engine.default_praet())
            .unwrap_or("split")
    }

    pub fn set_case_handling(&mut self, case_handling: CaseHandling) {
        self.update_config(|config| config.with_case_handling(case_handling));
    }

    pub fn case_handling(&self) -> CaseHandling {
        self.engine.config().case_handling()
    }

    pub fn set_token_numbering(&mut self, token_numbering: TokenNumbering) {
        self.update_config(|config| config.with_numbering(NumberingScope::from(token_numbering)));
        self.session.reset_numbering();
    }

    pub fn token_numbering(&self) -> TokenNumbering {
        TokenNumbering::from(self.engine.config().numbering())
    }

    pub fn set_whitespace_handling(&mut self, whitespace_handling: WhitespaceHandling) {
        self.update_config(|config| config.with_whitespace(whitespace_handling));
    }

    pub fn whitespace_handling(&self) -> WhitespaceHandling {
        self.engine.config().whitespace()
    }

    pub fn set_debug(&mut self, debug: bool) {
        self.update_config(|config| config.with_debug(debug));
    }

    pub fn debug(&self) -> bool {
        self.engine.config().debug()
    }

    pub fn set_dictionary(&mut self, dictionary: Dictionary) {
        let config = self.engine.config().clone();
        self.engine = Engine::builder().lexicon(dictionary).config(config).build();
        self.session = self.engine.session();
    }

    pub fn set_dictionary_named(&mut self, dict_name: &str) -> Result<()> {
        let repository = BinaryDictionaryRepository::default();
        self.set_dictionary_named_with_repository(&repository, dict_name)
    }

    pub fn set_dictionary_named_with_repository(
        &mut self,
        repository: &BinaryDictionaryRepository,
        dict_name: &str,
    ) -> Result<()> {
        let loaded = repository.load_named(dict_name, self.usage)?;
        self.replace_with_loaded_dictionary(loaded);
        Ok(())
    }

    pub fn set_dictionary_path(&mut self, path: impl AsRef<Path>) -> Result<()> {
        let repository = BinaryDictionaryRepository::default();
        self.set_dictionary_path_with_repository(&repository, path)
    }

    pub fn set_dictionary_path_with_repository(
        &mut self,
        repository: &BinaryDictionaryRepository,
        path: impl AsRef<Path>,
    ) -> Result<()> {
        let loaded = repository.load_path(path.as_ref(), self.usage)?;
        self.replace_with_loaded_dictionary(loaded);
        Ok(())
    }

    pub fn available_aggl_options(&self) -> Vec<String> {
        self.engine.available_aggl_options()
    }

    pub fn available_praet_options(&self) -> Vec<String> {
        self.engine.available_praet_options()
    }

    fn update_config(&mut self, update: impl FnOnce(Config) -> Config) {
        let config = update(self.engine.config().clone());
        self.engine = self.engine.with_config(config);
        self.session = self.engine.session();
    }

    fn config_for_dictionary_switch(&self) -> Config {
        Config::default()
            .with_charset(self.engine.config().charset())
            .with_case_handling(self.engine.config().case_handling())
            .with_numbering(self.engine.config().numbering())
            .with_whitespace(self.engine.config().whitespace())
            .with_debug(self.engine.config().debug())
    }

    fn replace_with_loaded_dictionary(&mut self, loaded: Self) {
        self.usage = loaded.usage;
        self.engine = loaded
            .engine
            .with_config(self.config_for_dictionary_switch());
        self.session = self.engine.session();
    }

    fn ensure_can_analyse(&self) -> Result<()> {
        match self.usage {
            MorfeuszUsage::AnalyseOnly | MorfeuszUsage::BothAnalyseAndGenerate => Ok(()),
            MorfeuszUsage::GenerateOnly => Err(Error::Unsupported(
                "Cannot analyse with given Morfeusz instance.".to_owned(),
            )),
        }
    }

    fn ensure_can_generate(&self) -> Result<()> {
        match self.usage {
            MorfeuszUsage::GenerateOnly | MorfeuszUsage::BothAnalyseAndGenerate => Ok(()),
            MorfeuszUsage::AnalyseOnly => Err(Error::Unsupported(
                "Cannot generate with given Morfeusz instance.".to_owned(),
            )),
        }
    }
}

impl From<SegmentationPreset> for Config {
    fn from(segmentation: SegmentationPreset) -> Self {
        Config::default().with_segmentation(segmentation)
    }
}

#[derive(Debug, Clone)]
pub struct ResultsIterator {
    inner: std::vec::IntoIter<MorphInterpretation>,
    peeked: Option<MorphInterpretation>,
}

impl ResultsIterator {
    pub fn new(items: Vec<MorphInterpretation>) -> Self {
        Self {
            inner: items.into_iter(),
            peeked: None,
        }
    }

    pub fn has_next(&mut self) -> bool {
        if self.peeked.is_none() {
            self.peeked = self.inner.next();
        }
        self.peeked.is_some()
    }

    pub fn peek(&mut self) -> Option<&MorphInterpretation> {
        if self.peeked.is_none() {
            self.peeked = self.inner.next();
        }
        self.peeked.as_ref()
    }

    pub fn peek_result(&mut self) -> Result<&MorphInterpretation> {
        self.peek().ok_or_else(|| {
            Error::OutOfRange("No more interpretations available to ResultsIterator".to_owned())
        })
    }

    pub fn next_result(&mut self) -> Result<MorphInterpretation> {
        self.next().ok_or_else(|| {
            Error::OutOfRange("No more interpretations available to ResultsIterator".to_owned())
        })
    }
}

impl Iterator for ResultsIterator {
    type Item = MorphInterpretation;

    fn next(&mut self) -> Option<Self::Item> {
        self.peeked.take().or_else(|| self.inner.next())
    }
}
