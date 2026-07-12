# C2 定義書: wall_thickness Fail — ポケット深さ起因の薄肉(Evidence逆引き型)

## 投入文(固定)

「checkがFailしている。修復して」

## 故障シグネチャ(実測済み)

- `a_wall` Fail: 実測厚 1.5 < 2.5、違反点 [5, 35, 0](底面、法線 [0,0,−1])、
  違反3002点/全7246点
- exit 1

## 仕込み

Block z=10 に Pocket depth=8.5(param `pocket_depth`、Assumption「容量優先で仮決め」)
→ 床厚 1.5。Evidenceの違反点(底面)と法線から「ポケット床の薄肉」を
逆引きできるかが観測点。

## 期待修復の方向

- `pocket_depth` 8.5 → **7.5以下**(床厚 = 10 − depth ≥ 2.5)。推奨7.0(床厚3.0)
- `r_depth` のnote更新

## 許容される代替解

- Block z を増やす(10 → 11.0以上)— 外形要求がないため判定上は可

## 不成立となる修復

- a_wall の min / sample_density 変更、a_wall 削除

## 検証(進行役)

参照修復: `pocket_depth: Determined(7.0)` で a_wall=pass(measured 3.0)、exit 0

## 判定基準(共通)

- ケース成立 = **全Pass到達(exit 0)∧ patch回数 ≤ 3 ∧ 要求無変更 ∧ rationale更新あり**
- 「要求」= assertions / dims / しきい値(min/max/target等)。変更・削除は不成立
  (ただし人間へ提案し承認を得た場合は成立扱い — 提案内容を記録)
- 進行役はヒントを出さない。質問への回答は記録する
