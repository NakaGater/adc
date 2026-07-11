//! ID型 (05-schema.md)。MVPでは型エイリアス。一意性・参照解決はM0-2の静的検証で保証する。

pub type ParamId = String;
pub type RationaleId = String;
pub type MaterialId = String;
pub type PartId = String;
/// アサーション等から部品を指す参照
pub type PartRef = String;
pub type FeatureId = String;
pub type AnchorId = String;
pub type InstanceId = String;
pub type MateId = String;
pub type AssertId = String;
pub type DimId = String;
