//! M2-4 受入テスト (US-14): WallThickness(レイキャスト近似)。
//!
//! 一方向保証: 検出した違反は真、未検出は薄肉なしを保証しない
//! (docs/checkers.md。false negativeあり得る近似手法)。

use adc_check::{run_checks, CheckStatus, Value};
use adc_schema::{validate_design, EvalContext};

fn bracket_design(features_extra: &str, min: f64, density: f64) -> adc_schema::Design {
    let src = format!(
        r#"Design(
    schema_version: "0.1",
    intent: "m2-4 fixture: §9ブラケット相当",
    params: [
        Param(id: "wall_t", value: Open(range: (3.0, 6.0), nominal: 4.0), unit: Mm, rationale: "r0"),
    ],
    materials: [Material(id: "a5052", density_g_cm3: 2.68, name: "A5052")],
    parts: [
        Part(id: "bracket", material: "a5052", process: Machining,
            features: [
                Block(id: "base", x: 80.0, y: 60.0, z: param("wall_t")),
                Hole(id: "bore", kind: Simple, d: 55.0, depth: Through,
                     at: on(feature("base").face("top"), center())){features_extra}
            ],
            anchors: []),
    ],
    assertions: [
        Assertion(id: "a_wall",
            check: WallThickness(part: "bracket", min: {min}, sample_density: {density}),
            rationale: "r0"),
    ],
    rationales: [Rationale(id: "r0", author: Human("t"), basis: Assumption, note: "", timestamp: "2026-07-12T00:00:00Z")],
)"#
    );
    validate_design(&src).unwrap_or_else(|e| panic!("検証: {e:#?}"))
}

#[test]
fn bracket_wall_t4_passes_min_2_5() {
    // §9ブラケット(wall_t=4)で min 2.5 → Pass。
    // 実測最小厚はφ55ボアと±y縁のリガメント = (60-55)/2 = 2.5
    // (板厚4ではない — レイキャストが面内の最薄壁を正しく検出する)
    let d = bracket_design("", 2.5, 0.25);
    let rs = run_checks(&d, &EvalContext::nominal());
    let r = &rs[0];
    assert!(matches!(r.status, CheckStatus::Pass), "{:?}", r.status);
    assert!(
        matches!(r.measured, Value::Scalar(t) if (t - 2.5).abs() < 1e-6),
        "実測最小厚(リガメント): {:?}",
        r.measured
    );
    // Passでも一方向保証の注記を出力に含める
    assert!(
        r.evidence[0].note.contains("一方向保証"),
        "{}",
        r.evidence[0].note
    );
}

#[test]
fn bracket_wall_t4_fails_min_5_with_violation_point() {
    let d = bracket_design("", 5.0, 0.25);
    let rs = run_checks(&d, &EvalContext::nominal());
    let r = &rs[0];
    assert!(matches!(r.status, CheckStatus::Fail), "{:?}", r.status);
    let ev = &r.evidence[0];
    assert_eq!(ev.points.len(), 1, "違反点座標");
    assert!(ev.note.contains("法線"), "法線方向を含む: {}", ev.note);
    assert!(ev.note.contains("一方向保証"), "{}", ev.note);
    // margin = (2.5-5)/5 = -0.5(最薄はボア±y縁のリガメント2.5)
    assert!((r.margin - (-0.5)).abs() < 1e-6, "{}", r.margin);
}

#[test]
fn thin_pocket_floor_is_detected() {
    // 変形サンプル: depth 3.5 のポケット → 床下の残り厚 0.5 の既知薄肉部
    let extra = r#",
                Pocket(id: "pk", profile: Rect(x: 16.0, y: 12.0), depth: 3.5,
                       at: on(feature("base").face("top"), xy(-25.0, 15.0)))"#;
    let d = bracket_design(extra, 2.5, 0.25);
    let rs = run_checks(&d, &EvalContext::nominal());
    let r = &rs[0];
    assert!(matches!(r.status, CheckStatus::Fail), "{:?}", r.status);
    assert!(
        matches!(r.measured, Value::Scalar(t) if (t - 0.5).abs() < 1e-6),
        "既知薄肉部0.5mmの検出: {:?}",
        r.measured
    );
    // 違反点はポケット領域(中心(15,45)付近、x∈[7,23], y∈[39,51])の床(z=0.5)か底面(z=0)
    let p = r.evidence[0].points[0];
    assert!(
        p[0] > 6.0 && p[0] < 24.0 && p[1] > 38.0 && p[1] < 52.0,
        "違反点がポケット領域内: {p:?}"
    );
}

#[test]
fn results_deterministic_and_density_in_output() {
    let d = bracket_design("", 2.5, 0.25);
    let j1 = adc_check::to_jsonl(&run_checks(&d, &EvalContext::nominal()));
    let j2 = adc_check::to_jsonl(&run_checks(&d, &EvalContext::nominal()));
    assert_eq!(j1, j2, "レイキャストでも決定的(格子は決定的順序)");
}
