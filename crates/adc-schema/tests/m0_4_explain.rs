//! M0-4 受入テスト: explain (US-03, US-04)。
//!
//! - 定義本体+rationale連鎖+逆参照(参照元一覧)をJSONで返す
//! - 種別横断検索。複数ヒット(Part内スコープの同名を含む)は候補一覧
//! - 受入(ユーザー指定): §9サンプルで explain wall_t が
//!   「参照元: a_wall(assertion), base.z(feature式)」を返すこと
//! - 出力スキーマは docs/explain-schema.md で確定(以後後方互換)

use adc_schema::*;

fn sample_design() -> Design {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/motor_bracket/design.ron"
    );
    let src = std::fs::read_to_string(path).expect("サンプル読み込み");
    validate_design(&src).expect("サンプルは検証を通ること")
}

#[test]
fn explain_wall_t_returns_definition_rationale_and_referrers() {
    let d = sample_design();
    let out = explain(&d, "wall_t");
    assert_eq!(out.status, ExplainStatus::Found);
    assert_eq!(out.matches.len(), 1);
    let m = &out.matches[0];
    assert_eq!(m.kind, "param");
    assert_eq!(m.id, "wall_t");

    // 定義本体: Open(range: (3,6), nominal: 4)
    let def = serde_json::to_string(&m.definition).unwrap();
    assert!(def.contains("Open"), "定義本体を含むこと: {def}");

    // rationale連鎖: r_wall (Assumption)
    assert_eq!(m.rationale_chain.len(), 1);
    let r = serde_json::to_string(&m.rationale_chain[0]).unwrap();
    assert!(r.contains("r_wall") && r.contains("Assumption"), "{r}");

    // 参照元: base.z (feature式) と a_wall (assertion、rationale共有)
    let has_feature_ref = m
        .referenced_by
        .iter()
        .any(|s| s.kind == "feature" && s.id == "base" && s.via == "z");
    assert!(
        has_feature_ref,
        "参照元に base.z (feature式) を含むこと: {:#?}",
        m.referenced_by
    );
    let has_assertion_ref = m
        .referenced_by
        .iter()
        .any(|s| s.kind == "assertion" && s.id == "a_wall" && s.via.contains("rationale"));
    assert!(
        has_assertion_ref,
        "参照元に a_wall (assertion、rationale共有) を含むこと: {:#?}",
        m.referenced_by
    );
}

#[test]
fn explain_rationale_lists_all_holders() {
    // r_wall を持つのは wall_t(param) と a_wall(assertion)
    let d = sample_design();
    let out = explain(&d, "r_wall");
    assert_eq!(out.status, ExplainStatus::Found);
    let m = &out.matches[0];
    assert_eq!(m.kind, "rationale");
    let kinds: Vec<(&str, &str)> = m
        .referenced_by
        .iter()
        .map(|s| (s.kind.as_str(), s.id.as_str()))
        .collect();
    assert!(kinds.contains(&("param", "wall_t")), "{kinds:?}");
    assert!(kinds.contains(&("assertion", "a_wall")), "{kinds:?}");
}

#[test]
fn explain_anchor_and_feature_are_part_scoped() {
    let d = sample_design();
    let out = explain(&d, "bearing_bore");
    assert_eq!(out.status, ExplainStatus::Found);
    let m = &out.matches[0];
    assert_eq!(m.kind, "anchor");
    assert_eq!(m.part.as_deref(), Some("bracket"));

    let out = explain(&d, "base");
    assert_eq!(out.status, ExplainStatus::Found);
    let m = &out.matches[0];
    assert_eq!(m.kind, "feature");
    assert_eq!(m.part.as_deref(), Some("bracket"));
    // base は bore/bolts/f1 の binding・placement から参照される
    assert!(
        m.referenced_by
            .iter()
            .any(|s| s.kind == "feature" && s.id == "bore"),
        "{:#?}",
        m.referenced_by
    );
    assert!(
        m.referenced_by
            .iter()
            .any(|s| s.kind == "anchor" && s.id == "mount_face" && s.via == "binding"),
        "{:#?}",
        m.referenced_by
    );
}

#[test]
fn explain_ambiguous_returns_candidates() {
    // 2部品が同名アンカー mount_face を持つ → 候補一覧
    let src = r#"Design(
    schema_version: "0.1",
    intent: "ambiguous fixture",
    params: [],
    materials: [Material(id: "a5052", density_g_cm3: 2.68, name: "A5052")],
    parts: [
        Part(id: "p1", material: "a5052", process: Machining,
            features: [Block(id: "base", x: 10.0, y: 10.0, z: 4.0)],
            anchors: [Anchor(id: "mount_face", kind: Face, binding: feature("base").face("bottom"))]),
        Part(id: "p2", material: "a5052", process: Machining,
            features: [Block(id: "base", x: 10.0, y: 10.0, z: 4.0)],
            anchors: [Anchor(id: "mount_face", kind: Face, binding: feature("base").face("bottom"))]),
    ],
    assertions: [],
    rationales: [],
)"#;
    let d = validate_design(src).expect("検証を通ること");
    let out = explain(&d, "mount_face");
    assert_eq!(out.status, ExplainStatus::Ambiguous);
    assert_eq!(out.matches.len(), 2);
    let parts: Vec<&str> = out
        .matches
        .iter()
        .map(|m| m.part.as_deref().unwrap())
        .collect();
    assert!(parts.contains(&"p1") && parts.contains(&"p2"), "{parts:?}");
}

#[test]
fn explain_not_found() {
    let d = sample_design();
    let out = explain(&d, "ghost");
    assert_eq!(out.status, ExplainStatus::NotFound);
    assert!(out.matches.is_empty());
}

#[test]
fn explain_output_is_json_with_stable_shape() {
    // docs/explain-schema.md のトップレベル形状(後方互換の対象)
    let d = sample_design();
    let json: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&explain(&d, "wall_t")).unwrap()).unwrap();
    for key in ["schema_version", "query", "status", "matches"] {
        assert!(json.get(key).is_some(), "トップレベルに {key} があること");
    }
    let m = &json["matches"][0];
    for key in ["kind", "id", "definition", "rationale_chain", "referenced_by"] {
        assert!(m.get(key).is_some(), "matchに {key} があること");
    }
}
