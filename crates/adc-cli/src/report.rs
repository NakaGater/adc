//! `adc report [<results.jsonl>]` (M4-4, US-27)。
//!
//! results.jsonl から margin一覧のMarkdownテーブルを生成する(PRコメント/
//! GitHub Actions Step Summary用の顔 — 08-workflows.md)。
//! 整形品質の要: **Fail先頭 → margin昇順**(最悪のものが最初に目に入る)。
//! exit: 0=成功 / 2=E-*(入力が読めない・パースできない)

use std::process::ExitCode;

use adc_check::{CheckResult, CheckStatus};

fn status_cell(s: &CheckStatus) -> String {
    match s {
        CheckStatus::Pass => "✅ Pass".into(),
        CheckStatus::Fail => "❌ Fail".into(),
        CheckStatus::Inconclusive { .. } => "⚠️ Inconclusive".into(),
    }
}

fn value_cell(v: &adc_check::Value) -> String {
    match v {
        adc_check::Value::Scalar(x) => x.to_string(),
        adc_check::Value::Triple(t) => format!("({}, {}, {})", t[0], t[1], t[2]),
        adc_check::Value::None => "-".into(),
    }
}

/// Markdownテーブルのセル用エスケープ+要約(長いnoteは切る)
fn escape_cell(s: &str, max: usize) -> String {
    let s = s.replace('|', "\\|").replace('\n', " ");
    if s.chars().count() > max {
        let cut: String = s.chars().take(max).collect();
        format!("{cut}…")
    } else {
        s
    }
}

fn evidence_cell(r: &CheckResult) -> String {
    if let CheckStatus::Inconclusive { reason } = &r.status {
        return escape_cell(reason, 120);
    }
    let notes: Vec<String> = r.evidence.iter().map(|e| e.note.clone()).collect();
    escape_cell(&notes.join(" / "), 120)
}

/// Fail先頭 → margin昇順(08-workflows.md 受入観点)。
/// グループ順: Fail → Inconclusive → Pass。同margin・同グループはassert_id昇順
fn sort_for_report(results: &mut [CheckResult]) {
    fn group(s: &CheckStatus) -> u8 {
        match s {
            CheckStatus::Fail => 0,
            CheckStatus::Inconclusive { .. } => 1,
            CheckStatus::Pass => 2,
        }
    }
    results.sort_by(|a, b| {
        group(&a.status)
            .cmp(&group(&b.status))
            .then(a.margin.partial_cmp(&b.margin).unwrap_or(std::cmp::Ordering::Equal))
            .then(a.assert_id.cmp(&b.assert_id))
    });
}

pub fn to_markdown(results: &[CheckResult]) -> String {
    let mut rs: Vec<CheckResult> = results.to_vec();
    sort_for_report(&mut rs);

    let n_fail = rs.iter().filter(|r| matches!(r.status, CheckStatus::Fail)).count();
    let n_inc = rs
        .iter()
        .filter(|r| matches!(r.status, CheckStatus::Inconclusive { .. }))
        .count();
    let n_pass = rs.iter().filter(|r| matches!(r.status, CheckStatus::Pass)).count();

    let mut out = String::new();
    out.push_str("# ADC 検証レポート\n\n");
    out.push_str(&format!(
        "**{n_fail} Fail / {n_inc} Inconclusive / {n_pass} Pass**(全{}件)\n\n",
        rs.len()
    ));
    out.push_str("| status | assert_id | checker | measured | threshold | margin | evidence |\n");
    out.push_str("|---|---|---|---|---|---|---|\n");
    for r in &rs {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} |\n",
            status_cell(&r.status),
            escape_cell(&r.assert_id, 40),
            escape_cell(&r.checker, 30),
            value_cell(&r.measured),
            value_cell(&r.threshold),
            r.margin,
            evidence_cell(r),
        ));
    }
    // 3点評価の標本内訳(Openあり設計のみ — 05-schema.md §6.1)
    let sampled: Vec<&CheckResult> = rs.iter().filter(|r| !r.samples.is_empty()).collect();
    if !sampled.is_empty() {
        out.push_str("\n<details><summary>Open 3点評価の標本内訳</summary>\n\n");
        out.push_str("| assert_id | param | sample | status | measured |\n");
        out.push_str("|---|---|---|---|---|\n");
        for r in &sampled {
            for s in &r.samples {
                out.push_str(&format!(
                    "| {} | {} | {} | {} | {} |\n",
                    escape_cell(&r.assert_id, 40),
                    escape_cell(&s.param, 30),
                    s.sample,
                    status_cell(&s.status),
                    value_cell(&s.measured),
                ));
            }
        }
        out.push_str("\n</details>\n");
    }
    out
}

pub fn report_cmd(args: &[String]) -> Result<ExitCode, String> {
    if args.iter().any(|a| a == "-h" || a == "--help") {
        print!("Usage: adc report [<results.jsonl>]\n\nmargin一覧のMarkdownテーブル(Fail先頭→margin昇順)。exit: 0=成功 / 2=E-*\n");
        return Ok(ExitCode::SUCCESS);
    }
    let mut path = "results.jsonl".to_string();
    let mut positional_seen = false;
    for a in args {
        match a.as_str() {
            other if !other.starts_with('-') && !positional_seen => {
                path = other.to_string();
                positional_seen = true;
            }
            other => return Err(format!("不明な引数: {other}")),
        }
    }
    let text = std::fs::read_to_string(&path).map_err(|e| {
        format!(
            "{path} を読めません: {e} — ヒント: `adc check --design design.ron --format=jsonl > results.jsonl` で生成できます"
        )
    })?;
    let mut results = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let r: CheckResult = serde_json::from_str(line)
            .map_err(|e| format!("{path}:{} をパースできません: {e}", i + 1))?;
        results.push(r);
    }
    print!("{}", to_markdown(&results));
    Ok(ExitCode::SUCCESS)
}
