//! # adc-check — 検証ハーネス (ADR-003, 05-schema.md §6)
//!
//! - 全チェッカーは共通トレイトを実装する**純関数**(グローバル状態・乱数・時刻に非依存)
//! - 判定は3値: Pass / Fail / Inconclusive(チェック不能はFailと厳密に区別)
//! - Passでもmarginを必ず返す。marginの定義はチェッカーごとに docs/checkers.md に文書化
//! - 出力は results.jsonl(assert_id昇順、浮動小数は1e-9量子化 → 同一入力でバイト再現)。
//!   **時間情報(cost_ms等)は正準出力に含めない**(2026-07-12決定)。
//!   タイミングは `run_checks_with_timings` が別データとして返し、CLIが `--timings`
//!   指定時のみstderrへ出す
//! - インスタンスはM3(mate解決)まで**恒等配置**(docs/checkers.md)

pub mod checkers;

use std::collections::BTreeMap;

use adc_compile::{compile_part, CompiledPart};
use adc_schema::{Assertion, Check, Design, EvalContext, Evaluator};
use serde::{Deserialize, Serialize};

/// 正準出力の浮動小数量子化(1e-9 — 05-schema.md §6)
pub fn q(v: f64) -> f64 {
    (v * 1e9).round() / 1e9
}

pub fn q3(p: [f64; 3]) -> [f64; 3] {
    [q(p[0]), q(p[1]), q(p[2])]
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Pass,
    Fail,
    Inconclusive { reason: String },
}

/// 測定値・しきい値(スカラー / 3成分 / なし)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Value {
    Scalar(f64),
    Triple([f64; 3]),
    None,
}

/// 修復可能な粒度のEvidence (US-16): アンカー参照+座標+補足
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Evidence {
    pub anchors: Vec<String>,
    pub points: Vec<[f64; 3]>,
    pub note: String,
}

/// チェック結果 (05-schema.md §6。cost_msは正準出力から除外 — 2026-07-12決定)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CheckResult {
    pub assert_id: String,
    pub checker: String,
    pub status: CheckStatus,
    pub measured: Value,
    pub threshold: Value,
    pub margin: f64,
    pub evidence: Vec<Evidence>,
}

impl CheckResult {
    pub fn inconclusive(
        assert_id: impl Into<String>,
        checker: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        CheckResult {
            assert_id: assert_id.into(),
            checker: checker.into(),
            status: CheckStatus::Inconclusive {
                reason: reason.into(),
            },
            measured: Value::None,
            threshold: Value::None,
            margin: 0.0,
            evidence: vec![],
        }
    }
}

/// コンパイル済みモデル。インスタンスはM3(mate解決)まで恒等配置。
pub struct CompiledModel {
    pub parts: BTreeMap<String, CompiledPart>,
    /// コンパイルに失敗した部品(→ 当該部品のチェックはInconclusive)
    pub part_errors: BTreeMap<String, String>,
    /// (instance_id, part_id)
    pub instances: Vec<(String, String)>,
}

pub fn compile_model(design: &Design, ctx: &EvalContext) -> CompiledModel {
    let mut parts = BTreeMap::new();
    let mut part_errors = BTreeMap::new();
    for p in &design.parts {
        match compile_part(design, &p.id, ctx) {
            Ok(cp) => {
                parts.insert(p.id.clone(), cp);
            }
            Err(e) => {
                part_errors.insert(p.id.clone(), e.to_string());
            }
        }
    }
    let instances = design
        .assembly
        .iter()
        .flat_map(|a| a.instances.iter())
        .map(|i| (i.id.clone(), i.part.clone()))
        .collect();
    CompiledModel {
        parts,
        part_errors,
        instances,
    }
}

/// チェッカー契約 (05-schema.md §6): 純関数。並列実行可能。
pub trait Checker {
    fn id(&self) -> &'static str;
    fn check(&self, model: &CompiledModel, ev: &Evaluator, a: &Assertion) -> CheckResult;
}

fn checker_for(check: &Check) -> Option<&'static dyn Checker> {
    match check {
        Check::BoundingBox { .. } => Some(&checkers::BoundingBoxChecker),
        Check::Clearance { .. } => Some(&checkers::ClearanceChecker),
        Check::NoInterference { .. } => Some(&checkers::NoInterferenceChecker),
        _ => None,
    }
}

/// 全アサーションを検証する(結果はassert_id昇順)
pub fn run_checks(design: &Design, ctx: &EvalContext) -> Vec<CheckResult> {
    run_checks_with_timings(design, ctx).0
}

/// タイミング付き実行。タイミングは正準出力ではない(--timings用、ミリ秒)。
/// 計測はチェッカーの外側で行う(チェッカー自体は時刻に依存しない — ADR-003)。
pub fn run_checks_with_timings(
    design: &Design,
    ctx: &EvalContext,
) -> (Vec<CheckResult>, Vec<(String, f64)>) {
    let model = compile_model(design, ctx);
    let ev = match Evaluator::new(design, ctx) {
        Ok(ev) => ev,
        Err(e) => {
            let mut rs: Vec<CheckResult> = design
                .assertions
                .iter()
                .map(|a| {
                    CheckResult::inconclusive(&a.id, "evaluator", format!("パラメータ評価に失敗: {e}"))
                })
                .collect();
            rs.sort_by(|x, y| x.assert_id.cmp(&y.assert_id));
            return (rs, vec![]);
        }
    };
    let mut results = Vec::new();
    let mut timings = Vec::new();
    for a in &design.assertions {
        let t0 = std::time::Instant::now();
        let r = match checker_for(&a.check) {
            Some(c) => c.check(&model, &ev, a),
            None => CheckResult::inconclusive(
                &a.id,
                "unimplemented",
                "チェッカー未実装(M2後続ユニット/T2以降)",
            ),
        };
        timings.push((a.id.clone(), t0.elapsed().as_secs_f64() * 1000.0));
        results.push(r);
    }
    results.sort_by(|x, y| x.assert_id.cmp(&y.assert_id));
    timings.sort_by(|x, y| x.0.cmp(&y.0));
    (results, timings)
}

/// results.jsonl の正準テキスト(1行1結果、決定的)
pub fn to_jsonl(results: &[CheckResult]) -> String {
    let mut out = String::new();
    for r in results {
        out.push_str(&serde_json::to_string(r).expect("CheckResultのシリアライズ"));
        out.push('\n');
    }
    out
}

/// exit code (07-cli.md): 0=全Pass / 1=Fail≥1 / 2=Fail=0かつInconclusive≥1
pub fn exit_code(results: &[CheckResult]) -> u8 {
    if results
        .iter()
        .any(|r| matches!(r.status, CheckStatus::Fail))
    {
        1
    } else if results
        .iter()
        .any(|r| matches!(r.status, CheckStatus::Inconclusive { .. }))
    {
        2
    } else {
        0
    }
}
