use serde::{Deserialize, Serialize};

use crate::assembly::Assembly;
use crate::assertion::Assertion;
use crate::ids::{MaterialId, ParamId, RationaleId};
use crate::part::Part;

/// 正典 design.ron のトップレベル (05-schema.md §1)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Design {
    pub schema_version: String,
    /// この設計の目的(自然言語)
    pub intent: String,
    pub params: Vec<Param>,
    pub materials: Vec<Material>,
    pub parts: Vec<Part>,
    #[serde(default)]
    pub assembly: Option<Assembly>,
    pub assertions: Vec<Assertion>,
    pub rationales: Vec<Rationale>,
}

/// パラメータ (05-schema.md §2, ADR-004)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Param {
    /// snake_case、Design内一意 (M0-2で検証)
    pub id: ParamId,
    pub value: ParamValue,
    pub unit: Unit,
    /// 必須 (US-04)
    pub rationale: RationaleId,
}

/// ADR-004: 未確定パラメータは第一級
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ParamValue {
    Determined(f64),
    /// nominal ∈ range (M0-2で検証)
    Open { range: (f64, f64), nominal: f64 },
}

/// 単位はスキーマ全体で mm / 度 / g に固定(単位混在はスコープ外)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Unit {
    Mm,
    Deg,
    G,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Material {
    pub id: MaterialId,
    pub density_g_cm3: f64,
    pub name: String,
}

/// Rationale (05-schema.md §3)。全ての制約に「誰が・何を根拠に」を付ける。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rationale {
    pub id: RationaleId,
    pub author: Author,
    pub basis: Basis,
    pub note: String,
    /// ISO 8601
    pub timestamp: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Author {
    Human(String),
    Agent(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Basis {
    /// 要求文書参照
    Requirement(String),
    /// 規格番号 e.g. "JIS B 1176"
    Standard(String),
    /// 過去知見への参照(Design Memory連携点)
    Lesson(String),
    /// 仮決め。後で確定する義務を負う
    Assumption,
}
