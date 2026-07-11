//! M3-1 静的規則: 非ground部品のグローバル配置とmate位置決めの併用禁止、
//! groundを被拘束側(b)にできないこと。

use adc_schema::{validate_design, ErrorCode};

fn src(shaft_at: &str, mate_b: &str) -> String {
    format!(
        r#"Design(
    schema_version: "0.1",
    intent: "m3 static fixture",
    params: [],
    materials: [Material(id: "a5052", density_g_cm3: 2.68, name: "A5052")],
    parts: [
        Part(id: "bracket", material: "a5052", process: Machining,
            features: [Block(id: "base", x: 80.0, y: 64.0, z: 4.0)],
            anchors: [Anchor(id: "top", kind: Face, binding: feature("base").face("top"))]),
        Part(id: "shaft", material: "a5052", process: Machining,
            features: [Cylinder(id: "body", d: 20.0, h: 30.0{shaft_at})],
            anchors: [Anchor(id: "base_face", kind: Face, binding: feature("body").face("bottom"))]),
    ],
    assembly: Assembly(id: "assy",
        instances: [Instance(id: "bracket_i", part: "bracket"), Instance(id: "shaft_i", part: "shaft")],
        mates: [Mate(id: "m1", kind: Coincident, a: "bracket_i.top", b: "{mate_b}", rationale: "r0")],
        ground: "bracket_i"),
    assertions: [],
    rationales: [Rationale(id: "r0", author: Human("t"), basis: Assumption, note: "", timestamp: "2026-07-12T00:00:00Z")],
)"#
    )
}

#[test]
fn non_ground_global_placement_with_mates_is_static_error() {
    // shaftのルートがOffsetグローバル配置+mateで被拘束 → 併用禁止
    let errs = validate_design(&src(
        ", at: Offset(from: Origin, d: (40.0, 30.0, -8.0))",
        "shaft_i.base_face",
    ))
    .unwrap_err();
    assert!(
        errs.iter().any(|e| e.code == ErrorCode::SchemaRef
            && e.message.contains("併用できません")),
        "{errs:#?}"
    );
}

#[test]
fn unmated_global_placement_is_allowed() {
    // mateを持たないインスタンスのグローバル配置は許容(事実上の固定配置。
    // エラーになるのは「mate位置決めとの併用」のみ — 2026-07-12指示)
    let src_no_mate = src(", at: Offset(from: Origin, d: (40.0, 30.0, -8.0))", "shaft_i.base_face")
        .replace(
            r#"mates: [Mate(id: "m1", kind: Coincident, a: "bracket_i.top", b: "shaft_i.base_face", rationale: "r0")],"#,
            "mates: [],",
        );
    validate_design(&src_no_mate).expect("mate無しのOffset配置は許容されること");
}

#[test]
fn ground_as_constrained_side_is_static_error() {
    let errs = validate_design(&src("", "bracket_i.top")).unwrap_err();
    assert!(
        errs.iter().any(|e| e.code == ErrorCode::SchemaRef
            && e.message.contains("被拘束側")),
        "{errs:#?}"
    );
}
