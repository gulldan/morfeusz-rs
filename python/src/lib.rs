#![allow(non_snake_case)]

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

// Global allocator for this extension module's Rust allocations. Python's own
// allocations are unaffected; this only speeds up the analyzer's short-lived
// per-word / per-interpretation allocations and scales across no-GIL threads.
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use morfeusz::{
    BinaryDictionaryRepository, CaseHandling, Charset, Dictionary, Error, IdResolver,
    Morfeusz as CoreMorfeusz, MorfeuszUsage, MorphInterpretation,
    ResultsIterator as CoreResultsIterator, TokenNumbering, TsvLexiconLoader, WhitespaceHandling,
};
use pyo3::exceptions::{PyIOError, PyRuntimeError, PyStopIteration, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyList, PySet};
use rayon::prelude::*;
use std::collections::{BTreeMap, BTreeSet};

// `skip_from_py_object`: this type is only ever produced (returned to Python),
// never extracted from a Python object as a function argument, so we opt out of
// the (now opt-in, in pyo3 0.28+) automatic `FromPyObject` derive for `Clone`
// pyclasses.
#[pyclass(name = "MorphInterpretation", skip_from_py_object)]
#[derive(Clone)]
struct PyMorphInterpretation {
    #[pyo3(get, set)]
    startNode: i32,
    #[pyo3(get, set)]
    endNode: i32,
    #[pyo3(get, set)]
    orth: String,
    #[pyo3(get, set)]
    lemma: String,
    #[pyo3(get, set)]
    tagId: i32,
    #[pyo3(get, set)]
    nameId: i32,
    #[pyo3(get, set)]
    labelsId: i32,
}

#[pymethods]
impl PyMorphInterpretation {
    #[new]
    fn new() -> Self {
        MorphInterpretation::default().into()
    }

    fn isIgn(&self) -> bool {
        self.tagId == 0
    }

    fn isWhitespace(&self) -> bool {
        self.tagId == 1
    }

    #[staticmethod]
    fn createIgn(startNode: i32, endNode: i32, orth: &str, lemma: &str) -> Self {
        MorphInterpretation::create_ign(startNode, endNode, orth, lemma).into()
    }

    #[staticmethod]
    fn createWhitespace(startNode: i32, endNode: i32, orth: &str) -> Self {
        MorphInterpretation::create_whitespace(startNode, endNode, orth).into()
    }

    #[getter("_orth")]
    fn get_shadow_orth(&self) -> String {
        self.orth.clone()
    }

    #[setter("_orth")]
    fn set_shadow_orth(&mut self, value: String) {
        self.orth = value;
    }

    #[getter("_lemma")]
    fn get_shadow_lemma(&self) -> String {
        self.lemma.clone()
    }

    #[setter("_lemma")]
    fn set_shadow_lemma(&mut self, value: String) {
        self.lemma = value;
    }

    fn getTag(&self, morfeusz: &Bound<'_, PyAny>) -> PyResult<String> {
        with_py_morfeusz_resolver(morfeusz, |resolver| {
            resolver
                .tag(self.tagId)
                .map(ToOwned::to_owned)
                .ok_or_else(|| PyRuntimeError::new_err(format!("Invalid tag id: {}", self.tagId)))
        })
    }

    fn getName(&self, morfeusz: &Bound<'_, PyAny>) -> PyResult<Vec<String>> {
        let name = with_py_morfeusz_resolver(morfeusz, |resolver| {
            resolver
                .name(self.nameId)
                .map(ToOwned::to_owned)
                .ok_or_else(|| PyRuntimeError::new_err(format!("Invalid name id: {}", self.nameId)))
        })?;
        Ok(split_optional_id_string(&name))
    }

    fn getLabelsAsString(&self, morfeusz: &Bound<'_, PyAny>) -> PyResult<String> {
        with_py_morfeusz_resolver(morfeusz, |resolver| {
            resolver
                .labels_as_string(self.labelsId)
                .map(ToOwned::to_owned)
                .ok_or_else(|| {
                    PyRuntimeError::new_err(format!("Invalid labels id: {}", self.labelsId))
                })
        })
    }

    fn getLabelsAsUnicode(&self, morfeusz: &Bound<'_, PyAny>) -> PyResult<String> {
        self.getLabelsAsString(morfeusz)
    }

    fn getLabels(&self, morfeusz: &Bound<'_, PyAny>) -> PyResult<Vec<String>> {
        with_py_morfeusz_resolver(morfeusz, |resolver| {
            resolver
                .labels(self.labelsId)
                .map(labels_set_to_vec)
                .ok_or_else(|| {
                    PyRuntimeError::new_err(format!("Invalid labels id: {}", self.labelsId))
                })
        })
    }
}

impl From<MorphInterpretation> for PyMorphInterpretation {
    fn from(value: MorphInterpretation) -> Self {
        Self {
            startNode: value.start_node,
            endNode: value.end_node,
            orth: value.orth,
            lemma: value.lemma,
            tagId: value.tag_id,
            nameId: value.name_id,
            labelsId: value.labels_id,
        }
    }
}

#[pyclass(name = "ResultsIterator")]
struct PyResultsIterator {
    inner: CoreResultsIterator,
}

#[pymethods]
impl PyResultsIterator {
    fn __iter__(slf: PyRefMut<'_, Self>) -> PyRefMut<'_, Self> {
        slf
    }

    fn __next__(&mut self) -> Option<PyMorphInterpretation> {
        self.inner.next().map(Into::into)
    }

    fn next(&mut self) -> PyResult<PyMorphInterpretation> {
        self.inner
            .next()
            .map(Into::into)
            .ok_or_else(|| PyStopIteration::new_err(()))
    }

    fn hasNext(&mut self) -> bool {
        self.inner.has_next()
    }

    fn peek(&mut self) -> PyResult<PyMorphInterpretation> {
        self.inner
            .peek_result()
            .cloned()
            .map(Into::into)
            .map_err(to_py_err)
    }
}

#[pyclass(name = "_Morfeusz", skip_from_py_object)]
#[derive(Clone)]
struct PyLowMorfeusz {
    inner: CoreMorfeusz,
}

#[pymethods]
impl PyLowMorfeusz {
    #[new]
    #[pyo3(signature = (dictionary_path=None, tagset_path=None, usage=403))]
    fn new(dictionary_path: Option<&str>, tagset_path: Option<&str>, usage: i32) -> PyResult<Self> {
        let usage = morfeusz_usage_from_i32(usage)?;
        Ok(Self::from_inner(load_core_morfeusz(
            dictionary_path,
            tagset_path,
            usage,
        )?))
    }

    #[staticmethod]
    #[pyo3(signature = (dictName=None, usage=403))]
    fn createInstance(dictName: Option<&Bound<'_, PyAny>>, usage: i32) -> PyResult<Self> {
        let (dict_name, usage) = parse_create_instance_args(dictName, usage)?;
        Ok(Self::from_inner(load_create_instance_core(
            dict_name.as_deref(),
            usage,
        )?))
    }

    #[staticmethod]
    #[pyo3(signature = (usage=403))]
    fn _createInstance(usage: i32) -> PyResult<Self> {
        Self::createInstance(None, usage)
    }

    #[staticmethod]
    fn getVersion() -> String {
        CoreMorfeusz::version().to_owned()
    }

    #[staticmethod]
    fn getDefaultDictName() -> String {
        CoreMorfeusz::default_dict_name().to_owned()
    }

    #[staticmethod]
    fn getCopyright() -> String {
        CoreMorfeusz::copyright().to_owned()
    }

    fn getDictID(&self) -> String {
        self.inner.dict_id().to_owned()
    }

    fn getDictCopyright(&self) -> String {
        self.inner.dict_copyright().to_owned()
    }

    fn dict_id(&self) -> String {
        self.getDictID()
    }

    fn dict_copyright(&self) -> String {
        self.getDictCopyright()
    }

    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }

    fn analyse(&mut self, text: &str) -> PyResult<Vec<PyMorphInterpretation>> {
        self.inner
            .analyse(text)
            .map(|items| items.into_iter().map(Into::into).collect())
            .map_err(to_py_err)
    }

    fn analyse_iter(&mut self, text: &str) -> PyResult<PyResultsIterator> {
        self.inner
            .analyse_iter(text)
            .map(|inner| PyResultsIterator { inner })
            .map_err(to_py_err)
    }

    fn _analyseAsIterator(&mut self, text: &str) -> PyResult<PyResultsIterator> {
        self.analyse_iter(text)
    }

    #[pyo3(signature = (lemma, tagId=None))]
    fn generate(&self, lemma: &str, tagId: Option<i32>) -> PyResult<Vec<PyMorphInterpretation>> {
        let result = match tagId {
            Some(tag_id) => self.inner.generate_by_tag_id(lemma, tag_id),
            None => self.inner.generate(lemma),
        };
        result
            .map(|items| items.into_iter().map(Into::into).collect())
            .map_err(to_py_err)
    }

    fn _generateByTagId(&self, lemma: &str, tagId: i32) -> PyResult<Vec<PyMorphInterpretation>> {
        self.inner
            .generate_by_tag_id(lemma, tagId)
            .map(|items| items.into_iter().map(Into::into).collect())
            .map_err(to_py_err)
    }

    fn setCharset(&mut self, option: i32) -> PyResult<()> {
        self.inner.set_charset(charset_from_i32(option)?);
        Ok(())
    }

    fn getCharset(&self) -> i32 {
        self.inner.charset() as i32
    }

    fn setAggl(&mut self, option: &str) -> PyResult<()> {
        self.inner.set_aggl(option).map_err(to_py_err)
    }

    fn getAggl(&self) -> String {
        self.inner.aggl().to_owned()
    }

    fn setPraet(&mut self, option: &str) -> PyResult<()> {
        self.inner.set_praet(option).map_err(to_py_err)
    }

    fn getPraet(&self) -> String {
        self.inner.praet().to_owned()
    }

    fn setCaseHandling(&mut self, option: i32) -> PyResult<()> {
        self.inner
            .set_case_handling(case_handling_from_i32(option)?);
        Ok(())
    }

    fn getCaseHandling(&self) -> i32 {
        self.inner.case_handling() as i32
    }

    fn setTokenNumbering(&mut self, option: i32) -> PyResult<()> {
        self.inner
            .set_token_numbering(token_numbering_from_i32(option)?);
        Ok(())
    }

    fn getTokenNumbering(&self) -> i32 {
        self.inner.token_numbering() as i32
    }

    fn setWhitespaceHandling(&mut self, option: i32) -> PyResult<()> {
        self.inner
            .set_whitespace_handling(whitespace_handling_from_i32(option)?);
        Ok(())
    }

    fn getWhitespaceHandling(&self) -> i32 {
        self.inner.whitespace_handling() as i32
    }

    fn setDebug(&mut self, debug: bool) {
        self.inner.set_debug(debug);
    }

    fn getIdResolver(&self) -> PyIdResolver {
        PyIdResolver {
            inner: self.inner.id_resolver().clone(),
        }
    }

    fn setDictionary(&mut self, dictName: &str) -> PyResult<()> {
        set_core_dictionary_preserving_options(&mut self.inner, dictName)
    }

    fn addDictionaryPath(&self, dict_path: &str) {
        add_dictionary_search_path(Path::new(dict_path));
    }

    fn add_dictionary_path(&self, dict_path: &str) {
        self.addDictionaryPath(dict_path);
    }

    fn getAvailableAgglOptions(&self) -> Vec<String> {
        self.inner
            .available_aggl_options()
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    fn getAvailablePraetOptions(&self) -> Vec<String> {
        self.inner
            .available_praet_options()
            .iter()
            .map(ToString::to_string)
            .collect()
    }
}

impl PyLowMorfeusz {
    fn from_inner(inner: CoreMorfeusz) -> Self {
        Self { inner }
    }
}

#[pyclass(name = "IdResolver")]
struct PyIdResolver {
    inner: IdResolver,
}

#[pymethods]
impl PyIdResolver {
    fn getTagsetId(&self) -> String {
        self.inner.tagset_id().to_owned()
    }

    fn getTag(&self, tag_id: i32) -> PyResult<String> {
        self.inner
            .tag(tag_id)
            .map(ToOwned::to_owned)
            .ok_or_else(|| PyRuntimeError::new_err(format!("Invalid tag id: {tag_id}")))
    }

    fn getTagId(&self, tag: &str) -> PyResult<i32> {
        self.inner.tag_id(tag).map_err(to_py_err)
    }

    fn getName(&self, name_id: i32) -> PyResult<String> {
        self.inner
            .name(name_id)
            .map(ToOwned::to_owned)
            .ok_or_else(|| PyRuntimeError::new_err(format!("Invalid name id: {name_id}")))
    }

    fn getNameId(&self, name: &str) -> PyResult<i32> {
        self.inner.name_id(name).map_err(to_py_err)
    }

    fn getLabelsAsString(&self, labels_id: i32) -> PyResult<String> {
        self.inner
            .labels_as_string(labels_id)
            .map(ToOwned::to_owned)
            .ok_or_else(|| PyRuntimeError::new_err(format!("Invalid labels id: {labels_id}")))
    }

    fn getLabelsAsUnicode(&self, labels_id: i32) -> PyResult<String> {
        self.getLabelsAsString(labels_id)
    }

    fn getLabels(&self, labels_id: i32) -> PyResult<Vec<String>> {
        self.inner
            .labels(labels_id)
            .map(labels_set_to_vec)
            .ok_or_else(|| PyRuntimeError::new_err(format!("Invalid labels id: {labels_id}")))
    }

    fn getLabelsId(&self, labels: &str) -> PyResult<i32> {
        self.inner.labels_id(labels).map_err(to_py_err)
    }

    fn getTagsCount(&self) -> usize {
        self.inner.tags_count()
    }

    fn getNamesCount(&self) -> usize {
        self.inner.names_count()
    }

    fn getLabelsCount(&self) -> usize {
        self.inner.labels_count()
    }
}

#[pyclass(name = "Morfeusz", dict)]
struct PyMorfeusz {
    #[pyo3(get, set)]
    _morfeusz_obj: Py<PyLowMorfeusz>,
    #[pyo3(get, set)]
    expand_dag: bool,
    #[pyo3(get, set)]
    expand_tags: bool,
    #[pyo3(get, set)]
    expand_dot: bool,
    #[pyo3(get, set)]
    expand_underscore: bool,
}

#[pymethods]
impl PyMorfeusz {
    #[new]
    #[pyo3(signature = (
        dictionary_path=None,
        tagset_path=None,
        usage=403,
        dict_name=None,
        dict_path=None,
        analyse=true,
        generate=true,
        expand_dag=false,
        expand_tags=false,
        expand_dot=true,
        expand_underscore=true,
        aggl=None,
        praet=None,
        separate_numbering=true,
        case_handling=100,
        whitespace=301
    ))]
    fn new(
        py: Python<'_>,
        dictionary_path: Option<&str>,
        tagset_path: Option<&str>,
        usage: i32,
        dict_name: Option<&str>,
        dict_path: Option<&str>,
        analyse: bool,
        generate: bool,
        expand_dag: bool,
        expand_tags: bool,
        expand_dot: bool,
        expand_underscore: bool,
        aggl: Option<&str>,
        praet: Option<&str>,
        separate_numbering: bool,
        case_handling: i32,
        whitespace: i32,
    ) -> PyResult<Self> {
        if let Some(path) = dict_path {
            add_dictionary_search_path(Path::new(path));
        }

        let usage = effective_usage(usage, analyse, generate)?;
        let mut inner = if let Some(name) = dict_name {
            load_named_binary_core_morfeusz_for_create(name, usage)?
        } else if let Some(path) = dictionary_path {
            if tagset_path.is_some() || is_dictionary_path(path) {
                load_core_morfeusz(Some(path), tagset_path, usage)?
            } else {
                load_named_binary_core_morfeusz_for_create(path, usage)?
            }
        } else {
            load_named_binary_core_morfeusz_for_create(CoreMorfeusz::default_dict_name(), usage)?
        };

        if let Some(aggl) = aggl {
            inner.set_aggl(aggl).map_err(to_py_err)?;
        }
        if let Some(praet) = praet {
            inner.set_praet(praet).map_err(to_py_err)?;
        }
        if !separate_numbering {
            inner.set_token_numbering(TokenNumbering::Continuous);
        }
        inner.set_case_handling(case_handling_from_i32(case_handling)?);
        inner.set_whitespace_handling(whitespace_handling_from_i32(whitespace)?);

        Ok(Self {
            _morfeusz_obj: Py::new(py, PyLowMorfeusz::from_inner(inner))?,
            expand_dag,
            expand_tags,
            expand_dot,
            expand_underscore,
        })
    }

    #[staticmethod]
    #[pyo3(signature = (dictName=None, usage=403))]
    fn createInstance(
        py: Python<'_>,
        dictName: Option<&Bound<'_, PyAny>>,
        usage: i32,
    ) -> PyResult<Self> {
        let (dict_name, usage) = parse_create_instance_args(dictName, usage)?;
        Self::from_inner(py, load_create_instance_core(dict_name.as_deref(), usage)?)
    }

    #[staticmethod]
    #[pyo3(signature = (usage=403))]
    fn _createInstance(py: Python<'_>, usage: i32) -> PyResult<Self> {
        Self::createInstance(py, None, usage)
    }

    #[staticmethod]
    fn getVersion() -> String {
        CoreMorfeusz::version().to_owned()
    }

    #[staticmethod]
    fn getDefaultDictName() -> String {
        CoreMorfeusz::default_dict_name().to_owned()
    }

    #[staticmethod]
    fn getCopyright() -> String {
        CoreMorfeusz::copyright().to_owned()
    }

    fn getDictID(&self, py: Python<'_>) -> PyResult<String> {
        Ok(self.low(py)?.inner.dict_id().to_owned())
    }

    fn getDictCopyright(&self, py: Python<'_>) -> PyResult<String> {
        Ok(self.low(py)?.inner.dict_copyright().to_owned())
    }

    fn dict_id(&self, py: Python<'_>) -> PyResult<String> {
        self.getDictID(py)
    }

    fn dict_copyright(&self, py: Python<'_>) -> PyResult<String> {
        self.getDictCopyright(py)
    }

    fn clone(&self, py: Python<'_>) -> PyResult<Self> {
        Ok(Self {
            _morfeusz_obj: Py::new(py, self.low(py)?.clone())?,
            expand_dag: self.expand_dag,
            expand_tags: self.expand_tags,
            expand_dot: self.expand_dot,
            expand_underscore: self.expand_underscore,
        })
    }

    fn analyse(&self, py: Python<'_>, text: &str) -> PyResult<Py<PyAny>> {
        let items = self.analyse_one(py, text)?;
        let low = self.low(py)?;
        self.build_analysis_object(py, low.inner.id_resolver(), items)
    }

    /// Analyse many texts in parallel and return one analysis list per input
    /// text (in input order), byte-identical to a serial `analyse()` loop. Work
    /// fans out across a work-stealing pool: each worker forks its own analyzer
    /// (the dictionary is shared, the decode cache is private) with the GIL
    /// released. On free-threaded (no-GIL) builds the per-text result objects
    /// are also built on the workers, so Python-object construction parallelizes
    /// too; on GIL builds the analysis runs in parallel and construction is
    /// serial.
    ///
    /// Each text is numbered independently from node 0 (i.e. SEPARATE
    /// numbering), since the texts are processed concurrently; for the default
    /// numbering every element equals `analyse(text)`. Pool size follows
    /// `RAYON_NUM_THREADS` (default: all cores).
    fn analyse_many(&self, py: Python<'_>, texts: Vec<String>) -> PyResult<Py<PyAny>> {
        self.analyse_many_impl(py, texts)
    }

    fn analyse_iter(&mut self, py: Python<'_>, text: &str) -> PyResult<PyResultsIterator> {
        self.low_mut(py)?
            .inner
            .analyse_iter(text)
            .map(|inner| PyResultsIterator { inner })
            .map_err(to_py_err)
    }

    fn _analyseAsIterator(&mut self, py: Python<'_>, text: &str) -> PyResult<PyResultsIterator> {
        self.analyse_iter(py, text)
    }

    #[pyo3(signature = (lemma, tagId=None))]
    fn generate(&self, py: Python<'_>, lemma: &str, tagId: Option<i32>) -> PyResult<Py<PyAny>> {
        let items = {
            let low = self.low(py)?;
            match tagId {
                Some(tag_id) => low.inner.generate_by_tag_id(lemma, tag_id),
                None => low.inner.generate(lemma),
            }
            .map_err(to_py_err)?
        };
        let mut tuples = self.interp_tuples(py, items)?;
        if self.expand_tags {
            tuples = self.expand_tags_for_interps(tuples);
        }
        let python_tuples: Vec<PyInterpTuple> =
            tuples.into_iter().map(ExpandedInterp::into_tuple).collect();
        Ok(PyList::new(py, python_tuples)?.into_any().unbind())
    }

    fn _expand_tag(&self, tag: &str) -> Vec<String> {
        self.expand_tag(tag)
    }

    fn _expand_interp(&self, interp: PyInterpTuple) -> Vec<PyInterpTuple> {
        self.expand_interp_tags(expanded_interp_from_tuple(interp))
            .into_iter()
            .map(ExpandedInterp::into_tuple)
            .collect()
    }

    #[staticmethod]
    fn _dag_to_list(interps: Vec<PyAnalysisTuple>) -> Vec<Vec<PyInterpTuple>> {
        let tuples = interps
            .into_iter()
            .map(|(start, end, interp)| (start, end, expanded_interp_from_tuple(interp)))
            .collect();
        dag_to_lists(tuples)
    }

    fn _interp2tuple(
        &self,
        py: Python<'_>,
        interp: PyRef<'_, PyMorphInterpretation>,
    ) -> PyResult<PyInterpTuple> {
        let resolver = self.low(py)?.inner.id_resolver().clone();
        self.interp_tuple(&resolver, morph_interpretation_from_py(&interp))
            .map(ExpandedInterp::into_tuple)
    }

    fn _generateByTagId(
        &self,
        py: Python<'_>,
        lemma: &str,
        tagId: i32,
    ) -> PyResult<Vec<PyMorphInterpretation>> {
        self.low(py)?
            .inner
            .generate_by_tag_id(lemma, tagId)
            .map(|items| items.into_iter().map(Into::into).collect())
            .map_err(to_py_err)
    }

    fn setCharset(&mut self, py: Python<'_>, option: i32) -> PyResult<()> {
        self.low_mut(py)?
            .inner
            .set_charset(charset_from_i32(option)?);
        Ok(())
    }

    fn getCharset(&self, py: Python<'_>) -> PyResult<i32> {
        Ok(self.low(py)?.inner.charset() as i32)
    }

    fn setAggl(&mut self, py: Python<'_>, option: &str) -> PyResult<()> {
        self.low_mut(py)?.inner.set_aggl(option).map_err(to_py_err)
    }

    fn getAggl(&self, py: Python<'_>) -> PyResult<String> {
        Ok(self.low(py)?.inner.aggl().to_owned())
    }

    fn setPraet(&mut self, py: Python<'_>, option: &str) -> PyResult<()> {
        self.low_mut(py)?.inner.set_praet(option).map_err(to_py_err)
    }

    fn getPraet(&self, py: Python<'_>) -> PyResult<String> {
        Ok(self.low(py)?.inner.praet().to_owned())
    }

    fn setCaseHandling(&mut self, py: Python<'_>, option: i32) -> PyResult<()> {
        self.low_mut(py)?
            .inner
            .set_case_handling(case_handling_from_i32(option)?);
        Ok(())
    }

    fn getCaseHandling(&self, py: Python<'_>) -> PyResult<i32> {
        Ok(self.low(py)?.inner.case_handling() as i32)
    }

    fn setTokenNumbering(&mut self, py: Python<'_>, option: i32) -> PyResult<()> {
        self.low_mut(py)?
            .inner
            .set_token_numbering(token_numbering_from_i32(option)?);
        Ok(())
    }

    fn getTokenNumbering(&self, py: Python<'_>) -> PyResult<i32> {
        Ok(self.low(py)?.inner.token_numbering() as i32)
    }

    fn setWhitespaceHandling(&mut self, py: Python<'_>, option: i32) -> PyResult<()> {
        self.low_mut(py)?
            .inner
            .set_whitespace_handling(whitespace_handling_from_i32(option)?);
        Ok(())
    }

    fn getWhitespaceHandling(&self, py: Python<'_>) -> PyResult<i32> {
        Ok(self.low(py)?.inner.whitespace_handling() as i32)
    }

    fn setDebug(&mut self, py: Python<'_>, debug: bool) -> PyResult<()> {
        self.low_mut(py)?.inner.set_debug(debug);
        Ok(())
    }

    fn getIdResolver(&self, py: Python<'_>) -> PyResult<PyIdResolver> {
        Ok(PyIdResolver {
            inner: self.low(py)?.inner.id_resolver().clone(),
        })
    }

    fn setDictionary(&mut self, py: Python<'_>, dictName: &str) -> PyResult<()> {
        set_core_dictionary_preserving_options(&mut self.low_mut(py)?.inner, dictName)
    }

    fn addDictionaryPath(&self, dict_path: &str) {
        add_dictionary_search_path(Path::new(dict_path));
    }

    fn add_dictionary_path(&self, dict_path: &str) {
        self.addDictionaryPath(dict_path);
    }

    fn getAvailableAgglOptions(&self, py: Python<'_>) -> PyResult<Vec<String>> {
        Ok(self
            .low(py)?
            .inner
            .available_aggl_options()
            .iter()
            .map(ToString::to_string)
            .collect())
    }

    fn getAvailablePraetOptions(&self, py: Python<'_>) -> PyResult<Vec<String>> {
        Ok(self
            .low(py)?
            .inner
            .available_praet_options()
            .iter()
            .map(ToString::to_string)
            .collect())
    }
}

type PyInterpTuple = (String, String, String, Vec<String>, Vec<String>);
type PyAnalysisTuple = (i32, i32, PyInterpTuple);

#[derive(Clone)]
struct ExpandedInterp {
    orth: String,
    lemma: String,
    tag: String,
    names: Vec<String>,
    labels: Vec<String>,
}

impl ExpandedInterp {
    fn into_tuple(self) -> PyInterpTuple {
        (self.orth, self.lemma, self.tag, self.names, self.labels)
    }
}

impl PyMorfeusz {
    fn from_inner(py: Python<'_>, inner: CoreMorfeusz) -> PyResult<Self> {
        Ok(Self {
            _morfeusz_obj: Py::new(py, PyLowMorfeusz::from_inner(inner))?,
            expand_dag: false,
            expand_tags: false,
            expand_dot: true,
            expand_underscore: true,
        })
    }

    fn low<'py>(&self, py: Python<'py>) -> PyResult<PyRef<'py, PyLowMorfeusz>> {
        self._morfeusz_obj
            .bind(py)
            .try_borrow()
            .map_err(|_| PyRuntimeError::new_err("Morfeusz low-level object is already borrowed"))
    }

    fn low_mut<'py>(&self, py: Python<'py>) -> PyResult<PyRefMut<'py, PyLowMorfeusz>> {
        self._morfeusz_obj
            .bind(py)
            .try_borrow_mut()
            .map_err(|_| PyRuntimeError::new_err("Morfeusz low-level object is already borrowed"))
    }

    /// Run the core analysis with the GIL released so other Python threads make
    /// progress during the CPU-bound work. SEPARATE numbering (the default) uses
    /// the stateless `&self` path (`analyse_from(.., 0)`), so concurrent calls on
    /// a shared instance aren't serialized at the borrow level; CONTINUOUS
    /// numbering threads the per-instance node counter and keeps the `&mut`
    /// session path.
    fn analyse_one(&self, py: Python<'_>, text: &str) -> PyResult<Vec<MorphInterpretation>> {
        let continuous = self.low(py)?.inner.token_numbering() == TokenNumbering::Continuous;
        if continuous {
            let mut low = self.low_mut(py)?;
            let inner = &mut low.inner;
            py.detach(|| inner.analyse(text)).map_err(to_py_err)
        } else {
            let low = self.low(py)?;
            let inner = &low.inner;
            py.detach(|| inner.analyse_from(text, 0).map(|(items, _)| items))
                .map_err(to_py_err)
        }
    }

    /// Build the Python analysis object from raw interpretations: a list of
    /// `(start, end, (orth, lemma, tag, names, labels))` tuples, or a DAG of
    /// those when `expand_dag`, after optional tag expansion. Shared by
    /// `analyse` and `analyse_many`. `resolver` is borrowed from the low-level
    /// instance (cloning it per call — it owns the full id tables — was the
    /// dominant Python-binding cost).
    fn build_analysis_object(
        &self,
        py: Python<'_>,
        resolver: &IdResolver,
        items: Vec<MorphInterpretation>,
    ) -> PyResult<Py<PyAny>> {
        let mut tuples: Vec<(i32, i32, ExpandedInterp)> = items
            .into_iter()
            .map(|item| {
                let (start, end) = (item.start_node, item.end_node);
                self.interp_tuple(resolver, item)
                    .map(|interp| (start, end, interp))
            })
            .collect::<PyResult<_>>()?;
        if self.expand_tags {
            tuples = self.expand_analysis_tags(tuples);
        }
        if self.expand_dag {
            let paths = dag_to_lists(tuples);
            Ok(PyList::new(py, paths)?.into_any().unbind())
        } else {
            let python_tuples: Vec<PyAnalysisTuple> = tuples
                .into_iter()
                .map(|(start, end, interp)| (start, end, interp.into_tuple()))
                .collect();
            Ok(PyList::new(py, python_tuples)?.into_any().unbind())
        }
    }

    /// `analyse_many` on a GIL interpreter: analysis fans out across the pool
    /// with the GIL released, then the result objects are built serially (object
    /// construction needs the GIL, so parallelizing it would only serialize on
    /// the GIL and add overhead).
    #[cfg(not(Py_GIL_DISABLED))]
    fn analyse_many_impl(&self, py: Python<'_>, texts: Vec<String>) -> PyResult<Py<PyAny>> {
        let raw: Vec<Vec<MorphInterpretation>> = {
            let low = self.low(py)?;
            let core = &low.inner;
            py.detach(|| {
                texts
                    .par_iter()
                    .map_init(
                        || core.fork(),
                        |local, text| local.analyse_from(text, 0).map(|(items, _)| items),
                    )
                    .collect::<Result<Vec<_>, _>>()
            })
            .map_err(to_py_err)?
        };

        let low = self.low(py)?;
        let resolver = low.inner.id_resolver();
        let objects = raw
            .into_iter()
            .map(|items| self.build_analysis_object(py, resolver, items))
            .collect::<PyResult<Vec<_>>>()?;
        Ok(PyList::new(py, objects)?.into_any().unbind())
    }

    /// `analyse_many` on a free-threaded (no-GIL) interpreter: each worker
    /// analyses *and* builds its text's result objects while attached to the
    /// interpreter, so object construction parallelizes too. Multiple threads
    /// attach concurrently without a GIL; the main thread stays attached and
    /// joins (no deadlock, since there is no single lock to contend). rayon's
    /// ordered `collect` keeps results in input order — identical to the serial
    /// build.
    #[cfg(Py_GIL_DISABLED)]
    fn analyse_many_impl(&self, py: Python<'_>, texts: Vec<String>) -> PyResult<Py<PyAny>> {
        let low = self.low(py)?;
        let core = &low.inner;
        let resolver = low.inner.id_resolver();
        // Process the batch in CHUNKS: each worker forks once and `attach`es
        // once per chunk, then analyses and builds that chunk's result objects
        // under the single attach. Per-text attach (acquire/release thread
        // state every line) dominated and made it slower than serial; chunking
        // amortizes both the attach and the fork while keeping object
        // construction parallel across workers.
        //
        // The main thread DETACHES while workers run: otherwise a worker
        // allocation that triggers a stop-the-world (GC) would wait for the
        // main thread to reach a safe point, which never happens in native
        // rayon code — deadlock.
        // Aim for ~4 chunks per worker (load balance), with a floor so small
        // batches don't pay one fork + attach per handful of texts.
        let chunk_size = texts
            .len()
            .div_ceil((rayon::current_num_threads() * 4).max(1))
            .max(64);
        let nested: Vec<Vec<Py<PyAny>>> = py.detach(|| {
            texts
                .par_chunks(chunk_size)
                .map(|chunk| {
                    let mut local = core.fork();
                    Python::attach(|py| {
                        chunk
                            .iter()
                            .map(|text| {
                                let items = local
                                    .analyse_from(text, 0)
                                    .map(|(items, _)| items)
                                    .map_err(to_py_err)?;
                                self.build_analysis_object(py, resolver, items)
                            })
                            .collect::<PyResult<Vec<_>>>()
                    })
                })
                .collect::<PyResult<Vec<_>>>()
        })?;
        let objects: Vec<Py<PyAny>> = nested.into_iter().flatten().collect();
        Ok(PyList::new(py, objects)?.into_any().unbind())
    }

    fn interp_tuples(
        &self,
        py: Python<'_>,
        items: Vec<MorphInterpretation>,
    ) -> PyResult<Vec<ExpandedInterp>> {
        let low = self.low(py)?;
        let resolver = low.inner.id_resolver();
        items
            .into_iter()
            .map(|item| self.interp_tuple(resolver, item))
            .collect()
    }

    fn interp_tuple(
        &self,
        resolver: &IdResolver,
        item: MorphInterpretation,
    ) -> PyResult<ExpandedInterp> {
        let tag = item
            .tag(resolver)
            .map(ToOwned::to_owned)
            .ok_or_else(|| PyRuntimeError::new_err(format!("Invalid tag id: {}", item.tag_id)))?;
        let names = item
            .name(resolver)
            .map(split_optional_id_string)
            .ok_or_else(|| PyRuntimeError::new_err(format!("Invalid name id: {}", item.name_id)))?;
        let labels = item
            .labels(resolver)
            .map(labels_set_to_vec)
            .ok_or_else(|| {
                PyRuntimeError::new_err(format!("Invalid labels id: {}", item.labels_id))
            })?;

        Ok(ExpandedInterp {
            orth: item.orth,
            lemma: item.lemma,
            tag,
            names,
            labels,
        })
    }

    fn expand_analysis_tags(
        &self,
        tuples: Vec<(i32, i32, ExpandedInterp)>,
    ) -> Vec<(i32, i32, ExpandedInterp)> {
        tuples
            .into_iter()
            .flat_map(|(start, end, interp)| {
                self.expand_interp_tags(interp)
                    .into_iter()
                    .map(move |interp| (start, end, interp))
            })
            .collect()
    }

    fn expand_tags_for_interps(&self, tuples: Vec<ExpandedInterp>) -> Vec<ExpandedInterp> {
        tuples
            .into_iter()
            .flat_map(|interp| self.expand_interp_tags(interp))
            .collect()
    }

    fn expand_interp_tags(&self, interp: ExpandedInterp) -> Vec<ExpandedInterp> {
        self.expand_tag(&interp.tag)
            .into_iter()
            .map(|tag| ExpandedInterp {
                orth: interp.orth.clone(),
                lemma: interp.lemma.clone(),
                tag,
                names: interp.names.clone(),
                labels: interp.labels.clone(),
            })
            .collect()
    }

    fn expand_tag(&self, tag: &str) -> Vec<String> {
        let chunks: Vec<Vec<String>> = tag
            .split(':')
            .map(|chunk| {
                if chunk == "_" && self.expand_underscore {
                    vec!["m1", "m2", "m3", "f", "n"]
                        .into_iter()
                        .map(ToOwned::to_owned)
                        .collect()
                } else {
                    chunk.split('.').map(ToOwned::to_owned).collect()
                }
            })
            .collect();

        if !self.expand_dot {
            return vec![chunks
                .iter()
                .map(|values| values.join("."))
                .collect::<Vec<_>>()
                .join(":")];
        }

        let mut variants = vec![Vec::<String>::new()];
        for chunk in chunks {
            let mut next = Vec::new();
            for prefix in &variants {
                for value in &chunk {
                    let mut expanded = prefix.clone();
                    expanded.push(value.clone());
                    next.push(expanded);
                }
            }
            variants = next;
        }
        variants
            .into_iter()
            .map(|variant| variant.join(":"))
            .collect()
    }
}

fn dag_to_lists(tuples: Vec<(i32, i32, ExpandedInterp)>) -> Vec<Vec<PyInterpTuple>> {
    let mut dag: BTreeMap<i32, Vec<(ExpandedInterp, i32)>> = BTreeMap::new();
    for (start, end, interp) in tuples {
        dag.entry(start).or_default().push((interp, end));
    }
    expand_dag_from(0, &dag)
        .into_iter()
        .map(|path| path.into_iter().map(ExpandedInterp::into_tuple).collect())
        .collect()
}

fn expand_dag_from(
    start: i32,
    dag: &BTreeMap<i32, Vec<(ExpandedInterp, i32)>>,
) -> Vec<Vec<ExpandedInterp>> {
    let Some(nexts) = dag.get(&start) else {
        return vec![Vec::new()];
    };

    let mut result = Vec::new();
    for (head, end) in nexts {
        for tail in expand_dag_from(*end, dag) {
            let mut path = Vec::with_capacity(tail.len() + 1);
            path.push(head.clone());
            path.extend(tail);
            result.push(path);
        }
    }
    result
}

fn expanded_interp_from_tuple(interp: PyInterpTuple) -> ExpandedInterp {
    let (orth, lemma, tag, names, labels) = interp;
    ExpandedInterp {
        orth,
        lemma,
        tag,
        names,
        labels,
    }
}

fn morph_interpretation_from_py(interp: &PyMorphInterpretation) -> MorphInterpretation {
    MorphInterpretation {
        start_node: interp.startNode,
        end_node: interp.endNode,
        orth: interp.orth.clone(),
        lemma: interp.lemma.clone(),
        tag_id: interp.tagId,
        name_id: interp.nameId,
        labels_id: interp.labelsId,
    }
}

fn load_core_morfeusz(
    dictionary_path: Option<&str>,
    tagset_path: Option<&str>,
    usage: MorfeuszUsage,
) -> PyResult<CoreMorfeusz> {
    let Some(path) = dictionary_path else {
        return Ok(CoreMorfeusz::with_dictionary(Dictionary::empty(), usage));
    };

    if is_binary_dictionary_path(path) {
        if tagset_path.is_some() {
            return Err(PyValueError::new_err(
                "tagset_path is only supported for TSV dictionaries",
            ));
        }
        return BinaryDictionaryRepository::default()
            .load_path(Path::new(path), usage)
            .map_err(to_py_err);
    }

    let tagset = tagset_path.map(Path::new);
    let dictionary = TsvLexiconLoader::from_paths(Path::new(path), tagset).map_err(to_py_err)?;
    Ok(CoreMorfeusz::with_dictionary(dictionary, usage))
}

fn load_named_binary_core_morfeusz_for_create(
    dict_name: &str,
    usage: MorfeuszUsage,
) -> PyResult<CoreMorfeusz> {
    binary_dictionary_repository_from_search_paths()
        .load_named(dict_name, usage)
        .map_err(to_py_create_err)
}

fn load_create_instance_core(
    dict_name: Option<&str>,
    usage: MorfeuszUsage,
) -> PyResult<CoreMorfeusz> {
    if let Some(path) = dict_name {
        if is_dictionary_path(path) {
            return load_core_morfeusz(Some(path), None, usage);
        }
        return load_named_binary_core_morfeusz_for_create(path, usage);
    }
    load_named_binary_core_morfeusz_for_create(CoreMorfeusz::default_dict_name(), usage)
}

fn parse_create_instance_args(
    dict_name_arg: Option<&Bound<'_, PyAny>>,
    usage: i32,
) -> PyResult<(Option<String>, MorfeuszUsage)> {
    let Some(arg) = dict_name_arg else {
        return Ok((None, morfeusz_usage_from_i32(usage)?));
    };

    if arg.is_none() {
        return Ok((None, morfeusz_usage_from_i32(usage)?));
    }

    if let Ok(dict_name) = arg.extract::<String>() {
        return Ok((Some(dict_name), morfeusz_usage_from_i32(usage)?));
    }

    if let Ok(legacy_usage) = arg.extract::<i32>() {
        if usage != MorfeuszUsage::BothAnalyseAndGenerate as i32 {
            return Err(PyTypeError::new_err(
                "createInstance usage cannot be passed both positionally and as the second argument",
            ));
        }
        return Ok((None, morfeusz_usage_from_i32(legacy_usage)?));
    }

    Err(PyTypeError::new_err(
        "createInstance expects a dictionary name/path string or Morfeusz usage integer",
    ))
}

fn set_core_dictionary_preserving_options(
    inner: &mut CoreMorfeusz,
    dict_name: &str,
) -> PyResult<()> {
    if is_binary_dictionary_path(dict_name) {
        inner
            .set_dictionary_path_with_repository(
                &BinaryDictionaryRepository::default(),
                Path::new(dict_name),
            )
            .map_err(to_py_err)?;
    } else if Path::new(dict_name).exists() {
        let dictionary =
            TsvLexiconLoader::from_paths(Path::new(dict_name), None::<&Path>).map_err(to_py_err)?;
        inner.set_dictionary(dictionary);
    } else {
        let repository = binary_dictionary_repository_from_search_paths();
        inner
            .set_dictionary_named_with_repository(&repository, dict_name)
            .map_err(to_py_err)?;
    }
    Ok(())
}

fn binary_dictionary_repository_from_search_paths() -> BinaryDictionaryRepository {
    let search_dirs = dictionary_search_paths()
        .lock()
        .map(|paths| paths.clone())
        .unwrap_or_else(|_| vec![PathBuf::from(".")]);
    BinaryDictionaryRepository::new(search_dirs)
}

fn with_py_morfeusz_resolver<R>(
    morfeusz: &Bound<'_, PyAny>,
    f: impl FnOnce(&IdResolver) -> PyResult<R>,
) -> PyResult<R> {
    if let Ok(low) = morfeusz.extract::<PyRef<'_, PyLowMorfeusz>>() {
        return f(low.inner.id_resolver());
    }
    if let Ok(high) = morfeusz.extract::<PyRef<'_, PyMorfeusz>>() {
        let low = high.low(morfeusz.py())?;
        return f(low.inner.id_resolver());
    }
    Err(PyValueError::new_err(
        "Expected morfeusz2.Morfeusz or morfeusz2._Morfeusz instance",
    ))
}

fn effective_usage(usage: i32, analyse: bool, generate: bool) -> PyResult<MorfeuszUsage> {
    match (analyse, generate) {
        (false, false) => Err(PyValueError::new_err(
            "At least one of \"analyse\" and \"generate\" must be True",
        )),
        (true, true) => morfeusz_usage_from_i32(usage),
        (true, false) => Ok(MorfeuszUsage::AnalyseOnly),
        (false, true) => Ok(MorfeuszUsage::GenerateOnly),
    }
}

fn dictionary_search_paths() -> &'static Mutex<Vec<PathBuf>> {
    static PATHS: OnceLock<Mutex<Vec<PathBuf>>> = OnceLock::new();
    PATHS.get_or_init(|| Mutex::new(vec![PathBuf::from(".")]))
}

fn add_dictionary_search_path(path: &Path) {
    if let Ok(mut paths) = dictionary_search_paths().lock() {
        let path = path.to_path_buf();
        if !paths.iter().any(|existing| existing == &path) {
            paths.insert(0, path);
        }
    }
}

#[pyfunction(name = "_Morfeusz_dictionarySearchPaths_get")]
fn py_dictionary_search_paths_get() -> Vec<String> {
    dictionary_search_paths()
        .lock()
        .map(|paths| {
            paths
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default()
}

#[pyfunction(name = "_Morfeusz_dictionarySearchPaths_set")]
fn py_dictionary_search_paths_set(paths: Vec<String>) {
    if let Ok(mut search_paths) = dictionary_search_paths().lock() {
        *search_paths = paths.into_iter().map(PathBuf::from).collect();
    }
}

#[pyfunction(name = "_Morfeusz_getVersion")]
fn py_morfeusz_get_version() -> String {
    CoreMorfeusz::version().to_owned()
}

#[pyfunction(name = "_Morfeusz_getDefaultDictName")]
fn py_morfeusz_get_default_dict_name() -> String {
    CoreMorfeusz::default_dict_name().to_owned()
}

#[pyfunction(name = "_Morfeusz_getCopyright")]
fn py_morfeusz_get_copyright() -> String {
    CoreMorfeusz::copyright().to_owned()
}

fn is_dictionary_path(path: &str) -> bool {
    is_binary_dictionary_path(path) || Path::new(path).exists()
}

fn is_binary_dictionary_path(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("dict"))
}

fn split_optional_id_string(value: &str) -> Vec<String> {
    if value.is_empty() || value == "_" {
        Vec::new()
    } else {
        value.split('|').map(ToOwned::to_owned).collect()
    }
}

fn labels_set_to_vec(labels: &BTreeSet<String>) -> Vec<String> {
    labels.iter().cloned().collect()
}

fn to_py_err(err: Error) -> PyErr {
    match err {
        Error::Io(_) | Error::InvalidDictionary(_) | Error::NotFound(_) => {
            PyIOError::new_err(err.to_string())
        }
        Error::InvalidArgument(_) | Error::OutOfRange(_) | Error::Unsupported(_) => {
            PyRuntimeError::new_err(err.to_string())
        }
    }
}

fn to_py_create_err(err: Error) -> PyErr {
    match err {
        Error::NotFound(_) => PyRuntimeError::new_err(err.to_string()),
        Error::Io(_) | Error::InvalidDictionary(_) => PyIOError::new_err(err.to_string()),
        Error::InvalidArgument(_) | Error::OutOfRange(_) | Error::Unsupported(_) => {
            PyRuntimeError::new_err(err.to_string())
        }
    }
}

fn charset_from_i32(value: i32) -> PyResult<Charset> {
    match value {
        11 => Ok(Charset::Utf8),
        12 => Ok(Charset::Iso8859_2),
        13 => Ok(Charset::Cp1250),
        14 => Ok(Charset::Cp852),
        _ => Err(PyValueError::new_err("Invalid charset option")),
    }
}

fn case_handling_from_i32(value: i32) -> PyResult<CaseHandling> {
    match value {
        100 => Ok(CaseHandling::ConditionallyCaseSensitive),
        101 => Ok(CaseHandling::StrictlyCaseSensitive),
        102 => Ok(CaseHandling::IgnoreCase),
        _ => Err(PyValueError::new_err("Invalid caseHandling option")),
    }
}

fn token_numbering_from_i32(value: i32) -> PyResult<TokenNumbering> {
    match value {
        201 => Ok(TokenNumbering::Separate),
        202 => Ok(TokenNumbering::Continuous),
        _ => Err(PyValueError::new_err("Invalid tokenNumbering option")),
    }
}

fn whitespace_handling_from_i32(value: i32) -> PyResult<WhitespaceHandling> {
    match value {
        301 => Ok(WhitespaceHandling::Skip),
        302 => Ok(WhitespaceHandling::Append),
        303 => Ok(WhitespaceHandling::Keep),
        _ => Err(PyValueError::new_err("Invalid whitespaceHandling option")),
    }
}

fn morfeusz_usage_from_i32(value: i32) -> PyResult<MorfeuszUsage> {
    match value {
        401 => Ok(MorfeuszUsage::AnalyseOnly),
        402 => Ok(MorfeuszUsage::GenerateOnly),
        403 => Ok(MorfeuszUsage::BothAnalyseAndGenerate),
        _ => Err(PyValueError::new_err("Invalid Morfeusz usage option")),
    }
}

// `gil_used = false` opts this module into free-threaded (no-GIL) CPython
// 3.13t/3.14t. It is sound here: the only module-global state
// (`dictionary_search_paths`) is a `OnceLock<Mutex<..>>`, and per-instance
// `#[pyclass]` mutability is guarded by pyo3's runtime borrow checking, which
// remains in force without the GIL.
#[pymodule(gil_used = false)]
fn morfeusz2(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyMorphInterpretation>()?;
    m.add_class::<PyResultsIterator>()?;
    m.add_class::<PyLowMorfeusz>()?;
    m.add_class::<PyIdResolver>()?;
    m.add_class::<PyMorfeusz>()?;
    m.add_function(wrap_pyfunction!(py_morfeusz_get_version, m)?)?;
    m.add_function(wrap_pyfunction!(py_morfeusz_get_default_dict_name, m)?)?;
    m.add_function(wrap_pyfunction!(py_morfeusz_get_copyright, m)?)?;
    m.add_function(wrap_pyfunction!(py_dictionary_search_paths_get, m)?)?;
    m.add_function(wrap_pyfunction!(py_dictionary_search_paths_set, m)?)?;

    m.add("UTF8", Charset::Utf8 as i32)?;
    m.add("ISO8859_2", Charset::Iso8859_2 as i32)?;
    m.add("CP1250", Charset::Cp1250 as i32)?;
    m.add("CP852", Charset::Cp852 as i32)?;

    m.add(
        "CONDITIONALLY_CASE_SENSITIVE",
        CaseHandling::ConditionallyCaseSensitive as i32,
    )?;
    m.add(
        "STRICTLY_CASE_SENSITIVE",
        CaseHandling::StrictlyCaseSensitive as i32,
    )?;
    m.add("IGNORE_CASE", CaseHandling::IgnoreCase as i32)?;

    m.add("SEPARATE_NUMBERING", TokenNumbering::Separate as i32)?;
    m.add("CONTINUOUS_NUMBERING", TokenNumbering::Continuous as i32)?;

    m.add("SKIP_WHITESPACES", WhitespaceHandling::Skip as i32)?;
    m.add("APPEND_WHITESPACES", WhitespaceHandling::Append as i32)?;
    m.add("KEEP_WHITESPACES", WhitespaceHandling::Keep as i32)?;

    m.add("ANALYSE_ONLY", MorfeuszUsage::AnalyseOnly as i32)?;
    m.add("GENERATE_ONLY", MorfeuszUsage::GenerateOnly as i32)?;
    m.add(
        "BOTH_ANALYSE_AND_GENERATE",
        MorfeuszUsage::BothAnalyseAndGenerate as i32,
    )?;
    m.add("__version__", CoreMorfeusz::version())?;
    m.add("__copyright__", CoreMorfeusz::copyright())?;
    m.add("GENDERS", vec!["m1", "m2", "m3", "f", "n"])?;
    m.add("InterpsList", m.py().get_type::<PyList>())?;
    m.add("StringsList", m.py().get_type::<PyList>())?;
    m.add("StringsLinkedList", m.py().get_type::<PyList>())?;
    m.add("StringsSet", m.py().get_type::<PySet>())?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_set_to_vec_uses_swig_std_set_order() {
        let labels = ["zzz", "aaa", "aaa", "euro"]
            .into_iter()
            .map(ToOwned::to_owned)
            .collect::<BTreeSet<_>>();

        assert_eq!(labels_set_to_vec(&labels), ["aaa", "euro", "zzz"]);
    }
}
