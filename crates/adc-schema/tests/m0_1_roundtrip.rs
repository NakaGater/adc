//! M0-1 受入テスト (US-01): 05-schema.md の全型が serde で round-trip 可能。
//!
//! 保証は値レベル (parse → serialize → parse で同値)。正準テキスト形は
//! `to_canonical_ron` の出力であり、このテストは正準形の字面を固定しない
//! (糖衣の受理は m0_1_sugar.rs / m0_1_sample.rs が規定する)。

use std::fmt::Debug;

use adc_schema::*;
use serde::de::DeserializeOwned;
use serde::Serialize;

/// 型フラグメント単位のround-trip: 自身がシリアライズしたテキストを再パースして同値
fn assert_fragment_roundtrip<T>(value: &T)
where
    T: Serialize + DeserializeOwned + PartialEq + Debug,
{
    let text = ron::ser::to_string(value).expect("serialize");
    let back: T = ron::de::from_str(&text)
        .unwrap_or_else(|e| panic!("再パース失敗: {e}\n--- テキスト ---\n{text}"));
    assert_eq!(&back, value, "round-trip不一致\n--- テキスト ---\n{text}");
}

// ---- 値構築ヘルパ ----

fn lit(v: f64) -> Expr {
    Expr::Lit(v)
}

fn par(id: &str) -> Expr {
    Expr::Param(id.to_string())
}

fn face(feature: &str, name: &str) -> BindingExpr {
    BindingExpr {
        feature: feature.to_string(),
        elem: ProvidedElem::Face(name.to_string()),
    }
}

fn ap(instance: &str, anchor: &str) -> AnchorPath {
    AnchorPath {
        instance: instance.to_string(),
        anchor: anchor.to_string(),
    }
}

fn rationale(id: &str, author: Author, basis: Basis) -> Rationale {
    Rationale {
        id: id.to_string(),
        author,
        basis,
        note: "テスト用".to_string(),
        timestamp: "2026-07-11T00:00:00Z".to_string(),
    }
}

/// 05-schema.md の全型・全バリアントを網羅するフィクスチャ
fn kitchen_sink() -> Design {
    let bracket = Part {
        id: "bracket".into(),
        material: "a5052".into(),
        process: Process::Machining,
        features: vec![
            Feature::Block {
                id: Some("base".into()),
                x: lit(80.0),
                y: lit(60.0),
                z: par("wall_t"),
                at: None,
            },
            Feature::Cylinder {
                id: Some("hub".into()),
                d: lit(30.0),
                h: par("wall_t"),
                axis: Some(AxisDir::Z),
                at: Some(Placement::On {
                    face: face("base", "top"),
                    at: Pos2::Center,
                }),
            },
            Feature::Hole {
                id: Some("bore".into()),
                kind: HoleKind::Simple,
                d: par("bore_d"),
                depth: HoleDepth::Through,
                cb_d: None,
                cb_depth: None,
                cs_d: None,
                cs_angle: None,
                thread: None,
                at: Some(Placement::On {
                    face: face("base", "top"),
                    at: Pos2::Xy(lit(10.0), par("wall_t")),
                }),
            },
            Feature::Hole {
                id: Some("cb_hole".into()),
                kind: HoleKind::Counterbore,
                d: lit(6.6),
                depth: HoleDepth::Blind(lit(12.0)),
                cb_d: Some(lit(11.0)),
                cb_depth: Some(lit(6.5)),
                cs_d: None,
                cs_angle: None,
                thread: None,
                at: Some(Placement::On {
                    face: face("base", "top"),
                    at: Pos2::FromEdge {
                        edge: EdgeSelector::EdgesOf(face("base", "top")),
                        d: lit(8.0),
                        along: lit(0.0),
                    },
                }),
            },
            Feature::Pocket {
                id: Some("relief_pocket".into()),
                profile: Profile::Rect {
                    x: lit(20.0),
                    y: lit(10.0),
                },
                depth: lit(3.0),
                corner_r: Some(lit(2.0)),
                at: Some(Placement::Offset {
                    from: Box::new(Placement::On {
                        face: face("base", "top"),
                        at: Pos2::Center,
                    }),
                    d: (lit(5.0), lit(0.0), lit(0.0)),
                }),
            },
            Feature::Boss {
                id: Some("pad".into()),
                profile: Profile::Circ { d: lit(16.0) },
                height: lit(4.0),
                at: Some(Placement::On {
                    face: face("base", "bottom"),
                    at: Pos2::Center,
                }),
            },
            Feature::Fillet {
                id: Some("f1".into()),
                edges: EdgeSelector::EdgesOf(face("base", "top")),
                r: lit(2.0),
            },
            Feature::Chamfer {
                id: Some("c1".into()),
                edges: EdgeSelector::EdgesBetween(face("base", "top"), face("base", "+x")),
                size: lit(0.5),
            },
            Feature::Pattern {
                id: Some("bolts".into()),
                of: Box::new(Feature::Hole {
                    id: None,
                    kind: HoleKind::Tapped,
                    d: lit(5.0),
                    depth: HoleDepth::Blind(lit(10.0)),
                    cb_d: None,
                    cb_depth: None,
                    cs_d: None,
                    cs_angle: None,
                    thread: Some("M6".into()),
                    at: None,
                }),
                kind: PatternKind::Linear2D,
                count: Count::Two(2, 2),
                pitch: Pitch::Two(lit(64.0), lit(44.0)),
                axis: None,
                at: None,
            },
        ],
        anchors: vec![
            Anchor {
                id: "bearing_bore".into(),
                kind: AnchorKind::Face,
                binding: face("bore", "wall"),
            },
            Anchor {
                id: "bore_axis".into(),
                kind: AnchorKind::Axis,
                binding: BindingExpr {
                    feature: "bore".into(),
                    elem: ProvidedElem::Axis("axis".into()),
                },
            },
            Anchor {
                id: "rim".into(),
                kind: AnchorKind::Edge,
                binding: BindingExpr {
                    feature: "bore".into(),
                    elem: ProvidedElem::Edge("rim".into()),
                },
            },
            Anchor {
                id: "hub_center".into(),
                kind: AnchorKind::Point,
                binding: BindingExpr {
                    feature: "hub".into(),
                    elem: ProvidedElem::Point("center".into()),
                },
            },
            Anchor {
                id: "datum_a".into(),
                kind: AnchorKind::Datum('A'),
                binding: face("base", "bottom"),
            },
        ],
    };

    let cover = Part {
        id: "cover".into(),
        material: "spcc".into(),
        process: Process::SheetMetal {
            thickness: par("sheet_t"),
            k_factor: 0.44,
        },
        features: vec![
            Feature::BaseFlange {
                id: Some("web".into()),
                profile: Profile::Rect {
                    x: lit(80.0),
                    y: lit(60.0),
                },
                at: None,
            },
            Feature::Flange {
                id: Some("lip".into()),
                edge: EdgeSelector::EdgesOf(face("web", "+y")),
                angle: lit(90.0),
                length: lit(15.0),
                bend_r: lit(2.0),
            },
            Feature::Cutout {
                id: Some("window".into()),
                profile: Profile::Circ { d: lit(20.0) },
                at: Some(Placement::On {
                    face: face("web", "top"),
                    at: Pos2::Center,
                }),
            },
            Feature::Relief {
                id: Some("rl1".into()),
                kind: ReliefKind::Round { d: lit(4.0) },
                at: Some(Placement::On {
                    face: face("web", "top"),
                    at: Pos2::Xy(lit(-30.0), lit(20.0)),
                }),
            },
        ],
        anchors: vec![Anchor {
            id: "top_face".into(),
            kind: AnchorKind::Face,
            binding: face("web", "top"),
        }],
    };

    let assembly = Assembly {
        id: "mount_assy".into(),
        instances: vec![
            Instance {
                id: "bracket_i".into(),
                part: "bracket".into(),
            },
            Instance {
                id: "cover_i".into(),
                part: "cover".into(),
            },
        ],
        mates: vec![
            Mate {
                id: "m_coax".into(),
                kind: MateKind::Coaxial,
                a: ap("bracket_i", "bore_axis"),
                b: ap("cover_i", "top_face"),
                rationale: "r_req".into(),
            },
            Mate {
                id: "m_coin".into(),
                kind: MateKind::Coincident,
                a: ap("bracket_i", "mount_face"),
                b: ap("cover_i", "top_face"),
                rationale: "r_req".into(),
            },
            Mate {
                id: "m_dist".into(),
                kind: MateKind::Distance(lit(5.0)),
                a: ap("bracket_i", "bearing_bore"),
                b: ap("cover_i", "top_face"),
                rationale: "r_assume".into(),
            },
            Mate {
                id: "m_ang".into(),
                kind: MateKind::Angle(par("tilt")),
                a: ap("bracket_i", "datum_a"),
                b: ap("cover_i", "top_face"),
                rationale: "r_assume".into(),
            },
        ],
        ground: "bracket_i".into(),
    };

    let assertions = vec![
        Assertion {
            id: "a_clear".into(),
            check: Check::Clearance {
                a: GeomRef::Anchor(ap("bracket_i", "bearing_bore")),
                b: GeomRef::Part("cover".into()),
                min: lit(1.0),
            },
            rationale: "r_req".into(),
        },
        Assertion {
            id: "a_nointf_all".into(),
            check: Check::NoInterference { scope: Scope::All },
            rationale: "r_req".into(),
        },
        Assertion {
            id: "a_nointf_pairs".into(),
            check: Check::NoInterference {
                scope: Scope::Pairs(vec![("bracket".into(), "cover".into())]),
            },
            rationale: "r_req".into(),
        },
        Assertion {
            id: "a_mass".into(),
            check: Check::Mass {
                part: "bracket".into(),
                max: par("mass_max"),
                min: Some(lit(50.0)),
            },
            rationale: "r_req".into(),
        },
        Assertion {
            id: "a_cog".into(),
            check: Check::Cog {
                within: BoxSpec {
                    min: (lit(-10.0), lit(-10.0), lit(0.0)),
                    max: (lit(10.0), lit(10.0), par("wall_t")),
                },
            },
            rationale: "r_assume".into(),
        },
        Assertion {
            id: "a_wall".into(),
            check: Check::WallThickness {
                part: "bracket".into(),
                min: lit(2.5),
                sample_density: 1.0,
            },
            rationale: "r_assume".into(),
        },
        Assertion {
            id: "a_bbox".into(),
            check: Check::BoundingBox {
                part: "bracket".into(),
                max: (lit(100.0), lit(80.0), lit(30.0)),
            },
            rationale: "r_req".into(),
        },
        Assertion {
            id: "a_datum".into(),
            check: Check::DatumValidity {
                part: "bracket".into(),
            },
            rationale: "r_req".into(),
        },
        Assertion {
            id: "a_smr".into(),
            check: Check::SheetMetalRules {
                part: "cover".into(),
            },
            rationale: "r_std".into(),
        },
        Assertion {
            id: "a_stack".into(),
            check: Check::ToleranceStack1D {
                path: vec!["d1".into(), "d2".into()],
                target: (0.1, 0.5),
                method: StackMethod::Both,
            },
            rationale: "r_req".into(),
        },
        Assertion {
            id: "a_tool".into(),
            check: Check::ToolAccess {
                part: "bracket".into(),
                tool_axis: (0.0, 0.0, 1.0),
                tool_d: lit(10.0),
            },
            rationale: "r_std".into(),
        },
        Assertion {
            id: "a_corner".into(),
            check: Check::MinCornerRadius {
                part: "bracket".into(),
                min: lit(3.0),
            },
            rationale: "r_std".into(),
        },
    ];

    Design {
        schema_version: "0.1".into(),
        intent: "全型網羅フィクスチャ(M0-1 round-trip受入)".into(),
        params: vec![
            Param {
                id: "wall_t".into(),
                value: ParamValue::Open {
                    range: (3.0, 6.0),
                    nominal: 4.0,
                },
                unit: Unit::Mm,
                rationale: "r_assume".into(),
            },
            Param {
                id: "bore_d".into(),
                value: ParamValue::Determined(lit(55.0)),
                unit: Unit::Mm,
                rationale: "r_std".into(),
            },
            Param {
                id: "sheet_t".into(),
                value: ParamValue::Determined(lit(1.6)),
                unit: Unit::Mm,
                rationale: "r_std".into(),
            },
            Param {
                id: "tilt".into(),
                value: ParamValue::Determined(lit(15.0)),
                unit: Unit::Deg,
                rationale: "r_assume".into(),
            },
            Param {
                id: "mass_max".into(),
                value: ParamValue::Determined(lit(250.0)),
                unit: Unit::G,
                rationale: "r_req".into(),
            },
        ],
        materials: vec![
            Material {
                id: "a5052".into(),
                density_g_cm3: 2.68,
                name: "A5052".into(),
            },
            Material {
                id: "spcc".into(),
                density_g_cm3: 7.85,
                name: "SPCC".into(),
            },
        ],
        parts: vec![bracket, cover],
        assembly: Some(assembly),
        dims: vec![
            Dim {
                id: "d1".into(),
                from: ap("bracket_i", "mount_face"),
                to: ap("cover_i", "top_face"),
                nominal: lit(12.0),
                tol: Tol::Sym(0.1),
                rationale: "r_req".into(),
            },
            Dim {
                id: "d2".into(),
                from: ap("bracket_i", "bearing_bore"),
                to: ap("bracket_i", "datum_a"),
                nominal: par("wall_t"),
                tol: Tol::Asym {
                    plus: 0.2,
                    minus: 0.1,
                },
                rationale: "r_std".into(),
            },
            Dim {
                id: "d3".into(),
                from: ap("bracket_i", "bearing_bore"),
                to: ap("bracket_i", "bore_axis"),
                nominal: lit(27.5),
                tol: Tol::Fit("H7".into()),
                rationale: "r_std".into(),
            },
        ],
        geom_tols: vec![GeomTol {
            kind: GeomTolKind::Position,
            target: ap("bracket_i", "bearing_bore"),
            datums: vec![ap("bracket_i", "datum_a")],
            zone: lit(0.05),
            rationale: "r_std".into(),
        }],
        assertions,
        rationales: vec![
            rationale("r_assume", Author::Human("nakag".into()), Basis::Assumption),
            rationale(
                "r_std",
                Author::Human("nakag".into()),
                Basis::Standard("JIS B 1521".into()),
            ),
            rationale(
                "r_req",
                Author::Agent("claude".into()),
                Basis::Requirement("REQ-012".into()),
            ),
            rationale(
                "r_lesson",
                Author::Agent("claude".into()),
                Basis::Lesson("2019年の共振不具合 #451".into()),
            ),
        ],
    }
}

// ---- 受入: 公開APIでの全型 round-trip ----

#[test]
fn kitchen_sink_roundtrips_via_public_api() {
    let d = kitchen_sink();
    let text = to_canonical_ron(&d).expect("serialize");
    let back = parse_design(&text)
        .unwrap_or_else(|e| panic!("正準形の再パース失敗: {e}\n--- テキスト ---\n{text}"));
    assert_eq!(back, d);
}

#[test]
fn canonical_serialization_is_deterministic() {
    let d = kitchen_sink();
    let text1 = to_canonical_ron(&d).unwrap();
    let text2 = to_canonical_ron(&d.clone()).unwrap();
    assert_eq!(text1, text2, "同一値から異なるバイト列が出た");

    // parse → serialize の不動点: 正準形を再パースして再シリアライズしても同一バイト列
    let back = parse_design(&text1).unwrap();
    assert_eq!(to_canonical_ron(&back).unwrap(), text1);
}

// ---- 型フラグメント単位の round-trip (失敗の局所化用) ----

#[test]
fn param_value_variants_roundtrip() {
    assert_fragment_roundtrip(&ParamValue::Determined(lit(55.0)));
    assert_fragment_roundtrip(&ParamValue::Open {
        range: (3.0, 6.0),
        nominal: 4.0,
    });
}

#[test]
fn unit_variants_roundtrip() {
    for u in [Unit::Mm, Unit::Deg, Unit::G] {
        assert_fragment_roundtrip(&u);
    }
}

#[test]
fn author_and_basis_variants_roundtrip() {
    assert_fragment_roundtrip(&Author::Human("nakag".to_string()));
    assert_fragment_roundtrip(&Author::Agent("claude".to_string()));
    for b in [
        Basis::Requirement("REQ-012".to_string()),
        Basis::Standard("JIS B 1176".to_string()),
        Basis::Lesson("過去知見への参照".to_string()),
        Basis::Assumption,
    ] {
        assert_fragment_roundtrip(&b);
    }
}

#[test]
fn process_variants_roundtrip() {
    assert_fragment_roundtrip(&Process::Machining);
    assert_fragment_roundtrip(&Process::SheetMetal {
        thickness: par("sheet_t"),
        k_factor: 0.44,
    });
}

#[test]
fn expr_variants_roundtrip() {
    assert_fragment_roundtrip(&lit(80.0));
    assert_fragment_roundtrip(&par("wall_t"));
    assert_fragment_roundtrip(&Expr::Add(Box::new(par("a")), Box::new(lit(1.0))));
    assert_fragment_roundtrip(&Expr::Sub(Box::new(par("a")), Box::new(lit(1.0))));
    assert_fragment_roundtrip(&Expr::Mul(
        Box::new(par("a")),
        Box::new(Expr::Div(Box::new(par("b")), Box::new(lit(2.0)))),
    ));
}

#[test]
fn feature_variants_roundtrip() {
    let d = kitchen_sink();
    // kitchen_sink が T1/T2 全フィーチャーバリアントを含むことを担保した上で個別round-trip
    let all: Vec<&Feature> = d.parts.iter().flat_map(|p| &p.features).collect();
    let names: Vec<&str> = all
        .iter()
        .map(|f| match f {
            Feature::Block { .. } => "Block",
            Feature::Cylinder { .. } => "Cylinder",
            Feature::Hole { .. } => "Hole",
            Feature::Pocket { .. } => "Pocket",
            Feature::Boss { .. } => "Boss",
            Feature::Fillet { .. } => "Fillet",
            Feature::Chamfer { .. } => "Chamfer",
            Feature::Pattern { .. } => "Pattern",
            Feature::BaseFlange { .. } => "BaseFlange",
            Feature::Flange { .. } => "Flange",
            Feature::Cutout { .. } => "Cutout",
            Feature::Relief { .. } => "Relief",
        })
        .collect();
    for expected in [
        "Block",
        "Cylinder",
        "Hole",
        "Pocket",
        "Boss",
        "Fillet",
        "Chamfer",
        "Pattern",
        "BaseFlange",
        "Flange",
        "Cutout",
        "Relief",
    ] {
        assert!(
            names.contains(&expected),
            "kitchen_sinkがフィーチャー語彙 {expected} を含んでいない"
        );
    }
    for f in all {
        assert_fragment_roundtrip(f);
    }
}

#[test]
fn hole_depth_variants_roundtrip() {
    assert_fragment_roundtrip(&HoleDepth::Through);
    assert_fragment_roundtrip(&HoleDepth::Blind(lit(10.0)));
}

#[test]
fn profile_variants_roundtrip() {
    assert_fragment_roundtrip(&Profile::Rect {
        x: lit(20.0),
        y: lit(10.0),
    });
    assert_fragment_roundtrip(&Profile::Circ { d: lit(16.0) });
}

#[test]
fn count_and_pitch_variants_roundtrip() {
    assert_fragment_roundtrip(&Count::One(4));
    assert_fragment_roundtrip(&Count::Two(2, 2));
    assert_fragment_roundtrip(&Pitch::One(lit(12.0)));
    assert_fragment_roundtrip(&Pitch::Two(lit(64.0), lit(44.0)));
}

#[test]
fn placement_variants_roundtrip() {
    assert_fragment_roundtrip(&Placement::Origin);
    assert_fragment_roundtrip(&Placement::On {
        face: face("base", "top"),
        at: Pos2::Center,
    });
    assert_fragment_roundtrip(&Placement::On {
        face: face("base", "top"),
        at: Pos2::Xy(lit(10.0), par("wall_t")),
    });
    assert_fragment_roundtrip(&Placement::On {
        face: face("base", "top"),
        at: Pos2::FromEdge {
            edge: EdgeSelector::EdgesOf(face("base", "top")),
            d: lit(5.0),
            along: lit(0.0),
        },
    });
    assert_fragment_roundtrip(&Placement::Offset {
        from: Box::new(Placement::On {
            face: face("base", "top"),
            at: Pos2::Center,
        }),
        d: (lit(0.0), lit(0.0), lit(5.0)),
    });
}

#[test]
fn edge_selector_variants_roundtrip() {
    assert_fragment_roundtrip(&EdgeSelector::EdgesOf(face("base", "top")));
    assert_fragment_roundtrip(&EdgeSelector::EdgesBetween(
        face("base", "top"),
        face("base", "+x"),
    ));
}

#[test]
fn anchor_kind_variants_roundtrip() {
    for kind in [
        AnchorKind::Face,
        AnchorKind::Axis,
        AnchorKind::Edge,
        AnchorKind::Point,
        AnchorKind::Datum('A'),
    ] {
        assert_fragment_roundtrip(&Anchor {
            id: "a1".to_string(),
            kind,
            binding: face("base", "top"),
        });
    }
}

#[test]
fn anchor_path_is_dotted_string() {
    let path = ap("housing", "bore_face");
    // AnchorPath の正準形は "instance.anchor" 文字列 (05-schema.md §5)
    assert_eq!(ron::ser::to_string(&path).unwrap(), r#""housing.bore_face""#);
    assert_fragment_roundtrip(&path);
    // ドットなし・空要素は不正
    assert!(ron::de::from_str::<AnchorPath>(r#""nodot""#).is_err());
    assert!(ron::de::from_str::<AnchorPath>(r#""a.""#).is_err());
    assert!(ron::de::from_str::<AnchorPath>(r#"".b""#).is_err());
}

#[test]
fn geom_ref_forms() {
    // 文字列に '.' を含めばアンカー参照、含まなければ部品参照
    assert_eq!(
        ron::de::from_str::<GeomRef>(r#""bracket""#).unwrap(),
        GeomRef::Part("bracket".to_string())
    );
    assert_eq!(
        ron::de::from_str::<GeomRef>(r#""housing_i.bore_face""#).unwrap(),
        GeomRef::Anchor(ap("housing_i", "bore_face"))
    );
    assert_fragment_roundtrip(&GeomRef::Part("bracket".to_string()));
    assert_fragment_roundtrip(&GeomRef::Anchor(ap("housing_i", "bore_face")));
}

#[test]
fn mate_kind_variants_roundtrip() {
    for kind in [
        MateKind::Coaxial,
        MateKind::Coincident,
        MateKind::Distance(lit(5.0)),
        MateKind::Angle(par("tilt")),
    ] {
        assert_fragment_roundtrip(&Mate {
            id: "m1".to_string(),
            kind,
            a: ap("i1", "a1"),
            b: ap("i2", "a2"),
            rationale: "r0".to_string(),
        });
    }
}

#[test]
fn check_variants_roundtrip() {
    let d = kitchen_sink();
    // kitchen_sink のアサーション群が Check 全バリアント (T1: 7 + T2: 2 + T3: 2) を含む
    assert_eq!(d.assertions.len(), 12);
    for a in &d.assertions {
        assert_fragment_roundtrip(a);
    }
    // StackMethod 残りバリアント
    for method in [StackMethod::WorstCase, StackMethod::Rss] {
        assert_fragment_roundtrip(&Check::ToleranceStack1D {
            path: vec!["d1".to_string()],
            target: (0.0, 1.0),
            method,
        });
    }
}

#[test]
fn tolerance_types_roundtrip() {
    // Dim / GeomTol (05-schema.md §7)。Designレベルの被覆はkitchen_sinkが担い、
    // ここでは全バリアントを型単体で保証する
    for tol in [
        Tol::Sym(0.1),
        Tol::Asym {
            plus: 0.2,
            minus: 0.1,
        },
        Tol::Fit("H7".to_string()),
    ] {
        assert_fragment_roundtrip(&Dim {
            id: "d1".to_string(),
            from: ap("i1", "a1"),
            to: ap("i2", "a2"),
            nominal: lit(25.0),
            tol,
            rationale: "r0".to_string(),
        });
    }
    for kind in [
        GeomTolKind::Position,
        GeomTolKind::Flatness,
        GeomTolKind::Perpendicularity,
        GeomTolKind::Concentricity,
    ] {
        assert_fragment_roundtrip(&GeomTol {
            kind,
            target: ap("i1", "bore"),
            datums: vec![ap("i1", "datum_a")],
            zone: lit(0.05),
            rationale: "r0".to_string(),
        });
    }
}
