# CLIリファレンス (v0.1)

`adc` バイナリのコマンド体系。**stdoutはデータ、stderrはログ**を厳守する(パイプ・エージェント消費の前提)。全コマンドは `--design <path>`(既定: `./design.ron`)を受ける。

## コマンド一覧

| コマンド | 役割 | 主な出力 |
|---|---|---|
| `adc build` | 正典→B-repコンパイル | `.adc/cache/*.brep`、ビルドサマリ |
| `adc check` | 全アサーション検証 | `results.jsonl` |
| `adc check --narrow` | Openパラメータの実行可能区間絞り込み(ADR-004) | suggested_range付きresults |
| `adc check -v` | cacheイベント等の診断ログをstderrへ(既定は静粛 — M6-0) | (stderr) |
| `adc diff <rev1> <rev2>` | 制約差分+ジオメトリ差分 | 差分レポート |
| `adc explain <id>` | Param/Anchor/Assertion/Mateの定義+rationale+参照元一覧 | JSON |
| `adc export --step [--out <dir>]` | STEP AP242出力(部品/Assy) | `.step` ファイル |
| `adc report [<results.jsonl>]` | margin一覧のMarkdownテーブル生成(PRコメント用) | Markdown |

## 共通フラグ

| フラグ | 意味 |
|---|---|
| `--format=text\|json\|jsonl` | 出力形式。既定はtext(人間向け)。エージェント/CIはjsonl |
| `--no-cache` | キャッシュ(ADR-003 §6)を無視して全再計算 |
| `--filter <assert_id,...>` | checkの対象アサーションを限定 |
| `-q / -v` | stderrログの量 |

## Exit code(check系)

| code | 意味 |
|---|---|
| 0 | 全アサーションPass |
| 1 | Fail ≥ 1 |
| 2 | Fail=0 かつ Inconclusive ≥ 1、またはコンパイル/スキーマエラー(E-*) |

buildは 0=成功 / 2=エラー。CIはexit codeのみでゲート判定できること。

## 出力契約

- `--format=jsonl` のとき、stdoutの各行は 05-schema.md §6 の `CheckResult`(check)または §8 の構造化エラー(build失敗)のJSON
- 浮動小数の出力桁数は固定(決定性: 同一入力でバイト再現。Intent成功基準2)
- `adc explain` の出力JSONスキーマはM0-4で確定し、以後後方互換を維持する(エージェントの依存面)

## 使用例

```bash
# 日常ループ(エージェントが実行する典型列)
adc build --format=json          # コンパイル+アンカー束縛
adc check --format=jsonl         # 全検証、results.jsonlも生成
adc explain wall_t --format=json # 修正前の影響調査

# CI(GitHub Actions テンプレートの中身)
adc check --format=jsonl > results.jsonl
adc report results.jsonl >> "$GITHUB_STEP_SUMMARY"

# 人間の確認
adc export --step --out ./out && open ./out/bracket.step  # 既存ビューアで目視
adc diff HEAD~1 HEAD
```
