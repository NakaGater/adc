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

use std::path::PathBuf;

use adc_compile::{compile_part, compile_part_cached, part_cache_key, CacheOutcome, CompiledPart};
use adc_schema::{Assertion, Check, Design, EvalContext, Evaluator, GeomRef, Scope};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

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
    /// part id → 材料密度 g/cm³(材料未定義はNone → Inconclusive)
    pub part_density: BTreeMap<String, Option<f64>>,
    /// part id → Datumアンカーidの列(宣言順)
    pub part_datums: BTreeMap<String, Vec<String>>,
}

/// キャッシュ設定 (M2-6)。cache_dir=Noneでキャッシュ無効(--no-cache)。
#[derive(Debug, Clone, Default)]
pub struct CheckOptions {
    pub cache_dir: Option<PathBuf>,
}

/// キャッシュイベント(ログ・テスト検証用。正準出力ではない)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CacheEvent {
    PartHit(String),
    PartCompiled(String),
    ResultHit(String),
    ResultComputed(String),
}

pub fn compile_model(design: &Design, ctx: &EvalContext) -> CompiledModel {
    compile_model_with(design, ctx, &CheckOptions::default()).0
}

pub fn compile_model_with(
    design: &Design,
    ctx: &EvalContext,
    opts: &CheckOptions,
) -> (CompiledModel, Vec<CacheEvent>) {
    let mut events = Vec::new();
    let mut parts = BTreeMap::new();
    let mut part_errors = BTreeMap::new();
    for p in &design.parts {
        let outcome = match &opts.cache_dir {
            Some(dir) => compile_part_cached(design, &p.id, ctx, dir)
                .map(|(cp, o)| (cp, o == CacheOutcome::Hit)),
            None => compile_part(design, &p.id, ctx).map(|cp| (cp, false)),
        };
        match outcome {
            Ok((cp, hit)) => {
                events.push(if hit {
                    CacheEvent::PartHit(p.id.clone())
                } else {
                    CacheEvent::PartCompiled(p.id.clone())
                });
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
    let materials: BTreeMap<&str, f64> = design
        .materials
        .iter()
        .map(|m| (m.id.as_str(), m.density_g_cm3))
        .collect();
    let mut part_density = BTreeMap::new();
    let mut part_datums = BTreeMap::new();
    for p in &design.parts {
        part_density.insert(p.id.clone(), materials.get(p.material.as_str()).copied());
        part_datums.insert(
            p.id.clone(),
            p.anchors
                .iter()
                .filter(|an| matches!(an.kind, adc_schema::AnchorKind::Datum(_)))
                .map(|an| an.id.clone())
                .collect(),
        );
    }
    (
        CompiledModel {
            parts,
            part_errors,
            instances,
            part_density,
            part_datums,
        },
        events,
    )
}

/// アサーションが依存する部品集合(結果キャッシュキー用)。
/// 解決できない参照を含む場合はNone(=キャッシュしない)。
fn involved_parts(check: &Check, model: &CompiledModel) -> Option<Vec<String>> {
    let inst_part = |iid: &str| -> Option<String> {
        model
            .instances
            .iter()
            .find(|(i, _)| i == iid)
            .map(|(_, p)| p.clone())
    };
    let geom_part = |g: &GeomRef| -> Option<String> {
        match g {
            GeomRef::Part(p) => Some(p.clone()),
            GeomRef::Anchor(path) => inst_part(&path.instance),
        }
    };
    let mut v: Vec<String> = match check {
        Check::BoundingBox { part, .. }
        | Check::Mass { part, .. }
        | Check::WallThickness { part, .. }
        | Check::DatumValidity { part }
        | Check::SheetMetalRules { part } => vec![part.clone()],
        Check::Clearance { a, b, .. } => vec![geom_part(a)?, geom_part(b)?],
        Check::NoInterference { scope } => match scope {
            Scope::All => model.instances.iter().map(|(_, p)| p.clone()).collect(),
            Scope::Pairs(ps) => ps.iter().flat_map(|(a, b)| [a.clone(), b.clone()]).collect(),
        },
        Check::Cog { .. } => {
            if model.instances.is_empty() {
                model.parts.keys().cloned().collect()
            } else {
                model.instances.iter().map(|(_, p)| p.clone()).collect()
            }
        }
        _ => return None,
    };
    v.sort();
    v.dedup();
    Some(v)
}

/// 結果キャッシュキー: hash(ADCバージョン + Assertion正準形 + 依存部品キー列)
/// Checker設定(sample_density等)はAssertion正準形に含まれる (ADR-003)
fn result_cache_key(
    design: &Design,
    a: &Assertion,
    parts: &[String],
    ev: &Evaluator,
) -> Option<String> {
    let a_ron = ron::ser::to_string(a).ok()?;
    let mut src = format!("adcres:{}|{}", env!("CARGO_PKG_VERSION"), a_ron);
    for p in parts {
        let key = part_cache_key(design, p, ev).ok()?;
        src.push_str(&format!("|part:{p}={key}"));
    }
    let mut h = Sha256::new();
    h.update(src.as_bytes());
    Some(format!("{:x}", h.finalize()))
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
        Check::Mass { .. } => Some(&checkers::MassChecker),
        Check::Cog { .. } => Some(&checkers::CogChecker),
        Check::WallThickness { .. } => Some(&checkers::WallThicknessChecker),
        Check::DatumValidity { .. } => Some(&checkers::DatumValidityChecker),
        _ => None,
    }
}

/// 全アサーションを検証する(結果はassert_id昇順)
pub fn run_checks(design: &Design, ctx: &EvalContext) -> Vec<CheckResult> {
    run_checks_with_timings(design, ctx).0
}

/// タイミング付き実行。タイミングは正準出力ではない(--timings用、ミリ秒)。
pub fn run_checks_with_timings(
    design: &Design,
    ctx: &EvalContext,
) -> (Vec<CheckResult>, Vec<(String, f64)>) {
    let (r, t, _) = run_checks_full(design, ctx, &CheckOptions::default());
    (r, t)
}

/// キャッシュ・タイミング・イベント付きのフル実行 (M2-6)。
/// 計測はチェッカーの外側で行う(チェッカー自体は時刻に依存しない — ADR-003)。
pub fn run_checks_full(
    design: &Design,
    ctx: &EvalContext,
    opts: &CheckOptions,
) -> (Vec<CheckResult>, Vec<(String, f64)>, Vec<CacheEvent>) {
    let (model, mut events) = compile_model_with(design, ctx, opts);
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
            return (rs, vec![], events);
        }
    };
    let mut results = Vec::new();
    let mut timings = Vec::new();
    for a in &design.assertions {
        let t0 = std::time::Instant::now();

        // 結果キャッシュ (M2-6): 依存部品が全てコンパイル済みのときのみ
        let rkey = opts.cache_dir.as_ref().and_then(|_| {
            let parts = involved_parts(&a.check, &model)?;
            if parts.iter().any(|p| !model.parts.contains_key(p)) {
                return None;
            }
            result_cache_key(design, a, &parts, &ev)
        });
        let rpath = match (&opts.cache_dir, &rkey) {
            (Some(dir), Some(k)) => Some(dir.join(format!("{k}.result.json"))),
            _ => None,
        };

        let cached: Option<CheckResult> = rpath.as_ref().and_then(|p| {
            let text = std::fs::read_to_string(p).ok()?;
            serde_json::from_str(&text).ok()
        });
        let r = match cached {
            Some(r) => {
                events.push(CacheEvent::ResultHit(a.id.clone()));
                r
            }
            None => {
                let r = match checker_for(&a.check) {
                    Some(c) => c.check(&model, &ev, a),
                    None => CheckResult::inconclusive(
                        &a.id,
                        "unimplemented",
                        "チェッカー未実装(M2後続ユニット/T2以降)",
                    ),
                };
                if let Some(p) = &rpath {
                    if !matches!(r.status, CheckStatus::Inconclusive { .. }) {
                        let _ = std::fs::create_dir_all(p.parent().unwrap());
                        let _ = std::fs::write(p, serde_json::to_string(&r).unwrap());
                    }
                }
                events.push(CacheEvent::ResultComputed(a.id.clone()));
                r
            }
        };
        timings.push((a.id.clone(), t0.elapsed().as_secs_f64() * 1000.0));
        results.push(r);
    }
    results.sort_by(|x, y| x.assert_id.cmp(&y.assert_id));
    timings.sort_by(|x, y| x.0.cmp(&y.0));
    (results, timings, events)
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
