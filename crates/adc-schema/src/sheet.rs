//! 板金の派生量 (M5-1, 05-schema.md §4.2、2026-07-12設計メモ承認)。
//!
//! - 曲げ補正 BA = angle_rad × (bend_r + k_factor × t)
//! - 展開長 L_flat = ベース長(曲げ方向のRect寸法)+ Σ(フランジ平坦長) + Σ BA
//! - **K-factorは長さ補正であり質量補正ではない**: 質量特性(Mass/Cog)は
//!   曲げ後ソリッドの体積から直接計算する(材料体積は曲げで保存される)
//! - 全て宣言からの代数計算(ジオメトリ不要)。explainの派生量と
//!   SheetMetalRules (M5-2) が共用する

use serde::Serialize;

use crate::{
    EdgeSelector, Evaluator, Feature, Part, Process, Profile, ProvidedElem, ValidationError,
};

/// 曲げ1つ分の派生量
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BendInfo {
    pub feature_id: String,
    /// 曲げ補正 BA [mm]
    pub ba: f64,
    /// 曲げ角 [度]
    pub angle_deg: f64,
    /// 曲げ内半径 [mm]
    pub bend_r: f64,
    /// フランジ平坦長 [mm]
    pub length: f64,
    /// 曲げ方向('x'/'y' = ベースRectのどの軸の縁か)。面名から判定できない場合None
    pub direction: Option<char>,
    /// 曲げ根元のエッジ側("+x"/"-x"/"+y"/"-y")。SheetMetalRulesのhole_to_bendが使う
    pub side: Option<String>,
}

/// SheetMetal部品の派生量一式
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SheetDerived {
    pub thickness: f64,
    pub k_factor: f64,
    pub bends: Vec<BendInfo>,
    /// 展開長。曲げ方向が混在する部品では未定義(None) — §4.2
    pub flat_length: Option<f64>,
}

/// EdgeSelectorの参照面名から曲げ根元のエッジ側を推定("+x"等)。
/// edges_betweenは両引数のうち側面(±x/±y)側を採用する
fn bend_side(sel: &EdgeSelector) -> Option<String> {
    let of_name = |b: &crate::BindingExpr| match &b.elem {
        ProvidedElem::Face(n) => match n.as_str() {
            "+x" | "-x" | "+y" | "-y" => Some(n.clone()),
            _ => None,
        },
        _ => None,
    };
    match sel {
        EdgeSelector::EdgesOf(b) => of_name(b),
        EdgeSelector::EdgesBetween(a, b) => of_name(a).or_else(|| of_name(b)),
    }
}

/// 部品の板金派生量。SheetMetal工程でなければ None。
/// 式評価の失敗(E-SCHEMA-EVAL)はエラーとして返す(チェッカー文脈ではInconclusive相当)
pub fn sheet_derived(part: &Part, ev: &Evaluator) -> Result<Option<SheetDerived>, ValidationError> {
    let Process::SheetMetal {
        thickness,
        k_factor,
    } = &part.process
    else {
        return Ok(None);
    };
    let t = ev.evaluate(thickness)?;
    let k = *k_factor;

    let mut bends = Vec::new();
    let mut base_dims: Option<(f64, f64)> = None;
    for f in &part.features {
        match f {
            Feature::BaseFlange {
                profile: Profile::Rect { x, y },
                ..
            } => {
                base_dims = Some((ev.evaluate(x)?, ev.evaluate(y)?));
            }
            Feature::Flange {
                id,
                edge,
                angle,
                length,
                bend_r,
                ..
            } => {
                let angle_deg = ev.evaluate(angle)?;
                let r = ev.evaluate(bend_r)?;
                let len = ev.evaluate(length)?;
                let side = bend_side(edge);
                bends.push(BendInfo {
                    feature_id: id.clone().unwrap_or_default(),
                    ba: angle_deg.to_radians() * (r + k * t),
                    angle_deg,
                    bend_r: r,
                    length: len,
                    direction: side.as_ref().and_then(|s| s.chars().nth(1)),
                    side,
                });
            }
            _ => {}
        }
    }

    // 展開長: 全曲げが同一方向のときのみ定義 (§4.2)
    let flat_length = match (&base_dims, bends.is_empty()) {
        (Some((bx, by)), false) => {
            let dirs: Vec<Option<char>> = bends.iter().map(|b| b.direction).collect();
            let first = dirs[0];
            if first.is_some() && dirs.iter().all(|d| *d == first) {
                let base = if first == Some('x') { *bx } else { *by };
                Some(base + bends.iter().map(|b| b.length + b.ba).sum::<f64>())
            } else {
                None
            }
        }
        (Some((bx, _)), true) => Some(*bx), // 曲げなし: ベースのx寸法(平板)
        _ => None,
    };

    Ok(Some(SheetDerived {
        thickness: t,
        k_factor: k,
        bends,
        flat_length,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validate_design;

    /// 展開長の計算式と検証値をテストで固定 (M5-1受入):
    /// t=2, bend_r=3, k=0.44, 90° → BA = (π/2)×(3+0.88) = 6.094689747964198
    #[test]
    fn ba_formula_and_flat_length_fixed() {
        let src = r#"Design(
    schema_version: "0.1",
    intent: "展開長",
    params: [],
    materials: [Material(id: "spcc", density_g_cm3: 7.85, name: "SPCC")],
    parts: [
        Part(id: "cover", material: "spcc",
            process: SheetMetal(thickness: 2.0, k_factor: 0.44),
            features: [
                BaseFlange(id: "web", profile: Rect(x: 50.0, y: 30.0)),
                Flange(id: "lip", edge: edges_between(feature("web").face("top"), feature("web").face("+x")),
                       angle: 90.0, length: 20.0, bend_r: 3.0),
            ],
            anchors: []),
    ],
    assertions: [],
    rationales: [],
)"#;
        let d = validate_design(src).unwrap_or_else(|e| panic!("{e:#?}"));
        let ev = Evaluator::new(&d, &crate::EvalContext::nominal()).unwrap();
        let sd = sheet_derived(&d.parts[0], &ev).unwrap().expect("SheetMetal");
        let ba = std::f64::consts::FRAC_PI_2 * (3.0 + 0.44 * 2.0);
        assert!((ba - 6.094689747964198).abs() < 1e-12, "検証値: {ba}");
        assert!((sd.bends[0].ba - ba).abs() < 1e-12, "{}", sd.bends[0].ba);
        // L_flat = 50 (x方向ベース長) + 20 (平坦長) + BA
        let expect = 50.0 + 20.0 + ba;
        assert!(
            (sd.flat_length.expect("同一方向なので定義される") - expect).abs() < 1e-12
        );
    }

    #[test]
    fn mixed_directions_have_no_flat_length() {
        let src = r#"Design(
    schema_version: "0.1",
    intent: "混在",
    params: [],
    materials: [Material(id: "spcc", density_g_cm3: 7.85, name: "SPCC")],
    parts: [
        Part(id: "cover", material: "spcc",
            process: SheetMetal(thickness: 2.0, k_factor: 0.44),
            features: [
                BaseFlange(id: "web", profile: Rect(x: 50.0, y: 30.0)),
                Flange(id: "f1", edge: edges_between(feature("web").face("top"), feature("web").face("+x")),
                       angle: 90.0, length: 20.0, bend_r: 3.0),
                Flange(id: "f2", edge: edges_between(feature("web").face("top"), feature("web").face("+y")),
                       angle: 90.0, length: 10.0, bend_r: 3.0),
            ],
            anchors: []),
    ],
    assertions: [],
    rationales: [],
)"#;
        let d = validate_design(src).unwrap_or_else(|e| panic!("{e:#?}"));
        let ev = Evaluator::new(&d, &crate::EvalContext::nominal()).unwrap();
        let sd = sheet_derived(&d.parts[0], &ev).unwrap().unwrap();
        assert_eq!(sd.bends.len(), 2);
        assert!(sd.flat_length.is_none(), "曲げ方向混在はL_flat未定義 (§4.2)");
    }
}
