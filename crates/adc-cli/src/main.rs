//! adc CLI (07-cli.md)。stdoutはデータ、stderrはログを厳守する。
//!
//! サブコマンド:
//! - `adc explain <id> [--design <path>] [--format=json]`
//!   exit: 0=一意に解決 / 1=not_found・ambiguous / 2=E-*エラー (docs/explain-schema.md)
//! - `adc export --step [--design <path>] [--out <dir>]`(M1-6)
//!   部品ごとに <out>/<part_id>.step を出力(既定スキーマAP214 — M1-6緩和)。
//!   exit: 0=成功 / 2=E-*エラー
//! - `adc check [--design <path>] [--format=jsonl|text] [--filter <id,..>] [--timings] [--no-cache] [--narrow]`(M2-1/M2-6/M4)
//!   stdout=results.jsonl(正準・決定的)またはtext。timingsはstderrのみ。
//!   Open含み設計は3点評価(M4-1)。--narrowはsuggested_range付加(M4-2)。
//!   exit: 0=全Pass / 1=Fail≥1 / 2=Inconclusive≥1またはE-*
//! - `adc diff <rev1> <rev2> [--design <path>] [--format=text|json]`(M4-3)
//!   制約差分(rationale込み)+param変更+体積差+margin変化表。exit: 0=成功 / 2=E-*
//! - `adc report [<results.jsonl>]`(M4-4)
//!   margin一覧のMarkdownテーブル(Fail先頭→margin昇順)。exit: 0=成功 / 2=E-*

use std::process::ExitCode;

mod diff;
mod report;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(code) => code,
        Err(msg) => {
            eprintln!("adc: {msg}");
            ExitCode::from(2)
        }
    }
}

const TOP_HELP: &str = "\
adc — AI-native Declarative CAD (07-cli.md)

Usage: adc <SUBCOMMAND> [OPTIONS]

Subcommands:
  check    コンパイル+全検証 → results.jsonl(Open含みは3点評価)
  explain  IDの定義・根拠・参照元をJSONで
  export   STEPエクスポート
  diff     2版(git rev)の制約/param/体積/margin差分
  report   results.jsonl → Markdownのmargin表(Fail先頭→margin昇順)
  help     このヘルプ

Options:
  -h, --help     ヘルプを表示
  -V, --version  バージョンを表示

exit code: 0=成功(checkは全Pass) / 1=Fail≥1(check) / 2=Inconclusive≥1またはE-*
";

/// サブコマンド共通: -h/--help でusageを表示して終了
fn wants_help(args: &[String]) -> bool {
    args.iter().any(|a| a == "-h" || a == "--help")
}

fn run(args: &[String]) -> Result<ExitCode, String> {
    match args.first().map(String::as_str) {
        Some("-h" | "--help" | "help") => {
            print!("{TOP_HELP}");
            Ok(ExitCode::SUCCESS)
        }
        Some("-V" | "--version") => {
            println!("adc {}", env!("CARGO_PKG_VERSION"));
            Ok(ExitCode::SUCCESS)
        }
        Some("explain") => explain_cmd(&args[1..]),
        Some("export") => export_cmd(&args[1..]),
        Some("check") => check_cmd(&args[1..]),
        Some("diff") => diff::diff_cmd(&args[1..]),
        Some("report") => report::report_cmd(&args[1..]),
        Some(other) => Err(format!(
            "未知のサブコマンド: {other}(使えるのは explain / export / check / diff / report。adc --help参照)"
        )),
        None => Err("usage: adc <explain|export|check|diff|report> ...(adc --help参照)".to_string()),
    }
}

fn explain_cmd(args: &[String]) -> Result<ExitCode, String> {
    if wants_help(args) {
        print!("Usage: adc explain <id> [--design <path>] [--format=json]\n\n定義+rationale連鎖+参照元(referenced_by/related)。板金partはderived(展開長)。\nexit: 0=一意に解決 / 1=not_found・ambiguous / 2=E-*\n");
        return Ok(ExitCode::SUCCESS);
    }
    let mut id: Option<&str> = None;
    let mut design_path = "./design.ron".to_string();
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
                if val != "json" {
                    return Err(format!("explain の出力は json のみ対応です: {val}"));
                }
                i += if f == "--format" { 2 } else { 1 };
            }
            other if !other.starts_with('-') && id.is_none() => {
                id = Some(other);
                i += 1;
            }
            other => return Err(format!("不明な引数: {other}")),
        }
    }
    let id = id.ok_or("usage: adc explain <id> [--design <path>] [--format=json]")?;

    let src = std::fs::read_to_string(&design_path)
        .map_err(|e| format!("{design_path} を読めません: {e}"))?;

    match adc_schema::validate_design(&src) {
        Err(errs) => {
            // スキーマエラーも構造化データとしてstdoutへ (07-cli.md 出力契約)
            let json = serde_json::to_string_pretty(&errs).map_err(|e| e.to_string())?;
            println!("{json}");
            Ok(ExitCode::from(2))
        }
        Ok(design) => {
            let out = adc_schema::explain(&design, id);
            let json = serde_json::to_string_pretty(&out).map_err(|e| e.to_string())?;
            println!("{json}");
            Ok(match out.status {
                adc_schema::ExplainStatus::Found => ExitCode::SUCCESS,
                _ => ExitCode::from(1),
            })
        }
    }
}


fn export_cmd(args: &[String]) -> Result<ExitCode, String> {
    if wants_help(args) {
        print!("Usage: adc export --step [--design <path>] [--out <dir>]\n\n部品ごとに <out>/<part_id>.step を出力(AP214)。exit: 0=成功 / 2=E-*\n");
        return Ok(ExitCode::SUCCESS);
    }
    let mut design_path = "./design.ron".to_string();
    let mut out_dir = "./out".to_string();
    let mut step = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--step" => {
                step = true;
                i += 1;
            }
            "--design" => {
                design_path = args
                    .get(i + 1)
                    .ok_or("--design にはパスが必要です")?
                    .clone();
                i += 2;
            }
            "--out" => {
                out_dir = args.get(i + 1).ok_or("--out にはパスが必要です")?.clone();
                i += 2;
            }
            other => return Err(format!("不明な引数: {other}")),
        }
    }
    if !step {
        return Err("usage: adc export --step [--design <path>] [--out <dir>]".to_string());
    }

    let src = std::fs::read_to_string(&design_path)
        .map_err(|e| format!("{design_path} を読めません: {e}"))?;
    let design = match adc_schema::validate_design(&src) {
        Ok(d) => d,
        Err(errs) => {
            let json = serde_json::to_string_pretty(&errs).map_err(|e| e.to_string())?;
            println!("{json}");
            return Ok(ExitCode::from(2));
        }
    };
    std::fs::create_dir_all(&out_dir).map_err(|e| format!("{out_dir}: {e}"))?;

    for part in &design.parts {
        let cp = adc_compile::compile_part(&design, &part.id, &adc_schema::EvalContext::nominal())
            .map_err(|e| format!("part \"{}\" のコンパイル失敗: {e}", part.id))?;
        let path = format!("{out_dir}/{}.step", part.id);
        cp.solid.write_step(&path).map_err(|e| format!("{path}: {e}"))?;
        println!("wrote {path}");
    }
    Ok(ExitCode::SUCCESS)
}

fn check_cmd(args: &[String]) -> Result<ExitCode, String> {
    if wants_help(args) {
        print!("Usage: adc check [--design <path>] [--format=jsonl|text] [--filter <id,..>] [--timings] [--no-cache] [--narrow] [-v|--verbose]\n\nコンパイル+全検証。Open含みは自動3点評価。--narrowは片端Fail軸のsuggested_range付加。\n-vでcacheログをstderrへ(既定は静粛)。exit: 0=全Pass / 1=Fail≥1 / 2=Inconclusive≥1またはE-*\n");
        return Ok(ExitCode::SUCCESS);
    }
    let mut design_path = "./design.ron".to_string();
    let mut format = "text".to_string();
    let mut filter: Option<Vec<String>> = None;
    let mut timings = false;
    let mut no_cache = false;
    let mut narrow = false;
    let mut verbose = false;
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
            "--filter" => {
                let v = args.get(i + 1).ok_or("--filter にはID列が必要です")?;
                filter = Some(v.split(',').map(|s| s.trim().to_string()).collect());
                i += 2;
            }
            "--timings" => {
                timings = true;
                i += 1;
            }
            "--no-cache" => {
                no_cache = true;
                i += 1;
            }
            "--narrow" => {
                narrow = true;
                i += 1;
            }
            "-v" | "--verbose" => {
                verbose = true;
                i += 1;
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
                if val != "jsonl" && val != "text" {
                    return Err(format!("checkの出力は jsonl | text: {val}"));
                }
                format = val;
                i += if f == "--format" { 2 } else { 1 };
            }
            other => return Err(format!("不明な引数: {other}")),
        }
    }

    let src = std::fs::read_to_string(&design_path)
        .map_err(|e| format!("{design_path} を読めません: {e}"))?;
    let design = match adc_schema::validate_design(&src) {
        Ok(d) => d,
        Err(errs) => {
            let json = serde_json::to_string_pretty(&errs).map_err(|e| e.to_string())?;
            println!("{json}");
            return Ok(ExitCode::from(2));
        }
    };

    // キャッシュ (M2-6): 既定は design と同じ場所の .adc/cache
    let cache_dir = if no_cache {
        None
    } else {
        std::path::Path::new(&design_path)
            .parent()
            .map(|d| d.join(".adc").join("cache"))
    };
    let opts = adc_check::CheckOptions { cache_dir };
    // Open含み設計は3点評価 (M4-1, ADR-004)。Openなしなら公称のみと同一。
    // --narrow は片端Failの軸を二分探索し suggested_range を付加 (M4-2)
    let (mut results, times, events, dof) = if narrow {
        adc_check::run_checks_narrow(&design, &opts)
    } else {
        adc_check::run_checks_interval(&design, &opts)
    };
    // 残自由度レポート (M3-2、未拘束=正常・報告のみ) — stderr
    for (inst, remaining, note) in &dof {
        eprintln!("dof\t{inst}\t残{remaining}\t{note}");
    }
    // cacheログは-v時のみ(既定は静粛 — M6-0、成功基準3予備実験の摩擦点②)
    if verbose {
        for ev in &events {
            match ev {
                adc_check::CacheEvent::PartHit(id) => eprintln!("cache	part:{id}	hit"),
                adc_check::CacheEvent::PartCompiled(id) => eprintln!("cache	part:{id}	compiled"),
                adc_check::CacheEvent::ResultHit(id) => eprintln!("cache	result:{id}	hit"),
                adc_check::CacheEvent::ResultComputed(id) => {
                    eprintln!("cache	result:{id}	computed")
                }
            }
        }
    }
    if let Some(f) = &filter {
        results.retain(|r| f.contains(&r.assert_id));
    }
    if timings {
        for (id, ms) in &times {
            if filter.as_ref().is_none_or(|f| f.contains(id)) {
                eprintln!("timing	{id}	{ms:.3}ms");
            }
        }
    }

    match format.as_str() {
        "jsonl" => print!("{}", adc_check::to_jsonl(&results)),
        _ => {
            for r in &results {
                match &r.status {
                    adc_check::CheckStatus::Pass => {
                        println!("[PASS] {} margin={}", r.assert_id, r.margin)
                    }
                    adc_check::CheckStatus::Fail => {
                        println!("[FAIL] {} margin={}", r.assert_id, r.margin);
                        for ev in &r.evidence {
                            println!("       {} {:?}", ev.note, ev.anchors);
                        }
                    }
                    adc_check::CheckStatus::Inconclusive { reason } => {
                        println!("[INCONCLUSIVE] {}: {reason}", r.assert_id)
                    }
                }
            }
        }
    }
    Ok(ExitCode::from(adc_check::exit_code(&results)))
}