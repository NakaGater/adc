//! M1-1 マイルストーンテスト (US-06 / ADR-001):
//! Block + Hole の2フィーチャーで、`feature("bore").face("wall")` に相当する
//! 「工具円柱の側面 → カット結果の穴壁面」の対応が BRepTools_History 経由で
//! 照会できること。
//!
//! §9サンプルのブラケット相当: Block(80x60x4) に φ55 貫通穴を中央にあける。
//! provides の意味付け(bore.wall = 工具円柱の側面の像)は adc-compile が
//! 担う予定で、ここではその土台となる History 照会の正しさを固定する。

use adc_kernel::{make_box, make_cylinder, FaceHandle, Solid};

const EPS: f64 = 1e-6;
const R: f64 = 27.5; // bore_d 55.0 / 2
const T: f64 = 4.0; // wall_t 公称

fn bracket_and_tool() -> (Solid, Solid) {
    let block = make_box(80.0, 60.0, T);
    // 貫通穴: 板厚を上下に1mmずつ突き抜ける工具円柱
    let tool = make_cylinder(40.0, 30.0, -1.0, R, T + 2.0);
    (block, tool)
}

/// 工具円柱の側面(= provides "wall" の源)を面積で同定する
fn tool_side_face(tool: &Solid) -> FaceHandle {
    let side_area = 2.0 * std::f64::consts::PI * R * (T + 2.0);
    tool.faces()
        .into_iter()
        .find(|f| (f.area() - side_area).abs() < EPS)
        .expect("工具円柱の側面が面積で同定できること")
}

#[test]
fn bore_wall_face_binds_via_history() {
    let (block, tool) = bracket_and_tool();
    let wall_source = tool_side_face(&tool);

    let (result, history) = block.cut_with_history(&tool);

    // 結果は 6面(直方体) + 1面(穴壁) = 7面
    assert_eq!(result.faces().len(), 7, "Block+貫通穴は7面のはず");

    // History照会: 工具側面 → 結果の穴壁面(1面に一意対応)
    let mapped = history.modified_faces(&wall_source);
    assert_eq!(
        mapped.len(),
        1,
        "工具円柱側面はカット結果でちょうど1面に対応すること"
    );
    let wall = &mapped[0];

    // 穴壁面の検証: 面積 = 2πR×板厚、重心 = 軸上の板厚中央
    let expected_area = 2.0 * std::f64::consts::PI * R * T;
    assert!(
        (wall.area() - expected_area).abs() < EPS,
        "穴壁面積: expected {expected_area}, got {}",
        wall.area()
    );
    let c = wall.center();
    assert!((c[0] - 40.0).abs() < EPS, "重心x: {c:?}");
    assert!((c[1] - 30.0).abs() < EPS, "重心y: {c:?}");
    assert!((c[2] - T / 2.0).abs() < EPS, "重心z: {c:?}");
}

#[test]
fn base_top_face_survives_cut_via_history() {
    // feature("base").face("top") 相当の再束縛: ブロック天面はカット後も
    // History経由で追跡でき、穴の分だけ面積が減った1面に対応する
    let (block, tool) = bracket_and_tool();
    let top_before = block
        .faces()
        .into_iter()
        .find(|f| (f.center()[2] - T).abs() < EPS)
        .expect("ブロック天面(z=板厚)が同定できること");

    let (_result, history) = block.cut_with_history(&tool);

    assert!(!history.is_removed_face(&top_before), "天面は消滅しない");
    let mapped = history.modified_faces(&top_before);
    assert_eq!(mapped.len(), 1, "天面はちょうど1面に対応すること");
    let expected_area = 80.0 * 60.0 - std::f64::consts::PI * R * R;
    assert!(
        (mapped[0].area() - expected_area).abs() < EPS,
        "天面積(穴あき): expected {expected_area}, got {}",
        mapped[0].area()
    );
}

#[test]
fn history_mapping_is_deterministic() {
    // 同一入力からの再ビルドで同じ束縛が得られる(Intent成功基準1の土台)
    let run = || {
        let (block, tool) = bracket_and_tool();
        let wall_source = tool_side_face(&tool);
        let (_result, history) = block.cut_with_history(&tool);
        let mapped = history.modified_faces(&wall_source);
        (mapped.len(), mapped[0].area(), mapped[0].center())
    };
    let (n1, a1, c1) = run();
    let (n2, a2, c2) = run();
    assert_eq!(n1, n2);
    assert_eq!(a1, a2, "面積がビット同一で再現すること");
    assert_eq!(c1, c2, "重心がビット同一で再現すること");
}

#[test]
fn unrelated_face_maps_to_nothing_removed_face_reports_removed() {
    let (block, tool) = bracket_and_tool();
    // 工具の上端面(z=T+1の円盤)は結果に残らない(ブロック外)
    let tool_top = tool
        .faces()
        .into_iter()
        .find(|f| (f.center()[2] - (T + 1.0)).abs() < EPS)
        .expect("工具上端面");
    let (_result, history) = block.cut_with_history(&tool);
    assert!(
        history.modified_faces(&tool_top).is_empty(),
        "結果に残らない面のModifiedは空"
    );
}
