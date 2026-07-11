//! M1-4 受入テスト(スキーマ側): Pattern添字ID (§4.1) の静的検証とexplain。

use adc_schema::{explain, validate_design, ErrorCode, ExplainStatus};

fn src(anchor_binding: &str) -> String {
    format!(
        r#"Design(
    schema_version: "0.1",
    intent: "m1-4 schema fixture",
    params: [],
    materials: [Material(id: "a5052", density_g_cm3: 2.68, name: "A5052")],
    parts: [
        Part(id: "p1", material: "a5052", process: Machining,
            features: [
                Block(id: "base", x: 80.0, y: 60.0, z: 4.0),
                Pattern(id: "bolts", of: Hole(kind: Simple, d: 6.6, depth: Through),
                        kind: Linear2D, count: (2, 2), pitch: (64.0, 44.0),
                        at: on(feature("base").face("top"), center())),
            ],
            anchors: [
                Anchor(id: "b", kind: Face, binding: feature("{anchor_binding}").face("wall")),
            ]),
    ],
    assertions: [],
    rationales: [Rationale(id: "r0", author: Human("t"), basis: Assumption, note: "", timestamp: "2026-07-12T00:00:00Z")],
)"#
    )
}

#[test]
fn indexed_pattern_reference_validates_in_bounds() {
    validate_design(&src("bolts[1][0]")).expect("範囲内の添字参照は検証を通ること");
}

#[test]
fn out_of_bounds_indexed_reference_is_ref_error() {
    let errs = validate_design(&src("bolts[5][0]")).unwrap_err();
    assert!(
        errs.iter().any(|e| e.code == ErrorCode::SchemaRef),
        "{errs:#?}"
    );
}

#[test]
fn explain_resolves_indexed_pattern_instance() {
    // 受入: [i][j] がexplainで引ける(定義+参照元)
    let design = validate_design(&src("bolts[1][0]")).unwrap();
    let out = explain(&design, "bolts[1][0]");
    assert_eq!(out.status, ExplainStatus::Found);
    let m = &out.matches[0];
    assert_eq!(m.kind, "feature");
    assert_eq!(m.id, "bolts[1][0]");
    assert_eq!(m.part.as_deref(), Some("p1"));
    // definitionにパターン定義とインスタンス添字
    let def = serde_json::to_string(&m.definition).unwrap();
    assert!(def.contains("\"instance\":[1,0]"), "{def}");
    // 参照元: アンカーbのbinding
    assert!(
        m.referenced_by
            .iter()
            .any(|s| s.kind == "anchor" && s.id == "b" && s.via == "binding"),
        "{:#?}",
        m.referenced_by
    );
}

#[test]
fn explain_out_of_bounds_instance_is_not_found() {
    let design = validate_design(&src("bolts[1][0]")).unwrap();
    assert_eq!(explain(&design, "bolts[9][9]").status, ExplainStatus::NotFound);
}
