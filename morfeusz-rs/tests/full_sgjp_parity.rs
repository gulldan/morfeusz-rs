use std::path::{Path, PathBuf};
use std::process::Command;

const DEFAULT_REAL_SGJP_DIR: &str = "/tmp/morfeusz-sgjp-20260601";
const DICT_NAME: &str = "sgjp";

#[test]
#[ignore = "runs the full real SGJP analyzer corpus against the C++ reference"]
fn full_real_sgjp_analyzer_matches_cpp_reference() {
    run_full_sgjp_cmp("analyzer", "morfeusz_analyzer", "forms.txt");
}

#[test]
#[ignore = "runs the full real SGJP generator corpus against the C++ reference"]
fn full_real_sgjp_generator_matches_cpp_reference() {
    run_full_sgjp_cmp("generator", "morfeusz_generator", "lemmas.txt");
}

fn run_full_sgjp_cmp(label: &str, binary_name: &str, corpus_name: &str) {
    let repo = repo_root();
    let dict_dir = std::env::var_os("MORFEUSZ_REAL_SGJP_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_REAL_SGJP_DIR));
    let corpus = dict_dir.join(corpus_name);
    let analyzer_dict = dict_dir.join("sgjp-a.dict");
    let generator_dict = dict_dir.join("sgjp-s.dict");
    if !corpus.exists() || !analyzer_dict.exists() || !generator_dict.exists() {
        eprintln!(
            "skipping full SGJP {label} parity test because {dict_dir} is incomplete",
            dict_dir = dict_dir.display()
        );
        return;
    }

    let cpp = repo.join("build-cpp-ref-O2").join(binary_name);
    let rust = repo.join("rust/target/release").join(binary_name);
    assert!(
        cpp.exists(),
        "missing C++ reference binary: {}",
        cpp.display()
    );
    assert!(
        rust.exists(),
        "missing Rust release binary: {}; run cargo build --release --workspace",
        rust.display()
    );

    let script = r#"
set -euo pipefail
if cmp -s <("$CPP_BIN" --dict-dir "$DICT_DIR" --dict "$DICT_NAME" < "$CORPUS" 2>/dev/null) <("$RUST_BIN" --dict-dir "$DICT_DIR" --dict "$DICT_NAME" < "$CORPUS" 2>/dev/null); then
  echo "full SGJP ${LABEL} cmp ok"
else
  status=$?
  echo "full SGJP ${LABEL} cmp failed" >&2
  exit "$status"
fi
"#;

    let output = Command::new("bash")
        .arg("-lc")
        .arg(script)
        .env("CPP_BIN", &cpp)
        .env("RUST_BIN", &rust)
        .env("DICT_DIR", &dict_dir)
        .env("DICT_NAME", DICT_NAME)
        .env("CORPUS", &corpus)
        .env("LABEL", label)
        .output()
        .expect("failed to execute bash full SGJP parity harness");

    assert!(
        output.status.success(),
        "full SGJP {label} parity failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    eprint!("{}", String::from_utf8_lossy(&output.stderr));
    print!("{}", String::from_utf8_lossy(&output.stdout));
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("morfeusz-rs crate lives at the workspace root")
        .to_path_buf()
}
