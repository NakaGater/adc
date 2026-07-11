//! M1-5 受入テスト (US-06 / Intent成功基準1): アンカー再束縛の総合テスト。
//!
//! §9サンプル(フルセット: Block+Hole+Pattern(Counterbore)+Fillet)を基点に、
//! 定義変更→再buildのシナリオをテーブル駆動で固定する。
//! M1 Exit条件「wall_t変更→再buildで全アンカーが再束縛される」の実体。
//!
//! 底面の穴はφ6.6×4(Simpleばか穴 — 2026-07-12サンプル修正)+φ55。

use adc_compile::{compile_part, BoundAnchorRef, CompileError, CompiledPart};
use adc_schema::{validate_design, AnchorBindCause, ErrorCode, EvalContext};

const EPS: f64 = 1e-6;
const PI: f64 = std::f64::consts::PI;
const R_BORE: f64 = 27.5;
const R_BOLT: f64 = 3.3;

const BORE_LINES: &str = r#"        Hole(id: "bore", kind: Simple, d: param("bore_d"), depth: Through,
             at: on(feature("base").face("top"), center())),
"#;
const FILLET_LINE: &str =
    "        Fillet(id: \"f1\", edges: edges_of(feature(\"base\").face(\"top\")), r: 2.0),\n";

fn sample_src() -> String {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/motor_bracket/design.ron"
    );
    std::fs::read_to_string(path).expect("サンプル読み込み")
}

/// features末尾(anchors直前)にフィーチャーを追加
fn with_feature(src: &str, feature: &str) -> String {
    let marker = "      ],\n      anchors:";
    assert!(src.contains(marker), "サンプル構造が想定と異なる");
    src.replacen(marker, &format!("        {feature},\n      ],\n      anchors:"), 1)
}

fn compile_with(src: &str, ctx: &EvalContext) -> Result<CompiledPart, CompileError> {
    let design = validate_design(src).unwrap_or_else(|e| panic!("静的検証: {e:#?}"));
    compile_part(&design, "bracket", ctx)
}

fn anchor_area(cp: &CompiledPart, id: &str) -> f64 {
    match cp.anchor(id) {
        Some(BoundAnchorRef::Face(f)) => f.area(),
        _ => panic!("アンカー {id} がFaceに束縛されていること"),
    }
}

/// 底面(mount_face/datum_a)の面積: 80x64 − φ55 − φ6.6×4
fn mount_area() -> f64 {
    80.0 * 64.0 - PI * R_BORE * R_BORE - 4.0 * PI * R_BOLT * R_BOLT
}

// ================================================================ 1. パラメータ変更

#[test]
fn s1_param_change_rebinds_all_anchors_and_geometry_follows() {
    let src = sample_src();
    // wall_t = 4(公称) と 5 で再build → 全アンカー再束縛+幾何値が追従
    for (t, label) in [(4.0, "公称4"), (5.0, "変更5")] {
        let cp = compile_with(&src, &EvalContext::nominal().assign("wall_t", t))
            .unwrap_or_else(|e| panic!("wall_t={t}: {e}"));
        // bearing_bore: edges_ofは外周のみ(リム非対象)なので壁は全高 t
        let wall = anchor_area(&cp, "bearing_bore");
        assert!(
            (wall - 2.0 * PI * R_BORE * t).abs() < EPS,
            "{label}: 穴壁面積が新寸法に追従: {wall}"
        );
        // mount_face / datum_a は板厚に不変
        assert!((anchor_area(&cp, "mount_face") - mount_area()).abs() < EPS, "{label}");
        assert!((anchor_area(&cp, "datum_a") - mount_area()).abs() < EPS, "{label}");
    }
}

// ================================================================ 2. フィーチャー削除

#[test]
fn s2_removing_fillet_keeps_all_anchors() {
    let src = sample_src().replace(FILLET_LINE, "");
    assert!(!src.contains("Fillet"), "Fillet行の除去");
    let cp = compile_with(&src, &EvalContext::nominal()).unwrap_or_else(|e| panic!("{e}"));
    // フィレットなし → 穴壁は全高
    let wall = anchor_area(&cp, "bearing_bore");
    assert!((wall - 2.0 * PI * R_BORE * 4.0).abs() < EPS, "{wall}");
    assert!((anchor_area(&cp, "mount_face") - mount_area()).abs() < EPS);
    assert!((anchor_area(&cp, "datum_a") - mount_area()).abs() < EPS);
}

// ================================================================ 3. アンカー喪失

#[test]
fn s3a_deleting_bore_feature_is_schema_ref_error() {
    // フィーチャーごと削除 → 束縛先フィーチャーが存在しない
    // = スキーマレベル(E-SCHEMA-REF)で最も早期に検出される
    let src = sample_src().replace(BORE_LINES, "");
    assert!(!src.contains("id: \"bore\""), "bore行の除去");
    let errs = validate_design(&src).expect_err("静的検証で落ちること");
    assert!(
        errs.iter()
            .any(|e| e.code == ErrorCode::SchemaRef && e.related.iter().any(|r| r == "bore")),
        "{errs:#?}"
    );
}

#[test]
fn s3b_consuming_bore_wall_reports_anchor_bind_deleted() {
    // フィーチャーは残るが、後続の大径ポケットが壁面を飲み込む
    // → E-ANCHOR-BIND {cause: Deleted, feature_id: 原因フィーチャー}
    let src = with_feature(
        &sample_src(),
        r#"Pocket(id: "swallow", profile: Circ(d: 70.0), depth: 7.0,
               at: on(feature("base").face("top"), center()))"#,
    );
    match compile_with(&src, &EvalContext::nominal()) {
        Err(CompileError::AnchorBind(e)) => {
            assert_eq!(e.anchor_id, "bearing_bore");
            assert_eq!(e.cause, AnchorBindCause::Deleted);
            assert_eq!(e.feature_id, "swallow", "原因フィーチャーの特定");
        }
        Ok(_) => panic!("Deletedになるはず"),
        Err(other) => panic!("AnchorBind(Deleted)のはず: {other}"),
    }
}

// ================================================================ 4. 分割

#[test]
fn s4_splitting_anchored_face_reports_ambiguous_with_hint() {
    // 底面(mount_face)を全幅スロットで2分割
    let src = with_feature(
        &sample_src(),
        r#"Pocket(id: "slot", profile: Rect(x: 90.0, y: 8.0), depth: 1.0,
               at: on(feature("base").face("bottom"), center()))"#,
    );
    match compile_with(&src, &EvalContext::nominal()) {
        Err(CompileError::AnchorBind(e)) => {
            assert_eq!(e.anchor_id, "mount_face");
            assert_eq!(e.cause, AnchorBindCause::Ambiguous);
            assert_eq!(e.feature_id, "slot");
            assert!(
                e.hint.as_deref().unwrap().contains("貼り直し"),
                "修復ヒント: {e:?}"
            );
        }
        Ok(_) => panic!("Ambiguousになるはず"),
        Err(other) => panic!("AnchorBind(Ambiguous)のはず: {other}"),
    }
}

// ================================================================ 5. 決定性

#[test]
fn s5_full_sample_rebuild_is_bit_identical() {
    let src = sample_src();
    let run = || {
        let cp = compile_with(&src, &EvalContext::nominal()).unwrap();
        ["bearing_bore", "mount_face", "datum_a"]
            .iter()
            .map(|id| match cp.anchor(id).unwrap() {
                BoundAnchorRef::Face(f) => (f.area(), f.center()),
                _ => unreachable!(),
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(run(), run(), "束縛結果がビット同一で再現すること");
}

// ================================================================ 6. Open 3点

#[test]
fn s6_open_three_point_builds_bind_all_anchors() {
    // wall_t = Open(range: (3,6), nominal: 4) の3点(下端・公称・上端)
    let src = sample_src();
    for t in [3.0, 4.0, 6.0] {
        let cp = compile_with(&src, &EvalContext::nominal().assign("wall_t", t))
            .unwrap_or_else(|e| panic!("3点build t={t}: {e}"));
        for id in ["bearing_bore", "mount_face", "datum_a"] {
            assert!(cp.anchor(id).is_some(), "t={t}: {id} が束縛されること");
        }
        let wall = anchor_area(&cp, "bearing_bore");
        assert!((wall - 2.0 * PI * R_BORE * t).abs() < EPS, "t={t}: {wall}");
    }
}

// ================================================================ OCCT穴の実測: フィレットに食われたエッジ

#[test]
fn measurement_rim_edge_consumed_by_fillet() {
    // rimアンカーを追加した上で、rim自体を edges_between(bore.wall, base.top)
    // で選択してフィレット → 「エッジがフィレットに消費された」ときの
    // History追跡の実測。期待: Deleted{by: rim_round}
    // (結果はdocs/occt-gotchas.mdに記録 — M2チェッカー設計の入力)
    let src = with_feature(
        &sample_src(),
        r#"Fillet(id: "rim_round", edges: edges_between(feature("bore").face("wall"), feature("base").face("top")), r: 0.5)"#,
    )
    .replace(
        r#"Anchor(id: "bearing_bore", kind: Face, binding: feature("bore").face("wall")),"#,
        r#"Anchor(id: "bore_rim", kind: Edge, binding: feature("bore").edge("rim")),"#,
    );
    match compile_with(&src, &EvalContext::nominal()) {
        Err(CompileError::AnchorBind(e)) => {
            assert_eq!(e.anchor_id, "bore_rim");
            assert_eq!(
                e.cause,
                AnchorBindCause::Deleted,
                "フィレットに食われたエッジはDeletedとして検出されること: {e:?}"
            );
            assert_eq!(e.feature_id, "rim_round");
        }
        Ok(cp) => {
            // もし束縛が生きていたら、それはHistoryがエッジ置換を報告しない
            // 「穴」の実測になる — その場合はこのassertを実測結果で更新する
            panic!(
                "rimはフィレットで消費されるはず(束縛が生存: {:?})",
                cp.anchor("bore_rim").is_some()
            );
        }
        Err(other) => panic!("AnchorBindのはず: {other}"),
    }
}
