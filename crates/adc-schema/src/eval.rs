//! M0-3 Expr評価器 (05-schema.md §2.1)。
//!
//! - `EvalContext` = 基底Openパラメータへの数値割当(未割当は公称値)
//! - 導出パラメータ(`Determined(Expr)`)は循環なし保証の下で位相順(メモ化再帰)に解決
//! - 実効的にOpen ⇔ 基底Openパラメータに推移的に依存
//! - 3点評価(ADR-004)の標本軸は基底Openパラメータのみ(`base_open_params`)
//! - 評価失敗(ゼロ除算・非有限値)は E-SCHEMA-EVAL。チェッカー文脈ではInconclusive相当

use std::collections::{BTreeSet, HashMap, HashSet};

use crate::error::{ErrorCode, ValidationError};
use crate::{Design, Expr, ParamValue};

/// 基底Openパラメータへの数値割当 (05-schema.md §2.1)
#[derive(Debug, Clone, Default)]
pub struct EvalContext {
    assignments: HashMap<String, f64>,
}

impl EvalContext {
    /// 全ての基底Openを公称値(nominal)で評価するコンテキスト
    pub fn nominal() -> Self {
        Self::default()
    }

    /// 基底Openパラメータに値を割り当てる(3点評価の区間端など)
    pub fn assign(mut self, param: impl Into<String>, value: f64) -> Self {
        self.assignments.insert(param.into(), value);
        self
    }
}

fn eval_err(message: String, related: Vec<String>) -> ValidationError {
    ValidationError {
        code: ErrorCode::SchemaEval,
        message,
        span: None,
        related,
    }
}

/// 位相順に解決済みのパラメータ環境。式の評価と実効Open判定を提供する。
#[derive(Debug)]
pub struct Evaluator {
    values: HashMap<String, f64>,
    /// param → 推移的に依存する基底Openパラメータ集合
    open_deps: HashMap<String, BTreeSet<String>>,
    base_open: Vec<String>,
}

impl Evaluator {
    /// 全パラメータを位相順に解決して評価器を構築する。
    /// 検証済みDesignを前提とするが、循環・ゼロ除算等は構造化エラーで返す。
    pub fn new(design: &Design, ctx: &EvalContext) -> Result<Evaluator, ValidationError> {
        let defs: HashMap<&str, &ParamValue> = design
            .params
            .iter()
            .map(|p| (p.id.as_str(), &p.value))
            .collect();

        // 割当キーの検証: 存在し、かつ基底Openであること
        for key in ctx.assignments.keys() {
            match defs.get(key.as_str()) {
                None => {
                    return Err(eval_err(
                        format!("EvalContextの割当先 \"{key}\" は未定義のパラメータです"),
                        vec![key.clone()],
                    ))
                }
                Some(ParamValue::Determined(_)) => {
                    return Err(eval_err(
                        format!(
                            "EvalContextの割当先 \"{key}\" は基底Openパラメータではありません (05-schema.md §2.1)"
                        ),
                        vec![key.clone()],
                    ))
                }
                Some(ParamValue::Open { .. }) => {}
            }
        }

        let mut ev = Evaluator {
            values: HashMap::new(),
            open_deps: HashMap::new(),
            base_open: Vec::new(),
        };
        let mut visiting: HashSet<String> = HashSet::new();
        for p in &design.params {
            ev.resolve(&p.id, &defs, ctx, &mut visiting)?;
        }
        ev.base_open = design
            .params
            .iter()
            .filter(|p| matches!(p.value, ParamValue::Open { .. }))
            .map(|p| p.id.clone())
            .collect();
        Ok(ev)
    }

    fn resolve(
        &mut self,
        id: &str,
        defs: &HashMap<&str, &ParamValue>,
        ctx: &EvalContext,
        visiting: &mut HashSet<String>,
    ) -> Result<f64, ValidationError> {
        if let Some(v) = self.values.get(id) {
            return Ok(*v);
        }
        let Some(def) = defs.get(id) else {
            return Err(ValidationError {
                code: ErrorCode::SchemaRef,
                message: format!("未定義のパラメータ \"{id}\" を評価しようとしました"),
                span: None,
                related: vec![id.to_string()],
            });
        };
        if !visiting.insert(id.to_string()) {
            return Err(ValidationError {
                code: ErrorCode::SchemaCycle,
                message: format!("パラメータ \"{id}\" の解決中に循環を検出しました"),
                span: None,
                related: vec![id.to_string()],
            });
        }
        let (value, deps) = match def {
            ParamValue::Open { nominal, .. } => {
                let v = ctx.assignments.get(id).copied().unwrap_or(*nominal);
                (v, BTreeSet::from([id.to_string()]))
            }
            ParamValue::Determined(e) => {
                let (v, deps) = self
                    .eval_resolving(e, defs, ctx, visiting)
                    .map_err(|mut err| {
                        if !err.related.iter().any(|r| r == id) {
                            err.related.push(id.to_string());
                        }
                        err
                    })?;
                (v, deps)
            }
        };
        visiting.remove(id);
        self.values.insert(id.to_string(), value);
        self.open_deps.insert(id.to_string(), deps);
        Ok(value)
    }

    /// 解決中(paramを再帰的に辿りながら)の式評価
    fn eval_resolving(
        &mut self,
        e: &Expr,
        defs: &HashMap<&str, &ParamValue>,
        ctx: &EvalContext,
        visiting: &mut HashSet<String>,
    ) -> Result<(f64, BTreeSet<String>), ValidationError> {
        match e {
            Expr::Lit(v) => Ok((*v, BTreeSet::new())),
            Expr::Param(id) => {
                let v = self.resolve(id, defs, ctx, visiting)?;
                let deps = self.open_deps.get(id).cloned().unwrap_or_default();
                Ok((v, deps))
            }
            Expr::Add(a, b) | Expr::Sub(a, b) | Expr::Mul(a, b) | Expr::Div(a, b) => {
                let (va, mut da) = self.eval_resolving(a, defs, ctx, visiting)?;
                let (vb, db) = self.eval_resolving(b, defs, ctx, visiting)?;
                da.extend(db);
                let v = apply(e, va, vb)?;
                Ok((v, da))
            }
        }
    }

    /// 解決済みのパラメータ値
    pub fn param(&self, id: &str) -> Option<f64> {
        self.values.get(id).copied()
    }

    /// 式を評価する。失敗(ゼロ除算・非有限値・未定義param)は構造化エラー。
    pub fn evaluate(&self, e: &Expr) -> Result<f64, ValidationError> {
        match e {
            Expr::Lit(v) => Ok(*v),
            Expr::Param(id) => self.param(id).ok_or_else(|| ValidationError {
                code: ErrorCode::SchemaRef,
                message: format!("未定義のパラメータ \"{id}\" を評価しようとしました"),
                span: None,
                related: vec![id.to_string()],
            }),
            Expr::Add(a, b) | Expr::Sub(a, b) | Expr::Mul(a, b) | Expr::Div(a, b) => {
                let va = self.evaluate(a)?;
                let vb = self.evaluate(b)?;
                apply(e, va, vb)
            }
        }
    }

    /// 式が推移的に依存する基底Openパラメータの集合 (05-schema.md §2.1)
    pub fn open_deps_of(&self, e: &Expr) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        self.collect_open_deps(e, &mut out);
        out
    }

    fn collect_open_deps(&self, e: &Expr, out: &mut BTreeSet<String>) {
        match e {
            Expr::Lit(_) => {}
            Expr::Param(id) => {
                if let Some(deps) = self.open_deps.get(id) {
                    out.extend(deps.iter().cloned());
                }
            }
            Expr::Add(a, b) | Expr::Sub(a, b) | Expr::Mul(a, b) | Expr::Div(a, b) => {
                self.collect_open_deps(a, out);
                self.collect_open_deps(b, out);
            }
        }
    }

    /// 実効的にOpen ⇔ 基底Openパラメータに推移的に依存する
    pub fn is_effectively_open(&self, e: &Expr) -> bool {
        !self.open_deps_of(e).is_empty()
    }

    /// 3点評価(ADR-004)の標本軸 = 基底Openパラメータのみ(宣言順)
    pub fn base_open_params(&self) -> Vec<String> {
        self.base_open.clone()
    }
}

fn apply(e: &Expr, a: f64, b: f64) -> Result<f64, ValidationError> {
    let v = match e {
        Expr::Add(..) => a + b,
        Expr::Sub(..) => a - b,
        Expr::Mul(..) => a * b,
        Expr::Div(..) => {
            if b == 0.0 {
                return Err(eval_err(
                    format!(
                        "ゼロ除算: \"{}\" の除数が0になりました",
                        crate::desugar::expr_dsl(e)
                    ),
                    vec![],
                ));
            }
            a / b
        }
        _ => unreachable!("applyは二項演算のみ"),
    };
    if !v.is_finite() {
        return Err(eval_err(
            format!(
                "式 \"{}\" の評価結果が非有限値です",
                crate::desugar::expr_dsl(e)
            ),
            vec![],
        ));
    }
    Ok(v)
}
