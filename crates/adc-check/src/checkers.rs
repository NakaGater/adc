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
