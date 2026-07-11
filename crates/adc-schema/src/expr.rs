use std::fmt;

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::ids::ParamId;

/// 数値を書ける全ての場所で受け付ける式 (05-schema.md §2)。
/// リテラル / `param(id)` / 四則演算。循環参照はM0-2の静的検証(E-SCHEMA-CYCLE)。
///
/// ## RON表現(値レベルround-trip契約)
/// - 純リテラルは数値そのもの(`80.0`)
/// - それ以外はDSL文字列(`"param(wall_t) * 2 + 1"`)
/// - 入力側では糖衣として非引用の関数風表記(`param("wall_t") * 2.0`)も受理する
///   (desugar前処理が文字列/数値形へ展開する — tests/m0_1_sugar.rs)
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Lit(f64),
    Param(ParamId),
    Add(Box<Expr>, Box<Expr>),
    Sub(Box<Expr>, Box<Expr>),
    Mul(Box<Expr>, Box<Expr>),
    Div(Box<Expr>, Box<Expr>),
}

impl Serialize for Expr {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Expr::Lit(v) => serializer.serialize_f64(*v),
            other => serializer.serialize_str(&crate::desugar::expr_dsl(other)),
        }
    }
}

impl<'de> Deserialize<'de> for Expr {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct V;
        impl Visitor<'_> for V {
            type Value = Expr;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("数値、または式文字列(例: \"param(wall_t) * 2\")")
            }

            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Self::Value, E> {
                Ok(Expr::Lit(v))
            }

            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
                Ok(Expr::Lit(v as f64))
            }

            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(Expr::Lit(v as f64))
            }

            fn visit_str<E: de::Error>(self, s: &str) -> Result<Self::Value, E> {
                crate::desugar::parse_expr_str(s).map_err(E::custom)
            }
        }
        deserializer.deserialize_any(V)
    }
}
