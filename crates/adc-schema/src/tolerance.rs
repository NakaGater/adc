use serde::{Deserialize, Serialize};

use crate::assembly::AnchorPath;
use crate::expr::Expr;
use crate::ids::{DimId, RationaleId};

// Dim / GeomTol は Design トップレベルの dims / geom_tols に置く(05-schema.md §1、
// 2026-07-11決定)。公差スタックは部品横断の寸法連鎖を扱うため AnchorPath で
// 部品横断参照できる階層が必要であり、公差は幾何の属性ではなく rationale 付き
// 制約として assertions と同じ制約レイヤーに属するため。

/// 寸法公差 (05-schema.md §7, P1)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dim {
    pub id: DimId,
    pub from: AnchorPath,
    pub to: AnchorPath,
    pub nominal: Expr,
    pub tol: Tol,
    pub rationale: RationaleId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Tol {
    /// ±
    Sym(f64),
    /// +u / -l
    Asym { plus: f64, minus: f64 },
    /// はめあい e.g. "H7"。主要はめあいテーブル内蔵(M5-3)
    Fit(String),
}

/// 幾何公差 (05-schema.md §7, P1)。
/// MVPでは (1) データム参照の妥当性検証 (2) ToleranceStack1Dへの寄与
/// (3) STEP AP242 PMI出力(努力目標)に使用。実測検証はスコープ外。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeomTol {
    pub kind: GeomTolKind,
    pub target: AnchorPath,
    /// kind: Datum のアンカーのみ許可 (M0-2で検証)
    pub datums: Vec<AnchorPath>,
    pub zone: Expr,
    pub rationale: RationaleId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GeomTolKind {
    Position,
    Flatness,
    Perpendicularity,
    Concentricity,
}
