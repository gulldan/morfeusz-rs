use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use morfeusz::charset::encode_lossy;
use morfeusz::Charset;

fn fixture(name: &str) -> String {
    format!(
        "{}/../morfeusz-rs/tests/fixtures/binary/{name}",
        env!("CARGO_MANIFEST_DIR")
    )
}

#[test]
fn analyzer_binary_prints_legacy_rows() {
    let output = run_with_stdin(
        env!("CARGO_BIN_EXE_morfeusz_analyzer"),
        &["--dict", &fixture("test-dict-copyright-v1-a.dict")],
        b"7\n",
    );

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "[0,1,7,7,dig,_,_]\n\n"
    );
}

#[test]
fn analyzer_binary_loads_named_dictionary_from_dict_dir() {
    let temp_dir = unique_temp_dir();
    fs::create_dir_all(&temp_dir).unwrap();
    fs::copy(
        fixture("test-dict-copyright-v1-a.dict"),
        temp_dir.join("cli-smoke-a.dict"),
    )
    .unwrap();

    let output = run_with_stdin(
        env!("CARGO_BIN_EXE_morfeusz_analyzer"),
        &[
            "--dict",
            "cli-smoke",
            "--dict-dir",
            temp_dir.to_str().unwrap(),
        ],
        b"7\n",
    );

    fs::remove_dir_all(&temp_dir).unwrap();

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "[0,1,7,7,dig,_,_]\n\n"
    );
}

#[test]
fn analyzer_binary_uses_default_dictionary_name_from_dict_dir() {
    let temp_dir = unique_temp_dir();
    fs::create_dir_all(&temp_dir).unwrap();
    fs::copy(
        fixture("test-dict-copyright-v1-a.dict"),
        temp_dir.join("sgjp-a.dict"),
    )
    .unwrap();

    let output = run_with_stdin(
        env!("CARGO_BIN_EXE_morfeusz_analyzer"),
        &["--dict-dir", temp_dir.to_str().unwrap()],
        b"7\n",
    );

    fs::remove_dir_all(&temp_dir).unwrap();

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "[0,1,7,7,dig,_,_]\n\n"
    );
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("Morfeusz analyzer, version: 1.99.15\n"));
    assert!(stderr.contains(&format!(
        "Setting dictionary search path to: {}\n",
        temp_dir.display()
    )));
    assert!(stderr.contains("Using dictionary: sgjp (default)\n"));
}

#[test]
fn analyzer_binary_round_trips_cp1250_charset_bytes() {
    let input = encode_lossy(Charset::Cp1250, "zażółć\n");

    let output = run_with_stdin(
        env!("CARGO_BIN_EXE_morfeusz_analyzer"),
        &[
            "--dict",
            &fixture("test-dict-copyright-v1-a.dict"),
            "--charset",
            "CP1250",
        ],
        &input,
    );

    assert!(output.status.success());
    assert_eq!(
        output.stdout,
        encode_lossy(Charset::Cp1250, "[0,1,zażółć,zażółć,ign,_,_]\n\n")
    );
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("setting charset to CP1250\n"));
}

#[test]
fn analyzer_binary_prints_legacy_option_diagnostics() {
    let output = run_with_stdin(
        env!("CARGO_BIN_EXE_morfeusz_analyzer"),
        &[
            "--dict",
            &fixture("test-dict-copyright-v1-a.dict"),
            "--case-handling",
            "IGNORE_CASE",
            "--token-numbering",
            "CONTINUOUS_NUMBERING",
            "--whitespace-handling",
            "KEEP_WHITESPACES",
        ],
        b"7 7\n",
    );

    assert!(output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("setting case handling to IGNORE_CASE\n"));
    assert!(stderr.contains("setting token numbering to CONTINUOUS_NUMBERING\n"));
    assert!(stderr.contains("setting whitespace handling to KEEP_WHITESPACES\n"));
}

#[test]
fn generator_binary_prints_legacy_rows() {
    let output = run_with_stdin(
        env!("CARGO_BIN_EXE_morfeusz_generator"),
        &["--dict", &fixture("test-digits-v1-s.dict")],
        b"123\n",
    );

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "[123,123,dig,_,_]\n\n"
    );
}

#[test]
fn generator_binary_uses_default_dictionary_name_from_dict_dir() {
    let temp_dir = unique_temp_dir();
    fs::create_dir_all(&temp_dir).unwrap();
    fs::copy(
        fixture("test-digits-v1-s.dict"),
        temp_dir.join("sgjp-s.dict"),
    )
    .unwrap();

    let output = run_with_stdin(
        env!("CARGO_BIN_EXE_morfeusz_generator"),
        &["--dict-dir", temp_dir.to_str().unwrap()],
        b"123\n",
    );

    fs::remove_dir_all(&temp_dir).unwrap();

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "[123,123,dig,_,_]\n\n"
    );
}

#[test]
fn analyzer_binary_prints_current_library_copyright() {
    let output = Command::new(env!("CARGO_BIN_EXE_morfeusz_analyzer"))
        .arg("--copyright")
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.starts_with("Copyright © 2014–2021 by Institute of Computer Science"));
}

#[test]
fn analyzer_binary_dict_copyright_does_not_print_startup_dictionary_messages() {
    let output = Command::new(env!("CARGO_BIN_EXE_morfeusz_analyzer"))
        .args([
            "--dict",
            &fixture("test-dict-copyright-v1-a.dict"),
            "--dict-copyright",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(stderr, "Morfeusz analyzer, version: 1.99.15\n");
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(stdout, "To jest testowa notka copyrightowa.\n\n");
}

#[test]
fn analyzer_help_lists_debug_and_dictionary_segmentation_options() {
    let temp_dir = unique_temp_dir();
    fs::create_dir_all(&temp_dir).unwrap();
    fs::copy(
        fixture("test-dict-copyright-v1-a.dict"),
        temp_dir.join("sgjp-a.dict"),
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_morfeusz_analyzer"))
        .args(["--dict-dir", temp_dir.to_str().unwrap(), "--help"])
        .output()
        .unwrap();

    fs::remove_dir_all(&temp_dir).unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("--debug"));
    assert!(stdout.contains("select agglutination rules"));
    assert!(stdout.contains(" * isolated"));
    assert!(stdout.contains(" * permissive"));
    assert!(stdout.contains(" * strict"));
    assert!(stdout.contains("select past tense segmentation"));
    assert!(stdout.contains(" * split"));
}

fn unique_temp_dir() -> PathBuf {
    static NEXT_ID: AtomicU64 = AtomicU64::new(0);
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("morfeusz-cli-smoke-{}-{id}", std::process::id()))
}

fn run_with_stdin(program: &str, args: &[&str], stdin: &[u8]) -> std::process::Output {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.as_mut().unwrap().write_all(stdin).unwrap();
    child.wait_with_output().unwrap()
}
