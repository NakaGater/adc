//! M1-7 受入テスト (US-08): E-FEATURE-FAILの全面展開。
//! 各操作カテゴリで人工的に失敗を起こし、abortゼロ+構造化エラーを確認する。
//! (フィレット過大はm1_3、ブーリアンTryNewは防御設置 — docs/occt-gotchas.md参照)

use adc_compile::{compile_part, CompileError};
use adc_schema::{validate_design, EvalContext};

fn compile(features: &str) -> Result<adc_compile::CompiledPart, CompileError> {
    let src = format!(
        r#"Design(
    schema_version: "0.1",
    intent: "m1-7 fixture",
    params: [],
    materials: [Material(id: "a5052", density_g_cm3: 2.68, name: "A5052")],
    parts: [
        Part(id: "p1", material: "a5052", process: Machining,
            features: [{features}], anchors: []),
    ],
    assertions: [],
    rationales: [Rationale(id: "r0", author: Human("t"), basis: Assumption, note: "", timestamp: "2026-07-12T00:00:00Z")],
)"#
    );
    let design = validate_design(&src).unwrap_or_else(|e| panic!("静的検証: {e:#?}"));
    compile_part(&design, "p1", &EvalContext::nominal())
}

fn expect_feature_fail(features: &str, fid: &str, hint_contains: &str) {
    match compile(features) {
        Err(CompileError::FeatureFail(e)) => {
            assert_eq!(e.feature_id, fid);
            assert!(
                e.hint.as_deref().unwrap_or("").contains(hint_contains),
                "hint: {e:?}"
            );
            serde_json::to_string(&e).expect("JSONシリアライズ可能");
        }
        Ok(_) => panic!("失敗するはず: {features}"),
        Err(other) => panic!("FeatureFailのはず: {other}"),
    }
}

const BLOCK: &str = r#"Block(id: "base", x: 80.0, y: 60.0, z: 4.0)"#;

// ---- プリミティブ/工具寸法(OCCTのDomainErrorをFFI前に決定的に検出)

#[test]
fn zero_diameter_hole_is_structured_error() {
    expect_feature_fail(
        &format!(
            r#"{BLOCK},
            Hole(id: "h1", kind: Simple, d: 0.0, depth: Through,
                 at: on(feature("base").face("top"), center()))"#
        ),
        "h1",
        "正の値",
    );
}

#[test]
fn negative_block_dimension_is_structured_error() {
    expect_feature_fail(
        r#"Block(id: "base", x: 80.0, y: -60.0, z: 4.0)"#,
        "base",
        "正の値",
    );
}

#[test]
fn zero_pocket_depth_is_structured_error() {
    expect_feature_fail(
        &format!(
            r#"{BLOCK},
            Pocket(id: "pk", profile: Rect(x: 10.0, y: 10.0), depth: 0.0,
                   at: on(feature("base").face("top"), center()))"#
        ),
        "pk",
        "正の値",
    );
}

// ---- 面取り過大(フィレット過大はm1_3で固定済み)

#[test]
fn oversized_chamfer_is_structured_error_without_abort() {
    expect_feature_fail(
        &format!(
            r#"{BLOCK},
            Chamfer(id: "c1", edges: edges_between(feature("base").face("top"), feature("base").face("+x")), size: 10.0)"#
        ),
        "c1",
        "過大",
    );
}

// ---- STEP I/O(kernelレベル)

#[test]
fn step_write_to_invalid_path_is_structured_error() {
    let solid = adc_kernel::make_box(1.0, 1.0, 1.0);
    let Err(err) = solid.write_step("/nonexistent-dir-adc/x.step") else {
        panic!("失敗するはず");
    };
    assert!(!err.is_empty(), "構造化メッセージ: {err}");
}

#[test]
fn step_read_of_corrupt_file_is_structured_error() {
    let dir = std::env::temp_dir().join("adc-m1-7");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("corrupt.step");
    std::fs::write(&path, "this is not a step file").unwrap();
    let Err(err) = adc_kernel::Solid::read_step(path.to_str().unwrap()) else {
        panic!("失敗するはず");
    };
    assert!(!err.is_empty(), "{err}");
}
