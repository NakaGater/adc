//! M3 受入テスト (US-22, US-23, US-24): アセンブリ逐次解決。
//!
//! 3部品(ブラケット+シャフト+ハウジング)の実例Assyで:
//! - mate解決(Coaxial/Distance/Coincident)後の干渉マップ+margin一覧 (M3-1/M3-3)
//! - 宣言順非依存(mate/instanceの順序を入れ替えても同一結果)
//! - 残自由度レポート (M3-2、未拘束=正常)
//! - E-MATE-UNSOLVED{mate_id, 原因} (M3-2)
//! - 部品変更→再buildでmateが生き残る/E-ANCHOR-BINDのAssy経由伝播 (M3-4)

use adc_check::{run_checks, run_checks_full, CheckOptions, CheckStatus, Value};
use adc_schema::{validate_design, EvalContext};

/// 3部品Assy: bracket(ground) + shaft(coaxial+distance) + housing(coincident)
fn assy_design(shaft_d: f64, mates: &str, instances: &str) -> String {
    format!(
        r#"Design(
    schema_version: "0.1",
    intent: "3部品Assy: ブラケット+シャフト+ハウジング",
    params: [],
    materials: [Material(id: "a5052", density_g_cm3: 2.68, name: "A5052")],
    parts: [
        Part(id: "bracket", material: "a5052", process: Machining,
            features: [
                Block(id: "base", x: 80.0, y: 64.0, z: 4.0),
                Hole(id: "bore", kind: Simple, d: 55.0, depth: Through,
                     at: on(feature("base").face("top"), center())),
            ],
            anchors: [
                Anchor(id: "bore_axis", kind: Axis, binding: feature("bore").axis("axis")),
                Anchor(id: "bearing_bore", kind: Face, binding: feature("bore").face("wall")),
                Anchor(id: "top_face", kind: Face, binding: feature("base").face("top")),
                Anchor(id: "mount_face", kind: Face, binding: feature("base").face("bottom")),
            ]),
        Part(id: "shaft", material: "a5052", process: Machining,
            features: [Cylinder(id: "body", d: {shaft_d}, h: 30.0)],
            anchors: [
                Anchor(id: "axis", kind: Axis, binding: feature("body").axis("axis")),
                Anchor(id: "base_face", kind: Face, binding: feature("body").face("bottom")),
                Anchor(id: "od", kind: Face, binding: feature("body").face("side")),
            ]),
        Part(id: "housing", material: "a5052", process: Machining,
            features: [Block(id: "box", x: 40.0, y: 40.0, z: 20.0)],
            anchors: [
                Anchor(id: "top_face", kind: Face, binding: feature("box").face("top")),
            ]),
    ],
    assembly: Assembly(id: "mount_assy",
        instances: [{instances}],
        mates: [{mates}],
        ground: "bracket_i"),
    assertions: [
        Assertion(id: "a_clear",
            check: Clearance(a: "bracket_i.bearing_bore", b: "shaft_i.od", min: 1.0), rationale: "r0"),
        Assertion(id: "b_map", check: NoInterference(scope: All), rationale: "r0"),
    ],
    rationales: [Rationale(id: "r0", author: Human("t"), basis: Assumption, note: "", timestamp: "2026-07-12T00:00:00Z")],
)"#
    )
}

const INSTANCES: &str = r#"Instance(id: "bracket_i", part: "bracket"),
        Instance(id: "shaft_i", part: "shaft"),
        Instance(id: "housing_i", part: "housing")"#;

const MATES: &str = r#"Mate(id: "m_coax", kind: Coaxial, a: "bracket_i.bore_axis", b: "shaft_i.axis", rationale: "r0"),
        Mate(id: "m_lift", kind: Distance(-2.0), a: "bracket_i.top_face", b: "shaft_i.base_face", rationale: "r0"),
        Mate(id: "m_house", kind: Coincident, a: "bracket_i.mount_face", b: "housing_i.top_face", rationale: "r0")"#;

fn design(shaft_d: f64) -> adc_schema::Design {
    validate_design(&assy_design(shaft_d, MATES, INSTANCES))
        .unwrap_or_else(|e| panic!("検証: {e:#?}"))
}

#[test]
fn three_part_assembly_solves_and_maps_interference() {
    let d = design(50.0);
    let rs = run_checks(&d, &EvalContext::nominal());

    // Clearance: mate解決後、シャフト(φ50)がボア(φ55)と同軸 → 半径ギャップ2.5
    let clear = &rs[0];
    assert!(matches!(clear.status, CheckStatus::Pass), "{:?}", clear.status);
    assert!(
        matches!(clear.measured, Value::Scalar(v) if (v - 2.5).abs() < 1e-6),
        "mate解決後の配置で距離2.5: {:?}",
        clear.measured
    );

    // 干渉マップ (M3-3): 3ペア全てのmargin一覧がEvidenceに載る
    let map = &rs[1];
    assert!(matches!(map.status, CheckStatus::Pass), "{:?}", map.status);
    assert_eq!(map.evidence.len(), 3, "全3ペアの一覧: {:#?}", map.evidence);
    // shaft-bracket ペア: ボアを貫くシャフト(Distance(-2.0)で板厚に食い込む配置)
    // → 最小距離は半径ギャップ2.5
    let sb = map
        .evidence
        .iter()
        .find(|e| e.anchors.contains(&"bracket_i".to_string()) && e.anchors.contains(&"shaft_i".to_string()))
        .expect("bracket-shaftペア");
    assert!(sb.note.contains("最小距離 2.5"), "{}", sb.note);
    // housing はブラケット底面に接触(距離0、交差なし)
    let bh = map
        .evidence
        .iter()
        .find(|e| e.anchors.contains(&"housing_i".to_string()) && e.anchors.contains(&"bracket_i".to_string()))
        .expect("bracket-housingペア");
    assert!(bh.note.contains("最小距離 0"), "{}", bh.note);
}

#[test]
fn declaration_order_independent() {
    // mate列とinstance列の宣言順を入れ替えても同一結果(位相ソート)
    let mates_shuffled = r#"Mate(id: "m_house", kind: Coincident, a: "bracket_i.mount_face", b: "housing_i.top_face", rationale: "r0"),
        Mate(id: "m_lift", kind: Distance(-2.0), a: "bracket_i.top_face", b: "shaft_i.base_face", rationale: "r0"),
        Mate(id: "m_coax", kind: Coaxial, a: "bracket_i.bore_axis", b: "shaft_i.axis", rationale: "r0")"#;
    let instances_shuffled = r#"Instance(id: "housing_i", part: "housing"),
        Instance(id: "shaft_i", part: "shaft"),
        Instance(id: "bracket_i", part: "bracket")"#;
    let d1 = design(50.0);
    let d2 = validate_design(&assy_design(50.0, mates_shuffled, instances_shuffled)).unwrap();
    let j1 = adc_check::to_jsonl(&run_checks(&d1, &EvalContext::nominal()));
    let j2 = adc_check::to_jsonl(&run_checks(&d2, &EvalContext::nominal()));
    assert_eq!(j1, j2, "宣言順に依存しないこと");
}

#[test]
fn dof_report_lists_remaining_freedom() {
    let d = design(50.0);
    let (_, _, _, dof) = run_checks_full(&d, &EvalContext::nominal(), &CheckOptions::default());
    let get = |inst: &str| {
        dof.iter()
            .find(|(i, _, _)| i == inst)
            .unwrap_or_else(|| panic!("{inst}のDOF報告"))
    };
    assert_eq!(get("bracket_i").1, 0, "groundは固定");
    // shaft: coaxial(-4) + distance(-3) → 0(近似計上)
    assert_eq!(get("shaft_i").1, 0, "{:?}", get("shaft_i"));
    // housing: coincident(-3)のみ → 残3(面内2並進+1回転。未拘束は正常・報告のみ)
    assert_eq!(get("housing_i").1, 3, "{:?}", get("housing_i"));
    assert!(get("housing_i").2.contains("coincident"));
}

#[test]
fn contradictory_mates_report_unsolved_with_mate_id() {
    // 同じ面ペアに Distance(2.0) と Distance(5.0) → 逐次解決で前者が壊れる
    let mates = r#"Mate(id: "m_coax", kind: Coaxial, a: "bracket_i.bore_axis", b: "shaft_i.axis", rationale: "r0"),
        Mate(id: "m_lift", kind: Distance(-2.0), a: "bracket_i.top_face", b: "shaft_i.base_face", rationale: "r0"),
        Mate(id: "m_lift2", kind: Distance(5.0), a: "bracket_i.top_face", b: "shaft_i.base_face", rationale: "r0"),
        Mate(id: "m_house", kind: Coincident, a: "bracket_i.mount_face", b: "housing_i.top_face", rationale: "r0")"#;
    let d = validate_design(&assy_design(50.0, mates, INSTANCES)).unwrap();
    let rs = run_checks(&d, &EvalContext::nominal());
    match &rs[0].status {
        CheckStatus::Inconclusive { reason } => {
            assert!(reason.contains("E-MATE-UNSOLVED"), "{reason}");
            assert!(reason.contains("m_lift"), "原因mate idを含む: {reason}");
        }
        other => panic!("Inconclusive(E-MATE-UNSOLVED)のはず: {other:?}"),
    }
}

// ================================================================ M3-4: 再生成

#[test]
fn shaft_diameter_change_rebuilds_and_mates_survive() {
    // φ50 → φ52: 再buildでmateが生き残り、クリアランスが追従 (2.5 → 1.5)
    let d = design(52.0);
    let rs = run_checks(&d, &EvalContext::nominal());
    let clear = &rs[0];
    assert!(matches!(clear.status, CheckStatus::Pass), "{:?}", clear.status);
    assert!(
        matches!(clear.measured, Value::Scalar(v) if (v - 1.5).abs() < 1e-6),
        "{:?}",
        clear.measured
    );

    // φ56: 干渉 → Clearance Fail + マップにも交差
    let d = design(56.0);
    let rs = run_checks(&d, &EvalContext::nominal());
    assert!(matches!(rs[0].status, CheckStatus::Fail), "{:?}", rs[0].status);
    assert!(matches!(rs[1].status, CheckStatus::Fail), "{:?}", rs[1].status);
    assert!(rs[1].evidence.iter().any(|e| e.note.contains("交差体積")));
}

#[test]
fn anchor_bind_failure_propagates_through_assembly() {
    // シャフトのod/axisアンカーを後続ポケットで食い潰す → E-ANCHOR-BIND →
    // 部品コンパイル失敗 → mate解決失敗(E-MATE-UNSOLVED、原因にE-ANCHOR-BIND)
    let src = assy_design(50.0, MATES, INSTANCES).replace(
        "h: 30.0)],",
        r#"h: 30.0),
                Pocket(id: "swallow", profile: Circ(d: 60.0), depth: 31.0,
                       at: on(feature("body").face("top"), center()))],"#,
    );
    assert!(src.contains("swallow"), "置換が適用されていること");
    let d = validate_design(&src).unwrap();
    let rs = run_checks(&d, &EvalContext::nominal());
    match &rs[0].status {
        CheckStatus::Inconclusive { reason } => {
            assert!(
                reason.contains("E-MATE-UNSOLVED") || reason.contains("コンパイルに失敗"),
                "{reason}"
            );
        }
        other => panic!("Inconclusiveのはず: {other:?}"),
    }
}
