use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};
use std::path::PathBuf;
use std::ptr;
use std::sync::{Mutex, OnceLock};

use morfeusz::charset::{decode_lossy, encode_lossy};
use morfeusz::{
    BinaryDictionaryRepository, CaseHandling, Charset, Morfeusz, MorfeuszUsage,
    MorphInterpretation, TokenNumbering, WhitespaceHandling,
};

const MORFOPT_ENCODING: c_int = 1;
const MORFOPT_WHITESPACE: c_int = 2;
const MORFOPT_CASE: c_int = 3;
const MORFOPT_TOKEN_NUMBERING: c_int = 4;

const MORFEUSZ_UTF_8: c_int = 8;
const MORFEUSZ_ISO8859_2: c_int = 88592;
const MORFEUSZ_CP1250: c_int = 1250;
const MORFEUSZ_CP852: c_int = 852;

const MORFEUSZ_SKIP_WHITESPACE: c_int = 0;
const MORFEUSZ_KEEP_WHITESPACE: c_int = 2;
const MORFEUSZ_APPEND_WHITESPACE: c_int = 4;

const MORFEUSZ_WEAK_CASE: c_int = 301;
const MORFEUSZ_STRICT_CASE: c_int = 302;
const MORFEUSZ_IGNORE_CASE: c_int = 303;

const MORFEUSZ_SEPARATE_TOKEN_NUMBERING: c_int = 401;
const MORFEUSZ_CONTINUOUS_TOKEN_NUMBERING: c_int = 402;

#[repr(C)]
pub struct InterpMorf {
    pub p: c_int,
    pub k: c_int,
    pub forma: *mut c_char,
    pub haslo: *mut c_char,
    pub interp: *mut c_char,
}

#[repr(C)]
pub struct MorfeuszInterp {
    pub start_node: c_int,
    pub end_node: c_int,
    pub orth: *const c_char,
    pub lemma: *const c_char,
    pub tag_id: c_int,
    pub name_id: c_int,
    pub labels_id: c_int,
}

pub struct MorfeuszOwnedInterps {
    strings: Vec<CString>,
    interps: Vec<MorfeuszInterp>,
}

pub struct MorfeuszInstance {
    morfeusz: Morfeusz,
    scratch: CString,
    last_error: CString,
}

// The raw pointers reference CString storage owned by the same mutex-protected
// CApiState, matching the legacy global C API result lifetime.
unsafe impl Send for InterpMorf {}

struct CApiState {
    morfeusz: Morfeusz,
    strings: Vec<CString>,
    results: Vec<InterpMorf>,
}

impl CApiState {
    fn new() -> Self {
        Self {
            morfeusz: default_c_api_morfeusz(),
            strings: Vec::new(),
            results: sentinel_results(),
        }
    }

    fn reset_results(&mut self) {
        self.strings.clear();
        self.results.clear();
    }

    fn push_c_string(&mut self, value: &str, charset: Charset) -> *mut c_char {
        let mut bytes = encode_lossy(charset, value);
        for byte in &mut bytes {
            if *byte == 0 {
                *byte = b' ';
            }
        }
        let c_string = CString::new(bytes).unwrap_or_default();
        self.strings.push(c_string);
        self.strings
            .last()
            .map(|value| value.as_ptr() as *mut c_char)
            .unwrap_or(ptr::null_mut())
    }

    fn set_sentinel(&mut self) {
        self.results.push(InterpMorf {
            p: -1,
            k: -1,
            forma: ptr::null_mut(),
            haslo: ptr::null_mut(),
            interp: ptr::null_mut(),
        });
    }

    fn convert_results(&mut self, results: Vec<MorphInterpretation>) -> *mut InterpMorf {
        self.reset_results();
        let charset = self.morfeusz.charset();
        for interp in results {
            let forma = self.push_c_string(&interp.orth, charset);
            let haslo = self.push_c_string(&interp.lemma, charset);
            let tag = interp
                .tag(self.morfeusz.id_resolver())
                .unwrap_or("ign")
                .to_owned();
            let interp_tag = self.push_c_string(&tag, charset);
            self.results.push(InterpMorf {
                p: interp.start_node,
                k: interp.end_node,
                forma,
                haslo,
                interp: interp_tag,
            });
        }
        self.set_sentinel();
        self.results.as_mut_ptr()
    }

    fn set_empty_results(&mut self) -> *mut InterpMorf {
        self.reset_results();
        self.set_sentinel();
        self.results.as_mut_ptr()
    }
}

impl MorfeuszOwnedInterps {
    fn from_results(results: Vec<MorphInterpretation>, charset: Charset) -> Self {
        let mut owned = Self {
            strings: Vec::with_capacity(results.len() * 2),
            interps: Vec::with_capacity(results.len()),
        };
        for interp in results {
            let orth = owned.push_c_string(&interp.orth, charset);
            let lemma = owned.push_c_string(&interp.lemma, charset);
            owned.interps.push(MorfeuszInterp {
                start_node: interp.start_node,
                end_node: interp.end_node,
                orth,
                lemma,
                tag_id: interp.tag_id,
                name_id: interp.name_id,
                labels_id: interp.labels_id,
            });
        }
        owned
    }

    fn push_c_string(&mut self, value: &str, charset: Charset) -> *const c_char {
        self.strings
            .push(cstring_from_bytes(encode_lossy(charset, value)));
        self.strings
            .last()
            .map(|value| value.as_ptr())
            .unwrap_or(ptr::null())
    }
}

impl MorfeuszInstance {
    fn new(morfeusz: Morfeusz) -> Self {
        Self {
            morfeusz,
            scratch: CString::default(),
            last_error: CString::default(),
        }
    }

    fn clear_error(&mut self) {
        self.last_error = CString::default();
    }

    fn set_error(&mut self, error: impl ToString) {
        self.last_error = cstring_from_bytes(error.to_string().into_bytes());
    }

    fn store_output_string(&mut self, value: &str) -> *const c_char {
        self.scratch = cstring_from_bytes(encode_lossy(self.morfeusz.charset(), value));
        self.scratch.as_ptr()
    }

    fn convert_results(
        &mut self,
        results: morfeusz::Result<Vec<MorphInterpretation>>,
    ) -> *mut MorfeuszOwnedInterps {
        match results {
            Ok(results) => {
                self.clear_error();
                Box::into_raw(Box::new(MorfeuszOwnedInterps::from_results(
                    results,
                    self.morfeusz.charset(),
                )))
            }
            Err(err) => {
                self.set_error(err);
                ptr::null_mut()
            }
        }
    }
}

fn default_c_api_morfeusz() -> Morfeusz {
    binary_dictionary_repository_from_search_paths()
        .load_named(Morfeusz::default_dict_name(), MorfeuszUsage::AnalyseOnly)
        .unwrap_or_else(|_| {
            Morfeusz::with_dictionary(Default::default(), MorfeuszUsage::AnalyseOnly)
        })
}

fn sentinel_results() -> Vec<InterpMorf> {
    vec![InterpMorf {
        p: -1,
        k: -1,
        forma: ptr::null_mut(),
        haslo: ptr::null_mut(),
        interp: ptr::null_mut(),
    }]
}

fn cstring_from_bytes(mut bytes: Vec<u8>) -> CString {
    for byte in &mut bytes {
        if *byte == 0 {
            *byte = b' ';
        }
    }
    CString::new(bytes).unwrap_or_default()
}

fn dictionary_search_paths() -> &'static Mutex<Vec<PathBuf>> {
    static PATHS: OnceLock<Mutex<Vec<PathBuf>>> = OnceLock::new();
    PATHS.get_or_init(|| Mutex::new(vec![PathBuf::from(".")]))
}

fn dictionary_path_item_scratch() -> &'static Mutex<CString> {
    static SCRATCH: OnceLock<Mutex<CString>> = OnceLock::new();
    SCRATCH.get_or_init(|| Mutex::new(CString::default()))
}

fn global_last_error() -> &'static Mutex<CString> {
    static LAST_ERROR: OnceLock<Mutex<CString>> = OnceLock::new();
    LAST_ERROR.get_or_init(|| Mutex::new(CString::default()))
}

fn clear_global_error() {
    if let Ok(mut error) = global_last_error().lock() {
        *error = CString::default();
    }
}

fn set_global_error(error: impl ToString) {
    if let Ok(mut last_error) = global_last_error().lock() {
        *last_error = cstring_from_bytes(error.to_string().into_bytes());
    }
}

fn binary_dictionary_repository_from_search_paths() -> BinaryDictionaryRepository {
    let search_dirs = dictionary_search_paths()
        .lock()
        .map(|paths| paths.clone())
        .unwrap_or_else(|_| vec![PathBuf::from(".")]);
    BinaryDictionaryRepository::new(search_dirs)
}

unsafe fn c_str_lossy<'a>(value: *const c_char) -> Option<std::borrow::Cow<'a, str>> {
    if value.is_null() {
        None
    } else {
        Some(CStr::from_ptr(value).to_string_lossy())
    }
}

unsafe fn c_str_decoded(value: *const c_char, charset: Charset) -> Option<String> {
    if value.is_null() {
        None
    } else {
        Some(decode_lossy(charset, CStr::from_ptr(value).to_bytes()))
    }
}

fn charset_from_cpp(value: c_int) -> Option<Charset> {
    match value {
        11 => Some(Charset::Utf8),
        12 => Some(Charset::Iso8859_2),
        13 => Some(Charset::Cp1250),
        14 => Some(Charset::Cp852),
        _ => None,
    }
}

fn case_from_cpp(value: c_int) -> Option<CaseHandling> {
    match value {
        100 => Some(CaseHandling::ConditionallyCaseSensitive),
        101 => Some(CaseHandling::StrictlyCaseSensitive),
        102 => Some(CaseHandling::IgnoreCase),
        _ => None,
    }
}

fn token_numbering_from_cpp(value: c_int) -> Option<TokenNumbering> {
    match value {
        201 => Some(TokenNumbering::Separate),
        202 => Some(TokenNumbering::Continuous),
        _ => None,
    }
}

fn usage_from_cpp(value: c_int) -> Option<MorfeuszUsage> {
    match value {
        401 => Some(MorfeuszUsage::AnalyseOnly),
        402 => Some(MorfeuszUsage::GenerateOnly),
        403 => Some(MorfeuszUsage::BothAnalyseAndGenerate),
        _ => None,
    }
}

fn whitespace_from_cpp(value: c_int) -> Option<WhitespaceHandling> {
    match value {
        301 => Some(WhitespaceHandling::Skip),
        302 => Some(WhitespaceHandling::Append),
        303 => Some(WhitespaceHandling::Keep),
        _ => None,
    }
}

fn state() -> &'static Mutex<CApiState> {
    static STATE: OnceLock<Mutex<CApiState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(CApiState::new()))
}

#[no_mangle]
pub extern "C" fn morfeusz_about() -> *mut c_char {
    static ABOUT: OnceLock<CString> = OnceLock::new();
    ABOUT
        .get_or_init(|| CString::new(Morfeusz::version()).unwrap_or_default())
        .as_ptr() as *mut c_char
}

#[no_mangle]
pub extern "C" fn morfeusz_get_default_dict_name() -> *mut c_char {
    static DEFAULT_DICT_NAME: OnceLock<CString> = OnceLock::new();
    DEFAULT_DICT_NAME
        .get_or_init(|| CString::new(Morfeusz::default_dict_name()).unwrap_or_default())
        .as_ptr() as *mut c_char
}

#[no_mangle]
pub extern "C" fn morfeusz_get_copyright() -> *mut c_char {
    static COPYRIGHT: OnceLock<CString> = OnceLock::new();
    COPYRIGHT
        .get_or_init(|| CString::new(Morfeusz::copyright()).unwrap_or_default())
        .as_ptr() as *mut c_char
}

#[no_mangle]
pub extern "C" fn morfeusz_last_error() -> *const c_char {
    global_last_error()
        .lock()
        .map(|error| error.as_ptr())
        .unwrap_or(ptr::null())
}

#[no_mangle]
pub unsafe extern "C" fn morfeusz_analyse(text: *mut c_char) -> *mut InterpMorf {
    let Ok(mut state) = state().lock() else {
        return ptr::null_mut();
    };
    if text.is_null() {
        return state.set_empty_results();
    }

    let bytes = CStr::from_ptr(text).to_bytes();
    let input = decode_lossy(state.morfeusz.charset(), bytes);
    match state.morfeusz.analyse(&input) {
        Ok(results) => state.convert_results(results),
        Err(_) => state.set_empty_results(),
    }
}

#[no_mangle]
pub extern "C" fn morfeusz_set_option(option: c_int, value: c_int) -> c_int {
    let Ok(mut state) = state().lock() else {
        return 0;
    };
    match option {
        MORFOPT_ENCODING => set_encoding_option(&mut state.morfeusz, value),
        MORFOPT_WHITESPACE => set_whitespace_option(&mut state.morfeusz, value),
        MORFOPT_CASE => set_case_option(&mut state.morfeusz, value),
        MORFOPT_TOKEN_NUMBERING => set_token_numbering_option(&mut state.morfeusz, value),
        _ => {
            eprintln!("Wrong option {option}");
            0
        }
    }
}

#[no_mangle]
pub extern "C" fn morfeusz_dictionary_search_paths_clear() {
    if let Ok(mut paths) = dictionary_search_paths().lock() {
        paths.clear();
    }
}

#[no_mangle]
pub unsafe extern "C" fn morfeusz_dictionary_search_paths_push(path: *const c_char) -> c_int {
    let Some(path) = c_str_lossy(path) else {
        return 0;
    };
    if let Ok(mut paths) = dictionary_search_paths().lock() {
        paths.push(PathBuf::from(path.as_ref()));
        1
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn morfeusz_dictionary_search_paths_count() -> usize {
    dictionary_search_paths()
        .lock()
        .map(|paths| paths.len())
        .unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn morfeusz_dictionary_search_paths_item(index: usize) -> *const c_char {
    let Ok(paths) = dictionary_search_paths().lock() else {
        return ptr::null();
    };
    let Some(path) = paths.get(index) else {
        return ptr::null();
    };
    let Ok(mut scratch) = dictionary_path_item_scratch().lock() else {
        return ptr::null();
    };
    *scratch = cstring_from_bytes(path.to_string_lossy().as_bytes().to_vec());
    scratch.as_ptr()
}

#[no_mangle]
pub extern "C" fn morfeusz_create_instance(usage: c_int) -> *mut MorfeuszInstance {
    let Some(usage) = usage_from_cpp(usage) else {
        set_global_error("Invalid usage option");
        return ptr::null_mut();
    };
    match binary_dictionary_repository_from_search_paths()
        .load_named(Morfeusz::default_dict_name(), usage)
    {
        Ok(morfeusz) => {
            clear_global_error();
            Box::into_raw(Box::new(MorfeuszInstance::new(morfeusz)))
        }
        Err(err) => {
            set_global_error(err);
            ptr::null_mut()
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn morfeusz_create_instance_named(
    dict_name: *const c_char,
    usage: c_int,
) -> *mut MorfeuszInstance {
    let Some(usage) = usage_from_cpp(usage) else {
        set_global_error("Invalid usage option");
        return ptr::null_mut();
    };
    let Some(dict_name) = c_str_lossy(dict_name) else {
        set_global_error("Invalid dictionary name");
        return ptr::null_mut();
    };
    match binary_dictionary_repository_from_search_paths().load_named(&dict_name, usage) {
        Ok(morfeusz) => {
            clear_global_error();
            Box::into_raw(Box::new(MorfeuszInstance::new(morfeusz)))
        }
        Err(err) => {
            set_global_error(err);
            ptr::null_mut()
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn morfeusz_clone_instance(
    instance: *const MorfeuszInstance,
) -> *mut MorfeuszInstance {
    if instance.is_null() {
        return ptr::null_mut();
    }
    Box::into_raw(Box::new(MorfeuszInstance::new(
        (*instance).morfeusz.clone(),
    )))
}

#[no_mangle]
pub unsafe extern "C" fn morfeusz_destroy_instance(instance: *mut MorfeuszInstance) {
    if !instance.is_null() {
        drop(Box::from_raw(instance));
    }
}

#[no_mangle]
pub unsafe extern "C" fn morfeusz_instance_last_error(
    instance: *const MorfeuszInstance,
) -> *const c_char {
    if instance.is_null() {
        return ptr::null();
    }
    (*instance).last_error.as_ptr()
}

#[no_mangle]
pub unsafe extern "C" fn morfeusz_instance_get_dict_id(
    instance: *mut MorfeuszInstance,
) -> *const c_char {
    if instance.is_null() {
        return ptr::null();
    }
    let value = (*instance).morfeusz.dict_id().to_owned();
    (*instance).store_output_string(&value)
}

#[no_mangle]
pub unsafe extern "C" fn morfeusz_instance_get_dict_copyright(
    instance: *mut MorfeuszInstance,
) -> *const c_char {
    if instance.is_null() {
        return ptr::null();
    }
    let value = (*instance).morfeusz.dict_copyright().to_owned();
    (*instance).store_output_string(&value)
}

#[no_mangle]
pub unsafe extern "C" fn morfeusz_instance_analyse(
    instance: *mut MorfeuszInstance,
    text: *const c_char,
) -> *mut MorfeuszOwnedInterps {
    if instance.is_null() || text.is_null() {
        return ptr::null_mut();
    }
    let input = decode_lossy(
        (*instance).morfeusz.charset(),
        CStr::from_ptr(text).to_bytes(),
    );
    let results = (*instance).morfeusz.analyse(&input);
    (*instance).convert_results(results)
}

#[no_mangle]
pub unsafe extern "C" fn morfeusz_instance_generate(
    instance: *mut MorfeuszInstance,
    lemma: *const c_char,
) -> *mut MorfeuszOwnedInterps {
    if instance.is_null() || lemma.is_null() {
        return ptr::null_mut();
    }
    let input = decode_lossy(
        (*instance).morfeusz.charset(),
        CStr::from_ptr(lemma).to_bytes(),
    );
    let results = (*instance).morfeusz.generate(&input);
    (*instance).convert_results(results)
}

#[no_mangle]
pub unsafe extern "C" fn morfeusz_instance_generate_by_tag_id(
    instance: *mut MorfeuszInstance,
    lemma: *const c_char,
    tag_id: c_int,
) -> *mut MorfeuszOwnedInterps {
    if instance.is_null() || lemma.is_null() {
        return ptr::null_mut();
    }
    let input = decode_lossy(
        (*instance).morfeusz.charset(),
        CStr::from_ptr(lemma).to_bytes(),
    );
    let results = (*instance).morfeusz.generate_by_tag_id(&input, tag_id);
    (*instance).convert_results(results)
}

#[no_mangle]
pub unsafe extern "C" fn morfeusz_interps_len(results: *const MorfeuszOwnedInterps) -> usize {
    if results.is_null() {
        0
    } else {
        (*results).interps.len()
    }
}

#[no_mangle]
pub unsafe extern "C" fn morfeusz_interps_data(
    results: *const MorfeuszOwnedInterps,
) -> *const MorfeuszInterp {
    if results.is_null() {
        ptr::null()
    } else {
        (*results).interps.as_ptr()
    }
}

#[no_mangle]
pub unsafe extern "C" fn morfeusz_destroy_interps(results: *mut MorfeuszOwnedInterps) {
    if !results.is_null() {
        drop(Box::from_raw(results));
    }
}

#[no_mangle]
pub unsafe extern "C" fn morfeusz_instance_set_charset(
    instance: *mut MorfeuszInstance,
    value: c_int,
) -> c_int {
    if instance.is_null() {
        return 0;
    }
    let Some(charset) = charset_from_cpp(value) else {
        (*instance).set_error("Invalid charset option");
        return 0;
    };
    (*instance).morfeusz.set_charset(charset);
    (*instance).clear_error();
    1
}

#[no_mangle]
pub unsafe extern "C" fn morfeusz_instance_get_charset(instance: *const MorfeuszInstance) -> c_int {
    if instance.is_null() {
        return 0;
    }
    (*instance).morfeusz.charset() as c_int
}

#[no_mangle]
pub unsafe extern "C" fn morfeusz_instance_set_aggl(
    instance: *mut MorfeuszInstance,
    value: *const c_char,
) -> c_int {
    if instance.is_null() {
        return 0;
    }
    let Some(value) = c_str_lossy(value) else {
        (*instance).set_error("Invalid aggl option");
        return 0;
    };
    match (*instance).morfeusz.set_aggl(&value) {
        Ok(()) => {
            (*instance).clear_error();
            1
        }
        Err(err) => {
            (*instance).set_error(err);
            0
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn morfeusz_instance_get_aggl(
    instance: *mut MorfeuszInstance,
) -> *const c_char {
    if instance.is_null() {
        return ptr::null();
    }
    let value = (*instance).morfeusz.aggl().to_owned();
    (*instance).store_output_string(&value)
}

#[no_mangle]
pub unsafe extern "C" fn morfeusz_instance_set_praet(
    instance: *mut MorfeuszInstance,
    value: *const c_char,
) -> c_int {
    if instance.is_null() {
        return 0;
    }
    let Some(value) = c_str_lossy(value) else {
        (*instance).set_error("Invalid praet option");
        return 0;
    };
    match (*instance).morfeusz.set_praet(&value) {
        Ok(()) => {
            (*instance).clear_error();
            1
        }
        Err(err) => {
            (*instance).set_error(err);
            0
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn morfeusz_instance_get_praet(
    instance: *mut MorfeuszInstance,
) -> *const c_char {
    if instance.is_null() {
        return ptr::null();
    }
    let value = (*instance).morfeusz.praet().to_owned();
    (*instance).store_output_string(&value)
}

#[no_mangle]
pub unsafe extern "C" fn morfeusz_instance_set_case_handling(
    instance: *mut MorfeuszInstance,
    value: c_int,
) -> c_int {
    if instance.is_null() {
        return 0;
    }
    let Some(case_handling) = case_from_cpp(value) else {
        (*instance).set_error("Invalid case handling option");
        return 0;
    };
    (*instance).morfeusz.set_case_handling(case_handling);
    (*instance).clear_error();
    1
}

#[no_mangle]
pub unsafe extern "C" fn morfeusz_instance_get_case_handling(
    instance: *const MorfeuszInstance,
) -> c_int {
    if instance.is_null() {
        return 0;
    }
    (*instance).morfeusz.case_handling() as c_int
}

#[no_mangle]
pub unsafe extern "C" fn morfeusz_instance_set_token_numbering(
    instance: *mut MorfeuszInstance,
    value: c_int,
) -> c_int {
    if instance.is_null() {
        return 0;
    }
    let Some(token_numbering) = token_numbering_from_cpp(value) else {
        (*instance).set_error("Invalid token numbering option");
        return 0;
    };
    (*instance).morfeusz.set_token_numbering(token_numbering);
    (*instance).clear_error();
    1
}

#[no_mangle]
pub unsafe extern "C" fn morfeusz_instance_get_token_numbering(
    instance: *const MorfeuszInstance,
) -> c_int {
    if instance.is_null() {
        return 0;
    }
    (*instance).morfeusz.token_numbering() as c_int
}

#[no_mangle]
pub unsafe extern "C" fn morfeusz_instance_set_whitespace_handling(
    instance: *mut MorfeuszInstance,
    value: c_int,
) -> c_int {
    if instance.is_null() {
        return 0;
    }
    let Some(whitespace_handling) = whitespace_from_cpp(value) else {
        (*instance).set_error("Invalid whitespace handling option");
        return 0;
    };
    (*instance)
        .morfeusz
        .set_whitespace_handling(whitespace_handling);
    (*instance).clear_error();
    1
}

#[no_mangle]
pub unsafe extern "C" fn morfeusz_instance_get_whitespace_handling(
    instance: *const MorfeuszInstance,
) -> c_int {
    if instance.is_null() {
        return 0;
    }
    (*instance).morfeusz.whitespace_handling() as c_int
}

#[no_mangle]
pub unsafe extern "C" fn morfeusz_instance_set_debug(
    instance: *mut MorfeuszInstance,
    debug: c_int,
) -> c_int {
    if instance.is_null() {
        return 0;
    }
    (*instance).morfeusz.set_debug(debug != 0);
    (*instance).clear_error();
    1
}

#[no_mangle]
pub unsafe extern "C" fn morfeusz_instance_set_dictionary(
    instance: *mut MorfeuszInstance,
    dict_name: *const c_char,
) -> c_int {
    if instance.is_null() {
        return 0;
    }
    let Some(dict_name) = c_str_lossy(dict_name) else {
        (*instance).set_error("Invalid dictionary name");
        return 0;
    };
    let repository = binary_dictionary_repository_from_search_paths();
    match (*instance)
        .morfeusz
        .set_dictionary_named_with_repository(&repository, &dict_name)
    {
        Ok(()) => {
            (*instance).clear_error();
            1
        }
        Err(err) => {
            (*instance).set_error(err);
            0
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn morfeusz_instance_available_aggl_count(
    instance: *const MorfeuszInstance,
) -> usize {
    if instance.is_null() {
        0
    } else {
        (*instance).morfeusz.available_aggl_options().len()
    }
}

#[no_mangle]
pub unsafe extern "C" fn morfeusz_instance_available_aggl_item(
    instance: *mut MorfeuszInstance,
    index: usize,
) -> *const c_char {
    if instance.is_null() {
        return ptr::null();
    }
    let options = (*instance).morfeusz.available_aggl_options();
    let Some(value) = options.get(index) else {
        return ptr::null();
    };
    (*instance).store_output_string(value)
}

#[no_mangle]
pub unsafe extern "C" fn morfeusz_instance_available_praet_count(
    instance: *const MorfeuszInstance,
) -> usize {
    if instance.is_null() {
        0
    } else {
        (*instance).morfeusz.available_praet_options().len()
    }
}

#[no_mangle]
pub unsafe extern "C" fn morfeusz_instance_available_praet_item(
    instance: *mut MorfeuszInstance,
    index: usize,
) -> *const c_char {
    if instance.is_null() {
        return ptr::null();
    }
    let options = (*instance).morfeusz.available_praet_options();
    let Some(value) = options.get(index) else {
        return ptr::null();
    };
    (*instance).store_output_string(value)
}

#[no_mangle]
pub unsafe extern "C" fn morfeusz_instance_get_tagset_id(
    instance: *mut MorfeuszInstance,
) -> *const c_char {
    if instance.is_null() {
        return ptr::null();
    }
    let value = (*instance).morfeusz.id_resolver().tagset_id().to_owned();
    (*instance).store_output_string(&value)
}

#[no_mangle]
pub unsafe extern "C" fn morfeusz_instance_get_tag(
    instance: *mut MorfeuszInstance,
    tag_id: c_int,
) -> *const c_char {
    if instance.is_null() {
        return ptr::null();
    }
    let Some(value) = (*instance)
        .morfeusz
        .id_resolver()
        .tag(tag_id)
        .map(ToOwned::to_owned)
    else {
        (*instance).set_error("Invalid tagId");
        return ptr::null();
    };
    (*instance).clear_error();
    (*instance).store_output_string(&value)
}

#[no_mangle]
pub unsafe extern "C" fn morfeusz_instance_get_tag_id(
    instance: *mut MorfeuszInstance,
    tag: *const c_char,
) -> c_int {
    if instance.is_null() {
        return -1;
    }
    let Some(tag) = c_str_decoded(tag, (*instance).morfeusz.charset()) else {
        (*instance).set_error("Invalid tag");
        return -1;
    };
    match (*instance).morfeusz.id_resolver().tag_id(tag.as_str()) {
        Ok(id) => {
            (*instance).clear_error();
            id
        }
        Err(err) => {
            (*instance).set_error(err);
            -1
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn morfeusz_instance_get_name(
    instance: *mut MorfeuszInstance,
    name_id: c_int,
) -> *const c_char {
    if instance.is_null() {
        return ptr::null();
    }
    let Some(value) = (*instance)
        .morfeusz
        .id_resolver()
        .name(name_id)
        .map(ToOwned::to_owned)
    else {
        (*instance).set_error("Invalid nameId");
        return ptr::null();
    };
    (*instance).clear_error();
    (*instance).store_output_string(&value)
}

#[no_mangle]
pub unsafe extern "C" fn morfeusz_instance_get_name_id(
    instance: *mut MorfeuszInstance,
    name: *const c_char,
) -> c_int {
    if instance.is_null() {
        return -1;
    }
    let Some(name) = c_str_decoded(name, (*instance).morfeusz.charset()) else {
        (*instance).set_error("Invalid name");
        return -1;
    };
    match (*instance).morfeusz.id_resolver().name_id(name.as_str()) {
        Ok(id) => {
            (*instance).clear_error();
            id
        }
        Err(err) => {
            (*instance).set_error(err);
            -1
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn morfeusz_instance_get_labels_as_string(
    instance: *mut MorfeuszInstance,
    labels_id: c_int,
) -> *const c_char {
    if instance.is_null() {
        return ptr::null();
    }
    let Some(value) = (*instance)
        .morfeusz
        .id_resolver()
        .labels_as_string(labels_id)
        .map(ToOwned::to_owned)
    else {
        (*instance).set_error("Invalid labelsId");
        return ptr::null();
    };
    (*instance).clear_error();
    (*instance).store_output_string(&value)
}

#[no_mangle]
pub unsafe extern "C" fn morfeusz_instance_get_labels_id(
    instance: *mut MorfeuszInstance,
    labels: *const c_char,
) -> c_int {
    if instance.is_null() {
        return -1;
    }
    let Some(labels) = c_str_decoded(labels, (*instance).morfeusz.charset()) else {
        (*instance).set_error("Invalid labels string");
        return -1;
    };
    match (*instance)
        .morfeusz
        .id_resolver()
        .labels_id(labels.as_str())
    {
        Ok(id) => {
            (*instance).clear_error();
            id
        }
        Err(err) => {
            (*instance).set_error(err);
            -1
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn morfeusz_instance_get_tags_count(
    instance: *const MorfeuszInstance,
) -> usize {
    if instance.is_null() {
        0
    } else {
        (*instance).morfeusz.id_resolver().tags_count()
    }
}

#[no_mangle]
pub unsafe extern "C" fn morfeusz_instance_get_names_count(
    instance: *const MorfeuszInstance,
) -> usize {
    if instance.is_null() {
        0
    } else {
        (*instance).morfeusz.id_resolver().names_count()
    }
}

#[no_mangle]
pub unsafe extern "C" fn morfeusz_instance_get_labels_count(
    instance: *const MorfeuszInstance,
) -> usize {
    if instance.is_null() {
        0
    } else {
        (*instance).morfeusz.id_resolver().labels_count()
    }
}

fn set_encoding_option(morfeusz: &mut Morfeusz, value: c_int) -> c_int {
    let charset = match value {
        MORFEUSZ_UTF_8 => Charset::Utf8,
        MORFEUSZ_ISO8859_2 => Charset::Iso8859_2,
        MORFEUSZ_CP1250 => Charset::Cp1250,
        MORFEUSZ_CP852 => Charset::Cp852,
        _ => {
            eprintln!("Wrong encoding option {value}");
            return 0;
        }
    };
    morfeusz.set_charset(charset);
    1
}

fn set_whitespace_option(morfeusz: &mut Morfeusz, value: c_int) -> c_int {
    let whitespace = match value {
        MORFEUSZ_SKIP_WHITESPACE => WhitespaceHandling::Skip,
        MORFEUSZ_KEEP_WHITESPACE => WhitespaceHandling::Keep,
        MORFEUSZ_APPEND_WHITESPACE => WhitespaceHandling::Append,
        _ => {
            eprintln!("Wrong whitespace option {value}");
            return 0;
        }
    };
    morfeusz.set_whitespace_handling(whitespace);
    1
}

fn set_case_option(morfeusz: &mut Morfeusz, value: c_int) -> c_int {
    let case_handling = match value {
        MORFEUSZ_WEAK_CASE => CaseHandling::ConditionallyCaseSensitive,
        MORFEUSZ_STRICT_CASE => CaseHandling::StrictlyCaseSensitive,
        MORFEUSZ_IGNORE_CASE => CaseHandling::IgnoreCase,
        _ => {
            eprintln!("Wrong case option {value}");
            return 0;
        }
    };
    morfeusz.set_case_handling(case_handling);
    1
}

fn set_token_numbering_option(morfeusz: &mut Morfeusz, value: c_int) -> c_int {
    let token_numbering = match value {
        MORFEUSZ_SEPARATE_TOKEN_NUMBERING => TokenNumbering::Separate,
        MORFEUSZ_CONTINUOUS_TOKEN_NUMBERING => TokenNumbering::Continuous,
        _ => {
            eprintln!("Wrong case option {value}");
            return 0;
        }
    };
    morfeusz.set_token_numbering(token_numbering);
    1
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn lock_test() -> MutexGuard<'static, ()> {
        let guard = TEST_LOCK.lock().unwrap();
        reset_options();
        guard
    }

    fn reset_options() {
        morfeusz_set_option(MORFOPT_ENCODING, MORFEUSZ_UTF_8);
        morfeusz_set_option(MORFOPT_WHITESPACE, MORFEUSZ_SKIP_WHITESPACE);
        morfeusz_set_option(MORFOPT_CASE, MORFEUSZ_WEAK_CASE);
        morfeusz_set_option(MORFOPT_TOKEN_NUMBERING, MORFEUSZ_SEPARATE_TOKEN_NUMBERING);
        clear_global_error();
    }

    unsafe fn str_at(ptr: *mut c_char) -> String {
        CStr::from_ptr(ptr).to_string_lossy().into_owned()
    }

    unsafe fn bytes_at(ptr: *mut c_char) -> Vec<u8> {
        CStr::from_ptr(ptr).to_bytes().to_vec()
    }

    #[test]
    fn exposes_version_string() {
        let _guard = lock_test();

        let about = unsafe { str_at(morfeusz_about()) };

        assert_eq!(about, "1.99.15");
    }

    #[test]
    fn analyzes_two_simple_invocations_with_sentinel() {
        let _guard = lock_test();
        let mut text = CString::new("AAaaBBbbCCcc DDDD.").unwrap();

        let results = unsafe { morfeusz_analyse(text.as_ptr() as *mut c_char) };

        assert_eq!(unsafe { (*results.add(0)).p }, 0);
        assert_eq!(unsafe { (*results.add(0)).k }, 1);
        assert_eq!(unsafe { str_at((*results.add(0)).forma) }, "AAaaBBbbCCcc");
        assert_eq!(unsafe { str_at((*results.add(0)).haslo) }, "AAaaBBbbCCcc");
        assert_eq!(unsafe { str_at((*results.add(0)).interp) }, "ign");
        assert_eq!(unsafe { (*results.add(3)).p }, -1);

        text = CString::new("EEeeFFff").unwrap();
        let results = unsafe { morfeusz_analyse(text.as_ptr() as *mut c_char) };
        assert_eq!(unsafe { (*results.add(0)).p }, 0);
        assert_eq!(unsafe { str_at((*results.add(0)).forma) }, "EEeeFFff");
        assert_eq!(unsafe { (*results.add(1)).p }, -1);
    }

    #[test]
    fn supports_keep_and_append_whitespace_options() {
        let _guard = lock_test();
        assert_eq!(
            morfeusz_set_option(MORFOPT_WHITESPACE, MORFEUSZ_KEEP_WHITESPACE),
            1
        );
        let text = CString::new("AAaaBBbbCCcc  .").unwrap();

        let results = unsafe { morfeusz_analyse(text.as_ptr() as *mut c_char) };

        assert_eq!(unsafe { str_at((*results.add(1)).forma) }, "  ");
        assert_eq!(unsafe { (*results.add(3)).p }, -1);

        assert_eq!(
            morfeusz_set_option(MORFOPT_WHITESPACE, MORFEUSZ_APPEND_WHITESPACE),
            1
        );
        let results = unsafe { morfeusz_analyse(text.as_ptr() as *mut c_char) };
        assert_eq!(unsafe { str_at((*results.add(0)).forma) }, "AAaaBBbbCCcc  ");
        assert_eq!(unsafe { str_at((*results.add(0)).haslo) }, "AAaaBBbbCCcc");
        assert_eq!(unsafe { (*results.add(2)).p }, -1);
    }

    #[test]
    fn supports_continuous_token_numbering() {
        let _guard = lock_test();
        assert_eq!(
            morfeusz_set_option(MORFOPT_TOKEN_NUMBERING, MORFEUSZ_CONTINUOUS_TOKEN_NUMBERING),
            1
        );
        let first = CString::new("aaaabbbb bbbbcccc.").unwrap();
        let second = CString::new("ccccdddd").unwrap();

        let results = unsafe { morfeusz_analyse(first.as_ptr() as *mut c_char) };
        assert_eq!(unsafe { (*results.add(0)).p }, 0);
        assert_eq!(unsafe { (*results.add(2)).k }, 3);

        let results = unsafe { morfeusz_analyse(second.as_ptr() as *mut c_char) };
        assert_eq!(unsafe { (*results.add(0)).p }, 3);
        assert_eq!(unsafe { (*results.add(0)).k }, 4);
    }

    #[test]
    fn round_trips_legacy_c_api_encodings() {
        let _guard = lock_test();
        for (encoding, bytes) in [
            (MORFEUSZ_ISO8859_2, vec![0xBF, 0xF3, 0xB3, 0xE6]),
            (MORFEUSZ_CP1250, vec![0xBF, 0xF3, 0xB3, 0xE6]),
            (MORFEUSZ_CP852, vec![0xBE, 0xA2, 0x88, 0x86]),
        ] {
            assert_eq!(morfeusz_set_option(MORFOPT_ENCODING, encoding), 1);
            let text = CString::new(bytes.clone()).unwrap();

            let results = unsafe { morfeusz_analyse(text.as_ptr() as *mut c_char) };

            assert_eq!(unsafe { bytes_at((*results.add(0)).forma) }, bytes);
            assert_eq!(unsafe { bytes_at((*results.add(0)).haslo) }, bytes);
            assert_eq!(unsafe { bytes_at((*results.add(0)).interp) }, b"ign");
            assert_eq!(unsafe { (*results.add(1)).p }, -1);
        }
    }

    #[test]
    fn round_trips_explicit_utf8_c_api_encoding() {
        let _guard = lock_test();
        let bytes = vec![b'z', b'a', 197, 188, 195, 179];
        assert_eq!(morfeusz_set_option(MORFOPT_ENCODING, MORFEUSZ_UTF_8), 1);
        let text = CString::new(bytes.clone()).unwrap();

        let results = unsafe { morfeusz_analyse(text.as_ptr() as *mut c_char) };

        assert_eq!(unsafe { (*results.add(0)).p }, 0);
        assert_eq!(unsafe { (*results.add(0)).k }, 1);
        assert_eq!(unsafe { bytes_at((*results.add(0)).forma) }, bytes);
        assert_eq!(unsafe { bytes_at((*results.add(0)).haslo) }, bytes);
        assert_eq!(unsafe { bytes_at((*results.add(0)).interp) }, b"ign");
        assert_eq!(unsafe { (*results.add(1)).p }, -1);
    }

    #[test]
    fn round_trips_legacy_c_api_utf8_bytes_declared_as_cp1250() {
        let _guard = lock_test();
        let bytes = vec![b'z', b'a', 197, 188, 195, 179];
        assert_eq!(morfeusz_set_option(MORFOPT_ENCODING, MORFEUSZ_CP1250), 1);
        let text = CString::new(bytes.clone()).unwrap();

        let results = unsafe { morfeusz_analyse(text.as_ptr() as *mut c_char) };

        assert_eq!(unsafe { (*results.add(0)).p }, 0);
        assert_eq!(unsafe { (*results.add(0)).k }, 1);
        assert_eq!(unsafe { bytes_at((*results.add(0)).forma) }, bytes);
        assert_eq!(unsafe { bytes_at((*results.add(0)).haslo) }, bytes);
        assert_eq!(unsafe { bytes_at((*results.add(0)).interp) }, b"ign");
    }

    #[test]
    fn rejects_unknown_option_values() {
        let _guard = lock_test();

        assert_eq!(morfeusz_set_option(MORFOPT_WHITESPACE, 666777), 0);
        assert_eq!(morfeusz_set_option(MORFOPT_ENCODING, 666777), 0);
        assert_eq!(morfeusz_set_option(MORFOPT_CASE, 666777), 0);
        assert_eq!(morfeusz_set_option(MORFOPT_TOKEN_NUMBERING, 666777), 0);
        assert_eq!(morfeusz_set_option(666777, 1), 0);
    }

    #[test]
    fn records_create_instance_error_before_instance_exists() {
        let _guard = lock_test();

        let instance = morfeusz_create_instance(666777);

        assert!(instance.is_null());
        let error = unsafe {
            CStr::from_ptr(morfeusz_last_error())
                .to_string_lossy()
                .into_owned()
        };
        assert!(error.contains("Invalid usage option"), "{error}");
    }
}
