//! M1-6 受入テスト (US-07): STEPエクスポート(既定スキーマAP214 — M1-6緩和)。
//!
//! プロセス内ゴールデン: §9ブラケットを export → re-import して体積が一致すること。
//! 外部ビューア(FreeCADヘッドレス)での開閲+体積一致は
//! scripts/golden-step-check.sh(コンテナ内、OCCT 7.8.1)が担う。

use adc_compile::compile_part;
use adc_kernel::Solid;
use adc_schema::{validate_design, EvalContext};

fn compile_bracket() -> adc_compile::CompiledPart {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/motor_bracket/design.ron"
    );
    let src = std::fs::read_to_string(path).expect("サンプル読み込み");
    let design = validate_design(&src).expect("検証");
    compile_part(&design, "bracket", &EvalContext::nominal()).expect("コンパイル")
}

#[test]
fn step_export_roundtrips_volume() {
    let cp = compile_bracket();
    let v1 = cp.solid.volume();
    // 妥当性: 板19200 − ボアπ·27.5²·4(≈9503) − ボルト穴φ6.6×4(≈547) − 外周フィレット(≈240) ≈ 8910
    assert!(v1 > 8000.0 && v1 < 10000.0, "{v1}");

    let dir = std::env::temp_dir().join("adc-m1-6");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("bracket.step");
    let path_str = path.to_str().unwrap();
    cp.solid.write_step(path_str).expect("STEP出力");

    // STEPファイルの体裁
    let content = std::fs::read_to_string(&path).expect("出力ファイル");
    assert!(content.starts_with("ISO-10303-21"), "STEPヘッダ");

    // re-import して体積一致(プロセス内ゴールデン)
    let back = Solid::read_step(path_str).expect("STEP再読込");
    let v2 = back.volume();
    assert!(
        ((v1 - v2) / v1).abs() < 1e-9,
        "体積往復一致: v1={v1}, v2={v2}"
    );
}

#[test]
fn export_is_deterministic_in_volume() {
    let v1 = compile_bracket().solid.volume();
    let v2 = compile_bracket().solid.volume();
    assert_eq!(v1, v2, "体積がビット同一で再現すること");
}
