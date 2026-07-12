//! M5-1 受入: 展開長がexplainの派生量として引けること (docs/explain-schema.md)。

use adc_schema::{explain, validate_design};

#[test]
fn explain_part_exposes_flat_length_and_ba() {
    let src = r#"Design(
    schema_version: "0.1",
    intent: "explain派生量",
    params: [],
    materials: [Material(id: "spcc", density_g_cm3: 7.85, name: "SPCC")],
    parts: [
        Part(id: "cover", material: "spcc",
            process: SheetMetal(thickness: 2.0, k_factor: 0.44),
            features: [
                BaseFlange(id: "web", profile: Rect(x: 50.0, y: 30.0)),
                Flange(id: "lip", edge: edges_between(feature("web").face("top"), feature("web").face("+x")),
                       angle: 90.0, length: 20.0, bend_r: 3.0),
            ],
            anchors: []),
    ],
    assertions: [],
    rationales: [],
)"#;
    let d = validate_design(src).unwrap();
    let out = explain(&d, "cover");
    let m = &out.matches[0];
    let derived = m.derived.as_ref().expect("SheetMetal部品はderivedを持つ");
    let ba = std::f64::consts::FRAC_PI_2 * (3.0 + 0.44 * 2.0);
    let flat = derived["flat_length"].as_f64().expect("flat_length");
    assert!((flat - (50.0 + 20.0 + ba)).abs() < 1e-9, "{flat}");
    let bends = derived["bends"].as_array().unwrap();
    assert_eq!(bends[0]["feature_id"], "lip");
    assert!((bends[0]["ba"].as_f64().unwrap() - ba).abs() < 1e-9);

    // 切削部品はderivedを持たない(フィールド省略 — 後方互換)
    let mach = src
        .replace("process: SheetMetal(thickness: 2.0, k_factor: 0.44)", "process: Machining")
        .replace(
            r#"BaseFlange(id: "web", profile: Rect(x: 50.0, y: 30.0)),
                Flange(id: "lip", edge: edges_between(feature("web").face("top"), feature("web").face("+x")),
                       angle: 90.0, length: 20.0, bend_r: 3.0),"#,
            r#"Block(id: "web", x: 50.0, y: 30.0, z: 2.0),"#,
        );
    let d = validate_design(&mach).unwrap();
    let out = explain(&d, "cover");
    assert!(out.matches[0].derived.is_none());
}
