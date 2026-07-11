//! # adc-schema
//!
//! ADC の正典IR (`design.ron`) の型定義・パース・シリアライズ。OCCT非依存 (ADR-002)。
//! スキーマ仕様は `05-schema.md`、型体系の根拠は ADR-001。
//!
//! ## round-trip 契約 (US-01 / M0-1)
//!
//! - 保証は**値レベル**: `parse_design(to_canonical_ron(&d)) == d`
//! - 糖衣 (05-schema.md §4.0 の関数風表記 — `param("id")`、裸数値、
//!   `feature("f").face("n")`、`on(...)`、`edges_of(...)` 等) は**入力側でのみ**受理し、
//!   正準形は `to_canonical_ron` の出力とする。糖衣の受入契約は
//!   `tests/m0_1_sugar.rs` / `tests/m0_1_sample.rs` が規定する
//! - 不正なRONは行番号付きの `E-SCHEMA-PARSE` (`SchemaError::Parse`)

mod assembly;
mod assertion;
mod design;
mod desugar;
mod error;
mod expr;
mod ids;
mod part;
mod tolerance;

pub use assembly::*;
pub use assertion::*;
pub use design::*;
pub use error::*;
pub use expr::*;
pub use ids::*;
pub use part::*;
pub use tolerance::*;

/// design.ron テキストをパースする。
///
/// 糖衣(05-schema.md §4.0)は前処理(desugar)で正準RONへ展開してからserdeに渡す。
/// 展開は行構造を保存するため、エラーの行番号は元テキストと一致する。
/// Optionフィールドは `implicit_some` 拡張により `Some(...)` を省略できる
/// (§9サンプルの `at: on(...)` / `cb_d: 11.0` 形)。
pub fn parse_design(src: &str) -> Result<Design, SchemaError> {
    let desugared = desugar::desugar(src)?;
    let options = ron::Options::default()
        .with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME);
    options
        .from_str(&desugared)
        .map_err(|e| SchemaError::Parse {
            message: e.code.to_string(),
            line: e.position.line,
            column: e.position.col,
        })
}

/// Design を正準RONテキストにシリアライズする(決定的: 同一値 → 同一バイト列)。
pub fn to_canonical_ron(design: &Design) -> Result<String, SchemaError> {
    let config = ron::ser::PrettyConfig::new()
        .struct_names(true)
        .extensions(ron::extensions::Extensions::IMPLICIT_SOME);
    ron::ser::to_string_pretty(design, config).map_err(|e| SchemaError::Serialize(e.to_string()))
}
