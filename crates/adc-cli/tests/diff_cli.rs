//! M4-3 受入テスト (US-10): `adc diff <rev1> <rev2>`。
//!
//! 一時gitリポジトリに2版のdesign.ronをコミットし、
//! 制約差分(rationale込み)・param変更・体積差・margin変化表が出ること、
//! --format=text|json の両方が決定的であることを固定する。

use std::process::Command;

fn design(t: &str, wall_min: &str, extra_assert: &str, extra_rationale: &str) -> String {
    format!(
        r#"Design(
    schema_version: "0.1",
    intent: "diffフィクスチャ",
    params: [Param(id: "t", value: Determined({t}), unit: Mm, rationale: "r0")],
    materials: [Material(id: "a5052", density_g_cm3: 2.68, name: "A5052")],
    parts: [
        Part(id: "plate", material: "a5052", process: Machining,
            features: [Block(id: "base", x: 30.0, y: 20.0, z: param("t"))],
            anchors: []),
    ],
    assertions: [
        Assertion(id: "a_wall",
            check: WallThickness(part: "plate", min: {wall_min}, sample_density: 1.0), rationale: "r0"),{extra_assert}
    ],
    rationales: [
        Rationale(id: "r0", author: Human("t"), basis: Assumption, note: "板厚仮置き", timestamp: "2026-07-12T00:00:00Z"),{extra_rationale}
    ],
)"#
    )
}

/// 一時gitリポジトリを作り、v1/v2をコミットしてdesign.ronのパスを返す
fn setup_repo(name: &str) -> String {
    let dir = std::env::temp_dir().join("adc-diff-cli").join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let git = |args: &[&str]| {
        let st = Command::new("git")
            .arg("-C")
            .arg(&dir)
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@example.com",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(st.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&st.stderr));
    };
    git(&["init", "-q"]);
    // v1: t=4.0, min 2.5 (Pass)
    std::fs::write(dir.join("design.ron"), design("4.0", "2.5", "", "")).unwrap();
    git(&["add", "design.ron"]);
    git(&["commit", "-q", "-m", "v1"]);
    // v2: t=3.0, min 3.5 (Fail)、a_mass追加(rationale込み)
    std::fs::write(
        dir.join("design.ron"),
        design(
            "3.0",
            "3.5",
            r#"
        Assertion(id: "b_mass", check: Mass(part: "plate", max: 200.0), rationale: "r1"),"#,
            r#"
        Rationale(id: "r1", author: Human("t"), basis: Requirement("REQ-012"), note: "質量上限の追加", timestamp: "2026-07-12T00:00:00Z"),"#,
        ),
    )
    .unwrap();
    git(&["add", "design.ron"]);
    git(&["commit", "-q", "-m", "v2"]);
    dir.join("design.ron").to_str().unwrap().to_string()
}

fn run(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_adc"))
        .args(args)
        .output()
        .expect("adc実行");
    (
        out.status.code().unwrap(),
        String::from_utf8(out.stdout).unwrap(),
        String::from_utf8(out.stderr).unwrap(),
    )
}

#[test]
fn diff_reports_constraints_params_volume_and_margins() {
    let p = setup_repo("text");
    let (code, out, err) = run(&["diff", "HEAD~1", "HEAD", "--design", &p]);
    assert_eq!(code, 0, "stderr: {err}");

    // param変更: t 4.0 → 3.0
    assert!(out.contains("t"), "{out}");
    assert!(out.contains("4.0") && out.contains("3.0"), "param変更: {out}");
    // 制約差分: b_mass追加(rationale込み)、a_wall変更
    assert!(out.contains("b_mass"), "{out}");
    assert!(out.contains("質量上限の追加"), "追加制約のrationale: {out}");
    assert!(out.contains("a_wall"), "{out}");
    // 体積差: 30×20×4=2400 → 30×20×3=1800(Δ -600)
    assert!(out.contains("2400") && out.contains("1800"), "体積差: {out}");
    assert!(out.contains("-600"), "体積デルタ: {out}");
    // margin変化表: a_wall Pass→Fail
    assert!(out.contains("pass") || out.contains("Pass"), "{out}");
    assert!(out.contains("fail") || out.contains("Fail"), "{out}");
}

#[test]
fn diff_json_is_structured_and_deterministic() {
    let p = setup_repo("json");
    let (code, out1, _) = run(&["diff", "HEAD~1", "HEAD", "--design", &p, "--format=json"]);
    assert_eq!(code, 0);
    let (_, out2, _) = run(&["diff", "HEAD~1", "HEAD", "--design", &p, "--format=json"]);
    assert_eq!(out1, out2, "決定的(バイト再現)");

    let v: serde_json::Value = serde_json::from_str(&out1).expect("JSONパース");
    // param変更
    let params = v["params"].as_array().unwrap();
    assert!(params.iter().any(|e| e["id"] == "t" && e["change"] == "changed"));
    // 制約差分(追加、rationale込み)
    let asserts = v["assertions"].as_array().unwrap();
    let added = asserts.iter().find(|e| e["id"] == "b_mass").unwrap();
    assert_eq!(added["change"], "added");
    assert!(added["rationale_note"].as_str().unwrap().contains("質量上限"));
    assert!(asserts.iter().any(|e| e["id"] == "a_wall" && e["change"] == "changed"));
    // 体積差
    let vols = v["volumes"].as_array().unwrap();
    let plate = vols.iter().find(|e| e["part"] == "plate").unwrap();
    assert_eq!(plate["old_mm3"], 2400.0);
    assert_eq!(plate["new_mm3"], 1800.0);
    assert_eq!(plate["delta_mm3"], -600.0);
    // margin変化表
    let margins = v["margins"].as_array().unwrap();
    let aw = margins.iter().find(|e| e["assert_id"] == "a_wall").unwrap();
    assert_eq!(aw["old"]["status"], "pass");
    assert_eq!(aw["new"]["status"], "fail");
    assert!(aw["new"]["margin"].as_f64().unwrap() < 0.0);
    let bm = margins.iter().find(|e| e["assert_id"] == "b_mass").unwrap();
    assert!(bm["old"].is_null(), "旧版に存在しない制約: {bm}");
}
