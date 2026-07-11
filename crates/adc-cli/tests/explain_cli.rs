//! M0-4 受入テスト (CLIレベル): `adc explain <id>` のJSON出力とexit code。
//! 契約: stdout=データ / stderr=ログ (07-cli.md)、スキーマは docs/explain-schema.md。

use std::process::Command;

fn sample_path() -> String {
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/motor_bracket/design.ron"
    )
    .to_string()
}

fn run_explain(id: &str, design: &str) -> (i32, serde_json::Value) {
    let out = Command::new(env!("CARGO_BIN_EXE_adc"))
        .args(["explain", id, "--design", design, "--format=json"])
        .output()
        .expect("adcバイナリの実行");
    let code = out.status.code().expect("exit code");
    let stdout = String::from_utf8(out.stdout).expect("stdoutはUTF-8");
    let json = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdoutはJSONであること: {e}\n--- stdout ---\n{stdout}"));
    (code, json)
}

#[test]
fn explain_wall_t_via_cli() {
    let (code, json) = run_explain("wall_t", &sample_path());
    assert_eq!(code, 0, "一意に解決 → exit 0");
    assert_eq!(json["status"], "found");
    let refs = json["matches"][0]["referenced_by"]
        .as_array()
        .expect("referenced_by");
    assert!(
        refs.iter()
            .any(|r| r["kind"] == "feature" && r["id"] == "base" && r["via"] == "z"),
        "referenced_by に base.z (feature式): {refs:?}"
    );
    let rel = json["matches"][0]["related"].as_array().expect("related");
    assert!(
        rel.iter()
            .any(|r| r["kind"] == "assertion" && r["id"] == "a_wall"),
        "related に a_wall (assertion): {rel:?}"
    );
}

#[test]
fn explain_unknown_id_exits_1() {
    let (code, json) = run_explain("ghost", &sample_path());
    assert_eq!(code, 1);
    assert_eq!(json["status"], "not_found");
}

#[test]
fn explain_on_broken_design_exits_2_with_structured_errors() {
    let dir = std::env::temp_dir().join("adc-cli-test");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("broken.ron");
    std::fs::write(&path, "Design(").unwrap();
    let (code, json) = run_explain("x", path.to_str().unwrap());
    assert_eq!(code, 2);
    assert_eq!(json[0]["code"], "E-SCHEMA-PARSE");
}
