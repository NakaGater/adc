//! M1-2 受入テスト (US-05, US-06): フィーチャーT1コンパイル。
//!
//! Block / Cylinder / Hole / Pocket / Boss の5種で「05-schema.md §4.1の
//! provides表どおりにアンカーが引ける」ことをテーブル駆動で固定する。
//! 初期同定述語は docs/provides-predicates.md、配置フレームは
//! docs/placement-frames.md が正典。

use adc_compile::{compile_part, BoundAnchorRef, CompileError};
use adc_kernel::FaceHandle;
use adc_schema::{validate_design, AnchorBindCause, EvalContext};

const EPS: f64 = 1e-6;
const PI: f64 = std::f64::consts::PI;

fn design_src(features: &str, anchors: &str) -> String {
    format!(
        r#"Design(
    schema_version: "0.1",
    intent: "m1-2 fixture",
    params: [
        Param(id: "wall_t", value: Determined(4.0), unit: Mm, rationale: "r0"),
    ],
    materials: [Material(id: "a5052", density_g_cm3: 2.68, name: "A5052")],
    parts: [
        Part(
            id: "p1", material: "a5052", process: Machining,
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

fn compile(features: &str, anchors: &str) -> Result<adc_compile::CompiledPart, CompileError> {
    let src = design_src(features, anchors);
    let design = validate_design(&src).unwrap_or_else(|e| panic!("静的検証: {e:#?}\n{src}"));
    compile_part(&design, "p1", &EvalContext::nominal())
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

fn assert_vec_close(a: [f64; 3], b: [f64; 3], what: &str) {
    for i in 0..3 {
        assert!(
            (a[i] - b[i]).abs() < EPS,
            "{what}[{i}]: expected {b:?}, got {a:?}"
        );
    }
}

const BLOCK: &str = r#"Block(id: "base", x: 80.0, y: 60.0, z: param("wall_t"))"#;

// ================================================================ Block

#[test]
fn block_provides_all_six_faces() {
    // §4.1 Block provides: face: top/bottom/±x/±y
    let table = [
        ("top", [40.0, 30.0, 4.0], [0.0, 0.0, 1.0]),
        ("bottom", [40.0, 30.0, 0.0], [0.0, 0.0, -1.0]),
        ("+x", [80.0, 30.0, 2.0], [1.0, 0.0, 0.0]),
        ("-x", [0.0, 30.0, 2.0], [-1.0, 0.0, 0.0]),
        ("+y", [40.0, 60.0, 2.0], [0.0, 1.0, 0.0]),
        ("-y", [40.0, 0.0, 2.0], [0.0, -1.0, 0.0]),
    ];
    let anchors: String = table
        .iter()
        .map(|(name, _, _)| {
            format!(
                r#"Anchor(id: "a_{}", kind: Face, binding: feature("base").face("{name}")),"#,
                name.replace('+', "p").replace('-', "m")
            )
        })
        .collect();
    let cp = compile_ok(BLOCK, &anchors);
    for (name, center, normal) in table {
        let f = anchor_face(&cp, &format!("a_{}", name.replace('+', "p").replace('-', "m")));
        assert_vec_close(f.center(), center, &format!("base.{name} center"));
        assert_vec_close(f.normal(), normal, &format!("base.{name} normal"));
    }
}

// ================================================================ Cylinder

#[test]
fn cylinder_provides_side_top_bottom_axis() {
    // §4.1 Cylinder provides: face: side/top/bottom, axis
    let features = r#"Cylinder(id: "c", d: 30.0, h: 20.0)"#;
    let anchors = r#"
        Anchor(id: "a_side", kind: Face, binding: feature("c").face("side")),
        Anchor(id: "a_top", kind: Face, binding: feature("c").face("top")),
        Anchor(id: "a_bottom", kind: Face, binding: feature("c").face("bottom")),
        Anchor(id: "a_axis", kind: Axis, binding: feature("c").axis("axis")),
    "#;
    let cp = compile_ok(features, anchors);

    let side = anchor_face(&cp, "a_side");
    assert_close(side.area(), 2.0 * PI * 15.0 * 20.0, "side面積");
    let top = anchor_face(&cp, "a_top");
    assert_vec_close(top.center(), [0.0, 0.0, 20.0], "top center");
    let bottom = anchor_face(&cp, "a_bottom");
    assert_vec_close(bottom.center(), [0.0, 0.0, 0.0], "bottom center");
    match cp.anchor("a_axis") {
        Some(BoundAnchorRef::Axis { origin, dir }) => {
            assert_vec_close(origin, [0.0, 0.0, 0.0], "axis origin");
            assert_vec_close(dir, [0.0, 0.0, 1.0], "axis dir");
        }
        _ => panic!("a_axis はAxis束縛のはず"),
    }
}

// ================================================================ Hole

#[test]
fn hole_simple_through_provides_wall_axis_rim() {
    // §4.1 Hole provides: face: wall, axis, edge: rim (§9サンプル相当)
    let features = format!(
        r#"{BLOCK},
        Hole(id: "bore", kind: Simple, d: 55.0, depth: Through,
             at: on(feature("base").face("top"), center()))"#
    );
    let anchors = r#"
        Anchor(id: "bearing_bore", kind: Face, binding: feature("bore").face("wall")),
        Anchor(id: "bore_axis", kind: Axis, binding: feature("bore").axis("axis")),
        Anchor(id: "rim", kind: Edge, binding: feature("bore").edge("rim")),
    "#;
    let cp = compile_ok(&features, anchors);

    let wall = anchor_face(&cp, "bearing_bore");
    assert_close(wall.area(), 2.0 * PI * 27.5 * 4.0, "穴壁面積(貫通・板厚4)");
    assert_vec_close(wall.center(), [40.0, 30.0, 2.0], "穴壁重心");

    match cp.anchor("bore_axis") {
        Some(BoundAnchorRef::Axis { origin, dir }) => {
            assert_vec_close(origin, [40.0, 30.0, 4.0], "axis origin(配置点)");
            assert_vec_close(dir, [0.0, 0.0, -1.0], "axis dir(掘り込み方向)");
        }
        _ => panic!("bore_axis はAxis束縛のはず"),
    }

    // rim: 開口円(配置面側 z=4)
    match cp.anchor("rim") {
        Some(BoundAnchorRef::Edge(e)) => {
            assert!(e.is_circle(), "rimは円エッジ");
            assert_close(e.start()[2], 4.0, "rimは配置面側(z=4)");
        }
        _ => panic!("rim はEdge束縛のはず"),
    }
}

#[test]
fn hole_blind_provides_bottom() {
    let features = format!(
        r#"{BLOCK},
        Hole(id: "h1", kind: Simple, d: 10.0, depth: 2.5,
             at: on(feature("base").face("top"), center()))"#
    );
    let anchors = r#"
        Anchor(id: "a_bottom", kind: Face, binding: feature("h1").face("bottom")),
        Anchor(id: "a_wall", kind: Face, binding: feature("h1").face("wall")),
    "#;
    let cp = compile_ok(&features, anchors);
    let bottom = anchor_face(&cp, "a_bottom");
    // 底面: z = 4.0 - 2.5 = 1.5、中心は配置点直下
    assert_vec_close(bottom.center(), [40.0, 30.0, 1.5], "止まり穴底面");
    assert_close(bottom.area(), PI * 5.0 * 5.0, "底面積");
    let wall = anchor_face(&cp, "a_wall");
    assert_close(wall.area(), 2.0 * PI * 5.0 * 2.5, "止まり穴壁面積");
}

#[test]
fn hole_counterbore_wall_is_small_bore_side() {
    // §9サンプルのボルト穴相当(座ぐり)。wallは小径側面 (provides-predicates.md)
    let features = format!(
        r#"{BLOCK},
        Hole(id: "cb", kind: Counterbore, d: 6.6, cb_d: 11.0, cb_depth: 2.0, depth: Through,
             at: on(feature("base").face("top"), xy(30.0, 20.0)))"#
    );
    let anchors =
        r#"Anchor(id: "a_wall", kind: Face, binding: feature("cb").face("wall"))"#;
    let cp = compile_ok(&features, anchors);
    let wall = anchor_face(&cp, "a_wall");
    // 小径部: 座ぐり底(z=2.0)から下面(z=0)まで = 高さ2.0
    assert_close(wall.area(), 2.0 * PI * 3.3 * 2.0, "座ぐり穴の小径壁面積");
    assert_vec_close(wall.center(), [70.0, 50.0, 1.0], "小径壁重心");
}

// ================================================================ Pocket

#[test]
fn pocket_rect_provides_floor_and_walls() {
    // §4.1 Pocket provides: face: floor/walls
    let features = format!(
        r#"{BLOCK},
        Pocket(id: "pk", profile: Rect(x: 20.0, y: 10.0), depth: 1.5,
               at: on(feature("base").face("top"), center()))"#
    );
    let anchors = r#"Anchor(id: "a_floor", kind: Face, binding: feature("pk").face("floor"))"#;
    let cp = compile_ok(&features, anchors);

    let floor = anchor_face(&cp, "a_floor");
    assert_vec_close(floor.center(), [40.0, 30.0, 2.5], "ポケット床");
    assert_close(floor.area(), 20.0 * 10.0, "床面積");

    let walls = cp.provided_face_set("pk", "walls").expect("walls集合");
    assert_eq!(walls.len(), 4, "Rectポケットの壁は4面");
    let wall_area: f64 = walls.iter().map(|f| f.area()).sum();
    assert_close(wall_area, 2.0 * (20.0 + 10.0) * 1.5, "壁面積合計");
}

#[test]
fn pocket_circ_and_rounded_rect() {
    // Circプロファイル
    let features = format!(
        r#"{BLOCK},
        Pocket(id: "pk", profile: Circ(d: 12.0), depth: 2.0,
               at: on(feature("base").face("top"), xy(-20.0, 10.0)))"#
    );
    let cp = compile_ok(&features, "");
    let walls = cp.provided_face_set("pk", "walls").expect("walls");
    assert_eq!(walls.len(), 1, "Circポケットの壁は円筒1面");
    assert_close(walls[0].area(), 2.0 * PI * 6.0 * 2.0, "円筒壁面積");

    // corner_r付きRect: 壁 = 平面4 + 丸め4 = 8面
    let features = format!(
        r#"{BLOCK},
        Pocket(id: "pk", profile: Rect(x: 20.0, y: 10.0), depth: 1.0, corner_r: 2.0,
               at: on(feature("base").face("top"), center()))"#
    );
    let cp = compile_ok(&features, "");
    let walls = cp.provided_face_set("pk", "walls").expect("walls");
    assert_eq!(walls.len(), 8, "corner_r付きRectの壁は平面4+丸め4");
}

// ================================================================ Boss

#[test]
fn boss_provides_top_and_side() {
    // §4.1 Boss provides: face: top/side
    let features = format!(
        r#"{BLOCK},
        Boss(id: "pad", profile: Circ(d: 16.0), height: 5.0,
             at: on(feature("base").face("top"), xy(10.0, -5.0)))"#
    );
    let anchors = r#"Anchor(id: "a_top", kind: Face, binding: feature("pad").face("top"))"#;
    let cp = compile_ok(&features, anchors);

    let top = anchor_face(&cp, "a_top");
    assert_vec_close(top.center(), [50.0, 25.0, 9.0], "ボス頂面(z=4+5)");
    assert_close(top.area(), PI * 8.0 * 8.0, "頂面積");

    let side = cp.provided_face_set("pad", "side").expect("side集合");
    assert_eq!(side.len(), 1, "円形ボスの側面は1面");
    assert_close(side[0].area(), 2.0 * PI * 8.0 * 5.0, "側面積");
}

// ================================================================ 前送り(再束縛)

#[test]
fn provides_forwarding_across_subsequent_features() {
    // base.top は Hole→Pocket の2操作を跨いで前送りされ、面積が両方の分だけ減る
    let features = format!(
        r#"{BLOCK},
        Hole(id: "bore", kind: Simple, d: 20.0, depth: Through,
             at: on(feature("base").face("top"), center())),
        Pocket(id: "pk", profile: Rect(x: 10.0, y: 10.0), depth: 1.0,
               at: on(feature("base").face("top"), xy(30.0, 20.0)))"#
    );
    let anchors = r#"Anchor(id: "a_top", kind: Face, binding: feature("base").face("top"))"#;
    let cp = compile_ok(&features, anchors);
    let top = anchor_face(&cp, "a_top");
    let expected = 80.0 * 60.0 - PI * 10.0 * 10.0 - 10.0 * 10.0;
    assert_close(top.area(), expected, "2操作跨ぎの天面積");
}

// ================================================================ E-ANCHOR-BIND

#[test]
fn face_anchor_on_face_set_is_ambiguous_with_hint() {
    // 決定(a): 集合provides(walls)への単一面アンカーは Ambiguous + 修復ヒント
    let features = format!(
        r#"{BLOCK},
        Pocket(id: "pk", profile: Rect(x: 20.0, y: 10.0), depth: 1.5,
               at: on(feature("base").face("top"), center()))"#
    );
    let anchors = r#"Anchor(id: "bad", kind: Face, binding: feature("pk").face("walls"))"#;
    match compile(&features, anchors) {
        Err(CompileError::AnchorBind(e)) => {
            assert_eq!(e.cause, AnchorBindCause::Ambiguous);
            assert_eq!(e.anchor_id, "bad");
            assert!(e.hint.as_deref().unwrap().contains("貼り直し"), "{e:?}");
        }
        Ok(_) => panic!("Ambiguousになるはず(成功した)"),
        Err(other) => panic!("Ambiguousになるはず: {other}"),
    }
}

#[test]
fn deleted_provides_reports_deleted_with_causing_feature() {
    // ボス頂面を、その後の大径貫通穴で完全に食い尽くす → Deleted {by: bore}
    let features = format!(
        r#"{BLOCK},
        Boss(id: "pad", profile: Circ(d: 10.0), height: 3.0,
             at: on(feature("base").face("top"), center())),
        Hole(id: "bore", kind: Simple, d: 30.0, depth: Through,
             at: on(feature("base").face("top"), center()))"#
    );
    let anchors = r#"Anchor(id: "gone", kind: Face, binding: feature("pad").face("top"))"#;
    match compile(&features, anchors) {
        Err(CompileError::AnchorBind(e)) => {
            assert_eq!(e.cause, AnchorBindCause::Deleted);
            assert_eq!(e.feature_id, "bore", "原因フィーチャーが特定されること");
        }
        Ok(_) => panic!("Deletedになるはず(成功した)"),
        Err(other) => panic!("Deletedになるはず: {other}"),
    }
}

#[test]
fn unknown_provides_element_is_error() {
    let anchors = r#"Anchor(id: "bad", kind: Face, binding: feature("base").face("nonexistent"))"#;
    match compile(BLOCK, anchors) {
        Err(CompileError::UnknownProvides { feature_id, elem }) => {
            assert_eq!(feature_id, "base");
            assert_eq!(elem, "nonexistent");
        }
        Ok(_) => panic!("UnknownProvidesになるはず(成功した)"),
        Err(other) => panic!("UnknownProvidesになるはず: {other}"),
    }
}

// ================================================================ 配置

#[test]
fn placement_xy_and_offset_follow_frame_rules() {
    // docs/placement-frames.md: 天面(z=+Z)では x=+X, y=+Y
    let features = format!(
        r#"{BLOCK},
        Hole(id: "h1", kind: Simple, d: 6.0, depth: Through,
             at: on(feature("base").face("top"), xy(10.0, 5.0))),
        Hole(id: "h2", kind: Simple, d: 6.0, depth: Through,
             at: offset(on(feature("base").face("top"), center()), (-15.0, 0.0, 0.0)))"#
    );
    let cp = compile_ok(&features, "");
    let w1 = cp.provided_face("h1", "wall").expect("h1.wall");
    assert_vec_close(w1.center(), [50.0, 35.0, 2.0], "xy(10,5) → 重心+(10,5)");
    let w2 = cp.provided_face("h2", "wall").expect("h2.wall");
    assert_vec_close(w2.center(), [25.0, 30.0, 2.0], "offset(-15,0,0)");
}

#[test]
fn placement_on_side_face_switches_reference_axis() {
    // +x側面(z=+X)では基準軸が+Yに切替: x=+Y射影, y = z×x = +Z…
    // 側面から水平に止まり穴を掘り、壁の重心で配置を検証する
    let features = format!(
        r#"{BLOCK},
        Hole(id: "h1", kind: Simple, d: 2.0, depth: 10.0,
             at: on(feature("base").face("+x"), center()))"#
    );
    let cp = compile_ok(&features, "");
    let wall = cp.provided_face("h1", "wall").expect("wall");
    // +x面の重心 (80, 30, 2) から -X方向に深さ10 → 壁重心 (75, 30, 2)
    assert_vec_close(wall.center(), [75.0, 30.0, 2.0], "側面穴の壁重心");
}

// ================================================================ 決定性

#[test]
fn compilation_is_deterministic() {
    let features = format!(
        r#"{BLOCK},
        Hole(id: "bore", kind: Simple, d: 55.0, depth: Through,
             at: on(feature("base").face("top"), center()))"#
    );
    let anchors = r#"Anchor(id: "a", kind: Face, binding: feature("bore").face("wall"))"#;
    let run = || {
        let cp = compile_ok(&features, anchors);
        let f = anchor_face(&cp, "a");
        (f.area(), f.center())
    };
    let (a1, c1) = run();
    let (a2, c2) = run();
    assert_eq!(a1, a2, "面積がビット同一");
    assert_eq!(c1, c2, "重心がビット同一");
}
