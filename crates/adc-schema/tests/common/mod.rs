//! 受入テスト共通ヘルパ

use adc_schema::{parse_design, Design};

/// features / anchors スロットにRON断片を差し込める最小のdesignテキスト
pub fn design_src(features: &str, anchors: &str) -> String {
    format!(
        r#"Design(
    schema_version: "0.1",
    intent: "test fixture",
    params: [
        Param(id: "wall_t", value: Determined(4.0), unit: Mm, rationale: "r0"),
        Param(id: "bore_d", value: Determined(55.0), unit: Mm, rationale: "r0"),
    ],
    materials: [Material(id: "a5052", density_g_cm3: 2.68, name: "A5052")],
    parts: [
        Part(
            id: "p1", material: "a5052", process: Machining,
            features: [{features}],
            anchors: [{anchors}],
        ),
    ],
    assertions: [],
    rationales: [
        Rationale(id: "r0", author: Human("test"), basis: Assumption, note: "", timestamp: "2026-07-11T00:00:00Z"),
    ],
)"#
    )
}

pub fn parse_ok(src: &str) -> Design {
    match parse_design(src) {
        Ok(d) => d,
        Err(e) => panic!("parse_design が失敗:\n  {e}\n--- 入力 ---\n{src}"),
    }
}
