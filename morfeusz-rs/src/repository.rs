use std::path::{Path, PathBuf};

use crate::{
    BinaryAnalyzerLexicon, BinaryGeneratorLexicon, BinaryLexicon, Error, Morfeusz, MorfeuszUsage,
    Result,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedDictionaryPaths {
    pub analyzer: Option<PathBuf>,
    pub generator: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinaryDictionaryRepository {
    search_paths: Vec<PathBuf>,
}

impl Default for BinaryDictionaryRepository {
    fn default() -> Self {
        Self {
            search_paths: vec![PathBuf::from(".")],
        }
    }
}

impl BinaryDictionaryRepository {
    pub fn new(search_paths: impl IntoIterator<Item = PathBuf>) -> Self {
        Self {
            search_paths: search_paths.into_iter().collect(),
        }
    }

    pub fn search_paths(&self) -> &[PathBuf] {
        &self.search_paths
    }

    pub fn find_named(&self, dict_name: &str) -> NamedDictionaryPaths {
        let analyzer_filename = format!("{dict_name}-a.dict");
        let generator_filename = format!("{dict_name}-s.dict");
        let mut paths = NamedDictionaryPaths {
            analyzer: None,
            generator: None,
        };

        for dir in &self.search_paths {
            if paths.analyzer.is_none() {
                let candidate = dir.join(&analyzer_filename);
                if candidate.exists() {
                    paths.analyzer = Some(candidate);
                }
            }
            if paths.generator.is_none() {
                let candidate = dir.join(&generator_filename);
                if candidate.exists() {
                    paths.generator = Some(candidate);
                }
            }
        }

        paths
    }

    pub fn load_named(&self, dict_name: &str, usage: MorfeuszUsage) -> Result<Morfeusz> {
        let paths = self.find_named(dict_name);
        match usage {
            MorfeuszUsage::AnalyseOnly => {
                let analyzer_path = paths.analyzer.ok_or_else(|| {
                    Error::NotFound(format!(
                        "Failed to load analyzer dictionary \"{dict_name}\""
                    ))
                })?;
                let lexicon = BinaryAnalyzerLexicon::from_path(analyzer_path)?;
                Ok(Morfeusz::with_lexicon(lexicon, usage))
            }
            MorfeuszUsage::GenerateOnly => {
                let generator_path = paths.generator.ok_or_else(|| {
                    Error::NotFound(format!(
                        "Failed to load generator dictionary \"{dict_name}\""
                    ))
                })?;
                let lexicon = BinaryGeneratorLexicon::from_path(generator_path)?;
                Ok(Morfeusz::with_lexicon(lexicon, usage))
            }
            MorfeuszUsage::BothAnalyseAndGenerate => {
                let analyzer_path = paths.analyzer.ok_or_else(|| {
                    Error::NotFound(format!(
                        "Failed to load analyzer dictionary \"{dict_name}\""
                    ))
                })?;
                let generator_path = paths.generator.ok_or_else(|| {
                    Error::NotFound(format!(
                        "Failed to load generator dictionary \"{dict_name}\""
                    ))
                })?;
                let lexicon = BinaryLexicon::from_paths(
                    Some(analyzer_path.as_path()),
                    Some(generator_path.as_path()),
                )?;
                Ok(Morfeusz::with_lexicon(lexicon, usage))
            }
        }
    }

    pub fn load_path(&self, path: &Path, usage: MorfeuszUsage) -> Result<Morfeusz> {
        match binary_dictionary_usage(path, usage) {
            BinaryDictionaryUsage::Analyzer(effective_usage) => {
                let lexicon = BinaryAnalyzerLexicon::from_path(path)?;
                Ok(Morfeusz::with_lexicon(lexicon, effective_usage))
            }
            BinaryDictionaryUsage::Generator(effective_usage) => {
                let lexicon = BinaryGeneratorLexicon::from_path(path)?;
                Ok(Morfeusz::with_lexicon(lexicon, effective_usage))
            }
        }
    }
}

enum BinaryDictionaryUsage {
    Analyzer(MorfeuszUsage),
    Generator(MorfeuszUsage),
}

fn binary_dictionary_usage(path: &Path, usage: MorfeuszUsage) -> BinaryDictionaryUsage {
    match usage {
        MorfeuszUsage::AnalyseOnly => BinaryDictionaryUsage::Analyzer(usage),
        MorfeuszUsage::GenerateOnly => BinaryDictionaryUsage::Generator(usage),
        MorfeuszUsage::BothAnalyseAndGenerate => {
            if file_name_ends_with(path, "-s.dict") {
                BinaryDictionaryUsage::Generator(MorfeuszUsage::GenerateOnly)
            } else {
                BinaryDictionaryUsage::Analyzer(MorfeuszUsage::AnalyseOnly)
            }
        }
    }
}

fn file_name_ends_with(path: &Path, suffix: &str) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(suffix))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn fixture(name: &str) -> PathBuf {
        fixture_dir().join(name)
    }

    fn fixture_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("binary")
    }

    #[test]
    fn loads_binary_analyzer_path() {
        let repo = BinaryDictionaryRepository::default();
        let mut morfeusz = repo
            .load_path(
                &fixture("test-dict-copyright-v1-a.dict"),
                MorfeuszUsage::BothAnalyseAndGenerate,
            )
            .unwrap();

        let result = morfeusz.analyse("7").unwrap();

        assert!(result
            .iter()
            .any(|item| item.orth == "7" && item.tag(morfeusz.id_resolver()) == Some("dig")));
    }

    #[test]
    fn finds_named_dictionary_parts_in_search_paths() {
        let repo = BinaryDictionaryRepository::new([fixture_dir()]);

        let paths = repo.find_named("test-dict-copyright-v1");

        assert!(paths
            .analyzer
            .as_ref()
            .is_some_and(|path| path.ends_with("test-dict-copyright-v1-a.dict")));
    }

    #[test]
    fn explicit_empty_search_paths_do_not_fall_back_to_current_directory() {
        let repo = BinaryDictionaryRepository::new([]);

        assert!(repo.search_paths().is_empty());
        assert!(repo.find_named("test-dict-copyright-v1").analyzer.is_none());
    }

    #[test]
    fn missing_named_dictionary_reports_not_found() {
        let repo = BinaryDictionaryRepository::new([fixture_dir()]);

        let err = repo
            .load_named("definitely-missing-dictionary", MorfeuszUsage::AnalyseOnly)
            .unwrap_err();

        assert!(matches!(err, Error::NotFound(_)));
    }

    #[test]
    fn invalid_named_dictionary_reports_invalid_dictionary() {
        let dir =
            std::env::temp_dir().join(format!("morfeusz-rs-invalid-dict-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("broken-a.dict");
        fs::write(&path, b"asfasdfa\n").unwrap();
        let repo = BinaryDictionaryRepository::new([dir.clone()]);

        let err = repo
            .load_named("broken", MorfeuszUsage::AnalyseOnly)
            .unwrap_err();

        let _ = fs::remove_file(path);
        let _ = fs::remove_dir(dir);
        assert!(matches!(err, Error::InvalidDictionary(_)));
    }
}
