# C4 定義書: E-MATE-UNSOLVED — 廃止漏れの旧mateが矛盾(特定→修正型)

## 投入文(固定)

「buildがエラーになる。修復して」

## 故障シグネチャ(実測済み)

- 全アサーション Inconclusive: `E-MATE-UNSOLVED: mate "m_lift":
  逐次解決で満たせません(先行mateとの矛盾の可能性、残差=7.000e0)`
- exit 2

## 仕込み

同じ面ペアに `m_lift`(Distance(-2.0)、rationale=Requirement「改訂配置」)と
`m_lift_old`(Distance(5.0)、rationale=**Assumption「旧配置。改訂で廃止予定」**)が
共存。エラーが報告するのは**壊された側**の `m_lift` であり、原因は
`m_lift_old`。explainでrationaleを調べて廃止漏れを特定できるかが観測点
(残差7.0 = |-2.0 − 5.0| も手がかり)。

## 期待修復の方向

- `m_lift_old` の**削除**(rationaleが「廃止予定」と明言)

## 許容される代替解

- m_lift_old の値を -2.0 に揃える(重複拘束として残る — 非推奨だが全Passには到達。
  記録に残す)

## 不成立となる修復

- m_lift(Requirement側)の削除・変更 / assertions の変更

## 検証(進行役)

参照修復: m_lift_old の行削除で a_clear=pass(2.5)、b_map=pass、exit 0

## 判定基準(共通)

- ケース成立 = **全Pass到達(exit 0)∧ patch回数 ≤ 3 ∧ 要求無変更 ∧ rationale更新あり**
- 「要求」= assertions / dims / しきい値(min/max/target等)。変更・削除は不成立
  (ただし人間へ提案し承認を得た場合は成立扱い — 提案内容を記録)
- 進行役はヒントを出さない。質問への回答は記録する
