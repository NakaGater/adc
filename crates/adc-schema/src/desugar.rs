//! RON入力糖衣(05-schema.md §2, §4.0)を正準RONへ展開する前処理。
//!
//! 受理する糖衣(契約は tests/m0_1_sugar.rs / tests/m0_1_sample.rs):
//! - Expr位置: 裸数値 / `param("id")` / 中置四則(乗除優先・括弧) → 数値 or 式文字列
//! - BindingExpr: `feature("f").face("n")`(.axis / .edge / .point も同様)
//! - Placement: `on(<binding>, <pos2>)` / `offset(<placement>, (dx, dy, dz))`
//! - Pos2: `center()` / `xy(x, y)` / `from_edge(<edges>, d, along)`
//! - EdgeSelector: `edges_of(<binding>)` / `edges_between(<a>, <b>)`
//! - `Hole.depth`: 数値式 → `Blind(...)`
//! - `Pattern.count` / `Pattern.pitch`: スカラー → `One(...)`、2要素タプル → `Two(...)`
//!
//! 置換は行構造を保存する(置換で行数を変えない)。これにより後段の
//! RONパースエラーの行番号が元テキストと一致する(US-01)。

use crate::error::SchemaError;
use crate::expr::Expr;

// ---------------------------------------------------------------- 字句解析

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TokKind {
    Ident,
    Number,
    Str,
    Char,
    Punct(char),
}

#[derive(Debug, Clone, Copy)]
struct Tok<'a> {
    kind: TokKind,
    text: &'a str,
    start: usize,
    end: usize,
    line: usize,
    col: usize,
}

fn parse_err(line: usize, col: usize, message: impl Into<String>) -> SchemaError {
    SchemaError::Parse {
        message: message.into(),
        line,
        column: col,
    }
}

struct Lexer<'a> {
    src: &'a str,
    i: usize,
    line: usize,
    col: usize,
}

impl<'a> Lexer<'a> {
    fn peek(&self) -> Option<char> {
        self.src[self.i..].chars().next()
    }

    fn peek2(&self) -> Option<char> {
        let mut it = self.src[self.i..].chars();
        it.next();
        it.next()
    }

    fn bump(&mut self) -> char {
        let c = self.peek().expect("bump beyond EOF");
        if c == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        self.i += c.len_utf8();
        c
    }
}

fn lex(src: &str) -> Result<Vec<Tok<'_>>, SchemaError> {
    let mut lx = Lexer {
        src,
        i: 0,
        line: 1,
        col: 1,
    };
    let mut toks = Vec::new();
    while let Some(c) = lx.peek() {
        let (start, line, col) = (lx.i, lx.line, lx.col);
        match c {
            c if c.is_whitespace() => {
                lx.bump();
            }
            '/' if lx.peek2() == Some('/') => {
                while let Some(ch) = lx.peek() {
                    if ch == '\n' {
                        break;
                    }
                    lx.bump();
                }
            }
            '/' if lx.peek2() == Some('*') => {
                lx.bump();
                lx.bump();
                let mut depth = 1u32;
                while depth > 0 {
                    match (lx.peek(), lx.peek2()) {
                        (Some('/'), Some('*')) => {
                            lx.bump();
                            lx.bump();
                            depth += 1;
                        }
                        (Some('*'), Some('/')) => {
                            lx.bump();
                            lx.bump();
                            depth -= 1;
                        }
                        (Some(_), _) => {
                            lx.bump();
                        }
                        (None, _) => {
                            return Err(parse_err(line, col, "閉じられていないブロックコメント"))
                        }
                    }
                }
            }
            quote @ ('"' | '\'') => {
                lx.bump();
                let mut closed = false;
                while let Some(ch) = lx.peek() {
                    if ch == '\\' {
                        lx.bump();
                        if lx.peek().is_some() {
                            lx.bump();
                        }
                        continue;
                    }
                    lx.bump();
                    if ch == quote {
                        closed = true;
                        break;
                    }
                }
                if !closed {
                    return Err(parse_err(line, col, "閉じられていないリテラル"));
                }
                let kind = if quote == '"' { TokKind::Str } else { TokKind::Char };
                toks.push(Tok {
                    kind,
                    text: &src[start..lx.i],
                    start,
                    end: lx.i,
                    line,
                    col,
                });
            }
            c if c.is_ascii_digit() => {
                while matches!(lx.peek(), Some(ch) if ch.is_ascii_digit() || ch == '_') {
                    lx.bump();
                }
                if lx.peek() == Some('.') && matches!(lx.peek2(), Some(ch) if ch.is_ascii_digit()) {
                    lx.bump();
                    while matches!(lx.peek(), Some(ch) if ch.is_ascii_digit() || ch == '_') {
                        lx.bump();
                    }
                }
                if matches!(lx.peek(), Some('e' | 'E')) {
                    lx.bump();
                    if matches!(lx.peek(), Some('+' | '-')) {
                        lx.bump();
                    }
                    while matches!(lx.peek(), Some(ch) if ch.is_ascii_digit()) {
                        lx.bump();
                    }
                }
                toks.push(Tok {
                    kind: TokKind::Number,
                    text: &src[start..lx.i],
                    start,
                    end: lx.i,
                    line,
                    col,
                });
            }
            c if c.is_alphabetic() || c == '_' => {
                while matches!(lx.peek(), Some(ch) if ch.is_alphanumeric() || ch == '_') {
                    lx.bump();
                }
                toks.push(Tok {
                    kind: TokKind::Ident,
                    text: &src[start..lx.i],
                    start,
                    end: lx.i,
                    line,
                    col,
                });
            }
            c => {
                lx.bump();
                toks.push(Tok {
                    kind: TokKind::Punct(c),
                    text: &src[start..lx.i],
                    start,
                    end: lx.i,
                    line,
                    col,
                });
            }
        }
    }
    Ok(toks)
}

// ---------------------------------------------------------------- Expr DSL

/// f64の決定的テキスト表現(Rust Displayは最短表現で決定的)
fn fmt_f64(v: f64) -> String {
    format!("{v}")
}

fn prec(e: &Expr) -> u8 {
    match e {
        Expr::Lit(_) | Expr::Param(_) => 3,
        Expr::Mul(..) | Expr::Div(..) => 2,
        Expr::Add(..) | Expr::Sub(..) => 1,
    }
}

/// ExprのDSL文字列表現。`parse_expr_str` と往復可能(木構造を保存する括弧付け)。
pub(crate) fn expr_dsl(e: &Expr) -> String {
    fn bin(a: &Expr, op: &str, b: &Expr, p: u8) -> String {
        let l = if prec(a) < p {
            format!("({})", expr_dsl(a))
        } else {
            expr_dsl(a)
        };
        // 右辺は同順位でも括弧(左結合の木構造を保存)
        let r = if prec(b) <= p {
            format!("({})", expr_dsl(b))
        } else {
            expr_dsl(b)
        };
        format!("{l} {op} {r}")
    }
    match e {
        Expr::Lit(v) => fmt_f64(*v),
        Expr::Param(id) => format!("param({id})"),
        Expr::Add(a, b) => bin(a, "+", b, 1),
        Expr::Sub(a, b) => bin(a, "-", b, 1),
        Expr::Mul(a, b) => bin(a, "*", b, 2),
        Expr::Div(a, b) => bin(a, "/", b, 2),
    }
}

/// Exprの正準RON値表現: 純リテラルは数値、それ以外はDSL文字列
pub(crate) fn expr_ron_value(e: &Expr) -> String {
    match e {
        Expr::Lit(v) => fmt_f64(*v),
        other => format!("\"{}\"", expr_dsl(other)),
    }
}

/// DSL文字列からExprをパースする(Expr::deserialize の visit_str 用)
pub(crate) fn parse_expr_str(s: &str) -> Result<Expr, String> {
    let toks = lex(s).map_err(|e| e.to_string())?;
    let (e, next) = parse_expr(&toks, 0).map_err(|e| e.to_string())?;
    if next != toks.len() {
        return Err(format!("式の末尾に余分なトークン: {s:?}"));
    }
    Ok(e)
}

// ------------------------------------------------------- トークン列パーサ

type PResult<T> = Result<(T, usize), SchemaError>;

fn tok_err(toks: &[Tok], i: usize, msg: &str) -> SchemaError {
    match toks.get(i) {
        Some(t) => parse_err(t.line, t.col, format!("{msg} (found {:?})", t.text)),
        None => parse_err(
            toks.last().map_or(1, |t| t.line),
            toks.last().map_or(1, |t| t.col),
            format!("{msg} (入力終端)"),
        ),
    }
}

fn is_punct(toks: &[Tok], i: usize, c: char) -> bool {
    matches!(toks.get(i), Some(t) if t.kind == TokKind::Punct(c))
}

fn expect_punct(toks: &[Tok], i: usize, c: char) -> Result<usize, SchemaError> {
    if is_punct(toks, i, c) {
        Ok(i + 1)
    } else {
        Err(tok_err(toks, i, &format!("'{c}' が必要")))
    }
}

fn is_ident(toks: &[Tok], i: usize, s: &str) -> bool {
    matches!(toks.get(i), Some(t) if t.kind == TokKind::Ident && t.text == s)
}

fn number_at(toks: &[Tok], i: usize) -> Option<f64> {
    match toks.get(i) {
        Some(t) if t.kind == TokKind::Number => t.text.replace('_', "").parse().ok(),
        _ => None,
    }
}

/// 文字列/識別子トークンから素の名前を取り出す
fn name_of(toks: &[Tok], i: usize, what: &str) -> Result<(String, usize), SchemaError> {
    match toks.get(i) {
        Some(t) if t.kind == TokKind::Str => {
            let inner = &t.text[1..t.text.len() - 1];
            if inner.contains('\\') {
                return Err(tok_err(toks, i, "エスケープを含む名前は未対応"));
            }
            Ok((inner.to_string(), i + 1))
        }
        Some(t) if t.kind == TokKind::Ident => Ok((t.text.to_string(), i + 1)),
        _ => Err(tok_err(toks, i, &format!("{what}(文字列)が必要"))),
    }
}

/// 文字列/識別子トークンを正準RON文字列リテラルとして再出力する
fn quoted_of(toks: &[Tok], i: usize, what: &str) -> Result<(String, usize), SchemaError> {
    match toks.get(i) {
        Some(t) if t.kind == TokKind::Str => Ok((t.text.to_string(), i + 1)),
        Some(t) if t.kind == TokKind::Ident => Ok((format!("\"{}\"", t.text), i + 1)),
        _ => Err(tok_err(toks, i, &format!("{what}(文字列)が必要"))),
    }
}

/// 四則演算式: expr := term (('+'|'-') term)*
fn parse_expr(toks: &[Tok], i: usize) -> PResult<Expr> {
    let (mut lhs, mut i) = parse_term(toks, i)?;
    loop {
        let op = match toks.get(i) {
            Some(t) if t.kind == TokKind::Punct('+') => '+',
            Some(t) if t.kind == TokKind::Punct('-') => '-',
            _ => break,
        };
        let (rhs, next) = parse_term(toks, i + 1)?;
        lhs = match op {
            '+' => Expr::Add(Box::new(lhs), Box::new(rhs)),
            _ => Expr::Sub(Box::new(lhs), Box::new(rhs)),
        };
        i = next;
    }
    Ok((lhs, i))
}

fn parse_term(toks: &[Tok], i: usize) -> PResult<Expr> {
    let (mut lhs, mut i) = parse_factor(toks, i)?;
    loop {
        let op = match toks.get(i) {
            Some(t) if t.kind == TokKind::Punct('*') => '*',
            Some(t) if t.kind == TokKind::Punct('/') => '/',
            _ => break,
        };
        let (rhs, next) = parse_factor(toks, i + 1)?;
        lhs = match op {
            '*' => Expr::Mul(Box::new(lhs), Box::new(rhs)),
            _ => Expr::Div(Box::new(lhs), Box::new(rhs)),
        };
        i = next;
    }
    Ok((lhs, i))
}

fn parse_factor(toks: &[Tok], i: usize) -> PResult<Expr> {
    match toks.get(i) {
        Some(t) if t.kind == TokKind::Number => {
            let v = number_at(toks, i).ok_or_else(|| tok_err(toks, i, "数値が不正"))?;
            Ok((Expr::Lit(v), i + 1))
        }
        Some(t) if t.kind == TokKind::Punct('-') => {
            let _ = t;
            let (inner, next) = parse_factor(toks, i + 1)?;
            match inner {
                Expr::Lit(v) => Ok((Expr::Lit(-v), next)),
                other => Ok((
                    Expr::Sub(Box::new(Expr::Lit(0.0)), Box::new(other)),
                    next,
                )),
            }
        }
        Some(t) if t.kind == TokKind::Ident && t.text == "param" => {
            let i = expect_punct(toks, i + 1, '(')?;
            let (id, i) = name_of(toks, i, "パラメータID")?;
            let i = expect_punct(toks, i, ')')?;
            Ok((Expr::Param(id), i))
        }
        Some(t) if t.kind == TokKind::Punct('(') => {
            let _ = t;
            let (inner, next) = parse_expr(toks, i + 1)?;
            let next = expect_punct(toks, next, ')')?;
            Ok((inner, next))
        }
        _ => Err(tok_err(toks, i, "式(数値 / param(id) / 括弧)が必要")),
    }
}

/// feature("f").face("n") → (feature: "f", elem: Face("n"))
fn parse_binding(toks: &[Tok], i: usize) -> PResult<String> {
    if !is_ident(toks, i, "feature") {
        return Err(tok_err(toks, i, "feature(...) が必要"));
    }
    let j = expect_punct(toks, i + 1, '(')?;
    let (feat, j) = quoted_of(toks, j, "フィーチャーID")?;
    let j = expect_punct(toks, j, ')')?;
    let j = expect_punct(toks, j, '.')?;
    let (variant, j) = match toks.get(j) {
        Some(t) if t.kind == TokKind::Ident => {
            let v = match t.text {
                "face" => "Face",
                "axis" => "Axis",
                "edge" => "Edge",
                "point" => "Point",
                other => {
                    return Err(tok_err(
                        toks,
                        j,
                        &format!("provides要素は face/axis/edge/point のいずれか (found {other})"),
                    ))
                }
            };
            (v, j + 1)
        }
        _ => return Err(tok_err(toks, j, "provides要素名が必要")),
    };
    let j = expect_punct(toks, j, '(')?;
    let (name, j) = quoted_of(toks, j, "要素名")?;
    let j = expect_punct(toks, j, ')')?;
    Ok((format!("(feature: {feat}, elem: {variant}({name}))"), j))
}

/// edges_of(<binding>) / edges_between(<a>, <b>)
fn parse_edges(toks: &[Tok], i: usize) -> PResult<String> {
    if is_ident(toks, i, "edges_of") {
        let j = expect_punct(toks, i + 1, '(')?;
        let (b, j) = parse_binding(toks, j)?;
        let j = expect_punct(toks, j, ')')?;
        Ok((format!("EdgesOf({b})"), j))
    } else if is_ident(toks, i, "edges_between") {
        let j = expect_punct(toks, i + 1, '(')?;
        let (a, j) = parse_binding(toks, j)?;
        let j = expect_punct(toks, j, ',')?;
        let (b, j) = parse_binding(toks, j)?;
        let j = expect_punct(toks, j, ')')?;
        Ok((format!("EdgesBetween({a}, {b})"), j))
    } else {
        Err(tok_err(toks, i, "edges_of(...) / edges_between(...) が必要"))
    }
}

/// center() / xy(x, y) / from_edge(<edges>, d, along)
fn parse_pos2(toks: &[Tok], i: usize) -> PResult<String> {
    if is_ident(toks, i, "center") {
        let j = expect_punct(toks, i + 1, '(')?;
        let j = expect_punct(toks, j, ')')?;
        Ok(("Center".to_string(), j))
    } else if is_ident(toks, i, "xy") {
        let j = expect_punct(toks, i + 1, '(')?;
        let (x, j) = parse_expr(toks, j)?;
        let j = expect_punct(toks, j, ',')?;
        let (y, j) = parse_expr(toks, j)?;
        let j = expect_punct(toks, j, ')')?;
        Ok((
            format!("Xy({}, {})", expr_ron_value(&x), expr_ron_value(&y)),
            j,
        ))
    } else if is_ident(toks, i, "from_edge") {
        let j = expect_punct(toks, i + 1, '(')?;
        let (edges, j) = parse_edges(toks, j)?;
        let j = expect_punct(toks, j, ',')?;
        let (d, j) = parse_expr(toks, j)?;
        let j = expect_punct(toks, j, ',')?;
        let (along, j) = parse_expr(toks, j)?;
        let j = expect_punct(toks, j, ')')?;
        Ok((
            format!(
                "FromEdge(edge: {edges}, d: {}, along: {})",
                expr_ron_value(&d),
                expr_ron_value(&along)
            ),
            j,
        ))
    } else {
        Err(tok_err(toks, i, "center() / xy(...) / from_edge(...) が必要"))
    }
}

/// on(<binding>, <pos2>) / offset(<placement>, (dx, dy, dz))
fn parse_placement(toks: &[Tok], i: usize) -> PResult<String> {
    if is_ident(toks, i, "on") {
        let j = expect_punct(toks, i + 1, '(')?;
        let (b, j) = parse_binding(toks, j)?;
        let j = expect_punct(toks, j, ',')?;
        let (p, j) = parse_pos2(toks, j)?;
        let j = expect_punct(toks, j, ')')?;
        Ok((format!("On(face: {b}, at: {p})"), j))
    } else if is_ident(toks, i, "offset") {
        let j = expect_punct(toks, i + 1, '(')?;
        let (from, j) = parse_placement(toks, j)?;
        let j = expect_punct(toks, j, ',')?;
        let j = expect_punct(toks, j, '(')?;
        let (dx, j) = parse_expr(toks, j)?;
        let j = expect_punct(toks, j, ',')?;
        let (dy, j) = parse_expr(toks, j)?;
        let j = expect_punct(toks, j, ',')?;
        let (dz, j) = parse_expr(toks, j)?;
        let j = expect_punct(toks, j, ')')?;
        let j = expect_punct(toks, j, ')')?;
        Ok((
            format!(
                "Offset(from: {from}, d: ({}, {}, {}))",
                expr_ron_value(&dx),
                expr_ron_value(&dy),
                expr_ron_value(&dz)
            ),
            j,
        ))
    } else {
        Err(tok_err(toks, i, "on(...) / offset(...) が必要"))
    }
}

// ------------------------------------------------------------ リライタ

#[derive(Debug)]
struct Frame {
    name: Option<String>,
    field: Option<String>,
}

fn frame_is(frames: &[Frame], name: &str, field: &str) -> bool {
    matches!(
        frames.last(),
        Some(f) if f.name.as_deref() == Some(name) && f.field.as_deref() == Some(field)
    )
}

/// src[emitted..toks[start].start] を素通しし、toks[start..end] を replacement で
/// 置き換える。元領域の改行数を保存する(行番号の同一性)。
fn emit_replacement(
    out: &mut String,
    src: &str,
    toks: &[Tok],
    emitted: &mut usize,
    start: usize,
    end: usize,
    replacement: &str,
) {
    let region_start = toks[start].start;
    let region_end = toks[end - 1].end;
    out.push_str(&src[*emitted..region_start]);
    out.push_str(replacement);
    let newlines = src[region_start..region_end]
        .bytes()
        .filter(|&b| b == b'\n')
        .count();
    for _ in 0..newlines {
        out.push('\n');
    }
    *emitted = region_end;
}

fn is_op_at(toks: &[Tok], i: usize) -> bool {
    matches!(
        toks.get(i),
        Some(t) if matches!(t.kind, TokKind::Punct('+') | TokKind::Punct('-') | TokKind::Punct('*') | TokKind::Punct('/'))
    )
}

fn is_terminator_at(toks: &[Tok], i: usize) -> bool {
    match toks.get(i) {
        None => true,
        Some(t) => matches!(
            t.kind,
            TokKind::Punct(',') | TokKind::Punct(')') | TokKind::Punct(']') | TokKind::Punct('}')
        ),
    }
}

/// `(a, b)` 形式の2要素数値タプル
fn try_pair(toks: &[Tok], i: usize) -> Option<(usize, usize)> {
    // 戻り値: (最初の数値index, 2番目の数値index)。終端は i+4 の ')'
    if is_punct(toks, i, '(')
        && number_at(toks, i + 1).is_some()
        && is_punct(toks, i + 2, ',')
        && number_at(toks, i + 3).is_some()
        && is_punct(toks, i + 4, ')')
    {
        Some((i + 1, i + 3))
    } else {
        None
    }
}

/// 糖衣展開の本体
pub(crate) fn desugar(src: &str) -> Result<String, SchemaError> {
    let toks = lex(src)?;
    let mut out = String::with_capacity(src.len());
    let mut emitted = 0usize;
    let mut frames: Vec<Frame> = Vec::new();
    let mut i = 0usize;

    while i < toks.len() {
        let t = &toks[i];

        // フィールド名: IDENT ':'
        if t.kind == TokKind::Ident && is_punct(&toks, i + 1, ':') {
            if let Some(f) = frames.last_mut() {
                f.field = Some(t.text.to_string());
            }
            i += 2;
            continue;
        }

        let at_value = i == 0
            || matches!(
                toks[i - 1].kind,
                TokKind::Punct(':')
                    | TokKind::Punct(',')
                    | TokKind::Punct('(')
                    | TokKind::Punct('[')
                    | TokKind::Punct('{')
            );

        if at_value {
            // ---- Pattern.count: スカラー/タプル → One/Two
            if frame_is(&frames, "Pattern", "count") {
                if t.kind == TokKind::Number {
                    let rep = format!("One({})", t.text);
                    emit_replacement(&mut out, src, &toks, &mut emitted, i, i + 1, &rep);
                    i += 1;
                    continue;
                }
                if let Some((a, b)) = try_pair(&toks, i) {
                    let rep = format!("Two({}, {})", toks[a].text, toks[b].text);
                    emit_replacement(&mut out, src, &toks, &mut emitted, i, i + 5, &rep);
                    i += 5;
                    continue;
                }
            }

            // ---- Pattern.pitch: スカラー式/タプル → One/Two
            if frame_is(&frames, "Pattern", "pitch") {
                if t.kind == TokKind::Punct('(') {
                    // 2要素の式タプルを試す
                    if let Ok((e1, j)) = parse_expr(&toks, i + 1) {
                        if is_punct(&toks, j, ',') {
                            if let Ok((e2, k)) = parse_expr(&toks, j + 1) {
                                if is_punct(&toks, k, ')') {
                                    let rep = format!(
                                        "Two({}, {})",
                                        expr_ron_value(&e1),
                                        expr_ron_value(&e2)
                                    );
                                    emit_replacement(
                                        &mut out, src, &toks, &mut emitted, i, k + 1, &rep,
                                    );
                                    i = k + 1;
                                    continue;
                                }
                            }
                        }
                    }
                } else if t.kind == TokKind::Number
                    || t.kind == TokKind::Punct('-')
                    || is_ident(&toks, i, "param")
                {
                    let (e, next) = parse_expr(&toks, i)?;
                    let rep = format!("One({})", expr_ron_value(&e));
                    emit_replacement(&mut out, src, &toks, &mut emitted, i, next, &rep);
                    i = next;
                    continue;
                }
            }

            // ---- Hole.depth: 数値式 → Blind(...)
            if frame_is(&frames, "Hole", "depth")
                && (t.kind == TokKind::Number
                    || t.kind == TokKind::Punct('-')
                    || is_ident(&toks, i, "param"))
            {
                let (e, next) = parse_expr(&toks, i)?;
                let rep = format!("Blind({})", expr_ron_value(&e));
                emit_replacement(&mut out, src, &toks, &mut emitted, i, next, &rep);
                i = next;
                continue;
            }

            // ---- 糖衣キーワード呼び出し
            if t.kind == TokKind::Ident && is_punct(&toks, i + 1, '(') {
                let handled: Option<(String, usize)> = match t.text {
                    "param" => {
                        let (e, next) = parse_expr(&toks, i)?;
                        Some((expr_ron_value(&e), next))
                    }
                    "feature" => Some(parse_binding(&toks, i)?),
                    "on" | "offset" => Some(parse_placement(&toks, i)?),
                    "center" | "xy" | "from_edge" => Some(parse_pos2(&toks, i)?),
                    "edges_of" | "edges_between" => Some(parse_edges(&toks, i)?),
                    _ => None,
                };
                if let Some((rep, next)) = handled {
                    emit_replacement(&mut out, src, &toks, &mut emitted, i, next, &rep);
                    i = next;
                    continue;
                }
            }

            // ---- 中置四則: 数値/'-'/'(' 始まりで演算子を含む場合のみ書き換え
            let starts_arith = (t.kind == TokKind::Number && is_op_at(&toks, i + 1))
                || t.kind == TokKind::Punct('(');
            if starts_arith {
                if let Ok((e, next)) = parse_expr(&toks, i) {
                    let worthy = !matches!(e, Expr::Lit(_));
                    if worthy && is_terminator_at(&toks, next) {
                        let rep = expr_ron_value(&e);
                        emit_replacement(&mut out, src, &toks, &mut emitted, i, next, &rep);
                        i = next;
                        continue;
                    }
                }
            }
        }

        // ---- 構造の記録
        match t.kind {
            TokKind::Ident if is_punct(&toks, i + 1, '(') => {
                frames.push(Frame {
                    name: Some(t.text.to_string()),
                    field: None,
                });
                i += 2;
            }
            TokKind::Punct('(') | TokKind::Punct('[') | TokKind::Punct('{') => {
                frames.push(Frame {
                    name: None,
                    field: None,
                });
                i += 1;
            }
            TokKind::Punct(')') | TokKind::Punct(']') | TokKind::Punct('}') => {
                frames.pop();
                i += 1;
            }
            _ => {
                i += 1;
            }
        }
    }

    out.push_str(&src[emitted..]);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_ron_is_untouched() {
        let src = r#"Design(
    schema_version: "0.1", // comment
    params: [Param(id: "x", value: Determined(1.0), unit: Mm, rationale: "r")],
)"#;
        assert_eq!(desugar(src).unwrap(), src);
    }

    #[test]
    fn negative_literals_are_untouched() {
        let src = "Pocket(at: (0.0, -5.0, 1e-3))";
        assert_eq!(desugar(src).unwrap(), src);
    }

    #[test]
    fn replacement_preserves_line_count() {
        let src = "Block(z: param(\n\"wall_t\"\n))";
        let out = desugar(src).unwrap();
        assert_eq!(
            src.matches('\n').count(),
            out.matches('\n').count(),
            "改行数を保存すること: {out}"
        );
    }

    #[test]
    fn expr_dsl_roundtrips_tree_structure() {
        // a * (b / 2) と (a * b) / 2 を区別して往復できること
        let e1 = Expr::Mul(
            Box::new(Expr::Param("a".into())),
            Box::new(Expr::Div(
                Box::new(Expr::Param("b".into())),
                Box::new(Expr::Lit(2.0)),
            )),
        );
        let e2 = Expr::Div(
            Box::new(Expr::Mul(
                Box::new(Expr::Param("a".into())),
                Box::new(Expr::Param("b".into())),
            )),
            Box::new(Expr::Lit(2.0)),
        );
        for e in [e1, e2] {
            let s = expr_dsl(&e);
            let back = parse_expr_str(&s).unwrap();
            assert_eq!(back, e, "DSL: {s}");
        }
    }
}
