# ADR-001: 正典は型付きIR、幾何参照は意味的アンカーの2段束縛

**Status:** Accepted

## Context

従来CADの正典は操作履歴+B-repであり、(1) 設計意図が保存されない、(2) 幾何ID参照(Face#42)が再生成で黙って壊れる(topological naming problem)、(3) LLMが安全に編集できない。独自テキスト構文から始めるとパーサー実装が先行して工数を圧迫する。

## Decision

1. **正典は型付きIR(Rust構造体)とし、シリアライズ形式はRON。** 独自構文フロントエンドはPhase 2の糖衣とする。serdeによるround-trip保証をスキーマのテストとする
2. **幾何参照は全て意味的アンカー(`bearing_bore`等)経由。** 直接の幾何ID参照はスキーマ上存在させない
3. **束縛は2段解決:** アンカー → 生成フィーチャー(`provides`宣言) → B-rep実体(OCCT History API `BRepTools_History` で追跡)
4. **再束縛失敗はコンパイルエラー(E-ANCHOR-BIND)。** 黙って壊れる状態遷移をシステムから排除する

## Consequences

- (+) Git差分・マージ・レビューが正典に対して意味を持つ。LLMはRONを直接読み書きできる
- (+) 再生成に対する参照の安定性が「祈り」ではなく「保証(または明示的失敗)」になる
- (−) 全フィーチャーが`provides`を正しく宣言する規律が必要。フィーチャー実装ごとにHistory追跡のテストを書くこと
- (−) RON手書きは人間には冗長。MVPではAIが書く前提で許容

## Alternatives considered

- **CadQuery/build123d型のスクリプト正典:** 手続き的で再生成が脆く、静的検証が困難。却下
- **JSON:** コメント・enum表現が貧弱。RONはRust型と1:1で写る。JSONはAPI境界(explain出力等)でのみ使用
