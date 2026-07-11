# 正典IRスキーマ仕様 (v0.1)

正典ファイルは `design.ron`。Rust型と1:1で対応し、serdeでround-trip可能であること。単位はスキーマ全体でmm / g / 度に固定(単位混在はスコープ外)。

## 1. トップレベル

```rust
struct Design {
  schema_version: String,        // "0.1"
  intent: String,                // この設計の目的(自然言語)
  params: Vec<Param>,
  materials: Vec<Material>,      // id, density_g_cm3, name
  parts: Vec<Part>,
  assembly: Option<Assembly>,
  dims: Vec<Dim>,                // §7。トップレベルに置く: 公差スタックは部品横断のためAnchorPathで参照する
  geom_tols: Vec<GeomTol>,       // §7。公差は幾何の属性ではなくrationale付き制約(assertionsと同じ制約レイヤー)
  assertions: Vec<Assertion>,
  rationales: Vec<Rationale>,
}
```

`dims` / `geom_tols` は省略時空配列。Partは幾何の定義(features/anchors)に純化し、制約系(公差・アサーション)は全てDesign直下に集約する。

### 1.1 IDの名前空間規則(2026-07-11決定)

- IDは**種別**(param / part / anchor / feature / assertion / rationale / mate / instance / material / dim)**内で一意**。種別が異なれば同名を許す
- ただし **feature / anchor の一意性スコープは所属Part内**とする(部品を跨ぐアンカー参照は常に `instance.anchor` で修飾されるため。複数の部品が同名の `mount_face` アンカーを持つことは正当)
- `adc explain <id>` は全種別を横断検索し、複数ヒット時(種別間の同名、または部品間の同名feature/anchor)は候補一覧を返す
- 種別内重複は静的検証エラー E-SCHEMA-DUP(§8)

## 2. パラメータ(ADR-004)

```rust
struct Param {
  id: ParamId,                   // snake_case、Design内一意
  value: ParamValue,
  unit: Unit,                    // Mm | Deg | G
  rationale: RationaleId,        // 必須
}

enum ParamValue {
  Determined(Expr),              // リテラルまたは導出式(他paramの参照可)
  Open { range: (f64, f64), nominal: f64 },  // nominal ∈ range (E-SCHEMA-RANGE)
}
```

数値を書ける全ての場所は `Expr` を受け付ける: リテラル / `param(id)` / 四則演算。`Determined` も式を許す(導出パラメータ)。param間の循環参照は静的検証でエラー(E-SCHEMA-CYCLE)。`Open` の range / nominal は探索境界であり式を許さない(リテラル固定)。

### 2.1 評価とOpen伝播の意味論(2026-07-11決定、M0-3)

- **EvalContext** = 基底Openパラメータ(`value: Open` で宣言されたparam)への数値割当。未割当の基底Openは公称値(nominal)で評価する。`evaluate(expr, ctx) -> f64`
- 導出パラメータ(`Determined(Expr)`)は循環なし保証(E-SCHEMA-CYCLE)の下で**位相順に解決**する
- ある値が**実効的にOpen** ⇔ その式が基底Openパラメータに**推移的に依存**する
- ADR-004の3点評価の**標本軸は基底Openパラメータのみ**。導出値は軸にしない(組み合わせ爆発の防止。1変数ずつの絞り込み原則とも整合)
- 評価時の失敗(ゼロ除算・非有限値)は E-SCHEMA-EVAL(§8)。チェッカー文脈ではInconclusive相当として扱う

## 3. Rationale

```rust
struct Rationale {
  id: RationaleId,
  author: Author,                // Human(String) | Agent(String)
  basis: Basis,
  note: String,
  timestamp: String,             // ISO 8601
}

enum Basis {
  Requirement(String),           // 要求文書参照
  Standard(String),              // 規格番号 e.g. "JIS B 1176"
  Lesson(String),                // 過去知見への参照(Design Memory連携点)
  Assumption,                    // 仮決め。後で確定する義務を負う
}
```

## 4. 部品・フィーチャー・アンカー

```rust
struct Part {
  id: PartId,
  material: MaterialId,
  process: Process,              // Machining | SheetMetal { thickness: Expr, k_factor: f64 }
  features: Vec<Feature>,        // 宣言順にコンパイル
  anchors: Vec<Anchor>,          // 部品が公開する意味的参照点
}

struct Anchor {
  id: AnchorId,                  // e.g. "bearing_bore"
  kind: AnchorKind,              // Face | Axis | Edge | Point | Datum(char)
  binding: BindingExpr,          // どのフィーチャーのどの生成要素か
}
```

`BindingExpr` はフィーチャーの `provides` する名前付き要素への参照:
`feature("bore_hole").face("wall")` / `feature("base").face("top")` 等。
コンパイラはOCCT History(ADR-001)でB-rep実体に解決する。失敗は E-ANCHOR-BIND。

### 4.0 Placement(配置式)

フィーチャーの配置は全て既存の面/アンカーからの相対で宣言する。グローバル座標の直書きは最初のフィーチャー(ルート)以外で許可しない。

```rust
enum Placement {
  Origin,                                    // ルートフィーチャー専用
  On { face: BindingExpr, at: Pos2 },        // 面上の2D位置
  Offset { from: Placement, d: (Expr, Expr, Expr) },
}

enum Pos2 {
  Center,                                    // 面の重心
  Xy(Expr, Expr),                            // 面ローカル座標(原点=重心、軸=面の主軸)
  FromEdge { edge: EdgeSelector, d: Expr, along: Expr },
}
```

面ローカル座標系の定義(原点・軸の取り方)はコンパイラが決定的に導出し、docに固定する(同一入力で同一配置)。RON表記は `on(feature("base").face("top"), center())` のような関数風の糖衣を許可する。

### 4.1 フィーチャー語彙 T1 — 切削 (P0)

| フィーチャー | 主パラメータ | provides |
|---|---|---|
| `Block` | x, y, z | face: top/bottom/±x/±y |
| `Cylinder` | d, h, axis | face: side/top/bottom, axis |
| `Hole` | kind(Simple/Counterbore/Countersink/Tapped), d, depth(Through可), at | face: wall/bottom, axis, edge: rim |
| `Pocket` | profile(Rect/Circ), depth, corner_r, at | face: floor/walls |
| `Boss` | profile, height, at | face: top/side |
| `Fillet` | edges(EdgeSelector), r | — |
| `Chamfer` | edges, size | — |
| `Pattern` | of(FeatureRef), kind(Linear/Linear2D/Circular), count, pitch, **at(必須: グリッド中心)** | 各インスタンスのprovidesに `[i]` 添字(Linear2Dは `[i][j]`)。参照は `feature("bolts[0][1]")`。展開規則は docs/provides-predicates.md |

`EdgeSelector` は意味選択のみ: `edges_of(<face>)` = **束縛面の外周ワイヤのエッジのみ**(内周ループ=穴リム等は含まない — 2026-07-12確定、docs/occt-gotchas.md §5)/ `edges_between(<a>, <b>)` = 両面(グループ)の共有辺。内周を選ぶには `edges_between(wall面, 対象面)` を使う。幾何ID指定は存在しない。解決は遅延(適用フィーチャーのコンパイル時点、前送り済み束縛面の境界辺から導出 — docs/provides-predicates.md)。

### 4.2 フィーチャー語彙 T2 — 板金 (P1)

| フィーチャー | 主パラメータ | 備考 |
|---|---|---|
| `BaseFlange` | profile, thickness | 板金のルート |
| `Flange` | edge(EdgeSelector), angle, length, bend_r | 展開長はk_factorで算出 |
| `Cutout` | profile, at | フランジ面上 |
| `Relief` | kind(Rect/Round), at | 曲げ逃げ |

## 5. アセンブリ(ADR-005)

```rust
struct Assembly {
  id: String,
  instances: Vec<Instance>,      // { id, part: PartId }
  mates: Vec<Mate>,
  ground: InstanceId,            // 基準部品
}

struct Mate {
  id: MateId,
  kind: MateKind,                // Coaxial | Coincident | Distance(Expr) | Angle(Expr)
  a: AnchorPath,                 // instance.anchor — 基準側(先行部品)
  b: AnchorPath,                 // 被拘束側(aに対して位置決めされる)
  rationale: RationaleId,
}
```

mateの方向規約: `a` が基準側、`b` が被拘束側。instance間の「aに対してbを決める」有向グラフはDAGであること(循環は E-MATE-CYCLE、自己参照も同様)。逐次解決の詳細はADR-005。

- **Distance(d) の符号規約 (M3-1)**: `a` の合わせ面の外向き法線方向に `b` の合わせ面を
  `d` だけオフセットする。**負値は法線と逆方向**(例: ブラケット上面基準でシャフト
  基面を板厚側へ食い込ませ、ボアを貫通させる配置は負値で表す)。`Coincident` は
  Distance(0) と等価な平面合わせ+法線反平行。
- **配置の優先規則 (M3-1)**: グローバル配置(ルートの `Origin`/`Offset`)が許されるのは
  ground部品のみ。非ground部品はmateのみで位置決めする。**mateで被拘束かつ
  グローバル配置を持つ**場合は E-SCHEMA-REF(併用禁止)。mateを持たない部品の
  グローバル配置は許容(事実上の固定配置)。groundを被拘束側(`b`)にすることも
  E-SCHEMA-REF。

## 6. アサーションとチェッカー契約(ADR-003)

```rust
struct Assertion {
  id: AssertId,
  check: Check,
  rationale: RationaleId,
}

enum Check {
  // T1 (P0)
  Clearance { a: AnchorPath | PartRef, b: ..., min: Expr },
  NoInterference { scope: All | Pairs(Vec<(PartRef, PartRef)>) },
  Mass { part: PartRef | Assembly, max: Expr, min: Option<Expr> },
  Cog { within: BoxSpec },
  WallThickness { part: PartRef, min: Expr, sample_density: f64 },
  BoundingBox { part: PartRef, max: (Expr, Expr, Expr) },
  DatumValidity { part: PartRef },
  // T2 (P1)
  SheetMetalRules { part: PartRef },   // bend_r>=k*t, hole_to_bend, flange_min
  ToleranceStack1D {
    path: Vec<DimRef>,                 // 公差付き寸法の連鎖
    target: (f64, f64),                // 許容範囲
    method: WorstCase | Rss | Both,
  },
  // T3 (P2)
  ToolAccess { part: PartRef, tool_axis: Vec3, tool_d: Expr },
  MinCornerRadius { part: PartRef, min: Expr },
}
```

チェッカー実装契約:

```rust
trait Checker {
  fn id(&self) -> CheckerId;
  fn check(&self, m: &CompiledModel, a: &Assertion) -> CheckResult;
}

struct CheckResult {
  assert_id: AssertId,
  status: Pass | Fail | Inconclusive { reason },
  measured: Value,
  threshold: Value,
  margin: f64,          // 基本形 (measured - threshold) / |threshold|。チェッカーごとの定義はdocs/checkers.mdに文書化必須 (ADR-003)
  evidence: Vec<Evidence>,   // { anchors, points, note }
}
```

出力: `results.jsonl`(1行1結果、assert_id昇順)。決定性: 同一入力でバイト再現(浮動小数は1e-9量子化して出力)。**cost_ms等の時間情報は正準出力に含めない**(バイト再現と矛盾するため — 2026-07-12決定)。`--timings` 指定時のみ別ストリーム(stderr)に出力する。

### 6.1 Open 3点評価の表現 (M4-1, ADR-004)

基底Openパラメータを含む設計の `check` は3点評価(各軸=基底Openパラメータ1変数ずつ、
他は公称固定。標本は lo / nominal / hi)。results.jsonl は**アサーションごとに1行を維持**し:

- `status` = **全標本の最悪値**(Fail > Inconclusive > Pass)
- `samples` = 標本別サブ結果の配列。各要素は
  `{param, sample: "lo"|"nominal"|"hi", status, measured}`。
  配列順は **基底Openパラメータの宣言順 × 各軸内 lo→nominal→hi** で決定的。
  nominal標本(全軸共通の1回の評価)は**各軸のトリプルに再掲**する(軸ごとの
  3点が1箇所で読めることを優先)
- 行トップレベルの `measured` / `margin` / `evidence` = **代表標本**のもの。
  代表標本 = statusランク降順(Fail=2, Inconclusive=1, Pass=0)→margin昇順→
  samples配列順、で最初の標本(= 最悪ケースの実測が行の顔になる。
  reportのソートキーも最悪marginになる)
- 基底Openパラメータが無い設計では `samples` フィールド自体を出力しない
  (M2-1以来の出力とバイト互換)

区間保証ではなく**標本保証**である(ADR-004。単調応答を仮定できるチェッカー群が
前提。非単調チェッカー導入時に再検討)。

`--narrow` (M4-2): **片端Fail**(公称Pass・区間端の一方のみFail)のアサーションに
対し、当該Open軸を二分探索(反復上限8・中点規則・他パラメータ公称固定)し、
実行可能区間の推定を `suggested_range: <param> ∈ [lo, hi](…)` の形で当該
アサーションのevidenceに付加する。固定回数・中点規則により決定的(バイト再現)。
推定境界はPassを実測した標本値を採用する(保証側に丸める)。粒度 = 初期探索区間/2⁸。

## 7. 寸法公差・幾何公差 (P1)

```rust
struct Dim {
  id: DimId,
  from: AnchorPath, to: AnchorPath,
  nominal: Expr,
  tol: Tol,                      // Sym(±) | Asym(+u/-l) | Fit("H7") 主要はめあいテーブル内蔵
  rationale: RationaleId,
}

struct GeomTol {
  kind: Position | Flatness | Perpendicularity | Concentricity,
  target: AnchorPath,
  datums: Vec<AnchorPath>,       // kind: Datum のアンカーのみ許可
  zone: Expr,
  rationale: RationaleId,
}
```

MVPでのGeomTolは(1)静的検証(データム参照の妥当性=DatumValidity)と(2)ToleranceStack1Dへの寄与、(3)STEP AP242 PMI出力(努力目標)に使用する。実測検証はスコープ外。

## 8. エラーコード体系

| コード | 意味 |
|---|---|
| E-SCHEMA-PARSE / -REF / -UNIT / -CYCLE / -RATIONALE / -DUP / -RANGE / -EVAL | 静的検証・評価エラー(-DUP: 種別内重複ID、-RANGE: Open範囲の不整合、-EVAL: 式評価の失敗=ゼロ除算等) |
| E-ANCHOR-BIND | アンカー再束縛失敗 {anchor_id, feature_id, cause} |
| E-FEATURE-FAIL | OCCT操作失敗 {feature_id, occt_error, hint} |
| E-MATE-UNSOLVED / -CYCLE | アセンブリ解決失敗(-CYCLEは静的検証でも検出) |
| E-EXPORT | STEP出力失敗 |

全エラーはJSONで構造化出力可能であること(エージェント修復ループの入力)。静的検証エラーの構造化形式は `{code, message, span(行番号), related(関連ID一覧)}`(2026-07-11決定)。

## 9. 最小サンプル

```ron
Design(
  schema_version: "0.1",
  intent: "モーターマウントブラケット: M6x4でフレームに締結、6006ベアリング(外径φ55)の座面を保持",
  params: [
    Param(id: "wall_t", value: Open(range: (2.0, 6.0), nominal: 4.0), unit: Mm, rationale: "r_wall"),
    Param(id: "bore_d", value: Determined(55.0), unit: Mm, rationale: "r_bore"),
    Param(id: "bolt_d", value: Determined(6.6), unit: Mm, rationale: "r_bolt"),
  ],
  materials: [Material(id: "a5052", density_g_cm3: 2.68, name: "A5052")],
  parts: [
    Part(
      id: "bracket", material: "a5052", process: Machining,
      features: [
        Block(id: "base", x: 80.0, y: 64.0, z: param("wall_t")),
        Hole(id: "bore", kind: Simple, d: param("bore_d"), depth: Through,
             at: on(feature("base").face("top"), center())),
        Pattern(id: "bolts", of: Hole(kind: Simple, d: param("bolt_d"), depth: Through),
                kind: Linear2D, count: (2,2), pitch: (64.0, 44.0),
                at: on(feature("base").face("top"), center())),
        Fillet(id: "f1", edges: edges_of(feature("base").face("top")), r: 2.0),
      ],
      anchors: [
        Anchor(id: "bearing_bore", kind: Face, binding: feature("bore").face("wall")),
        Anchor(id: "mount_face",  kind: Face, binding: feature("base").face("bottom")),
        Anchor(id: "datum_a",     kind: Datum('A'), binding: feature("base").face("bottom")),
      ],
    ),
  ],
  assertions: [
    Assertion(id: "a_mass", check: Mass(part: "bracket", max: 250.0), rationale: "r_mass"),
    Assertion(id: "a_wall", check: WallThickness(part: "bracket", min: 2.5, sample_density: 1.0), rationale: "r_wall"),
  ],
  rationales: [
    Rationale(id: "r_wall", author: Human("nakag"), basis: Assumption,
              note: "剛性未評価のため仮置き。DFM検証後に確定。板幅64はφ55ボアとの面内リガメント(64-55)/2=4.5mmを確保する(最小肉厚2.5に対し余裕)", timestamp: "2026-07-11T00:00:00Z"),
    Rationale(id: "r_bore", author: Human("nakag"), basis: Standard("JIS B 1521 深溝玉軸受 6006 外径φ55"), note: "座面はめあいH7を想定", timestamp: "2026-07-11T00:00:00Z"),
    Rationale(id: "r_bolt", author: Human("nakag"), basis: Standard("JIS B 1180 六角ボルト M6"),
              note: "六角ボルト+平ワッシャ想定のためφ6.6ばか穴(座ぐり不要)", timestamp: "2026-07-12T00:00:00Z"),
    Rationale(id: "r_mass", author: Human("nakag"), basis: Requirement("REQ-012 質量目標"), note: "", timestamp: "2026-07-11T00:00:00Z"),
  ],
)
```
