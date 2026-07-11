# `adc explain` 出力JSONスキーマ (v0.1)

M0-4で確定。**以後このスキーマは後方互換を維持する**(エージェントの依存面 — 07-cli.md 出力契約)。フィールドの追加は許可、既存フィールドの削除・意味変更・リネームは不可。

## 呼び出し

```bash
adc explain <id> [--design <path>] [--format=json]
```

- `--design` 既定: `./design.ron`
- 出力はJSON固定(`--format=json` のみ受理)。stdout=データ、stderr=ログ

## exit code

| code | 意味 |
|---|---|
| 0 | 一意に解決(status: found) |
| 1 | not_found または ambiguous(データはstdoutに出力される) |
| 2 | designのE-*エラー(構造化エラー配列をstdoutに出力) |

## トップレベル

```json
{
  "schema_version": "0.1",
  "query": "wall_t",
  "status": "found" | "ambiguous" | "not_found",
  "matches": [ Explanation, ... ]
}
```

- `status`: found=1件 / ambiguous=複数ヒット(候補全件をmatchesに返す) / not_found=0件
- 種別横断で検索する(05-schema.md §1.1)。ambiguousは種別間の同名、またはPart内スコープのfeature/anchor同名で発生する

## Explanation

```json
{
  "kind": "param" | "material" | "rationale" | "part" | "feature" | "anchor"
        | "instance" | "mate" | "assertion" | "dim",
  "id": "wall_t",
  "part": "bracket",            // feature/anchorのみ(スコープ §1.1)。他はフィールド省略
  "definition": { ... },        // 定義本体のJSON表現(型のserde形)
  "rationale_chain": [ Rationale, ... ],  // 根拠の連鎖。現状は直接rationaleの1段
                                          // (Lesson参照の追跡は将来拡張。配列形は維持)
  "referenced_by": [ RefSite, ... ]
}
```

## RefSite(参照元)

```json
{
  "kind": "feature",            // 参照元の種別
  "id": "base",
  "part": "bracket",            // 参照元がPart内要素のとき
  "via": "z"                    // 参照箇所
}
```

`via` の語彙:
- フィールド名: `"z"`, `"d"`, `"at"`, `"binding"`, `"edges"`, `"pitch"`, `"value"`,
  `"nominal"`, `"from"`, `"to"`, `"target"`, `"datums"`, `"zone"`, `"material"`,
  `"process.thickness"`, `"a"` / `"b"`(mateの両側), `"ground"`, `"kind"`(mateの距離/角度式),
  `"check"` / `"check.path"`(assertion), `"rationale"`(rationale本体への参照)
- `"rationale:<id>"`: **同一rationaleを共有する制約**(直接参照ではなく根拠共有の連鎖。
  US-04「根拠の連鎖が辿れる」のための双方向リンク)

## 例(§9サンプル、`adc explain wall_t` 抜粋)

```json
{
  "schema_version": "0.1",
  "query": "wall_t",
  "status": "found",
  "matches": [{
    "kind": "param",
    "id": "wall_t",
    "definition": { "id": "wall_t", "value": { "Open": { "range": [3.0, 6.0], "nominal": 4.0 } }, "unit": "Mm", "rationale": "r_wall" },
    "rationale_chain": [{ "id": "r_wall", "author": { "Human": "nakag" }, "basis": "Assumption", "note": "剛性未評価のため仮置き。DFM検証後に確定", "timestamp": "2026-07-11T00:00:00Z" }],
    "referenced_by": [
      { "kind": "feature", "id": "base", "part": "bracket", "via": "z" },
      { "kind": "assertion", "id": "a_wall", "via": "rationale:r_wall" }
    ]
  }]
}
```
