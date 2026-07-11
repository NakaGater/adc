//! M2-3 受入テスト (US-13): Mass / Cog。

use adc_check::{run_checks, CheckStatus, Value};
use adc_schema::{parse_design, validate_design, EvalContext};

fn design(parts: &str, assertions: &str) -> adc_schema::Design {
    let src = format!(
        r#"Design(
    schema_version: "0.1",
    intent: "m2-3 fixture",
    params: [],
    materials: [Material(id: "a5052", density_g_cm3: 2.68, name: "A5052")],
    parts: [{parts}],
    assertions: [{assertions}],
    rationales: [Rationale(id: "r0", author: Human("t"), basis: Assumption, note: "", timestamp: "2026-07-12T00:00:00Z")],
)"#
    );
    validate_design(&src).unwrap_or_else(|e| panic!("検証: {e:#?}"))
}

const CUBE10: &str = r#"Part(id: "p1", material: "a5052", process: Machining,
    features: [Block(id: "base", x: 10.0, y: 10.0, z: 10.0)], anchors: [])"#;

#[test]
fn mass_unit_conversion_g_per_cm3_to_mm() {
    // 単位換算の典型バグポイント: 10mm立方 = 1 cm³ → 2.68 g ちょうど
    let d = design(
        CUBE10,
        r#"Assertion(id: "a_mass", check: Mass(part: "p1", max: 3.0), rationale: "r0")"#,
    );
    let rs = run_checks(&d, &EvalContext::nominal());
    let r = &rs[0];
    assert!(matches!(r.status, CheckStatus::Pass), "{:?}", r.status);
    assert!(
        matches!(r.measured, Value::Scalar(m) if (m - 2.68).abs() < 1e-9),
        "質量2.68g: {:?}",
        r.measured
    );
    // margin = (3 - 2.68)/3
    assert!((r.margin - (3.0 - 2.68) / 3.0).abs() < 1e-6, "{}", r.margin);
}

#[test]
fn mass_fail_and_min_bound() {
    let d = design(
        CUBE10,
        r#"Assertion(id: "a_mass", check: Mass(part: "p1", max: 2.0), rationale: "r0")"#,
    );
    let rs = run_checks(&d, &EvalContext::nominal());
    assert!(matches!(rs[0].status, CheckStatus::Fail));
    assert!(rs[0].evidence[0].note.contains("上限"), "{}", rs[0].evidence[0].note);

    // min違反
    let d = design(
        CUBE10,
        r#"Assertion(id: "a_mass", check: Mass(part: "p1", max: 10.0, min: 5.0), rationale: "r0")"#,
    );
    let rs = run_checks(&d, &EvalContext::nominal());
    assert!(matches!(rs[0].status, CheckStatus::Fail));
    assert!(rs[0].evidence[0].note.contains("下限"), "{}", rs[0].evidence[0].note);
    assert!(rs[0].margin < 0.0);
}

#[test]
fn undefined_material_is_inconclusive() {
    // 材料未定義はInconclusive(仕様どおり)— 検証をスキップしてパースのみで構築
    let src = r#"Design(
    schema_version: "0.1",
    intent: "no material",
    params: [],
    materials: [],
    parts: [
        Part(id: "p1", material: "ghost", process: Machining,
            features: [Block(id: "base", x: 10.0, y: 10.0, z: 10.0)], anchors: []),
    ],
    assertions: [Assertion(id: "a_mass", check: Mass(part: "p1", max: 3.0), rationale: "r0")],
    rationales: [Rationale(id: "r0", author: Human("t"), basis: Assumption, note: "", timestamp: "2026-07-12T00:00:00Z")],
)"#;
    let d = parse_design(src).expect("パース");
    let rs = run_checks(&d, &EvalContext::nominal());
    match &rs[0].status {
        CheckStatus::Inconclusive { reason } => assert!(reason.contains("材料"), "{reason}"),
        other => panic!("Inconclusiveのはず: {other:?}"),
    }
}

#[test]
fn cog_pass_and_fail_with_deviating_axis() {
    // 10mm立方の重心 = (5,5,5)
    let d = design(
        CUBE10,
        r#"Assertion(id: "a_cog", check: Cog(within: BoxSpec(min: (4.0, 4.0, 4.0), max: (6.0, 6.0, 6.0))), rationale: "r0")"#,
    );
    let rs = run_checks(&d, &EvalContext::nominal());
    let r = &rs[0];
    assert!(matches!(r.status, CheckStatus::Pass), "{:?}", r.status);
    assert!(matches!(r.measured, Value::Triple(c) if (c[0]-5.0).abs()<1e-9), "{:?}", r.measured);
    assert!((r.margin - 1.0).abs() < 1e-6, "中心ど真ん中 → margin=1: {}", r.margin);

    // z軸で逸脱するbox
    let d = design(
        CUBE10,
        r#"Assertion(id: "a_cog", check: Cog(within: BoxSpec(min: (4.0, 4.0, 6.0), max: (6.0, 6.0, 8.0))), rationale: "r0")"#,
    );
    let rs = run_checks(&d, &EvalContext::nominal());
    let r = &rs[0];
    assert!(matches!(r.status, CheckStatus::Fail));
    let ev = &r.evidence[0];
    assert_eq!(ev.points[0], [5.0, 5.0, 5.0], "実測重心座標");
    assert!(ev.note.contains("逸脱軸: z"), "{}", ev.note);
}
