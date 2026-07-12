//! M5-3 受入テスト (US-19): ToleranceStack1D (worst-case / RSS)。
//!
//! - 3寸法チェーン(ブラケット+シャフト+ハウジング)で手計算値一致
//! - 片側公差(Asym)の符号処理、Fit("h6")の内蔵テーブル解決
//! - 未知はめあい記号・サイズ域外は Inconclusive
//! - nominalのparam参照(Open含み)が3点評価に乗ること
//!
//! 手計算(05-schema.md §7.1):
//!   d1: 4.0 Sym(0.1)            → mid 4.0,     half 0.1
//!   d2: 2.0 Asym(+0.05/−0.02)   → mid 2.015,   half 0.035
//!   d3: 30.0 Fit(h6)=0/−0.013   → mid 29.9935, half 0.0065  (IT6@18〜30=13µm)
//!   Σmid = 36.0085 / WC = [35.867, 36.15] / RSS半幅 = √Σhalf² = 0.10614730331007002

use adc_check::{run_checks, run_checks_interval, CheckOptions, CheckStatus, Value};
use adc_schema::{validate_design, EvalContext};

fn design(d1_nominal: &str, d3_tol: &str, target: &str, method: &str, params: &str) -> String {
    format!(
        r#"Design(
    schema_version: "0.1",
    intent: "M5-3 公差スタックフィクスチャ",
    params: [{params}],
    materials: [Material(id: "a5052", density_g_cm3: 2.68, name: "A5052")],
    parts: [
        Part(id: "bracket", material: "a5052", process: Machining,
            features: [Block(id: "base", x: 80.0, y: 64.0, z: 4.0)],
            anchors: [Anchor(id: "top", kind: Face, binding: feature("base").face("top"))]),
        Part(id: "shaft", material: "a5052", process: Machining,
            features: [Cylinder(id: "body", d: 20.0, h: 30.0)],
            anchors: [
                Anchor(id: "base", kind: Face, binding: feature("body").face("bottom")),
                Anchor(id: "top", kind: Face, binding: feature("body").face("top")),
            ]),
        Part(id: "housing", material: "a5052", process: Machining,
            features: [Block(id: "box", x: 40.0, y: 40.0, z: 20.0)],
            anchors: [Anchor(id: "top", kind: Face, binding: feature("box").face("top"))]),
    ],
    assembly: Assembly(id: "assy",
        instances: [Instance(id: "bracket_i", part: "bracket"),
                    Instance(id: "shaft_i", part: "shaft"),
                    Instance(id: "housing_i", part: "housing")],
        mates: [], ground: "bracket_i"),
    dims: [
        Dim(id: "d1", from: "housing_i.top", to: "bracket_i.top",
            nominal: {d1_nominal}, tol: Sym(0.1), rationale: "r0"),
        Dim(id: "d2", from: "bracket_i.top", to: "shaft_i.base",
            nominal: 2.0, tol: Asym(plus: 0.05, minus: 0.02), rationale: "r0"),
        Dim(id: "d3", from: "shaft_i.base", to: "shaft_i.top",
            nominal: 30.0, tol: {d3_tol}, rationale: "r0"),
    ],
    assertions: [
        Assertion(id: "a_stack",
            check: ToleranceStack1D(path: ["d1", "d2", "d3"], target: {target}, method: {method}),
            rationale: "r0"),
    ],
    rationales: [Rationale(id: "r0", author: Human("t"), basis: Assumption, note: "", timestamp: "2026-07-12T00:00:00Z")],
)"#
    )
}

fn run_one(src: &str) -> adc_check::CheckResult {
    let d = validate_design(src).unwrap_or_else(|e| panic!("検証: {e:#?}"));
    run_checks(&d, &EvalContext::nominal()).remove(0)
}

#[test]
fn worst_case_matches_hand_calculation() {
    let r = run_one(&design("4.0", r#"Fit("h6")"#, "(35.8, 36.2)", "WorstCase", ""));
    assert!(matches!(r.status, CheckStatus::Pass), "{:?}", r.status);
    // measured = [合成lo, Σmid, 合成hi](1e-9量子化)
    let Value::Triple([lo, mid, hi]) = r.measured else {
        panic!("{:?}", r.measured)
    };
    assert_eq!(lo, 35.867, "WC下限(手計算)");
    assert_eq!(mid, 36.0085, "Σmid(手計算)");
    assert_eq!(hi, 36.15, "WC上限(手計算)");
    assert!((r.margin - 0.25).abs() < 1e-9, "margin=(36.2-36.15)/0.2: {}", r.margin);
    // Evidence: 寄与一覧にFit解決値が見える
    let note = &r.evidence[0].note;
    assert!(note.contains("d3"), "{note}");
    assert!(note.contains("29.9935"), "Fit(h6)のmid: {note}");
}

#[test]
fn rss_is_tighter_than_worst_case() {
    // target (35.88, 36.16): WC下限35.867 < 35.88 → Fail、RSS下限35.9024 → Pass
    let wc = run_one(&design("4.0", r#"Fit("h6")"#, "(35.88, 36.16)", "WorstCase", ""));
    assert!(matches!(wc.status, CheckStatus::Fail), "{:?}", wc.status);

    let rss = run_one(&design("4.0", r#"Fit("h6")"#, "(35.88, 36.16)", "Rss", ""));
    assert!(matches!(rss.status, CheckStatus::Pass), "{:?}", rss.status);
    let Value::Triple([lo, _, hi]) = rss.measured else {
        panic!("{:?}", rss.measured)
    };
    assert!((lo - 35.902352697).abs() < 1e-9, "RSS下限 36.0085−√Σhalf²: {lo}");
    assert!((hi - 36.114647303).abs() < 1e-9, "RSS上限: {hi}");

    // Both: 判定はworst-case側、Evidenceに両区間
    let both = run_one(&design("4.0", r#"Fit("h6")"#, "(35.88, 36.16)", "Both", ""));
    assert!(matches!(both.status, CheckStatus::Fail), "BothはWC判定: {:?}", both.status);
    let note = &both.evidence[0].note;
    assert!(note.contains("worst-case") && note.contains("RSS"), "{note}");
}

#[test]
fn asym_sign_convention_follows_path_direction() {
    // d2のAsym(+0.05/−0.02): +側は経路方向に値を増やす → Σmid に +0.015 が乗る。
    // 対称公差に差し替えると mid が 36.0085 → 36.0 に戻ることで符号処理を固定
    let sym = design("4.0", r#"Fit("h6")"#, "(35.8, 36.2)", "WorstCase", "").replace(
        "tol: Asym(plus: 0.05, minus: 0.02)",
        "tol: Sym(0.035)",
    );
    let r = run_one(&sym);
    let Value::Triple([_, mid, _]) = r.measured else {
        panic!()
    };
    assert_eq!(mid, 35.9935, "Asym非対称分(+0.015)が消えること");
}

#[test]
fn unknown_fit_and_out_of_range_are_inconclusive() {
    let r = run_one(&design("4.0", r#"Fit("Z9")"#, "(35.8, 36.2)", "WorstCase", ""));
    match &r.status {
        CheckStatus::Inconclusive { reason } => assert!(reason.contains("Z9"), "{reason}"),
        other => panic!("未知記号はInconclusive: {other:?}"),
    }

    // サイズ域外(>120mm)
    let big = design("4.0", r#"Fit("H7")"#, "(35.8, 36.2)", "WorstCase", "")
        .replace("nominal: 30.0, tol:", "nominal: 150.0, tol:");
    let r = run_one(&big);
    assert!(
        matches!(r.status, CheckStatus::Inconclusive { .. }),
        "サイズ域外はInconclusive: {:?}",
        r.status
    );
}

#[test]
fn open_nominal_rides_three_point_evaluation() {
    // d1のnominalをOpen paramに: t=Open(3.8, 4.2)公称4.0 →
    // lo: Σmid 35.8085 → WC下限35.667 < 35.8 Fail / hi: WC上限36.35 > 36.2 Fail /
    // nominal: Pass — 標本別に見える
    let src = design(
        r#"param("t")"#,
        r#"Fit("h6")"#,
        "(35.8, 36.2)",
        "WorstCase",
        r#"Param(id: "t", value: Open(range: (3.8, 4.2), nominal: 4.0), unit: Mm, rationale: "r0")"#,
    );
    let d = validate_design(&src).unwrap();
    let (rs, ..) = run_checks_interval(&d, &CheckOptions::default());
    let r = &rs[0];
    assert!(matches!(r.status, CheckStatus::Fail), "{:?}", r.status);
    let by = |k: &str| {
        r.samples
            .iter()
            .find(|s| s.sample == k)
            .unwrap_or_else(|| panic!("標本{k}: {:#?}", r.samples))
    };
    assert!(matches!(by("lo").status, CheckStatus::Fail));
    assert!(matches!(by("nominal").status, CheckStatus::Pass));
    assert!(matches!(by("hi").status, CheckStatus::Fail));
}
