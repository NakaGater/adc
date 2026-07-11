//! M4-4 受入テスト (US-27): `adc report [<results.jsonl>]`。
//!
//! - margin一覧のMarkdownテーブル(PRコメント用の顔 — 08-workflows.md)
//! - 整形品質: Fail先頭 → margin昇順。決定的
//! - suggested_range等のEvidence noteが表に出ること

use std::process::Command;

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

fn write_jsonl(name: &str, lines: &[&str]) -> String {
    let dir = std::env::temp_dir().join("adc-report-cli");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, lines.join("\n") + "\n").unwrap();
    path.to_str().unwrap().to_string()
}

const PASS_BIG: &str = r#"{"assert_id":"a_ok","checker":"bounding_box","status":"pass","measured":30.0,"threshold":40.0,"margin":0.25,"evidence":[]}"#;
const PASS_SMALL: &str = r#"{"assert_id":"b_tight","checker":"clearance","status":"pass","measured":1.1,"threshold":1.0,"margin":0.1,"evidence":[{"anchors":["i.a","j.b"],"points":[],"note":"最小距離 1.1"}]}"#;
const FAIL_BAD: &str = r#"{"assert_id":"c_bad","checker":"wall_thickness","status":"fail","measured":1.0,"threshold":2.5,"margin":-0.6,"evidence":[{"anchors":["p"],"points":[[1.0,2.0,3.0]],"note":"実測厚 1 < 2.5"}]}"#;
const FAIL_WORSE: &str = r#"{"assert_id":"d_worse","checker":"no_interference","status":"fail","measured":348.7,"threshold":0.0,"margin":-0.9,"evidence":[{"anchors":["x","y"],"points":[],"note":"交差体積 348.7 mm^3"}]}"#;
const INC: &str = r#"{"assert_id":"e_inc","checker":"sheet_metal_rules","status":{"inconclusive":{"reason":"チェッカー未実装(M2後続ユニット/T2以降)"}},"measured":null,"threshold":null,"margin":0.0,"evidence":[]}"#;

#[test]
fn report_is_markdown_table_fail_first_margin_ascending() {
    let p = write_jsonl(
        "mix.jsonl",
        &[PASS_BIG, PASS_SMALL, FAIL_BAD, FAIL_WORSE, INC],
    );
    let (code, out, err) = run(&["report", &p]);
    assert_eq!(code, 0, "{err}");

    // Markdownテーブル
    assert!(out.contains("| status |") || out.contains("|status|"), "{out}");
    assert!(out.contains("|---"), "{out}");

    // 行順: Fail先頭(margin昇順: d_worse(-0.9) → c_bad(-0.6))→
    //        Inconclusive → Pass(margin昇順: b_tight(0.1) → a_ok(0.25))
    let pos = |id: &str| out.find(id).unwrap_or_else(|| panic!("{id}が表にない: {out}"));
    assert!(pos("d_worse") < pos("c_bad"), "Fail内はmargin昇順: {out}");
    assert!(pos("c_bad") < pos("e_inc"), "Fail先頭: {out}");
    assert!(pos("e_inc") < pos("b_tight"), "Inconclusive→Pass: {out}");
    assert!(pos("b_tight") < pos("a_ok"), "Pass内もmargin昇順: {out}");

    // サマリ行(件数)とEvidence note
    assert!(out.contains("2 Fail"), "{out}");
    assert!(out.contains("交差体積"), "Evidence noteが表に出る: {out}");
    // Inconclusiveのreasonが出る
    assert!(out.contains("チェッカー未実装"), "{out}");
}

#[test]
fn report_is_deterministic_and_errors_are_exit_2() {
    let p = write_jsonl("det.jsonl", &[PASS_BIG, FAIL_BAD]);
    let (_, o1, _) = run(&["report", &p]);
    let (_, o2, _) = run(&["report", &p]);
    assert_eq!(o1, o2, "バイト再現");

    let (code, _, err) = run(&["report", "/nonexistent/results.jsonl"]);
    assert_eq!(code, 2);
    assert!(!err.is_empty());
}
