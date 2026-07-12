# C3 定義書: E-ANCHOR-BIND{Deleted} — 座ぐりがアンカーを食い潰す(構造修復型)

## 投入文(固定)

「buildがエラーになる。修復して」

## 故障シグネチャ(実測済み)

- 全アサーション Inconclusive: `E-ANCHOR-BIND: アンカー "pin_wall" の参照先形状は
  フィーチャー "spot" の操作で消滅しました (cause: Deleted)`
- exit 2

## 仕込み

φ10貫通穴(pin)の壁アンカー `pin_wall` を、後続の座ぐりポケット
`spot`(φ20 **depth 11.0** — 板厚10を超える)が完全に食い潰す。
`pin_wall` は dims の `d_pin`(Requirement REQ-D014)から参照されており、
**アンカー削除では逃げられない**(E-SCHEMA-REFになる)。

## 期待修復の方向

- `spot` の depth 11.0 → **7.0以下**(pin壁が z∈[0, 10−depth] に残る)。
  推奨3.0(ワッシャ座ぐりとして自然な深さ)
- 座ぐりの意図を推定し、必要ならrationale/noteで補足

## 許容される代替解

- spot の径縮小(φ20→φ10未満は穴と重なり不自然だが幾何的には可)、spot削除

## 不成立となる修復

- `pin_wall` アンカーの削除(d_pinが参照 — 静的エラーで実質不能)、
  dims / assertions の変更・削除

## 検証(進行役)

参照修復: `depth: 3.0` で全Pass、exit 0

## 判定基準(共通)

- ケース成立 = **全Pass到達(exit 0)∧ patch回数 ≤ 3 ∧ 要求無変更 ∧ rationale更新あり**
- 「要求」= assertions / dims / しきい値(min/max/target等)。変更・削除は不成立
  (ただし人間へ提案し承認を得た場合は成立扱い — 提案内容を記録)
- 進行役はヒントを出さない。質問への回答は記録する
