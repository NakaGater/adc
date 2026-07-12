//! adc-mcp コア (M6-1, US-28, docs/design-notes/m6-1-mcp-server.md)。
//!
//! MCPプロトコル層(main.rs / rmcp)から独立した純ロジック。
//! - サーバーは1設計に束縛(design_path)。ツール引数にパスは持たせない
//! - 出力は既存の正準構造(CheckResult / ExplainOutput / ValidationError)を
//!   そのままJSONで返す。新しい表現形式を発明しない
//! - 失敗はプロトコルエラーにせず `{error: {code: "E-...", ...}}` を正常応答で返す
//! - --gated: patchは全Pass(exit 0)時のみ書き込み。用途は無人・自動適用の
//!   安全装置(対話セッションは非gated=人間のPRレビューがゲート — 2026-07-12修正②)

use std::path::PathBuf;

use adc_check::{
    exit_code, run_checks_interval, run_checks_narrow_structured, CheckOptions, CheckResult,
    CheckStatus,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub struct AdcCore {
    pub design_path: PathBuf,
    pub gated: bool,
}

/// design_patch の1編集(完全一致文字列置換)
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Edit {
    pub old_string: String,
    pub new_string: String,
}

fn sha256_hex(s: &str) -> String {
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    format!("{:x}", h.finalize())
}

fn to_value<T: serde::Serialize>(v: &T) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}

fn status_label(s: &CheckStatus) -> &'static str {
    match s {
        CheckStatus::Pass => "pass",
        CheckStatus::Fail => "fail",
        CheckStatus::Inconclusive { .. } => "inconclusive",
    }
}

impl AdcCore {
    pub fn new(design_path: impl Into<PathBuf>, gated: bool) -> Self {
        Self {
            design_path: design_path.into(),
            gated,
        }
    }

    /// CLIと同じキャッシュ配置: design親/.adc/cache
    fn opts(&self, no_cache: bool) -> CheckOptions {
        CheckOptions {
            cache_dir: if no_cache {
                None
            } else {
                self.design_path.parent().map(|d| d.join(".adc").join("cache"))
            },
        }
    }

    fn load(&self) -> Result<String, Value> {
        std::fs::read_to_string(&self.design_path).map_err(|e| {
            json!({"error": {"code": "E-IO", "message": format!("{} を読めません: {e}", self.design_path.display())}})
        })
    }

    fn check_of(&self, src: &str, no_cache: bool) -> Result<(u8, Vec<CheckResult>, Value), Value> {
        let design = match adc_schema::validate_design(src) {
            Ok(d) => d,
            Err(errs) => {
                return Err(json!({"error": {"code": "E-SCHEMA", "errors": to_value(&errs)}}))
            }
        };
        let (results, _, _, dof) = run_checks_interval(&design, &self.opts(no_cache));
        let code = exit_code(&results);
        let dof_v: Vec<Value> = dof
            .iter()
            .map(|(i, r, n)| json!({"instance": i, "remaining": r, "note": n}))
            .collect();
        Ok((code, results, Value::Array(dof_v)))
    }

    // ---------------------------------------------------------------- tools

    pub fn design_read(&self) -> Value {
        let source = match self.load() {
            Ok(s) => s,
            Err(e) => return e,
        };
        let (valid, errors) = match adc_schema::validate_design(&source) {
            Ok(_) => (true, Value::Array(vec![])),
            Err(errs) => (false, to_value(&errs)),
        };
        json!({"source": source, "sha256": sha256_hex(&source), "valid": valid, "errors": errors})
    }

    pub fn design_patch(
        &self,
        base_sha256: &str,
        edits: Option<Vec<Edit>>,
        full_source: Option<String>,
        dry_run: bool,
    ) -> Value {
        let source = match self.load() {
            Ok(s) => s,
            Err(e) => return e,
        };
        let current = sha256_hex(&source);
        if current != base_sha256 {
            return json!({"applied": false, "rejected_reason": "conflict", "current_sha256": current});
        }
        // 表現: edits XOR full_source (設計メモ§1)
        let new_source = match (edits, full_source) {
            (Some(_), Some(_)) | (None, None) => {
                return json!({"error": {"code": "E-PATCH", "kind": "invalid_args",
                    "message": "edits と full_source はどちらか一方を指定すること"}})
            }
            (None, Some(f)) => f,
            (Some(edits), None) => {
                let mut s = source.clone();
                for (i, e) in edits.iter().enumerate() {
                    // 一意一致必須 (2026-07-12承認時修正①): 0件/複数件は適用曖昧性として拒否
                    let occurrences = s.matches(&e.old_string).count();
                    if occurrences != 1 {
                        let kind = if occurrences == 0 { "not_found" } else { "ambiguous" };
                        return json!({"error": {"code": "E-PATCH", "kind": kind,
                            "edit_index": i, "occurrences": occurrences,
                            "message": format!("edits[{i}] の old_string が {occurrences} 箇所に一致(1箇所であること)")}});
                    }
                    s = s.replacen(&e.old_string, &e.new_string, 1);
                }
                s
            }
        };
        // 静的検証NG → 適用しない
        if let Err(errs) = adc_schema::validate_design(&new_source) {
            return json!({"applied": false, "validation": {"ok": false, "errors": to_value(&errs)}});
        }
        // gated: 全Pass(exit 0)のときのみ書き込み(無人・自動適用の安全装置)
        let gated_check = if self.gated {
            match self.check_of(&new_source, false) {
                Ok((code, results, _)) => {
                    if code != 0 {
                        return json!({"applied": false, "validation": {"ok": true},
                            "rejected_reason": "gated_fail",
                            "gated_check": {"exit_code": code, "results": to_value(&results)}});
                    }
                    Some(json!({"exit_code": code, "results": to_value(&results)}))
                }
                Err(e) => return e,
            }
        } else {
            None
        };
        let new_sha = sha256_hex(&new_source);
        if dry_run {
            return json!({"applied": false, "dry_run": true, "validation": {"ok": true},
                "would_sha256": new_sha, "gated_check": gated_check});
        }
        if let Err(e) = std::fs::write(&self.design_path, &new_source) {
            return json!({"error": {"code": "E-IO", "message": format!("書き込み失敗: {e}")}});
        }
        json!({"applied": true, "new_sha256": new_sha, "validation": {"ok": true},
            "gated_check": gated_check})
    }

    pub fn build_and_check(
        &self,
        narrow: bool,
        filter: Option<Vec<String>>,
        no_cache: bool,
    ) -> Value {
        let source = match self.load() {
            Ok(s) => s,
            Err(e) => return e,
        };
        let design = match adc_schema::validate_design(&source) {
            Ok(d) => d,
            Err(errs) => {
                return json!({"error": {"code": "E-SCHEMA", "errors": to_value(&errs)}})
            }
        };
        let opts = self.opts(no_cache);
        let ((mut results, _, _, dof), suggestions) = if narrow {
            run_checks_narrow_structured(&design, &opts, None)
        } else {
            (run_checks_interval(&design, &opts), vec![])
        };
        if let Some(f) = &filter {
            results.retain(|r| f.contains(&r.assert_id));
        }
        let dof_v: Vec<Value> = dof
            .iter()
            .map(|(i, r, n)| json!({"instance": i, "remaining": r, "note": n}))
            .collect();
        let mut out = json!({"exit_code": exit_code(&results), "results": to_value(&results), "dof": dof_v});
        if narrow {
            out["suggestions"] = to_value(&suggestions);
        }
        out
    }

    pub fn evidence_query(&self, assert_id: Option<&str>, status: Option<&str>) -> Value {
        let source = match self.load() {
            Ok(s) => s,
            Err(e) => return e,
        };
        match self.check_of(&source, false) {
            Ok((_, results, _)) => {
                let filtered: Vec<&CheckResult> = results
                    .iter()
                    .filter(|r| assert_id.is_none_or(|id| r.assert_id == id))
                    .filter(|r| status.is_none_or(|s| status_label(&r.status) == s))
                    .collect();
                json!({"results": to_value(&filtered)})
            }
            Err(e) => e,
        }
    }

    pub fn narrow_param(&self, param: Option<&str>) -> Value {
        let source = match self.load() {
            Ok(s) => s,
            Err(e) => return e,
        };
        let design = match adc_schema::validate_design(&source) {
            Ok(d) => d,
            Err(errs) => {
                return json!({"error": {"code": "E-SCHEMA", "errors": to_value(&errs)}})
            }
        };
        let ((results, _, _, _), suggestions) =
            run_checks_narrow_structured(&design, &self.opts(false), param);
        json!({"exit_code": exit_code(&results), "suggestions": to_value(&suggestions),
            "results": to_value(&results)})
    }

    pub fn explain(&self, id: &str) -> Value {
        let source = match self.load() {
            Ok(s) => s,
            Err(e) => return e,
        };
        match adc_schema::validate_design(&source) {
            Ok(d) => to_value(&adc_schema::explain(&d, id)),
            Err(errs) => json!({"error": {"code": "E-SCHEMA", "errors": to_value(&errs)}}),
        }
    }
}
