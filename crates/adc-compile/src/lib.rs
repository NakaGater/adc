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
//! 対象 (M1-2〜M1-4): Block / Cylinder / Hole(Simple/Tapped/Counterbore/Countersink) /
//! Pocket / Boss / Fillet / Chamfer / Pattern(Linear/Linear2D/Circular、添字provides)。
//! EdgeSelectorは**遅延解決**: Fillet/Chamfer/FromEdgeのコンパイル時点で、
//! 前送り済み束縛面の境界辺から導出する。永続的なエッジ台帳は作らない
//! (エッジのHistory追跡は面より弱いため長期参照で運ばない — 2026-07-12決定)。
//! 板金(M5)、非ルート配置のBlock(M1-3以降)は未対応。

pub mod assembly;
mod cache;
mod frame;

pub use cache::{collect_param_ids, compile_part_cached, part_cache_key, BindingTable, CacheOutcome, CachedBinding};

use std::collections::HashMap;
use std::fmt;

use adc_kernel::{
    make_box, make_cone_dir, make_cylinder_dir, make_prism, EdgeHandle, FaceHandle, History,
    Solid, SurfaceKind,
};
use adc_schema::{
    AnchorBindError, AnchorKind, AxisDir, BindingExpr, Count, Design, EdgeSelector, EvalContext,
    Evaluator, Expr, Feature, FeatureFailError, HoleDepth, HoleKind, PatternKind, Placement,
    Pitch, Pos2, Process, Profile, ProvidedElem, ReliefKind, ValidationError,
};
use frame::{
    add, dot, frame_from_origin_normal, normalize, rotate_frame, scale, sub, world_frame, Frame,
};

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
    /// E-FEATURE-FAIL: OCCT操作の失敗 {feature_id, occt_error, hint}。
    /// abortさせずエージェント修復ループの入力にする (US-08)
    FeatureFail(FeatureFailError),
    /// 未対応フィーチャー等
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
            CompileError::FeatureFail(e) => e.fmt(f),
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

fn forward_entry(entry: Provided, h: &History, by: &str, result: &Solid) -> Provided {
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
                    // OCCT実測の穴 (docs/occt-gotchas.md): BRepFilletAPI系のHistoryは
                    // 幾何的に無傷のエッジをIsRemoved=trueと誤報告することがある。
                    // 結果ソリッド内をIsSameで再走査して実在なら束縛を維持する
                    if h.is_removed_edge(&e) && !result.edges().iter().any(|x| x.is_same(&e)) {
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
    fn forward_all(&mut self, h: &History, by: &str, result: &Solid) {
        for entry in self.ledger.values_mut() {
            let old = std::mem::replace(
                entry,
                Provided::Deleted {
                    by_feature: String::new(),
                },
            );
            *entry = forward_entry(old, h, by, result);
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

    /// 束縛表(anchor → 結果B-rep内の部分形状インデックス)を作る (M2-6 —
    /// docs/binding-cache.md。決定的な部分形状列挙順に対するローカル整数)
    pub fn binding_table(&self) -> Result<cache::BindingTable, String> {
        let faces = self.solid.faces();
        let edges = self.solid.edges();
        let mut anchors = std::collections::BTreeMap::new();
        for (anchor_id, key) in &self.anchor_keys {
            let entry = self.ledger.get(key).ok_or("台帳不整合")?;
            let cached = match entry {
                Provided::Face(f) => {
                    let idx = faces
                        .iter()
                        .position(|x| x.is_same(f))
                        .ok_or_else(|| format!("アンカー {anchor_id} の面が結果に見つかりません"))?;
                    cache::CachedBinding::Face { index: idx }
                }
                Provided::Edge(e) => {
                    let idx = edges
                        .iter()
                        .position(|x| x.is_same(e))
                        .ok_or_else(|| format!("アンカー {anchor_id} のエッジが結果に見つかりません"))?;
                    cache::CachedBinding::Edge { index: idx }
                }
                Provided::Axis { origin, dir } => cache::CachedBinding::Axis {
                    origin: *origin,
                    dir: *dir,
                },
                _ => return Err(format!("アンカー {anchor_id} がキャッシュ不能な状態です")),
            };
            anchors.insert(anchor_id.clone(), cached);
        }
        Ok(cache::BindingTable { anchors })
    }

    /// キャッシュ(.brep+束縛表)からの復元 (M2-6)
    pub fn from_cache(
        part_id: &str,
        solid: Solid,
        table: &cache::BindingTable,
    ) -> Result<CompiledPart, String> {
        let faces = solid.faces();
        let edges = solid.edges();
        let mut ledger = HashMap::new();
        let mut anchor_keys = HashMap::new();
        for (anchor_id, cb) in &table.anchors {
            let key = ("__cache__".to_string(), anchor_id.clone());
            let provided = match cb {
                cache::CachedBinding::Face { index } => {
                    // 復元は同一.brepの決定的列挙順に対してのみ有効
                    let f = faces
                        .get(*index)
                        .ok_or_else(|| format!("束縛表の面インデックス{index}が範囲外"))?;
                    // FaceHandleは所有型のためインデックスから再取得
                    let all = solid.faces();
                    let f2 = all.into_iter().nth(*index).unwrap();
                    let _ = f;
                    Provided::Face(f2)
                }
                cache::CachedBinding::Edge { index } => {
                    let _ = edges
                        .get(*index)
                        .ok_or_else(|| format!("束縛表のエッジインデックス{index}が範囲外"))?;
                    let e2 = solid.edges().into_iter().nth(*index).unwrap();
                    Provided::Edge(e2)
                }
                cache::CachedBinding::Axis { origin, dir } => Provided::Axis {
                    origin: *origin,
                    dir: *dir,
                },
            };
            ledger.insert(key.clone(), provided);
            anchor_keys.insert(anchor_id.clone(), key);
        }
        Ok(CompiledPart {
            part_id: part_id.to_string(),
            solid,
            ledger,
            anchor_keys,
        })
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
        compile_feature(f, &mut st, &ev, &part.process)?;
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

/// 正の寸法値を要求する評価(E-FEATURE-FAILの早期検出 — OCCTのDomainErrorをFFI前に防ぐ)
fn e_pos(ev: &Evaluator, x: &Expr, fid: &str, what: &str) -> Result<f64, CompileError> {
    let v = e(ev, x)?;
    if v <= 0.0 {
        return Err(CompileError::FeatureFail(FeatureFailError {
            feature_id: fid.to_string(),
            occt_error: format!("{what} = {v} は不正です"),
            hint: Some(format!("{what} は正の値であること")),
        }));
    }
    Ok(v)
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
                Pos2::FromEdge { edge, d, along } => {
                    let edges = resolve_edges(edge, st, fid)?;
                    if edges.len() != 1 {
                        return Err(CompileError::Geometry {
                            feature_id: fid.to_string(),
                            message: format!(
                                "from_edgeのエッジ選択が1本に定まりません({}本)。edges_between等でより特定的に選択してください",
                                edges.len()
                            ),
                        });
                    }
                    let (d_v, along_v) = (e(ev, d)?, e(ev, along)?);
                    let (s0, t0) = (edges[0].start(), edges[0].end());
                    let m = scale(add(s0, t0), 0.5);
                    let uraw = sub(t0, s0);
                    if frame::norm(uraw) < 1e-9 {
                        return Err(CompileError::Geometry {
                            feature_id: fid.to_string(),
                            message: "from_edgeの対象エッジが退化しています(閉エッジ・点エッジは未対応)".into(),
                        });
                    }
                    // 決定的向き付け: +X/+Y/+Zの順で最初に非直交な軸と正の内積を持つ向き
                    let mut u = normalize(uraw);
                    for axis in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]] {
                        let dp = dot(u, axis);
                        if dp.abs() > 1e-9 {
                            if dp < 0.0 {
                                u = scale(u, -1.0);
                            }
                            break;
                        }
                    }
                    // 面内向き: w = z×u を面重心(生成時点)側に向ける
                    let mut w = frame::cross(frame.z, u);
                    if dot(sub(c, m), w) < 0.0 {
                        w = scale(w, -1.0);
                    }
                    frame.origin = add(add(m, scale(u, along_v)), scale(w, d_v));
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

/// ルートのグローバル配置はワールド軸に限る(回転ルート配置は未対応)
fn require_world_axes(f: &Frame, fid: &str) -> Result<(), CompileError> {
    let ok = (f.x[0] - 1.0).abs() < 1e-9
        && f.x[1].abs() < 1e-9
        && (f.z[2] - 1.0).abs() < 1e-9
        && f.z[0].abs() < 1e-9;
    if ok {
        Ok(())
    } else {
        Err(CompileError::Unsupported {
            feature_id: fid.to_string(),
            what: "ルートの回転配置は未対応(Origin/Offsetのグローバル平行移動のみ)".to_string(),
        })
    }
}

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

/// bindingが指す面グループ(単一面 or 集合)を台帳から引く(遅延解決)
fn binding_faces<'a>(
    st: &'a State,
    b: &BindingExpr,
    fid: &str,
) -> Result<Vec<&'a FaceHandle>, CompileError> {
    let name = match &b.elem {
        ProvidedElem::Face(n) => n.clone(),
        other => {
            return Err(CompileError::Geometry {
                feature_id: fid.to_string(),
                message: format!("エッジ選択の対象はface要素であること: {other:?}"),
            })
        }
    };
    let key = (b.feature.clone(), name);
    match st.ledger.get(&key) {
        None => Err(CompileError::UnknownProvides {
            feature_id: key.0,
            elem: key.1,
        }),
        Some(Provided::Face(f)) => Ok(vec![f]),
        Some(Provided::FaceSet(v)) => Ok(v.iter().collect()),
        Some(Provided::Deleted { by_feature }) => Err(CompileError::Geometry {
            feature_id: fid.to_string(),
            message: format!(
                "エッジ選択の参照面 {}.{} は \"{by_feature}\" の操作で消滅しています",
                key.0, key.1
            ),
        }),
        Some(other) => Err(CompileError::Geometry {
            feature_id: fid.to_string(),
            message: format!(
                "エッジ選択の参照 {}.{} が面ではありません({})",
                key.0,
                key.1,
                other.type_name()
            ),
        }),
    }
}

/// EdgeSelectorの遅延解決 (05-schema.md §4.1、2026-07-12決定):
/// Fillet/Chamfer/FromEdgeのコンパイル時点で、前送り済み束縛面の境界辺から導出する。
/// エッジは長期参照で運ばない(永続的なエッジ台帳は作らない)。
fn resolve_edges(
    sel: &EdgeSelector,
    st: &State,
    fid: &str,
) -> Result<Vec<EdgeHandle>, CompileError> {
    fn edges_of_group(faces: &[&FaceHandle]) -> Vec<EdgeHandle> {
        let mut out: Vec<EdgeHandle> = Vec::new();
        for f in faces {
            for e in f.edges() {
                if !out.iter().any(|x| x.is_same(&e)) {
                    out.push(e);
                }
            }
        }
        out
    }
    match sel {
        EdgeSelector::EdgesOf(b) => {
            // 外周ワイヤのみ (2026-07-12決定: §9サンプルの意図=外形の丸め。
            // 内周ループ=穴リム等は edges_between(wall, face) で選択する)
            let faces = binding_faces(st, b, fid)?;
            let mut out: Vec<EdgeHandle> = Vec::new();
            for f in &faces {
                for e in f.outer_edges() {
                    if !out.iter().any(|x| x.is_same(&e)) {
                        out.push(e);
                    }
                }
            }
            Ok(out)
        }
        EdgeSelector::EdgesBetween(a, b) => {
            let fa = binding_faces(st, a, fid)?;
            let fb = binding_faces(st, b, fid)?;
            let ea = edges_of_group(&fa);
            let eb = edges_of_group(&fb);
            Ok(ea
                .into_iter()
                .filter(|e| eb.iter().any(|x| x.is_same(e)))
                .collect())
        }
    }
}

/// profileから工具プリズムを作る。断面はフレーム平面(baseを含むz直交面)、押出は dir×len。
#[allow(clippy::too_many_arguments)]
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
            let r = e_pos(ev, d, fid, "d")? / 2.0;
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

/// 板金Partの板厚 (M5-1, §4.2: BaseFlangeは工程から板厚を取る)
fn sheet_thickness(process: &Process, ev: &Evaluator, fid: &str) -> Result<f64, CompileError> {
    match process {
        Process::SheetMetal { thickness, .. } => e_pos(ev, thickness, fid, "process.thickness"),
        Process::Machining => Err(CompileError::Unsupported {
            feature_id: fid.to_string(),
            what: "板金フィーチャーは process: SheetMetal のPartのみ (05-schema.md §4.2)".into(),
        }),
    }
}

fn compile_feature(
    f: &Feature,
    st: &mut State,
    ev: &Evaluator,
    process: &Process,
) -> Result<(), CompileError> {
    match f {
        Feature::Block { id, x, y, z, at } => {
            let fid = req_id(id, "Block")?.to_string();
            if st.solid.is_some() {
                return Err(CompileError::Unsupported {
                    feature_id: fid,
                    what: "2つ目のルートソリッドは未対応".to_string(),
                });
            }
            // ルートはグローバル配置(Origin / Offset(Origin基点))を許可 (§4.0)
            let frame = match at {
                None => world_frame(),
                Some(p) => resolve_placement(p, st, ev, &fid)?,
            };
            require_world_axes(&frame, &fid)?;
            let (dx, dy, dz) = (
                e_pos(ev, x, &fid, "x")?,
                e_pos(ev, y, &fid, "y")?,
                e_pos(ev, z, &fid, "z")?,
            );
            let solid = make_box(dx, dy, dz).translated(frame.origin);
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
            let r = e_pos(ev, d, req_id(id, "Cylinder")?, "d")? / 2.0;
            let hh = e_pos(ev, h, req_id(id, "Cylinder")?, "h")?;
            match (&st.solid, at) {
                (None, root_at @ (None | Some(Placement::Origin | Placement::Offset { .. }))) => {
                    let frame = match root_at {
                        None => world_frame(),
                        Some(p) => resolve_placement(p, st, ev, req_id(id, "Cylinder")?)?,
                    };
                    require_world_axes(&frame, req_id(id, "Cylinder")?)?;
                    let dir = axis_dir(axis);
                    let tool = make_cylinder_dir(frame.origin, dir, r, hh);
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
                            origin: frame.origin,
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
                    let (result, hist) = solid.fuse_with_history(&tool).map_err(|occt_error| {
                CompileError::FeatureFail(FeatureFailError {
                    feature_id: fid.clone(),
                    occt_error,
                    hint: Some("付加ブーリアンが失敗しました。工具寸法・配置が対象ソリッドと整合しているか確認してください".into()),
                })
            })?;
                    st.forward_all(&hist, &fid, &result);
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
                (None, Some(Placement::On { .. })) => Err(CompileError::Geometry {
                    feature_id: fid,
                    message: "ルートフィーチャーの配置はグローバル(Origin/Offset)のみ (05-schema.md §4.0)".into(),
                }),
                (Some(_), None) => Err(CompileError::Geometry {
                    feature_id: fid,
                    message: "非ルートフィーチャーには配置(at)が必要".into(),
                }),
            }
        }

        Feature::Hole { id, at, .. } => {
            let fid = req_id(id, "Hole")?.to_string();
            let p = at.as_ref().ok_or_else(|| CompileError::Geometry {
                feature_id: fid.clone(),
                message: "Holeには配置(at)が必要".into(),
            })?;
            let frame = resolve_placement(p, st, ev, &fid)?;
            apply_hole(&fid, f, frame, st, ev)
        }

        Feature::Pocket { id, at, .. } => {
            let fid = req_id(id, "Pocket")?.to_string();
            let p = at.as_ref().ok_or_else(|| CompileError::Geometry {
                feature_id: fid.clone(),
                message: "Pocketには配置(at)が必要".into(),
            })?;
            let frame = resolve_placement(p, st, ev, &fid)?;
            apply_pocket(&fid, f, frame, st, ev)
        }

        Feature::Boss { id, at, .. } => {
            let fid = req_id(id, "Boss")?.to_string();
            let p = at.as_ref().ok_or_else(|| CompileError::Geometry {
                feature_id: fid.clone(),
                message: "Bossには配置(at)が必要".into(),
            })?;
            let frame = resolve_placement(p, st, ev, &fid)?;
            apply_boss(&fid, f, frame, st, ev)
        }

        Feature::Fillet { id, edges, r } => {
            let fid = req_id(id, "Fillet")?.to_string();
            let edge_handles = resolve_edges(edges, st, &fid)?;
            if edge_handles.is_empty() {
                return Err(CompileError::Geometry {
                    feature_id: fid,
                    message: "エッジ選択が空です".into(),
                });
            }
            let r_v = e_pos(ev, r, &fid, "r")?;
            let solid = st.solid.take().ok_or_else(|| CompileError::Geometry {
                feature_id: fid.clone(),
                message: "Filletの前にソリッドが必要".into(),
            })?;
            let refs: Vec<&EdgeHandle> = edge_handles.iter().collect();
            let n_edges = refs.len();
            let (result, hist) =
                solid
                    .fillet_edges_with_history(&refs, r_v)
                    .map_err(|occt_error| {
                        CompileError::FeatureFail(FeatureFailError {
                            feature_id: fid.clone(),
                            occt_error,
                            hint: Some(format!(
                                "フィレット半径 {r_v} が対象辺(計{n_edges}本)の隣接形状に対して過大な可能性があります。隣接面の最小寸法・対象辺の最小曲率半径より小さい半径を試してください"
                            )),
                        })
                    })?;
            st.forward_all(&hist, &fid, &result);
            st.solid = Some(result);
            Ok(())
        }

        Feature::Chamfer { id, edges, size } => {
            let fid = req_id(id, "Chamfer")?.to_string();
            let edge_handles = resolve_edges(edges, st, &fid)?;
            if edge_handles.is_empty() {
                return Err(CompileError::Geometry {
                    feature_id: fid,
                    message: "エッジ選択が空です".into(),
                });
            }
            let size_v = e_pos(ev, size, &fid, "size")?;
            let solid = st.solid.take().ok_or_else(|| CompileError::Geometry {
                feature_id: fid.clone(),
                message: "Chamferの前にソリッドが必要".into(),
            })?;
            let refs: Vec<&EdgeHandle> = edge_handles.iter().collect();
            let n_edges = refs.len();
            let (result, hist) = solid
                .chamfer_edges_with_history(&refs, size_v)
                .map_err(|occt_error| {
                    CompileError::FeatureFail(FeatureFailError {
                        feature_id: fid.clone(),
                        occt_error,
                        hint: Some(format!(
                            "面取り量 {size_v} が対象辺(計{n_edges}本)の隣接形状に対して過大な可能性があります。隣接面の最小寸法より小さい値を試してください"
                        )),
                    })
                })?;
            st.forward_all(&hist, &fid, &result);
            st.solid = Some(result);
            Ok(())
        }

        Feature::Pattern {
            id,
            of,
            kind,
            count,
            pitch,
            axis,
            at,
        } => {
            let pid = req_id(id, "Pattern")?.to_string();
            let p = at.as_ref().ok_or_else(|| CompileError::Geometry {
                feature_id: pid.clone(),
                message: "Patternには配置(at: グリッド中心)が必要".into(),
            })?;
            let base = resolve_placement(p, st, ev, &pid)?;
            let mut frames: Vec<(String, Frame)> = Vec::new();
            match kind {
                PatternKind::Linear => {
                    let Count::One(n) = count else {
                        return Err(CompileError::Geometry {
                            feature_id: pid,
                            message: "Linearパターンのcountはスカラーであること".into(),
                        });
                    };
                    let Pitch::One(pe) = pitch else {
                        return Err(CompileError::Geometry {
                            feature_id: pid,
                            message: "Linearパターンのpitchはスカラーであること".into(),
                        });
                    };
                    let pv = e(ev, pe)?;
                    for i in 0..*n {
                        let off = (i as f64 - (*n as f64 - 1.0) / 2.0) * pv;
                        let mut fr = base;
                        fr.origin = add(base.origin, scale(base.x, off));
                        frames.push((format!("{pid}[{i}]"), fr));
                    }
                }
                PatternKind::Linear2D => {
                    let Count::Two(nx, ny) = count else {
                        return Err(CompileError::Geometry {
                            feature_id: pid,
                            message: "Linear2Dパターンのcountは2要素タプルであること".into(),
                        });
                    };
                    let Pitch::Two(pxe, pye) = pitch else {
                        return Err(CompileError::Geometry {
                            feature_id: pid,
                            message: "Linear2Dパターンのpitchは2要素タプルであること".into(),
                        });
                    };
                    let (pxv, pyv) = (e(ev, pxe)?, e(ev, pye)?);
                    for i in 0..*nx {
                        for j in 0..*ny {
                            let ox = (i as f64 - (*nx as f64 - 1.0) / 2.0) * pxv;
                            let oy = (j as f64 - (*ny as f64 - 1.0) / 2.0) * pyv;
                            let mut fr = base;
                            fr.origin =
                                add(base.origin, add(scale(base.x, ox), scale(base.y, oy)));
                            frames.push((format!("{pid}[{i}][{j}]"), fr));
                        }
                    }
                }
                PatternKind::Circular => {
                    let Count::One(n) = count else {
                        return Err(CompileError::Geometry {
                            feature_id: pid,
                            message: "Circularパターンのcountはスカラーであること".into(),
                        });
                    };
                    let Pitch::One(pe) = pitch else {
                        return Err(CompileError::Geometry {
                            feature_id: pid,
                            message: "Circularパターンのpitch(角度step、度)はスカラーであること".into(),
                        });
                    };
                    let step = e(ev, pe)?.to_radians();
                    let ab = axis.as_ref().ok_or_else(|| CompileError::Geometry {
                        feature_id: pid.clone(),
                        message: "Circularパターンにはaxis(回転軸のprovides参照)が必要".into(),
                    })?;
                    let aname = match &ab.elem {
                        ProvidedElem::Axis(n) => n.clone(),
                        other => {
                            return Err(CompileError::Geometry {
                                feature_id: pid,
                                message: format!("Circularのaxisはaxis要素であること: {other:?}"),
                            })
                        }
                    };
                    let key = (ab.feature.clone(), aname);
                    let (ao, ad) = match st.ledger.get(&key) {
                        Some(Provided::Axis { origin, dir }) => (*origin, *dir),
                        Some(other) => {
                            return Err(CompileError::Geometry {
                                feature_id: pid,
                                message: format!(
                                    "Circularのaxis参照 {}.{} が軸ではありません({})",
                                    key.0,
                                    key.1,
                                    other.type_name()
                                ),
                            })
                        }
                        None => {
                            return Err(CompileError::UnknownProvides {
                                feature_id: key.0,
                                elem: key.1,
                            })
                        }
                    };
                    for k in 0..*n {
                        let fr = rotate_frame(&base, ao, ad, k as f64 * step);
                        frames.push((format!("{pid}[{k}]"), fr));
                    }
                }
            }
            for (key, fr) in frames {
                match of.as_ref() {
                    Feature::Hole { .. } => apply_hole(&key, of, fr, st, ev)?,
                    Feature::Pocket { .. } => apply_pocket(&key, of, fr, st, ev)?,
                    Feature::Boss { .. } => apply_boss(&key, of, fr, st, ev)?,
                    other => {
                        return Err(CompileError::Unsupported {
                            feature_id: pid,
                            what: format!(
                                "Pattern内フィーチャーはHole/Pocket/Bossのみ対応 (M1-4): {other:?}"
                            ),
                        })
                    }
                }
            }
            Ok(())
        }

        // ---- T2 板金 (M5-1, docs/design-notes/m5-1-sheet-metal.md 案B) ----
        Feature::BaseFlange { id, profile, at } => {
            let fid = req_id(id, "BaseFlange")?.to_string();
            if st.solid.is_some() {
                return Err(CompileError::Unsupported {
                    feature_id: fid,
                    what: "BaseFlangeはルート(先頭フィーチャー)専用 (05-schema.md §4.2)".into(),
                });
            }
            let t = sheet_thickness(process, ev, &fid)?;
            let frame = match at {
                None => world_frame(),
                Some(p) => resolve_placement(p, st, ev, &fid)?,
            };
            require_world_axes(&frame, &fid)?;
            let Profile::Rect { x, y } = profile else {
                return Err(CompileError::Unsupported {
                    feature_id: fid,
                    what: "BaseFlangeのprofileはMVPではRectのみ (05-schema.md §4.2)".into(),
                });
            };
            let (hx, hy) = (e_pos(ev, x, &fid, "x")? / 2.0, e_pos(ev, y, &fid, "y")? / 2.0);
            // profile中心=配置原点 (§4.2 — Blockの角原点と異なる)
            let o = frame.origin;
            let corners = [
                add(o, [hx, hy, 0.0]),
                add(o, [-hx, hy, 0.0]),
                add(o, [-hx, -hy, 0.0]),
                add(o, [hx, -hy, 0.0]),
            ];
            let solid =
                make_prism(&corners, 0.0, [0.0, 0.0, t]).map_err(|m| CompileError::Geometry {
                    feature_id: fid.clone(),
                    message: m,
                })?;
            // provides: Blockと同一の法線分類 (docs/provides-predicates.md T2)
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

        Feature::Flange {
            id,
            edge,
            angle,
            length,
            bend_r,
        } => {
            let fid = req_id(id, "Flange")?.to_string();
            let t = sheet_thickness(process, ev, &fid)?;
            let alpha_deg = e_pos(ev, angle, &fid, "angle")?;
            if alpha_deg >= 180.0 {
                return Err(CompileError::FeatureFail(FeatureFailError {
                    feature_id: fid,
                    occt_error: format!("angle {alpha_deg}° は範囲外"),
                    hint: Some("Flangeのangleは (0°, 180°) (05-schema.md §4.2)".into()),
                }));
            }
            let alpha = alpha_deg.to_radians();
            let len = e_pos(ev, length, &fid, "length")?;
            let r = e_pos(ev, bend_r, &fid, "bend_r")?;

            // 曲げエッジ: 直線1本に解決されること
            let edges = resolve_edges(edge, st, &fid)?;
            if edges.len() != 1 {
                return Err(CompileError::FeatureFail(FeatureFailError {
                    feature_id: fid,
                    occt_error: format!("曲げエッジが{}本に解決されました", edges.len()),
                    hint: Some(
                        "edges_between(<面>, <面>) で直線エッジ1本に特定してください (05-schema.md §4.2)".into(),
                    ),
                }));
            }
            let e0 = &edges[0];
            if e0.is_circle() {
                return Err(CompileError::FeatureFail(FeatureFailError {
                    feature_id: fid,
                    occt_error: "曲げエッジが直線ではありません".into(),
                    hint: Some("Flangeは直線エッジのみ対応 (05-schema.md §4.2)".into()),
                }));
            }
            let (p0, p1) = (e0.start(), e0.end());
            let ev_vec = sub(p1, p0);
            let w = dot(ev_vec, ev_vec).sqrt();
            let u = normalize(ev_vec);

            // 曲げ向き (docs/provides-predicates.md T2): 選択面(第1引数)の反対側へ。
            // 隣接2面: 選択面の法線 = 回転始点、相手面の法線 = 張出方向
            let solid_ref = st.solid.as_ref().ok_or_else(|| CompileError::Geometry {
                feature_id: fid.clone(),
                message: "Flangeの前にBaseFlangeが必要".into(),
            })?;
            let adj: Vec<FaceHandle> = solid_ref
                .faces()
                .into_iter()
                .filter(|f| f.edges().iter().any(|e| e.is_same(e0)))
                .collect();
            let first_b = match edge {
                EdgeSelector::EdgesOf(b) => b,
                EdgeSelector::EdgesBetween(a, _) => a,
            };
            let sel_faces = binding_faces(st, first_b, &fid)?;
            let from_face = adj
                .iter()
                .find(|f| sel_faces.iter().any(|sf| sf.is_same(f)));
            let (z_hat, d_out) = match (from_face, adj.len()) {
                (Some(ff), 2) => {
                    let other = adj.iter().find(|f| !f.is_same(ff)).unwrap();
                    (normalize(ff.normal()), normalize(other.normal()))
                }
                _ => {
                    return Err(CompileError::FeatureFail(FeatureFailError {
                        feature_id: fid,
                        occt_error: format!(
                            "曲げエッジの隣接面を特定できません(隣接{}面)",
                            adj.len()
                        ),
                        hint: Some("エッジ選択の第1引数を板面(top/bottom)にしてください".into()),
                    }))
                }
            };
            if dot(z_hat, u).abs() > 1e-6 || dot(d_out, u).abs() > 1e-6 || dot(z_hat, d_out).abs() > 1e-6
            {
                return Err(CompileError::FeatureFail(FeatureFailError {
                    feature_id: fid,
                    occt_error: "曲げエッジと隣接面が直交していません".into(),
                    hint: Some("MVPのFlangeは矩形ベースの直交エッジのみ対応".into()),
                }));
            }

            // 曲げ部: 軸=エッジ平行、内半径r。エッジは選択面側の稜線なので
            // 軸位置 = エッジ − (t + r)·ẑ (docs/design-notes/m5-1-sheet-metal.md)
            let a0 = add(p0, scale(z_hat, -(t + r)));
            let ring_err = |occt_error: String| {
                CompileError::FeatureFail(FeatureFailError {
                    feature_id: fid.clone(),
                    occt_error,
                    hint: Some("曲げ部の構築に失敗しました。bend_r・板厚・エッジ長を確認してください".into()),
                })
            };
            let outer = make_cylinder_dir(a0, u, r + t, w);
            let inner = make_cylinder_dir(sub(a0, scale(u, 0.25)), u, r, w + 0.5);
            let (tube, _) = outer.cut_with_history(&inner).map_err(&ring_err)?;
            // 扇形を含む凸ウェッジ(α<180°): dir(θ) = ẑcosθ + d̂sinθ
            let dir_th = |th: f64| add(scale(z_hat, th.cos()), scale(d_out, th.sin()));
            let big = 2.0 * (r + t) + 1.0;
            let rp = big / (alpha / 2.0).cos();
            let wb = sub(a0, scale(u, 0.25));
            let wedge_pts = [
                wb,
                add(wb, scale(dir_th(0.0), big)),
                add(wb, scale(dir_th(alpha / 2.0), rp)),
                add(wb, scale(dir_th(alpha), big)),
            ];
            let wedge = make_prism(&wedge_pts, 0.0, scale(u, w + 0.5))
                .map_err(&ring_err)?;
            let (sector, _) = tube.intersect_with_history(&wedge).map_err(&ring_err)?;

            // 曲げ円筒面の同定(工具上、軸からの重心距離順: 近=inner / 遠=outer)
            let dist_to_axis = |c: [f64; 3]| {
                let d = sub(c, a0);
                let along = dot(d, u);
                let radial = sub(d, scale(u, along));
                dot(radial, radial).sqrt()
            };
            let mut cyl: Vec<(f64, FaceHandle)> = sector
                .faces()
                .into_iter()
                .filter(|f| f.surface_kind() == SurfaceKind::Cylinder)
                .map(|f| (dist_to_axis(f.center()), f))
                .collect();
            cyl.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
            if cyl.len() != 2 {
                return Err(CompileError::FeatureFail(FeatureFailError {
                    feature_id: fid,
                    occt_error: format!("曲げ部の円筒面が{}枚(期待2枚)", cyl.len()),
                    hint: Some("曲げ部形状が退化しています。bend_r・angleを確認してください".into()),
                }));
            }
            let bend_outer_f = cyl.pop().unwrap().1;
            let bend_inner_f = cyl.pop().unwrap().1;

            // 平坦部: 曲げ終端の接平面上のスラブ(断面共有で接合 — 接線接触ではない)
            let dir_a = dir_th(alpha);
            let tang = add(scale(z_hat, -alpha.sin()), scale(d_out, alpha.cos()));
            let p_in = add(a0, scale(dir_a, r));
            let p_out = add(a0, scale(dir_a, r + t));
            let slab_pts = [
                p_in,
                p_out,
                add(p_out, scale(tang, len)),
                add(p_in, scale(tang, len)),
            ];
            let slab = make_prism(&slab_pts, 0.0, scale(u, w)).map_err(&ring_err)?;
            let mut inner_f = None;
            let mut outer_f = None;
            let mut tip_f = None;
            for f in slab.faces() {
                let n = normalize(f.normal());
                if dot(n, dir_a) < -1.0 + 1e-6 {
                    inner_f = Some(f);
                } else if dot(n, dir_a) > 1.0 - 1e-6 {
                    outer_f = Some(f);
                } else if dot(n, tang) > 1.0 - 1e-6 {
                    tip_f = Some(f);
                }
            }

            // フューズ(2段): base ∪ 曲げ部 ∪ 平坦部。providesはHistoryで前送り
            let fuse_err = |occt_error: String| {
                CompileError::FeatureFail(FeatureFailError {
                    feature_id: fid.clone(),
                    occt_error,
                    hint: Some("フランジの接合ブーリアンが失敗しました".into()),
                })
            };
            let solid = st.solid.take().unwrap();
            let (s1, h1) = solid.fuse_with_history(&sector).map_err(&fuse_err)?;
            st.forward_all(&h1, &fid, &s1);
            let bend_inner_p = forward_face(bend_inner_f, &h1, &fid);
            let bend_outer_p = forward_face(bend_outer_f, &h1, &fid);
            let (s2, h2) = s1.fuse_with_history(&slab).map_err(&fuse_err)?;
            st.forward_all(&h2, &fid, &s2);
            st.insert(&fid, "bend_inner", forward_entry(bend_inner_p, &h2, &fid, &s2));
            st.insert(&fid, "bend_outer", forward_entry(bend_outer_p, &h2, &fid, &s2));
            if let Some(f) = inner_f {
                st.insert(&fid, "inner", forward_face(f, &h2, &fid));
            }
            if let Some(f) = outer_f {
                st.insert(&fid, "outer", forward_face(f, &h2, &fid));
            }
            if let Some(f) = tip_f {
                st.insert(&fid, "tip", forward_face(f, &h2, &fid));
            }
            st.solid = Some(s2);
            Ok(())
        }

        Feature::Cutout { id, profile, at } => {
            let fid = req_id(id, "Cutout")?.to_string();
            let t = sheet_thickness(process, ev, &fid)?;
            let p = at.as_ref().ok_or_else(|| CompileError::Geometry {
                feature_id: fid.clone(),
                message: "非ルートフィーチャーには配置(at)が必要".into(),
            })?;
            let frame = resolve_placement(p, st, ev, &fid)?;
            apply_through_cut(&fid, profile, &frame, t, st, ev, true)
        }

        Feature::Relief { id, kind, at } => {
            let fid = req_id(id, "Relief")?.to_string();
            let t = sheet_thickness(process, ev, &fid)?;
            let p = at.as_ref().ok_or_else(|| CompileError::Geometry {
                feature_id: fid.clone(),
                message: "非ルートフィーチャーには配置(at)が必要".into(),
            })?;
            let frame = resolve_placement(p, st, ev, &fid)?;
            // 実装はCutoutの特殊形 (§4.2)。providesは載せない
            let profile = match kind {
                ReliefKind::Rect { w, d } => Profile::Rect {
                    x: w.clone(),
                    y: d.clone(),
                },
                ReliefKind::Round { d } => Profile::Circ { d: d.clone() },
            };
            apply_through_cut(&fid, &profile, &frame, t, st, ev, false)
        }

    }
}

/// 板厚貫通の切欠き (Cutout / Relief 共通)。providesはCutoutのみ (§4.2)
fn apply_through_cut(
    fid: &str,
    profile: &Profile,
    frame: &Frame,
    t: f64,
    st: &mut State,
    ev: &Evaluator,
    with_provides: bool,
) -> Result<(), CompileError> {
    let solid = st.solid.take().ok_or_else(|| CompileError::Geometry {
        feature_id: fid.to_string(),
        message: "切欠きの前にソリッドが必要".into(),
    })?;
    let n = frame.z;
    let drill = scale(n, -1.0);
    let base = add(frame.origin, scale(n, 0.5));
    let tool = profile_tool(profile, frame, base, drill, t + 1.0, 0.0, ev, fid)?;
    let (sides, _, _) = classify_prism_faces(&tool, drill);
    let (result, hist) = solid.cut_with_history(&tool).map_err(|occt_error| {
        CompileError::FeatureFail(FeatureFailError {
            feature_id: fid.to_string(),
            occt_error,
            hint: Some("切欠きブーリアンが失敗しました。工具寸法・配置を確認してください".into()),
        })
    })?;
    st.forward_all(&hist, fid, &result);
    if with_provides {
        match profile {
            Profile::Circ { .. } => {
                if let Some(s) = sides.into_iter().next() {
                    st.insert(fid, "wall", forward_face(s, &hist, fid));
                }
            }
            Profile::Rect { .. } => {
                let mut walls = Vec::new();
                for s in sides {
                    if let Provided::Face(f) = forward_face(s, &hist, fid) {
                        walls.push(f);
                    }
                }
                st.insert(fid, "walls", Provided::FaceSet(walls));
            }
        }
    }
    st.solid = Some(result);
    Ok(())
}

fn apply_hole(
    key: &str,
    f: &Feature,
    frame: Frame,
    st: &mut State,
    ev: &Evaluator,
) -> Result<(), CompileError> {
    let Feature::Hole {
        kind,
        d,
        depth,
        cb_d,
        cb_depth,
        cs_d,
        cs_angle,
        ..
    } = f
    else {
        unreachable!("apply_holeはHoleのみ")
    };
    let fid = key.to_string();
    let solid = st.solid.take().ok_or_else(|| CompileError::Geometry {
        feature_id: fid.clone(),
        message: "Holeの前にソリッドが必要".into(),
    })?;
    {
            let n = frame.z;
            let drill = scale(n, -1.0); // 掘り込み方向
            let r = e_pos(ev, d, &fid, "d")? / 2.0;

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
                    let (fused, tool_hist) = small.fuse_with_history(&cb).map_err(|occt_error| {
                        CompileError::FeatureFail(FeatureFailError {
                            feature_id: fid.clone(),
                            occt_error,
                            hint: None,
                        })
                    })?;
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
                    let csd = cs_d.as_ref().ok_or_else(|| CompileError::Geometry {
                        feature_id: fid.clone(),
                        message: "Countersinkには cs_d が必要".into(),
                    })?;
                    let csang = cs_angle.as_ref().ok_or_else(|| CompileError::Geometry {
                        feature_id: fid.clone(),
                        message: "Countersinkには cs_angle(全頂角、度)が必要".into(),
                    })?;
                    let cs_r = e(ev, csd)? / 2.0;
                    let half = e(ev, csang)?.to_radians() / 2.0;
                    if cs_r <= r || half <= 0.0 {
                        return Err(CompileError::Geometry {
                            feature_id: fid,
                            message: "Countersinkは cs_d > d かつ cs_angle > 0 であること".into(),
                        });
                    }
                    // 皿もみ深さ
                    let t_cs = (cs_r - r) / half.tan();
                    let small_base = add(frame.origin, scale(n, 0.5 - t_cs));
                    let small_len = match depth {
                        HoleDepth::Through => over,
                        HoleDepth::Blind(dep) => {
                            let dep = e(ev, dep)?;
                            if dep <= t_cs {
                                return Err(CompileError::Geometry {
                                    feature_id: fid,
                                    message: format!(
                                        "Countersinkのdepth({dep})は皿もみ深さ({t_cs:.3})より深いこと"
                                    ),
                                });
                            }
                            (dep - t_cs) + 0.5
                        }
                    };
                    let small = make_cylinder_dir(small_base, drill, r, small_len);
                    let (side, far, _) = classify_cylinder_faces(&small, drill);
                    let side = side.ok_or_else(|| CompileError::Geometry {
                        feature_id: fid.clone(),
                        message: "工具円柱の側面を同定できません".into(),
                    })?;
                    // 円錐工具(表面から0.5mm上に拡張、テーパー一致)
                    let cone_base = add(frame.origin, scale(n, 0.5));
                    let r_at_base = cs_r + 0.5 * half.tan();
                    let cone = make_cone_dir(cone_base, drill, r_at_base, r, t_cs + 0.5);
                    let (fused, tool_hist) = small.fuse_with_history(&cone).map_err(|occt_error| {
                        CompileError::FeatureFail(FeatureFailError {
                            feature_id: fid.clone(),
                            occt_error,
                            hint: None,
                        })
                    })?;
                    let wall = match forward_face(side, &tool_hist, &fid) {
                        Provided::Face(f) => f,
                        _ => {
                            return Err(CompileError::Geometry {
                                feature_id: fid,
                                message: "皿もみ工具の合成で小径側面を追跡できません".into(),
                            })
                        }
                    };
                    let bottom = far.and_then(|f| match forward_face(f, &tool_hist, &fid) {
                        Provided::Face(f) => Some(f),
                        _ => None,
                    });
                    (fused, wall, bottom)
                }
            };

            let (result, hist) = solid.cut_with_history(&tool).map_err(|occt_error| {
                CompileError::FeatureFail(FeatureFailError {
                    feature_id: fid.clone(),
                    occt_error,
                    hint: Some("切削ブーリアンが失敗しました。工具寸法・配置が対象ソリッドと整合しているか確認してください".into()),
                })
            })?;
            st.forward_all(&hist, &fid, &result);

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
}

fn apply_pocket(
    key: &str,
    f: &Feature,
    frame: Frame,
    st: &mut State,
    ev: &Evaluator,
) -> Result<(), CompileError> {
    let Feature::Pocket {
        profile,
        depth,
        corner_r,
        ..
    } = f
    else {
        unreachable!("apply_pocketはPocketのみ")
    };
    let fid = key.to_string();
    let solid = st.solid.take().ok_or_else(|| CompileError::Geometry {
        feature_id: fid.clone(),
        message: "Pocketの前にソリッドが必要".into(),
    })?;
    {
            let n = frame.z;
            let drill = scale(n, -1.0);
            let dep = e_pos(ev, depth, &fid, "depth")?;
            let cr = match corner_r {
                Some(x) => e(ev, x)?,
                None => 0.0,
            };
            let base = add(frame.origin, scale(n, 0.5));
            let tool = profile_tool(profile, &frame, base, drill, dep + 0.5, cr, ev, &fid)?;
            let (sides, far, _near) = classify_prism_faces(&tool, drill);

            let (result, hist) = solid.cut_with_history(&tool).map_err(|occt_error| {
                CompileError::FeatureFail(FeatureFailError {
                    feature_id: fid.clone(),
                    occt_error,
                    hint: Some("切削ブーリアンが失敗しました。工具寸法・配置が対象ソリッドと整合しているか確認してください".into()),
                })
            })?;
            st.forward_all(&hist, &fid, &result);
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
}

fn apply_boss(
    key: &str,
    f: &Feature,
    frame: Frame,
    st: &mut State,
    ev: &Evaluator,
) -> Result<(), CompileError> {
    let Feature::Boss {
        profile, height, ..
    } = f
    else {
        unreachable!("apply_bossはBossのみ")
    };
    let fid = key.to_string();
    let solid = st.solid.take().ok_or_else(|| CompileError::Geometry {
        feature_id: fid.clone(),
        message: "Bossの前にソリッドが必要".into(),
    })?;
    {
            let n = frame.z;
            let h = e_pos(ev, height, &fid, "height")?;
            let tool = profile_tool(profile, &frame, frame.origin, n, h, 0.0, ev, &fid)?;
            let (sides, far, _near) = classify_prism_faces(&tool, n);

            let (result, hist) = solid.fuse_with_history(&tool).map_err(|occt_error| {
                CompileError::FeatureFail(FeatureFailError {
                    feature_id: fid.clone(),
                    occt_error,
                    hint: Some("付加ブーリアンが失敗しました。工具寸法・配置が対象ソリッドと整合しているか確認してください".into()),
                })
            })?;
            st.forward_all(&hist, &fid, &result);
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
}
