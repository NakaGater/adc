//! # adc-compile
//!
//! フィーチャー(05-schema.md §4.1)→ B-rep のコンパイルと意味的アンカー束縛 (ADR-001)。
//!
//! ## 束縛モデル(2026-07-12設計レビュー承認)
//! - **provides台帳**: 各フィーチャーのコンパイル時に、生成部分形状を
//!   `(feature_id, 要素名)` で台帳に登録する。初期同定は docs/provides-predicates.md の
//!   決定的述語で**工具側/生成直後に一回だけ**行う
//! - **History前送り**: 後続の各操作(cut/fuse)の後、台帳の全エントリを
//!   OCCT History (Modified/IsRemoved) で前送りする。単一面providesの分割は
//!   Ambiguous、消滅は Deleted として状態化し、アンカーが参照した時点で
//!   E-ANCHOR-BIND {anchor_id, feature_id, cause, hint} を返す(案1: 1対1のみ許容)
//! - 集合provides(walls / side)は分割・消滅を集合の伸縮として吸収する
//! - 配置(§4.0)のフレーム導出は docs/placement-frames.md が正典
//!
//! M1-2の対象: Block / Cylinder / Hole(Simple, Counterbore) / Pocket / Boss。
//! Fillet/Chamfer(M1-3)、Pattern(M1-4)、板金(M5)、Pos2::FromEdge(M1-3)は未対応。

mod frame;

use std::collections::HashMap;
use std::fmt;

use adc_kernel::{
    make_box, make_cylinder_dir, make_prism, EdgeHandle, FaceHandle, History, Solid, SurfaceKind,
};
use adc_schema::{
    AnchorBindError, AnchorKind, AxisDir, Design, EvalContext, Evaluator, Expr, Feature,
    HoleDepth, HoleKind, Placement, Pos2, Profile, ProvidedElem, ValidationError,
};
use frame::{add, dot, frame_from_origin_normal, normalize, scale, sub, world_frame, Frame};

// ---------------------------------------------------------------- エラー

#[derive(Debug)]
pub enum CompileError {
    /// E-ANCHOR-BIND (adc-schema::AnchorBindError)
    AnchorBind(AnchorBindError),
    /// 式評価の失敗 (E-SCHEMA-EVAL / E-SCHEMA-REF)
    Eval(ValidationError),
    /// providesに存在しない要素への参照
    UnknownProvides { feature_id: String, elem: String },
    /// アンカー種別とprovides要素の型不一致
    KindMismatch {
        anchor_id: String,
        expected: String,
        found: String,
    },
    /// M1-2の範囲外(E-FEATURE-FAILの前身。M1-7で構造化)
    Unsupported { feature_id: String, what: String },
    /// 幾何・構造の不整合
    Geometry { feature_id: String, message: String },
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CompileError::AnchorBind(e) => e.fmt(f),
            CompileError::Eval(e) => e.fmt(f),
            CompileError::UnknownProvides { feature_id, elem } => write!(
                f,
                "E-ANCHOR-BIND: フィーチャー \"{feature_id}\" はprovides要素 \"{elem}\" を提供しません (docs/provides-predicates.md)"
            ),
            CompileError::KindMismatch {
                anchor_id,
                expected,
                found,
            } => write!(
                f,
                "E-ANCHOR-BIND: アンカー \"{anchor_id}\" の種別({expected})とprovides要素の型({found})が一致しません"
            ),
            CompileError::Unsupported { feature_id, what } => {
                write!(f, "E-FEATURE-FAIL: \"{feature_id}\": {what}")
            }
            CompileError::Geometry {
                feature_id,
                message,
            } => write!(f, "E-FEATURE-FAIL: \"{feature_id}\": {message}"),
        }
    }
}

// ---------------------------------------------------------------- 台帳

/// providesされた部分形状(またはその前送り状態)
enum Provided {
    Face(FaceHandle),
    /// 集合provides (walls / side)。分割・消滅を伸縮として吸収する
    FaceSet(Vec<FaceHandle>),
    Axis { origin: [f64; 3], dir: [f64; 3] },
    Edge(EdgeHandle),
    /// 前送りで消滅(参照された時点で E-ANCHOR-BIND {cause: Deleted})
    Deleted { by_feature: String },
    /// 前送りで複数対応(参照された時点で E-ANCHOR-BIND {cause: Ambiguous})
    Ambiguous { by_feature: String, count: usize },
}

impl Provided {
    fn type_name(&self) -> &'static str {
        match self {
            Provided::Face(_) => "face",
            Provided::FaceSet(_) => "face集合",
            Provided::Axis { .. } => "axis",
            Provided::Edge(_) => "edge",
            Provided::Deleted { .. } => "(消滅)",
            Provided::Ambiguous { .. } => "(多義)",
        }
    }
}

fn forward_face(f: FaceHandle, h: &History, by: &str) -> Provided {
    let mut mapped = h.modified_faces(&f);
    match mapped.len() {
        0 => {
            if h.is_removed_face(&f) {
                Provided::Deleted {
                    by_feature: by.to_string(),
                }
            } else {
                // 操作で変化しなかった面はそのまま結果に残る
                Provided::Face(f)
            }
        }
        1 => Provided::Face(mapped.pop().unwrap()),
        n => Provided::Ambiguous {
            by_feature: by.to_string(),
            count: n,
        },
    }
}

fn forward_entry(entry: Provided, h: &History, by: &str) -> Provided {
    match entry {
        Provided::Face(f) => forward_face(f, h, by),
        Provided::FaceSet(faces) => {
            // 集合は伸縮を吸収: 消滅はドロップ、分割は展開
            let mut out = Vec::new();
            for f in faces {
                let mapped = h.modified_faces(&f);
                if mapped.is_empty() {
                    if !h.is_removed_face(&f) {
                        out.push(f);
                    }
                } else {
                    out.extend(mapped);
                }
            }
            Provided::FaceSet(out)
        }
        Provided::Edge(e) => {
            let mut mapped = h.modified_edges(&e);
            match mapped.len() {
                0 => {
                    if h.is_removed_edge(&e) {
                        Provided::Deleted {
                            by_feature: by.to_string(),
                        }
                    } else {
                        Provided::Edge(e)
                    }
                }
                1 => Provided::Edge(mapped.pop().unwrap()),
                n => Provided::Ambiguous {
                    by_feature: by.to_string(),
                    count: n,
                },
            }
        }
        other @ (Provided::Axis { .. } | Provided::Deleted { .. } | Provided::Ambiguous { .. }) => {
            other
        }
    }
}

// ---------------------------------------------------------------- コンパイル状態

struct State {
    solid: Option<Solid>,
    ledger: HashMap<(String, String), Provided>,
    /// 配置用: providesされた単一平面の**生成時点**の幾何(重心・外向き法線)。
    /// 前送りしない — 後続フィーチャーが参照面を切り欠いても既存配置が動かない
    /// (順序安定性 — docs/placement-frames.md)
    initial_face_frames: HashMap<(String, String), ([f64; 3], [f64; 3])>,
}

impl State {
    fn forward_all(&mut self, h: &History, by: &str) {
        for entry in self.ledger.values_mut() {
            let old = std::mem::replace(
                entry,
                Provided::Deleted {
                    by_feature: String::new(),
                },
            );
            *entry = forward_entry(old, h, by);
        }
    }

    fn insert(&mut self, fid: &str, name: &str, p: Provided) {
        let key = (fid.to_string(), name.to_string());
        if let Provided::Face(f) = &p {
            if f.surface_kind() == SurfaceKind::Plane {
                self.initial_face_frames
                    .insert(key.clone(), (f.center(), f.normal()));
            }
        }
        self.ledger.insert(key, p);
    }
}

// ---------------------------------------------------------------- 公開API

/// 束縛済みアンカーへの参照
pub enum BoundAnchorRef<'a> {
    Face(&'a FaceHandle),
    Edge(&'a EdgeHandle),
    Axis { origin: [f64; 3], dir: [f64; 3] },
}

pub struct CompiledPart {
    pub part_id: String,
    pub solid: Solid,
    ledger: HashMap<(String, String), Provided>,
    /// anchor_id → 検証済みの台帳キー
    anchor_keys: HashMap<String, (String, String)>,
}

impl CompiledPart {
    /// 束縛済みアンカーを引く
    pub fn anchor(&self, anchor_id: &str) -> Option<BoundAnchorRef<'_>> {
        let key = self.anchor_keys.get(anchor_id)?;
        match self.ledger.get(key)? {
            Provided::Face(f) => Some(BoundAnchorRef::Face(f)),
            Provided::Edge(e) => Some(BoundAnchorRef::Edge(e)),
            Provided::Axis { origin, dir } => Some(BoundAnchorRef::Axis {
                origin: *origin,
                dir: *dir,
            }),
            _ => None,
        }
    }

    /// providesの単一面を引く(テスト・上位層照会用)
    pub fn provided_face(&self, feature: &str, name: &str) -> Option<&FaceHandle> {
        match self.ledger.get(&(feature.to_string(), name.to_string()))? {
            Provided::Face(f) => Some(f),
            _ => None,
        }
    }

    /// providesの面集合を引く
    pub fn provided_face_set(&self, feature: &str, name: &str) -> Option<&[FaceHandle]> {
        match self.ledger.get(&(feature.to_string(), name.to_string()))? {
            Provided::FaceSet(v) => Some(v),
            _ => None,
        }
    }

    /// providesの軸を引く
    pub fn provided_axis(&self, feature: &str, name: &str) -> Option<([f64; 3], [f64; 3])> {
        match self.ledger.get(&(feature.to_string(), name.to_string()))? {
            Provided::Axis { origin, dir } => Some((*origin, *dir)),
            _ => None,
        }
    }

    /// providesのエッジを引く
    pub fn provided_edge(&self, feature: &str, name: &str) -> Option<&EdgeHandle> {
        match self.ledger.get(&(feature.to_string(), name.to_string()))? {
            Provided::Edge(e) => Some(e),
            _ => None,
        }
    }
}

/// Partをコンパイルし、全アンカーを束縛する
pub fn compile_part(
    design: &Design,
    part_id: &str,
    ctx: &EvalContext,
) -> Result<CompiledPart, CompileError> {
    let part = design
        .parts
        .iter()
        .find(|p| p.id == part_id)
        .ok_or_else(|| CompileError::Geometry {
            feature_id: part_id.to_string(),
            message: format!("part \"{part_id}\" がDesignに存在しません"),
        })?;
    let ev = Evaluator::new(design, ctx).map_err(CompileError::Eval)?;

    let mut st = State {
        solid: None,
        ledger: HashMap::new(),
        initial_face_frames: HashMap::new(),
    };
    for f in &part.features {
        compile_feature(f, &mut st, &ev)?;
    }
    let solid = st.solid.ok_or_else(|| CompileError::Geometry {
        feature_id: part_id.to_string(),
        message: "partがソリッドを生成しませんでした".to_string(),
    })?;

    // アンカー束縛 (E-ANCHOR-BIND)
    let mut anchor_keys = HashMap::new();
    for a in &part.anchors {
        let key = bind_anchor(a, &st.ledger)?;
        anchor_keys.insert(a.id.clone(), key);
    }

    Ok(CompiledPart {
        part_id: part_id.to_string(),
        solid,
        ledger: st.ledger,
        anchor_keys,
    })
}

fn bind_anchor(
    anchor: &adc_schema::Anchor,
    ledger: &HashMap<(String, String), Provided>,
) -> Result<(String, String), CompileError> {
    let name = match &anchor.binding.elem {
        ProvidedElem::Face(n) | ProvidedElem::Axis(n) | ProvidedElem::Edge(n) => n.clone(),
        ProvidedElem::Point(n) => {
            return Err(CompileError::UnknownProvides {
                feature_id: anchor.binding.feature.clone(),
                elem: format!("point({n}) はT1語彙で未提供"),
            })
        }
    };
    let key = (anchor.binding.feature.clone(), name.clone());
    let entry = ledger
        .get(&key)
        .ok_or_else(|| CompileError::UnknownProvides {
            feature_id: anchor.binding.feature.clone(),
            elem: name.clone(),
        })?;

    let expected = match anchor.kind {
        AnchorKind::Face | AnchorKind::Datum(_) => "face",
        AnchorKind::Axis => "axis",
        AnchorKind::Edge => "edge",
        AnchorKind::Point => "point",
    };
    match (expected, entry) {
        (_, Provided::Deleted { by_feature }) => Err(CompileError::AnchorBind(
            AnchorBindError::deleted(&anchor.id, by_feature),
        )),
        (_, Provided::Ambiguous { by_feature, count }) => Err(CompileError::AnchorBind(
            AnchorBindError::ambiguous(&anchor.id, by_feature, *count),
        )),
        // 集合providesへの単一面アンカーは多義 (決定(a))
        ("face", Provided::FaceSet(v)) => Err(CompileError::AnchorBind(
            AnchorBindError::ambiguous(&anchor.id, &anchor.binding.feature, v.len()),
        )),
        ("face", Provided::Face(_))
        | ("axis", Provided::Axis { .. })
        | ("edge", Provided::Edge(_)) => Ok(key),
        (_, found) => Err(CompileError::KindMismatch {
            anchor_id: anchor.id.clone(),
            expected: expected.to_string(),
            found: found.type_name().to_string(),
        }),
    }
}

// ---------------------------------------------------------------- 式・配置

fn e(ev: &Evaluator, x: &Expr) -> Result<f64, CompileError> {
    ev.evaluate(x).map_err(CompileError::Eval)
}

fn resolve_placement(
    p: &Placement,
    st: &State,
    ev: &Evaluator,
    fid: &str,
) -> Result<Frame, CompileError> {
    match p {
        Placement::Origin => Ok(world_frame()),
        Placement::On { face, at } => {
            let key = match &face.elem {
                ProvidedElem::Face(n) => (face.feature.clone(), n.clone()),
                other => {
                    return Err(CompileError::Geometry {
                        feature_id: fid.to_string(),
                        message: format!("配置面参照はface要素であること: {other:?}"),
                    })
                }
            };
            // 配置は参照面の生成時点の幾何に対して決定される(順序安定性)
            let Some((c, n)) = st.initial_face_frames.get(&key).copied() else {
                return Err(if st.ledger.contains_key(&key) {
                    CompileError::Geometry {
                        feature_id: fid.to_string(),
                        message: format!(
                            "配置参照 {}.{} が配置可能な平面ではありません(曲面・集合・軸への配置は未対応 — docs/placement-frames.md)",
                            key.0, key.1
                        ),
                    }
                } else {
                    CompileError::UnknownProvides {
                        feature_id: key.0.clone(),
                        elem: key.1.clone(),
                    }
                });
            };
            let mut frame = frame_from_origin_normal(c, n);
            match at {
                Pos2::Center => {}
                Pos2::Xy(u, v) => {
                    let (u, v) = (e(ev, u)?, e(ev, v)?);
                    frame.origin = add(frame.origin, add(scale(frame.x, u), scale(frame.y, v)));
                }
                Pos2::FromEdge { .. } => {
                    return Err(CompileError::Unsupported {
                        feature_id: fid.to_string(),
                        what: "Pos2::FromEdge はM1-3(EdgeSelector解決)で対応".to_string(),
                    })
                }
            }
            Ok(frame)
        }
        Placement::Offset { from, d } => {
            let mut frame = resolve_placement(from, st, ev, fid)?;
            let (dx, dy, dz) = (e(ev, &d.0)?, e(ev, &d.1)?, e(ev, &d.2)?);
            // d はfromフレームのローカル(x, y, z)成分 (docs/placement-frames.md)
            frame.origin = add(
                frame.origin,
                add(
                    add(scale(frame.x, dx), scale(frame.y, dy)),
                    scale(frame.z, dz),
                ),
            );
            Ok(frame)
        }
    }
}

// ---------------------------------------------------------------- フィーチャー

fn req_id<'a>(id: &'a Option<String>, kind: &str) -> Result<&'a str, CompileError> {
    id.as_deref().ok_or_else(|| CompileError::Unsupported {
        feature_id: "(無名)".to_string(),
        what: format!("トップレベルの{kind}にはidが必要です"),
    })
}

/// 工具の面を押出/軸方向で同定: (side集合, +dir端, −dir端)。
/// 法線は平面にのみ問い合わせる(曲面の重心法線はOCCT例外になりうる)。
fn classify_prism_faces(
    tool: &Solid,
    dir: [f64; 3],
) -> (Vec<FaceHandle>, Option<FaceHandle>, Option<FaceHandle>) {
    let (mut sides, mut far, mut near) = (Vec::new(), None, None);
    for f in tool.faces() {
        if f.surface_kind() == SurfaceKind::Plane {
            let n = normalize(f.normal());
            let d = dot(n, dir);
            if d > 1.0 - 1e-6 {
                far = Some(f);
                continue;
            } else if d < -1.0 + 1e-6 {
                near = Some(f);
                continue;
            }
        }
        sides.push(f);
    }
    (sides, far, near)
}

/// 円柱工具の面を軸方向で同定: (side, +dir端, −dir端)
fn classify_cylinder_faces(
    tool: &Solid,
    dir: [f64; 3],
) -> (Option<FaceHandle>, Option<FaceHandle>, Option<FaceHandle>) {
    let (mut sides, far, near) = classify_prism_faces(tool, dir);
    (sides.pop(), far, near)
}

fn axis_dir(a: &Option<AxisDir>) -> [f64; 3] {
    match a.unwrap_or(AxisDir::Z) {
        AxisDir::X => [1.0, 0.0, 0.0],
        AxisDir::Y => [0.0, 1.0, 0.0],
        AxisDir::Z => [0.0, 0.0, 1.0],
    }
}

/// profileから工具プリズムを作る。断面はフレーム平面(baseを含むz直交面)、押出は dir×len。
fn profile_tool(
    profile: &Profile,
    frame: &Frame,
    base: [f64; 3],
    dir: [f64; 3],
    len: f64,
    corner_r: f64,
    ev: &Evaluator,
    fid: &str,
) -> Result<Solid, CompileError> {
    match profile {
        Profile::Circ { d } => {
            let r = e(ev, d)? / 2.0;
            Ok(make_cylinder_dir(base, dir, r, len))
        }
        Profile::Rect { x, y } => {
            let (sx, sy) = (e(ev, x)?, e(ev, y)?);
            let (hx, hy) = (sx / 2.0, sy / 2.0);
            let corners = [
                add(base, add(scale(frame.x, hx), scale(frame.y, hy))),
                add(base, add(scale(frame.x, -hx), scale(frame.y, hy))),
                add(base, add(scale(frame.x, -hx), scale(frame.y, -hy))),
                add(base, add(scale(frame.x, hx), scale(frame.y, -hy))),
            ];
            make_prism(&corners, corner_r, scale(dir, len)).map_err(|m| CompileError::Geometry {
                feature_id: fid.to_string(),
                message: m,
            })
        }
    }
}

fn compile_feature(f: &Feature, st: &mut State, ev: &Evaluator) -> Result<(), CompileError> {
    match f {
        Feature::Block { id, x, y, z, at } => {
            let fid = req_id(id, "Block")?.to_string();
            if !matches!(at, None | Some(Placement::Origin)) {
                return Err(CompileError::Unsupported {
                    feature_id: fid,
                    what: "非ルート配置のBlockはM1-3以降".to_string(),
                });
            }
            if st.solid.is_some() {
                return Err(CompileError::Unsupported {
                    feature_id: fid,
                    what: "2つ目のルートソリッドはM1-3以降".to_string(),
                });
            }
            let (dx, dy, dz) = (e(ev, x)?, e(ev, y)?, e(ev, z)?);
            let solid = make_box(dx, dy, dz);
            // provides同定 (docs/provides-predicates.md: 法線方向)
            for face in solid.faces() {
                let n = normalize(face.normal());
                let name = if n[2] > 1.0 - 1e-6 {
                    "top"
                } else if n[2] < -1.0 + 1e-6 {
                    "bottom"
                } else if n[0] > 1.0 - 1e-6 {
                    "+x"
                } else if n[0] < -1.0 + 1e-6 {
                    "-x"
                } else if n[1] > 1.0 - 1e-6 {
                    "+y"
                } else {
                    "-y"
                };
                st.insert(&fid, name, Provided::Face(face));
            }
            st.solid = Some(solid);
            Ok(())
        }

        Feature::Cylinder { id, d, h, axis, at } => {
            let fid = req_id(id, "Cylinder")?.to_string();
            let r = e(ev, d)? / 2.0;
            let hh = e(ev, h)?;
            match (&st.solid, at) {
                (None, None | Some(Placement::Origin)) => {
                    let dir = axis_dir(axis);
                    let tool = make_cylinder_dir([0.0, 0.0, 0.0], dir, r, hh);
                    let (side, far, near) = classify_cylinder_faces(&tool, dir);
                    if let Some(s) = side {
                        st.insert(&fid, "side", Provided::Face(s));
                    }
                    if let Some(t) = far {
                        st.insert(&fid, "top", Provided::Face(t));
                    }
                    if let Some(b) = near {
                        st.insert(&fid, "bottom", Provided::Face(b));
                    }
                    st.insert(
                        &fid,
                        "axis",
                        Provided::Axis {
                            origin: [0.0, 0.0, 0.0],
                            dir,
                        },
                    );
                    st.solid = Some(tool);
                    Ok(())
                }
                (Some(_), Some(p)) => {
                    // 面上の円柱 = 外向きに生やしてフューズ(Boss circと同型)
                    let frame = resolve_placement(p, st, ev, &fid)?;
                    let dir = frame.z;
                    let tool = make_cylinder_dir(frame.origin, dir, r, hh);
                    let (side, far, _near) = classify_cylinder_faces(&tool, dir);
                    let solid = st.solid.take().unwrap();
                    let (result, hist) = solid.fuse_with_history(&tool);
                    st.forward_all(&hist, &fid);
                    if let Some(s) = side {
                        st.insert(&fid, "side", forward_face(s, &hist, &fid));
                    }
                    if let Some(t) = far {
                        st.insert(&fid, "top", forward_face(t, &hist, &fid));
                    }
                    st.insert(
                        &fid,
                        "axis",
                        Provided::Axis {
                            origin: frame.origin,
                            dir,
                        },
                    );
                    st.solid = Some(result);
                    Ok(())
                }
                (None, _) => Err(CompileError::Geometry {
                    feature_id: fid,
                    message: "ルートフィーチャーの配置はOriginのみ (05-schema.md §4.0)".into(),
                }),
                (Some(_), None) => Err(CompileError::Geometry {
                    feature_id: fid,
                    message: "非ルートフィーチャーには配置(at)が必要".into(),
                }),
            }
        }

        Feature::Hole {
            id,
            kind,
            d,
            depth,
            cb_d,
            cb_depth,
            at,
            ..
        } => {
            let fid = req_id(id, "Hole")?.to_string();
            let solid = st.solid.take().ok_or_else(|| CompileError::Geometry {
                feature_id: fid.clone(),
                message: "Holeの前にソリッドが必要".into(),
            })?;
            let p = at.as_ref().ok_or_else(|| CompileError::Geometry {
                feature_id: fid.clone(),
                message: "Holeには配置(at)が必要".into(),
            })?;
            let frame = resolve_placement(p, st, ev, &fid)?;
            let n = frame.z;
            let drill = scale(n, -1.0); // 掘り込み方向
            let r = e(ev, d)? / 2.0;

            let (bb_min, bb_max) = solid.bounding_box();
            let diag = frame::norm(sub(bb_max, bb_min));
            let over = diag + 1.0;

            // 工具の確定(Counterboreは座ぐり円柱をフューズし、小径側面をその履歴で写す)
            let (tool, wall_src, bottom_src) = match kind {
                HoleKind::Simple | HoleKind::Tapped => {
                    let small = match depth {
                        HoleDepth::Through => make_cylinder_dir(
                            add(frame.origin, scale(n, over)),
                            drill,
                            r,
                            2.0 * over,
                        ),
                        HoleDepth::Blind(dep) => {
                            let dep = e(ev, dep)?;
                            make_cylinder_dir(add(frame.origin, scale(n, 1.0)), drill, r, dep + 1.0)
                        }
                    };
                    let (side, far, _) = classify_cylinder_faces(&small, drill);
                    let side = side.ok_or_else(|| CompileError::Geometry {
                        feature_id: fid.clone(),
                        message: "工具円柱の側面を同定できません".into(),
                    })?;
                    (small, side, far)
                }
                HoleKind::Counterbore => {
                    let cbd = cb_d.as_ref().ok_or_else(|| CompileError::Geometry {
                        feature_id: fid.clone(),
                        message: "Counterboreには cb_d が必要".into(),
                    })?;
                    let cbdep_e = cb_depth.as_ref().ok_or_else(|| CompileError::Geometry {
                        feature_id: fid.clone(),
                        message: "Counterboreには cb_depth が必要".into(),
                    })?;
                    let cbr = e(ev, cbd)? / 2.0;
                    let cbdep = e(ev, cbdep_e)?;
                    // 小径工具は座ぐり底から0.5mmだけ座ぐり側に食い込ませる
                    // (全通しにするとfuseで小径側面が2分割されAmbiguousになるため)
                    let small_base = add(frame.origin, scale(n, 0.5 - cbdep));
                    let small_len = match depth {
                        HoleDepth::Through => over,
                        HoleDepth::Blind(dep) => {
                            let dep = e(ev, dep)?;
                            if dep <= cbdep {
                                return Err(CompileError::Geometry {
                                    feature_id: fid,
                                    message: format!(
                                        "Counterboreのdepth({dep})はcb_depth({cbdep})より深いこと"
                                    ),
                                });
                            }
                            (dep - cbdep) + 0.5
                        }
                    };
                    let small = make_cylinder_dir(small_base, drill, r, small_len);
                    let (side, far, _) = classify_cylinder_faces(&small, drill);
                    let side = side.ok_or_else(|| CompileError::Geometry {
                        feature_id: fid.clone(),
                        message: "工具円柱の側面を同定できません".into(),
                    })?;
                    let cb = make_cylinder_dir(
                        add(frame.origin, scale(n, 1.0)),
                        drill,
                        cbr,
                        cbdep + 1.0,
                    );
                    let (fused, tool_hist) = small.fuse_with_history(&cb);
                    let wall = match forward_face(side, &tool_hist, &fid) {
                        Provided::Face(f) => f,
                        _ => {
                            return Err(CompileError::Geometry {
                                feature_id: fid,
                                message: "座ぐり工具の合成で小径側面を追跡できません".into(),
                            })
                        }
                    };
                    let bottom = far.and_then(|f| match forward_face(f, &tool_hist, &fid) {
                        Provided::Face(f) => Some(f),
                        _ => None,
                    });
                    (fused, wall, bottom)
                }
                HoleKind::Countersink => {
                    return Err(CompileError::Unsupported {
                        feature_id: fid,
                        what: "CountersinkはM1-3以降(円錐工具)".to_string(),
                    })
                }
            };

            let (result, hist) = solid.cut_with_history(&tool);
            st.forward_all(&hist, &fid);

            let wall = forward_face(wall_src, &hist, &fid);
            // rim: wall面の円エッジのうち配置面に最も近いもの (docs/provides-predicates.md)
            if let Provided::Face(wall_face) = &wall {
                let rim = wall_face
                    .edges()
                    .into_iter()
                    .filter(|e| e.is_circle())
                    .max_by(|a, b| {
                        let pa = dot(sub(a.start(), frame.origin), n);
                        let pb = dot(sub(b.start(), frame.origin), n);
                        pa.partial_cmp(&pb).unwrap()
                    });
                if let Some(rim) = rim {
                    st.insert(&fid, "rim", Provided::Edge(rim));
                }
            }
            st.insert(&fid, "wall", wall);
            if matches!(depth, HoleDepth::Blind(_)) {
                if let Some(b) = bottom_src {
                    st.insert(&fid, "bottom", forward_face(b, &hist, &fid));
                }
            }
            st.insert(
                &fid,
                "axis",
                Provided::Axis {
                    origin: frame.origin,
                    dir: drill,
                },
            );
            st.solid = Some(result);
            Ok(())
        }

        Feature::Pocket {
            id,
            profile,
            depth,
            corner_r,
            at,
        } => {
            let fid = req_id(id, "Pocket")?.to_string();
            let solid = st.solid.take().ok_or_else(|| CompileError::Geometry {
                feature_id: fid.clone(),
                message: "Pocketの前にソリッドが必要".into(),
            })?;
            let p = at.as_ref().ok_or_else(|| CompileError::Geometry {
                feature_id: fid.clone(),
                message: "Pocketには配置(at)が必要".into(),
            })?;
            let frame = resolve_placement(p, st, ev, &fid)?;
            let n = frame.z;
            let drill = scale(n, -1.0);
            let dep = e(ev, depth)?;
            let cr = match corner_r {
                Some(x) => e(ev, x)?,
                None => 0.0,
            };
            let base = add(frame.origin, scale(n, 0.5));
            let tool = profile_tool(profile, &frame, base, drill, dep + 0.5, cr, ev, &fid)?;
            let (sides, far, _near) = classify_prism_faces(&tool, drill);

            let (result, hist) = solid.cut_with_history(&tool);
            st.forward_all(&hist, &fid);
            if let Some(floor) = far {
                st.insert(&fid, "floor", forward_face(floor, &hist, &fid));
            }
            let mut walls = Vec::new();
            for s in sides {
                match forward_face(s, &hist, &fid) {
                    Provided::Face(f) => walls.push(f),
                    Provided::Ambiguous { .. } | Provided::Deleted { .. } => {}
                    _ => {}
                }
            }
            st.insert(&fid, "walls", Provided::FaceSet(walls));
            st.solid = Some(result);
            Ok(())
        }

        Feature::Boss {
            id,
            profile,
            height,
            at,
        } => {
            let fid = req_id(id, "Boss")?.to_string();
            let solid = st.solid.take().ok_or_else(|| CompileError::Geometry {
                feature_id: fid.clone(),
                message: "Bossの前にソリッドが必要".into(),
            })?;
            let p = at.as_ref().ok_or_else(|| CompileError::Geometry {
                feature_id: fid.clone(),
                message: "Bossには配置(at)が必要".into(),
            })?;
            let frame = resolve_placement(p, st, ev, &fid)?;
            let n = frame.z;
            let h = e(ev, height)?;
            let tool = profile_tool(profile, &frame, frame.origin, n, h, 0.0, ev, &fid)?;
            let (sides, far, _near) = classify_prism_faces(&tool, n);

            let (result, hist) = solid.fuse_with_history(&tool);
            st.forward_all(&hist, &fid);
            if let Some(top) = far {
                st.insert(&fid, "top", forward_face(top, &hist, &fid));
            }
            let mut side_set = Vec::new();
            for s in sides {
                if let Provided::Face(f) = forward_face(s, &hist, &fid) {
                    side_set.push(f);
                }
            }
            st.insert(&fid, "side", Provided::FaceSet(side_set));
            st.solid = Some(result);
            Ok(())
        }

        other => Err(CompileError::Unsupported {
            feature_id: other.id().unwrap_or("(無名)").to_string(),
            what: format!(
                "このフィーチャーはM1-2の範囲外です(Fillet/Chamfer=M1-3、Pattern=M1-4、板金=M5): {other:?}"
            ),
        }),
    }
}
