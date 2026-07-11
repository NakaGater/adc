//! M0-1 受入テスト (US-01) / M0 Exit条件:
//! 05-schema.md §9 の最小サンプル (examples/motor_bracket/design.ron) が
//! 仕様の字面のまま parse でき、値レベルで round-trip すること。

use adc_schema::*;

fn sample_src() -> String {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/motor_bracket/design.ron"
    );
    std::fs::read_to_string(path).expect("examples/motor_bracket/design.ron を読めること")
}

fn base_top() -> BindingExpr {
    BindingExpr {
        feature: "base".to_string(),
        elem: ProvidedElem::Face("top".to_string()),
    }
}

#[test]
fn spec_sample_parses() {
    let d = parse_design(&sample_src())
        .unwrap_or_else(|e| panic!("05-schema.md §9 サンプルがparseできること (US-01): {e}"));

    assert_eq!(d.schema_version, "0.1");
    assert!(d.intent.contains("モーターマウントブラケット"));

    // params
    assert_eq!(d.params.len(), 2);
    assert_eq!(
        d.params[0].value,
        ParamValue::Open {
            range: (3.0, 6.0),
            nominal: 4.0
        }
    );
    assert_eq!(d.params[0].unit, Unit::Mm);
    assert_eq!(d.params[1].value, ParamValue::Determined(55.0));

    // part / features
    assert_eq!(d.parts.len(), 1);
    let bracket = &d.parts[0];
    assert_eq!(bracket.id, "bracket");
    assert_eq!(bracket.process, Process::Machining);
    assert_eq!(bracket.features.len(), 4);

    match &bracket.features[0] {
        Feature::Block { id, x, z, .. } => {
            assert_eq!(id.as_deref(), Some("base"));
            assert_eq!(x, &Expr::Lit(80.0));
            assert_eq!(z, &Expr::Param("wall_t".to_string()));
        }
        other => panic!("features[0] は Block のはず: {other:?}"),
    }
    match &bracket.features[1] {
        Feature::Hole {
            id, kind, d, depth, at, ..
        } => {
            assert_eq!(id.as_deref(), Some("bore"));
            assert_eq!(kind, &HoleKind::Simple);
            assert_eq!(d, &Expr::Param("bore_d".to_string()));
            assert_eq!(depth, &HoleDepth::Through);
            assert_eq!(
                at,
                &Some(Placement::On {
                    face: base_top(),
                    at: Pos2::Center
                })
            );
        }
        other => panic!("features[1] は Hole のはず: {other:?}"),
    }
    match &bracket.features[2] {
        Feature::Pattern {
            id, of, kind, count, pitch, ..
        } => {
            assert_eq!(id.as_deref(), Some("bolts"));
            assert_eq!(kind, &PatternKind::Linear2D);
            assert_eq!(count, &Count::Two(2, 2));
            assert_eq!(pitch, &Pitch::Two(Expr::Lit(64.0), Expr::Lit(44.0)));
            match of.as_ref() {
                Feature::Hole {
                    kind, d, cb_d, cb_depth, depth, ..
                } => {
                    assert_eq!(kind, &HoleKind::Counterbore);
                    assert_eq!(d, &Expr::Lit(6.6));
                    assert_eq!(cb_d, &Some(Expr::Lit(11.0)));
                    assert_eq!(cb_depth, &Some(Expr::Lit(6.5)));
                    assert_eq!(depth, &HoleDepth::Through);
                }
                other => panic!("Pattern.of は Hole のはず: {other:?}"),
            }
        }
        other => panic!("features[2] は Pattern のはず: {other:?}"),
    }
    match &bracket.features[3] {
        Feature::Fillet { id, edges, r } => {
            assert_eq!(id.as_deref(), Some("f1"));
            assert_eq!(edges, &EdgeSelector::EdgesOf(base_top()));
            assert_eq!(r, &Expr::Lit(2.0));
        }
        other => panic!("features[3] は Fillet のはず: {other:?}"),
    }

    // anchors
    assert_eq!(bracket.anchors.len(), 3);
    assert_eq!(bracket.anchors[0].id, "bearing_bore");
    assert_eq!(bracket.anchors[0].kind, AnchorKind::Face);
    assert_eq!(
        bracket.anchors[0].binding,
        BindingExpr {
            feature: "bore".to_string(),
            elem: ProvidedElem::Face("wall".to_string())
        }
    );
    assert_eq!(bracket.anchors[2].kind, AnchorKind::Datum('A'));

    // assembly なし、dims/geom_tols は省略時空配列 (05-schema.md §1)
    assert!(d.assembly.is_none());
    assert!(d.dims.is_empty());
    assert!(d.geom_tols.is_empty());

    // assertions
    assert_eq!(d.assertions.len(), 2);
    match &d.assertions[0].check {
        Check::Mass { part, max, min } => {
            assert_eq!(part, "bracket");
            assert_eq!(max, &Expr::Lit(250.0));
            assert_eq!(min, &None);
        }
        other => panic!("assertions[0] は Mass のはず: {other:?}"),
    }
    match &d.assertions[1].check {
        Check::WallThickness {
            part, min, sample_density,
        } => {
            assert_eq!(part, "bracket");
            assert_eq!(min, &Expr::Lit(2.5));
            assert_eq!(sample_density, &1.0);
        }
        other => panic!("assertions[1] は WallThickness のはず: {other:?}"),
    }

    // rationales
    assert_eq!(d.rationales.len(), 3);
    assert_eq!(d.rationales[0].basis, Basis::Assumption);
    assert_eq!(d.rationales[0].author, Author::Human("nakag".to_string()));
    assert_eq!(
        d.rationales[1].basis,
        Basis::Standard("JIS B 1521 深溝玉軸受 6006 外径φ55".to_string())
    );
    assert_eq!(
        d.rationales[2].basis,
        Basis::Requirement("REQ-012 質量目標".to_string())
    );
}

#[test]
fn spec_sample_round_trips() {
    let d = parse_design(&sample_src()).expect("サンプルのparse");
    let text = to_canonical_ron(&d).expect("正準形へのシリアライズ");
    let back = parse_design(&text)
        .unwrap_or_else(|e| panic!("正準形の再パース失敗: {e}\n--- テキスト ---\n{text}"));
    assert_eq!(back, d, "parse→serialize→parse が同値でない");
}
