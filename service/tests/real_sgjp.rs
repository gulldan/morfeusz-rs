use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::{json, Value};

const REAL_SGJP: &str = "/tmp/morfeusz-sgjp-20260601";

#[test]
fn service_binary_handles_real_sgjp_jsonl_protocol() {
    let dict_dir = Path::new(REAL_SGJP);
    if !dict_dir.join("sgjp-a.dict").exists() || !dict_dir.join("sgjp-s.dict").exists() {
        eprintln!("skipping real SGJP service test because {REAL_SGJP} is missing");
        return;
    }

    let requests = [
        json!({
            "mode": "set_dictionary",
            "dict": "sgjp",
            "dict_dir": REAL_SGJP,
            "whitespace": "keep",
            "token_numbering": 202
        }),
        json!({"mode": "metadata"}),
        json!({"mode": "analyze", "text": "zażółć"}),
        json!({"mode": "generate", "lemma": "zażółcić"}),
    ];

    let mut child = Command::new(env!("CARGO_BIN_EXE_morfeusz-service"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    {
        let stdin = child.stdin.as_mut().unwrap();
        for request in requests {
            writeln!(stdin, "{request}").unwrap();
        }
    }

    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "service failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let responses = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(responses.len(), 4);
    assert!(responses.iter().all(|response| response["ok"] == true));

    let metadata = &responses[1]["metadata"];
    assert_eq!(metadata["dict_id"], "pl.sgjp.sgjp-2026.06.01");
    assert_eq!(metadata["tagset_id"], "pl.sgjp.morfeusz-0.8.0");
    assert_eq!(metadata["whitespace"], "KEEP_WHITESPACES");
    assert_eq!(metadata["token_numbering"], "CONTINUOUS_NUMBERING");

    let analyzed = responses[2]["results"].as_array().unwrap();
    assert_eq!(analyzed.len(), 1);
    assert_eq!(analyzed[0]["orth"], "zażółć");
    assert_eq!(analyzed[0]["lemma"], "zażółcić");
    assert_eq!(analyzed[0]["tag"], "impt:sg:sec:perf");

    let generated = responses[3]["results"].as_array().unwrap();
    assert!(generated.iter().any(|item| {
        item["orth"] == "zażółć" && item["lemma"] == "zażółcić" && item["tag"] == "impt:sg:sec:perf"
    }));
}
