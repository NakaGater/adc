# M6-1 設計メモ: MCPサーバー(レビュー用)

2026-07-12。実装前のレビュー対象。ADR-006(LLM非搭載・APIキー持ち込み禁止・
外部通信ゼロ)を上位制約とする。

## 0. 全体方針

- **クレート**: `crates/adc-mcp`(バイナリ `adc-mcp`)。stdio transportのみ
  (ローカルプロセス間 — 外部通信ゼロを構造的に満たす)
- **サーバーは1設計に束縛**: 起動時に `adc-mcp --design <path> [--gated]`。
  ツール引数にパスを持たせない(パストラバーサル面の縮小+.mcp.jsonの
  リポジトリ単位設定と一致)
- **SDK(要判断a)**: 公式Rust SDK(rmcp)を推奨。手書きJSON-RPCは
  プロトコル追従リスクが高い。rmcpの依存(tokio等)はLLMクライアントでは
  ないためADR-006に抵触しない(CI依存チェックの禁止対象は
  reqwest等のHTTPクライアント+LLM SDK — 対象リストを明文化して継続)
- 全ツールの出力は既存の正準構造(CheckResult / ExplainOutput /
  ValidationError)をそのままJSONで返す。**新しい表現形式を発明しない**

## 1. ツールI/Oスキーマ

### design_read

| in | out |
|---|---|
| `{}` | `{ source: string, sha256: string, valid: bool, errors: ValidationError[] }` |

sha256は楽観ロック用(design_patchのbase_sha256に渡す)。

### design_patch

| in | out |
|---|---|
| `{ base_sha256: string, edits?: [{old_string, new_string}], full_source?: string, dry_run?: bool }` | `{ applied: bool, new_sha256?: string, validation: { ok: bool, errors: ValidationError[] }, gated_check?: { exit_code, results: CheckResult[] }, rejected_reason?: string }` |

- **表現(承認済みb+修正)**: `edits`(完全一致文字列置換の列、適用は宣言順)
  **または** `full_source`(全置換)。両方指定はエラー。
  **editsは対象文字列の一意一致を必須とする**: 一致0件・複数件は適用せず
  構造化エラー `{code: "E-PATCH", kind: "not_found"|"ambiguous", edit_index,
  occurrences}` を返す(適用曖昧性の拒否 — 2026-07-12承認時修正①)
- `base_sha256` 不一致 → 適用せず `rejected_reason: "conflict"`(楽観ロック)
- 静的検証NG → 適用せず errors返却
- **--gatedモード(承認済みc+修正)**: サーバー起動フラグ。ONのとき
  patch適用前にフルcheckを走らせ、**exit 0(全Pass)のときのみ**書き込む。
  Fail/Inconclusiveなら書き込まずresultsを返す(エージェントは結果を見て
  patchを修正して再試行)。呼び出し側からgateを外すことはできない。
  **用途限定(2026-07-12承認時修正②)**: 対話セッションでは**非gated**が既定
  (人間のPRレビューがゲートの役割を果たす。Redを正典に刻めることが
  TDDフローに必要)。gatedは**無人・自動適用**(cron的なエージェント運転等)の
  安全装置として使う。adc-repairスキルにも同旨を明記する
- `dry_run: true` は検証+gated checkまで行い書き込まない

### build_and_check

| in | out |
|---|---|
| `{ narrow?: bool, filter?: string[], no_cache?: bool }` | `{ exit_code: 0|1|2, results: CheckResult[], dof: [{instance, remaining, note}] }` |

results.jsonlの各行を**構造のまま**返す(samples/suggested_range含む)。

### evidence_query(要判断d)

| in | out |
|---|---|
| `{ assert_id?: string, status?: "pass"|"fail"|"inconclusive" }` | `{ results: CheckResult[] }` |

意味論: **キャッシュ付きcheckを再実行してフィルタ**(直近結果の保存状態に
依存しない=ステートレス。キャッシュにより2回目以降は軽い)。

### narrow_param(要判断e)

| in | out |
|---|---|
| `{ param?: string }` | `{ suggestions: [{assert_id, param, lo, hi, granularity}], results: CheckResult[] }` |

実装: adc-checkの`run_checks_narrow`を**構造化戻り値に拡張**
(現状はevidence noteの文字列のみ → `SuggestedRange`構造体をlibに追加し、
noteは従来どおり維持=results.jsonlはバイト互換)。`param`指定時は
当該軸のみ探索(未指定は全片端Fail軸)。

### explain(実験で最頻の影響調査操作のため昇格)

| in | out |
|---|---|
| `{ id: string }` | ExplainOutput(docs/explain-schema.mdそのまま。derived含む) |

## 2. エラーモデル

- ツールは失敗してもプロトコルエラーにせず、`{ error: {code: "E-...", ...} }`
  を正常応答で返す(エージェント修復ループの入力 — 既存のE-*体系のJSON)
- パニックはサーバーで捕捉してE-INTERNALに変換(abortゼロの原則をMCP境界でも)

## 3. agent-skills/adc-repair(同梱スキル)

実験(成功基準3予備)で観測した操作列をスキル化:

```
1. build_and_check → Fail/E-*の一覧を得る
2. 修正対象のid(param/anchor/feature)を特定したら、編集前に必ず
   explain(id) で referenced_by / rationale を確認(影響調査を先に)
3. design_patch(edits, base_sha256)(gatedならPassまで書き込まれない)
4. build_and_check で確認 → 人間へは diff(git)+margin表で提示
```

配置: `agent-skills/adc-repair/SKILL.md`(Claude Codeのスキル形式)。
starter-repoにも同梱し、.mcp.json雛形(adc-mcp起動設定)を追加する。

## 4. テスト計画(受入)

- プロトコル層: stdioでtools/list→6ツール、tools/callの往復
  (プロセス起動の統合テスト)
- gated: Failになるpatch → applied=false+results返却+ファイル不変(バイト比較)
- 楽観ロック: base_sha256不一致 → conflict
- narrow_param: M4-2フィクスチャで構造化suggestionが返る
- ADR-006: CI依存チェックに禁止クレートリスト(reqwest/hyper-client系/
  openai系等)を追加し、adc-mcp含む全クレートを検査

## 5. スコープ外(明示)

- HTTP/SSE transport、複数設計の同時サービング、認証(ローカルstdioのみ)
- design_patchの構造化パッチ(IRレベルのJSON Patch)— MVPは文字列edits
