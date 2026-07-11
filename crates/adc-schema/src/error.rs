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
