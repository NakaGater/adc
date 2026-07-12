use serde::{Deserialize, Serialize};

use crate::expr::Expr;
use crate::ids::{AnchorId, FeatureId, MaterialId, PartId};

/// 部品 (05-schema.md §4)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Part {
    pub id: PartId,
    pub material: MaterialId,
    pub process: Process,
    /// 宣言順にコンパイル
    pub features: Vec<Feature>,
    /// 部品が公開する意味的参照点
    pub anchors: Vec<Anchor>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Process {
    Machining,
    SheetMetal { thickness: Expr, k_factor: f64 },
}

/// フィーチャー語彙 T1(切削, §4.1)+ T2(板金, §4.2)。
///
/// `id` / `at` は型上Optionだが、トップレベルフィーチャーのid必須・
/// 非ルートフィーチャーのat必須はM0-2の静的検証で保証する
/// (Pattern の `of` に埋め込むインラインフィーチャーは id/at を省略できるため)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Feature {
    Block {
        #[serde(default)]
        id: Option<FeatureId>,
        x: Expr,
        y: Expr,
        z: Expr,
        #[serde(default)]
        at: Option<Placement>,
    },
    Cylinder {
        #[serde(default)]
        id: Option<FeatureId>,
        d: Expr,
        h: Expr,
        #[serde(default)]
        axis: Option<AxisDir>,
        #[serde(default)]
        at: Option<Placement>,
    },
    Hole {
        #[serde(default)]
        id: Option<FeatureId>,
        kind: HoleKind,
        d: Expr,
        depth: HoleDepth,
        /// Counterbore用
        #[serde(default)]
        cb_d: Option<Expr>,
        #[serde(default)]
        cb_depth: Option<Expr>,
        /// Countersink用
        #[serde(default)]
        cs_d: Option<Expr>,
        #[serde(default)]
        cs_angle: Option<Expr>,
        /// Tapped用 e.g. "M6"
        #[serde(default)]
        thread: Option<String>,
        #[serde(default)]
        at: Option<Placement>,
    },
    Pocket {
        #[serde(default)]
        id: Option<FeatureId>,
        profile: Profile,
        depth: Expr,
        #[serde(default)]
        corner_r: Option<Expr>,
        #[serde(default)]
        at: Option<Placement>,
    },
    Boss {
        #[serde(default)]
        id: Option<FeatureId>,
        profile: Profile,
        height: Expr,
        #[serde(default)]
        at: Option<Placement>,
    },
    Fillet {
        #[serde(default)]
        id: Option<FeatureId>,
        edges: EdgeSelector,
        r: Expr,
    },
    Chamfer {
        #[serde(default)]
        id: Option<FeatureId>,
        edges: EdgeSelector,
        size: Expr,
    },
    /// 各インスタンスのprovidesに `[i]` 添字(Linear2Dは `[i][j]`)— M1-4
    Pattern {
        #[serde(default)]
        id: Option<FeatureId>,
        of: Box<Feature>,
        kind: PatternKind,
        count: Count,
        pitch: Pitch,
        /// Circular用の回転軸
        #[serde(default)]
        axis: Option<BindingExpr>,
        #[serde(default)]
        at: Option<Placement>,
    },
    // ---- T2 板金 (P1, M5-1) ----
    /// 板金Partのルート専用。板厚は process: SheetMetal.thickness から取る
    /// (二重定義を排除 — 2026-07-12 M5-1設計メモ承認)。profile中心=配置原点
    BaseFlange {
        #[serde(default)]
        id: Option<FeatureId>,
        profile: Profile,
        #[serde(default)]
        at: Option<Placement>,
    },
    /// 曲げ+平坦部。曲げ補正BA = angle_rad × (bend_r + k_factor × t) は派生量
    Flange {
        #[serde(default)]
        id: Option<FeatureId>,
        edge: EdgeSelector,
        angle: Expr,
        length: Expr,
        bend_r: Expr,
    },
    /// フランジ面上
    Cutout {
        #[serde(default)]
        id: Option<FeatureId>,
        profile: Profile,
        at: Option<Placement>,
    },
    /// 曲げ逃げ
    Relief {
        #[serde(default)]
        id: Option<FeatureId>,
        kind: ReliefKind,
        at: Option<Placement>,
    },
}

impl Feature {
    /// フィーチャーID(インラインフィーチャーはNoneを許容 — 05-schema.md §4)
    pub fn id(&self) -> Option<&str> {
        match self {
            Feature::Block { id, .. }
            | Feature::Cylinder { id, .. }
            | Feature::Hole { id, .. }
            | Feature::Pocket { id, .. }
            | Feature::Boss { id, .. }
            | Feature::Fillet { id, .. }
            | Feature::Chamfer { id, .. }
            | Feature::Pattern { id, .. }
            | Feature::BaseFlange { id, .. }
            | Feature::Flange { id, .. }
            | Feature::Cutout { id, .. }
            | Feature::Relief { id, .. } => id.as_deref(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HoleKind {
    Simple,
    Counterbore,
    Countersink,
    Tapped,
}

/// 穴深さ。RON糖衣: 裸の数値は `Blind` (m0_1_sugar.rs)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum HoleDepth {
    Through,
    Blind(Expr),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Profile {
    Rect { x: Expr, y: Expr },
    Circ { d: Expr },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PatternKind {
    Linear,
    Linear2D,
    Circular,
}

/// パターン数。RON糖衣: `4` / `(2, 2)` (m0_1_sugar.rs)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Count {
    One(u32),
    Two(u32, u32),
}

/// パターンピッチ。RON糖衣: `12.0` / `(64.0, 44.0)` (m0_1_sugar.rs)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Pitch {
    One(Expr),
    Two(Expr, Expr),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ReliefKind {
    Rect { w: Expr, d: Expr },
    Round { d: Expr },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AxisDir {
    X,
    Y,
    Z,
}

/// 配置式 (05-schema.md §4.0)。
/// グローバル座標の直書きはルートフィーチャー(Origin)以外で許可しない(M0-2で検証)。
/// RON糖衣: `on(feature("base").face("top"), center())` / `offset(<placement>, (dx, dy, dz))`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Placement {
    /// ルートフィーチャー専用
    Origin,
    /// 面上の2D位置
    On { face: BindingExpr, at: Pos2 },
    Offset {
        from: Box<Placement>,
        d: (Expr, Expr, Expr),
    },
}

/// 面ローカル座標系(原点=重心、軸=面の主軸)はコンパイラが決定的に導出する。
/// RON糖衣: `center()` / `xy(x, y)` / `from_edge(<edges>, d, along)`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Pos2 {
    /// 面の重心
    Center,
    /// 面ローカル座標
    Xy(Expr, Expr),
    FromEdge {
        edge: EdgeSelector,
        d: Expr,
        along: Expr,
    },
}

/// エッジは意味選択のみ。幾何ID指定は存在しない (ADR-001)。
/// RON糖衣: `edges_of(<binding>)` / `edges_between(<binding>, <binding>)`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum EdgeSelector {
    EdgesOf(BindingExpr),
    EdgesBetween(BindingExpr, BindingExpr),
}

/// フィーチャーが `provides` する名前付き要素への参照 (05-schema.md §4)。
/// コンパイラはOCCT History (ADR-001) でB-rep実体に解決する。失敗は E-ANCHOR-BIND。
/// RON糖衣: `feature("bore").face("wall")` / `.axis("axis")` / `.edge("rim")`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BindingExpr {
    pub feature: FeatureId,
    pub elem: ProvidedElem,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProvidedElem {
    Face(String),
    Axis(String),
    Edge(String),
    Point(String),
}

/// 意味的アンカー (05-schema.md §4, ADR-001)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Anchor {
    /// e.g. "bearing_bore"
    pub id: AnchorId,
    pub kind: AnchorKind,
    pub binding: BindingExpr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnchorKind {
    Face,
    Axis,
    Edge,
    Point,
    Datum(char),
}
