//! `adc diff <rev1> <rev2>` (M4-3, US-10)。
//!
//! git経由で2版のdesign.ronを取得(`git show <rev>:<path>` — gitはCLI外部コマンド可)し、
//! 制約差分(追加/削除/変更、rationale込み)・param変更・体積差(両版build)・
//! margin変化表(両版check)を決定的順序で出力する。--format=text|json。
//! exit: 0=成功 / 2=E-*エラー(git取得失敗・検証失敗を含む)

use std::collections::BTreeSet;
use std::process::{Command, ExitCode};

use adc_check::{q, run_checks_interval, CheckOptions, CheckResult, CheckStatus};
use adc_schema::{Design, EvalContext};
use serde::Serialize;

#[derive(Serialize)]
struct ParamDiff {
    id: String,
    change: &'static str, // added | removed | changed
    #[serde(skip_serializing_if = "Option::is_none")]
    old: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    new: Option<String>,
}

#[derive(Serialize)]
struct AssertDiff {
    id: String,
    change: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    old: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    new: Option<String>,
    /// 制約差分はrationale込みで提示する(新版優先、削除は旧版)
    rationale_note: String,
}

#[derive(Serialize)]
struct VolumeDiff {
    part: String,
    old_mm3: Option<f64>,
    new_mm3: Option<f64>,
    delta_mm3: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    note: Option<String>,
}

#[derive(Serialize)]
struct MarginEntry {
    status: CheckStatus,
    margin: f64,
}

#[derive(Serialize)]
struct MarginDiff {
    assert_id: String,
    old: Option<MarginEntry>,
    new: Option<MarginEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    delta: Option<f64>,
}

#[derive(Serialize)]
struct DiffReport {
    rev1: String,
    rev2: String,
    params: Vec<ParamDiff>,
    assertions: Vec<AssertDiff>,
    volumes: Vec<VolumeDiff>,
    margins: Vec<MarginDiff>,
}

/// `git show <rev>:<repo相対パス>` で当該版のdesign.ronテキストを得る
fn git_show(design_path: &std::path::Path, rev: &str) -> Result<String, String> {
    let dir = design_path.parent().filter(|p| !p.as_os_str().is_empty());
    let dir = dir.map(|p| p.to_path_buf()).unwrap_or_else(|| ".".into());
    let fname = design_path
        .file_name()
        .ok_or("designパスにファイル名がありません")?
        .to_string_lossy()
        .to_string();
    let prefix = Command::new("git")
        .arg("-C")
        .arg(&dir)
        .args(["rev-parse", "--show-prefix"])
        .output()
        .map_err(|e| format!("gitを実行できません: {e}"))?;
    if !prefix.status.success() {
        return Err(format!(
            "gitリポジトリではありません: {}",
            String::from_utf8_lossy(&prefix.stderr).trim()
        ));
    }
    let prefix = String::from_utf8_lossy(&prefix.stdout).trim().to_string();
    let spec = format!("{rev}:{prefix}{fname}");
    let out = Command::new("git")
        .arg("-C")
        .arg(&dir)
        .args(["show", &spec])
        .output()
        .map_err(|e| format!("gitを実行できません: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git show {spec} に失敗: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    String::from_utf8(out.stdout).map_err(|e| format!("{spec} がUTF-8ではありません: {e}"))
}

fn rationale_note(d: &Design, id: &str) -> String {
    d.rationales
        .iter()
        .find(|r| r.id == id)
        .map(|r| r.note.clone())
        .unwrap_or_default()
}

fn ron_of<T: Serialize>(v: &T) -> String {
    ron::ser::to_string(v).unwrap_or_else(|_| "<serialize error>".into())
}

fn margin_entry(r: &CheckResult) -> MarginEntry {
    MarginEntry {
        status: r.status.clone(),
        margin: r.margin,
    }
}

fn build_report(rev1: &str, rev2: &str, d1: &Design, d2: &Design) -> DiffReport {
    // ---- param変更(id昇順、変更のあるもののみ)
    let mut params = Vec::new();
    let ids: BTreeSet<&str> = d1
        .params
        .iter()
        .chain(d2.params.iter())
        .map(|p| p.id.as_str())
        .collect();
    for id in ids {
        let o = d1.params.iter().find(|p| p.id == id).map(|p| ron_of(&p.value));
        let n = d2.params.iter().find(|p| p.id == id).map(|p| ron_of(&p.value));
        let change = match (&o, &n) {
            (None, Some(_)) => "added",
            (Some(_), None) => "removed",
            (Some(a), Some(b)) if a != b => "changed",
            _ => continue,
        };
        params.push(ParamDiff {
            id: id.to_string(),
            change,
            old: o,
            new: n,
        });
    }

    // ---- 制約差分(id昇順、rationale込み)
    let mut assertions = Vec::new();
    let ids: BTreeSet<&str> = d1
        .assertions
        .iter()
        .chain(d2.assertions.iter())
        .map(|a| a.id.as_str())
        .collect();
    for id in ids {
        let oa = d1.assertions.iter().find(|a| a.id == id);
        let na = d2.assertions.iter().find(|a| a.id == id);
        let key = |a: &adc_schema::Assertion, d: &Design| {
            format!("{} — rationale: {}", ron_of(&a.check), rationale_note(d, &a.rationale))
        };
        let (o, n) = (oa.map(|a| key(a, d1)), na.map(|a| key(a, d2)));
        let change = match (&o, &n) {
            (None, Some(_)) => "added",
            (Some(_), None) => "removed",
            (Some(a), Some(b)) if a != b => "changed",
            _ => continue,
        };
        let note = match (na, oa) {
            (Some(a), _) => rationale_note(d2, &a.rationale),
            (None, Some(a)) => rationale_note(d1, &a.rationale),
            _ => String::new(),
        };
        assertions.push(AssertDiff {
            id: id.to_string(),
            change,
            old: o,
            new: n,
            rationale_note: note,
        });
    }

    // ---- 体積差(両版をbuild、公称値。全部品をid昇順で列挙)
    let mut volumes = Vec::new();
    let ids: BTreeSet<&str> = d1
        .parts
        .iter()
        .chain(d2.parts.iter())
        .map(|p| p.id.as_str())
        .collect();
    let ctx = EvalContext::nominal();
    for id in ids {
        let mut note = None;
        let mut vol = |d: &Design, exists: bool| -> Option<f64> {
            if !exists {
                return None;
            }
            match adc_compile::compile_part(d, id, &ctx) {
                Ok(cp) => Some(q(cp.solid.volume())),
                Err(e) => {
                    note = Some(format!("build失敗: {e}"));
                    None
                }
            }
        };
        let o = vol(d1, d1.parts.iter().any(|p| p.id == id));
        let n = vol(d2, d2.parts.iter().any(|p| p.id == id));
        volumes.push(VolumeDiff {
            part: id.to_string(),
            old_mm3: o,
            new_mm3: n,
            delta_mm3: match (o, n) {
                (Some(a), Some(b)) => Some(q(b - a)),
                _ => None,
            },
            note,
        });
    }

    // ---- margin変化表(両版をcheck — Open含みは3点評価の集約margin)
    let (r1, ..) = run_checks_interval(d1, &CheckOptions::default());
    let (r2, ..) = run_checks_interval(d2, &CheckOptions::default());
    let mut margins = Vec::new();
    let ids: BTreeSet<&str> = r1
        .iter()
        .chain(r2.iter())
        .map(|r| r.assert_id.as_str())
        .collect();
    for id in ids {
        let o = r1.iter().find(|r| r.assert_id == id).map(margin_entry);
        let n = r2.iter().find(|r| r.assert_id == id).map(margin_entry);
        let delta = match (&o, &n) {
            (Some(a), Some(b)) => Some(q(b.margin - a.margin)),
            _ => None,
        };
        margins.push(MarginDiff {
            assert_id: id.to_string(),
            old: o,
            new: n,
            delta,
        });
    }

    DiffReport {
        rev1: rev1.to_string(),
        rev2: rev2.to_string(),
        params,
        assertions,
        volumes,
        margins,
    }
}

fn status_label(s: &CheckStatus) -> String {
    match s {
        CheckStatus::Pass => "Pass".into(),
        CheckStatus::Fail => "Fail".into(),
        CheckStatus::Inconclusive { reason } => format!("Inconclusive({reason})"),
    }
}

fn print_text(rep: &DiffReport) {
    println!("# adc diff {}..{}", rep.rev1, rep.rev2);
    println!("\n## パラメータ変更");
    if rep.params.is_empty() {
        println!("(なし)");
    }
    for p in &rep.params {
        match p.change {
            "added" => println!("+ {}: {}", p.id, p.new.as_deref().unwrap_or("-")),
            "removed" => println!("- {}: {}", p.id, p.old.as_deref().unwrap_or("-")),
            _ => println!(
                "~ {}: {} → {}",
                p.id,
                p.old.as_deref().unwrap_or("-"),
                p.new.as_deref().unwrap_or("-")
            ),
        }
    }
    println!("\n## 制約差分");
    if rep.assertions.is_empty() {
        println!("(なし)");
    }
    for a in &rep.assertions {
        match a.change {
            "added" => println!("+ {}: {}", a.id, a.new.as_deref().unwrap_or("-")),
            "removed" => println!("- {}: {}", a.id, a.old.as_deref().unwrap_or("-")),
            _ => println!(
                "~ {}: {} → {}",
                a.id,
                a.old.as_deref().unwrap_or("-"),
                a.new.as_deref().unwrap_or("-")
            ),
        }
    }
    println!("\n## 体積 [mm³]");
    for v in &rep.volumes {
        let f = |x: Option<f64>| x.map(|v| v.to_string()).unwrap_or_else(|| "-".into());
        let delta = v
            .delta_mm3
            .map(|d| format!("(Δ {d})"))
            .unwrap_or_default();
        let note = v.note.as_deref().unwrap_or("");
        println!("{}: {} → {} {delta}{note}", v.part, f(v.old_mm3), f(v.new_mm3));
    }
    println!("\n## margin変化");
    for m in &rep.margins {
        let f = |e: &Option<MarginEntry>| {
            e.as_ref()
                .map(|e| format!("{}({})", status_label(&e.status), e.margin))
                .unwrap_or_else(|| "-".into())
        };
        let worse = match (&m.old, &m.new) {
            (Some(a), Some(b))
                if matches!(b.status, CheckStatus::Fail)
                    && !matches!(a.status, CheckStatus::Fail) =>
            {
                " ★悪化"
            }
            _ => "",
        };
        println!("{}: {} → {}{worse}", m.assert_id, f(&m.old), f(&m.new));
    }
}

pub fn diff_cmd(args: &[String]) -> Result<ExitCode, String> {
    let mut design_path = "./design.ron".to_string();
    let mut format = "text".to_string();
    let mut revs: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--design" => {
                design_path = args
                    .get(i + 1)
                    .ok_or("--design にはパスが必要です")?
                    .clone();
                i += 2;
            }
            f if f.starts_with("--format") => {
                let val = f
                    .strip_prefix("--format=")
                    .map(str::to_string)
                    .or_else(|| {
                        if f == "--format" {
                            args.get(i + 1).cloned()
                        } else {
                            None
                        }
                    })
                    .ok_or("--format には値が必要です")?;
                if val != "json" && val != "text" {
                    return Err(format!("diffの出力は text | json: {val}"));
                }
                format = val.clone();
                i += if f == "--format" { 2 } else { 1 };
            }
            other if !other.starts_with('-') => {
                revs.push(other.to_string());
                i += 1;
            }
            other => return Err(format!("不明な引数: {other}")),
        }
    }
    let [rev1, rev2] = revs.as_slice() else {
        return Err("usage: adc diff <rev1> <rev2> [--design <path>] [--format=text|json]".into());
    };

    let path = std::path::Path::new(&design_path);
    let src1 = git_show(path, rev1)?;
    let src2 = git_show(path, rev2)?;
    let d1 = adc_schema::validate_design(&src1)
        .map_err(|e| format!("{rev1} の検証に失敗: {}", e.first().map(|x| x.to_string()).unwrap_or_default()))?;
    let d2 = adc_schema::validate_design(&src2)
        .map_err(|e| format!("{rev2} の検証に失敗: {}", e.first().map(|x| x.to_string()).unwrap_or_default()))?;

    let rep = build_report(rev1, rev2, &d1, &d2);
    if format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(&rep).map_err(|e| e.to_string())?
        );
    } else {
        print_text(&rep);
    }
    Ok(ExitCode::SUCCESS)
}
