use std::fmt;

use serde::{Serialize, Serializer};
use thiserror::Error;

/// adc-schema の公開エラー。コード体系は 05-schema.md §8 に従う。
/// 全エラーはJSONで構造化出力可能であること(エージェント修復ループの入力)。
#[derive(Debug, Error)]
pub enum SchemaError {
    /// E-SCHEMA-PARSE: RONの構文エラー・型エラー。行番号付き (US-01)
    #[error("E-SCHEMA-PARSE: {message} (line {line}, column {column})")]
    Parse {
        message: String,
        line: usize,
        column: usize,
    },

    /// シリアライズ失敗(整合した Design 値に対しては到達しない)
    #[error("E-SCHEMA-SERIALIZE: {0}")]
    Serialize(String),
}

/// 静的検証のエラーコード (05-schema.md §8)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorCode {
    /// RON構文・型エラー
    SchemaParse,
    /// 未定義参照 (param/anchor/feature/material/part/instance/dim)
    SchemaRef,
    /// rationale欠落・未定義rationale参照
    SchemaRationale,
    /// 種別内の重複ID (§1.1)
    SchemaDup,
    /// param間の循環参照
    SchemaCycle,
    /// Open範囲の不整合 (nominal ∉ range / min > max)
    SchemaRange,
    /// 式評価の失敗(ゼロ除算・非有限値)。チェッカー文脈ではInconclusive相当
    SchemaEval,
    /// mateグラフの循環・自己参照
    MateCycle,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::SchemaParse => "E-SCHEMA-PARSE",
            ErrorCode::SchemaRef => "E-SCHEMA-REF",
            ErrorCode::SchemaRationale => "E-SCHEMA-RATIONALE",
            ErrorCode::SchemaDup => "E-SCHEMA-DUP",
            ErrorCode::SchemaCycle => "E-SCHEMA-CYCLE",
            ErrorCode::SchemaRange => "E-SCHEMA-RANGE",
            ErrorCode::SchemaEval => "E-SCHEMA-EVAL",
            ErrorCode::MateCycle => "E-MATE-CYCLE",
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for ErrorCode {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

/// エラーの発生位置(元テキストの行・桁)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Span {
    pub line: usize,
    pub column: usize,
}

/// 静的検証の構造化エラー (05-schema.md §8):
/// {code, message, span(行番号), related(関連ID一覧)}。JSONシリアライズ可能。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ValidationError {
    pub code: ErrorCode,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span: Option<Span>,
    pub related: Vec<String>,
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)?;
        if let Some(s) = &self.span {
            write!(f, " (line {}, column {})", s.line, s.column)?;
        }
        Ok(())
    }
}

/// E-ANCHOR-BIND の原因。3値で固定(2026-07-12設計レビュー決定)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum AnchorBindCause {
    /// 参照先の部分形状が後続フィーチャーの操作で消滅した (IsRemoved)
    Deleted,
    /// OCCT History が対応を追跡できなかった(既知の穴 — ADR-002)。
    /// Modified/Generated とも空だが結果に元形状が残っていない場合
    Untracked,
    /// 対応が一意でない(分割等で複数の部分形状に対応)。
    /// 1対1のみを束縛として許容する(2026-07-12決定・案1)
    Ambiguous,
}

/// E-ANCHOR-BIND: アンカー再束縛失敗 (05-schema.md §8, ADR-001)。
/// 黙って壊れる状態遷移を排除するための明示的コンパイルエラー。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AnchorBindError {
    pub anchor_id: String,
    /// 束縛を壊した原因フィーチャー
    pub feature_id: String,
    pub cause: AnchorBindCause,
    pub message: String,
    /// 修復ヒント(Ambiguous では必須: より特定的な面への貼り直し等)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

impl AnchorBindError {
    pub fn deleted(anchor_id: impl Into<String>, feature_id: impl Into<String>) -> Self {
        let (anchor_id, feature_id) = (anchor_id.into(), feature_id.into());
        Self {
            message: format!(
                "アンカー \"{anchor_id}\" の参照先形状はフィーチャー \"{feature_id}\" の操作で消滅しました"
            ),
            anchor_id,
            feature_id,
            cause: AnchorBindCause::Deleted,
            hint: Some("参照先のprovides要素を、操作後も残る面に変更するか、原因フィーチャーの定義を見直してください".into()),
        }
    }

    pub fn untracked(anchor_id: impl Into<String>, feature_id: impl Into<String>) -> Self {
        let (anchor_id, feature_id) = (anchor_id.into(), feature_id.into());
        Self {
            message: format!(
                "アンカー \"{anchor_id}\" の対応をフィーチャー \"{feature_id}\" の操作を跨いで追跡できませんでした(OCCT Historyの既知の制約)"
            ),
            anchor_id,
            feature_id,
            cause: AnchorBindCause::Untracked,
            hint: Some("追跡可能な別のprovides要素へアンカーを貼り直してください".into()),
        }
    }

    /// Ambiguous は修復ヒントを必ず含む(2026-07-12決定)
    pub fn ambiguous(
        anchor_id: impl Into<String>,
        feature_id: impl Into<String>,
        candidates: usize,
    ) -> Self {
        let (anchor_id, feature_id) = (anchor_id.into(), feature_id.into());
        Self {
            message: format!(
                "アンカー \"{anchor_id}\" の参照先はフィーチャー \"{feature_id}\" の操作で{candidates}個の面に分割され、一意に対応しません"
            ),
            anchor_id,
            feature_id,
            cause: AnchorBindCause::Ambiguous,
            hint: Some(format!(
                "分割後のいずれか1面を特定できる、より特定的なprovides要素へアンカーを貼り直してください(候補{candidates}面)。分割を意図しない場合は原因フィーチャーの配置・寸法を見直してください"
            )),
        }
    }
}

/// E-FEATURE-FAIL: OCCT操作失敗 {feature_id, occt_error, hint} (05-schema.md §8)。
/// プロセスをabortさせず、エージェント修復ループの入力として返す (US-08)。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FeatureFailError {
    pub feature_id: String,
    /// OCCT側のエラーメッセージ
    pub occt_error: String,
    /// 修復の示唆(半径過大の可能性等)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

impl fmt::Display for FeatureFailError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "E-FEATURE-FAIL: \"{}\": {}",
            self.feature_id, self.occt_error
        )?;
        if let Some(h) = &self.hint {
            write!(f, " — ヒント: {h}")?;
        }
        Ok(())
    }
}

impl fmt::Display for AnchorBindError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "E-ANCHOR-BIND: {} (anchor: {}, feature: {}, cause: {:?})",
            self.message, self.anchor_id, self.feature_id, self.cause
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchor_bind_error_is_structured_and_json_serializable() {
        let e = AnchorBindError::ambiguous("bearing_bore", "pocket1", 2);
        assert_eq!(e.cause, AnchorBindCause::Ambiguous);
        assert!(e.hint.as_deref().unwrap().contains("貼り直し"), "{e:?}");
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("\"Ambiguous\""), "{json}");
        assert!(json.contains("\"hint\""), "{json}");
        assert!(e.to_string().contains("E-ANCHOR-BIND"));

        // 3値のシリアライズ表現
        for (cause, s) in [
            (AnchorBindCause::Deleted, "\"Deleted\""),
            (AnchorBindCause::Untracked, "\"Untracked\""),
            (AnchorBindCause::Ambiguous, "\"Ambiguous\""),
        ] {
            assert_eq!(serde_json::to_string(&cause).unwrap(), s);
        }
    }
}
