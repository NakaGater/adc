//! M4-2 受入テスト (US-26, ADR-004): `--narrow` 二分探索+suggested_range。
//!
//! - 片端Fail時のみ二分探索(反復上限8、他パラメータ公称固定)
//! - suggested_rangeを当該アサーションのevidenceに付加
//! - 探索は決定的(固定回数・中点規則)。バイト再現必須
//! - 受入: 既知の実行可能境界に±探索粒度で収束

use adc_check::{run_checks_narrow, to_jsonl, CheckOptions, CheckStatus};
use adc_schema::validate_design;

/// 人工フィクスチャ: 板厚 t = Open(1,5) 公称4。WallThickness(min: 2.5) の
/// 実測厚は t そのもの → 実行可能境界はちょうど t = 2.5(既知)
fn src() -> &'static str {
    r#"Design(
    schema_version: "0.1",
    intent: "narrow人工フィクスチャ: 境界既知(t=2.5)",
    params: [Param(id: "t", value: Open(range: (1.0, 5.0), nominal: 4.0), unit: Mm, rationale: "r0")],
    materials: [Material(id: "a5052", density_g_cm3: 2.68, name: "A5052")],
    parts: [
        Part(id: "plate", material: "a5052", process: Machining,
            features: [Block(id: "base", x: 30.0, y: 20.0, z: param("t"))],
            anchors: []),
    ],
    assertions: [
        Assertion(id: "a_wall",
            check: WallThickness(part: "plate", min: 2.5, sample_density: 1.0), rationale: "r0"),
    ],
    rationales: [Rationale(id: "r0", author: Human("t"), basis: Assumption, note: "", timestamp: "2026-07-12T00:00:00Z")],
)"#
}

#[test]
fn one_sided_fail_converges_to_known_boundary_within_granularity() {
    let d = validate_design(src()).unwrap();
    let (rs, ..) = run_checks_narrow(&d, &CheckOptions::default());
    let r = &rs[0];

    // lo端(t=1.0)のみFail → 区間statusはFail
    assert!(matches!(r.status, CheckStatus::Fail), "{:?}", r.status);

    // suggested_range が evidence に付加される
    let sug = r
        .evidence
        .iter()
        .find(|e| e.note.starts_with("suggested_range"))
        .unwrap_or_else(|| panic!("suggested_rangeのevidence: {:#?}", r.evidence));
    assert_eq!(sug.anchors, vec!["t".to_string()]);

    // note形式: suggested_range: t ∈ [<lo>, <hi>](…)
    let inner = sug
        .note
        .split('[')
        .nth(1)
        .and_then(|s| s.split(']').next())
        .expect("区間表記");
    let mut it = inner.split(',').map(|v| v.trim().parse::<f64>().unwrap());
    let (lo, hi) = (it.next().unwrap(), it.next().unwrap());

    // 既知境界 t=2.5 に±探索粒度で収束(探索区間 [1.0, 4.0] → 粒度 3/256)
    let g = 3.0 / 256.0;
    assert!(
        (lo - 2.5).abs() <= g + 1e-9,
        "下限が境界2.5に±{g}で収束: {lo}"
    );
    assert!(lo >= 2.5 - 1e-9, "推定区間は実行可能側(Pass標本)に取る: {lo}");
    assert_eq!(hi, 5.0, "上限はOpen区間の上端");
}

#[test]
fn narrow_output_is_byte_reproducible() {
    let d = validate_design(src()).unwrap();
    let (r1, ..) = run_checks_narrow(&d, &CheckOptions::default());
    let (r2, ..) = run_checks_narrow(&d, &CheckOptions::default());
    assert_eq!(to_jsonl(&r1), to_jsonl(&r2), "バイト再現");
}

#[test]
fn no_narrow_when_interval_passes_or_both_ends_fail() {
    // 全点Pass → suggested_rangeなし
    let pass_src = src().replace("min: 2.5", "min: 0.5");
    let d = validate_design(&pass_src).unwrap();
    let (rs, ..) = run_checks_narrow(&d, &CheckOptions::default());
    assert!(matches!(rs[0].status, CheckStatus::Pass));
    assert!(
        !rs[0].evidence.iter().any(|e| e.note.starts_with("suggested_range")),
        "全点Passにsuggested_rangeを付けない"
    );

    // 両端Fail(公称もFail) → 片端Failではないので探索しない
    let fail_src = src().replace("min: 2.5", "min: 10.0");
    let d = validate_design(&fail_src).unwrap();
    let (rs, ..) = run_checks_narrow(&d, &CheckOptions::default());
    assert!(matches!(rs[0].status, CheckStatus::Fail));
    assert!(
        !rs[0].evidence.iter().any(|e| e.note.starts_with("suggested_range")),
        "両端Failでは探索しない(片端Fail時のみ)"
    );
}
