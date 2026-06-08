use std::env;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use morfeusz::{
    BinaryDictionaryRepository, CaseHandling, Engine, IdResolver, Morfeusz, MorfeuszUsage,
    MorphInterpretation, NumberingScope, Session, TokenNumbering, TsvLexiconLoader,
    WhitespaceHandling,
};
use serde::{Deserialize, Serialize};

#[derive(Debug)]
struct Args {
    dictionary: Option<PathBuf>,
    tagset: Option<PathBuf>,
    dict: Option<String>,
    dict_dir: Option<PathBuf>,
}

enum Runtime {
    Engine { engine: Engine, session: Session },
    Morfeusz(Morfeusz),
}

#[derive(Debug, Deserialize)]
struct Request {
    mode: Option<Mode>,
    text: Option<String>,
    lemma: Option<String>,
    tag_id: Option<i32>,
    dict: Option<String>,
    dict_dir: Option<PathBuf>,
    dictionary: Option<PathBuf>,
    tagset: Option<PathBuf>,
    aggl: Option<String>,
    praet: Option<String>,
    case_handling: Option<OptionValue>,
    token_numbering: Option<OptionValue>,
    whitespace: Option<OptionValue>,
    debug: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Mode {
    #[serde(alias = "analyse")]
    Analyze,
    Generate,
    SetDictionary,
    Metadata,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum OptionValue {
    String(String),
    Int(i32),
}

#[derive(Debug, Serialize)]
struct Response {
    ok: bool,
    results: Vec<InterpretationDto>,
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    metadata: Option<MetadataDto>,
}

#[derive(Debug, Serialize)]
struct InterpretationDto {
    start_node: i32,
    end_node: i32,
    orth: String,
    lemma: String,
    tag_id: i32,
    tag: String,
    name_id: i32,
    name: String,
    labels_id: i32,
    labels: String,
}

#[derive(Debug, Serialize)]
struct MetadataDto {
    version: &'static str,
    copyright: &'static str,
    default_dict_name: &'static str,
    dict_id: String,
    dict_copyright: String,
    tagset_id: String,
    aggl: String,
    praet: String,
    available_aggl_options: Vec<String>,
    available_praet_options: Vec<String>,
    case_handling: &'static str,
    case_handling_id: i32,
    token_numbering: &'static str,
    token_numbering_id: i32,
    whitespace: &'static str,
    whitespace_id: i32,
    debug: bool,
}

fn main() {
    if let Err(err) = run() {
        eprintln!("{err}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let args = parse_args(env::args().skip(1))?;
    let mut runtime = build_runtime(args)?;
    serve_jsonl(&mut runtime, io::stdin().lock(), io::stdout().lock())
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut dictionary = None;
    let mut tagset = None;
    let mut dict = None;
    let mut dict_dir = None;
    let mut iter = args.into_iter();

    while let Some(arg) = iter.next() {
        match split_flag_value(&arg) {
            ("--dict", value) => {
                dict = Some(take_arg_value(&mut iter, "--dict", value)?);
            }
            ("--dict-dir", value) => {
                dict_dir = Some(take_arg_value(&mut iter, "--dict-dir", value)?.into());
            }
            ("--dictionary", value) => {
                dictionary = Some(take_arg_value(&mut iter, "--dictionary", value)?.into());
            }
            ("--tagset", value) => {
                tagset = Some(take_arg_value(&mut iter, "--tagset", value)?.into());
            }
            ("--help" | "-h", None) => {
                return Err(
                    "usage: morfeusz-service [--dict NAME] [--dict-dir DIR] [--dictionary PATH] [--tagset PATH] < requests.jsonl"
                        .to_owned(),
                );
            }
            ("--help" | "-h", Some(_)) => {
                return Err("invalid value for flag without argument: --help".to_owned());
            }
            other => return Err(format!("unknown argument: {}", other.0)),
        }
    }

    Ok(Args {
        dictionary,
        tagset,
        dict,
        dict_dir,
    })
}

fn split_flag_value(arg: &str) -> (&str, Option<&str>) {
    arg.split_once('=')
        .map(|(flag, value)| (flag, Some(value)))
        .unwrap_or((arg, None))
}

fn take_arg_value(
    iter: &mut impl Iterator<Item = String>,
    flag: &str,
    inline: Option<&str>,
) -> Result<String, String> {
    if let Some(value) = inline {
        return Ok(value.to_owned());
    }
    iter.next()
        .ok_or_else(|| format!("{flag} requires {}", flag_value_name(flag)))
}

fn flag_value_name(flag: &str) -> &'static str {
    match flag {
        "--dict" => "a name",
        "--dict-dir" | "--dictionary" | "--tagset" => "a path",
        _ => "a value",
    }
}

fn build_runtime(args: Args) -> Result<Runtime, String> {
    if let Some(dictionary_path) = args.dictionary {
        if is_binary_dictionary_path(&dictionary_path) {
            return BinaryDictionaryRepository::default()
                .load_path(&dictionary_path, MorfeuszUsage::BothAnalyseAndGenerate)
                .map(Runtime::Morfeusz)
                .map_err(|err| err.to_string());
        }
        return {
            let dictionary = TsvLexiconLoader::from_paths(dictionary_path, args.tagset)
                .map_err(|err| err.to_string())?;
            Ok(engine_runtime(
                Engine::builder().lexicon(dictionary).build(),
            ))
        };
    }

    if args.dict.is_some() || args.dict_dir.is_some() {
        let search_path = args.dict_dir.unwrap_or_else(|| PathBuf::from("."));
        let dict_name = args
            .dict
            .unwrap_or_else(|| Morfeusz::default_dict_name().to_owned());
        return BinaryDictionaryRepository::new([search_path])
            .load_named(&dict_name, MorfeuszUsage::BothAnalyseAndGenerate)
            .map(Runtime::Morfeusz)
            .map_err(|err| err.to_string());
    }

    match BinaryDictionaryRepository::default().load_named(
        Morfeusz::default_dict_name(),
        MorfeuszUsage::BothAnalyseAndGenerate,
    ) {
        Ok(morfeusz) => Ok(Runtime::Morfeusz(morfeusz)),
        Err(_) => Ok(engine_runtime(Engine::builder().build())),
    }
}

fn engine_runtime(engine: Engine) -> Runtime {
    let session = engine.session();
    Runtime::Engine { engine, session }
}

impl Runtime {
    fn set_dictionary(&mut self, request: &Request) -> Result<(), String> {
        if let Some(dictionary_path) = &request.dictionary {
            if is_binary_dictionary_path(dictionary_path) {
                return self.set_binary_dictionary_path(dictionary_path);
            }
            let dictionary = TsvLexiconLoader::from_paths(dictionary_path, request.tagset.as_ref())
                .map_err(|err| err.to_string())?;
            match self {
                Self::Engine { engine, session } => {
                    let config = engine.config().clone();
                    let loaded = Engine::builder().lexicon(dictionary).config(config).build();
                    *engine = loaded;
                    *session = engine.session();
                }
                Self::Morfeusz(morfeusz) => {
                    morfeusz.set_dictionary(dictionary);
                }
            }
            return Ok(());
        }

        let dict_name = request
            .dict
            .as_deref()
            .unwrap_or(Morfeusz::default_dict_name());
        let search_path = request
            .dict_dir
            .clone()
            .unwrap_or_else(|| PathBuf::from("."));
        let repository = BinaryDictionaryRepository::new([search_path]);

        match self {
            Self::Engine { .. } => {
                *self = Runtime::Morfeusz(
                    repository
                        .load_named(dict_name, MorfeuszUsage::BothAnalyseAndGenerate)
                        .map_err(|err| err.to_string())?,
                );
            }
            Self::Morfeusz(morfeusz) => {
                morfeusz
                    .set_dictionary_named_with_repository(&repository, dict_name)
                    .map_err(|err| err.to_string())?;
            }
        }
        Ok(())
    }

    fn set_binary_dictionary_path(
        &mut self,
        dictionary_path: &std::path::Path,
    ) -> Result<(), String> {
        match self {
            Self::Engine { .. } => {
                *self = Runtime::Morfeusz(
                    BinaryDictionaryRepository::default()
                        .load_path(dictionary_path, MorfeuszUsage::BothAnalyseAndGenerate)
                        .map_err(|err| err.to_string())?,
                );
            }
            Self::Morfeusz(morfeusz) => {
                morfeusz
                    .set_dictionary_path_with_repository(
                        &BinaryDictionaryRepository::default(),
                        dictionary_path,
                    )
                    .map_err(|err| err.to_string())?;
            }
        }
        Ok(())
    }

    fn apply_options(&mut self, request: &Request) -> Result<(), String> {
        match self {
            Self::Engine { engine, session } => {
                let mut config = engine.config().clone();
                if let Some(aggl) = request.aggl.as_deref() {
                    let segmentation = config
                        .segmentation()
                        .clone()
                        .with_aggl(aggl)
                        .map_err(|err| err.to_string())?;
                    engine
                        .validate_segmentation(&segmentation, "aggl", aggl)
                        .map_err(|err| err.to_string())?;
                    config = config.with_segmentation(segmentation);
                }
                if let Some(praet) = request.praet.as_deref() {
                    let segmentation = config
                        .segmentation()
                        .clone()
                        .with_praet(praet)
                        .map_err(|err| err.to_string())?;
                    engine
                        .validate_segmentation(&segmentation, "praet", praet)
                        .map_err(|err| err.to_string())?;
                    config = config.with_segmentation(segmentation);
                }
                if let Some(case_handling) = &request.case_handling {
                    config = config.with_case_handling(parse_case_handling(case_handling)?);
                }
                if let Some(token_numbering) = &request.token_numbering {
                    config = config.with_numbering(NumberingScope::from(parse_token_numbering(
                        token_numbering,
                    )?));
                }
                if let Some(whitespace) = &request.whitespace {
                    config = config.with_whitespace(parse_whitespace(whitespace)?);
                }
                if let Some(debug) = request.debug {
                    config = config.with_debug(debug);
                }
                if &config != engine.config() {
                    *engine = engine.with_config(config);
                    *session = engine.session();
                }
                Ok(())
            }
            Self::Morfeusz(morfeusz) => {
                if let Some(aggl) = request.aggl.as_deref() {
                    morfeusz.set_aggl(aggl).map_err(|err| err.to_string())?;
                }
                if let Some(praet) = request.praet.as_deref() {
                    morfeusz.set_praet(praet).map_err(|err| err.to_string())?;
                }
                if let Some(case_handling) = &request.case_handling {
                    morfeusz.set_case_handling(parse_case_handling(case_handling)?);
                }
                if let Some(token_numbering) = &request.token_numbering {
                    morfeusz.set_token_numbering(parse_token_numbering(token_numbering)?);
                }
                if let Some(whitespace) = &request.whitespace {
                    morfeusz.set_whitespace_handling(parse_whitespace(whitespace)?);
                }
                if let Some(debug) = request.debug {
                    morfeusz.set_debug(debug);
                }
                Ok(())
            }
        }
    }

    fn analyze(&mut self, text: &str) -> Result<Vec<MorphInterpretation>, String> {
        match self {
            Self::Engine { session, .. } => session.analyze(text).map_err(|err| err.to_string()),
            Self::Morfeusz(morfeusz) => morfeusz.analyse(text).map_err(|err| err.to_string()),
        }
    }

    fn generate(
        &mut self,
        lemma: &str,
        tag_id: Option<i32>,
    ) -> Result<Vec<MorphInterpretation>, String> {
        match self {
            Self::Engine { engine, .. } => match tag_id {
                Some(tag_id) => engine
                    .generate_by_tag_id(lemma, tag_id)
                    .map_err(|err| err.to_string()),
                None => engine.generate(lemma).map_err(|err| err.to_string()),
            },
            Self::Morfeusz(morfeusz) => match tag_id {
                Some(tag_id) => morfeusz
                    .generate_by_tag_id(lemma, tag_id)
                    .map_err(|err| err.to_string()),
                None => morfeusz.generate(lemma).map_err(|err| err.to_string()),
            },
        }
    }

    fn resolver(&self) -> &IdResolver {
        match self {
            Self::Engine { engine, .. } => engine.resolver(),
            Self::Morfeusz(morfeusz) => morfeusz.id_resolver(),
        }
    }

    fn metadata(&self) -> MetadataDto {
        match self {
            Self::Engine { engine, .. } => {
                let config = engine.config();
                let case_handling = config.case_handling();
                let token_numbering = TokenNumbering::from(config.numbering());
                let whitespace = config.whitespace();
                MetadataDto {
                    version: Morfeusz::version(),
                    copyright: Morfeusz::copyright(),
                    default_dict_name: Morfeusz::default_dict_name(),
                    dict_id: engine.lexicon_id().to_owned(),
                    dict_copyright: engine.lexicon_copyright().to_owned(),
                    tagset_id: engine.resolver().tagset_id().to_owned(),
                    aggl: config
                        .segmentation()
                        .aggl()
                        .or_else(|| engine.default_aggl())
                        .unwrap_or("permissive")
                        .to_owned(),
                    praet: config
                        .segmentation()
                        .praet()
                        .or_else(|| engine.default_praet())
                        .unwrap_or("split")
                        .to_owned(),
                    available_aggl_options: engine.available_aggl_options(),
                    available_praet_options: engine.available_praet_options(),
                    case_handling: case_handling_name(case_handling),
                    case_handling_id: case_handling as i32,
                    token_numbering: token_numbering_name(token_numbering),
                    token_numbering_id: token_numbering as i32,
                    whitespace: whitespace_name(whitespace),
                    whitespace_id: whitespace as i32,
                    debug: config.debug(),
                }
            }
            Self::Morfeusz(morfeusz) => {
                let case_handling = morfeusz.case_handling();
                let token_numbering = morfeusz.token_numbering();
                let whitespace = morfeusz.whitespace_handling();
                MetadataDto {
                    version: Morfeusz::version(),
                    copyright: Morfeusz::copyright(),
                    default_dict_name: Morfeusz::default_dict_name(),
                    dict_id: morfeusz.dict_id().to_owned(),
                    dict_copyright: morfeusz.dict_copyright().to_owned(),
                    tagset_id: morfeusz.id_resolver().tagset_id().to_owned(),
                    aggl: morfeusz.aggl().to_owned(),
                    praet: morfeusz.praet().to_owned(),
                    available_aggl_options: morfeusz.available_aggl_options(),
                    available_praet_options: morfeusz.available_praet_options(),
                    case_handling: case_handling_name(case_handling),
                    case_handling_id: case_handling as i32,
                    token_numbering: token_numbering_name(token_numbering),
                    token_numbering_id: token_numbering as i32,
                    whitespace: whitespace_name(whitespace),
                    whitespace_id: whitespace as i32,
                    debug: morfeusz.debug(),
                }
            }
        }
    }
}

fn is_binary_dictionary_path(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("dict"))
}

fn serve_jsonl(
    runtime: &mut Runtime,
    input: impl BufRead,
    mut output: impl Write,
) -> Result<(), String> {
    for line in input.lines() {
        let line = line.map_err(|err| err.to_string())?;
        if line.trim().is_empty() {
            continue;
        }

        let response = handle_request(runtime, &line);
        serde_json::to_writer(&mut output, &response).map_err(|err| err.to_string())?;
        output.write_all(b"\n").map_err(|err| err.to_string())?;
    }
    Ok(())
}

fn handle_request(runtime: &mut Runtime, line: &str) -> Response {
    let request = match serde_json::from_str::<Request>(line) {
        Ok(request) => request,
        Err(_) => Request {
            mode: Some(Mode::Analyze),
            text: Some(line.to_owned()),
            lemma: None,
            tag_id: None,
            dict: None,
            dict_dir: None,
            dictionary: None,
            tagset: None,
            aggl: None,
            praet: None,
            case_handling: None,
            token_numbering: None,
            whitespace: None,
            debug: None,
        },
    };

    let result = match request.mode.unwrap_or(Mode::Analyze) {
        Mode::SetDictionary => runtime
            .set_dictionary(&request)
            .and_then(|()| runtime.apply_options(&request))
            .map(|()| Vec::new()),
        Mode::Metadata => runtime.apply_options(&request).map(|()| Vec::new()),
        mode => runtime.apply_options(&request).and_then(|()| match mode {
            Mode::Analyze => request
                .text
                .ok_or_else(|| "analyze request requires text".to_owned())
                .and_then(|text| runtime.analyze(&text)),
            Mode::Generate => request
                .lemma
                .or(request.text)
                .ok_or_else(|| "generate request requires lemma or text".to_owned())
                .and_then(|lemma| runtime.generate(&lemma, request.tag_id)),
            Mode::SetDictionary => unreachable!("set_dictionary handled before data requests"),
            Mode::Metadata => unreachable!("metadata handled before data requests"),
        }),
    };

    match result {
        Ok(results) => Response {
            ok: true,
            results: results_to_dto(runtime.resolver(), results),
            error: None,
            metadata: (request.mode == Some(Mode::Metadata)).then(|| runtime.metadata()),
        },
        Err(error) => Response {
            ok: false,
            results: Vec::new(),
            error: Some(error),
            metadata: None,
        },
    }
}

fn results_to_dto(
    resolver: &IdResolver,
    results: Vec<MorphInterpretation>,
) -> Vec<InterpretationDto> {
    results
        .into_iter()
        .map(|interp| InterpretationDto {
            start_node: interp.start_node,
            end_node: interp.end_node,
            tag: interp.tag(resolver).unwrap_or("_").to_owned(),
            name: interp.name(resolver).unwrap_or("_").to_owned(),
            labels: interp.labels_as_string(resolver).unwrap_or("_").to_owned(),
            orth: interp.orth,
            lemma: interp.lemma,
            tag_id: interp.tag_id,
            name_id: interp.name_id,
            labels_id: interp.labels_id,
        })
        .collect()
}

fn parse_case_handling(value: &OptionValue) -> Result<CaseHandling, String> {
    match value {
        OptionValue::Int(100) => Ok(CaseHandling::ConditionallyCaseSensitive),
        OptionValue::Int(101) => Ok(CaseHandling::StrictlyCaseSensitive),
        OptionValue::Int(102) => Ok(CaseHandling::IgnoreCase),
        OptionValue::Int(value) => Err(format!("invalid case_handling option: {value}")),
        OptionValue::String(value) => match normalize_option(value).as_str() {
            "weak" | "conditional" | "conditionally_case_sensitive" => {
                Ok(CaseHandling::ConditionallyCaseSensitive)
            }
            "strict" | "strictly_case_sensitive" => Ok(CaseHandling::StrictlyCaseSensitive),
            "ignore" | "ignore_case" => Ok(CaseHandling::IgnoreCase),
            _ => Err(format!("invalid case_handling option: {value}")),
        },
    }
}

fn case_handling_name(value: CaseHandling) -> &'static str {
    match value {
        CaseHandling::ConditionallyCaseSensitive => "CONDITIONALLY_CASE_SENSITIVE",
        CaseHandling::StrictlyCaseSensitive => "STRICTLY_CASE_SENSITIVE",
        CaseHandling::IgnoreCase => "IGNORE_CASE",
    }
}

fn parse_token_numbering(value: &OptionValue) -> Result<TokenNumbering, String> {
    match value {
        OptionValue::Int(201) => Ok(TokenNumbering::Separate),
        OptionValue::Int(202) => Ok(TokenNumbering::Continuous),
        OptionValue::Int(value) => Err(format!("invalid token_numbering option: {value}")),
        OptionValue::String(value) => match normalize_option(value).as_str() {
            "separate" | "separate_numbering" => Ok(TokenNumbering::Separate),
            "continuous" | "continuous_numbering" => Ok(TokenNumbering::Continuous),
            _ => Err(format!("invalid token_numbering option: {value}")),
        },
    }
}

fn token_numbering_name(value: TokenNumbering) -> &'static str {
    match value {
        TokenNumbering::Separate => "SEPARATE_NUMBERING",
        TokenNumbering::Continuous => "CONTINUOUS_NUMBERING",
    }
}

fn parse_whitespace(value: &OptionValue) -> Result<WhitespaceHandling, String> {
    match value {
        OptionValue::Int(301) => Ok(WhitespaceHandling::Skip),
        OptionValue::Int(302) => Ok(WhitespaceHandling::Append),
        OptionValue::Int(303) => Ok(WhitespaceHandling::Keep),
        OptionValue::Int(value) => Err(format!("invalid whitespace option: {value}")),
        OptionValue::String(value) => match normalize_option(value).as_str() {
            "skip" | "skip_whitespace" | "skip_whitespaces" => Ok(WhitespaceHandling::Skip),
            "append" | "append_whitespace" | "append_whitespaces" => Ok(WhitespaceHandling::Append),
            "keep" | "keep_whitespace" | "keep_whitespaces" => Ok(WhitespaceHandling::Keep),
            _ => Err(format!("invalid whitespace option: {value}")),
        },
    }
}

fn whitespace_name(value: WhitespaceHandling) -> &'static str {
    match value {
        WhitespaceHandling::Skip => "SKIP_WHITESPACES",
        WhitespaceHandling::Append => "APPEND_WHITESPACES",
        WhitespaceHandling::Keep => "KEEP_WHITESPACES",
    }
}

fn normalize_option(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace(['-', ' '], "_")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn parse_args_accepts_inline_flag_values() {
        let args = parse_args(
            [
                "--dict=sgjp",
                "--dict-dir=/tmp/dicts",
                "--dictionary=/tmp/custom.tab",
                "--tagset=/tmp/tagset.dat",
            ]
            .into_iter()
            .map(ToOwned::to_owned),
        )
        .unwrap();

        assert_eq!(args.dict.as_deref(), Some("sgjp"));
        assert_eq!(
            args.dict_dir.as_deref(),
            Some(std::path::Path::new("/tmp/dicts"))
        );
        assert_eq!(
            args.dictionary.as_deref(),
            Some(std::path::Path::new("/tmp/custom.tab"))
        );
        assert_eq!(
            args.tagset.as_deref(),
            Some(std::path::Path::new("/tmp/tagset.dat"))
        );
    }

    #[test]
    fn raw_line_is_treated_as_analyze_request() {
        let mut runtime = engine_runtime(Engine::builder().build());
        let response = handle_request(&mut runtime, "Aaaa żżżż");

        assert!(response.ok);
        assert_eq!(response.results.len(), 2);
        assert_eq!(response.results[0].orth, "Aaaa");
    }

    #[test]
    fn json_generate_request_is_supported() {
        let mut runtime = engine_runtime(Engine::builder().build());
        let response = handle_request(&mut runtime, r#"{"mode":"generate","lemma":"Aaaa"}"#);

        assert!(response.ok);
        assert_eq!(response.results.len(), 1);
        assert_eq!(response.results[0].tag, "ign");
    }

    #[test]
    fn json_analyse_alias_matches_project_api_spelling() {
        let mut runtime = engine_runtime(Engine::builder().build());
        let response = handle_request(&mut runtime, r#"{"mode":"analyse","text":"Aaaa"}"#);

        assert!(response.ok);
        assert_eq!(response.results.len(), 1);
        assert_eq!(response.results[0].orth, "Aaaa");
    }

    #[test]
    fn json_runtime_options_are_applied_statefully() {
        let mut runtime = engine_runtime(Engine::builder().build());

        let keep = handle_request(
            &mut runtime,
            r#"{"mode":"analyze","text":"Aaaa  żżżż","whitespace":"keep","token_numbering":202}"#,
        );
        let next = handle_request(&mut runtime, r#"{"mode":"analyze","text":"BBBB"}"#);
        let invalid = handle_request(
            &mut runtime,
            r#"{"mode":"analyze","text":"BBBB","case_handling":"not-a-case-mode"}"#,
        );

        assert!(keep.ok);
        assert!(keep.results.iter().any(|item| item.orth == "  "));
        assert!(next.ok);
        assert_eq!(next.results[0].start_node, 3);
        assert_eq!(next.results[0].end_node, 4);
        assert!(!invalid.ok);
        assert!(invalid
            .error
            .as_deref()
            .is_some_and(|error| error.contains("invalid case_handling option")));
    }

    #[test]
    fn json_set_dictionary_switches_runtime_dictionary() {
        let temp_dir = unique_temp_dir();
        fs::create_dir_all(&temp_dir).unwrap();
        copy_dictionary_pair(&temp_dir, "svc");
        let dict_dir_json = serde_json::to_string(temp_dir.to_str().unwrap()).unwrap();
        let mut runtime = engine_runtime(Engine::builder().build());

        let switched = handle_request(
            &mut runtime,
            &format!(
                r#"{{"mode":"set_dictionary","dict":"svc","dict_dir":{dict_dir_json},"whitespace":"keep","token_numbering":"continuous"}}"#
            ),
        );
        let analyzed = handle_request(&mut runtime, r#"{"mode":"analyze","text":"7 7"}"#);
        let generated = handle_request(&mut runtime, r#"{"mode":"generate","lemma":"123"}"#);

        fs::remove_dir_all(temp_dir).unwrap();

        assert!(switched.ok);
        assert!(switched.results.is_empty());
        assert!(analyzed.ok);
        assert!(analyzed.results.iter().any(|item| item.orth == " "));
        assert!(analyzed
            .results
            .iter()
            .any(|item| item.orth == "7" && item.tag == "dig"));
        assert!(generated.ok);
        assert!(generated
            .results
            .iter()
            .any(|item| item.orth == "123" && item.tag == "dig"));
    }

    #[test]
    fn json_metadata_exposes_legacy_getter_surface() {
        let temp_dir = dictionary_pair("meta");
        let mut runtime = build_runtime(Args {
            dictionary: None,
            tagset: None,
            dict: Some("meta".to_owned()),
            dict_dir: Some(temp_dir.clone()),
        })
        .unwrap();

        let response = handle_request(
            &mut runtime,
            r#"{"mode":"metadata","whitespace":"keep","token_numbering":202,"case_handling":"ignore"}"#,
        );

        fs::remove_dir_all(temp_dir).unwrap();

        assert!(response.ok);
        assert!(response.results.is_empty());
        let metadata = response.metadata.expect("metadata response");
        assert_eq!(metadata.version, Morfeusz::version());
        assert_eq!(metadata.default_dict_name, "sgjp");
        assert_eq!(metadata.dict_id, "identyfikator_słownika");
        assert_eq!(metadata.tagset_id, "pl.sgjp.morfeusz-0.5.1");
        assert_eq!(metadata.whitespace, "KEEP_WHITESPACES");
        assert_eq!(metadata.whitespace_id, WhitespaceHandling::Keep as i32);
        assert_eq!(metadata.token_numbering, "CONTINUOUS_NUMBERING");
        assert_eq!(
            metadata.token_numbering_id,
            TokenNumbering::Continuous as i32
        );
        assert_eq!(metadata.case_handling, "IGNORE_CASE");
        assert_eq!(metadata.case_handling_id, CaseHandling::IgnoreCase as i32);
        assert!(metadata.available_aggl_options.contains(&metadata.aggl));
        assert!(metadata.available_praet_options.contains(&metadata.praet));
        assert!(metadata.copyright.starts_with("Copyright © 2014–2021"));
        assert!(!metadata.dict_copyright.is_empty());
    }

    #[test]
    fn named_binary_dictionary_pair_supports_analyze_and_generate() {
        let temp_dir = dictionary_pair("svc");
        let mut runtime = build_runtime(Args {
            dictionary: None,
            tagset: None,
            dict: Some("svc".to_owned()),
            dict_dir: Some(temp_dir.clone()),
        })
        .unwrap();

        let analyzed = handle_request(&mut runtime, r#"{"mode":"analyze","text":"7"}"#);
        let generated = handle_request(&mut runtime, r#"{"mode":"generate","lemma":"123"}"#);
        let dig_tag_id = runtime.resolver().tag_id("dig").unwrap();
        let generated_by_tag = handle_request(
            &mut runtime,
            &format!(r#"{{"mode":"generate","lemma":"123","tag_id":{dig_tag_id}}}"#),
        );
        let invalid_tag = handle_request(
            &mut runtime,
            r#"{"mode":"generate","lemma":"123","tag_id":999999}"#,
        );

        fs::remove_dir_all(temp_dir).unwrap();

        assert!(analyzed.ok);
        assert!(analyzed
            .results
            .iter()
            .any(|item| item.orth == "7" && item.tag == "dig"));
        assert!(generated.ok);
        assert!(generated
            .results
            .iter()
            .any(|item| item.orth == "123" && item.tag == "dig"));
        assert!(generated_by_tag.ok);
        assert!(!generated_by_tag.results.is_empty());
        assert!(generated_by_tag
            .results
            .iter()
            .all(|item| item.tag_id == dig_tag_id && item.tag == "dig"));
        assert!(!invalid_tag.ok);
        assert!(invalid_tag
            .error
            .as_deref()
            .is_some_and(|error| error.contains("Invalid tag id")));
    }

    #[test]
    fn default_binary_dictionary_loads_from_current_directory_when_available() {
        let temp_dir = dictionary_pair("sgjp");
        let old_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(&temp_dir).unwrap();
        let mut runtime = build_runtime(Args {
            dictionary: None,
            tagset: None,
            dict: None,
            dict_dir: None,
        })
        .unwrap();

        let response = handle_request(&mut runtime, "7");

        std::env::set_current_dir(old_dir).unwrap();
        fs::remove_dir_all(temp_dir).unwrap();

        assert!(response.ok);
        assert!(response
            .results
            .iter()
            .any(|item| item.orth == "7" && item.tag == "dig"));
    }

    fn dictionary_pair(name: &str) -> PathBuf {
        let temp_dir = unique_temp_dir();
        fs::create_dir_all(&temp_dir).unwrap();
        copy_dictionary_pair(&temp_dir, name);
        temp_dir
    }

    fn copy_dictionary_pair(temp_dir: &std::path::Path, name: &str) {
        fs::copy(
            fixture("test-dict-copyright-v1-a.dict"),
            temp_dir.join(format!("{name}-a.dict")),
        )
        .unwrap();
        fs::copy(
            fixture("test-digits-v1-s.dict"),
            temp_dir.join(format!("{name}-s.dict")),
        )
        .unwrap();
    }

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../morfeusz-rs/tests/fixtures/binary")
            .join(name)
    }

    fn unique_temp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "morfeusz-service-{}-{nanos}-{counter}",
            std::process::id()
        ))
    }
}
