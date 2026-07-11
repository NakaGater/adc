//! adc CLI (07-cli.md)。stdoutはデータ、stderrはログを厳守する。
//!
//! M0-4時点のサブコマンド: `adc explain <id> [--design <path>] [--format=json]`
//! exit code (explain): 0=一意に解決 / 1=not_found・ambiguous / 2=E-*エラー
//! (docs/explain-schema.md)

use std::process::ExitCode;

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

fn run(args: &[String]) -> Result<ExitCode, String> {
    match args.first().map(String::as_str) {
        Some("explain") => explain_cmd(&args[1..]),
        Some(other) => Err(format!(
            "未知のサブコマンド: {other}(M0-4時点で使えるのは explain のみ。07-cli.md参照)"
        )),
        None => Err("usage: adc explain <id> [--design <path>] [--format=json]".to_string()),
    }
}

fn explain_cmd(args: &[String]) -> Result<ExitCode, String> {
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
