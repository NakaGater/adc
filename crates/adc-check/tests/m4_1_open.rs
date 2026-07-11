//! M4-1 受入テスト (US-25, ADR-004): Open 3点評価。
//!
//! - 標本軸=基底Openパラメータのみ、各軸1変数ずつ・他は公称固定
//! - results.jsonl: アサーションごと1行維持、status=全標本の最悪値、
//!   samples=標本別サブ結果 {param, sample(lo/nominal/hi), status, measured}
//! - 受入: §9のwall_t=Open(3,6)で全点Pass→区間Pass、
//!   minを吊り上げた変形で片端Failが標本別に見えること

use adc_check::{run_checks_interval, to_jsonl, CheckOptions, CheckStatus};
use adc_schema::validate_design;

/// §9サンプル(examples/motor_bracket/design.ron)を正とする
fn sample_src() -> String {
    std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/motor_bracket/design.ron"
    ))
    .expect("§9サンプル")
}

#[test]
fn section9_wall_t_open_interval_passes_with_samples() {
    // §9: wall_t = Open(range: (3.0, 6.0), nominal: 4.0)。全点Pass→区間Pass
    let d = validate_design(&sample_src()).unwrap();
    let (rs, ..) = run_checks_interval(&d, &CheckOptions::default());

    let wall = rs
        .iter()
        .find(|r| r.assert_id == "a_wall")
        .expect("a_wall(WallThickness)");
    assert!(matches!(wall.status, CheckStatus::Pass), "{:?}", wall.status);

    // samples: 基底Open宣言順(wall_tのみ)× lo→nominal→hi
    assert_eq!(wall.samples.len(), 3, "{:#?}", wall.samples);
    let kinds: Vec<&str> = wall.samples.iter().map(|s| s.sample.as_str()).collect();
    assert_eq!(kinds, ["lo", "nominal", "hi"]);
    assert!(wall.samples.iter().all(|s| s.param == "wall_t"));
    assert!(
        wall.samples
            .iter()
            .all(|s| matches!(s.status, CheckStatus::Pass)),
        "全点Pass: {:#?}",
        wall.samples
    );
    // 全アサーションが区間Pass
    assert!(
        rs.iter().all(|r| matches!(r.status, CheckStatus::Pass)),
        "{:#?}",
        rs.iter().map(|r| (&r.assert_id, &r.status)).collect::<Vec<_>>()
    );
}

#[test]
fn raised_min_fails_only_at_lo_end_visible_per_sample() {
    // minを吊り上げ: wall_t=Open(3,6)に対して min 3.5 → lo端(3.0)のみFail
    let src = sample_src().replace(
        "check: WallThickness(part: \"bracket\", min: 2.5, sample_density: 1.0)",
        "check: WallThickness(part: \"bracket\", min: 3.5, sample_density: 1.0)",
    );
    assert!(src.contains("min: 3.5"), "置換が適用されていること");
    let d = validate_design(&src).unwrap();
    let (rs, ..) = run_checks_interval(&d, &CheckOptions::default());

    let wall = rs.iter().find(|r| r.assert_id == "a_wall").unwrap();
    // 区間status = 全標本の最悪値 = Fail
    assert!(matches!(wall.status, CheckStatus::Fail), "{:?}", wall.status);

    // 標本別: lo(3.0)のみFail、nominal(4.0)/hi(6.0)はPass
    let by_kind = |k: &str| {
        wall.samples
            .iter()
            .find(|s| s.sample == k && s.param == "wall_t")
            .unwrap_or_else(|| panic!("標本 {k}"))
    };
    assert!(matches!(by_kind("lo").status, CheckStatus::Fail), "{:#?}", wall.samples);
    assert!(matches!(by_kind("nominal").status, CheckStatus::Pass));
    assert!(matches!(by_kind("hi").status, CheckStatus::Pass));

    // 行トップレベルは代表標本(最悪=loのFail)の実測: 板厚3.0 < 3.5
    assert!(
        matches!(wall.measured, adc_check::Value::Scalar(v) if (v - 3.0).abs() < 0.2),
        "代表標本(lo端)の実測: {:?}",
        wall.measured
    );
    assert!(wall.margin < 0.0, "最悪margin: {}", wall.margin);
}

#[test]
fn interval_output_is_deterministic_and_openless_designs_have_no_samples() {
    // バイト再現(2回実行で同一)
    let d = validate_design(&sample_src()).unwrap();
    let (r1, ..) = run_checks_interval(&d, &CheckOptions::default());
    let (r2, ..) = run_checks_interval(&d, &CheckOptions::default());
    assert_eq!(to_jsonl(&r1), to_jsonl(&r2), "バイト再現");

    // samplesフィールドはOpenありのときのみ出力(M2-1以来の出力とバイト互換)
    assert!(to_jsonl(&r1).contains("\"samples\""));
    let openless = sample_src().replace(
        "value: Open(range: (3.0, 6.0), nominal: 4.0)",
        "value: Determined(4.0)",
    );
    let d2 = validate_design(&openless).unwrap();
    let (r3, ..) = run_checks_interval(&d2, &CheckOptions::default());
    assert!(
        !to_jsonl(&r3).contains("\"samples\""),
        "Openなし設計にsamplesフィールドを出さない"
    );
}
