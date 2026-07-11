//! M2-5 受入テスト: BoundingBox正式化(gap除去前提)/ DatumValidity。
//! 幾何公差の実測検証はスコープ外(§7)— データム参照の存在+平面性+直交性のみ。

use adc_check::{run_checks, CheckStatus, Value};
use adc_schema::{validate_design, EvalContext};

fn design(anchors: &str, assertions: &str) -> adc_schema::Design {
    let src = format!(
        r#"Design(
    schema_version: "0.1",
    intent: "m2-5 fixture",
    params: [],
    materials: [Material(id: "a5052", density_g_cm3: 2.68, name: "A5052")],
    parts: [
        Part(id: "p1", material: "a5052", process: Machining,
            features: [
                Block(id: "base", x: 30.0, y: 20.0, z: 10.0),
                Hole(id: "bore", kind: Simple, d: 8.0, depth: Through,
                     at: on(feature("base").face("top"), center())),
            ],
            anchors: [{anchors}]),
    ],
    assertions: [{assertions}],
    rationales: [Rationale(id: "r0", author: Human("t"), basis: Assumption, note: "", timestamp: "2026-07-12T00:00:00Z")],
)"#
    );
    validate_design(&src).unwrap_or_else(|e| panic!("検証: {e:#?}"))
}

const DV: &str = r#"Assertion(id: "a_datum", check: DatumValidity(part: "p1"), rationale: "r0")"#;

#[test]
fn bounding_box_exact_after_gap_removal() {
    // Bnd_Boxのgap(1e-7)除去(gotcha)を前提に、量子化後の実測が正確な寸法になる
    let d = design(
        "",
        r#"Assertion(id: "a_bbox", check: BoundingBox(part: "p1", max: (30.0, 20.0, 10.0)), rationale: "r0")"#,
    );
    let rs = run_checks(&d, &EvalContext::nominal());
    let r = &rs[0];
    assert!(matches!(r.status, CheckStatus::Pass), "ちょうど上限 → Pass: {:?}", r.status);
    assert!(
        matches!(r.measured, Value::Triple(s) if s == [30.0, 20.0, 10.0]),
        "gap除去+量子化で正確な寸法: {:?}",
        r.measured
    );
    assert_eq!(r.margin, 0.0, "ちょうど → margin 0");
}

#[test]
fn datum_validity_pass_with_orthogonal_planes() {
    let anchors = r#"
        Anchor(id: "datum_a", kind: Datum('A'), binding: feature("base").face("bottom")),
        Anchor(id: "datum_b", kind: Datum('B'), binding: feature("base").face("-x")),
        Anchor(id: "datum_c", kind: Datum('C'), binding: feature("base").face("-y")),
    "#;
    let d = design(anchors, DV);
    let rs = run_checks(&d, &EvalContext::nominal());
    let r = &rs[0];
    assert!(matches!(r.status, CheckStatus::Pass), "{:?}", r.status);
    assert!((r.margin - 1.0).abs() < 1e-9, "完全直交 → margin=1: {}", r.margin);
}

#[test]
fn datum_validity_fails_on_parallel_datums() {
    // AとBが平行(bottomとtop) → 直交性違反
    let anchors = r#"
        Anchor(id: "datum_a", kind: Datum('A'), binding: feature("base").face("bottom")),
        Anchor(id: "datum_b", kind: Datum('B'), binding: feature("base").face("top")),
    "#;
    let d = design(anchors, DV);
    let rs = run_checks(&d, &EvalContext::nominal());
    let r = &rs[0];
    assert!(matches!(r.status, CheckStatus::Fail), "{:?}", r.status);
    let ev = &r.evidence[0];
    assert_eq!(ev.anchors, vec!["datum_a".to_string(), "datum_b".to_string()]);
    assert!(ev.note.contains("直交していません"), "{}", ev.note);
}

#[test]
fn datum_validity_fails_on_non_planar_datum() {
    // 円筒面(ボア壁)をデータムに束縛 → 平面性違反
    let anchors = r#"
        Anchor(id: "datum_a", kind: Datum('A'), binding: feature("bore").face("wall")),
    "#;
    let d = design(anchors, DV);
    let rs = run_checks(&d, &EvalContext::nominal());
    let r = &rs[0];
    assert!(matches!(r.status, CheckStatus::Fail), "{:?}", r.status);
    assert!(r.evidence[0].note.contains("平面ではありません"), "{}", r.evidence[0].note);
}

#[test]
fn datum_validity_without_datums_is_inconclusive() {
    let d = design("", DV);
    let rs = run_checks(&d, &EvalContext::nominal());
    match &rs[0].status {
        CheckStatus::Inconclusive { reason } => {
            assert!(reason.contains("Datumアンカーがありません"), "{reason}")
        }
        other => panic!("Inconclusiveのはず: {other:?}"),
    }
}
