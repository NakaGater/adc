use serde::{Deserialize, Serialize};

use crate::assembly::AnchorPath;
use crate::expr::Expr;
use crate::ids::{DimId, RationaleId};

// NOTE: 05-schema.md §7 の Dim / GeomTol は §1 の Design にまだ保持フィールドが
// 定義されていない(仕様の未決事項 — ToleranceStack1D が DimId を参照するため
// どこかに置く必要がある)。M0-1 では型定義と round-trip 保証のみ提供し、
// 添付先はスキーマ仕様の更新とあわせて M0-2 以降で確定する。

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
