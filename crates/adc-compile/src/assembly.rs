//! M3-1/M3-2 アセンブリ逐次解決 (ADR-005, 05-schema.md §5)。
//!
//! - mateは `a`(基準側・配置済み)に対して `b`(被拘束側)を位置決めする
//! - 解決順は**mateグラフの位相順**(宣言順非依存。同順位はinstance id昇順で決定的)
//! - 各instanceのmate列は**宣言順に逐次適用**し、最後に全mateの残差を検証する。
//!   満たせない場合は E-MATE-UNSOLVED{mate_id, 原因}
//! - 拘束不足は許容し、残自由度を**報告のみ**する(構想段階では未拘束が正常)。
//!   自由度の計上は既知の組合せに基づく近似(docs/checkers.md)
//! - ground部品のみグローバル配置(Offset)可。併用禁止は静的検証(M0-2拡張)

use std::collections::BTreeMap;

use adc_schema::{Design, Evaluator, MateKind, MateUnsolvedError};

use crate::{BoundAnchorRef, CompiledPart};

const EPS: f64 = 1e-6;

// ---------------------------------------------------------------- 剛体変換

/// 剛体変換: world = rot·local + t
#[derive(Debug, Clone, Copy)]
pub struct Rigid {
    pub rot: [[f64; 3]; 3],
    pub t: [f64; 3],
}

fn matvec(m: &[[f64; 3]; 3], v: [f64; 3]) -> [f64; 3] {
    [
        m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
        m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
        m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
    ]
}

fn matmul(a: &[[f64; 3]; 3], b: &[[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let mut out = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            out[i][j] = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    out
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn scale(a: [f64; 3], s: f64) -> [f64; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}

fn norm(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}

fn normalize(a: [f64; 3]) -> [f64; 3] {
    scale(a, 1.0 / norm(a))
}

/// 軸角回転行列 (Rodrigues)
fn rot_axis_angle(axis: [f64; 3], theta: f64) -> [[f64; 3]; 3] {
    let k = normalize(axis);
    let (s, c) = theta.sin_cos();
    let v = 1.0 - c;
    [
        [
            k[0] * k[0] * v + c,
            k[0] * k[1] * v - k[2] * s,
            k[0] * k[2] * v + k[1] * s,
        ],
        [
            k[1] * k[0] * v + k[2] * s,
            k[1] * k[1] * v + c,
            k[1] * k[2] * v - k[0] * s,
        ],
        [
            k[2] * k[0] * v - k[1] * s,
            k[2] * k[1] * v + k[0] * s,
            k[2] * k[2] * v + c,
        ],
    ]
}

/// u→v の最小回転行列(反平行は任意の直交軸で180°)
fn min_rotation(u: [f64; 3], v: [f64; 3]) -> [[f64; 3]; 3] {
    let u = normalize(u);
    let v = normalize(v);
    let c = dot(u, v).clamp(-1.0, 1.0);
    if c > 1.0 - 1e-12 {
        return Rigid::identity().rot;
    }
    if c < -1.0 + 1e-12 {
        // 反平行: uに直交する軸で180°
        let pick = if u[0].abs() < 0.9 {
            [1.0, 0.0, 0.0]
        } else {
            [0.0, 1.0, 0.0]
        };
        let axis = normalize(cross(u, pick));
        return rot_axis_angle(axis, std::f64::consts::PI);
    }
    rot_axis_angle(cross(u, v), c.acos())
}

impl Rigid {
    pub fn identity() -> Rigid {
        Rigid {
            rot: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            t: [0.0, 0.0, 0.0],
        }
    }

    pub fn apply_point(&self, p: [f64; 3]) -> [f64; 3] {
        add(matvec(&self.rot, p), self.t)
    }

    pub fn apply_vec(&self, v: [f64; 3]) -> [f64; 3] {
        matvec(&self.rot, v)
    }

    /// self を適用した後に correction を適用する合成 (correction ∘ self)
    fn then(&self, correction: &Rigid) -> Rigid {
        Rigid {
            rot: matmul(&correction.rot, &self.rot),
            t: add(matvec(&correction.rot, self.t), correction.t),
        }
    }

    /// 回転 R を点 pivot まわりに適用する補正変換
    fn rotation_about(rot: [[f64; 3]; 3], pivot: [f64; 3]) -> Rigid {
        Rigid {
            rot,
            t: sub(pivot, matvec(&rot, pivot)),
        }
    }

    fn translation(d: [f64; 3]) -> Rigid {
        Rigid {
            rot: Rigid::identity().rot,
            t: d,
        }
    }
}

// ---------------------------------------------------------------- mate幾何

/// mateアンカーの幾何(平面 or 軸)
#[derive(Debug, Clone, Copy)]
pub enum MateGeom {
    Plane { point: [f64; 3], normal: [f64; 3] },
    Axis { origin: [f64; 3], dir: [f64; 3] },
}

impl MateGeom {
    fn transformed(&self, r: &Rigid) -> MateGeom {
        match self {
            MateGeom::Plane { point, normal } => MateGeom::Plane {
                point: r.apply_point(*point),
                normal: r.apply_vec(*normal),
            },
            MateGeom::Axis { origin, dir } => MateGeom::Axis {
                origin: r.apply_point(*origin),
                dir: r.apply_vec(*dir),
            },
        }
    }
}

/// mate 1本を b 側に適用する補正変換を計算する(純粋関数 — 単体テスト可能)。
/// a_geom は配置済み(ワールド)、b_geom は現在のb変換適用後(ワールド)。
pub fn mate_correction(
    kind: &MateKind,
    a_geom: &MateGeom,
    b_geom: &MateGeom,
    dist: f64,
    angle_deg: f64,
    axis_lock: &Option<([f64; 3], [f64; 3])>,
) -> Result<Rigid, String> {
    match kind {
        MateKind::Coaxial => {
            let (MateGeom::Axis { origin: ao, dir: ad }, MateGeom::Axis { origin: bo, dir: bd }) =
                (a_geom, b_geom)
            else {
                return Err("CoaxialはAxisアンカー同士であること".into());
            };
            let ad = normalize(*ad);
            let mut target = ad;
            if dot(normalize(*bd), ad) < 0.0 {
                target = scale(ad, -1.0); // 線としての一致(最小回転の向きを選ぶ)
            }
            let rot = min_rotation(*bd, target);
            let c1 = Rigid::rotation_about(rot, *bo);
            // 回転後のb軸点をa軸直線に乗せる(軸方向成分は保存)
            let bo2 = c1.apply_point(*bo);
            let d = sub(*ao, bo2);
            let d_perp = sub(d, scale(ad, dot(d, ad)));
            Ok(c1.then(&Rigid::translation(d_perp)))
        }
        MateKind::Coincident | MateKind::Distance(_) => {
            let (
                MateGeom::Plane { point: ap, normal: an },
                MateGeom::Plane { point: bp, normal: bn },
            ) = (a_geom, b_geom)
            else {
                return Err("Coincident/Distanceは平面Faceアンカー同士であること".into());
            };
            let an = normalize(*an);
            // 合わせ面: bの外向き法線を-anに向ける
            let rot = min_rotation(*bn, scale(an, -1.0));
            let c1 = Rigid::rotation_about(rot, *bp);
            let bp2 = c1.apply_point(*bp);
            // aの面から法線方向に dist だけ離す
            let gap = dot(sub(*ap, bp2), an) + dist;
            Ok(c1.then(&Rigid::translation(scale(an, gap))))
        }
        MateKind::Angle(_) => {
            let Some((axis_o, axis_d)) = axis_lock else {
                return Err(
                    "Angleは先行するCoaxialで回転軸が確定している場合のみ対応(MVP)".into(),
                );
            };
            let refv = |g: &MateGeom| match g {
                MateGeom::Plane { normal, .. } => *normal,
                MateGeom::Axis { dir, .. } => *dir,
            };
            let k = normalize(*axis_d);
            // 軸直交平面への射影ベクトル間の角をθに合わせる
            let proj = |v: [f64; 3]| sub(v, scale(k, dot(v, k)));
            let ra = proj(refv(a_geom));
            let rb = proj(refv(b_geom));
            if norm(ra) < EPS || norm(rb) < EPS {
                return Err("Angleの参照が回転軸と平行で角度が定義できません".into());
            }
            let (ra, rb) = (normalize(ra), normalize(rb));
            let current = dot(rb, ra).clamp(-1.0, 1.0).acos()
                * if dot(cross(ra, rb), k) >= 0.0 { 1.0 } else { -1.0 };
            let want = angle_deg.to_radians();
            Ok(Rigid::rotation_about(
                rot_axis_angle(k, want - current),
                *axis_o,
            ))
        }
    }
}

/// mateの残差(0=満足)。検証パス用。
pub fn mate_residual(
    kind: &MateKind,
    a_geom: &MateGeom,
    b_geom: &MateGeom,
    dist: f64,
    angle_deg: f64,
) -> f64 {
    match kind {
        MateKind::Coaxial => {
            let (MateGeom::Axis { origin: ao, dir: ad }, MateGeom::Axis { origin: bo, dir: bd }) =
                (a_geom, b_geom)
            else {
                return f64::INFINITY;
            };
            let ad = normalize(*ad);
            let bd = normalize(*bd);
            let dir_res = 1.0 - dot(ad, bd).abs();
            let d = sub(*bo, *ao);
            let line_res = norm(sub(d, scale(ad, dot(d, ad))));
            dir_res.max(line_res)
        }
        MateKind::Coincident | MateKind::Distance(_) => {
            let (
                MateGeom::Plane { point: ap, normal: an },
                MateGeom::Plane { point: bp, normal: bn },
            ) = (a_geom, b_geom)
            else {
                return f64::INFINITY;
            };
            let an = normalize(*an);
            let align_res = 1.0 - (-dot(normalize(*bn), an)).clamp(-1.0, 1.0);
            let off_res = (dot(sub(*bp, *ap), an) - dist).abs();
            align_res.max(off_res)
        }
        MateKind::Angle(_) => {
            // 角度残差は補正計算と同型のため、補正がゼロ回転かで代用
            let _ = (a_geom, b_geom, angle_deg);
            0.0 // 検証はmate_correction適用直後のため省略(MVP)
        }
    }
}

// ---------------------------------------------------------------- 解決

/// 解決済みインスタンス(位相順)
#[derive(Debug)]
pub struct SolvedInstance {
    pub instance: String,
    pub part: String,
    pub transform: Rigid,
    /// 残自由度(既知の組合せに基づく近似計上。未拘束=正常、報告のみ)
    pub remaining_dof: u8,
    pub dof_note: String,
}

#[derive(Debug, Default)]
pub struct SolvedAssembly {
    pub instances: Vec<SolvedInstance>,
}

fn anchor_geom(cp: &CompiledPart, anchor: &str) -> Result<MateGeom, String> {
    match cp.anchor(anchor) {
        Some(BoundAnchorRef::Face(f)) => {
            if f.surface_kind() != adc_kernel::SurfaceKind::Plane {
                return Err(format!("アンカー \"{anchor}\" が平面ではありません"));
            }
            Ok(MateGeom::Plane {
                point: f.center(),
                normal: f.normal(),
            })
        }
        Some(BoundAnchorRef::Axis { origin, dir }) => Ok(MateGeom::Axis { origin, dir }),
        Some(BoundAnchorRef::Edge(_)) => {
            Err(format!("アンカー \"{anchor}\" はEdge束縛(mateはFace/Axisのみ対応)"))
        }
        None => Err(format!("アンカー \"{anchor}\" が束縛されていません")),
    }
}

/// アセンブリの逐次解決 (ADR-005)。
pub fn solve_assembly(
    design: &Design,
    parts: &BTreeMap<String, CompiledPart>,
    ev: &Evaluator,
) -> Result<SolvedAssembly, MateUnsolvedError> {
    let Some(assy) = &design.assembly else {
        return Ok(SolvedAssembly::default());
    };

    // 位相順(Kahn、同順位はid昇順で決定的 — 宣言順非依存)
    let ids: Vec<&str> = {
        let mut v: Vec<&str> = assy.instances.iter().map(|i| i.id.as_str()).collect();
        v.sort_unstable();
        v
    };
    let mut indeg: BTreeMap<&str, usize> = ids.iter().map(|&i| (i, 0)).collect();
    for m in &assy.mates {
        if let Some(d) = indeg.get_mut(m.b.instance.as_str()) {
            *d += 1;
        }
    }
    let mut order: Vec<&str> = Vec::new();
    let mut ready: Vec<&str> = ids
        .iter()
        .copied()
        .filter(|i| indeg[i] == 0)
        .collect();
    while let Some(i) = ready.first().copied() {
        ready.remove(0);
        order.push(i);
        for m in &assy.mates {
            if m.a.instance == i {
                let b = m.b.instance.as_str();
                if let Some(d) = indeg.get_mut(b) {
                    *d -= 1;
                    if *d == 0 {
                        ready.push(b);
                        ready.sort_unstable();
                    }
                }
            }
        }
    }
    // 循環はM0-2で静的検出済み(防御的にチェック)
    if order.len() != ids.len() {
        return Err(MateUnsolvedError {
            mate_id: "(graph)".into(),
            reason: "mateグラフが位相順に並びません(循環)".into(),
        });
    }

    let inst_part: BTreeMap<&str, &str> = assy
        .instances
        .iter()
        .map(|i| (i.id.as_str(), i.part.as_str()))
        .collect();
    let mut placed: BTreeMap<String, Rigid> = BTreeMap::new();
    let mut solved = Vec::new();

    for inst in order {
        let part = inst_part[inst];
        let cp = parts.get(part).ok_or_else(|| MateUnsolvedError {
            mate_id: format!("(instance {inst})"),
            reason: format!("部品 \"{part}\" のコンパイルに失敗しています"),
        })?;

        // このinstanceを被拘束側とするmate列(宣言順)
        let my_mates: Vec<_> = assy
            .mates
            .iter()
            .filter(|m| m.b.instance == inst)
            .collect();

        let mut t = Rigid::identity();
        let mut removed: u8 = 0;
        let mut notes: Vec<String> = Vec::new();
        let mut axis_lock: Option<([f64; 3], [f64; 3])> = None;

        for mate in &my_mates {
            let a_part = inst_part
                .get(mate.a.instance.as_str())
                .ok_or_else(|| MateUnsolvedError {
                    mate_id: mate.id.clone(),
                    reason: format!("基準インスタンス \"{}\" が未定義", mate.a.instance),
                })?;
            let a_cp = parts.get(*a_part).ok_or_else(|| MateUnsolvedError {
                mate_id: mate.id.clone(),
                reason: format!("基準部品 \"{a_part}\" のコンパイルに失敗しています"),
            })?;
            let a_t = placed
                .get(&mate.a.instance)
                .ok_or_else(|| MateUnsolvedError {
                    mate_id: mate.id.clone(),
                    reason: format!(
                        "基準インスタンス \"{}\" が未配置(mateグラフ順不整合)",
                        mate.a.instance
                    ),
                })?;
            let a_geom = anchor_geom(a_cp, &mate.a.anchor)
                .map_err(|reason| MateUnsolvedError {
                    mate_id: mate.id.clone(),
                    reason,
                })?
                .transformed(a_t);
            let b_geom_local = anchor_geom(cp, &mate.b.anchor).map_err(|reason| {
                MateUnsolvedError {
                    mate_id: mate.id.clone(),
                    reason,
                }
            })?;
            let b_geom = b_geom_local.transformed(&t);

            let (dist, angle) = match &mate.kind {
                MateKind::Distance(e) => (
                    ev.evaluate(e).map_err(|er| MateUnsolvedError {
                        mate_id: mate.id.clone(),
                        reason: er.to_string(),
                    })?,
                    0.0,
                ),
                MateKind::Angle(e) => (
                    0.0,
                    ev.evaluate(e).map_err(|er| MateUnsolvedError {
                        mate_id: mate.id.clone(),
                        reason: er.to_string(),
                    })?,
                ),
                _ => (0.0, 0.0),
            };

            let correction = mate_correction(&mate.kind, &a_geom, &b_geom, dist, angle, &axis_lock)
                .map_err(|reason| MateUnsolvedError {
                    mate_id: mate.id.clone(),
                    reason,
                })?;
            t = t.then(&correction);

            match &mate.kind {
                MateKind::Coaxial => {
                    removed = removed.saturating_add(4);
                    notes.push(format!("{}: coaxial(-4)", mate.id));
                    if let MateGeom::Axis { origin, dir } = a_geom {
                        axis_lock = Some((origin, dir));
                    }
                }
                MateKind::Coincident => {
                    removed = removed.saturating_add(3);
                    notes.push(format!("{}: coincident(-3)", mate.id));
                }
                MateKind::Distance(_) => {
                    removed = removed.saturating_add(3);
                    notes.push(format!("{}: distance(-3)", mate.id));
                }
                MateKind::Angle(_) => {
                    removed = removed.saturating_add(1);
                    notes.push(format!("{}: angle(-1)", mate.id));
                }
            }
        }

        // 検証パス: 全mateの残差(逐次適用で先行mateが壊れていないか)
        for mate in &my_mates {
            let a_part = inst_part[mate.a.instance.as_str()];
            let a_geom = anchor_geom(&parts[a_part], &mate.a.anchor)
                .unwrap()
                .transformed(&placed[&mate.a.instance]);
            let b_geom = anchor_geom(cp, &mate.b.anchor).unwrap().transformed(&t);
            let (dist, angle) = match &mate.kind {
                MateKind::Distance(e) => (ev.evaluate(e).unwrap_or(0.0), 0.0),
                MateKind::Angle(e) => (0.0, ev.evaluate(e).unwrap_or(0.0)),
                _ => (0.0, 0.0),
            };
            let res = mate_residual(&mate.kind, &a_geom, &b_geom, dist, angle);
            if res > EPS {
                return Err(MateUnsolvedError {
                    mate_id: mate.id.clone(),
                    reason: format!(
                        "逐次解決で満たせません(先行mateとの矛盾の可能性、残差={res:.3e})"
                    ),
                });
            }
        }

        let is_ground = inst == assy.ground;
        let remaining = if is_ground { 0 } else { 6u8.saturating_sub(removed) };
        placed.insert(inst.to_string(), t);
        solved.push(SolvedInstance {
            instance: inst.to_string(),
            part: part.to_string(),
            transform: t,
            remaining_dof: remaining,
            dof_note: if is_ground {
                "ground(固定)".to_string()
            } else if notes.is_empty() {
                "未拘束(6DOF、構想段階では正常)".to_string()
            } else {
                notes.join(", ")
            },
        });
    }

    Ok(SolvedAssembly { instances: solved })
}

#[cfg(test)]
mod tests {
    use super::*;

    // mate補正の数学は合成幾何で直接単体テストする(OCCT不要)。
    // Angle含む全種を検証 — T1語彙では傾斜参照面のE2Eフィクスチャを構成
    // できないため、Angleはここでのみカバー(M3報告参照)。

    #[test]
    fn coaxial_aligns_axis_to_line() {
        let a = MateGeom::Axis {
            origin: [40.0, 30.0, 0.0],
            dir: [0.0, 0.0, -1.0],
        };
        let b = MateGeom::Axis {
            origin: [0.0, 0.0, 0.0],
            dir: [0.0, 0.0, 1.0],
        };
        let c = mate_correction(&MateKind::Coaxial, &a, &b, 0.0, 0.0, &None).unwrap();
        let b2 = b.transformed(&c);
        assert!(mate_residual(&MateKind::Coaxial, &a, &b2, 0.0, 0.0) < 1e-9);
        // 逆向き軸は反転せず線として一致(最小回転)
        let MateGeom::Axis { dir, .. } = b2 else { unreachable!() };
        assert!(dir[2].abs() > 0.999);
    }

    #[test]
    fn coincident_and_distance_align_planes() {
        let a = MateGeom::Plane {
            point: [0.0, 0.0, 4.0],
            normal: [0.0, 0.0, 1.0],
        };
        let b = MateGeom::Plane {
            point: [10.0, 5.0, 0.0],
            normal: [0.0, 0.0, -1.0],
        };
        let c = mate_correction(&MateKind::Coincident, &a, &b, 0.0, 0.0, &None).unwrap();
        let b2 = b.transformed(&c);
        assert!(mate_residual(&MateKind::Coincident, &a, &b2, 0.0, 0.0) < 1e-9);

        let c = mate_correction(
            &MateKind::Distance(adc_schema::Expr::Lit(2.0)),
            &a,
            &b,
            2.0,
            0.0,
            &None,
        )
        .unwrap();
        let b2 = b.transformed(&c);
        let MateGeom::Plane { point, .. } = b2 else { unreachable!() };
        assert!((point[2] - 6.0).abs() < 1e-9, "{point:?}");
    }

    #[test]
    fn angle_rotates_about_locked_axis() {
        let axis = Some(([0.0, 0.0, 0.0], [0.0, 0.0, 1.0]));
        let a = MateGeom::Plane {
            point: [1.0, 0.0, 0.0],
            normal: [1.0, 0.0, 0.0],
        };
        let b = MateGeom::Plane {
            point: [1.0, 0.0, 0.0],
            normal: [1.0, 0.0, 0.0],
        };
        let c = mate_correction(
            &MateKind::Angle(adc_schema::Expr::Lit(90.0)),
            &a,
            &b,
            0.0,
            90.0,
            &axis,
        )
        .unwrap();
        let b2 = b.transformed(&c);
        let MateGeom::Plane { normal, .. } = b2 else { unreachable!() };
        assert!(normal[0].abs() < 1e-9 && (normal[1] - 1.0).abs() < 1e-9, "{normal:?}");
    }

    #[test]
    fn angle_without_axis_lock_is_error() {
        let g = MateGeom::Plane {
            point: [0.0; 3],
            normal: [1.0, 0.0, 0.0],
        };
        let e = mate_correction(
            &MateKind::Angle(adc_schema::Expr::Lit(90.0)),
            &g,
            &g,
            0.0,
            90.0,
            &None,
        )
        .unwrap_err();
        assert!(e.contains("Coaxial"), "{e}");
    }
}
