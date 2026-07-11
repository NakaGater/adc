//! # adc-kernel
//!
//! OCCT FFI境界。ワークスペースで唯一OCCT(opencascade-rs fork)に触れる層 (ADR-002)。
//! 公開APIは自前の型(`Solid` / `FaceHandle` / `EdgeHandle` / `History`)のみを露出し、
//! opencascade / OCCT の型を他クレートへリークさせない(CI依存グラフ規律)。
//!
//! History APIは「操作前の部分形状 → 操作後の対応形状リスト」の**純粋な照会**に
//! 徹する。意味的アンカーの束縛ロジック(provides宣言 → 面の同定 → 再束縛)は
//! adc-compile 側に置く (ADR-001, ADR-002)。面の初期同定述語は
//! docs/provides-predicates.md が正典。

use glam::{dvec3, DVec3};
use opencascade::history::ShapeHistory;
use opencascade::primitives::{Edge, EdgeType, Face, FaceType, Shape, Wire};

/// B-repソリッド(OCCT TopoDS_Shapeのラッパー)
pub struct Solid {
    inner: Shape,
}

/// B-rep面へのハンドル(OCCT TopoDS_Faceのラッパー)。
/// 幾何ID(Face#42)としては公開しない — 照会・比較のための不透明ハンドル。
pub struct FaceHandle {
    inner: Face,
}

/// 面の下地曲面の種別
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceKind {
    Plane,
    Cylinder,
    Cone,
    Sphere,
    Torus,
    Other,
}

/// B-repエッジへのハンドル
pub struct EdgeHandle {
    inner: Edge,
}

/// モデリング操作の履歴 (BRepTools_History)。
/// 「入力の部分形状 → 結果の対応形状」の純粋な照会のみを提供する。
pub struct History {
    inner: ShapeHistory,
}

fn v(a: [f64; 3]) -> DVec3 {
    dvec3(a[0], a[1], a[2])
}

/// 直方体を原点コーナーから +x/+y/+z に作る
pub fn make_box(dx: f64, dy: f64, dz: f64) -> Solid {
    Solid {
        inner: Shape::box_with_dimensions(dx, dy, dz),
    }
}

/// 円柱: 底面中心 (cx, cy, cz)、+Z軸方向、半径 r、高さ h
pub fn make_cylinder(cx: f64, cy: f64, cz: f64, r: f64, h: f64) -> Solid {
    make_cylinder_dir([cx, cy, cz], [0.0, 0.0, 1.0], r, h)
}

/// 円柱: 底面中心 base、任意軸方向 dir(単位ベクトル)、半径 r、高さ h
pub fn make_cylinder_dir(base: [f64; 3], dir: [f64; 3], r: f64, h: f64) -> Solid {
    Solid {
        inner: Shape::cylinder(v(base), r, v(dir), h),
    }
}

/// 円錐(台): 底面中心 base、軸方向 dir(単位ベクトル)、
/// 底面半径 r_base(base側)、上面半径 r_far(base+dir*h側)、高さ h
pub fn make_cone_dir(base: [f64; 3], dir: [f64; 3], r_base: f64, r_far: f64, h: f64) -> Solid {
    // +Z軸の円錐を生成し、dirへ回転してから移動する
    let cone = Shape::cone()
        .bottom_radius(r_base)
        .top_radius(r_far)
        .height(h)
        .build();
    let d = v(dir).normalize();
    let z = DVec3::Z;
    let dot = z.dot(d).clamp(-1.0, 1.0);
    let rotated = if dot > 1.0 - 1e-12 {
        cone
    } else if dot < -1.0 + 1e-12 {
        cone.rotated(DVec3::X, std::f64::consts::PI)
    } else {
        let axis = z.cross(d).normalize();
        cone.rotated(axis, dot.acos())
    };
    Solid {
        inner: rotated.translated(v(base)),
    }
}

/// 平面閉多角形(頂点列、同一平面上)を押し出したプリズム。
/// corner_r > 0 なら断面の角を2Dフィレットしてから押し出す。
/// extrude は方向×長さ(非正規化)。
pub fn make_prism(
    points: &[[f64; 3]],
    corner_r: f64,
    extrude: [f64; 3],
) -> Result<Solid, String> {
    let wire = Wire::from_ordered_points(points.iter().map(|p| v(*p)))
        .map_err(|e| format!("プロファイルのワイヤ構築に失敗: {e}"))?;
    let mut face = Face::from_wire(&wire);
    if corner_r > 0.0 {
        face = face.fillet(corner_r);
    }
    let solid = face.extrude(v(extrude));
    Ok(Solid {
        inner: solid.into(),
    })
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

    /// ブーリアン和(self + tool)。結果と履歴を返す。
    pub fn fuse_with_history(&self, tool: &Solid) -> (Solid, History) {
        let (boolean_shape, history) = self.inner.union_with_history(&tool.inner);
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

    /// エッジ列にフィレットを適用。結果と履歴を返す。
    /// 失敗(半径過大等)はOCCT例外を捕捉して構造化メッセージで返す(abortしない)。
    pub fn fillet_edges_with_history(
        &self,
        edges: &[&EdgeHandle],
        radius: f64,
    ) -> Result<(Solid, History), String> {
        let (shape, history) = self
            .inner
            .fillet_edges_with_history(radius, edges.iter().map(|e| &e.inner))
            .map_err(|e| e.to_string())?;
        Ok((Solid { inner: shape }, History { inner: history }))
    }

    /// エッジ列に面取りを適用。結果と履歴を返す。失敗はabortせず構造化メッセージ。
    pub fn chamfer_edges_with_history(
        &self,
        edges: &[&EdgeHandle],
        distance: f64,
    ) -> Result<(Solid, History), String> {
        let (shape, history) = self
            .inner
            .chamfer_edges_with_history(distance, edges.iter().map(|e| &e.inner))
            .map_err(|e| e.to_string())?;
        Ok((Solid { inner: shape }, History { inner: history }))
    }

    /// 軸平行バウンディングボックス (min, max)
    pub fn bounding_box(&self) -> ([f64; 3], [f64; 3]) {
        let aabb = opencascade::bounding_box::aabb(&self.inner);
        let (min, max) = (aabb.min(), aabb.max());
        ([min.x, min.y, min.z], [max.x, max.y, max.z])
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

    /// 下地曲面の種別
    pub fn surface_kind(&self) -> SurfaceKind {
        match self.inner.surface_type() {
            FaceType::Plane => SurfaceKind::Plane,
            FaceType::Cylinder => SurfaceKind::Cylinder,
            FaceType::Cone => SurfaceKind::Cone,
            FaceType::Sphere => SurfaceKind::Sphere,
            FaceType::Torus => SurfaceKind::Torus,
            _ => SurfaceKind::Other,
        }
    }

    /// 重心位置での外向き法線。**平面前提**(曲面では重心の面上射影が
    /// 多義になりOCCT例外が出うる。曲面はまず surface_kind で除外すること)
    pub fn normal(&self) -> [f64; 3] {
        let n = self.inner.normal_at_center();
        [n.x, n.y, n.z]
    }

    /// 面の境界エッジ
    pub fn edges(&self) -> Vec<EdgeHandle> {
        self.inner
            .edges()
            .map(|e| EdgeHandle { inner: e })
            .collect()
    }
}

impl EdgeHandle {
    pub fn start(&self) -> [f64; 3] {
        let p = self.inner.start_point();
        [p.x, p.y, p.z]
    }

    pub fn end(&self) -> [f64; 3] {
        let p = self.inner.end_point();
        [p.x, p.y, p.z]
    }

    /// 同一の位相実体を指すか(向きの違いは無視)
    pub fn is_same(&self, other: &EdgeHandle) -> bool {
        self.inner.is_same(&other.inner)
    }

    /// 円(弧)エッジか
    pub fn is_circle(&self) -> bool {
        self.inner.edge_type() == EdgeType::Circle
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

    /// 操作前のエッジ → 操作後に対応するエッジリスト
    pub fn modified_edges(&self, of: &EdgeHandle) -> Vec<EdgeHandle> {
        self.inner
            .modified_edges(&of.inner)
            .into_iter()
            .map(|e| EdgeHandle { inner: e })
            .collect()
    }

    /// 操作前のエッジが操作で消滅したか
    pub fn is_removed_edge(&self, of: &EdgeHandle) -> bool {
        self.inner.is_removed_edge(&of.inner)
    }
}
