//! M0-4 explain (US-03, US-04)。
//!
//! 定義本体+rationale連鎖+逆参照(参照元一覧)をJSON化可能な構造で返す。
//! 出力スキーマは docs/explain-schema.md で確定し、以後後方互換を維持する。
//!
//! - 種別横断検索。複数ヒット(種別間の同名、Part内スコープの同名feature/anchor)は
//!   status: ambiguous として候補全件を返す (05-schema.md §1.1)
//! - referenced_by = 直接の構造的参照のみ(式・binding・AnchorPath・ID参照。
//!   変更が機械的に伝播する硬い依存)
//! - related = rationale共有などの意味的関連(via付き。柔らかい連想 — 根拠の連鎖 US-04)。
//!   エージェントの影響調査で両者の扱いが異なるため、リストを分離して誤消費を防ぐ

use std::collections::HashMap;

use serde::Serialize;
use serde_json::Value;

use crate::{
    Check, Design, EdgeSelector, Expr, Feature, GeomRef, MateKind, ParamValue, Pitch, Placement,
    Pos2, Process, Scope,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExplainStatus {
    Found,
    Ambiguous,
    NotFound,
}

/// explain のトップレベル出力 (docs/explain-schema.md)
#[derive(Debug, Clone, Serialize)]
pub struct ExplainOutput {
    /// explain出力スキーマのバージョン(後方互換の対象)
    pub schema_version: String,
    pub query: String,
    pub status: ExplainStatus,
    /// found: 1件 / ambiguous: 候補全件 / not_found: 空
    pub matches: Vec<Explanation>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Explanation {
    pub kind: String,
    pub id: String,
    /// feature / anchor のスコープ (§1.1)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub part: Option<String>,
    /// 定義本体のJSON表現
    pub definition: Value,
    /// 根拠の連鎖(現状は直接rationaleの1段。Lesson参照の追跡は将来拡張)
    pub rationale_chain: Vec<Value>,
    /// 直接の構造的参照(硬い依存)
    pub referenced_by: Vec<RefSite>,
    /// 意味的関連(rationale共有等。柔らかい連想)
    pub related: Vec<RefSite>,
}

/// 参照元(逆参照)の1サイト
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RefSite {
    pub kind: String,
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub part: Option<String>,
    /// 参照箇所: フィールド名("z")、"binding"、"check"、"rationale:<id>" 等
    pub via: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Target {
    Param(String),
    Rationale(String),
    Material(String),
    Part(String),
    Feature(String, String), // (part, feature)
    Anchor(String, String),  // (part, anchor)
    Instance(String),
    Dim(String),
}

/// IDを全種別横断で検索し、定義+rationale連鎖+参照元を返す
pub fn explain(design: &Design, query: &str) -> ExplainOutput {
    let edges = reverse_index(design);
    let mut matches: Vec<Explanation> = Vec::new();

    let refs_of = |t: &Target| -> Vec<RefSite> {
        let mut out: Vec<RefSite> = Vec::new();
        for (target, site) in &edges {
            if target == t && !out.contains(site) {
                out.push(site.clone());
            }
        }
        out
    };

    // ---- param / material / rationale / part / instance / mate / assertion / dim
    for p in &design.params {
        if p.id == query {
            matches.push(Explanation {
                kind: "param".into(),
                id: p.id.clone(),
                part: None,
                definition: to_value(p),
                rationale_chain: chain(design, &p.rationale),
                referenced_by: refs_of(&Target::Param(p.id.clone())),
                related: rationale_siblings(design, &p.rationale, "param", &p.id),
            });
        }
    }
    for m in &design.materials {
        if m.id == query {
            matches.push(Explanation {
                kind: "material".into(),
                id: m.id.clone(),
                part: None,
                definition: to_value(m),
                rationale_chain: vec![],
                referenced_by: refs_of(&Target::Material(m.id.clone())),
                related: vec![],
            });
        }
    }
    for r in &design.rationales {
        if r.id == query {
            matches.push(Explanation {
                kind: "rationale".into(),
                id: r.id.clone(),
                part: None,
                definition: to_value(r),
                rationale_chain: vec![to_value(r)],
                referenced_by: refs_of(&Target::Rationale(r.id.clone())),
                related: vec![],
            });
        }
    }
    for part in &design.parts {
        if part.id == query {
            matches.push(Explanation {
                kind: "part".into(),
                id: part.id.clone(),
                part: None,
                definition: to_value(part),
                rationale_chain: vec![],
                referenced_by: refs_of(&Target::Part(part.id.clone())),
                related: vec![],
            });
        }
        // feature / anchor はPart内スコープ (§1.1)
        for f in &part.features {
            find_features(f, query, &part.id, &mut matches, &refs_of);
        }
        for a in &part.anchors {
            if a.id == query {
                matches.push(Explanation {
                    kind: "anchor".into(),
                    id: a.id.clone(),
                    part: Some(part.id.clone()),
                    definition: to_value(a),
                    rationale_chain: vec![],
                    referenced_by: refs_of(&Target::Anchor(part.id.clone(), a.id.clone())),
                    related: vec![],
                });
            }
        }
    }
    if let Some(assy) = &design.assembly {
        for inst in &assy.instances {
            if inst.id == query {
                matches.push(Explanation {
                    kind: "instance".into(),
                    id: inst.id.clone(),
                    part: None,
                    definition: to_value(inst),
                    rationale_chain: vec![],
                    referenced_by: refs_of(&Target::Instance(inst.id.clone())),
                    related: vec![],
                });
            }
        }
        for mate in &assy.mates {
            if mate.id == query {
                matches.push(Explanation {
                    kind: "mate".into(),
                    id: mate.id.clone(),
                    part: None,
                    definition: to_value(mate),
                    rationale_chain: chain(design, &mate.rationale),
                    referenced_by: vec![],
                    related: rationale_siblings(design, &mate.rationale, "mate", &mate.id),
                });
            }
        }
    }
    for a in &design.assertions {
        if a.id == query {
            matches.push(Explanation {
                kind: "assertion".into(),
                id: a.id.clone(),
                part: None,
                definition: to_value(a),
                rationale_chain: chain(design, &a.rationale),
                referenced_by: vec![],
                related: rationale_siblings(design, &a.rationale, "assertion", &a.id),
            });
        }
    }
    for d in &design.dims {
        if d.id == query {
            matches.push(Explanation {
                kind: "dim".into(),
                id: d.id.clone(),
                part: None,
                definition: to_value(d),
                rationale_chain: chain(design, &d.rationale),
                referenced_by: refs_of(&Target::Dim(d.id.clone())),
                related: rationale_siblings(design, &d.rationale, "dim", &d.id),
            });
        }
    }

    let status = match matches.len() {
        0 => ExplainStatus::NotFound,
        1 => ExplainStatus::Found,
        _ => ExplainStatus::Ambiguous,
    };
    ExplainOutput {
        schema_version: "0.1".into(),
        query: query.to_string(),
        status,
        matches,
    }
}

fn to_value<T: Serialize>(v: &T) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}

fn chain(design: &Design, rid: &str) -> Vec<Value> {
    design
        .rationales
        .iter()
        .filter(|r| r.id == rid)
        .map(to_value)
        .collect()
}

/// 同一rationaleを共有する制約(根拠の連鎖: US-04)。自分自身は除く。
fn rationale_siblings(design: &Design, rid: &str, self_kind: &str, self_id: &str) -> Vec<RefSite> {
    let via = format!("rationale:{rid}");
    let mut out = Vec::new();
    let mut push = |kind: &str, id: &str| {
        if !(kind == self_kind && id == self_id) {
            out.push(RefSite {
                kind: kind.to_string(),
                id: id.to_string(),
                part: None,
                via: via.clone(),
            });
        }
    };
    for p in &design.params {
        if p.rationale == rid {
            push("param", &p.id);
        }
    }
    for a in &design.assertions {
        if a.rationale == rid {
            push("assertion", &a.id);
        }
    }
    if let Some(assy) = &design.assembly {
        for m in &assy.mates {
            if m.rationale == rid {
                push("mate", &m.id);
            }
        }
    }
    for d in &design.dims {
        if d.rationale == rid {
            push("dim", &d.id);
        }
    }
    for (i, gt) in design.geom_tols.iter().enumerate() {
        if gt.rationale == rid {
            push("geom_tol", &format!("geom_tols[{i}]"));
        }
    }
    out
}

/// "bolts[1][0]" → ("bolts", [1, 0])
fn parse_indexed(q: &str) -> Option<(&str, Vec<u32>)> {
    let open = q.find('[')?;
    let (base, rest) = q.split_at(open);
    if base.is_empty() {
        return None;
    }
    let mut idx = Vec::new();
    let mut rest = rest;
    while !rest.is_empty() {
        let inner = rest.strip_prefix('[')?;
        let close = inner.find(']')?;
        idx.push(inner[..close].parse().ok()?);
        rest = &inner[close + 1..];
    }
    (!idx.is_empty()).then_some((base, idx))
}

fn pattern_instance_in_bounds(f: &Feature, idx: &[u32]) -> bool {
    let Feature::Pattern { kind, count, .. } = f else {
        return false;
    };
    use crate::{Count, PatternKind};
    match (kind, count, idx) {
        (PatternKind::Linear | PatternKind::Circular, Count::One(n), [i]) => i < n,
        (PatternKind::Linear2D, Count::Two(nx, ny), [i, j]) => i < nx && j < ny,
        _ => false,
    }
}

fn find_features(
    f: &Feature,
    query: &str,
    part: &str,
    matches: &mut Vec<Explanation>,
    refs_of: &dyn Fn(&Target) -> Vec<RefSite>,
) {
    if f.id() == Some(query) {
        matches.push(Explanation {
            kind: "feature".into(),
            id: query.to_string(),
            part: Some(part.to_string()),
            definition: to_value(f),
            rationale_chain: vec![],
            referenced_by: refs_of(&Target::Feature(part.to_string(), query.to_string())),
            related: vec![],
        });
    }
    // Patternの添字インスタンス "p[i]" / "p[i][j]" (§4.1)
    if let Some((base, idx)) = parse_indexed(query) {
        if f.id() == Some(base) && pattern_instance_in_bounds(f, &idx) {
            matches.push(Explanation {
                kind: "feature".into(),
                id: query.to_string(),
                part: Some(part.to_string()),
                definition: serde_json::json!({
                    "pattern": to_value(f),
                    "instance": idx,
                }),
                rationale_chain: vec![],
                referenced_by: refs_of(&Target::Feature(part.to_string(), query.to_string())),
                related: vec![],
            });
        }
    }
    if let Feature::Pattern { of, .. } = f {
        find_features(of, query, part, matches, refs_of);
    }
}

// ---------------------------------------------------------------- 逆参照索引

struct Ix {
    edges: Vec<(Target, RefSite)>,
    inst_part: HashMap<String, String>,
}

fn site(kind: &str, id: &str, part: Option<&str>, via: &str) -> RefSite {
    RefSite {
        kind: kind.to_string(),
        id: id.to_string(),
        part: part.map(str::to_string),
        via: via.to_string(),
    }
}

impl Ix {
    fn expr(&mut self, e: &Expr, s: &RefSite) {
        match e {
            Expr::Lit(_) => {}
            Expr::Param(id) => self.edges.push((Target::Param(id.clone()), s.clone())),
            Expr::Add(a, b) | Expr::Sub(a, b) | Expr::Mul(a, b) | Expr::Div(a, b) => {
                self.expr(a, s);
                self.expr(b, s);
            }
        }
    }

    fn opt_expr(&mut self, e: &Option<Expr>, s: &RefSite) {
        if let Some(e) = e {
            self.expr(e, s);
        }
    }

    fn rationale(&mut self, rid: &str, s: &RefSite) {
        self.edges.push((Target::Rationale(rid.to_string()), s.clone()));
    }

    fn binding(&mut self, b: &crate::BindingExpr, part: &str, s: &RefSite) {
        self.edges.push((
            Target::Feature(part.to_string(), b.feature.clone()),
            s.clone(),
        ));
    }

    fn edge_sel(&mut self, sel: &EdgeSelector, part: &str, s: &RefSite) {
        match sel {
            EdgeSelector::EdgesOf(b) => self.binding(b, part, s),
            EdgeSelector::EdgesBetween(a, b) => {
                self.binding(a, part, s);
                self.binding(b, part, s);
            }
        }
    }

    fn placement(&mut self, p: &Placement, part: &str, s: &RefSite) {
        match p {
            Placement::Origin => {}
            Placement::On { face, at } => {
                self.binding(face, part, s);
                match at {
                    Pos2::Center => {}
                    Pos2::Xy(x, y) => {
                        self.expr(x, s);
                        self.expr(y, s);
                    }
                    Pos2::FromEdge { edge, d, along } => {
                        self.edge_sel(edge, part, s);
                        self.expr(d, s);
                        self.expr(along, s);
                    }
                }
            }
            Placement::Offset { from, d } => {
                self.placement(from, part, s);
                self.expr(&d.0, s);
                self.expr(&d.1, s);
                self.expr(&d.2, s);
            }
        }
    }

    fn anchor_path(&mut self, path: &crate::AnchorPath, s: &RefSite) {
        self.edges
            .push((Target::Instance(path.instance.clone()), s.clone()));
        if let Some(part) = self.inst_part.get(&path.instance).cloned() {
            self.edges
                .push((Target::Anchor(part, path.anchor.clone()), s.clone()));
        }
    }

    fn geom_ref(&mut self, g: &GeomRef, s: &RefSite) {
        match g {
            GeomRef::Part(p) => self.edges.push((Target::Part(p.clone()), s.clone())),
            GeomRef::Anchor(a) => self.anchor_path(a, s),
        }
    }

    fn feature(&mut self, part: &str, f: &Feature, parent_id: Option<&str>) {
        let fid = f.id().or(parent_id).unwrap_or("(無名)").to_string();
        let s = |via: &str| site("feature", &fid, Some(part), via);
        match f {
            Feature::Block { x, y, z, at, .. } => {
                self.expr(x, &s("x"));
                self.expr(y, &s("y"));
                self.expr(z, &s("z"));
                self.opt_placement(at, part, &s("at"));
            }
            Feature::Cylinder { d, h, at, .. } => {
                self.expr(d, &s("d"));
                self.expr(h, &s("h"));
                self.opt_placement(at, part, &s("at"));
            }
            Feature::Hole {
                d,
                depth,
                cb_d,
                cb_depth,
                cs_d,
                cs_angle,
                at,
                ..
            } => {
                self.expr(d, &s("d"));
                if let crate::HoleDepth::Blind(e) = depth {
                    self.expr(e, &s("depth"));
                }
                self.opt_expr(cb_d, &s("cb_d"));
                self.opt_expr(cb_depth, &s("cb_depth"));
                self.opt_expr(cs_d, &s("cs_d"));
                self.opt_expr(cs_angle, &s("cs_angle"));
                self.opt_placement(at, part, &s("at"));
            }
            Feature::Pocket {
                profile,
                depth,
                corner_r,
                at,
                ..
            } => {
                self.profile(profile, &s("profile"));
                self.expr(depth, &s("depth"));
                self.opt_expr(corner_r, &s("corner_r"));
                self.opt_placement(at, part, &s("at"));
            }
            Feature::Boss {
                profile, height, at, ..
            } => {
                self.profile(profile, &s("profile"));
                self.expr(height, &s("height"));
                self.opt_placement(at, part, &s("at"));
            }
            Feature::Fillet { edges, r, .. } => {
                self.edge_sel(edges, part, &s("edges"));
                self.expr(r, &s("r"));
            }
            Feature::Chamfer { edges, size, .. } => {
                self.edge_sel(edges, part, &s("edges"));
                self.expr(size, &s("size"));
            }
            Feature::Pattern {
                of, pitch, axis, at, ..
            } => {
                self.feature(part, of, Some(&fid));
                match pitch {
                    Pitch::One(e) => self.expr(e, &s("pitch")),
                    Pitch::Two(a, b) => {
                        self.expr(a, &s("pitch"));
                        self.expr(b, &s("pitch"));
                    }
                }
                if let Some(axis) = axis {
                    self.binding(axis, part, &s("axis"));
                }
                self.opt_placement(at, part, &s("at"));
            }
            Feature::BaseFlange {
                profile, thickness, ..
            } => {
                self.profile(profile, &s("profile"));
                self.expr(thickness, &s("thickness"));
            }
            Feature::Flange {
                edge,
                angle,
                length,
                bend_r,
                ..
            } => {
                self.edge_sel(edge, part, &s("edge"));
                self.expr(angle, &s("angle"));
                self.expr(length, &s("length"));
                self.expr(bend_r, &s("bend_r"));
            }
            Feature::Cutout { profile, at, .. } => {
                self.profile(profile, &s("profile"));
                self.opt_placement(at, part, &s("at"));
            }
            Feature::Relief { at, .. } => {
                self.opt_placement(at, part, &s("at"));
            }
        }
    }

    fn profile(&mut self, p: &Profile, s: &RefSite) {
        match p {
            Profile::Rect { x, y } => {
                self.expr(x, s);
                self.expr(y, s);
            }
            Profile::Circ { d } => self.expr(d, s),
        }
    }

    fn opt_placement(&mut self, at: &Option<Placement>, part: &str, s: &RefSite) {
        if let Some(p) = at {
            self.placement(p, part, s);
        }
    }
}

use crate::Profile;

fn reverse_index(design: &Design) -> Vec<(Target, RefSite)> {
    let mut ix = Ix {
        edges: Vec::new(),
        inst_part: design
            .assembly
            .iter()
            .flat_map(|a| a.instances.iter())
            .map(|i| (i.id.clone(), i.part.clone()))
            .collect(),
    };

    for p in &design.params {
        let s = site("param", &p.id, None, "value");
        if let ParamValue::Determined(e) = &p.value {
            ix.expr(e, &s);
        }
        ix.rationale(&p.rationale, &site("param", &p.id, None, "rationale"));
    }

    for part in &design.parts {
        ix.edges.push((
            Target::Material(part.material.clone()),
            site("part", &part.id, None, "material"),
        ));
        if let Process::SheetMetal { thickness, .. } = &part.process {
            ix.expr(thickness, &site("part", &part.id, None, "process.thickness"));
        }
        for f in &part.features {
            ix.feature(&part.id, f, None);
        }
        for a in &part.anchors {
            ix.binding(
                &a.binding,
                &part.id,
                &site("anchor", &a.id, Some(&part.id), "binding"),
            );
        }
    }

    if let Some(assy) = &design.assembly {
        for inst in &assy.instances {
            ix.edges.push((
                Target::Part(inst.part.clone()),
                site("instance", &inst.id, None, "part"),
            ));
        }
        ix.edges.push((
            Target::Instance(assy.ground.clone()),
            site("assembly", &assy.id, None, "ground"),
        ));
        for mate in &assy.mates {
            ix.anchor_path(&mate.a, &site("mate", &mate.id, None, "a"));
            ix.anchor_path(&mate.b, &site("mate", &mate.id, None, "b"));
            if let MateKind::Distance(e) | MateKind::Angle(e) = &mate.kind {
                ix.expr(e, &site("mate", &mate.id, None, "kind"));
            }
            ix.rationale(&mate.rationale, &site("mate", &mate.id, None, "rationale"));
        }
    }

    for a in &design.assertions {
        let s = site("assertion", &a.id, None, "check");
        match &a.check {
            Check::Clearance { a: ga, b: gb, min } => {
                ix.geom_ref(ga, &s);
                ix.geom_ref(gb, &s);
                ix.expr(min, &s);
            }
            Check::NoInterference { scope } => {
                if let Scope::Pairs(pairs) = scope {
                    for (pa, pb) in pairs {
                        ix.edges.push((Target::Part(pa.clone()), s.clone()));
                        ix.edges.push((Target::Part(pb.clone()), s.clone()));
                    }
                }
            }
            Check::Mass { part, max, min } => {
                ix.edges.push((Target::Part(part.clone()), s.clone()));
                ix.expr(max, &s);
                ix.opt_expr(min, &s);
            }
            Check::Cog { within } => {
                for e in [
                    &within.min.0, &within.min.1, &within.min.2,
                    &within.max.0, &within.max.1, &within.max.2,
                ] {
                    ix.expr(e, &s);
                }
            }
            Check::WallThickness { part, min, .. } => {
                ix.edges.push((Target::Part(part.clone()), s.clone()));
                ix.expr(min, &s);
            }
            Check::BoundingBox { part, max } => {
                ix.edges.push((Target::Part(part.clone()), s.clone()));
                ix.expr(&max.0, &s);
                ix.expr(&max.1, &s);
                ix.expr(&max.2, &s);
            }
            Check::DatumValidity { part } | Check::SheetMetalRules { part } => {
                ix.edges.push((Target::Part(part.clone()), s.clone()));
            }
            Check::ToleranceStack1D { path, .. } => {
                for dim in path {
                    ix.edges.push((
                        Target::Dim(dim.clone()),
                        site("assertion", &a.id, None, "check.path"),
                    ));
                }
            }
            Check::ToolAccess { part, tool_d, .. } => {
                ix.edges.push((Target::Part(part.clone()), s.clone()));
                ix.expr(tool_d, &s);
            }
            Check::MinCornerRadius { part, min } => {
                ix.edges.push((Target::Part(part.clone()), s.clone()));
                ix.expr(min, &s);
            }
        }
        ix.rationale(&a.rationale, &site("assertion", &a.id, None, "rationale"));
    }

    for d in &design.dims {
        ix.anchor_path(&d.from, &site("dim", &d.id, None, "from"));
        ix.anchor_path(&d.to, &site("dim", &d.id, None, "to"));
        ix.expr(&d.nominal, &site("dim", &d.id, None, "nominal"));
        ix.rationale(&d.rationale, &site("dim", &d.id, None, "rationale"));
    }
    for (i, gt) in design.geom_tols.iter().enumerate() {
        let gid = format!("geom_tols[{i}]");
        ix.anchor_path(&gt.target, &site("geom_tol", &gid, None, "target"));
        for datum in &gt.datums {
            ix.anchor_path(datum, &site("geom_tol", &gid, None, "datums"));
        }
        ix.expr(&gt.zone, &site("geom_tol", &gid, None, "zone"));
        ix.rationale(&gt.rationale, &site("geom_tol", &gid, None, "rationale"));
    }

    ix.edges
}
