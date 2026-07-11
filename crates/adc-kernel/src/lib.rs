//! # adc-kernel
//!
//! OCCT FFI境界。ワークスペースで唯一OCCT(opencascade-rs fork)に触れる層 (ADR-002)。
//! 公開APIは自前の型(`Solid` / `FaceHandle` / `History`)のみを露出し、
//! opencascade / OCCT の型を他クレートへリークさせない(CI依存グラフ規律)。
//!
//! History APIは「操作前の部分形状 → 操作後の対応形状リスト」の**純粋な照会**に
//! 徹する。意味的アンカーの束縛ロジック(provides宣言 → 面の同定 → 再束縛)は
//! adc-compile 側に置く (ADR-001, ADR-002)。

use glam::{dvec3, DVec3};
use opencascade::history::ShapeHistory;
use opencascade::primitives::{Face, Shape};

/// B-repソリッド(OCCT TopoDS_Shapeのラッパー)
pub struct Solid {
    inner: Shape,
}

/// B-rep面へのハンドル(OCCT TopoDS_Faceのラッパー)。
/// 幾何ID(Face#42)としては公開しない — 照会・比較のための不透明ハンドル。
pub struct FaceHandle {
    inner: Face,
}

/// モデリング操作の履歴 (BRepTools_History)。
/// 「入力の部分形状 → 結果の対応形状」の純粋な照会のみを提供する。
pub struct History {
    inner: ShapeHistory,
}

/// 直方体を原点コーナーから +x/+y/+z に作る
pub fn make_box(dx: f64, dy: f64, dz: f64) -> Solid {
    Solid {
        inner: Shape::box_with_dimensions(dx, dy, dz),
    }
}

/// 円柱: 底面中心 (cx, cy, cz)、+Z軸方向、半径 r、高さ h
pub fn make_cylinder(cx: f64, cy: f64, cz: f64, r: f64, h: f64) -> Solid {
    Solid {
        inner: Shape::cylinder(dvec3(cx, cy, cz), r, DVec3::Z, h),
    }
}

impl Solid {
    /// ブーリアン差(self - tool)。結果と履歴を返す。
    pub fn cut_with_history(&self, tool: &Solid) -> (Solid, History) {
        let (boolean_shape, history) = self.inner.subtract_with_history(&tool.inner);
        (
            Solid {
                inner: boolean_shape.shape,
            },
            History { inner: history },
        )
    }

    /// 全ての面
    pub fn faces(&self) -> Vec<FaceHandle> {
        self.inner
            .faces()
            .map(|f| FaceHandle { inner: f })
            .collect()
    }
}

impl FaceHandle {
    /// 面積 (mm^2)
    pub fn area(&self) -> f64 {
        self.inner.surface_area()
    }

    /// 面の重心 [x, y, z]
    pub fn center(&self) -> [f64; 3] {
        let c = self.inner.center_of_mass();
        [c.x, c.y, c.z]
    }
}

impl History {
    /// 操作前の面 → 操作後に対応する面リスト(分割・トリムされた面)
    pub fn modified_faces(&self, of: &FaceHandle) -> Vec<FaceHandle> {
        self.inner
            .modified_faces(&of.inner)
            .into_iter()
            .map(|f| FaceHandle { inner: f })
            .collect()
    }

    /// 操作前の面 → 操作で新規生成された対応面リスト
    pub fn generated_faces(&self, of: &FaceHandle) -> Vec<FaceHandle> {
        self.inner
            .generated_faces(&of.inner)
            .into_iter()
            .map(|f| FaceHandle { inner: f })
            .collect()
    }

    /// 操作前の面が操作で消滅したか
    pub fn is_removed_face(&self, of: &FaceHandle) -> bool {
        self.inner.is_removed_face(&of.inner)
    }
}
