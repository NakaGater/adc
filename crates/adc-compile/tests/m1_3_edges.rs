//! M1-3 受入テスト (US-05, US-08): Fillet/Chamfer + EdgeSelector遅延解決
//! + FromEdge / Countersink + E-FEATURE-FAIL構造化。

use adc_compile::{compile_part, BoundAnchorRef, CompileError};
use adc_schema::{validate_design, AnchorBindCause, EvalContext};

const EPS: f64 = 1e-6;
const PI: f64 = std::f64::consts::PI;

fn design_src(features: &str, anchors: &str) -> String {
    format!(
        r#"Design(
    schema_version: "0.1",
    intent: "m1-3 fixture",
    params: [],
    materials: [Material(id: "a5052", density_g_cm3: 2.68, name: "A5052")],
    parts: [
        Part(
            id: "p1", material: "a5052", process: Machining,
            features: [{features}],
            anchors: [{anchors}],
        ),
    ],
    assertions: [],
    rationales: [Rationale(id: "r0", author: Human("t"), basis: Assumption, note: "", timestamp: "2026-07-12T00:00:00Z")],
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

const BLOCK: &str = r#"Block(id: "base", x: 80.0, y: 60.0, z: 4.0)"#;

// ================================================================ Fillet

#[test]
fn fillet_edges_of_top_then_anchors_survive() {
    // 受入: edges_of(base.top)へのFillet後、天面/側面アンカーが前送りで生存
    let features = format!(
        r#"{BLOCK},
        Fillet(id: "f1", edges: edges_of(feature("base").face("top")), r: 2.0),
        Hole(id: "bore", kind: Simple, d: 20.0, depth: Through,
             at: on(feature("base").face("top"), center()))"#
    );
    let anchors = r#"
        Anchor(id: "a_top", kind: Face, binding: feature("base").face("top")),
        Anchor(id: "a_side", kind: Face, binding: feature("base").face("+x")),
        Anchor(id: "a_wall", kind: Face, binding: feature("bore").face("wall")),
    "#;
    let cp = compile_ok(&features, anchors);

    // 天面: フィレットで外周が内側に寄り、穴の分も減る
    let top = match cp.anchor("a_top") {
        Some(BoundAnchorRef::Face(f)) => f,
        _ => panic!("a_top"),
    };
    let n = top.normal();
    assert!((n[2] - 1.0).abs() < EPS, "天面法線は+Zのまま: {n:?}");
    assert!(
        top.area() < 80.0 * 60.0 - PI * 100.0 && top.area() > 70.0 * 50.0 - PI * 100.0,
        "天面積がフィレット+穴分減っていること: {}",
        top.area()
    );

    // 側面: 上端ストリップがフィレットに置換されて面積減
    let side = match cp.anchor("a_side") {
        Some(BoundAnchorRef::Face(f)) => f,
        _ => panic!("a_side"),
    };
    assert!(
        side.area() < 80.0 * 4.0 && side.area() > 80.0 * 1.0,
        "側面積: {}",
        side.area()
    );

    // 穴壁はフィレットの影響を受けない
    let wall = match cp.anchor("a_wall") {
        Some(BoundAnchorRef::Face(f)) => f,
        _ => panic!("a_wall"),
    };
    assert!((wall.area() - 2.0 * PI * 10.0 * 4.0).abs() < EPS, "{}", wall.area());
}

// ================================================================ Chamfer (edges_between)

#[test]
fn chamfer_edges_between_boss_side_and_base_top() {
    // 受入: edges_between(boss.side, base.top)の共有辺(ボス基部円)をChamfer
    let features = format!(
        r#"{BLOCK},
        Boss(id: "pad", profile: Circ(d: 16.0), height: 5.0,
             at: on(feature("base").face("top"), center())),
        Chamfer(id: "c1", edges: edges_between(feature("pad").face("side"), feature("base").face("top")), size: 1.0)"#
    );
    let anchors = r#"
        Anchor(id: "a_top", kind: Face, binding: feature("base").face("top")),
        Anchor(id: "a_boss_top", kind: Face, binding: feature("pad").face("top")),
    "#;
    let cp = compile_ok(&features, anchors);

    // 天面: ボス基部の面取りで r=9 の円まで削られる
    let top = match cp.anchor("a_top") {
        Some(BoundAnchorRef::Face(f)) => f,
        _ => panic!("a_top"),
    };
    assert!(
        (top.area() - (80.0 * 60.0 - PI * 81.0)).abs() < EPS,
        "天面積(面取り後): {}",
        top.area()
    );

    // ボス側面: 下端1mmが面取りに置換 → 高さ4
    let side = cp.provided_face_set("pad", "side").expect("side");
    assert_eq!(side.len(), 1);
    assert!(
        (side[0].area() - 2.0 * PI * 8.0 * 4.0).abs() < EPS,
        "ボス側面積: {}",
        side[0].area()
    );

    // ボス頂面は不変
    let btop = match cp.anchor("a_boss_top") {
        Some(BoundAnchorRef::Face(f)) => f,
        _ => panic!("a_boss_top"),
    };
    assert!((btop.area() - PI * 64.0).abs() < EPS);
}

// ================================================================ E-FEATURE-FAIL

#[test]
fn oversized_fillet_reports_structured_failure_without_abort() {
    // 受入: 半径過大フィレットがE-FEATURE-FAILで構造化報告される(abortしない)
    let features = format!(
        r#"{BLOCK},
        Fillet(id: "f1", edges: edges_of(feature("base").face("top")), r: 50.0)"#
    );
    match compile(&features, "") {
        Err(CompileError::FeatureFail(e)) => {
            assert_eq!(e.feature_id, "f1");
            assert!(!e.occt_error.is_empty(), "OCCTエラーを含む: {e:?}");
            let hint = e.hint.as_deref().expect("hint必須");
            assert!(hint.contains("過大") && hint.contains("最小曲率半径"), "{hint}");
            // JSONシリアライズ可能 (US-08: エージェント修復ループの入力)
            let json = serde_json::to_string(&e).unwrap();
            assert!(json.contains("\"occt_error\""), "{json}");
        }
        Ok(_) => panic!("失敗するはず(r=50 on 80x60x4)"),
        Err(other) => panic!("FeatureFailになるはず: {other}"),
    }
}

// ================================================================ Countersink + FromEdge

#[test]
fn countersink_hole_with_from_edge_placement() {
    // 受入: Countersink(円錐工具)+ FromEdge配置
    // エッジ = top×(+x)の共有辺(中点(80,30,4)) → d=10で面内側 → (70,30,4)
    let features = format!(
        r#"{BLOCK},
        Hole(id: "cs", kind: Countersink, d: 6.0, cs_d: 12.0, cs_angle: 90.0, depth: Through,
             at: on(feature("base").face("top"),
                    from_edge(edges_between(feature("base").face("top"), feature("base").face("+x")), 10.0, 0.0)))"#
    );
    let anchors = r#"
        Anchor(id: "a_wall", kind: Face, binding: feature("cs").face("wall")),
        Anchor(id: "a_rim", kind: Edge, binding: feature("cs").edge("rim")),
    "#;
    let cp = compile_ok(&features, anchors);

    // 皿もみ深さ t_cs = (6-3)/tan(45°) = 3 → wall は z∈[0,1]
    let wall = match cp.anchor("a_wall") {
        Some(BoundAnchorRef::Face(f)) => f,
        _ => panic!("a_wall"),
    };
    assert!((wall.area() - 2.0 * PI * 3.0 * 1.0).abs() < EPS, "{}", wall.area());
    let c = wall.center();
    assert!((c[0] - 70.0).abs() < EPS && (c[1] - 30.0).abs() < EPS && (c[2] - 0.5).abs() < EPS,
        "wall重心: {c:?}");

    // rim = wallの円エッジのうち配置面に最も近い側 → z=1(皿もみ底)
    match cp.anchor("a_rim") {
        Some(BoundAnchorRef::Edge(e)) => {
            assert!(e.is_circle());
            assert!((e.start()[2] - 1.0).abs() < EPS, "rim z: {:?}", e.start());
        }
        _ => panic!("a_rim"),
    }
}

#[test]
fn from_edge_along_offset_is_deterministic() {
    // along=5: エッジ向きは+X/+Y/+Z優先の決定的向き付け(このエッジは+Y) → y=35
    let features = format!(
        r#"{BLOCK},
        Hole(id: "h1", kind: Simple, d: 4.0, depth: Through,
             at: on(feature("base").face("top"),
                    from_edge(edges_between(feature("base").face("top"), feature("base").face("+x")), 10.0, 5.0)))"#
    );
    let cp = compile_ok(&features, "");
    let wall = cp.provided_face("h1", "wall").expect("wall");
    let c = wall.center();
    assert!((c[0] - 70.0).abs() < EPS && (c[1] - 35.0).abs() < EPS, "{c:?}");
}

#[test]
fn from_edge_with_multiple_edges_is_error() {
    // edges_of(+x面)は4辺 → 1本に定まらない
    let features = format!(
        r#"{BLOCK},
        Hole(id: "h1", kind: Simple, d: 4.0, depth: Through,
             at: on(feature("base").face("top"),
                    from_edge(edges_of(feature("base").face("+x")), 10.0, 0.0)))"#
    );
    match compile(&features, "") {
        Err(CompileError::Geometry { message, .. }) => {
            assert!(message.contains("1本に定まりません"), "{message}");
        }
        Ok(_) => panic!("エラーになるはず"),
        Err(other) => panic!("Geometryエラーのはず: {other}"),
    }
}

// ================================================================ 分割→Ambiguous

#[test]
fn provides_face_split_reports_ambiguous() {
    // 受入: provides面(base.top)を全幅スロットで2分割 → Ambiguous
    // (分割検出の前送りロジックは操作種別に依存しない)
    let features = format!(
        r#"{BLOCK},
        Pocket(id: "slot", profile: Rect(x: 90.0, y: 10.0), depth: 2.0,
               at: on(feature("base").face("top"), center()))"#
    );
    let anchors = r#"Anchor(id: "a_top", kind: Face, binding: feature("base").face("top"))"#;
    match compile(&features, anchors) {
        Err(CompileError::AnchorBind(e)) => {
            assert_eq!(e.cause, AnchorBindCause::Ambiguous);
            assert_eq!(e.feature_id, "slot");
            assert!(e.hint.is_some());
        }
        Ok(_) => panic!("Ambiguousになるはず"),
        Err(other) => panic!("AnchorBindになるはず: {other}"),
    }
}
