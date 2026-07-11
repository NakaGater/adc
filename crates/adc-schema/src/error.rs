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
