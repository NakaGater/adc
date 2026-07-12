//! M5-3 静的規則: ToleranceStack1D経路の連結性 (dim[i].to == dim[i+1].from)。
//! 非連結は E-SCHEMA-REF (05-schema.md §7.1)。

use adc_schema::{validate_design, ErrorCode};

fn src(path: &str) -> String {
    format!(
        r#"Design(
    schema_version: "0.1",
    intent: "m5-3 static fixture",
    params: [],
    materials: [Material(id: "a5052", density_g_cm3: 2.68, name: "A5052")],
    parts: [
        Part(id: "p1", material: "a5052", process: Machining,
            features: [Block(id: "base", x: 10.0, y: 10.0, z: 4.0)],
            anchors: [
                Anchor(id: "top", kind: Face, binding: feature("base").face("top")),
                Anchor(id: "bottom", kind: Face, binding: feature("base").face("bottom")),
            ]),
    ],
    assembly: Assembly(id: "assy",
        instances: [Instance(id: "i1", part: "p1"), Instance(id: "i2", part: "p1")],
        mates: [], ground: "i1"),
    dims: [
        Dim(id: "d1", from: "i1.bottom", to: "i1.top", nominal: 4.0, tol: Sym(0.1), rationale: "r0"),
        Dim(id: "d2", from: "i1.top", to: "i2.bottom", nominal: 2.0, tol: Sym(0.1), rationale: "r0"),
        Dim(id: "d3", from: "i2.bottom", to: "i2.top", nominal: 4.0, tol: Sym(0.1), rationale: "r0"),
    ],
    assertions: [
        Assertion(id: "a_stack",
            check: ToleranceStack1D(path: [{path}], target: (9.0, 11.0), method: WorstCase),
            rationale: "r0"),
    ],
    rationales: [Rationale(id: "r0", author: Human("t"), basis: Assumption, note: "", timestamp: "2026-07-12T00:00:00Z")],
)"#
    )
}

#[test]
fn connected_path_passes_validation() {
    validate_design(&src(r#""d1", "d2", "d3""#)).expect("連結経路は妥当");
}

#[test]
fn disconnected_path_is_static_error() {
    // d1.to = i1.top だが d3.from = i2.bottom → 非連結
    let errs = validate_design(&src(r#""d1", "d3""#)).unwrap_err();
    assert!(
        errs.iter().any(|e| e.code == ErrorCode::SchemaRef && e.message.contains("連結")),
        "{errs:#?}"
    );
}
