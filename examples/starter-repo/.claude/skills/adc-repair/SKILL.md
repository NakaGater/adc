---
name: adc-repair
description: ADC設計の検証Fail・E-*エラーを修復する定型ループ。MCPツール(adc)のbuild_and_check/explain/design_patchを使い、影響調査→修正→検証→差分提示の順で進める。検証Failの修復、E-ANCHOR-BIND/E-FEATURE-FAIL/E-MATE-UNSOLVEDの解消、Openパラメータの絞り込みに使う。
---

# adc-repair: ADC修復ループ

成功基準3予備実験(2026-07-12)で観測された有効な操作列のスキル化。
正典は design.ron。**ジオメトリではなく意図(assertions/rationale)を直す**のが原則。

## 手順

1. **現状把握**: `build_and_check` → exit_code と Fail/Inconclusive の一覧を得る。
   Evidence(anchors / points / note)が修復の一次材料
2. **影響調査を先に(必須)**: 修正対象のid(param / anchor / feature)を特定したら、
   編集前に必ず `explain(id)` で **referenced_by**(構造的参照元)と
   **rationale**(旧根拠)を確認する。参照元が多いidの変更は人間に先に報告
3. **修正**: `design_read` でsha256を取得 → `design_patch(base_sha256, edits)`。
   - editsのold_stringは**一意一致必須**(0件/複数件はE-PATCHで拒否される。
     周辺行を含めて一意にする)
   - 数値を変える場合はrationaleのnoteも同じpatchで更新する(根拠の鮮度維持)
4. **検証**: `build_and_check` で確認。Openパラメータの片端Failなら
   `narrow_param` の suggested_range を使い、範囲を狭めた根拠をrationaleに記録
5. **人間への提示**: git diff(コミット単位)+margin表で報告。
   ループの中身(2〜4の反復)は見せない。1〜3回で収束しないときだけ相談

## 要求変更の禁止(規律)

- **assertions / dims / しきい値(min / max / target等)の変更・削除は人間の承認事項**。
  修復は形状・パラメータ側(features / params / anchors / mates)で行う
- 要求側が誤りだと考える場合も**勝手に変更せず**、根拠付きで人間に提案する
  (例: 「a_massのmax値がREQ-012の記載と不整合です。○○に変更しますか?」)。
  承認されるまで正典の要求は不変

## E-*別の入口

| エラー | 最初にすること |
|---|---|
| E-ANCHOR-BIND | `explain(anchor_id)` で参照元(mate/assertion)を確認 → causeとhintに従い張替え |
| E-FEATURE-FAIL | hintに従う。寸法起因なら param化+`narrow_param` |
| E-MATE-UNSOLVED | 当該mateの基準側(a)→被拘束側(b)の順にアンカー面を確認 |
| E-PATCH (ambiguous/not_found) | old_stringを周辺行込みで一意化して再試行 |

## gatedモードについて

- **対話セッション(このスキルの通常の使い方)= 非gated**。
  Failになる状態も正典に刻める(Red先行のTDDに必要)。ゲートは人間のPRレビュー
- **gated(`adc-mcp --gated`)は無人・自動適用時の安全装置**。
  patchは全Pass時のみ書き込まれ、Fail時は`gated_check.results`が返るので
  それを材料に修正して再試行する
