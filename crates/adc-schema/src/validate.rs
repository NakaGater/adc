//! M0-2 静的検証 (US-02, US-04)。
//!
//! パイプライン: parse(desugar+deserialize) → シンボルテーブル構築 → 検証。
//! エラーは全て構造化形式 {code, message, span, related} (05-schema.md §8)。
//!
//! 検証項目:
//! - 未定義参照: param / feature(Part内) / anchor(instance.anchor) / material /
//!   part / instance / dim — E-SCHEMA-REF
//! - rationale参照の未定義 — E-SCHEMA-RATIONALE
//! - 種別内重複ID(§1.1: feature/anchorは所属Part内、他はDesign内) — E-SCHEMA-DUP
//! - param間の循環参照 — E-SCHEMA-CYCLE
//! - Open範囲の妥当性 (min <= nominal <= max) — E-SCHEMA-RANGE
//! - mateグラフ(a=基準側→b=被拘束側)のDAG性・自己参照 — E-MATE-CYCLE
//! - GeomTol.datums は kind: Datum のアンカーのみ(§7) — E-SCHEMA-REF
//!
//! spanは元テキスト上の該当ID出現位置をヒューリスティックに解決する
//! (参照エラー: 引用付きIDの初出 / 重複エラー: `id: "x"` 定義のn番目)。

use std::collections::{BTreeSet, HashMap, HashSet};

use crate::error::{ErrorCode, Span, ValidationError};
use crate::{
    parse_design, AnchorKind, AnchorPath, BindingExpr, Check, Design, EdgeSelector, Expr, Feature,
    GeomRef, MateKind, ParamValue, Pitch, Placement, Pos2, Process, Profile, SchemaError, Scope,
};

/// design.ron をパースし静的検証する。
/// エラーが1件でもあれば全件を返す(最初のエラーで停止しない)。
pub fn validate_design(src: &str) -> Result<Design, Vec<ValidationError>> {
    let design = match parse_design(src) {
        Ok(d) => d,
        Err(SchemaError::Parse {
            message,
            line,
            column,
        }) => {
            return Err(vec![ValidationError {
                code: ErrorCode::SchemaParse,
                message,
                span: Some(Span { line, column }),
                related: vec![],
            }])
        }
        Err(other) => {
            return Err(vec![ValidationError {
                code: ErrorCode::SchemaParse,
                message: other.to_string(),
                span: None,
                related: vec![],
            }])
        }
    };
    let errors = validate(&design, src);
    if errors.is_empty() {
        Ok(design)
    } else {
        Err(errors)
    }
}

// ---------------------------------------------------------------- Locator

struct Locator<'a> {
    src: &'a str,
}

impl Locator<'_> {
    fn span_at(&self, byte: usize) -> Span {
        let upto = &self.src[..byte];
        let line = upto.bytes().filter(|&b| b == b'\n').count() + 1;
        let column = upto.chars().rev().take_while(|&c| c != '\n').count() + 1;
        Span { line, column }
    }

    /// 引用付き `"id"` の nth 番目(0始まり)の出現位置
    fn quoted(&self, id: &str, nth: usize) -> Option<Span> {
        let needle = format!("\"{id}\"");
        let mut from = 0usize;
        let mut count = 0usize;
        while let Some(pos) = self.src[from..].find(&needle) {
            let abs = from + pos;
            if count == nth {
                return Some(self.span_at(abs));
            }
            count += 1;
            from = abs + needle.len();
        }
        None
    }

    /// 参照箇所: 引用付きIDの初出
    fn reference(&self, id: &str) -> Option<Span> {
        self.quoted(id, 0)
    }

    /// 定義箇所: `id: "x"` パターンの nth 番目(0始まり)
    fn definition(&self, id: &str, nth: usize) -> Option<Span> {
        let needle = format!("\"{id}\"");
        let mut from = 0usize;
        let mut count = 0usize;
        while let Some(pos) = self.src[from..].find(&needle) {
            let abs = from + pos;
            if self.src[..abs].trim_end().ends_with("id:") {
                if count == nth {
                    return Some(self.span_at(abs));
                }
                count += 1;
            }
            from = abs + needle.len();
        }
        None
    }
}

// ---------------------------------------------------------------- 検証本体

struct Ctx<'a> {
    errs: Vec<ValidationError>,
    loc: Locator<'a>,
    params: HashSet<&'a str>,
    materials: HashSet<&'a str>,
    rationales: HashSet<&'a str>,
    parts: HashSet<&'a str>,
    dims: HashSet<&'a str>,
    /// instance id → part id(partの存在は別途検証)
    instances: HashMap<&'a str, &'a str>,
    /// part id → そのPartのフィーチャーID集合(Pattern.of内も含む)
    part_features: HashMap<&'a str, HashSet<&'a str>>,
    /// part id → アンカーID → kind
    part_anchors: HashMap<&'a str, HashMap<&'a str, AnchorKind>>,
}

impl Ctx<'_> {
    fn push(
        &mut self,
        code: ErrorCode,
        message: String,
        span: Option<Span>,
        related: Vec<String>,
    ) {
        self.errs.push(ValidationError {
            code,
            message,
            span,
            related,
        });
    }

    fn check_rationale(&mut self, id: &str, owner: &str) {
        if !self.rationales.contains(id) {
            let span = self.loc.reference(id);
            self.push(
                ErrorCode::SchemaRationale,
                format!("\"{owner}\" が参照する rationale \"{id}\" が未定義です"),
                span,
                vec![id.to_string(), owner.to_string()],
            );
        }
    }

    fn check_part_ref(&mut self, id: &str, where_: &str) {
        if !self.parts.contains(id) {
            let span = self.loc.reference(id);
            self.push(
                ErrorCode::SchemaRef,
                format!("{where_} が参照する part \"{id}\" が未定義です"),
                span,
                vec![id.to_string()],
            );
        }
    }

    fn check_expr(&mut self, e: &Expr, where_: &str) {
        match e {
            Expr::Lit(_) => {}
            Expr::Param(id) => {
                if !self.params.contains(id.as_str()) {
                    let span = self.loc.reference(id);
                    self.push(
                        ErrorCode::SchemaRef,
                        format!("{where_} が参照するパラメータ \"{id}\" が未定義です"),
                        span,
                        vec![id.to_string()],
                    );
                }
            }
            Expr::Add(a, b) | Expr::Sub(a, b) | Expr::Mul(a, b) | Expr::Div(a, b) => {
                self.check_expr(a, where_);
                self.check_expr(b, where_);
            }
        }
    }

    fn check_opt_expr(&mut self, e: &Option<Expr>, where_: &str) {
        if let Some(e) = e {
            self.check_expr(e, where_);
        }
    }

    fn check_binding(&mut self, b: &BindingExpr, part: &str) {
        let known = self
            .part_features
            .get(part)
            .is_some_and(|s| s.contains(b.feature.as_str()));
        if !known {
            let span = self.loc.reference(&b.feature);
            self.push(
                ErrorCode::SchemaRef,
                format!(
                    "part \"{part}\" 内に未定義のフィーチャー \"{}\" への参照があります",
                    b.feature
                ),
                span,
                vec![b.feature.to_string(), part.to_string()],
            );
        }
    }

    fn check_edges(&mut self, e: &EdgeSelector, part: &str) {
        match e {
            EdgeSelector::EdgesOf(b) => self.check_binding(b, part),
            EdgeSelector::EdgesBetween(a, b) => {
                self.check_binding(a, part);
                self.check_binding(b, part);
            }
        }
    }

    fn check_placement(&mut self, p: &Placement, part: &str, where_: &str) {
        match p {
            Placement::Origin => {}
            Placement::On { face, at } => {
                self.check_binding(face, part);
                match at {
                    Pos2::Center => {}
                    Pos2::Xy(x, y) => {
                        self.check_expr(x, where_);
                        self.check_expr(y, where_);
                    }
                    Pos2::FromEdge { edge, d, along } => {
                        self.check_edges(edge, part);
                        self.check_expr(d, where_);
                        self.check_expr(along, where_);
                    }
                }
            }
            Placement::Offset { from, d } => {
                self.check_placement(from, part, where_);
                self.check_expr(&d.0, where_);
                self.check_expr(&d.1, where_);
                self.check_expr(&d.2, where_);
            }
        }
    }

    fn check_profile(&mut self, p: &Profile, where_: &str) {
        match p {
            Profile::Rect { x, y } => {
                self.check_expr(x, where_);
                self.check_expr(y, where_);
            }
            Profile::Circ { d } => self.check_expr(d, where_),
        }
    }

    fn check_feature(&mut self, f: &Feature, part: &str) {
        let where_ = format!(
            "part \"{part}\" のフィーチャー \"{}\"",
            feature_id(f).unwrap_or("(無名)")
        );
        let w = where_.as_str();
        match f {
            Feature::Block { x, y, z, at, .. } => {
                self.check_expr(x, w);
                self.check_expr(y, w);
                self.check_expr(z, w);
                self.check_opt_placement(at, part, w);
            }
            Feature::Cylinder { d, h, at, .. } => {
                self.check_expr(d, w);
                self.check_expr(h, w);
                self.check_opt_placement(at, part, w);
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
                self.check_expr(d, w);
                if let crate::HoleDepth::Blind(e) = depth {
                    self.check_expr(e, w);
                }
                self.check_opt_expr(cb_d, w);
                self.check_opt_expr(cb_depth, w);
                self.check_opt_expr(cs_d, w);
                self.check_opt_expr(cs_angle, w);
                self.check_opt_placement(at, part, w);
            }
            Feature::Pocket {
                profile,
                depth,
                corner_r,
                at,
                ..
            } => {
                self.check_profile(profile, w);
                self.check_expr(depth, w);
                self.check_opt_expr(corner_r, w);
                self.check_opt_placement(at, part, w);
            }
            Feature::Boss {
                profile,
                height,
                at,
                ..
            } => {
                self.check_profile(profile, w);
                self.check_expr(height, w);
                self.check_opt_placement(at, part, w);
            }
            Feature::Fillet { edges, r, .. } => {
                self.check_edges(edges, part);
                self.check_expr(r, w);
            }
            Feature::Chamfer { edges, size, .. } => {
                self.check_edges(edges, part);
                self.check_expr(size, w);
            }
            Feature::Pattern {
                of, pitch, axis, at, ..
            } => {
                self.check_feature(of, part);
                match pitch {
                    Pitch::One(e) => self.check_expr(e, w),
                    Pitch::Two(a, b) => {
                        self.check_expr(a, w);
                        self.check_expr(b, w);
                    }
                }
                if let Some(axis) = axis {
                    self.check_binding(axis, part);
                }
                self.check_opt_placement(at, part, w);
            }
            Feature::BaseFlange {
                profile, thickness, ..
            } => {
                self.check_profile(profile, w);
                self.check_expr(thickness, w);
            }
            Feature::Flange {
                edge,
                angle,
                length,
                bend_r,
                ..
            } => {
                self.check_edges(edge, part);
                self.check_expr(angle, w);
                self.check_expr(length, w);
                self.check_expr(bend_r, w);
            }
            Feature::Cutout { profile, at, .. } => {
                self.check_profile(profile, w);
                self.check_opt_placement(at, part, w);
            }
            Feature::Relief { at, .. } => {
                self.check_opt_placement(at, part, w);
            }
        }
    }

    fn check_opt_placement(&mut self, at: &Option<Placement>, part: &str, where_: &str) {
        if let Some(p) = at {
            self.check_placement(p, part, where_);
        }
    }

    /// instance.anchor を解決する。解決できた場合はアンカーのkindを返す。
    /// instanceのpart自体が未定義の場合は(instance側で報告済みのため)黙ってNone。
    fn check_anchor_path(&mut self, path: &AnchorPath, where_: &str) -> Option<AnchorKind> {
        let Some(&part) = self.instances.get(path.instance.as_str()) else {
            let key = path.to_string();
            let span = self.loc.reference(&key);
            self.push(
                ErrorCode::SchemaRef,
                format!("{where_} が参照するインスタンス \"{}\" が未定義です", path.instance),
                span,
                vec![path.instance.to_string()],
            );
            return None;
        };
        let Some(anchors) = self.part_anchors.get(part) else {
            return None; // partが未定義(instance側で報告済み)
        };
        match anchors.get(path.anchor.as_str()) {
            Some(kind) => Some(*kind),
            None => {
                let key = path.to_string();
                let span = self.loc.reference(&key);
                self.push(
                    ErrorCode::SchemaRef,
                    format!(
                        "{where_} が参照するアンカー \"{}\" は part \"{part}\" に存在しません",
                        path.anchor
                    ),
                    span,
                    vec![path.to_string(), part.to_string()],
                );
                None
            }
        }
    }

    fn check_geom_ref(&mut self, g: &GeomRef, where_: &str) {
        match g {
            GeomRef::Part(p) => self.check_part_ref(p, where_),
            GeomRef::Anchor(a) => {
                self.check_anchor_path(a, where_);
            }
        }
    }
}

fn feature_id(f: &Feature) -> Option<&str> {
    match f {
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

fn collect_feature_ids<'a>(f: &'a Feature, out: &mut Vec<&'a str>) {
    if let Some(id) = feature_id(f) {
        out.push(id);
    }
    if let Feature::Pattern { of, .. } = f {
        collect_feature_ids(of, out);
    }
}

fn expr_param_refs<'a>(e: &'a Expr, out: &mut Vec<&'a str>) {
    match e {
        Expr::Lit(_) => {}
        Expr::Param(id) => out.push(id),
        Expr::Add(a, b) | Expr::Sub(a, b) | Expr::Mul(a, b) | Expr::Div(a, b) => {
            expr_param_refs(a, out);
            expr_param_refs(b, out);
        }
    }
}

/// 種別内のID一意性を検証しつつ集合を構築する
fn collect_unique<'a>(
    errs: &mut Vec<ValidationError>,
    loc: &Locator<'_>,
    kind_label: &str,
    ids: impl Iterator<Item = &'a str>,
) -> HashSet<&'a str> {
    let mut seen: HashSet<&'a str> = HashSet::new();
    let mut dup_n: HashMap<&'a str, usize> = HashMap::new();
    for id in ids {
        if !seen.insert(id) {
            let n = dup_n.entry(id).or_insert(0);
            *n += 1;
            errs.push(ValidationError {
                code: ErrorCode::SchemaDup,
                message: format!("{kind_label} ID \"{id}\" が重複しています(種別内で一意であること: 05-schema.md §1.1)"),
                span: loc.definition(id, *n),
                related: vec![id.to_string()],
            });
        }
    }
    seen
}

fn validate(design: &Design, src: &str) -> Vec<ValidationError> {
    let loc = Locator { src };
    let mut errs: Vec<ValidationError> = Vec::new();

    // ---- シンボルテーブル構築+種別内重複検出 (§1.1)
    let params = collect_unique(&mut errs, &loc, "param", design.params.iter().map(|p| p.id.as_str()));
    let materials = collect_unique(&mut errs, &loc, "material", design.materials.iter().map(|m| m.id.as_str()));
    let rationales = collect_unique(&mut errs, &loc, "rationale", design.rationales.iter().map(|r| r.id.as_str()));
    let parts = collect_unique(&mut errs, &loc, "part", design.parts.iter().map(|p| p.id.as_str()));
    collect_unique(&mut errs, &loc, "assertion", design.assertions.iter().map(|a| a.id.as_str()));
    let dims = collect_unique(&mut errs, &loc, "dim", design.dims.iter().map(|d| d.id.as_str()));

    let mut part_features: HashMap<&str, HashSet<&str>> = HashMap::new();
    let mut part_anchors: HashMap<&str, HashMap<&str, AnchorKind>> = HashMap::new();
    for part in &design.parts {
        let mut fids: Vec<&str> = Vec::new();
        for f in &part.features {
            collect_feature_ids(f, &mut fids);
        }
        let fset = collect_unique(
            &mut errs,
            &loc,
            &format!("part \"{}\" のフィーチャー", part.id),
            fids.into_iter(),
        );
        part_features.insert(part.id.as_str(), fset);

        collect_unique(
            &mut errs,
            &loc,
            &format!("part \"{}\" のアンカー", part.id),
            part.anchors.iter().map(|a| a.id.as_str()),
        );
        let amap: HashMap<&str, AnchorKind> = part
            .anchors
            .iter()
            .map(|a| (a.id.as_str(), a.kind))
            .collect();
        part_anchors.insert(part.id.as_str(), amap);
    }

    let mut instances: HashMap<&str, &str> = HashMap::new();
    if let Some(assy) = &design.assembly {
        collect_unique(&mut errs, &loc, "instance", assy.instances.iter().map(|i| i.id.as_str()));
        collect_unique(&mut errs, &loc, "mate", assy.mates.iter().map(|m| m.id.as_str()));
        for inst in &assy.instances {
            instances.insert(inst.id.as_str(), inst.part.as_str());
        }
    }

    let mut ctx = Ctx {
        errs,
        loc,
        params,
        materials,
        rationales,
        parts,
        dims,
        instances,
        part_features,
        part_anchors,
    };

    // ---- params: rationale / Determined式 / Open範囲 (E-SCHEMA-RANGE)
    for p in &design.params {
        ctx.check_rationale(&p.rationale, &format!("param \"{}\"", p.id));
        match &p.value {
            ParamValue::Determined(e) => {
                ctx.check_expr(e, &format!("param \"{}\" の値", p.id));
            }
            ParamValue::Open {
                range: (min, max),
                nominal,
            } => {
                if min > max {
                    let span = ctx.loc.reference(&p.id);
                    ctx.push(
                        ErrorCode::SchemaRange,
                        format!(
                            "param \"{}\" のrangeが不正です: 最小値 {min} > 最大値 {max}",
                            p.id
                        ),
                        span,
                        vec![p.id.to_string()],
                    );
                } else if !(*min <= *nominal && *nominal <= *max) {
                    let span = ctx.loc.reference(&p.id);
                    ctx.push(
                        ErrorCode::SchemaRange,
                        format!(
                            "param \"{}\" のnominal {nominal} がrange ({min}, {max}) の外です",
                            p.id
                        ),
                        span,
                        vec![p.id.to_string()],
                    );
                }
            }
        }
    }

    // ---- param間の循環参照 (E-SCHEMA-CYCLE)
    check_param_cycles(design, &mut ctx);

    // ---- parts: material / process / features / anchors
    for part in &design.parts {
        if !ctx.materials.contains(part.material.as_str()) {
            let span = ctx.loc.reference(&part.material);
            ctx.push(
                ErrorCode::SchemaRef,
                format!(
                    "part \"{}\" が参照する material \"{}\" が未定義です",
                    part.id, part.material
                ),
                span,
                vec![part.material.to_string(), part.id.to_string()],
            );
        }
        if let Process::SheetMetal { thickness, .. } = &part.process {
            ctx.check_expr(thickness, &format!("part \"{}\" の板厚", part.id));
        }
        for f in &part.features {
            ctx.check_feature(f, &part.id);
        }
        for a in &part.anchors {
            ctx.check_binding(&a.binding, &part.id);
        }
    }

    // ---- assembly: instance.part / ground / mates(参照+DAG)
    if let Some(assy) = &design.assembly {
        for inst in &assy.instances {
            if !ctx.parts.contains(inst.part.as_str()) {
                let span = ctx.loc.reference(&inst.part);
                ctx.push(
                    ErrorCode::SchemaRef,
                    format!(
                        "instance \"{}\" が参照する part \"{}\" が未定義です",
                        inst.id, inst.part
                    ),
                    span,
                    vec![inst.part.to_string(), inst.id.to_string()],
                );
            }
        }
        if !ctx.instances.contains_key(assy.ground.as_str()) {
            let span = ctx.loc.reference(&assy.ground);
            ctx.push(
                ErrorCode::SchemaRef,
                format!("ground \"{}\" が未定義のインスタンスです", assy.ground),
                span,
                vec![assy.ground.to_string()],
            );
        }
        for mate in &assy.mates {
            let where_ = format!("mate \"{}\"", mate.id);
            ctx.check_rationale(&mate.rationale, &where_);
            ctx.check_anchor_path(&mate.a, &where_);
            ctx.check_anchor_path(&mate.b, &where_);
            match &mate.kind {
                MateKind::Distance(e) | MateKind::Angle(e) => ctx.check_expr(e, &where_),
                MateKind::Coaxial | MateKind::Coincident => {}
            }
        }
        check_mate_dag(assy, &mut ctx);
    }

    // ---- dims / geom_tols
    for dim in &design.dims {
        let where_ = format!("dim \"{}\"", dim.id);
        ctx.check_rationale(&dim.rationale, &where_);
        ctx.check_anchor_path(&dim.from, &where_);
        ctx.check_anchor_path(&dim.to, &where_);
        ctx.check_expr(&dim.nominal, &where_);
    }
    for (idx, gt) in design.geom_tols.iter().enumerate() {
        let where_ = format!("geom_tols[{idx}]");
        ctx.check_rationale(&gt.rationale, &where_);
        ctx.check_anchor_path(&gt.target, &where_);
        ctx.check_expr(&gt.zone, &where_);
        for datum in &gt.datums {
            if let Some(kind) = ctx.check_anchor_path(datum, &where_) {
                if !matches!(kind, AnchorKind::Datum(_)) {
                    let key = datum.to_string();
                    let span = ctx.loc.reference(&key);
                    ctx.push(
                        ErrorCode::SchemaRef,
                        format!(
                            "{where_} のdatum参照 \"{datum}\" は kind: Datum のアンカーではありません (05-schema.md §7)"
                        ),
                        span,
                        vec![datum.to_string()],
                    );
                }
            }
        }
    }

    // ---- assertions
    for a in &design.assertions {
        let where_ = format!("assertion \"{}\"", a.id);
        let w = where_.as_str();
        ctx.check_rationale(&a.rationale, w);
        match &a.check {
            Check::Clearance { a: ga, b: gb, min } => {
                ctx.check_geom_ref(ga, w);
                ctx.check_geom_ref(gb, w);
                ctx.check_expr(min, w);
            }
            Check::NoInterference { scope } => {
                if let Scope::Pairs(pairs) = scope {
                    for (pa, pb) in pairs {
                        ctx.check_part_ref(pa, w);
                        ctx.check_part_ref(pb, w);
                    }
                }
            }
            Check::Mass { part, max, min } => {
                ctx.check_part_ref(part, w);
                ctx.check_expr(max, w);
                ctx.check_opt_expr(min, w);
            }
            Check::Cog { within } => {
                for e in [
                    &within.min.0, &within.min.1, &within.min.2,
                    &within.max.0, &within.max.1, &within.max.2,
                ] {
                    ctx.check_expr(e, w);
                }
            }
            Check::WallThickness { part, min, .. } => {
                ctx.check_part_ref(part, w);
                ctx.check_expr(min, w);
            }
            Check::BoundingBox { part, max } => {
                ctx.check_part_ref(part, w);
                ctx.check_expr(&max.0, w);
                ctx.check_expr(&max.1, w);
                ctx.check_expr(&max.2, w);
            }
            Check::DatumValidity { part }
            | Check::SheetMetalRules { part } => ctx.check_part_ref(part, w),
            Check::ToleranceStack1D { path, .. } => {
                for dim_id in path {
                    if !ctx.dims.contains(dim_id.as_str()) {
                        let span = ctx.loc.reference(dim_id);
                        ctx.push(
                            ErrorCode::SchemaRef,
                            format!("{w} の公差スタック経路が参照する dim \"{dim_id}\" が未定義です"),
                            span,
                            vec![dim_id.to_string(), a.id.to_string()],
                        );
                    }
                }
            }
            Check::ToolAccess { part, tool_d, .. } => {
                ctx.check_part_ref(part, w);
                ctx.check_expr(tool_d, w);
            }
            Check::MinCornerRadius { part, min } => {
                ctx.check_part_ref(part, w);
                ctx.check_expr(min, w);
            }
        }
    }

    ctx.errs
}

/// param依存グラフ(Determined式のparam参照)の循環検出。DFS 3色。
fn check_param_cycles(design: &Design, ctx: &mut Ctx<'_>) {
    let mut deps: HashMap<&str, Vec<&str>> = HashMap::new();
    for p in &design.params {
        if let ParamValue::Determined(e) = &p.value {
            let mut refs = Vec::new();
            expr_param_refs(e, &mut refs);
            // 存在するparamへの辺のみ(未定義参照はE-SCHEMA-REFで別途報告)
            refs.retain(|r| ctx.params.contains(r));
            deps.insert(p.id.as_str(), refs);
        }
    }

    #[derive(Clone, Copy, PartialEq)]
    enum Color {
        White,
        Gray,
        Black,
    }
    let mut color: HashMap<&str, Color> = deps.keys().map(|&k| (k, Color::White)).collect();
    let mut reported: HashSet<BTreeSet<String>> = HashSet::new();

    fn dfs<'a>(
        node: &'a str,
        deps: &HashMap<&'a str, Vec<&'a str>>,
        color: &mut HashMap<&'a str, Color>,
        stack: &mut Vec<&'a str>,
        cycles: &mut Vec<Vec<String>>,
    ) {
        color.insert(node, Color::Gray);
        stack.push(node);
        for &next in deps.get(node).map(Vec::as_slice).unwrap_or(&[]) {
            match color.get(next).copied().unwrap_or(Color::Black) {
                Color::White => dfs(next, deps, color, stack, cycles),
                Color::Gray => {
                    let start = stack.iter().position(|&n| n == next).unwrap_or(0);
                    cycles.push(stack[start..].iter().map(|s| s.to_string()).collect());
                }
                Color::Black => {}
            }
        }
        stack.pop();
        color.insert(node, Color::Black);
    }

    let mut cycles: Vec<Vec<String>> = Vec::new();
    let nodes: Vec<&str> = deps.keys().copied().collect();
    for node in nodes {
        if color.get(node) == Some(&Color::White) {
            let mut stack = Vec::new();
            dfs(node, &deps, &mut color, &mut stack, &mut cycles);
        }
    }
    for cycle in cycles {
        let key: BTreeSet<String> = cycle.iter().cloned().collect();
        if !reported.insert(key) {
            continue;
        }
        let span = ctx.loc.reference(&cycle[0]);
        let chain = {
            let mut c = cycle.clone();
            c.push(cycle[0].clone());
            c.join(" → ")
        };
        ctx.push(
            ErrorCode::SchemaCycle,
            format!("パラメータの循環参照: {chain}"),
            span,
            cycle,
        );
    }
}

/// mateグラフ(a=基準側 → b=被拘束側)のDAG検証。自己参照・循環は E-MATE-CYCLE。
fn check_mate_dag(assy: &crate::Assembly, ctx: &mut Ctx<'_>) {
    // 有向辺: a.instance → b.instance(両端が解決可能なmateのみ)
    let mut edges: HashMap<&str, Vec<(&str, &str)>> = HashMap::new(); // from → [(to, mate_id)]
    for mate in &assy.mates {
        let (ia, ib) = (mate.a.instance.as_str(), mate.b.instance.as_str());
        if !ctx.instances.contains_key(ia) || !ctx.instances.contains_key(ib) {
            continue; // 参照エラーとして報告済み
        }
        if ia == ib {
            let span = ctx.loc.definition(&mate.id, 0);
            ctx.push(
                ErrorCode::MateCycle,
                format!(
                    "mate \"{}\" が同一インスタンス \"{ia}\" 内で自己参照しています",
                    mate.id
                ),
                span,
                vec![mate.id.to_string(), ia.to_string()],
            );
            continue;
        }
        edges.entry(ia).or_default().push((ib, mate.id.as_str()));
    }

    #[derive(Clone, Copy, PartialEq)]
    enum Color {
        White,
        Gray,
        Black,
    }
    let mut color: HashMap<&str, Color> = ctx
        .instances
        .keys()
        .map(|&k| (k, Color::White))
        .collect();
    let mut cycles: Vec<Vec<String>> = Vec::new();

    fn dfs<'a>(
        node: &'a str,
        edges: &HashMap<&'a str, Vec<(&'a str, &'a str)>>,
        color: &mut HashMap<&'a str, Color>,
        stack: &mut Vec<&'a str>,
        cycles: &mut Vec<Vec<String>>,
    ) {
        color.insert(node, Color::Gray);
        stack.push(node);
        for &(next, _mate) in edges.get(node).map(Vec::as_slice).unwrap_or(&[]) {
            match color.get(next).copied().unwrap_or(Color::Black) {
                Color::White => dfs(next, edges, color, stack, cycles),
                Color::Gray => {
                    let start = stack.iter().position(|&n| n == next).unwrap_or(0);
                    cycles.push(stack[start..].iter().map(|s| s.to_string()).collect());
                }
                Color::Black => {}
            }
        }
        stack.pop();
        color.insert(node, Color::Black);
    }

    let nodes: Vec<&str> = color.keys().copied().collect();
    for node in nodes {
        if color.get(node) == Some(&Color::White) {
            let mut stack = Vec::new();
            dfs(node, &edges, &mut color, &mut stack, &mut cycles);
        }
    }

    let mut reported: HashSet<BTreeSet<String>> = HashSet::new();
    for cycle in cycles {
        let key: BTreeSet<String> = cycle.iter().cloned().collect();
        if !reported.insert(key) {
            continue;
        }
        // 循環に関与するmate idを集める
        let cycle_set: HashSet<&str> = cycle.iter().map(String::as_str).collect();
        let mut mate_ids: Vec<String> = assy
            .mates
            .iter()
            .filter(|m| {
                cycle_set.contains(m.a.instance.as_str())
                    && cycle_set.contains(m.b.instance.as_str())
            })
            .map(|m| m.id.to_string())
            .collect();
        let chain = {
            let mut c = cycle.clone();
            c.push(cycle[0].clone());
            c.join(" → ")
        };
        let span = mate_ids
            .first()
            .and_then(|id| ctx.loc.definition(id, 0));
        let mut related = cycle.clone();
        related.append(&mut mate_ids);
        ctx.push(
            ErrorCode::MateCycle,
            format!("mateグラフに循環があります(基準側→被拘束側): {chain}"),
            span,
            related,
        );
    }
}
