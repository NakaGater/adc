use std::fmt;

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::assembly::AnchorPath;
use crate::expr::Expr;
use crate::ids::{AssertId, DimId, PartRef, RationaleId};

/// アサーション (05-schema.md §6, ADR-003)。
/// チェッカー実装契約(Checkerトレイト/CheckResult/results.jsonl)は adc-check (M2-1) が持つ。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assertion {
    pub id: AssertId,
    pub check: Check,
    pub rationale: RationaleId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Check {
    // ---- T1 (P0) ----
    Clearance {
        a: GeomRef,
        b: GeomRef,
        min: Expr,
    },
    NoInterference {
        scope: Scope,
    },
    Mass {
        part: PartRef,
        max: Expr,
        #[serde(default)]
        min: Option<Expr>,
    },
    Cog {
        within: BoxSpec,
    },
    WallThickness {
        part: PartRef,
        min: Expr,
        /// 近似手法(レイキャスト)のサンプリング密度。キャッシュキーに含める (ADR-003)
        sample_density: f64,
    },
    BoundingBox {
        part: PartRef,
        max: (Expr, Expr, Expr),
    },
    DatumValidity {
        part: PartRef,
    },
    // ---- T2 (P1) ----
    /// bend_r>=k*t, hole_to_bend, flange_min
    SheetMetalRules {
        part: PartRef,
    },
    ToleranceStack1D {
        /// 公差付き寸法の連鎖
        path: Vec<DimId>,
        /// 許容範囲
        target: (f64, f64),
        method: StackMethod,
    },
    // ---- T3 (P2) ----
    ToolAccess {
        part: PartRef,
        tool_axis: (f64, f64, f64),
        tool_d: Expr,
    },
    MinCornerRadius {
        part: PartRef,
        min: Expr,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Scope {
    All,
    Pairs(Vec<(PartRef, PartRef)>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StackMethod {
    WorstCase,
    Rss,
    Both,
}

/// Cog の許容領域
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoxSpec {
    pub min: (Expr, Expr, Expr),
    pub max: (Expr, Expr, Expr),
}

/// `AnchorPath | PartRef` の直和 (05-schema.md §6 Clearance)。
/// RONでは文字列: `.` を含めば `"instance.anchor"`、含まなければ部品参照 `"part_id"`。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum GeomRef {
    Part(PartRef),
    Anchor(AnchorPath),
}

impl fmt::Display for GeomRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GeomRef::Part(p) => f.write_str(p),
            GeomRef::Anchor(a) => a.fmt(f),
        }
    }
}

impl Serialize for GeomRef {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for GeomRef {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct V;
        impl Visitor<'_> for V {
            type Value = GeomRef;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("部品参照 \"part_id\" または \"instance.anchor\" 形式の文字列")
            }
            fn visit_str<E: de::Error>(self, s: &str) -> Result<Self::Value, E> {
                if s.contains('.') {
                    s.parse().map(GeomRef::Anchor).map_err(E::custom)
                } else if s.is_empty() {
                    Err(E::custom("空のGeomRefは不正"))
                } else {
                    Ok(GeomRef::Part(s.to_string()))
                }
            }
        }
        deserializer.deserialize_str(V)
    }
}
