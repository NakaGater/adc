use std::fmt;
use std::str::FromStr;

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::expr::Expr;
use crate::ids::{AnchorId, InstanceId, MateId, PartId, RationaleId};

/// アセンブリ (05-schema.md §5, ADR-005)。mateグラフはDAG(循環はM0-2/M3で検証)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assembly {
    pub id: String,
    pub instances: Vec<Instance>,
    pub mates: Vec<Mate>,
    /// 基準部品
    pub ground: InstanceId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Instance {
    pub id: InstanceId,
    pub part: PartId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mate {
    pub id: MateId,
    pub kind: MateKind,
    pub a: AnchorPath,
    pub b: AnchorPath,
    pub rationale: RationaleId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MateKind {
    Coaxial,
    Coincident,
    Distance(Expr),
    Angle(Expr),
}

/// `instance.anchor` 参照。RONでは文字列 `"housing.bore_face"` として(デ)シリアライズする。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AnchorPath {
    pub instance: InstanceId,
    pub anchor: AnchorId,
}

impl fmt::Display for AnchorPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.instance, self.anchor)
    }
}

impl FromStr for AnchorPath {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.split_once('.') {
            Some((instance, anchor)) if !instance.is_empty() && !anchor.is_empty() => {
                Ok(AnchorPath {
                    instance: instance.to_string(),
                    anchor: anchor.to_string(),
                })
            }
            _ => Err(format!(
                "AnchorPathは \"instance.anchor\" 形式の文字列であること: {s:?}"
            )),
        }
    }
}

impl Serialize for AnchorPath {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for AnchorPath {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct V;
        impl Visitor<'_> for V {
            type Value = AnchorPath;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("\"instance.anchor\" 形式の文字列")
            }
            fn visit_str<E: de::Error>(self, s: &str) -> Result<Self::Value, E> {
                s.parse().map_err(E::custom)
            }
        }
        deserializer.deserialize_str(V)
    }
}
