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

use adc_compile::{collect_param_ids, compile_part, compile_part_cached, part_cache_key, CacheOutcome, CompiledPart};
use adc_schema::{Assertion, Check, Design, EvalContext, Evaluator, GeomRef, ParamValue, Scope};
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
    /// Open 3点評価の標本別サブ結果 (05-schema.md §6.1, M4-1)。
    /// 基底Openパラメータの宣言順 × 各軸内 lo→nominal→hi。
    /// Openなし設計ではフィールド自体を出力しない(M2-1出力とバイト互換)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub samples: Vec<SampleResult>,
}

/// 3点評価の標本別サブ結果 {param, sample(lo/nominal/hi), status, measured}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SampleResult {
    pub param: String,
    pub sample: String,
    pub status: CheckStatus,
    pub measured: Value,
}

impl CheckResult {
    pub fn inconclusive(
        assert_id: impl Into<String>,
        checker: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        CheckResult {
            samples: Vec::new(),
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
    /// instance id → mate解決済み剛体変換 (M3。mateなしは恒等)
    pub placements: BTreeMap<String, adc_compile::assembly::Rigid>,
    /// instance id → 配置済みソリッド
    pub instance_solids: BTreeMap<String, adc_kernel::Solid>,
    /// mate解決失敗 (E-MATE-UNSOLVED) — Assy依存チェックはInconclusiveへ
    pub assembly_error: Option<String>,
    /// 残自由度レポート (instance, 残DOF, note) — 報告のみ(M3-2)
    pub dof_report: Vec<(String, u8, String)>,
    /// 寸法定義 (§7) — ToleranceStack1D(M5-3)は代数計算のみでここを読む
    pub dims: Vec<adc_schema::Dim>,
}

impl CompiledModel {
    /// アンカーの配置済みFace(束縛表インデックス→インスタンスソリッドの面)
    pub fn placed_anchor_face(
        &self,
        instance: &str,
        anchor: &str,
    ) -> Result<adc_kernel::FaceHandle, String> {
        let part = self
            .instances
            .iter()
            .find(|(i, _)| i == instance)
            .map(|(_, p)| p.clone())
            .ok_or_else(|| format!("インスタンス \"{instance}\" が存在しません"))?;
        let cp = self
            .parts
            .get(&part)
            .ok_or_else(|| format!("part \"{part}\" のコンパイルに失敗しています"))?;
        let table = cp.binding_table().map_err(|e| e.to_string())?;
        let index = match table.anchors.get(anchor) {
            Some(adc_compile::CachedBinding::Face { index }) => *index,
            Some(_) => {
                return Err(format!(
                    "アンカー \"{instance}.{anchor}\" はFace束縛ではありません"
                ))
            }
            None => return Err(format!("アンカー \"{anchor}\" が存在しません")),
        };
        let solid = self
            .instance_solids
            .get(instance)
            .ok_or_else(|| format!("インスタンス \"{instance}\" が未配置です"))?;
        solid
            .faces()
            .into_iter()
            .nth(index)
            .ok_or_else(|| "束縛表インデックスが範囲外".to_string())
    }
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
    let mut placements = BTreeMap::new();
    let mut instance_solids = BTreeMap::new();
    let mut assembly_error = None;
    let mut dof_report = Vec::new();
    if design.assembly.is_some() {
        match Evaluator::new(design, ctx) {
            Err(e) => assembly_error = Some(e.to_string()),
            Ok(ev) => match adc_compile::assembly::solve_assembly(design, &parts, &ev) {
                Err(e) => assembly_error = Some(e.to_string()),
                Ok(solved) => {
                    for si in solved.instances {
                        if let Some(cp) = parts.get(&si.part) {
                            instance_solids.insert(
                                si.instance.clone(),
                                cp.solid.transformed(si.transform.rot, si.transform.t),
                            );
                        }
                        dof_report.push((si.instance.clone(), si.remaining_dof, si.dof_note));
                        placements.insert(si.instance, si.transform);
                    }
                }
            },
        }
    }
    (
        CompiledModel {
            parts,
            part_errors,
            instances,
            part_density,
            part_datums,
            placements,
            instance_solids,
            assembly_error,
            dof_report,
            dims: design.dims.clone(),
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
    // Assertion正準形は param(<id>) を未解決のまま含むため、解決値を明示的に混ぜる
    // (Open割当が変わってもAssertion正準形が同じ、を誤ヒットさせない — M4-1)
    for id in collect_param_ids(&a_ron) {
        src.push_str(&format!("|p:{id}={:?}", ev.param(&id)));
    }
    // Assy依存チェックは配置(mate)にも依存する
    let assembly_dependent = matches!(
        &a.check,
        Check::NoInterference { .. } | Check::Cog { .. }
    ) || matches!(&a.check, Check::Clearance { a, b, .. }
        if matches!(a, GeomRef::Anchor(_)) || matches!(b, GeomRef::Anchor(_)));
    if assembly_dependent {
        if let Some(assy) = &design.assembly {
            src.push_str(&format!("|assy:{}", ron::ser::to_string(assy).ok()?));
        }
    }
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
        Check::ToleranceStack1D { .. } => Some(&checkers::ToleranceStackChecker),
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
    let (r, t, _, _) = run_checks_full(design, ctx, &CheckOptions::default());
    (r, t)
}

/// `run_checks_full` の戻り値: (結果列, タイミング, キャッシュイベント, 残自由度レポート)。
/// 残自由度は (instance_id, 残DOF数, 内訳note) — M3-2、未拘束は正常で報告のみ。
pub type FullRunOutput = (
    Vec<CheckResult>,
    Vec<(String, f64)>,
    Vec<CacheEvent>,
    Vec<(String, u8, String)>,
);

/// キャッシュ・タイミング・イベント付きのフル実行 (M2-6)。
/// 計測はチェッカーの外側で行う(チェッカー自体は時刻に依存しない — ADR-003)。
pub fn run_checks_full(design: &Design, ctx: &EvalContext, opts: &CheckOptions) -> FullRunOutput {
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
            return (rs, vec![], events, model.dof_report.clone());
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
    (results, timings, events, model.dof_report.clone())
}

/// statusの悪さ(§6.1: Fail > Inconclusive > Pass)
fn status_rank(s: &CheckStatus) -> u8 {
    match s {
        CheckStatus::Fail => 2,
        CheckStatus::Inconclusive { .. } => 1,
        CheckStatus::Pass => 0,
    }
}

/// Open 3点評価 (M4-1, ADR-004, 05-schema.md §6.1)。
///
/// 基底Openパラメータ各軸につき lo/hi(他は公称固定)+全軸共通の公称の標本で
/// 全アサーションを評価し、アサーションごとに1行へ集約する:
/// status=全標本の最悪値、samples=標本別サブ結果(宣言順×lo→nominal→hi)、
/// トップレベルのmeasured/threshold/margin/evidence=代表標本(最悪)のもの。
/// Openなし設計では公称のみの評価と完全に同一(samplesなし)。
pub fn run_checks_interval(design: &Design, opts: &CheckOptions) -> FullRunOutput {
    let (nominal, mut timings, mut events, dof) =
        run_checks_full(design, &EvalContext::nominal(), opts);
    // 標本軸 = 基底Openパラメータのみ、宣言順 (M0-3 base_open_params と同一)
    let axes: Vec<(String, f64, f64)> = design
        .params
        .iter()
        .filter_map(|p| match &p.value {
            ParamValue::Open { range, .. } => Some((p.id.clone(), range.0, range.1)),
            _ => None,
        })
        .collect();
    if axes.is_empty() {
        return (nominal, timings, events, dof);
    }

    // 各軸の端点評価(1変数ずつ、他は公称固定 — ADR-004)
    let mut end_runs: Vec<(String, &'static str, Vec<CheckResult>)> = Vec::new();
    for (p, lo, hi) in &axes {
        for (kind, v) in [("lo", *lo), ("hi", *hi)] {
            let ctx = EvalContext::nominal().assign(p.clone(), v);
            let (rs, t, e, _) = run_checks_full(design, &ctx, opts);
            timings.extend(t);
            events.extend(e);
            end_runs.push((p.clone(), kind, rs));
        }
    }

    let mut merged = Vec::with_capacity(nominal.len());
    for (i, nom) in nominal.iter().enumerate() {
        // samples: 宣言順 × lo→nominal→hi。nominal(全軸共通の1回の評価)は
        // 各軸のトリプルに再掲する (§6.1)
        let mut full: Vec<(&str, &str, &CheckResult)> = Vec::new();
        for (p, _, _) in &axes {
            let get = |kind: &str| -> &CheckResult {
                &end_runs
                    .iter()
                    .find(|(rp, k, _)| rp == p && *k == kind)
                    .expect("端点評価は軸ごとに実行済み")
                    .2[i]
            };
            full.push((p, "lo", get("lo")));
            full.push((p, "nominal", nom));
            full.push((p, "hi", get("hi")));
        }
        // 代表標本: statusランク降順 → margin昇順 → 配列順 (§6.1)
        let mut rep = 0usize;
        for j in 1..full.len() {
            let (a, b) = (full[rep].2, full[j].2);
            let (ra, rb) = (status_rank(&a.status), status_rank(&b.status));
            if rb > ra || (rb == ra && b.margin < a.margin) {
                rep = j;
            }
        }
        let repr = full[rep].2;
        merged.push(CheckResult {
            assert_id: nom.assert_id.clone(),
            checker: nom.checker.clone(),
            status: repr.status.clone(),
            measured: repr.measured.clone(),
            threshold: repr.threshold.clone(),
            margin: repr.margin,
            evidence: repr.evidence.clone(),
            samples: full
                .iter()
                .map(|(p, k, r)| SampleResult {
                    param: (*p).to_string(),
                    sample: (*k).to_string(),
                    status: r.status.clone(),
                    measured: r.measured.clone(),
                })
                .collect(),
        });
    }
    (merged, timings, events, dof)
}

/// `--narrow` の二分探索反復上限 (ADR-004: デフォルト8)
const NARROW_MAX_ITERS: u32 = 8;

/// `adc check --narrow` (M4-2, ADR-004)。
///
/// 3点評価で**片端Fail**(公称Pass・区間端の一方のみFail)のアサーションに対し、
/// 当該Open軸を二分探索(反復上限8・中点規則・他パラメータ公称固定)して
/// 実行可能区間の推定を `suggested_range: <param> ∈ [lo, hi](…)` として
/// 当該アサーションのevidenceに付加する。探索は固定回数で決定的(バイト再現)。
/// 推定区間の境界側はPassを実測した標本値を採用する(保証側に丸める)。
pub fn run_checks_narrow(design: &Design, opts: &CheckOptions) -> FullRunOutput {
    let (mut results, mut timings, mut events, dof) = run_checks_interval(design, opts);
    let axes: Vec<(String, f64, f64, f64)> = design
        .params
        .iter()
        .filter_map(|p| match &p.value {
            ParamValue::Open { range, nominal } => {
                Some((p.id.clone(), range.0, range.1, *nominal))
            }
            _ => None,
        })
        .collect();
    for r in results.iter_mut() {
        let assert_id = r.assert_id.clone();
        for (p, lo, hi, nom) in &axes {
            fn status_of(r: &CheckResult, p: &str, kind: &str) -> Option<CheckStatus> {
                r.samples
                    .iter()
                    .find(|s| s.param == p && s.sample == kind)
                    .map(|s| s.status.clone())
            }
            let (Some(slo), Some(snom), Some(shi)) = (
                status_of(r, p, "lo"),
                status_of(r, p, "nominal"),
                status_of(r, p, "hi"),
            ) else {
                continue;
            };
            // 片端Failのみ探索 (ADR-004): 公称Passを探索の実行可能側アンカーにする
            if !matches!(snom, CheckStatus::Pass) {
                continue;
            }
            let lo_fail = matches!(slo, CheckStatus::Fail);
            let hi_fail = matches!(shi, CheckStatus::Fail);
            let fail_end = match (lo_fail, hi_fail) {
                (true, false) => *lo,
                (false, true) => *hi,
                _ => continue,
            };
            let (mut bad, mut good) = (fail_end, *nom);
            for _ in 0..NARROW_MAX_ITERS {
                let mid = 0.5 * (bad + good);
                let ctx = EvalContext::nominal().assign(p.clone(), mid);
                let (rs, t, e, _) = run_checks_full(design, &ctx, opts);
                timings.extend(t);
                events.extend(e);
                match rs
                    .iter()
                    .find(|x| x.assert_id == assert_id)
                    .map(|x| x.status.clone())
                {
                    Some(CheckStatus::Fail) => bad = mid,
                    Some(CheckStatus::Pass) => good = mid,
                    // 中点でInconclusive → 打ち切り(現在のPass側を採用)
                    _ => break,
                }
            }
            let granularity = (fail_end - *nom).abs() / f64::from(1u32 << NARROW_MAX_ITERS);
            let (rlo, rhi) = if lo_fail { (good, *hi) } else { (*lo, good) };
            r.evidence.push(Evidence {
                anchors: vec![p.clone()],
                points: vec![],
                note: format!(
                    "suggested_range: {p} ∈ [{}, {}](二分探索{NARROW_MAX_ITERS}回、粒度±{}、他パラメータ公称固定)",
                    q(rlo),
                    q(rhi),
                    q(granularity)
                ),
            });
        }
    }
    (results, timings, events, dof)
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
