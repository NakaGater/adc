//! M0-2 受入テスト (US-02, US-04): 静的検証。
//!
//! テーブル駆動: 不正RON → 期待エラーコード。検証項目:
//! 未定義参照(param/anchor/feature/rationale/material/part/instance/dim)、
//! 種別内重複ID(§1.1)、Expr循環(E-SCHEMA-CYCLE)、rationale欠落、
//! Open範囲の妥当性(E-SCHEMA-RANGE)、mate DAG(E-MATE-CYCLE)。
//! エラーは構造化形式 {code, message, span, related} でJSONシリアライズ可能。

use adc_schema::{validate_design, ErrorCode, ValidationError};

const R0: &str = r#"Rationale(id: "r0", author: Human("t"), basis: Assumption, note: "", timestamp: "2026-07-11T00:00:00Z")"#;
const P_WALL: &str = r#"Param(id: "wall_t", value: Determined(4.0), unit: Mm, rationale: "r0")"#;
const PART_P1: &str = r#"Part(id: "p1", material: "a5052", process: Machining,
            features: [Block(id: "base", x: 10.0, y: 10.0, z: 4.0)],
            anchors: [Anchor(id: "top", kind: Face, binding: feature("base").face("top"))])"#;

/// 各セクションを差し替え可能な最小designテキスト
fn fixture(params: &str, parts: &str, tail: &str) -> String {
    format!(
        r#"Design(
    schema_version: "0.1",
    intent: "m0-2 fixture",
    params: [{params}],
    materials: [Material(id: "a5052", density_g_cm3: 2.68, name: "A5052")],
    parts: [{parts}],
    {tail}
    rationales: [{R0}],
)"#
    )
}

fn plain(params: &str, parts: &str) -> String {
    fixture(params, parts, "assertions: [],")
}

/// 2部品+assembly(mates差し替え)のフィクスチャ
fn assy(mates: &str) -> String {
    let parts = r#"
        Part(id: "p1", material: "a5052", process: Machining,
            features: [Block(id: "base", x: 10.0, y: 10.0, z: 4.0)],
            anchors: [Anchor(id: "a", kind: Face, binding: feature("base").face("top"))]),
        Part(id: "p2", material: "a5052", process: Machining,
            features: [Block(id: "base", x: 10.0, y: 10.0, z: 4.0)],
            anchors: [Anchor(id: "a", kind: Face, binding: feature("base").face("top"))]),
    "#;
    let tail = format!(
        r#"assembly: Assembly(id: "assy", instances: [
        Instance(id: "i1", part: "p1"),
        Instance(id: "i2", part: "p2"),
    ], mates: [{mates}], ground: "i1"),
    assertions: [],"#
    );
    fixture(P_WALL, parts, &tail)
}

fn expect_codes(name: &str, src: &str, expected: &[ErrorCode]) {
    match validate_design(src) {
        Ok(_) => assert!(
            expected.is_empty(),
            "{name}: エラー {expected:?} を期待したが検証成功\n--- 入力 ---\n{src}"
        ),
        Err(errs) => {
            let mut got: Vec<&str> = errs.iter().map(|e| e.code.as_str()).collect();
            got.sort_unstable();
            let mut want: Vec<&str> = expected.iter().map(|c| c.as_str()).collect();
            want.sort_unstable();
            assert_eq!(
                got, want,
                "{name}: エラーコード不一致\n詳細: {errs:#?}\n--- 入力 ---\n{src}"
            );
        }
    }
}

// ================================================================ テーブル駆動

#[test]
fn undefined_references() {
    use ErrorCode::*;
    let cases: Vec<(&str, String, Vec<ErrorCode>)> = vec![
        (
            "undefined_param_ref",
            plain(
                P_WALL,
                r#"Part(id: "p1", material: "a5052", process: Machining,
                    features: [Block(id: "base", x: 10.0, y: 10.0, z: param("nope"))],
                    anchors: [])"#,
            ),
            vec![SchemaRef],
        ),
        (
            "undefined_param_in_determined",
            plain(
                r#"Param(id: "wall_t", value: Determined(param("ghost") * 2.0), unit: Mm, rationale: "r0")"#,
                PART_P1,
            ),
            vec![SchemaRef],
        ),
        (
            "undefined_feature_in_binding",
            plain(
                P_WALL,
                r#"Part(id: "p1", material: "a5052", process: Machining,
                    features: [Block(id: "base", x: 10.0, y: 10.0, z: 4.0)],
                    anchors: [Anchor(id: "top", kind: Face, binding: feature("ghost").face("top"))])"#,
            ),
            vec![SchemaRef],
        ),
        (
            "undefined_material",
            plain(
                P_WALL,
                r#"Part(id: "p1", material: "ghost", process: Machining, features: [], anchors: [])"#,
            ),
            vec![SchemaRef],
        ),
        (
            "undefined_rationale",
            plain(
                r#"Param(id: "wall_t", value: Determined(4.0), unit: Mm, rationale: "r_missing")"#,
                PART_P1,
            ),
            vec![SchemaRationale],
        ),
        (
            "undefined_part_in_check",
            fixture(
                P_WALL,
                PART_P1,
                r#"assertions: [Assertion(id: "a1", check: Mass(part: "ghost", max: 100.0), rationale: "r0")],"#,
            ),
            vec![SchemaRef],
        ),
        (
            "undefined_dim_in_stack_path",
            fixture(
                P_WALL,
                PART_P1,
                r#"assertions: [Assertion(id: "a1", check: ToleranceStack1D(path: ["nope"], target: (0.0, 1.0), method: Both), rationale: "r0")],"#,
            ),
            vec![SchemaRef],
        ),
    ];
    for (name, src, expected) in &cases {
        expect_codes(name, src, expected);
    }
}

#[test]
fn undefined_references_in_assembly() {
    use ErrorCode::*;
    let cases: Vec<(&str, String, Vec<ErrorCode>)> = vec![
        (
            "undefined_instance_in_mate",
            assy(r#"Mate(id: "m1", kind: Coaxial, a: "ghost.a", b: "i2.a", rationale: "r0")"#),
            vec![SchemaRef],
        ),
        (
            "undefined_anchor_in_mate",
            assy(r#"Mate(id: "m1", kind: Coaxial, a: "i1.nope", b: "i2.a", rationale: "r0")"#),
            vec![SchemaRef],
        ),
        ("valid_assembly_passes", assy(r#"Mate(id: "m1", kind: Coincident, a: "i1.a", b: "i2.a", rationale: "r0")"#), vec![]),
    ];
    for (name, src, expected) in &cases {
        expect_codes(name, src, expected);
    }

    // instance が未定義 part を指す
    let src = fixture(
        P_WALL,
        PART_P1,
        r#"assembly: Assembly(id: "assy", instances: [Instance(id: "i1", part: "ghost")], mates: [], ground: "i1"),
    assertions: [],"#,
    );
    expect_codes("undefined_part_in_instance", &src, &[ErrorCode::SchemaRef]);
}

#[test]
fn duplicate_ids_within_kind() {
    use ErrorCode::*;
    let cases: Vec<(&str, String, Vec<ErrorCode>)> = vec![
        (
            "dup_param_id",
            plain(
                r#"Param(id: "wall_t", value: Determined(4.0), unit: Mm, rationale: "r0"),
                   Param(id: "wall_t", value: Determined(5.0), unit: Mm, rationale: "r0")"#,
                PART_P1,
            ),
            vec![SchemaDup],
        ),
        (
            "dup_feature_id_within_part",
            plain(
                P_WALL,
                r#"Part(id: "p1", material: "a5052", process: Machining,
                    features: [Block(id: "base", x: 10.0, y: 10.0, z: 4.0),
                               Block(id: "base", x: 5.0, y: 5.0, z: 2.0)],
                    anchors: [])"#,
            ),
            vec![SchemaDup],
        ),
        (
            // §1.1: feature/anchorのスコープは所属Part内 — 部品間の同名anchorは正当
            "same_anchor_id_across_parts_is_ok",
            plain(
                P_WALL,
                r#"Part(id: "p1", material: "a5052", process: Machining,
                    features: [Block(id: "base", x: 10.0, y: 10.0, z: 4.0)],
                    anchors: [Anchor(id: "mount_face", kind: Face, binding: feature("base").face("bottom"))]),
                   Part(id: "p2", material: "a5052", process: Machining,
                    features: [Block(id: "base", x: 10.0, y: 10.0, z: 4.0)],
                    anchors: [Anchor(id: "mount_face", kind: Face, binding: feature("base").face("bottom"))])"#,
            ),
            vec![],
        ),
        (
            "dup_part_id",
            plain(
                P_WALL,
                r#"Part(id: "p1", material: "a5052", process: Machining, features: [], anchors: []),
                   Part(id: "p1", material: "a5052", process: Machining, features: [], anchors: [])"#,
            ),
            vec![SchemaDup],
        ),
    ];
    for (name, src, expected) in &cases {
        expect_codes(name, src, expected);
    }
}

#[test]
fn param_cycles() {
    use ErrorCode::*;
    let cases: Vec<(&str, String, Vec<ErrorCode>)> = vec![
        (
            "param_cycle_pair",
            plain(
                r#"Param(id: "a", value: Determined(param("b")), unit: Mm, rationale: "r0"),
                   Param(id: "b", value: Determined(param("a") + 1.0), unit: Mm, rationale: "r0")"#,
                PART_P1,
            ),
            vec![SchemaCycle],
        ),
        (
            "param_self_cycle",
            plain(
                r#"Param(id: "a", value: Determined(param("a") * 2.0), unit: Mm, rationale: "r0")"#,
                PART_P1,
            ),
            vec![SchemaCycle],
        ),
        (
            "derived_param_chain_is_ok",
            plain(
                r#"Param(id: "a", value: Determined(2.0), unit: Mm, rationale: "r0"),
                   Param(id: "b", value: Determined(param("a") * 3.0), unit: Mm, rationale: "r0")"#,
                PART_P1,
            ),
            vec![],
        ),
    ];
    for (name, src, expected) in &cases {
        expect_codes(name, src, expected);
    }
}

#[test]
fn open_range_validity() {
    use ErrorCode::*;
    let cases: Vec<(&str, String, Vec<ErrorCode>)> = vec![
        (
            "nominal_outside_range",
            plain(
                r#"Param(id: "wall_t", value: Open(range: (3.0, 6.0), nominal: 8.0), unit: Mm, rationale: "r0")"#,
                PART_P1,
            ),
            vec![SchemaRange],
        ),
        (
            "inverted_range",
            plain(
                r#"Param(id: "wall_t", value: Open(range: (6.0, 3.0), nominal: 4.0), unit: Mm, rationale: "r0")"#,
                PART_P1,
            ),
            vec![SchemaRange],
        ),
        (
            "valid_open_param",
            plain(
                r#"Param(id: "wall_t", value: Open(range: (3.0, 6.0), nominal: 4.0), unit: Mm, rationale: "r0")"#,
                PART_P1,
            ),
            vec![],
        ),
    ];
    for (name, src, expected) in &cases {
        expect_codes(name, src, expected);
    }
}

#[test]
fn mate_dag() {
    use ErrorCode::*;
    let cases: Vec<(&str, String, Vec<ErrorCode>)> = vec![
        (
            "mate_cycle_pair",
            assy(
                r#"Mate(id: "m1", kind: Coaxial, a: "i1.a", b: "i2.a", rationale: "r0"),
                   Mate(id: "m2", kind: Coincident, a: "i2.a", b: "i1.a", rationale: "r0")"#,
            ),
            vec![MateCycle],
        ),
        (
            "mate_self_reference",
            assy(r#"Mate(id: "m1", kind: Coincident, a: "i1.a", b: "i1.a", rationale: "r0")"#),
            vec![MateCycle],
        ),
        (
            // 同一ペアへの複数mate(coaxial+coincident)は正当(拘束の組合せ)
            "multiple_mates_same_direction_ok",
            assy(
                r#"Mate(id: "m1", kind: Coaxial, a: "i1.a", b: "i2.a", rationale: "r0"),
                   Mate(id: "m2", kind: Coincident, a: "i1.a", b: "i2.a", rationale: "r0")"#,
            ),
            vec![],
        ),
    ];
    for (name, src, expected) in &cases {
        expect_codes(name, src, expected);
    }
}

#[test]
fn geom_tol_datum_reference() {
    // datums は kind: Datum のアンカーのみ許可 (05-schema.md §7)
    let parts = r#"Part(id: "p1", material: "a5052", process: Machining,
        features: [Block(id: "base", x: 10.0, y: 10.0, z: 4.0)],
        anchors: [
            Anchor(id: "top", kind: Face, binding: feature("base").face("top")),
            Anchor(id: "datum_a", kind: Datum('A'), binding: feature("base").face("bottom")),
        ])"#;
    let assy_tail = |datum: &str| {
        format!(
            r#"assembly: Assembly(id: "assy", instances: [Instance(id: "i1", part: "p1")], mates: [], ground: "i1"),
    geom_tols: [GeomTol(kind: Flatness, target: "i1.top", datums: ["i1.{datum}"], zone: 0.05, rationale: "r0")],
    assertions: [],"#
        )
    };
    expect_codes(
        "datum_ref_to_face_anchor_is_error",
        &fixture(P_WALL, parts, &assy_tail("top")),
        &[ErrorCode::SchemaRef],
    );
    expect_codes(
        "datum_ref_to_datum_anchor_is_ok",
        &fixture(P_WALL, parts, &assy_tail("datum_a")),
        &[],
    );
}

// ================================================================ 個別検証

#[test]
fn spec_sample_validates_clean() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/motor_bracket/design.ron"
    );
    let src = std::fs::read_to_string(path).expect("サンプル読み込み");
    match validate_design(&src) {
        Ok(_) => {}
        Err(errs) => panic!("§9サンプルは検証を通ること: {errs:#?}"),
    }
}

#[test]
fn parse_error_is_structured() {
    let errs = validate_design("Design(").unwrap_err();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, ErrorCode::SchemaParse);
    assert!(errs[0].span.is_some(), "parse errorはspan(行番号)を持つこと");
}

#[test]
fn errors_carry_span_and_related() {
    // 4行目の未定義rationale参照
    let src = r#"Design(
    schema_version: "0.1",
    intent: "span test",
    params: [Param(id: "wall_t", value: Determined(4.0), unit: Mm, rationale: "r_missing")],
    materials: [],
    parts: [],
    assertions: [],
    rationales: [],
)"#;
    let errs = validate_design(src).unwrap_err();
    let e = errs
        .iter()
        .find(|e| e.code == ErrorCode::SchemaRationale)
        .unwrap_or_else(|| panic!("E-SCHEMA-RATIONALEがあること: {errs:#?}"));
    assert!(
        e.related.iter().any(|r| r == "r_missing"),
        "relatedに参照IDを含むこと: {e:#?}"
    );
    let span = e.span.expect("spanがあること");
    assert_eq!(span.line, 4, "spanが参照箇所の行を指すこと: {e:#?}");
}

#[test]
fn errors_are_json_serializable() {
    let src = plain(
        r#"Param(id: "wall_t", value: Determined(4.0), unit: Mm, rationale: "r_missing")"#,
        PART_P1,
    );
    let errs: Vec<ValidationError> = validate_design(&src).unwrap_err();
    let json = serde_json::to_string(&errs).expect("JSONシリアライズ可能なこと");
    assert!(json.contains("\"E-SCHEMA-RATIONALE\""), "{json}");
    assert!(json.contains("\"related\""), "{json}");
}

#[test]
fn multiple_errors_are_all_reported() {
    // 1回の検証で全エラーを収集する(最初のエラーで停止しない)
    let src = plain(
        r#"Param(id: "a", value: Determined(param("a")), unit: Mm, rationale: "r_missing")"#,
        r#"Part(id: "p1", material: "ghost", process: Machining, features: [], anchors: [])"#,
    );
    let errs = validate_design(&src).unwrap_err();
    let codes: Vec<ErrorCode> = errs.iter().map(|e| e.code).collect();
    assert!(codes.contains(&ErrorCode::SchemaCycle), "{errs:#?}");
    assert!(codes.contains(&ErrorCode::SchemaRationale), "{errs:#?}");
    assert!(codes.contains(&ErrorCode::SchemaRef), "{errs:#?}");
}
