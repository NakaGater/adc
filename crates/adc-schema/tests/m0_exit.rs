//! M0 Exit条件 (04-units-of-work.md):
//! 「05-schema.md §9のサンプルがparse→検証→explainまで通る」

use adc_schema::*;

#[test]
fn m0_exit_sample_parses_validates_and_explains() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/motor_bracket/design.ron"
    );
    let src = std::fs::read_to_string(path).expect("§9サンプル読み込み");

    // parse
    let parsed = parse_design(&src).expect("parseが通ること");
    assert_eq!(parsed.schema_version, "0.1");

    // 検証
    let design = validate_design(&src).expect("静的検証が通ること");

    // explain (JSON出力可能であること)
    for id in ["wall_t", "bore_d", "bearing_bore", "a_mass", "r_wall", "bracket"] {
        let out = explain(&design, id);
        assert_eq!(
            out.status,
            ExplainStatus::Found,
            "explain {id} が一意に解決すること"
        );
        serde_json::to_string(&out).expect("explain出力はJSONシリアライズ可能なこと");
    }

    // 受入(修正版): explain wall_t →
    //   referenced_by: [base.z(feature式)] / related: [a_wall (via rationale:r_wall)]
    let out = explain(&design, "wall_t");
    let m = &out.matches[0];
    assert!(m
        .referenced_by
        .iter()
        .any(|s| s.kind == "feature" && s.id == "base" && s.via == "z"));
    assert!(m
        .related
        .iter()
        .any(|s| s.kind == "assertion" && s.id == "a_wall" && s.via == "rationale:r_wall"));
    assert!(!m.referenced_by.iter().any(|s| s.id == "a_wall"));
}
