//! M5-1 バックログ回収 (M3繰越): Angle mate のE2E受入。
//!
//! M3ではT1語彙で傾斜参照面を構成できず単体テストのみだった。
//! 板金Flange(45°)のinner面(傾斜平面)を参照面にしてE2Eで固定する。
//! 規約 (assembly.rs): ロック軸直交平面への射影ベクトル間の符号付き角を
//! θ に合わせる(右手系)。

use adc_check::{compile_model, run_checks_full, CheckOptions};
use adc_schema::{validate_design, EvalContext};

fn design() -> adc_schema::Design {
    let src = r#"Design(
    schema_version: "0.1",
    intent: "Angle mate E2E: 45°フランジ面に合わせる",
    params: [],
    materials: [Material(id: "spcc", density_g_cm3: 7.85, name: "SPCC")],
    parts: [
        Part(id: "bracket", material: "spcc",
            process: SheetMetal(thickness: 2.0, k_factor: 0.44),
            features: [
                BaseFlange(id: "web", profile: Rect(x: 50.0, y: 30.0)),
                Flange(id: "lip", edge: edges_between(feature("web").face("top"), feature("web").face("+x")),
                       angle: 45.0, length: 20.0, bend_r: 3.0),
                Hole(id: "bore", kind: Simple, d: 10.0, depth: Through,
                     at: on(feature("web").face("top"), center())),
            ],
            anchors: [
                Anchor(id: "bore_axis", kind: Axis, binding: feature("bore").axis("axis")),
                Anchor(id: "flange_inner", kind: Face, binding: feature("lip").face("inner")),
            ]),
        Part(id: "slider", material: "spcc", process: Machining,
            features: [
                Block(id: "body", x: 20.0, y: 20.0, z: 5.0),
                Cylinder(id: "pin", d: 8.0, h: 10.0,
                         at: on(feature("body").face("top"), center())),
            ],
            anchors: [
                Anchor(id: "axis", kind: Axis, binding: feature("pin").axis("axis")),
                Anchor(id: "tab", kind: Face, binding: feature("body").face("+x")),
            ]),
    ],
    assembly: Assembly(id: "assy",
        instances: [Instance(id: "bracket_i", part: "bracket"),
                    Instance(id: "slider_i", part: "slider")],
        mates: [
            Mate(id: "m_coax", kind: Coaxial, a: "bracket_i.bore_axis", b: "slider_i.axis", rationale: "r0"),
            Mate(id: "m_angle", kind: Angle(45.0), a: "bracket_i.flange_inner", b: "slider_i.tab", rationale: "r0"),
        ],
        ground: "bracket_i"),
    assertions: [],
    rationales: [Rationale(id: "r0", author: Human("t"), basis: Assumption, note: "", timestamp: "2026-07-12T00:00:00Z")],
)"#;
    validate_design(src).unwrap_or_else(|e| panic!("検証: {e:#?}"))
}

fn norm3(v: [f64; 3]) -> f64 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

#[test]
fn angle_mate_45deg_flange_face_e2e() {
    let d = design();
    let model = compile_model(&d, &EvalContext::nominal());
    assert!(
        model.assembly_error.is_none(),
        "mate解決成功のはず: {:?}",
        model.assembly_error
    );

    // 配置後の両参照面の法線をロック軸直交平面へ射影し、符号付き角=45°を実測。
    // ロック軸はCoaxialの基準側 = Holeの軸で、その向きは**ドリル方向(−z)**
    // (provides: Hole axis)。右手系の符号付き角は−z軸まわりで測る
    let fa = model
        .placed_anchor_face("bracket_i", "flange_inner")
        .expect("flange_inner");
    let fb = model.placed_anchor_face("slider_i", "tab").expect("tab");
    let (na, nb) = (fa.normal(), fb.normal());
    let proj = |n: [f64; 3]| [n[0], n[1], 0.0];
    let (ra, rb) = (proj(na), proj(nb));
    assert!(norm3(ra) > 1e-9 && norm3(rb) > 1e-9, "射影が退化しない");
    let dot = (ra[0] * rb[0] + ra[1] * rb[1]) / (norm3(ra) * norm3(rb));
    // −z軸まわりの右手系: cross成分の符号を反転
    let cross_mz = -(ra[0] * rb[1] - ra[1] * rb[0]) / (norm3(ra) * norm3(rb));
    let signed_deg = cross_mz.atan2(dot).to_degrees();
    assert!(
        (signed_deg - 45.0).abs() < 1e-6,
        "ロック軸(−z)まわりの射影間符号付き角が45°: {signed_deg}"
    );
}

#[test]
fn angle_mate_dof_accounting() {
    // slider: coaxial(-4) + angle(-1) → 残1(軸方向並進)。未拘束=正常・報告のみ
    let d = design();
    let (_, _, _, dof) = run_checks_full(&d, &EvalContext::nominal(), &CheckOptions::default());
    let s = dof
        .iter()
        .find(|(i, _, _)| i == "slider_i")
        .expect("slider_iのDOF報告");
    assert_eq!(s.1, 1, "{s:?}");
    assert!(s.2.contains("angle"), "{s:?}");
}
