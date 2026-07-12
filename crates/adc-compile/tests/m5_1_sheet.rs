//! M5-1 受入テスト (US-09): 板金フィーチャーT2コンパイル。
//!
//! BaseFlange / Flange / Cutout / Relief の4種で「05-schema.md §4.2の
//! provides表どおりにアンカーが引ける」ことをテーブル駆動で固定する
//! (M1-2と同形式)。構築方式は docs/design-notes/m5-1-sheet-metal.md 案B、
//! provides述語は docs/provides-predicates.md T2節が正典。

use adc_compile::{compile_part, BoundAnchorRef, CompileError};
use adc_kernel::{FaceHandle, SurfaceKind};
use adc_schema::{validate_design, EvalContext};

const EPS: f64 = 1e-6;
const PI: f64 = std::f64::consts::PI;

/// t=2の板金カバー: 50×30ベース+90°フランジ(+x縁, len20, r3)
fn design_src(features: &str, anchors: &str) -> String {
    format!(
        r#"Design(
    schema_version: "0.1",
    intent: "m5-1 fixture",
    params: [],
    materials: [Material(id: "spcc", density_g_cm3: 7.85, name: "SPCC")],
    parts: [
        Part(
            id: "cover", material: "spcc",
            process: SheetMetal(thickness: 2.0, k_factor: 0.44),
            features: [{features}],
            anchors: [{anchors}],
        ),
    ],
    assertions: [],
    rationales: [
        Rationale(id: "r0", author: Human("t"), basis: Assumption, note: "", timestamp: "2026-07-12T00:00:00Z"),
    ],
)"#
    )
}

const BASE: &str = r#"BaseFlange(id: "web", profile: Rect(x: 50.0, y: 30.0))"#;
const FLANGE: &str = r#"Flange(id: "lip", edge: edges_between(feature("web").face("top"), feature("web").face("+x")),
               angle: 90.0, length: 20.0, bend_r: 3.0)"#;

fn compile(features: &str, anchors: &str) -> Result<adc_compile::CompiledPart, CompileError> {
    let src = design_src(features, anchors);
    let design = validate_design(&src).unwrap_or_else(|e| panic!("静的検証: {e:#?}\n{src}"));
    compile_part(&design, "cover", &EvalContext::nominal())
}

fn compile_ok(features: &str, anchors: &str) -> adc_compile::CompiledPart {
    compile(features, anchors).unwrap_or_else(|e| panic!("コンパイル失敗: {e}"))
}

fn anchor_face<'a>(cp: &'a adc_compile::CompiledPart, id: &str) -> &'a FaceHandle {
    match cp.anchor(id) {
        Some(BoundAnchorRef::Face(f)) => f,
        other => panic!("アンカー {id} はFaceに束縛されるはず: {:?}", other.is_some()),
    }
}

fn assert_close(a: f64, b: f64, what: &str) {
    assert!((a - b).abs() < EPS, "{what}: expected {b}, got {a}");
}

/// §4.2のprovides表どおりにアンカーが引ける(テーブル駆動)
#[test]
fn t2_provides_table() {
    // (アンカー定義, 期待面積, 期待面種, 説明)
    let table: Vec<(&str, &str, f64, SurfaceKind, &str)> = vec![
        (
            "a_top",
            r#"feature("web").face("top")"#,
            50.0 * 30.0,
            SurfaceKind::Plane,
            "ベース上面(フランジは+x側面に接合し上面を消費しない)",
        ),
        (
            "a_bend_in",
            r#"feature("lip").face("bend_inner")"#,
            (PI / 2.0) * 3.0 * 30.0,
            SurfaceKind::Cylinder,
            "曲げ内円筒面 α·r·w",
        ),
        (
            "a_bend_out",
            r#"feature("lip").face("bend_outer")"#,
            (PI / 2.0) * 5.0 * 30.0,
            SurfaceKind::Cylinder,
            "曲げ外円筒面 α·(r+t)·w",
        ),
        (
            "a_inner",
            r#"feature("lip").face("inner")"#,
            20.0 * 30.0,
            SurfaceKind::Plane,
            "平坦部内面 length×エッジ長",
        ),
        (
            "a_outer",
            r#"feature("lip").face("outer")"#,
            20.0 * 30.0,
            SurfaceKind::Plane,
            "平坦部外面",
        ),
        (
            "a_tip",
            r#"feature("lip").face("tip")"#,
            2.0 * 30.0,
            SurfaceKind::Plane,
            "先端小口 t×エッジ長",
        ),
    ];
    for (aid, binding, area, kind, what) in table {
        let features = format!("{BASE},\n            {FLANGE}");
        let anchors = format!(r#"Anchor(id: "{aid}", kind: Face, binding: {binding})"#);
        let cp = compile_ok(&features, &anchors);
        let f = anchor_face(&cp, aid);
        assert_eq!(f.surface_kind(), kind, "{what}");
        assert_close(f.area(), area, what);
    }
}

/// 90°曲げの体積 = ベース + 曲げ扇形 + 平坦部(材料体積の検算)
#[test]
fn bent_volume_matches_analytic() {
    let features = format!("{BASE},\n            {FLANGE}");
    let cp = compile_ok(&features, "");
    let bend = (PI / 4.0) * (5.0f64.powi(2) - 3.0f64.powi(2)) * 30.0;
    let expect = 50.0 * 30.0 * 2.0 + bend + 20.0 * 2.0 * 30.0;
    assert!(
        (cp.solid.volume() - expect).abs() < 1e-6,
        "体積: expected {expect}, got {}",
        cp.solid.volume()
    );
}

/// Cutout(Circ)はwall、Relief(Round)はprovidesなしで体積のみ減る
#[test]
fn cutout_and_relief() {
    let features = format!(
        r#"{BASE},
            Cutout(id: "window", profile: Circ(d: 10.0),
                   at: on(feature("web").face("top"), center())),
            Relief(id: "rl", kind: Round(d: 4.0),
                   at: on(feature("web").face("top"), xy(-20.0, 10.0)))"#
    );
    let anchors = r#"Anchor(id: "a_wall", kind: Face, binding: feature("window").face("wall"))"#;
    let cp = compile_ok(&features, anchors);
    let f = anchor_face(&cp, "a_wall");
    assert_eq!(f.surface_kind(), SurfaceKind::Cylinder);
    assert_close(f.area(), PI * 10.0 * 2.0, "Cutout wall = πdt");
    let expect = 50.0 * 30.0 * 2.0 - PI * 25.0 * 2.0 - PI * 4.0 * 2.0;
    assert!(
        (cp.solid.volume() - expect).abs() < 1e-6,
        "貫通体積: expected {expect}, got {}",
        cp.solid.volume()
    );
}

/// E-FEATURE-FAIL: エッジ複数(edges_ofは外周4本)/ angle範囲外
#[test]
fn flange_edge_and_angle_failures() {
    let multi = format!(
        r#"{BASE},
            Flange(id: "lip", edge: edges_of(feature("web").face("top")),
                   angle: 90.0, length: 20.0, bend_r: 3.0)"#
    );
    match compile(&multi, "") {
        Err(CompileError::FeatureFail(e)) => {
            assert!(e.occt_error.contains("4本"), "{e:?}");
            assert!(e.hint.as_deref().unwrap().contains("edges_between"), "{e:?}");
        }
        Err(other) => panic!("E-FEATURE-FAILのはず: {other}"),
        Ok(_) => panic!("E-FEATURE-FAILのはず(成功してしまった)"),
    }

    let flat = format!("{BASE},\n            {FLANGE}").replace("angle: 90.0", "angle: 180.0");
    match compile(&flat, "") {
        Err(CompileError::FeatureFail(e)) => {
            assert!(e.hint.as_deref().unwrap().contains("(0°, 180°)"), "{e:?}");
        }
        Err(other) => panic!("E-FEATURE-FAILのはず: {other}"),
        Ok(_) => panic!("E-FEATURE-FAILのはず(成功してしまった)"),
    }
}

/// 45°曲げ: 円筒面積が角度に比例(Angle mate E2Eフィクスチャの下ごしらえ)
#[test]
fn angled_flange_45deg() {
    let features = format!("{BASE},\n            {FLANGE}").replace("angle: 90.0", "angle: 45.0");
    let anchors = r#"Anchor(id: "a_in", kind: Face, binding: feature("lip").face("inner"))"#;
    let cp = compile_ok(&features, anchors);
    let f = anchor_face(&cp, "a_in");
    assert_close(f.area(), 20.0 * 30.0, "45°でも平坦部面積は同じ");
    // 内面の法線は45°傾く(z成分 = -cos45 … 曲げ中心側を向く)
    let n = f.normal();
    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    let nz = (n[2] / len).abs();
    assert!(
        (nz - (PI / 4.0).cos()).abs() < 1e-6,
        "45°傾斜面: |nz|=cos45 expected, got {nz}"
    );
}
