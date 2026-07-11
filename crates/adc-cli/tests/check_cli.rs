//! M2-1 受入テスト (CLIレベル): `adc check` の exit code / --format / --filter / --timings。

use std::process::Command;

fn write_design(name: &str, assertions: &str) -> String {
    let dir = std::env::temp_dir().join("adc-check-cli");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    let src = format!(
        r#"Design(
    schema_version: "0.1",
    intent: "check cli fixture",
    params: [],
    materials: [Material(id: "a5052", density_g_cm3: 2.68, name: "A5052")],
    parts: [
        Part(id: "p1", material: "a5052", process: Machining,
            features: [Block(id: "base", x: 30.0, y: 60.0, z: 4.0)],
            anchors: []),
    ],
    assertions: [{assertions}],
    rationales: [Rationale(id: "r0", author: Human("t"), basis: Assumption, note: "", timestamp: "2026-07-12T00:00:00Z")],
)"#
    );
    std::fs::write(&path, src).unwrap();
    path.to_str().unwrap().to_string()
}

fn run(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_adc"))
        .args(args)
        .output()
        .expect("adc実行");
    (
        out.status.code().unwrap(),
        String::from_utf8(out.stdout).unwrap(),
        String::from_utf8(out.stderr).unwrap(),
    )
}

const PASS_A: &str = r#"Assertion(id: "a_ok", check: BoundingBox(part: "p1", max: (40.0, 70.0, 5.0)), rationale: "r0")"#;
const FAIL_A: &str = r#"Assertion(id: "b_ng", check: BoundingBox(part: "p1", max: (20.0, 70.0, 5.0)), rationale: "r0")"#;
const INC_A: &str = r#"Assertion(id: "c_inc", check: Mass(part: "p1", max: 100.0), rationale: "r0")"#;

#[test]
fn exit_codes_0_1_2() {
    let p = write_design("pass.ron", PASS_A);
    assert_eq!(run(&["check", "--design", &p]).0, 0);

    let p = write_design("fail.ron", &format!("{PASS_A}, {FAIL_A}"));
    assert_eq!(run(&["check", "--design", &p]).0, 1);

    let p = write_design("inc.ron", &format!("{PASS_A}, {INC_A}"));
    assert_eq!(run(&["check", "--design", &p]).0, 2);
}

#[test]
fn jsonl_format_is_parseable_and_sorted() {
    let p = write_design("jsonl.ron", &format!("{FAIL_A}, {PASS_A}"));
    let (code, stdout, _) = run(&["check", "--design", &p, "--format=jsonl"]);
    assert_eq!(code, 1);
    let lines: Vec<serde_json::Value> = stdout
        .lines()
        .map(|l| serde_json::from_str(l).expect("各行がJSON"))
        .collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0]["assert_id"], "a_ok", "assert_id昇順");
    assert_eq!(lines[1]["assert_id"], "b_ng");
    assert_eq!(lines[1]["status"], "fail");
    assert!(lines[0]["margin"].is_number());
}

#[test]
fn filter_limits_assertions_and_exit_code() {
    let p = write_design("filter.ron", &format!("{PASS_A}, {FAIL_A}"));
    // Failを除外すれば exit 0
    let (code, stdout, _) = run(&["check", "--design", &p, "--format=jsonl", "--filter", "a_ok"]);
    assert_eq!(code, 0);
    assert_eq!(stdout.lines().count(), 1);
}

#[test]
fn timings_go_to_stderr_only_when_requested() {
    let p = write_design("timings.ron", PASS_A);
    let (_, stdout, stderr) = run(&["check", "--design", &p, "--format=jsonl"]);
    assert!(!stdout.contains("timing"), "正準出力に時間情報を含めない");
    assert!(!stderr.contains("timing"));

    let (_, stdout, stderr) = run(&["check", "--design", &p, "--format=jsonl", "--timings"]);
    assert!(!stdout.contains("timing"), "stdoutは正準のまま");
    assert!(stderr.contains("timing\ta_ok"), "stderrにタイミング: {stderr}");
}
