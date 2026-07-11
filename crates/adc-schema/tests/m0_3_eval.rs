//! M0-3 受入テスト: Expr評価器 (US-01, US-25 / 05-schema.md §2.1)。
//!
//! - 導出チェーン(b = a*3, c = b+1)の位相順解決
//! - ゼロ除算は構造化エラー E-SCHEMA-EVAL
//! - 実効Open判定: 基底Open a に推移的に依存する c は実効的にOpen
//! - 3点評価の標本軸は基底Openパラメータのみ(導出値は軸にしない)

use adc_schema::*;

fn design_of(params: &str) -> Design {
    let src = format!(
        r#"Design(
    schema_version: "0.1",
    intent: "m0-3 fixture",
    params: [{params}],
    materials: [Material(id: "a5052", density_g_cm3: 2.68, name: "A5052")],
    parts: [],
    assertions: [],
    rationales: [Rationale(id: "r0", author: Human("t"), basis: Assumption, note: "", timestamp: "2026-07-11T00:00:00Z")],
)"#
    );
    validate_design(&src).unwrap_or_else(|e| panic!("フィクスチャは検証を通ること: {e:#?}"))
}

fn derived_chain() -> Design {
    design_of(
        r#"Param(id: "a", value: Determined(2.0), unit: Mm, rationale: "r0"),
           Param(id: "b", value: Determined(param("a") * 3.0), unit: Mm, rationale: "r0"),
           Param(id: "c", value: Determined(param("b") + 1.0), unit: Mm, rationale: "r0")"#,
    )
}

fn open_chain() -> Design {
    design_of(
        r#"Param(id: "a", value: Open(range: (3.0, 6.0), nominal: 4.0), unit: Mm, rationale: "r0"),
           Param(id: "b", value: Determined(param("a") * 3.0), unit: Mm, rationale: "r0"),
           Param(id: "c", value: Determined(param("b") + 1.0), unit: Mm, rationale: "r0"),
           Param(id: "d", value: Determined(5.0), unit: Mm, rationale: "r0")"#,
    )
}

#[test]
fn derived_chain_resolves_topologically() {
    let d = derived_chain();
    let ev = Evaluator::new(&d, &EvalContext::nominal()).expect("評価器の構築");
    assert_eq!(ev.param("a"), Some(2.0));
    assert_eq!(ev.param("b"), Some(6.0));
    assert_eq!(ev.param("c"), Some(7.0));
    // 式の評価
    let e = Expr::Add(
        Box::new(Expr::Param("c".to_string())),
        Box::new(Expr::Lit(1.0)),
    );
    assert_eq!(ev.evaluate(&e).unwrap(), 8.0);
}

#[test]
fn zero_division_in_expr_is_structured_eval_error() {
    let d = derived_chain();
    let ev = Evaluator::new(&d, &EvalContext::nominal()).unwrap();
    let e = Expr::Div(Box::new(Expr::Lit(1.0)), Box::new(Expr::Lit(0.0)));
    let err = ev.evaluate(&e).unwrap_err();
    assert_eq!(err.code, ErrorCode::SchemaEval);
    assert_eq!(err.code.as_str(), "E-SCHEMA-EVAL");
}

#[test]
fn zero_division_in_derived_param_fails_construction() {
    // b = 1 / (a - 2) で a=2 → 導出解決時にゼロ除算
    let d = design_of(
        r#"Param(id: "a", value: Determined(2.0), unit: Mm, rationale: "r0"),
           Param(id: "b", value: Determined(1.0 / (param("a") - 2.0)), unit: Mm, rationale: "r0")"#,
    );
    let err = Evaluator::new(&d, &EvalContext::nominal()).unwrap_err();
    assert_eq!(err.code, ErrorCode::SchemaEval);
    assert!(
        err.related.iter().any(|r| r == "b"),
        "relatedに解決中のparamを含むこと: {err:#?}"
    );
}

#[test]
fn effective_open_is_transitive() {
    let d = open_chain();
    let ev = Evaluator::new(&d, &EvalContext::nominal()).unwrap();

    // c = b+1 = a*3+1 は基底Open a に推移的に依存 → 実効的にOpen
    let c = Expr::Param("c".to_string());
    assert!(ev.is_effectively_open(&c));
    assert_eq!(
        ev.open_deps_of(&c).into_iter().collect::<Vec<_>>(),
        vec!["a".to_string()]
    );

    // d = 5.0 は依存なし → Openでない
    let dd = Expr::Param("d".to_string());
    assert!(!ev.is_effectively_open(&dd));
    assert!(ev.open_deps_of(&dd).is_empty());

    // リテラルもOpenでない
    assert!(!ev.is_effectively_open(&Expr::Lit(1.0)));
}

#[test]
fn sample_axes_are_base_open_params_only() {
    // 3点評価の標本軸は基底Openのみ(導出c は実効的にOpenだが軸ではない)
    let d = open_chain();
    let ev = Evaluator::new(&d, &EvalContext::nominal()).unwrap();
    assert_eq!(ev.base_open_params(), vec!["a".to_string()]);
}

#[test]
fn eval_context_assigns_base_open_values() {
    let d = open_chain();
    // 未割当 → nominal (a=4) で b=12
    let ev = Evaluator::new(&d, &EvalContext::nominal()).unwrap();
    assert_eq!(ev.param("b"), Some(12.0));
    // 区間上端 a=6 → b=18
    let ev = Evaluator::new(&d, &EvalContext::nominal().assign("a", 6.0)).unwrap();
    assert_eq!(ev.param("b"), Some(18.0));
}

#[test]
fn assignment_to_non_open_param_is_error() {
    let d = open_chain();
    let err = Evaluator::new(&d, &EvalContext::nominal().assign("d", 9.0)).unwrap_err();
    assert_eq!(err.code, ErrorCode::SchemaEval);
    let err = Evaluator::new(&d, &EvalContext::nominal().assign("ghost", 1.0)).unwrap_err();
    assert_eq!(err.code, ErrorCode::SchemaEval);
}

#[test]
fn unknown_param_in_expr_is_ref_error() {
    let d = derived_chain();
    let ev = Evaluator::new(&d, &EvalContext::nominal()).unwrap();
    let err = ev.evaluate(&Expr::Param("ghost".to_string())).unwrap_err();
    assert_eq!(err.code, ErrorCode::SchemaRef);
}
