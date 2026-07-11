//! M0-1 受入テスト (US-01): 不正なRONは行番号付きエラー。
//! エラーコードは 05-schema.md §8 の E-SCHEMA-PARSE。

use adc_schema::{parse_design, SchemaError};

fn expect_parse_err(src: &str) -> (String, usize, usize) {
    match parse_design(src) {
        Ok(_) => panic!("パースが成功してはならない入力:\n{src}"),
        Err(SchemaError::Parse {
            message,
            line,
            column,
        }) => (message, line, column),
        Err(other) => panic!("SchemaError::Parse のはず: {other}"),
    }
}

#[test]
fn syntax_error_reports_line_number() {
    // 3行目: フィールド名の後のコロン欠落
    let src = r#"Design(
    schema_version: "0.1",
    intent "コロン欠落",
)"#;
    let (message, line, column) = expect_parse_err(src);
    assert_eq!(line, 3, "エラー行が3行目を指すこと: {message}");
    assert!(column > 0);
}

#[test]
fn type_error_reports_line_number() {
    // 5行目: density_g_cm3 に文字列
    let src = r#"Design(
    schema_version: "0.1",
    intent: "型不一致",
    params: [],
    materials: [Material(id: "m1", density_g_cm3: "abc", name: "M")],
    parts: [],
    assertions: [],
    rationales: [],
)"#;
    let (message, line, _) = expect_parse_err(src);
    assert_eq!(line, 5, "エラー行が5行目を指すこと: {message}");
}

#[test]
fn unknown_field_reports_line_number() {
    // 4行目: Param の未知フィールド idd
    let src = r#"Design(
    schema_version: "0.1",
    intent: "未知フィールド",
    params: [Param(idd: "x", value: Determined(1.0), unit: Mm, rationale: "r0")],
    materials: [],
    parts: [],
    assertions: [],
    rationales: [],
)"#;
    let (message, line, _) = expect_parse_err(src);
    assert!(
        message.contains("idd"),
        "未知フィールド名がメッセージに含まれること: {message}"
    );
    assert_eq!(line, 4, "エラー行が4行目を指すこと: {message}");
}

#[test]
fn missing_required_field_reports_error() {
    // Param に rationale がない (US-04: rationale必須はまず型レベルで落ちる)
    let src = r#"Design(
    schema_version: "0.1",
    intent: "必須フィールド欠落",
    params: [Param(id: "x", value: Determined(1.0), unit: Mm)],
    materials: [],
    parts: [],
    assertions: [],
    rationales: [],
)"#;
    let (message, line, _) = expect_parse_err(src);
    assert!(
        message.contains("rationale"),
        "欠落フィールド名がメッセージに含まれること: {message}"
    );
    assert!(line > 0, "行番号が付くこと");
}

#[test]
fn error_display_includes_error_code_and_line() {
    let err = parse_design("Design(").unwrap_err();
    let text = err.to_string();
    assert!(
        text.contains("E-SCHEMA-PARSE"),
        "表示にエラーコードを含むこと (05-schema.md §8): {text}"
    );
    assert!(text.contains("line"), "表示に行番号を含むこと: {text}");
}
