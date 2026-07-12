//! M6-0 受入テスト: 実験フォローアップ小修正(CLI分)。
//!
//! 1. --help/-h/--version(clap慣習準拠)
//! 2. cacheログは-v時のみ(既定は静粛)
//! 3. report: 入力不在時のエラーに生成コマンドのヒント

use std::process::Command;

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

fn write_design(name: &str) -> String {
    let dir = std::env::temp_dir().join("adc-m6-0-cli");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(
        &path,
        r#"Design(
    schema_version: "0.1",
    intent: "m6-0 fixture",
    params: [],
    materials: [Material(id: "a5052", density_g_cm3: 2.68, name: "A5052")],
    parts: [
        Part(id: "p1", material: "a5052", process: Machining,
            features: [Block(id: "base", x: 30.0, y: 60.0, z: 4.0)],
            anchors: []),
    ],
    assertions: [Assertion(id: "a_ok", check: BoundingBox(part: "p1", max: (40.0, 70.0, 5.0)), rationale: "r0")],
    rationales: [Rationale(id: "r0", author: Human("t"), basis: Assumption, note: "", timestamp: "2026-07-12T00:00:00Z")],
)"#,
    )
    .unwrap();
    path.to_str().unwrap().to_string()
}

#[test]
fn help_and_version_follow_clap_conventions() {
    for flag in ["--help", "-h", "help"] {
        let (code, out, _) = run(&[flag]);
        assert_eq!(code, 0, "{flag}");
        for sub in ["check", "explain", "export", "diff", "report"] {
            assert!(out.contains(sub), "{flag}: サブコマンド {sub} が載る\n{out}");
        }
        assert!(out.to_lowercase().contains("usage"), "{out}");
    }
    for flag in ["--version", "-V"] {
        let (code, out, _) = run(&[flag]);
        assert_eq!(code, 0, "{flag}");
        assert!(out.starts_with("adc "), "{out}");
        assert!(out.contains(env!("CARGO_PKG_VERSION")), "{out}");
    }
    // サブコマンドの--help
    for sub in ["check", "explain", "export", "diff", "report"] {
        let (code, out, _) = run(&[sub, "--help"]);
        assert_eq!(code, 0, "{sub} --help");
        assert!(out.to_lowercase().contains("usage"), "{sub}: {out}");
    }
    // 引数なしはこれまでどおりusageエラー(exit 2)
    let (code, _, err) = run(&[]);
    assert_eq!(code, 2);
    assert!(err.contains("usage"), "{err}");
}

#[test]
fn cache_log_is_quiet_by_default_and_shown_with_verbose() {
    let p = write_design("quiet.ron");
    let (code, _, err) = run(&["check", "--design", &p]);
    assert_eq!(code, 0);
    assert!(!err.contains("cache"), "既定は静粛: {err}");

    let (code, _, err) = run(&["check", "--design", &p, "-v"]);
    assert_eq!(code, 0);
    assert!(err.contains("cache"), "-vでcacheログ: {err}");

    let (code, _, err) = run(&["check", "--design", &p, "--verbose"]);
    assert_eq!(code, 0);
    assert!(err.contains("cache"), "--verboseでも: {err}");
}

#[test]
fn report_missing_input_shows_generation_hint() {
    let (code, _, err) = run(&["report", "/nonexistent/results.jsonl"]);
    assert_eq!(code, 2);
    assert!(
        err.contains("adc check") && err.contains("--format=jsonl"),
        "生成コマンドのヒント: {err}"
    );
}
