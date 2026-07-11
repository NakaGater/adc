//! M1-4 受入テスト (US-05): Pattern展開と添字provides。
//! `feature("<pattern_id>[i]")` / `[i][j]` でインスタンスのprovidesが引けること。
//! 展開規則(センタリング・添字順・回転方向)は docs/provides-predicates.md。

use adc_compile::{compile_part, BoundAnchorRef, CompileError};
use adc_schema::{validate_design, EvalContext};

const EPS: f64 = 1e-6;

fn compile(parts_body: &str) -> Result<adc_compile::CompiledPart, CompileError> {
    let src = format!(
        r#"Design(
    schema_version: "0.1",
    intent: "m1-4 fixture",
    params: [],
    materials: [Material(id: "a5052", density_g_cm3: 2.68, name: "A5052")],
    parts: [
        Part(id: "p1", material: "a5052", process: Machining, {parts_body}),
    ],
    assertions: [],
    rationales: [Rationale(id: "r0", author: Human("t"), basis: Assumption, note: "", timestamp: "2026-07-12T00:00:00Z")],
)"#
    );
    let design = validate_design(&src).unwrap_or_else(|e| panic!("静的検証: {e:#?}\n{src}"));
    compile_part(&design, "p1", &EvalContext::nominal())
}

fn wall_center(cp: &adc_compile::CompiledPart, key: &str) -> [f64; 3] {
    cp.provided_face(key, "wall")
        .unwrap_or_else(|| panic!("{key}.wall が引けること"))
        .center()
}

fn assert_at(c: [f64; 3], x: f64, y: f64, z: f64, what: &str) {
    assert!(
        (c[0] - x).abs() < EPS && (c[1] - y).abs() < EPS && (c[2] - z).abs() < EPS,
        "{what}: expected [{x}, {y}, {z}], got {c:?}"
    );
}

#[test]
fn linear2d_pattern_indexed_provides_and_anchor() {
    // §9サンプルのボルトパターン相当(2x2、pitch 64x44、中心=天面中心)
    let body = r#"
        features: [
            Block(id: "base", x: 80.0, y: 60.0, z: 4.0),
            Pattern(id: "bolts", of: Hole(kind: Simple, d: 6.6, depth: Through),
                    kind: Linear2D, count: (2, 2), pitch: (64.0, 44.0),
                    at: on(feature("base").face("top"), center())),
        ],
        anchors: [
            Anchor(id: "bolt00", kind: Face, binding: feature("bolts[0][0]").face("wall")),
            Anchor(id: "bolt11_axis", kind: Axis, binding: feature("bolts[1][1]").axis("axis")),
        ]"#;
    let cp = compile(body).unwrap_or_else(|e| panic!("コンパイル失敗: {e}"));

    // センタリング: (40∓32, 30∓22)
    assert_at(wall_center(&cp, "bolts[0][0]"), 8.0, 8.0, 2.0, "[0][0]");
    assert_at(wall_center(&cp, "bolts[1][0]"), 72.0, 8.0, 2.0, "[1][0]");
    assert_at(wall_center(&cp, "bolts[0][1]"), 8.0, 52.0, 2.0, "[0][1]");
    assert_at(wall_center(&cp, "bolts[1][1]"), 72.0, 52.0, 2.0, "[1][1]");

    // アンカー束縛
    match cp.anchor("bolt00") {
        Some(BoundAnchorRef::Face(f)) => assert_at(f.center(), 8.0, 8.0, 2.0, "bolt00"),
        _ => panic!("bolt00はFace束縛"),
    }
    match cp.anchor("bolt11_axis") {
        Some(BoundAnchorRef::Axis { origin, .. }) => {
            assert_at(origin, 72.0, 52.0, 4.0, "bolt11 axis origin")
        }
        _ => panic!("bolt11_axisはAxis束縛"),
    }
}

#[test]
fn linear_pattern_single_index() {
    let body = r#"
        features: [
            Block(id: "base", x: 80.0, y: 60.0, z: 4.0),
            Pattern(id: "row", of: Hole(kind: Simple, d: 5.0, depth: Through),
                    kind: Linear, count: 3, pitch: 20.0,
                    at: on(feature("base").face("top"), center())),
        ],
        anchors: []"#;
    let cp = compile(body).unwrap_or_else(|e| panic!("コンパイル失敗: {e}"));
    assert_at(wall_center(&cp, "row[0]"), 20.0, 30.0, 2.0, "[0]");
    assert_at(wall_center(&cp, "row[1]"), 40.0, 30.0, 2.0, "[1]");
    assert_at(wall_center(&cp, "row[2]"), 60.0, 30.0, 2.0, "[2]");
}

#[test]
fn circular_pattern_rotates_about_axis() {
    // 円板の軸まわりに3穴(基準配置r=20、120°step、反時計回り)
    let body = r#"
        features: [
            Cylinder(id: "disk", d: 60.0, h: 4.0),
            Pattern(id: "ring", of: Hole(kind: Simple, d: 6.0, depth: Through),
                    kind: Circular, count: 3, pitch: 120.0,
                    axis: feature("disk").axis("axis"),
                    at: on(feature("disk").face("top"), xy(20.0, 0.0))),
        ],
        anchors: []"#;
    let cp = compile(body).unwrap_or_else(|e| panic!("コンパイル失敗: {e}"));
    let s3 = 3.0f64.sqrt();
    assert_at(wall_center(&cp, "ring[0]"), 20.0, 0.0, 2.0, "[0]");
    assert_at(wall_center(&cp, "ring[1]"), -10.0, 10.0 * s3, 2.0, "[1] (+120°)");
    assert_at(wall_center(&cp, "ring[2]"), -10.0, -10.0 * s3, 2.0, "[2] (+240°)");
}

#[test]
fn pattern_without_at_is_error() {
    let body = r#"
        features: [
            Block(id: "base", x: 80.0, y: 60.0, z: 4.0),
            Pattern(id: "bolts", of: Hole(kind: Simple, d: 6.0, depth: Through),
                    kind: Linear, count: 2, pitch: 20.0),
        ],
        anchors: []"#;
    match compile(body) {
        Err(CompileError::Geometry { message, .. }) => {
            assert!(message.contains("配置(at"), "{message}");
        }
        Ok(_) => panic!("エラーになるはず"),
        Err(other) => panic!("Geometryエラーのはず: {other}"),
    }
}
