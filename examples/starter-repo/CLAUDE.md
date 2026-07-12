# ADC設計リポジトリ運用ガイド(Claude Code用)

このリポジトリの正典は `design.ron`(typed IR)。**ジオメトリはビルド成果物**であり、
コミットするのは意図(要求・根拠・制約)である。あなた(Claude Code)の役割は、
人間の要求を正典に翻訳し、検証ループを自分で回し、人間には**差分とEvidence**で
報告すること。人間はRON文法もCLIも知らない前提で動く。

## 鉄則(順番に効く)

1. **Red先行**: ジオメトリより先に assertions + rationales を書く。
   「何を満たせば完成か」が正典に載ってから形状に入る
2. **未確定は `Open(range)` + `basis: Assumption`**: 根拠なく数値を確定しない。
   仮決めは必ずAssumptionと明示し、確定したらrationaleを更新する
3. **変更前に explain**: 既存のparam/anchorを触る前に
   `adc explain <id> --format=json` で参照元(referenced_by)と旧根拠を確認し、
   影響範囲を先に人間へ報告する(パターンBの核心)
4. **報告はEvidence/margin/差分で**: RON全文を貼らない。checkのEvidence、
   reportのmargin表、diffの変化表が人間の見る顔
5. **外部に見えるリソースは事前確認**: リポジトリの作成・公開、push先の変更、
   Issue/PRの作成、外部サービスへの送信は、実行前に必ず人間の承認を取る
   (ローカルのファイル編集・コミットはこの限りではない)

## CLI一覧(stdout=データ / stderr=ログ)

| コマンド | 用途 |
|---|---|
| `adc check --design design.ron --format=jsonl` | コンパイル+全検証 → results.jsonl。Open含みは自動で3点評価(lo/nominal/hi) |
| `adc check --narrow ...` | 片端Failの軸を二分探索し `suggested_range` をevidenceに付加 |
| `adc explain <id> --format=json` | 定義+rationale連鎖+referenced_by/related。板金partは`derived`(展開長/BA) |
| `adc export --step --design design.ron --out ./out` | STEP出力(人間の目視確認用) |
| `adc diff <rev1> <rev2> --design design.ron --format=json` | 制約差分(rationale込み)+param変更+体積差+margin変化。**コミット済みrevのみ**(作業ツリーは対象外 — 変更をコミットしてから) |
| `adc report results.jsonl` | Markdownのmargin表(Fail先頭→margin昇順)。PR/報告の顔 |

- exit code: **0=全Pass / 1=Fail≥1 / 2=Inconclusive≥1またはE-\***
- `adc build` というコマンドは**存在しない**(コンパイルはcheck/exportに含まれる)
- 検証結果を機械で読むときは常に `--format=jsonl`

## パターンA: 新規部品の設計

1. 要求を聞いたら、まず assertions + rationales(+ Open params)を書く。
   部品はプレースホルダでよい。**注意**: 性能系チェッカー(mass/wall等)は
   プレースホルダでもPassしうる — Redの本質はcheckの赤ではなく
   「intentの要求する形状がまだ正典にない」こと。「何を満たせば完成か」を
   先に固定するのが目的(リハーサル2026-07-12の実測知見)
2. 要求にない寸法・仮定を列挙し、`basis: Assumption` で仮置きした旨を人間に確認する
3. features + anchors を書く → `adc check --format=jsonl`
4. FailはEvidence(anchors / points / note)を材料に自己修正。**ループの中身を
   人間に見せない**(1〜3回で収束しないときだけ相談)
5. 全Pass後: `adc report` の表と `adc export --step` の成果物を提示して確認を取る
6. コミット(CIが同じcheckを再実行し、Step Summaryにreportが載る)

## パターンB: 既存設計の変更

1. **いきなり編集しない**。`adc explain <対象id>` で参照元・旧根拠を確認し、
   影響範囲を1メッセージで報告する
2. param + rationale を更新 → check(キャッシュにより変更部品のみ再計算)
3. `adc diff HEAD~1 HEAD` の差分(制約・体積・margin変化)を人間に提示して承認を取る

## E-*エラー時の修復手順(必ずexplainでの影響調査を先に)

| エラー | 修復手順 |
|---|---|
| E-SCHEMA-* | messageのspan/related IDから該当箇所を修正。参照切れは対象idをexplainして逆参照を確認 |
| E-ANCHOR-BIND {anchor, feature, cause, hint} | cause=**Deleted**→操作後も残る面へ張替え / **Ambiguous**→hintに従いより特定的なprovidesへ / **Untracked**→別のprovides要素へ。張替え前に当該anchorの参照元(mate/assertion)をexplainで確認 |
| E-FEATURE-FAIL {feature, hint} | hintに従う(過大フィレット等)。寸法をparam化して--narrowで実行可能域を探るのも有効 |
| E-MATE-UNSOLVED {mate_id, 原因} | 当該mateの幾何矛盾。基準側(a)→被拘束側(b)の順に、参照アンカーの面をexplain/exportで確認 |
| Inconclusive | **Failではない**。reason(材料未定義・工程不一致・評価不能等)を解消するか、判定不能のまま人間へ報告 |

## --narrow の使いどころ

Open paramを含む設計で**区間の片端だけFail**したとき(results.jsonlの
`samples` でlo/nominal/hiの別が見える)、`adc check --narrow` を実行すると
当該アサーションのevidenceに `suggested_range: <param> ∈ [lo, hi]` が付く。
→ rangeを狭めて、その根拠(narrowの結果)をrationaleに記録する。
公称もFailする場合はnarrowは働かない(設計自体の見直し)。

## 検証結果の読み方

- results.jsonl: 1行=1アサーション。`margin` 正=余裕/負=違反。
  `samples` があればOpen 3点評価の標本別結果
- 一部チェッカーは近似であることをnoteに明記している
  (例: wall_thicknessの一方向保証=「検出した違反は真、未検出は保証しない。
  (反)平行面間の壁のみ検出」)。この注記は人間への報告にも含めること
- 板金部品の曲げ規則(最小曲げ半径・フランジ最小長・穴-曲げ距離)は
  `SheetMetalRules` チェッカーが担当。展開長は `adc explain <part>` の `derived`
