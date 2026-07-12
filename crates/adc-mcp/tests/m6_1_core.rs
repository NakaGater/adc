//! M6-1 受入テスト(コア): design_patchの一意一致・楽観ロック・gated、
//! narrow構造化、evidence_query。プロトコル層はm6_1_protocol.rsが担当。

use adc_mcp::{AdcCore, Edit};
use sha2::{Digest, Sha256};

fn sha(s: &str) -> String {
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    format!("{:x}", h.finalize())
}

/// t=Open(1,5)公称4の板。WallThickness min 2.5 → narrow境界は t=2.5(既知)
fn fixture() -> &'static str {
    r#"Design(
    schema_version: "0.1",
    intent: "m6-1 fixture",
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

fn setup(name: &str, gated: bool) -> (AdcCore, std::path::PathBuf) {
    let dir = std::env::temp_dir().join("adc-mcp-core").join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("design.ron");
    std::fs::write(&path, fixture()).unwrap();
    (AdcCore::new(&path, gated), path)
}

fn edit(old: &str, new: &str) -> Edit {
    Edit {
        old_string: old.into(),
        new_string: new.into(),
    }
}

#[test]
fn design_read_returns_source_sha_and_validity() {
    let (core, _) = setup("read", false);
    let v = core.design_read();
    assert_eq!(v["valid"], true);
    assert_eq!(v["sha256"], sha(fixture()));
    assert!(v["source"].as_str().unwrap().contains("a_wall"));
}

#[test]
fn edits_require_unique_match_zero_and_multiple_are_structured_errors() {
    let (core, path) = setup("unique", false);
    let base = sha(fixture());

    // 0件一致 → E-PATCH not_found
    let v = core.design_patch(&base, Some(vec![edit("存在しない文字列", "x")]), None, false);
    assert_eq!(v["error"]["code"], "E-PATCH", "{v}");
    assert_eq!(v["error"]["kind"], "not_found");
    assert_eq!(v["error"]["occurrences"], 0);
    assert_eq!(v["error"]["edit_index"], 0);

    // 複数一致("plate" は2箇所: part id と assertion参照) → ambiguous
    let occurrences = fixture().matches(r#""plate""#).count();
    assert!(occurrences > 1, "前提: 複数一致 ({occurrences})");
    let v = core.design_patch(&base, Some(vec![edit(r#""plate""#, r#""p2""#)]), None, false);
    assert_eq!(v["error"]["code"], "E-PATCH", "{v}");
    assert_eq!(v["error"]["kind"], "ambiguous");
    assert_eq!(v["error"]["occurrences"], occurrences);

    // どちらのケースもファイル不変(バイト比較)
    assert_eq!(std::fs::read_to_string(&path).unwrap(), fixture());
}

#[test]
fn optimistic_lock_conflict_rejects_without_write() {
    let (core, path) = setup("lock", false);
    let v = core.design_patch("deadbeef", Some(vec![edit("min: 2.5", "min: 3.0")]), None, false);
    assert_eq!(v["applied"], false, "{v}");
    assert_eq!(v["rejected_reason"], "conflict");
    assert_eq!(v["current_sha256"], sha(fixture()));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), fixture());
}

#[test]
fn gated_rejects_failing_patch_and_file_is_byte_identical() {
    let (core, path) = setup("gated", true);
    let before = std::fs::read(&path).unwrap();
    // min 2.5→10.0: 公称(t=4)もFail → gatedは書き込まない
    let v = core.design_patch(&sha(fixture()), Some(vec![edit("min: 2.5", "min: 10.0")]), None, false);
    assert_eq!(v["applied"], false, "{v}");
    assert_eq!(v["rejected_reason"], "gated_fail");
    assert_eq!(v["gated_check"]["exit_code"], 1);
    assert!(v["gated_check"]["results"].as_array().unwrap().len() == 1);
    assert_eq!(std::fs::read(&path).unwrap(), before, "ファイル不変(バイト比較)");

    // 全Passになるpatchは適用される
    let v = core.design_patch(&sha(fixture()), Some(vec![edit("min: 2.5", "min: 0.5")]), None, false);
    assert_eq!(v["applied"], true, "{v}");
    assert_eq!(v["gated_check"]["exit_code"], 0);
    assert!(std::fs::read_to_string(&path).unwrap().contains("min: 0.5"));
}

#[test]
fn non_gated_allows_red_state_writes() {
    // 対話セッション既定(非gated): Failになるpatchも正典に刻める(Red許容 —
    // 人間のPRレビューがゲート。2026-07-12承認時修正②)
    let (core, path) = setup("nongated", false);
    let v = core.design_patch(&sha(fixture()), Some(vec![edit("min: 2.5", "min: 10.0")]), None, false);
    assert_eq!(v["applied"], true, "{v}");
    assert!(v.get("gated_check").is_none_or(|g| g.is_null()));
    assert!(std::fs::read_to_string(&path).unwrap().contains("min: 10.0"));
}

#[test]
fn invalid_patch_is_rejected_with_validation_errors() {
    let (core, path) = setup("invalid", false);
    let v = core.design_patch(
        &sha(fixture()),
        Some(vec![edit(r#"rationale: "r0"), rationale"#, r#"rationale: "ghost"), rationale"#)]),
        None,
        false,
    );
    // 一意一致しない可能性があるためfull_sourceでも検証
    let broken = fixture().replace(r#"Param(id: "t""#, r#"Param(id: "t2""#);
    let v2 = core.design_patch(&sha(fixture()), None, Some(broken), false);
    for v in [&v, &v2] {
        let rejected = v["applied"] == false || v["error"]["code"] == "E-PATCH";
        assert!(rejected, "{v}");
    }
    assert_eq!(std::fs::read_to_string(&path).unwrap(), fixture());
}

#[test]
fn narrow_param_returns_structured_suggestion() {
    let (core, _) = setup("narrow", false);
    let v = core.narrow_param(Some("t"));
    let sug = &v["suggestions"][0];
    assert_eq!(sug["param"], "t", "{v}");
    assert_eq!(sug["assert_id"], "a_wall");
    let (lo, hi) = (sug["lo"].as_f64().unwrap(), sug["hi"].as_f64().unwrap());
    let g = sug["granularity"].as_f64().unwrap();
    assert!((lo - 2.5).abs() <= g + 1e-9, "既知境界2.5±粒度: {lo} (g={g})");
    assert_eq!(hi, 5.0);
    // 存在しないparam指定 → suggestionsは空(探索対象なし)
    let v = core.narrow_param(Some("nope"));
    assert_eq!(v["suggestions"].as_array().unwrap().len(), 0);
}

#[test]
fn evidence_query_filters_by_status_and_assert_id() {
    let (core, _) = setup("evq", false);
    let v = core.evidence_query(None, Some("fail"));
    assert_eq!(v["results"].as_array().unwrap().len(), 1, "{v}"); // 区間Fail(lo端)
    let v = core.evidence_query(Some("a_wall"), Some("pass"));
    assert_eq!(v["results"].as_array().unwrap().len(), 0);
    let v = core.evidence_query(Some("nope"), None);
    assert_eq!(v["results"].as_array().unwrap().len(), 0);
}

#[test]
fn explain_returns_referenced_by() {
    let (core, _) = setup("explain", false);
    let v = core.explain("t");
    assert_eq!(v["status"], "found", "{v}");
    let refs = v["matches"][0]["referenced_by"].as_array().unwrap();
    assert!(refs.iter().any(|r| r["id"] == "base"), "{v}");
}
