use serde::{Deserialize, Serialize};

use crate::ids::ParamId;

/// 数値を書ける全ての場所で受け付ける式 (05-schema.md §2)。
/// リテラル / `param(id)` / 四則演算。循環参照はM0-2の静的検証(E-SCHEMA-CYCLE)。
///
/// RON入力では糖衣を受理する契約(受入テスト `tests/m0_1_sugar.rs`):
/// 裸の数値リテラル(`80.0`)、`param("wall_t")`、中置四則(`param("t") * 2.0 + 1.0`)。
/// round-trip保証は値レベル(parse→serialize→parseで同値)であり、
/// 糖衣は入力側でのみ受理し、正準形は `to_canonical_ron` の出力とする。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Expr {
    Lit(f64),
    Param(ParamId),
    Add(Box<Expr>, Box<Expr>),
    Sub(Box<Expr>, Box<Expr>),
    Mul(Box<Expr>, Box<Expr>),
    Div(Box<Expr>, Box<Expr>),
}
