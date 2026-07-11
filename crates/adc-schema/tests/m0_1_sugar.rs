//! M0-1 受入テスト (US-01): RON入力糖衣の受理契約。
//!
//! 05-schema.md §4.0 は `on(feature("base").face("top"), center())` のような
//! 関数風の糖衣を許可する。§2 は数値位置で リテラル / param(id) / 四則演算 を要求する。
//! ここでは糖衣→型付きIRの対応を固定する(正準出力の字面は固定しない)。

mod common;

use adc_schema::*;
use common::{design_src, parse_ok};

fn first_feature(d: &Design) -> &Feature {
    &d.parts[0].features[0]
}

fn base_top() -> BindingExpr {
    BindingExpr {
        feature: "base".to_string(),
        elem: ProvidedElem::Face("top".to_string()),
    }
}

fn block_with_z(z_src: &str) -> Feature {
    let src = design_src(
        &format!(r#"Block(id: "base", x: 80.0, y: 60.0, z: {z_src})"#),
        "",
    );
    first_feature(&parse_ok(&src)).clone()
}

fn z_of(f: &Feature) -> Expr {
    match f {
        Feature::Block { z, .. } => z.clone(),
        other => panic!("Blockのはず: {other:?}"),
    }
}

// ---- Expr 糖衣 (05-schema.md §2) ----

#[test]
fn expr_accepts_bare_numeric_literal() {
    let f = block_with_z("4.5");
    assert_eq!(z_of(&f), Expr::Lit(4.5));
    // 整数表記も数値リテラルとして受理
    let f = block_with_z("4");
    assert_eq!(z_of(&f), Expr::Lit(4.0));
}

#[test]
fn expr_accepts_param_call() {
    let f = block_with_z(r#"param("wall_t")"#);
    assert_eq!(z_of(&f), Expr::Param("wall_t".to_string()));
}

#[test]
fn expr_accepts_infix_arithmetic_with_precedence() {
    // 乗除が加減に優先する
    let f = block_with_z(r#"param("wall_t") * 2.0 + 1.0"#);
    assert_eq!(
        z_of(&f),
        Expr::Add(
            Box::new(Expr::Mul(
                Box::new(Expr::Param("wall_t".to_string())),
                Box::new(Expr::Lit(2.0)),
            )),
            Box::new(Expr::Lit(1.0)),
        )
    );
}

#[test]
fn expr_accepts_parentheses() {
    let f = block_with_z(r#"(param("wall_t") + 1.0) / 2.0"#);
    assert_eq!(
        z_of(&f),
        Expr::Div(
            Box::new(Expr::Add(
                Box::new(Expr::Param("wall_t".to_string())),
                Box::new(Expr::Lit(1.0)),
            )),
            Box::new(Expr::Lit(2.0)),
        )
    );
}

// ---- BindingExpr 糖衣 (05-schema.md §4) ----

#[test]
fn binding_accepts_feature_face_chain() {
    let src = design_src(
        r#"Block(id: "base", x: 10.0, y: 10.0, z: 4.0)"#,
        r#"Anchor(id: "a1", kind: Face, binding: feature("base").face("top"))"#,
    );
    let d = parse_ok(&src);
    assert_eq!(d.parts[0].anchors[0].binding, base_top());
}

#[test]
fn binding_accepts_axis_and_edge_chains() {
    let src = design_src(
        r#"Block(id: "base", x: 10.0, y: 10.0, z: 4.0)"#,
        r#"
        Anchor(id: "ax", kind: Axis, binding: feature("bore").axis("axis")),
        Anchor(id: "rim", kind: Edge, binding: feature("bore").edge("rim")),
        "#,
    );
    let d = parse_ok(&src);
    assert_eq!(
        d.parts[0].anchors[0].binding,
        BindingExpr {
            feature: "bore".to_string(),
            elem: ProvidedElem::Axis("axis".to_string())
        }
    );
    assert_eq!(
        d.parts[0].anchors[1].binding,
        BindingExpr {
            feature: "bore".to_string(),
            elem: ProvidedElem::Edge("rim".to_string())
        }
    );
}

// ---- Placement / Pos2 糖衣 (05-schema.md §4.0) ----

fn hole_at(at_src: &str) -> Placement {
    let src = design_src(
        &format!(
            r#"Hole(id: "h1", kind: Simple, d: 6.0, depth: Through, at: {at_src})"#
        ),
        "",
    );
    match first_feature(&parse_ok(&src)) {
        Feature::Hole { at: Some(p), .. } => p.clone(),
        other => panic!("at付きHoleのはず: {other:?}"),
    }
}

#[test]
fn placement_on_center() {
    assert_eq!(
        hole_at(r#"on(feature("base").face("top"), center())"#),
        Placement::On {
            face: base_top(),
            at: Pos2::Center
        }
    );
}

#[test]
fn placement_on_xy() {
    assert_eq!(
        hole_at(r#"on(feature("base").face("top"), xy(10.0, param("wall_t")))"#),
        Placement::On {
            face: base_top(),
            at: Pos2::Xy(Expr::Lit(10.0), Expr::Param("wall_t".to_string()))
        }
    );
}

#[test]
fn placement_on_from_edge() {
    assert_eq!(
        hole_at(r#"on(feature("base").face("top"), from_edge(edges_of(feature("base").face("top")), 5.0, 0.0))"#),
        Placement::On {
            face: base_top(),
            at: Pos2::FromEdge {
                edge: EdgeSelector::EdgesOf(base_top()),
                d: Expr::Lit(5.0),
                along: Expr::Lit(0.0),
            }
        }
    );
}

#[test]
fn placement_offset() {
    assert_eq!(
        hole_at(r#"offset(on(feature("base").face("top"), center()), (0.0, 0.0, 5.0))"#),
        Placement::Offset {
            from: Box::new(Placement::On {
                face: base_top(),
                at: Pos2::Center
            }),
            d: (Expr::Lit(0.0), Expr::Lit(0.0), Expr::Lit(5.0)),
        }
    );
}

// ---- EdgeSelector 糖衣 (05-schema.md §4.1) ----

#[test]
fn edge_selector_edges_of() {
    let src = design_src(
        r#"Fillet(id: "f1", edges: edges_of(feature("base").face("top")), r: 2.0)"#,
        "",
    );
    match first_feature(&parse_ok(&src)) {
        Feature::Fillet { edges, r, .. } => {
            assert_eq!(edges, &EdgeSelector::EdgesOf(base_top()));
            assert_eq!(r, &Expr::Lit(2.0));
        }
        other => panic!("Filletのはず: {other:?}"),
    }
}

#[test]
fn edge_selector_edges_between() {
    let src = design_src(
        r#"Chamfer(id: "c1", edges: edges_between(feature("base").face("top"), feature("base").face("+x")), size: 0.5)"#,
        "",
    );
    match first_feature(&parse_ok(&src)) {
        Feature::Chamfer { edges, .. } => {
            assert_eq!(
                edges,
                &EdgeSelector::EdgesBetween(
                    base_top(),
                    BindingExpr {
                        feature: "base".to_string(),
                        elem: ProvidedElem::Face("+x".to_string())
                    }
                )
            );
        }
        other => panic!("Chamferのはず: {other:?}"),
    }
}

// ---- Hole depth / Pattern count・pitch 糖衣 ----

#[test]
fn hole_depth_bare_number_is_blind() {
    let src = design_src(
        r#"Hole(id: "h1", kind: Simple, d: 6.0, depth: 10.0)"#,
        "",
    );
    match first_feature(&parse_ok(&src)) {
        Feature::Hole { depth, .. } => {
            assert_eq!(depth, &HoleDepth::Blind(Expr::Lit(10.0)));
        }
        other => panic!("Holeのはず: {other:?}"),
    }
}

#[test]
fn pattern_count_and_pitch_accept_scalar_and_tuple() {
    // Linear: スカラー
    let src = design_src(
        r#"Pattern(id: "p1", of: Hole(kind: Simple, d: 5.0, depth: Through), kind: Linear, count: 4, pitch: 12.0)"#,
        "",
    );
    match first_feature(&parse_ok(&src)) {
        Feature::Pattern { count, pitch, .. } => {
            assert_eq!(count, &Count::One(4));
            assert_eq!(pitch, &Pitch::One(Expr::Lit(12.0)));
        }
        other => panic!("Patternのはず: {other:?}"),
    }
    // Linear2D: タプル
    let src = design_src(
        r#"Pattern(id: "p2", of: Hole(kind: Simple, d: 5.0, depth: Through), kind: Linear2D, count: (2, 2), pitch: (64.0, 44.0))"#,
        "",
    );
    match first_feature(&parse_ok(&src)) {
        Feature::Pattern { count, pitch, .. } => {
            assert_eq!(count, &Count::Two(2, 2));
            assert_eq!(pitch, &Pitch::Two(Expr::Lit(64.0), Expr::Lit(44.0)));
        }
        other => panic!("Patternのはず: {other:?}"),
    }
}
