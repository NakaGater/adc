//! M5-2 受入テスト (US-18): SheetMetalRules(代数チェック)。
//!
//! - bend_r ≥ 1.0×t / length ≥ 4×t / 穴縁-曲げ根元 ≥ 2t+bend_r
//! - フィーチャー定義からの代数計算のみ。Evidence=feature_id+実測値+規則値
//! - 板厚・曲げRのパラメータ参照(Open含む)が3点評価に乗ること

use adc_check::{run_checks, run_checks_interval, CheckOptions, CheckStatus, Value};
use adc_schema::{validate_design, EvalContext};

fn design(t: &str, bend_r: &str, length: &str, hole_at: &str, params: &str) -> String {
    format!(
        r#"Design(
    schema_version: "0.1",
    intent: "M5-2 板金規則フィクスチャ",
    params: [{params}],
    materials: [Material(id: "spcc", density_g_cm3: 7.85, name: "SPCC")],
    parts: [
        Part(id: "cover", material: "spcc",
            process: SheetMetal(thickness: {t}, k_factor: 0.44),
            features: [
                BaseFlange(id: "web", profile: Rect(x: 50.0, y: 30.0)),
                Flange(id: "lip", edge: edges_between(feature("web").face("top"), feature("web").face("+x")),
                       angle: 90.0, length: {length}, bend_r: {bend_r}),
                Hole(id: "vent", kind: Simple, d: 10.0, depth: Through,
                     at: on(feature("web").face("top"), {hole_at})),
            ],
            anchors: []),
    ],
    assertions: [
        Assertion(id: "a_smr", check: SheetMetalRules(part: "cover"), rationale: "r0"),
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
fn all_rules_pass_with_margin() {
    // t=2: bend_r 3≥2 / length 20≥8 / 穴-曲げ 25-0-5=20 ≥ 2·2+3=7
    let r = run_one(&design("2.0", "3.0", "20.0", "center()", ""));
    assert!(matches!(r.status, CheckStatus::Pass), "{:?}", r.status);
    // margin = 最悪規則 = bend_r: (3-2)/2 = 0.5
    assert!((r.margin - 0.5).abs() < 1e-9, "{}", r.margin);
    let note = &r.evidence[0].note;
    assert!(note.contains("全規則Pass"), "{note}");
}

#[test]
fn bend_radius_violation_has_feature_id_and_values() {
    // bend_r 1.5 < t 2.0
    let r = run_one(&design("2.0", "1.5", "20.0", "center()", ""));
    assert!(matches!(r.status, CheckStatus::Fail), "{:?}", r.status);
    let ev = r
        .evidence
        .iter()
        .find(|e| e.note.contains("最小曲げ半径"))
        .unwrap_or_else(|| panic!("{:#?}", r.evidence));
    assert_eq!(ev.anchors, vec!["lip".to_string()], "Evidence=feature_id");
    assert!(ev.note.contains("1.5") && ev.note.contains("2"), "{}", ev.note);
    assert!(matches!(r.measured, Value::Scalar(v) if (v - 1.5).abs() < 1e-9));
}

#[test]
fn flange_length_and_hole_to_bend_violations() {
    // length 6 < 4t=8
    let r = run_one(&design("2.0", "3.0", "6.0", "center()", ""));
    assert!(matches!(r.status, CheckStatus::Fail));
    assert!(r.evidence.iter().any(|e| e.note.contains("フランジ最小長")));

    // 穴を+x曲げ根元へ寄せる: dist = 25-20-5 = 0 < 7
    let r = run_one(&design("2.0", "3.0", "20.0", "xy(20.0, 0.0)", ""));
    assert!(matches!(r.status, CheckStatus::Fail));
    let ev = r
        .evidence
        .iter()
        .find(|e| e.note.contains("穴-曲げ距離"))
        .unwrap_or_else(|| panic!("{:#?}", r.evidence));
    assert!(ev.anchors[0].contains("vent"), "{:?}", ev.anchors);
}

#[test]
fn machining_part_is_inconclusive_and_flat_sheet_passes() {
    let src = design("2.0", "3.0", "20.0", "center()", "")
        .replace("process: SheetMetal(thickness: 2.0, k_factor: 0.44)", "process: Machining")
        .replace(
            r#"BaseFlange(id: "web", profile: Rect(x: 50.0, y: 30.0)),
                Flange(id: "lip", edge: edges_between(feature("web").face("top"), feature("web").face("+x")),
                       angle: 90.0, length: 20.0, bend_r: 3.0),"#,
            r#"Block(id: "web", x: 50.0, y: 30.0, z: 2.0),"#,
        );
    let r = run_one(&src);
    assert!(
        matches!(&r.status, CheckStatus::Inconclusive { reason } if reason.contains("SheetMetal")),
        "{:?}",
        r.status
    );

    // 曲げなしの平板はPass(margin 1.0)
    let flat = design("2.0", "3.0", "20.0", "center()", "").replace(
        r#"Flange(id: "lip", edge: edges_between(feature("web").face("top"), feature("web").face("+x")),
                       angle: 90.0, length: 20.0, bend_r: 3.0),
                "#,
        "",
    );
    let r = run_one(&flat);
    assert!(matches!(r.status, CheckStatus::Pass), "{:?}", r.status);
    assert_eq!(r.margin, 1.0);
    assert!(r.evidence[0].note.contains("曲げなし"));
}

#[test]
fn open_thickness_rides_three_point_evaluation() {
    // t = Open(1.5, 4.0) 公称2.0、bend_r=3: hi端(t=4)で bend_r 3 < 4 → Fail。
    // lo端(t=1.5): 3≥1.5 / 20≥6 / 20≥3+3 → Pass。公称もPass → 片端Failが標本別に見える
    let src = design(
        r#"param("t")"#,
        "3.0",
        "20.0",
        "center()",
        r#"Param(id: "t", value: Open(range: (1.5, 4.0), nominal: 2.0), unit: Mm, rationale: "r0")"#,
    );
    let d = validate_design(&src).unwrap();
    let (rs, ..) = run_checks_interval(&d, &CheckOptions::default());
    let r = &rs[0];
    assert!(matches!(r.status, CheckStatus::Fail), "{:?}", r.status);
    let by = |k: &str| r.samples.iter().find(|s| s.sample == k).unwrap();
    assert!(matches!(by("lo").status, CheckStatus::Pass), "{:#?}", r.samples);
    assert!(matches!(by("nominal").status, CheckStatus::Pass));
    assert!(matches!(by("hi").status, CheckStatus::Fail), "{:#?}", r.samples);
}
