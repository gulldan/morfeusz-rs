use std::borrow::Cow;
use std::env;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

// Both CLI binaries route through this crate, so registering the global
// allocator here applies to `morfeusz_analyzer` and `morfeusz_generator`.
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use morfeusz::charset::{decode_lossy, encode_lossy};
use morfeusz::{
    BinaryDictionaryRepository, CaseHandling, Charset, Error, Morfeusz as CoreMorfeusz,
    MorfeuszUsage, MorphInterpretation, TokenNumbering, WhitespaceHandling,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Processor {
    Analyzer,
    Generator,
}

#[derive(Debug)]
struct CliOptions {
    dict: Option<String>,
    dict_dir: Option<PathBuf>,
    aggl: Option<String>,
    praet: Option<String>,
    charset: Charset,
    charset_label: Option<String>,
    case_handling: Option<CaseHandling>,
    case_handling_label: Option<String>,
    token_numbering: Option<TokenNumbering>,
    token_numbering_label: Option<String>,
    whitespace: Option<WhitespaceHandling>,
    whitespace_label: Option<String>,
    debug: bool,
    /// Worker threads for the parallel mode. `1` (default) keeps the serial
    /// path; `0` means "auto" (all available cores). Resolved at use.
    threads: usize,
    action: CliAction,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CliAction {
    Run,
    Help,
    Copyright,
    DictionaryCopyright,
}

impl Default for CliOptions {
    fn default() -> Self {
        Self {
            dict: None,
            dict_dir: None,
            aggl: None,
            praet: None,
            charset: Charset::Utf8,
            charset_label: None,
            case_handling: None,
            case_handling_label: None,
            token_numbering: None,
            token_numbering_label: None,
            whitespace: None,
            whitespace_label: None,
            debug: false,
            threads: 1,
            action: CliAction::Run,
        }
    }
}

pub fn run_analyzer() -> i32 {
    run(Processor::Analyzer)
}

pub fn run_generator() -> i32 {
    run(Processor::Generator)
}

fn run(processor: Processor) -> i32 {
    let args: Vec<String> = env::args().collect();
    eprintln!(
        "Morfeusz {}, version: {}",
        match processor {
            Processor::Analyzer => "analyzer",
            Processor::Generator => "generator",
        },
        CoreMorfeusz::version()
    );

    let options = match parse_args(&args[1..], processor) {
        Ok(options) => options,
        Err(err) => {
            eprintln!("{err}");
            return 1;
        }
    };

    match options.action {
        CliAction::Help => {
            print_usage(&args[0], processor, &options);
            0
        }
        CliAction::Copyright => {
            println!("{}", CoreMorfeusz::copyright());
            0
        }
        CliAction::DictionaryCopyright => match initialize_morfeusz(&options, processor, false) {
            Ok(morfeusz) => {
                println!("{}", morfeusz.dict_copyright());
                0
            }
            Err(err) => {
                eprintln!("Failed to start Morfeusz: {err}");
                1
            }
        },
        CliAction::Run => match initialize_morfeusz(&options, processor, true) {
            Ok(mut morfeusz) => {
                let workers = resolve_threads(options.threads);
                // Lines are independent (so parallelizable while staying
                // byte-identical to the serial path) for the generator and for
                // the analyzer under SEPARATE numbering. CONTINUOUS numbering
                // threads a running node counter across lines, so it stays
                // serial.
                let parallelizable = processor == Processor::Generator
                    || morfeusz.token_numbering() == TokenNumbering::Separate;
                let result = if workers > 1 && parallelizable {
                    process_stdin_parallel(&morfeusz, processor, options.charset, workers)
                } else {
                    process_stdin(&mut morfeusz, processor, options.charset)
                };
                result.map(|_| 0).unwrap_or_else(|err| {
                    eprintln!(
                        "Failed to perform morphosyntactic {}: {err}",
                        match processor {
                            Processor::Analyzer => "analysis",
                            Processor::Generator => "synthesis",
                        }
                    );
                    1
                })
            }
            Err(err) => {
                eprintln!("Failed to start Morfeusz: {err}");
                1
            }
        },
    }
}

fn parse_args(args: &[String], processor: Processor) -> Result<CliOptions, String> {
    let mut options = CliOptions::default();
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        match split_flag_value(arg) {
            ("-h" | "--help", value) => {
                reject_inline_value(arg, value)?;
                options.action = CliAction::Help;
            }
            ("--copyright", value) => {
                reject_inline_value(arg, value)?;
                options.action = CliAction::Copyright;
            }
            ("--dict-copyright", value) => {
                reject_inline_value(arg, value)?;
                options.action = CliAction::DictionaryCopyright;
            }
            ("-d" | "--dict", value) => {
                options.dict = Some(take_value(args, &mut index, arg, value)?);
            }
            ("--dict-dir", value) => {
                options.dict_dir = Some(PathBuf::from(take_value(args, &mut index, arg, value)?));
            }
            ("-a" | "--aggl", value) => {
                options.aggl = Some(take_value(args, &mut index, arg, value)?);
            }
            ("-p" | "--praet", value) => {
                options.praet = Some(take_value(args, &mut index, arg, value)?);
            }
            ("-c" | "--charset", value) => {
                let value = take_value(args, &mut index, arg, value)?;
                options.charset = parse_charset(&value)?;
                options.charset_label = Some(value);
            }
            ("--debug", value) => {
                reject_inline_value(arg, value)?;
                options.debug = true;
            }
            ("--threads", value) => {
                let value = take_value(args, &mut index, arg, value)?;
                options.threads = parse_threads(&value)?;
            }
            ("--case-handling", value) if processor == Processor::Analyzer => {
                let value = take_value(args, &mut index, arg, value)?;
                options.case_handling = Some(parse_case_handling(&value)?);
                options.case_handling_label = Some(value);
            }
            ("--token-numbering", value) if processor == Processor::Analyzer => {
                let value = take_value(args, &mut index, arg, value)?;
                options.token_numbering = Some(parse_token_numbering(&value)?);
                options.token_numbering_label = Some(value);
            }
            ("--whitespace-handling", value) if processor == Processor::Analyzer => {
                let value = take_value(args, &mut index, arg, value)?;
                options.whitespace = Some(parse_whitespace(&value)?);
                options.whitespace_label = Some(value);
            }
            _ => return Err(format!("Invalid argument (not bound to any flag): {arg}")),
        }
        index += 1;
    }
    Ok(options)
}

fn split_flag_value(arg: &str) -> (&str, Option<&str>) {
    arg.split_once('=')
        .map(|(flag, value)| (flag, Some(value)))
        .unwrap_or((arg, None))
}

fn reject_inline_value(arg: &str, value: Option<&str>) -> Result<(), String> {
    if value.is_some() {
        Err(format!("Invalid value for flag without argument: {arg}"))
    } else {
        Ok(())
    }
}

fn take_value(
    args: &[String],
    index: &mut usize,
    flag: &str,
    inline: Option<&str>,
) -> Result<String, String> {
    if let Some(value) = inline {
        return Ok(value.to_owned());
    }
    *index += 1;
    args.get(*index)
        .cloned()
        .ok_or_else(|| format!("Missing value for option: {flag}"))
}

fn parse_charset(value: &str) -> Result<Charset, String> {
    match value {
        "UTF8" => Ok(Charset::Utf8),
        "ISO8859_2" => Ok(Charset::Iso8859_2),
        "CP1250" => Ok(Charset::Cp1250),
        "CP852" => Ok(Charset::Cp852),
        _ => Err(format!(
            "Invalid encoding: '{value}'. Must be one of: UTF8, ISO8859_2, CP1250, CP852"
        )),
    }
}

fn parse_token_numbering(value: &str) -> Result<TokenNumbering, String> {
    match value {
        "SEPARATE_NUMBERING" => Ok(TokenNumbering::Separate),
        "CONTINUOUS_NUMBERING" => Ok(TokenNumbering::Continuous),
        _ => Err(format!(
            "Invalid token numbering: '{value}'. Must be one of: SEPARATE_NUMBERING, CONTINUOUS_NUMBERING"
        )),
    }
}

fn parse_threads(value: &str) -> Result<usize, String> {
    match value {
        "auto" | "0" => Ok(0),
        other => other
            .parse::<usize>()
            .map_err(|_| format!("Invalid thread count: '{value}'. Must be a number or 'auto'.")),
    }
}

/// Resolve the configured thread count to a concrete worker count: `0`/auto ->
/// all available cores; otherwise the requested value (at least 1).
fn resolve_threads(threads: usize) -> usize {
    if threads == 0 {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
    } else {
        threads
    }
}

fn parse_case_handling(value: &str) -> Result<CaseHandling, String> {
    match value {
        "CONDITIONALLY_CASE_SENSITIVE" => Ok(CaseHandling::ConditionallyCaseSensitive),
        "STRICTLY_CASE_SENSITIVE" => Ok(CaseHandling::StrictlyCaseSensitive),
        "IGNORE_CASE" => Ok(CaseHandling::IgnoreCase),
        _ => Err(format!(
            "Invalid case handling: '{value}'. Must be one of: CONDITIONALLY_CASE_SENSITIVE, STRICTLY_CASE_SENSITIVE, IGNORE_CASE"
        )),
    }
}

fn parse_whitespace(value: &str) -> Result<WhitespaceHandling, String> {
    match value {
        "SKIP_WHITESPACES" => Ok(WhitespaceHandling::Skip),
        "APPEND_WHITESPACES" => Ok(WhitespaceHandling::Append),
        "KEEP_WHITESPACES" => Ok(WhitespaceHandling::Keep),
        _ => Err(format!(
            "Invalid whitespace handling: '{value}'. Must be one of: SKIP_WHITESPACES, APPEND_WHITESPACES, KEEP_WHITESPACES"
        )),
    }
}

fn initialize_morfeusz(
    options: &CliOptions,
    processor: Processor,
    show_startup_options: bool,
) -> Result<CoreMorfeusz, Error> {
    let usage = match processor {
        Processor::Analyzer => MorfeuszUsage::AnalyseOnly,
        Processor::Generator => MorfeuszUsage::GenerateOnly,
    };
    if show_startup_options {
        eprintln!(
            "Setting dictionary search path to: {}",
            options
                .dict_dir
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| ".".to_owned())
        );
        match options.dict.as_deref() {
            Some(dict) => eprintln!("Using dictionary: {dict}"),
            None => eprintln!(
                "Using dictionary: {} (default)",
                CoreMorfeusz::default_dict_name()
            ),
        }
    }
    let mut morfeusz = if let Some(dict) = &options.dict {
        let direct = PathBuf::from(dict);
        if direct.exists() || direct.extension().is_some_and(|ext| ext == "dict") {
            BinaryDictionaryRepository::default().load_path(&direct, usage)?
        } else {
            let search_path = options
                .dict_dir
                .clone()
                .unwrap_or_else(|| PathBuf::from("."));
            BinaryDictionaryRepository::new([search_path]).load_named(dict, usage)?
        }
    } else {
        let search_path = options
            .dict_dir
            .clone()
            .unwrap_or_else(|| PathBuf::from("."));
        BinaryDictionaryRepository::new([search_path])
            .load_named(CoreMorfeusz::default_dict_name(), usage)?
    };

    if let Some(aggl) = &options.aggl {
        eprintln!("setting aggl option to {aggl}");
        morfeusz.set_aggl(aggl)?;
    }
    if let Some(praet) = &options.praet {
        eprintln!("setting praet option to {praet}");
        morfeusz.set_praet(praet)?;
    }
    if options.debug {
        eprintln!("setting debug to TRUE");
        morfeusz.set_debug(true);
    }
    if let Some(charset) = &options.charset_label {
        eprintln!("setting charset to {charset}");
    }
    morfeusz.set_charset(options.charset);
    if let Some(case_handling) = options.case_handling {
        if let Some(label) = &options.case_handling_label {
            eprintln!("setting case handling to {label}");
        }
        morfeusz.set_case_handling(case_handling);
    }
    if let Some(token_numbering) = options.token_numbering {
        if let Some(label) = &options.token_numbering_label {
            eprintln!("setting token numbering to {label}");
        }
        morfeusz.set_token_numbering(token_numbering);
    }
    if let Some(whitespace) = options.whitespace {
        if let Some(label) = &options.whitespace_label {
            eprintln!("setting whitespace handling to {label}");
        }
        morfeusz.set_whitespace_handling(whitespace);
    }
    Ok(morfeusz)
}

fn process_stdin(
    morfeusz: &mut CoreMorfeusz,
    processor: Processor,
    charset: Charset,
) -> Result<(), Error> {
    // Block-buffer stdout: the default Stdout is line-buffered, so emitting one
    // result block per input line would issue a write(2) syscall per line. A
    // BufWriter batches them, which is the single biggest CLI throughput win.
    let stdin = io::stdin();
    let mut stdin = io::BufReader::new(stdin.lock());
    let mut stdout = io::BufWriter::new(io::stdout().lock());
    let mut line_bytes = Vec::new();
    let mut output = String::new();

    loop {
        line_bytes.clear();
        if stdin
            .read_until(b'\n', &mut line_bytes)
            .map_err(Error::Io)?
            == 0
        {
            break;
        }
        if line_bytes.ends_with(b"\n") {
            line_bytes.pop();
        }
        if line_bytes.ends_with(b"\r") {
            line_bytes.pop();
        }
        let line = decode_line(charset, &line_bytes);
        let results = match processor {
            Processor::Analyzer => morfeusz.analyse(line.as_ref())?,
            Processor::Generator => morfeusz.generate(line.as_ref())?,
        };
        write_results_encoded(
            &mut stdout,
            charset,
            morfeusz,
            &results,
            processor == Processor::Analyzer,
            &mut output,
        )?;
    }
    write_encoded(&mut stdout, charset, "\n")?;
    stdout.flush().map_err(Error::Io)?;
    Ok(())
}

/// Parallel counterpart of [`process_stdin`] for the opt-in `--threads` mode.
///
/// Lines are read in batches; each batch is analysed across a work-stealing
/// pool and the per-line output blocks are written back in input order. Because
/// every line is formatted by the same `format_results_into` and analysed from
/// node 0 (SEPARATE numbering) or without numbering (generator), the byte stream
/// is identical to the serial path — only the work is spread across cores.
fn process_stdin_parallel(
    morfeusz: &CoreMorfeusz,
    processor: Processor,
    charset: Charset,
    workers: usize,
) -> Result<(), Error> {
    use rayon::prelude::*;
    use std::sync::mpsc::sync_channel;

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(workers)
        .build()
        .map_err(|err| Error::Io(io::Error::other(err.to_string())))?;

    let stdin = io::stdin();
    let mut stdin = io::BufReader::new(stdin.lock());
    let print_node_numbers = processor == Processor::Analyzer;

    // One batch in flight at a time: large enough to amortize fan-out and keep
    // every core busy, small enough to bound memory regardless of input size.
    const BATCH_LINES: usize = 16_384;

    // A dedicated writer thread consumes finished batches in order while the
    // pool computes the next one, so the serial output write (gigabytes on a
    // large corpus) overlaps compute instead of stalling every core. The
    // bounded channel both preserves order and caps how many batches sit in
    // memory at once.
    std::thread::scope(|scope| -> Result<(), Error> {
        let (tx, rx) = sync_channel::<Vec<Vec<u8>>>(2);
        let writer = scope.spawn(move || -> Result<(), Error> {
            let mut stdout = io::BufWriter::new(io::stdout().lock());
            for blocks in rx {
                for block in blocks {
                    stdout.write_all(&block).map_err(Error::Io)?;
                }
            }
            write_encoded(&mut stdout, charset, "\n")?;
            stdout.flush().map_err(Error::Io)?;
            Ok(())
        });

        let mut compute = || -> Result<(), Error> {
            let mut batch: Vec<Vec<u8>> = Vec::with_capacity(BATCH_LINES);
            loop {
                batch.clear();
                while batch.len() < BATCH_LINES {
                    let mut line_bytes = Vec::new();
                    if stdin.read_until(b'\n', &mut line_bytes).map_err(Error::Io)? == 0 {
                        break;
                    }
                    if line_bytes.ends_with(b"\n") {
                        line_bytes.pop();
                    }
                    if line_bytes.ends_with(b"\r") {
                        line_bytes.pop();
                    }
                    batch.push(line_bytes);
                }
                if batch.is_empty() {
                    break;
                }

                // Each worker forks its own analyzer once (`map_init`): the
                // dictionary is shared (Arc), but the decode caches are
                // per-thread, so workers never contend on a shared cache lock.
                // rayon's parallel `map` + ordered `collect` keeps the output
                // blocks in input order, so the byte stream matches the serial
                // loop exactly. A per-line error short-circuits the batch.
                let blocks: Vec<Vec<u8>> = pool.install(|| {
                    batch
                        .par_iter()
                        .map_init(
                            || morfeusz.fork(),
                            |local, line_bytes| {
                                let line = decode_line(charset, line_bytes);
                                let results = match processor {
                                    Processor::Analyzer => local.analyse_from(line.as_ref(), 0)?.0,
                                    Processor::Generator => local.generate(line.as_ref())?,
                                };
                                let mut block = String::new();
                                format_results_into(&mut block, local, &results, print_node_numbers);
                                Ok::<_, Error>(encode_block(charset, block))
                            },
                        )
                        .collect::<Result<Vec<Vec<u8>>, Error>>()
                })?;

                // If the writer has stopped (write error), stop feeding it; its
                // error is recovered from the join below.
                if tx.send(blocks).is_err() {
                    break;
                }
            }
            Ok(())
        };

        let compute_result = compute();
        // Close the channel so the writer finishes, then surface a compute error
        // first, otherwise any write/flush error from the writer.
        drop(tx);
        let writer_result = writer.join().expect("output writer thread panicked");
        compute_result?;
        writer_result
    })
}

fn encode_block(charset: Charset, value: String) -> Vec<u8> {
    match charset {
        // UTF-8 output is already the right bytes — move, don't copy.
        Charset::Utf8 => value.into_bytes(),
        _ => encode_lossy(charset, &value),
    }
}

fn decode_line(charset: Charset, bytes: &[u8]) -> Cow<'_, str> {
    match charset {
        Charset::Utf8 => String::from_utf8_lossy(bytes),
        _ => Cow::Owned(decode_lossy(charset, bytes)),
    }
}

fn write_encoded(writer: &mut impl Write, charset: Charset, value: &str) -> Result<(), Error> {
    match charset {
        Charset::Utf8 => writer.write_all(value.as_bytes()).map_err(Error::Io),
        _ => writer
            .write_all(&encode_lossy(charset, value))
            .map_err(Error::Io),
    }
}

fn write_results_encoded(
    writer: &mut impl Write,
    charset: Charset,
    morfeusz: &CoreMorfeusz,
    results: &[MorphInterpretation],
    print_node_numbers: bool,
    scratch: &mut String,
) -> Result<(), Error> {
    scratch.clear();
    format_results_into(scratch, morfeusz, results, print_node_numbers);
    write_encoded(writer, charset, scratch)
}

fn format_results_into(
    out: &mut String,
    morfeusz: &CoreMorfeusz,
    results: &[MorphInterpretation],
    print_node_numbers: bool,
) {
    out.push('[');
    let mut prev = None;
    let resolver = morfeusz.id_resolver();
    let mut start_node_buf = itoa::Buffer::new();
    let mut end_node_buf = itoa::Buffer::new();
    for item in results {
        let current = (item.start_node, item.end_node);
        match prev {
            Some(previous) if previous != current => out.push_str("]\n["),
            Some(_) => out.push_str("\n "),
            None => {}
        }
        if print_node_numbers {
            out.push_str(start_node_buf.format(item.start_node));
            out.push(',');
            out.push_str(end_node_buf.format(item.end_node));
            out.push(',');
        }
        let tag = item.tag(resolver).unwrap_or("ign");
        let name = if item.name_id == 0 {
            "_"
        } else {
            item.name(resolver).unwrap_or("_")
        };
        let labels = if item.labels_id == 0 {
            "_"
        } else {
            item.labels_as_string(resolver).unwrap_or("_")
        };
        out.push_str(&item.orth);
        out.push(',');
        out.push_str(&item.lemma);
        out.push(',');
        out.push_str(tag);
        out.push(',');
        out.push_str(name);
        out.push(',');
        out.push_str(labels);
        prev = Some(current);
    }
    out.push_str("]\n");
}

#[cfg(test)]
fn format_results(
    morfeusz: &CoreMorfeusz,
    results: &[MorphInterpretation],
    print_node_numbers: bool,
) -> String {
    let mut out = String::new();
    format_results_into(&mut out, morfeusz, results, print_node_numbers);
    out
}

fn print_usage(program: &str, processor: Processor, options: &CliOptions) {
    let (aggl_options, aggl_default, praet_options, praet_default) =
        help_segmentation_options(options, processor);
    let aggl_options = format_option_list(&aggl_options, aggl_default.as_deref());
    let praet_options = format_option_list(&praet_options, praet_default.as_deref());
    println!(
        "{}",
        match processor {
            Processor::Analyzer => format!(
                "Morfeusz analyzer\n\nUSAGE: {program} [OPTIONS]\n\nOPTIONS:\n\n-a, --aggl ARG              select agglutination rules (provide --dict and optionally --dict-dir options to see values for given custom dictionary):\n{aggl_options}-c, --charset ARG           input/output charset:\n                             * UTF8 (default)\n                             * ISO8859_2\n                             * CP1250\n                             * CP852\n-d, --dict ARG              dictionary name\n-h, --help                  Display usage instructions.\n-p, --praet ARG             select past tense segmentation (provide --dict and optionally --dict-dir options to see values for given custom dictionary):\n{praet_options}--case-handling ARG         case handling strategy\n                             * CONDITIONALLY_CASE_SENSITIVE (default) - Case-sensitive but allows interpretations that do not match case when there is no alternative\n                             * STRICTLY_CASE_SENSITIVE - strictly case-sensitive\n                             * IGNORE_CASE - ignores case\n--copyright                 Display morfeusz2 library copyright information.\n--debug                     show some debug information.\n--dict-copyright            Display dictionary copyright information.\n--dict-dir ARG              directory containing the dictionary (default is current dir)\n--token-numbering ARG       token numbering strategy\n                             * SEPARATE_NUMBERING (default) - Start from 0 and reset counter for each line of input text.\n                             * CONTINUOUS_NUMBERING - start from 0 and never reset counter\n--whitespace-handling ARG   whitespace handling strategy.\n                             * SKIP_WHITESPACES (default) - ignore whitespaces\n                             * APPEND_WHITESPACES - append whitespaces to preceding segment\n                             * KEEP_WHITESPACES - whitespaces are separate segments\n--threads ARG               worker threads for parallel analysis (default 1; 'auto' = all cores); output is byte-identical to single-threaded (CONTINUOUS_NUMBERING stays serial)\nEXAMPLES:\n\n{program} --aggl strict --praet split --dict sgjp --dict-dir /tmp/dictionaries\n"
            ),
            Processor::Generator => format!(
                "Morfeusz generator\n\nUSAGE: {program} [OPTIONS]\n\nOPTIONS:\n\n-a, --aggl ARG      select agglutination rules (provide --dict and optionally --dict-dir options to see values for given custom dictionary):\n{aggl_options}-c, --charset ARG   input/output charset:\n                     * UTF8 (default)\n                     * ISO8859_2\n                     * CP1250\n                     * CP852\n-d, --dict ARG      dictionary name\n-h, --help          Display usage instructions.\n-p, --praet ARG     select past tense segmentation (provide --dict and optionally --dict-dir options to see values for given custom dictionary):\n{praet_options}--copyright         Display morfeusz2 library copyright information.\n--debug             show some debug information.\n--dict-copyright    Display dictionary copyright information.\n--dict-dir ARG      directory containing the dictionary (default is current dir)\n--threads ARG       worker threads for parallel synthesis (default 1; 'auto' = all cores); output is byte-identical to single-threaded\nEXAMPLES:\n\n{program} --aggl strict --praet split --dict sgjp --dict-dir /tmp/dictionaries\n"
            ),
        }
    );
}

fn help_segmentation_options(
    options: &CliOptions,
    processor: Processor,
) -> (Vec<String>, Option<String>, Vec<String>, Option<String>) {
    match initialize_morfeusz(options, processor, false) {
        Ok(morfeusz) => (
            morfeusz.available_aggl_options(),
            Some(morfeusz.aggl().to_owned()),
            morfeusz.available_praet_options(),
            Some(morfeusz.praet().to_owned()),
        ),
        Err(_) => (
            vec![
                "isolated".to_owned(),
                "permissive".to_owned(),
                "strict".to_owned(),
            ],
            None,
            vec!["composite".to_owned(), "split".to_owned()],
            None,
        ),
    }
}

fn format_option_list(options: &[String], default: Option<&str>) -> String {
    let mut options = options.to_vec();
    options.sort();
    options
        .into_iter()
        .map(|option| {
            let suffix = if Some(option.as_str()) == default {
                " (default)"
            } else {
                ""
            };
            format!("                             * {option}{suffix}\n")
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_analysis_like_legacy_output() {
        let morfeusz =
            CoreMorfeusz::with_dictionary(Default::default(), MorfeuszUsage::AnalyseOnly);
        let results = vec![MorphInterpretation::create_ign(0, 1, "Aaaa", "Aaaa")];

        assert_eq!(
            format_results(&morfeusz, &results, true),
            "[0,1,Aaaa,Aaaa,ign,_,_]\n"
        );
    }

    #[test]
    fn parses_analyzer_specific_options() {
        let args = vec![
            "--token-numbering=CONTINUOUS_NUMBERING".to_owned(),
            "--case-handling".to_owned(),
            "IGNORE_CASE".to_owned(),
            "--whitespace-handling".to_owned(),
            "KEEP_WHITESPACES".to_owned(),
            "--charset".to_owned(),
            "CP1250".to_owned(),
        ];

        let options = parse_args(&args, Processor::Analyzer).unwrap();

        assert_eq!(options.token_numbering, Some(TokenNumbering::Continuous));
        assert_eq!(options.case_handling, Some(CaseHandling::IgnoreCase));
        assert_eq!(options.case_handling_label.as_deref(), Some("IGNORE_CASE"));
        assert_eq!(
            options.token_numbering_label.as_deref(),
            Some("CONTINUOUS_NUMBERING")
        );
        assert_eq!(options.whitespace, Some(WhitespaceHandling::Keep));
        assert_eq!(
            options.whitespace_label.as_deref(),
            Some("KEEP_WHITESPACES")
        );
        assert_eq!(options.charset, Charset::Cp1250);
        assert_eq!(options.charset_label.as_deref(), Some("CP1250"));
    }
}
