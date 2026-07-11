//! M2-2 受入テスト (US-12, US-16): Clearance / NoInterference。
//!
//! §9ブラケット相当+シャフトの2体。インスタンスはM3まで恒等配置のため、
//! シャフトはルートのグローバル配置(Offset)でボア同軸に置く。
//! - 非干渉配置(φ50 in φ55)で Pass{margin}
//! - 干渉配置(φ56)で Fail{最近接点座標+両アンカー帰属} / NoInterference Fail{交差体積}

use adc_check::{run_checks, CheckStatus, Value};
use adc_schema::{validate_design, EvalContext};

const PI: f64 = std::f64::consts::PI;

fn design(shaft_d: f64, assertions: &str) -> adc_schema::Design {
    let src = format!(
        r#"Design(
    schema_version: "0.1",
    intent: "m2-2 fixture: ブラケット+シャフト",
    params: [],
    materials: [Material(id: "a5052", density_g_cm3: 2.68, name: "A5052")],
    parts: [
        Part(id: "bracket", material: "a5052", process: Machining,
            features: [
                Block(id: "base", x: 80.0, y: 60.0, z: 4.0),
                Hole(id: "bore", kind: Simple, d: 55.0, depth: Through,
                     at: on(feature("base").face("top"), center())),
            ],
            anchors: [
                Anchor(id: "bearing_bore", kind: Face, binding: feature("bore").face("wall")),
            ]),
        Part(id: "shaft", material: "a5052", process: Machining,
            features: [
                Cylinder(id: "body", d: {shaft_d}, h: 20.0,
                         at: Offset(from: Origin, d: (40.0, 30.0, -8.0))),
            ],
            anchors: [
                Anchor(id: "od", kind: Face, binding: feature("body").face("side")),
            ]),
    ],
    assembly: Assembly(id: "assy",
        instances: [Instance(id: "bracket_i", part: "bracket"), Instance(id: "shaft_i", part: "shaft")],
        mates: [], ground: "bracket_i"),
    assertions: [{assertions}],
    rationales: [Rationale(id: "r0", author: Human("t"), basis: Assumption, note: "", timestamp: "2026-07-12T00:00:00Z")],
)"#
    );
    validate_design(&src).unwrap_or_else(|e| panic!("検証: {e:#?}"))
}

const CLEAR_A: &str = r#"Assertion(id: "a_clear",
    check: Clearance(a: "bracket_i.bearing_bore", b: "shaft_i.od", min: 1.0), rationale: "r0")"#;
const NOINTF_A: &str =
    r#"Assertion(id: "b_nointf", check: NoInterference(scope: All), rationale: "r0")"#;

#[test]
fn clearance_pass_with_margin() {
    // φ50 in φ55 → 半径ギャップ 2.5、margin = (2.5-1)/1 = 1.5
    let d = design(50.0, CLEAR_A);
    let rs = run_checks(&d, &EvalContext::nominal());
    let r = &rs[0];
    assert!(matches!(r.status, CheckStatus::Pass), "{:?}", r.status);
    assert!(matches!(r.measured, Value::Scalar(v) if (v - 2.5).abs() < 1e-6), "{:?}", r.measured);
    assert!((r.margin - 1.5).abs() < 1e-6, "{}", r.margin);
    // Passでも最近接点Evidenceを含む
    assert_eq!(r.evidence.len(), 1);
    assert_eq!(
        r.evidence[0].anchors,
        vec!["bracket_i.bearing_bore".to_string(), "shaft_i.od".to_string()]
    );
}

#[test]
fn clearance_fail_with_closest_points_and_anchors() {
    // φ56: ボア壁(r27.5)とシャフト側面(r28)は同軸入れ子 → 半径ギャップ0.5 < 要求1.0
    let d = design(56.0, CLEAR_A);
    let rs = run_checks(&d, &EvalContext::nominal());
    let r = &rs[0];
    assert!(matches!(r.status, CheckStatus::Fail), "{:?}", r.status);
    assert!(matches!(r.measured, Value::Scalar(v) if (v - 0.5).abs() < 1e-6), "{:?}", r.measured);
    assert!((r.margin - (-0.5)).abs() < 1e-6, "margin=(0.5-1)/1: {}", r.margin);
    let ev = &r.evidence[0];
    assert_eq!(ev.anchors.len(), 2, "両アンカー帰属");
    assert_eq!(ev.points.len(), 2, "最近接点座標");
    // 最近接点はボア壁近傍(半径27.5±εの円筒上、z∈[0,4])
    for p in &ev.points {
        let rr = ((p[0] - 40.0).powi(2) + (p[1] - 30.0).powi(2)).sqrt();
        assert!(rr > 26.0 && rr < 30.0, "最近接点の位置: {p:?}");
    }
}

#[test]
fn no_interference_pass_and_fail_with_overlap_volume() {
    // 非干渉: φ50シャフトはボア内 → Pass
    let d = design(50.0, NOINTF_A);
    let rs = run_checks(&d, &EvalContext::nominal());
    assert!(matches!(rs[0].status, CheckStatus::Pass), "{:?}", rs[0].status);
    assert!(rs[0].margin > 0.0);

    // 干渉: φ56 → 交差リング体積 π(28²-27.5²)·4
    let d = design(56.0, NOINTF_A);
    let rs = run_checks(&d, &EvalContext::nominal());
    let r = &rs[0];
    assert!(matches!(r.status, CheckStatus::Fail), "{:?}", r.status);
    let expected = PI * (28.0f64.powi(2) - 27.5f64.powi(2)) * 4.0;
    assert!(
        matches!(r.measured, Value::Scalar(v) if (v - expected).abs() / expected < 1e-6),
        "交差体積: {:?} (expected {expected})",
        r.measured
    );
    assert!(r.margin < 0.0);
    let ev = &r.evidence[0];
    assert_eq!(ev.anchors, vec!["bracket".to_string(), "shaft".to_string()]);
    assert!(ev.note.contains("交差体積"), "{}", ev.note);
}

#[test]
fn no_interference_without_assembly_is_inconclusive() {
    // 単部品内は対象外(仕様どおりAssy/ペア対象)
    let src = r#"Design(
    schema_version: "0.1",
    intent: "single part",
    params: [],
    materials: [Material(id: "a5052", density_g_cm3: 2.68, name: "A5052")],
    parts: [
        Part(id: "p1", material: "a5052", process: Machining,
            features: [Block(id: "base", x: 10.0, y: 10.0, z: 4.0)], anchors: []),
    ],
    assertions: [Assertion(id: "a", check: NoInterference(scope: All), rationale: "r0")],
    rationales: [Rationale(id: "r0", author: Human("t"), basis: Assumption, note: "", timestamp: "2026-07-12T00:00:00Z")],
)"#;
    let d = validate_design(src).unwrap();
    let rs = run_checks(&d, &EvalContext::nominal());
    match &rs[0].status {
        CheckStatus::Inconclusive { reason } => {
            assert!(reason.contains("対象ペアなし"), "{reason}")
        }
        other => panic!("Inconclusiveのはず: {other:?}"),
    }
}

#[test]
fn clearance_results_are_deterministic_bytes() {
    let d = design(50.0, &format!("{CLEAR_A}, {NOINTF_A}"));
    let j1 = adc_check::to_jsonl(&run_checks(&d, &EvalContext::nominal()));
    let j2 = adc_check::to_jsonl(&run_checks(&d, &EvalContext::nominal()));
    assert_eq!(j1, j2);
}
