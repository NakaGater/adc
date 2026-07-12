# ADC エラーカタログ (v0.1) — M0-5

05-schema.md §8 のコード体系の正式カタログ。全エラーは構造化形式
`{code, message, span, related}` でJSONシリアライズ可能(エージェント修復ループの入力)。

```json
{
  "code": "E-SCHEMA-REF",
  "message": "part \"p1\" 内に未定義のフィーチャー \"ghost\" への参照があります",
  "span": { "line": 7, "column": 62 },   // 解決不能時はフィールド省略
  "related": ["ghost", "p1"]             // 関連ID(欠落/重複IDが先頭)
}
```

- 静的検証(`validate_design`)は**全エラーを1回で収集**する(最初のエラーで停止しない)
- `span` は元テキスト上の該当ID出現位置(参照エラー=引用付きIDの初出、
  重複=`id: "x"` 定義パターンのn番目)。ヒューリスティックであり、複数出現時は近似
- `related` の先頭は問題のID、以降は文脈ID(所属part、参照元assertion等)

## M0実装済み(adc-schema)

| コード | 検出内容 | 検出例 | 検出フェーズ |
|---|---|---|---|
| E-SCHEMA-PARSE | RON構文・型エラー(行番号付き)。未知/欠落フィールド含む | `intent "コロン欠落"` / `Param(idd: ...)` | parse |
| E-SCHEMA-REF | 未定義参照: 式内param / Part内feature(binding) / anchor(`instance.anchor`) / material / part / instance / dim(公差スタック経路。**M5-3追加**: 経路の連結性 dim[i].to == dim[i+1].from も検査 — 05-schema.md §7.1) / GeomTol.datumsの非Datumアンカー。**M3追加**: 非ground部品のグローバル配置とmate位置決めの併用禁止・groundの被拘束側(b)指定禁止(05-schema.md §5) | `z: param("nope")` / `feature("ghost").face(...)` / `datums: ["i1.top"]`(topがFace) | validate |
| E-SCHEMA-RATIONALE | 未定義rationale参照(param/assertion/mate/dim/geom_tol) | `rationale: "r_missing"` | validate |
| E-SCHEMA-DUP | 種別内重複ID(§1.1。feature/anchorは所属Part内スコープ) | param `wall_t` ×2 / 同一Part内feature `base` ×2 | validate |
| E-SCHEMA-CYCLE | param間の循環参照(Determined式の依存グラフ) | `a: Determined(param("b"))` + `b: Determined(param("a") + 1.0)` | validate / eval |
| E-SCHEMA-RANGE | Open範囲の不整合 | `Open(range: (3.0, 6.0), nominal: 8.0)` / `range: (6.0, 3.0)` | validate |
| E-SCHEMA-EVAL | 式評価の失敗: ゼロ除算・非有限値・EvalContextの不正割当(非Openパラメータへの割当等)。チェッカー文脈では**Inconclusive相当**として扱う(ADR-003) | `1.0 / (param("a") - 2.0)` で a=2 | eval |

## M1以降で実装(コード予約済み)

| コード | 意味 | ユニット |
|---|---|---|
| E-SCHEMA-UNIT | 単位不整合(単位検証の導入時) | M2以降 |
| E-ANCHOR-BIND | アンカー再束縛失敗 {anchor_id, feature_id(原因フィーチャー), cause, hint}。causeは Deleted / Untracked / Ambiguous の3値で型固定。Ambiguousは修復ヒント必須。判定規則は docs/provides-predicates.md、実測記録は docs/occt-gotchas.md | **済 (M1-5)** |
| E-FEATURE-FAIL | OCCT操作失敗 {feature_id, occt_error, hint}。フィレット/面取り(Try+IsDone)・ブーリアン(TryNew)はOCCT例外をruntime_error変換して捕捉(abortゼロ)。プリミティブ寸法はFFI前の正値検証で遮断 | **済 (M1-7)** |
| E-MATE-UNSOLVED | アセンブリ逐次解決の失敗 {mate_id, 原因}。残差>1e-6(先行mateとの矛盾)、参照アンカーの束縛失敗(E-ANCHOR-BINDのAssy経由伝播)、groundから到達不能等。チェッカー文脈ではInconclusive相当 | **済 (M3-2)** |
| E-MATE-CYCLE | mateグラフの循環・自己参照(a=基準側→b=被拘束側の有向グラフ)。静的検証(M0-2)で検出 | **済** |
| E-EXPORT | STEP出力失敗(現状はkernel/CLIの構造化メッセージ+exit 2。コード付きJSONへの正式化はM2-1のresults契約と同時) | 実質済 (M1-6) |

## exit codeとの対応 (07-cli.md)

check系: 0=全Pass / 1=Fail≥1 / 2=Inconclusive≥1またはE-*。
E-SCHEMA-EVALはInconclusive相当(exit 2系)。build/explain等は 0=成功 / 2=E-*
(explainは 1=not_found・ambiguous を追加で持つ — docs/explain-schema.md)。
