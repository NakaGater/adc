//! M2-1 受入テスト (US-11, US-15, US-17): Checkerトレイト+出力基盤。
//!
//! - 同一designへの2回実行で results.jsonl がバイト同一(決定性)
//! - Pass / Fail / Inconclusive の3値と exit code 0/1/2
//! - assert_id昇順の決定的順序、浮動小数の1e-9量子化
//! - Passでも margin / measured / threshold を含む (US-15)

use adc_check::{exit_code, run_checks, to_jsonl, CheckStatus, Value};
use adc_schema::{validate_design, EvalContext};

fn design(assertions: &str) -> adc_schema::Design {
    let src = format!(
        r#"Design(
    schema_version: "0.1",
    intent: "m2-1 fixture",
    params: [],
    materials: [Material(id: "a5052", density_g_cm3: 2.68, name: "A5052")],
    parts: [
        Part(id: "p1", material: "a5052", process: Machining,
            features: [Block(id: "base", x: 0.1 + 0.2, y: 60.0, z: 4.0)],
            anchors: []),
    ],
    assertions: [{assertions}],
    rationales: [Rationale(id: "r0", author: Human("t"), basis: Assumption, note: "", timestamp: "2026-07-12T00:00:00Z")],
)"#
    );
    validate_design(&src).unwrap_or_else(|e| panic!("検証: {e:#?}"))
}

const PASS_A: &str =
    r#"Assertion(id: "a_bbox", check: BoundingBox(part: "p1", max: (1.0, 70.0, 5.0)), rationale: "r0")"#;
const FAIL_A: &str =
    r#"Assertion(id: "b_bbox", check: BoundingBox(part: "p1", max: (1.0, 50.0, 5.0)), rationale: "r0")"#;
const INC_A: &str =
    r#"Assertion(id: "c_smr", check: ToolAccess(part: "p1", tool_axis: (0.0, 0.0, 1.0), tool_d: 6.0), rationale: "r0")"#;

#[test]
fn results_jsonl_is_byte_identical_across_runs() {
    let d = design(&format!("{PASS_A}, {FAIL_A}, {INC_A}"));
    let j1 = to_jsonl(&run_checks(&d, &EvalContext::nominal()));
    let j2 = to_jsonl(&run_checks(&d, &EvalContext::nominal()));
    assert_eq!(j1, j2, "同一入力でバイト同一 (Intent成功基準2)");
    assert_eq!(j1.lines().count(), 3);
}

#[test]
fn three_valued_status_and_exit_codes() {
    // Pass のみ → 0
    let d = design(PASS_A);
    let rs = run_checks(&d, &EvalContext::nominal());
    assert!(matches!(rs[0].status, CheckStatus::Pass));
    assert_eq!(exit_code(&rs), 0);

    // Fail あり → 1(Inconclusiveが同居しても1)
    let d = design(&format!("{PASS_A}, {FAIL_A}, {INC_A}"));
    let rs = run_checks(&d, &EvalContext::nominal());
    assert_eq!(exit_code(&rs), 1);

    // Fail=0 かつ Inconclusive あり → 2
    let d = design(&format!("{PASS_A}, {INC_A}"));
    let rs = run_checks(&d, &EvalContext::nominal());
    assert_eq!(exit_code(&rs), 2);
    let inc = rs.iter().find(|r| r.assert_id == "c_smr").unwrap();
    match &inc.status {
        CheckStatus::Inconclusive { reason } => {
            assert!(reason.contains("未実装"), "{reason}")
        }
        other => panic!("Inconclusiveのはず: {other:?}"),
    }
}

#[test]
fn results_are_sorted_by_assert_id_and_quantized() {
    // 宣言順は b, a — 出力は a, b の昇順
    let d = design(&format!("{FAIL_A}, {PASS_A}"));
    let rs = run_checks(&d, &EvalContext::nominal());
    assert_eq!(rs[0].assert_id, "a_bbox");
    assert_eq!(rs[1].assert_id, "b_bbox");

    // 0.1 + 0.2 → 量子化により "0.3"(0.30000000000000004 ではない)
    let jsonl = to_jsonl(&rs);
    assert!(jsonl.contains("0.3,"), "量子化: {jsonl}");
    assert!(!jsonl.contains("0.30000000000000004"), "{jsonl}");
}

#[test]
fn pass_includes_margin_measured_threshold() {
    let d = design(PASS_A);
    let rs = run_checks(&d, &EvalContext::nominal());
    let r = &rs[0];
    assert!(r.margin > 0.0, "Passでも余裕率 (US-15): {}", r.margin);
    assert!(matches!(r.measured, Value::Triple(_)));
    assert!(matches!(r.threshold, Value::Triple(_)));
    // margin = min軸 = y軸 (70-60)/70 ≈ 0.142857143
    assert!((r.margin - 10.0 / 70.0).abs() < 1e-6, "{}", r.margin);
}

#[test]
fn compile_failure_makes_inconclusive_not_fail() {
    // 過大フィレット(E-FEATURE-FAIL)でコンパイル失敗する部品 → Inconclusive
    // (M5で板金が実装されたため、失敗源をフィレットに変更 — 意図は同じ)
    let src = r#"Design(
    schema_version: "0.1",
    intent: "inconclusive fixture",
    params: [],
    materials: [Material(id: "a5052", density_g_cm3: 2.68, name: "A5052")],
    parts: [
        Part(id: "p1", material: "a5052", process: Machining,
            features: [Block(id: "base", x: 10.0, y: 10.0, z: 10.0),
                Fillet(id: "f1", edges: edges_of(feature("base").face("top")), r: 20.0)],
            anchors: []),
    ],
    assertions: [Assertion(id: "a_bbox", check: BoundingBox(part: "p1", max: (20.0, 20.0, 20.0)), rationale: "r0")],
    rationales: [Rationale(id: "r0", author: Human("t"), basis: Assumption, note: "", timestamp: "2026-07-12T00:00:00Z")],
)"#;
    let d = validate_design(src).unwrap();
    let rs = run_checks(&d, &EvalContext::nominal());
    assert!(
        matches!(rs[0].status, CheckStatus::Inconclusive { .. }),
        "{:?}",
        rs[0].status
    );
    assert_eq!(exit_code(&rs), 2);
}
