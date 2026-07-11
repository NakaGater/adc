//! チェッカー実装群。marginの定義は docs/checkers.md が正典 (ADR-003)。

use adc_compile::BoundAnchorRef;
use adc_kernel::{min_distance, DistTarget, Solid};
use adc_schema::{Assertion, Check, Evaluator, GeomRef, Scope};

use crate::{q, q3, CheckResult, CheckStatus, Checker, CompiledModel, Evidence, Value};

const OVERLAP_TOL: f64 = 1e-9;

fn part_solid<'m>(
    model: &'m CompiledModel,
    part: &str,
) -> Result<&'m adc_compile::CompiledPart, String> {
    if let Some(cp) = model.parts.get(part) {
        Ok(cp)
    } else if let Some(err) = model.part_errors.get(part) {
        Err(format!("part \"{part}\" のコンパイルに失敗: {err}"))
    } else {
        Err(format!("part \"{part}\" が存在しません"))
    }
}

// ================================================================ BoundingBox

pub struct BoundingBoxChecker;

impl Checker for BoundingBoxChecker {
    fn id(&self) -> &'static str {
        "bounding_box"
    }

    fn check(&self, model: &CompiledModel, ev: &Evaluator, a: &Assertion) -> CheckResult {
        let Check::BoundingBox { part, max } = &a.check else {
            unreachable!()
        };
        let cp = match part_solid(model, part) {
            Ok(cp) => cp,
            Err(reason) => return CheckResult::inconclusive(&a.id, self.id(), reason),
        };
        let limits = match (ev.evaluate(&max.0), ev.evaluate(&max.1), ev.evaluate(&max.2)) {
            (Ok(x), Ok(y), Ok(z)) => [x, y, z],
            (Err(e), ..) | (_, Err(e), _) | (.., Err(e)) => {
                return CheckResult::inconclusive(&a.id, self.id(), e.to_string())
            }
        };
        let (bb_min, bb_max) = cp.solid.bounding_box();
        let sizes = [
            bb_max[0] - bb_min[0],
            bb_max[1] - bb_min[1],
            bb_max[2] - bb_min[2],
        ];
        // margin = 各軸 (limit - size)/|limit| の最小(docs/checkers.md)
        let margin = (0..3)
            .map(|i| (limits[i] - sizes[i]) / limits[i].abs())
            .fold(f64::INFINITY, f64::min);
        let pass = margin >= 0.0;
        let mut evidence = vec![];
        if !pass {
            let over: Vec<String> = (0..3)
                .filter(|&i| sizes[i] > limits[i])
                .map(|i| {
                    format!(
                        "{}軸 {} > {}",
                        ["x", "y", "z"][i],
                        q(sizes[i]),
                        q(limits[i])
                    )
                })
                .collect();
            evidence.push(Evidence {
                anchors: vec![part.clone()],
                points: vec![q3(bb_min), q3(bb_max)],
                note: format!("バウンディングボックス超過: {}", over.join(", ")),
            });
        }
        CheckResult {
            assert_id: a.id.clone(),
            checker: self.id().to_string(),
            status: if pass {
                CheckStatus::Pass
            } else {
                CheckStatus::Fail
            },
            measured: Value::Triple(q3(sizes)),
            threshold: Value::Triple(q3(limits)),
            margin: q(margin),
            evidence,
        }
    }
}

// ================================================================ Clearance

pub struct ClearanceChecker;

enum Target<'m> {
    Solid(&'m Solid),
    Face(&'m adc_kernel::FaceHandle),
}

/// GeomRef → 距離対象(+表示ラベル)。
/// アンカーはFaceのみ対応(エッジ/軸由来のEvidence帰属は gotcha #2 の実在検証を
/// 経た面providesに限る — docs/checkers.md)
fn resolve_target<'m>(model: &'m CompiledModel, g: &GeomRef) -> Result<(Target<'m>, String), String> {
    match g {
        GeomRef::Part(p) => {
            let cp = part_solid(model, p)?;
            Ok((Target::Solid(&cp.solid), p.clone()))
        }
        GeomRef::Anchor(path) => {
            let label = path.to_string();
            let part = model
                .instances
                .iter()
                .find(|(iid, _)| *iid == path.instance)
                .map(|(_, pid)| pid.clone())
                .ok_or_else(|| format!("インスタンス \"{}\" が存在しません", path.instance))?;
            let cp = part_solid(model, &part)?;
            match cp.anchor(&path.anchor) {
                Some(BoundAnchorRef::Face(f)) => Ok((Target::Face(f), label)),
                Some(_) => Err(format!(
                    "アンカー \"{label}\" はFace束縛ではありません(ClearanceのアンカーはFaceのみ対応)"
                )),
                None => Err(format!("アンカー \"{label}\" が束縛されていません")),
            }
        }
    }
}

impl Checker for ClearanceChecker {
    fn id(&self) -> &'static str {
        "clearance"
    }

    fn check(&self, model: &CompiledModel, ev: &Evaluator, a: &Assertion) -> CheckResult {
        let Check::Clearance { a: ga, b: gb, min } = &a.check else {
            unreachable!()
        };
        let min_v = match ev.evaluate(min) {
            Ok(v) => v,
            Err(e) => return CheckResult::inconclusive(&a.id, self.id(), e.to_string()),
        };
        let ((ta, la), (tb, lb)) = match (resolve_target(model, ga), resolve_target(model, gb)) {
            (Ok(x), Ok(y)) => (x, y),
            (Err(e), _) | (_, Err(e)) => {
                return CheckResult::inconclusive(&a.id, self.id(), e)
            }
        };
        fn to_dist<'m>(t: &Target<'m>) -> DistTarget<'m> {
            match t {
                Target::Solid(s) => DistTarget::Solid(s),
                Target::Face(f) => DistTarget::Face(f),
            }
        }
        let (d, p1, p2) = match min_distance(to_dist(&ta), to_dist(&tb)) {
            Ok(r) => r,
            Err(e) => return CheckResult::inconclusive(&a.id, self.id(), e),
        };
        // margin = (measured - min)/|min|(min≈0のときは measured そのもの — docs/checkers.md)
        let margin = if min_v.abs() < 1e-12 {
            d
        } else {
            (d - min_v) / min_v.abs()
        };
        let pass = d >= min_v;
        let evidence = vec![Evidence {
            anchors: vec![la, lb],
            points: vec![q3(p1), q3(p2)],
            note: format!("最小距離 {}(要求 {} 以上)", q(d), q(min_v)),
        }];
        CheckResult {
            assert_id: a.id.clone(),
            checker: self.id().to_string(),
            status: if pass {
                CheckStatus::Pass
            } else {
                CheckStatus::Fail
            },
            measured: Value::Scalar(q(d)),
            threshold: Value::Scalar(q(min_v)),
            margin: q(margin),
            evidence,
        }
    }
}

// ================================================================ NoInterference

pub struct NoInterferenceChecker;

impl Checker for NoInterferenceChecker {
    fn id(&self) -> &'static str {
        "no_interference"
    }

    fn check(&self, model: &CompiledModel, _ev: &Evaluator, a: &Assertion) -> CheckResult {
        let Check::NoInterference { scope } = &a.check else {
            unreachable!()
        };
        // ペア列挙(単部品内は対象外 — Assy全ペア or 明示ペア)
        let pairs: Vec<(String, String)> = match scope {
            Scope::All => {
                let mut v = Vec::new();
                for i in 0..model.instances.len() {
                    for j in (i + 1)..model.instances.len() {
                        v.push((
                            model.instances[i].1.clone(),
                            model.instances[j].1.clone(),
                        ));
                    }
                }
                v
            }
            Scope::Pairs(ps) => ps.clone(),
        };
        if pairs.is_empty() {
            return CheckResult::inconclusive(
                &a.id,
                self.id(),
                "対象ペアなし(単部品内は対象外 — Assyまたはペア指定が必要)",
            );
        }

        let mut total_overlap = 0.0;
        let mut worst_ratio = 0.0f64;
        let mut min_dist = f64::INFINITY;
        let mut evidence = vec![];
        for (pa, pb) in &pairs {
            let (ca, cb) = match (part_solid(model, pa), part_solid(model, pb)) {
                (Ok(x), Ok(y)) => (x, y),
                (Err(e), _) | (_, Err(e)) => {
                    return CheckResult::inconclusive(&a.id, self.id(), e)
                }
            };
            let (common, _hist) = match ca.solid.intersect_with_history(&cb.solid) {
                Ok(r) => r,
                Err(e) => return CheckResult::inconclusive(&a.id, self.id(), e),
            };
            let overlap = common.volume();
            if overlap > OVERLAP_TOL {
                total_overlap += overlap;
                worst_ratio = worst_ratio
                    .max(overlap / ca.solid.volume().min(cb.solid.volume()));
                evidence.push(Evidence {
                    anchors: vec![pa.clone(), pb.clone()],
                    points: vec![q3(common.center_of_mass())],
                    note: format!("交差体積 {} mm^3", q(overlap)),
                });
            } else {
                match min_distance(
                    DistTarget::Solid(&ca.solid),
                    DistTarget::Solid(&cb.solid),
                ) {
                    Ok((d, _, _)) => min_dist = min_dist.min(d),
                    Err(e) => return CheckResult::inconclusive(&a.id, self.id(), e),
                }
            }
        }

        let fail = total_overlap > OVERLAP_TOL;
        // margin: Fail = -(最悪交差体積比)、Pass = 最小ペア距離/結合bbox対角 (docs/checkers.md)
        let margin = if fail {
            -worst_ratio
        } else {
            let mut lo = [f64::INFINITY; 3];
            let mut hi = [f64::NEG_INFINITY; 3];
            for cp in model.parts.values() {
                let (mn, mx) = cp.solid.bounding_box();
                for i in 0..3 {
                    lo[i] = lo[i].min(mn[i]);
                    hi[i] = hi[i].max(mx[i]);
                }
            }
            let diag = ((hi[0] - lo[0]).powi(2) + (hi[1] - lo[1]).powi(2)
                + (hi[2] - lo[2]).powi(2))
            .sqrt();
            if diag > 0.0 {
                min_dist / diag
            } else {
                min_dist
            }
        };
        CheckResult {
            assert_id: a.id.clone(),
            checker: self.id().to_string(),
            status: if fail {
                CheckStatus::Fail
            } else {
                CheckStatus::Pass
            },
            measured: Value::Scalar(q(total_overlap)),
            threshold: Value::Scalar(0.0),
            margin: q(margin),
            evidence,
        }
    }
}

// ================================================================ Mass (M2-3)

pub struct MassChecker;

impl Checker for MassChecker {
    fn id(&self) -> &'static str {
        "mass"
    }

    fn check(&self, model: &CompiledModel, ev: &Evaluator, a: &Assertion) -> CheckResult {
        let Check::Mass { part, max, min } = &a.check else {
            unreachable!()
        };
        let cp = match part_solid(model, part) {
            Ok(cp) => cp,
            Err(reason) => return CheckResult::inconclusive(&a.id, self.id(), reason),
        };
        let Some(Some(density)) = model.part_density.get(part) else {
            return CheckResult::inconclusive(
                &a.id,
                self.id(),
                format!("part \"{part}\" の材料(密度)が未定義です"),
            );
        };
        let max_v = match ev.evaluate(max) {
            Ok(v) => v,
            Err(e) => return CheckResult::inconclusive(&a.id, self.id(), e.to_string()),
        };
        let min_v = match min {
            Some(e) => match ev.evaluate(e) {
                Ok(v) => Some(v),
                Err(e) => return CheckResult::inconclusive(&a.id, self.id(), e.to_string()),
            },
            None => None,
        };
        // 単位換算: volume [mm^3] × density [g/cm^3] ÷ 1000 = mass [g]
        let mass = cp.solid.volume() * density / 1000.0;
        // margin = (max − m)/|max| と(minがあれば)(m − min)/|min| の小さい方
        let mut margin = (max_v - mass) / max_v.abs();
        if let Some(min_v) = min_v {
            margin = margin.min((mass - min_v) / min_v.abs());
        }
        let pass = margin >= 0.0;
        let mut evidence = vec![];
        if !pass {
            let bound = if mass > max_v {
                format!("上限 {} g 超過", q(max_v))
            } else {
                format!("下限 {} g 未満", q(min_v.unwrap_or(0.0)))
            };
            evidence.push(Evidence {
                anchors: vec![part.clone()],
                points: vec![],
                note: format!("質量 {} g が{bound}(密度 {density} g/cm³)", q(mass)),
            });
        }
        CheckResult {
            assert_id: a.id.clone(),
            checker: self.id().to_string(),
            status: if pass {
                CheckStatus::Pass
            } else {
                CheckStatus::Fail
            },
            measured: Value::Scalar(q(mass)),
            threshold: Value::Scalar(q(max_v)),
            margin: q(margin),
            evidence,
        }
    }
}

// ================================================================ Cog (M2-3)

pub struct CogChecker;

impl Checker for CogChecker {
    fn id(&self) -> &'static str {
        "cog"
    }

    fn check(&self, model: &CompiledModel, ev: &Evaluator, a: &Assertion) -> CheckResult {
        let Check::Cog { within } = &a.check else {
            unreachable!()
        };
        // 対象 = Assyがあれば全インスタンス、なければ全部品(恒等配置)の質量加重合成重心
        let bodies: Vec<&String> = if model.instances.is_empty() {
            model.parts.keys().collect()
        } else {
            model.instances.iter().map(|(_, p)| p).collect()
        };
        if bodies.is_empty() {
            return CheckResult::inconclusive(&a.id, self.id(), "対象部品がありません");
        }
        let mut total_m = 0.0;
        let mut moment = [0.0; 3];
        for part in &bodies {
            let cp = match part_solid(model, part) {
                Ok(cp) => cp,
                Err(reason) => return CheckResult::inconclusive(&a.id, self.id(), reason),
            };
            let Some(Some(density)) = model.part_density.get(*part) else {
                return CheckResult::inconclusive(
                    &a.id,
                    self.id(),
                    format!("part \"{part}\" の材料(密度)が未定義です"),
                );
            };
            let m = cp.solid.volume() * density / 1000.0;
            let c = cp.solid.center_of_mass();
            total_m += m;
            for i in 0..3 {
                moment[i] += m * c[i];
            }
        }
        let c = [
            moment[0] / total_m,
            moment[1] / total_m,
            moment[2] / total_m,
        ];
        let lo = match (
            ev.evaluate(&within.min.0),
            ev.evaluate(&within.min.1),
            ev.evaluate(&within.min.2),
        ) {
            (Ok(x), Ok(y), Ok(z)) => [x, y, z],
            (Err(e), ..) | (_, Err(e), _) | (.., Err(e)) => {
                return CheckResult::inconclusive(&a.id, self.id(), e.to_string())
            }
        };
        let hi = match (
            ev.evaluate(&within.max.0),
            ev.evaluate(&within.max.1),
            ev.evaluate(&within.max.2),
        ) {
            (Ok(x), Ok(y), Ok(z)) => [x, y, z],
            (Err(e), ..) | (_, Err(e), _) | (.., Err(e)) => {
                return CheckResult::inconclusive(&a.id, self.id(), e.to_string())
            }
        };
        // margin = min軸 (半幅 − |重心 − box中心|)/半幅
        let mut margin = f64::INFINITY;
        let mut deviating = vec![];
        for i in 0..3 {
            let half = (hi[i] - lo[i]) / 2.0;
            if half <= 0.0 {
                return CheckResult::inconclusive(&a.id, self.id(), "BoxSpecの幅が非正です");
            }
            let dev = (c[i] - (lo[i] + hi[i]) / 2.0).abs();
            margin = margin.min((half - dev) / half);
            if c[i] < lo[i] || c[i] > hi[i] {
                deviating.push(["x", "y", "z"][i]);
            }
        }
        let pass = deviating.is_empty();
        let evidence = vec![Evidence {
            anchors: vec![],
            points: vec![q3(c)],
            note: if pass {
                format!("重心 {:?} は許容box内", q3(c))
            } else {
                format!("重心 {:?} が許容box外(逸脱軸: {})", q3(c), deviating.join(", "))
            },
        }];
        CheckResult {
            assert_id: a.id.clone(),
            checker: self.id().to_string(),
            status: if pass {
                CheckStatus::Pass
            } else {
                CheckStatus::Fail
            },
            measured: Value::Triple(q3(c)),
            threshold: Value::Triple(q3(hi)),
            margin: q(margin),
            evidence,
        }
    }
}

// ================================================================ WallThickness (M2-4)

pub struct WallThicknessChecker;

impl Checker for WallThicknessChecker {
    fn id(&self) -> &'static str {
        "wall_thickness"
    }

    fn check(&self, model: &CompiledModel, ev: &Evaluator, a: &Assertion) -> CheckResult {
        let Check::WallThickness {
            part,
            min,
            sample_density,
        } = &a.check
        else {
            unreachable!()
        };
        let cp = match part_solid(model, part) {
            Ok(cp) => cp,
            Err(reason) => return CheckResult::inconclusive(&a.id, self.id(), reason),
        };
        let min_v = match ev.evaluate(min) {
            Ok(v) => v,
            Err(e) => return CheckResult::inconclusive(&a.id, self.id(), e.to_string()),
        };
        if *sample_density <= 0.0 {
            return CheckResult::inconclusive(&a.id, self.id(), "sample_densityは正であること");
        }
        // 格子間隔 [mm] = 1/√density(docs/checkers.md: 密度は面上の点数/mm²)
        let spacing = 1.0 / sample_density.sqrt();

        let mut n_samples: u64 = 0;
        let mut min_thick = f64::INFINITY;
        let mut worst: Option<([f64; 3], [f64; 3], f64)> = None; // (点, 法線, 厚)
        let mut n_viol: u64 = 0;

        for face in cp.solid.faces() {
            if face.surface_kind() != adc_kernel::SurfaceKind::Plane {
                continue; // M2-4は平面フェイスのみサンプル(docs/checkers.md)
            }
            let n = normalize3(face.normal());
            let c = face.center();
            // 面内軸(placement-framesと同じ決定的規則)
            let x_ref = if dot3(n, [1.0, 0.0, 0.0]).abs() < 1.0 - 1e-6 {
                [1.0, 0.0, 0.0]
            } else {
                [0.0, 1.0, 0.0]
            };
            let u = normalize3(sub3(x_ref, scale3(n, dot3(x_ref, n))));
            let v = cross3(n, u);
            // 面のAABBの8角をu,vへ射影して格子範囲を得る
            let (bmin, bmax) = face.bounding_box();
            let mut ur = (f64::INFINITY, f64::NEG_INFINITY);
            let mut vr = (f64::INFINITY, f64::NEG_INFINITY);
            for cx in [bmin[0], bmax[0]] {
                for cy in [bmin[1], bmax[1]] {
                    for cz in [bmin[2], bmax[2]] {
                        let d = sub3([cx, cy, cz], c);
                        let pu = dot3(d, u);
                        let pv = dot3(d, v);
                        ur = (ur.0.min(pu), ur.1.max(pu));
                        vr = (vr.0.min(pv), vr.1.max(pv));
                    }
                }
            }
            let nu = ((ur.1 - ur.0) / spacing).ceil() as i64;
            let nv = ((vr.1 - vr.0) / spacing).ceil() as i64;
            for iu in 0..=nu {
                for iv in 0..=nv {
                    let pu = ur.0 + iu as f64 * spacing;
                    let pv = vr.0 + iv as f64 * spacing;
                    let p = add3(c, add3(scale3(u, pu), scale3(v, pv)));
                    // 面の外側0.1mmから内向きに照射
                    let origin = add3(p, scale3(n, 0.1));
                    let hits = cp.solid.ray_hits(origin, scale3(n, -1.0));
                    // 最初のヒットがこのサンプル点(t≈0.1)であるレイのみ採用
                    if hits.len() >= 2 && (hits[0].0 - 0.1).abs() < 1e-3 {
                        let thick = hits[1].0 - hits[0].0;
                        n_samples += 1;
                        if thick < min_thick {
                            min_thick = thick;
                            worst = Some((hits[0].1, n, thick));
                        }
                        if thick + 1e-9 < min_v {
                            n_viol += 1;
                        }
                    }
                }
            }
        }

        if n_samples == 0 {
            return CheckResult::inconclusive(
                &a.id,
                self.id(),
                "有効なサンプルが取得できませんでした(平面フェイスなし/密度過小)",
            );
        }
        let pass = n_viol == 0;
        let (wp, wn, wt) = worst.unwrap();
        let guarantee = "※レイキャスト近似の一方向保証: 検出した違反は真、未検出は薄肉なしを保証しない";
        let evidence = vec![Evidence {
            anchors: vec![part.clone()],
            points: vec![q3(wp)],
            note: if pass {
                format!(
                    "最小実測厚 {}(サンプル{}点、密度{}点/mm²) {guarantee}",
                    q(wt),
                    n_samples,
                    sample_density
                )
            } else {
                format!(
                    "実測厚 {} < {}(違反{}点/全{}点、法線 {:?}) {guarantee}",
                    q(wt),
                    q(min_v),
                    n_viol,
                    n_samples,
                    q3(wn)
                )
            },
        }];
        CheckResult {
            assert_id: a.id.clone(),
            checker: self.id().to_string(),
            status: if pass {
                CheckStatus::Pass
            } else {
                CheckStatus::Fail
            },
            measured: Value::Scalar(q(min_thick)),
            threshold: Value::Scalar(q(min_v)),
            margin: q((min_thick - min_v) / min_v.abs()),
            evidence,
        }
    }
}

fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn sub3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn scale3(a: [f64; 3], s: f64) -> [f64; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn cross3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn normalize3(a: [f64; 3]) -> [f64; 3] {
    let n = dot3(a, a).sqrt();
    scale3(a, 1.0 / n)
}

// ================================================================ DatumValidity (M2-5)

pub struct DatumValidityChecker;

impl Checker for DatumValidityChecker {
    fn id(&self) -> &'static str {
        "datum_validity"
    }

    fn check(&self, model: &CompiledModel, _ev: &Evaluator, a: &Assertion) -> CheckResult {
        let Check::DatumValidity { part } = &a.check else {
            unreachable!()
        };
        let cp = match part_solid(model, part) {
            Ok(cp) => cp,
            Err(reason) => return CheckResult::inconclusive(&a.id, self.id(), reason),
        };
        let datums = model.part_datums.get(part).cloned().unwrap_or_default();
        if datums.is_empty() {
            return CheckResult::inconclusive(
                &a.id,
                self.id(),
                format!("part \"{part}\" にDatumアンカーがありません"),
            );
        }
        // 存在+平面性
        let mut normals: Vec<(String, [f64; 3])> = vec![];
        for id in &datums {
            match cp.anchor(id) {
                Some(adc_compile::BoundAnchorRef::Face(f)) => {
                    if f.surface_kind() != adc_kernel::SurfaceKind::Plane {
                        return fail_datum(a, self.id(), &datums, format!("データム \"{id}\" が平面ではありません"));
                    }
                    normals.push((id.clone(), normalize3(f.normal())));
                }
                _ => {
                    return fail_datum(a, self.id(), &datums, format!("データム \"{id}\" がFaceに束縛されていません"))
                }
            }
        }
        // 直交性(複数データム間)
        let mut max_dot = 0.0f64;
        let mut worst_pair = None;
        for i in 0..normals.len() {
            for j in (i + 1)..normals.len() {
                let d = dot3(normals[i].1, normals[j].1).abs();
                if d > max_dot {
                    max_dot = d;
                    worst_pair = Some((normals[i].0.clone(), normals[j].0.clone()));
                }
            }
        }
        let pass = max_dot < 1e-6;
        let evidence = if pass {
            vec![]
        } else {
            let (a1, a2) = worst_pair.unwrap();
            vec![Evidence {
                anchors: vec![a1.clone(), a2.clone()],
                points: vec![],
                note: format!("データム \"{a1}\" と \"{a2}\" が直交していません(|cos|={})", q(max_dot)),
            }]
        };
        CheckResult {
            assert_id: a.id.clone(),
            checker: self.id().to_string(),
            status: if pass {
                CheckStatus::Pass
            } else {
                CheckStatus::Fail
            },
            measured: Value::Scalar(q(max_dot)),
            threshold: Value::Scalar(0.0),
            // margin = 1 − max|cos|(直交からの余裕 — docs/checkers.md)
            margin: q(1.0 - max_dot),
            evidence,
        }
    }
}

fn fail_datum(
    a: &Assertion,
    checker: &str,
    datums: &[String],
    note: String,
) -> CheckResult {
    CheckResult {
        assert_id: a.id.clone(),
        checker: checker.to_string(),
        status: CheckStatus::Fail,
        measured: Value::None,
        threshold: Value::None,
        margin: -1.0,
        evidence: vec![Evidence {
            anchors: datums.to_vec(),
            points: vec![],
            note,
        }],
    }
}
