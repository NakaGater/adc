# provides 初期同定述語 (v0.1) — provides契約の実体

**フィーチャーを追加・変更するときは本書の更新が必須**(2026-07-12設計レビュー決定(b))。

## 原則

1. OCCT History は「操作間」の写像しか与えない。**フィーチャー生成直後の面の同定**
   (どれがtopか)は、本書の**決定的な幾何述語**で一回だけ行う
2. 述語は**フィーチャー局所フレーム**(生成時の座標系。配置変換の適用前)で評価する。
   以後の全操作は History 前送りのみで運び、幾何述語による再同定は行わない
3. 束縛は1対1のみ(決定(a)・案1)。後続操作で分割された単一面は
   E-ANCHOR-BIND {cause: Ambiguous} となり、修復ヒントを返す。
   **集合provides**(walls等)は例外で、分割・消滅を自然に吸収する(集合が伸縮する)
4. 面の同定に部分形状インデックス(Face#n)や列挙順を用いてはならない
   (OCCTの列挙順は仕様保証がなく再生成安定性の根拠にできない)

局所フレーム: 原点=フィーチャー基準点、+Z=フィーチャー主軸。配置(§4.0)は
局所フレームからワールドへの剛体変換として適用される。

## T1 フィーチャー述語表

εは幾何許容差(1e-9、法線内積判定は |n·axis| > 1-ε)。

### Block (x, y, z)

局所フレーム: 原点=直方体の最小コーナー、軸=辺方向。

| provides | 述語 |
|---|---|
| face: top | 平面かつ法線=+Z |
| face: bottom | 平面かつ法線=−Z |
| face: +x / -x | 平面かつ法線=±X |
| face: +y / -y | 平面かつ法線=±Y |

### Cylinder (d, h, axis)

局所フレーム: 原点=底面中心、+Z=軸方向。

| provides | 述語 |
|---|---|
| face: side | 円筒面(非平面)ちょうど1面 |
| face: top | 平面かつ法線=+Z |
| face: bottom | 平面かつ法線=−Z |
| axis | 底面中心を通る+Z方向の直線(側面の回転軸) |

### Hole (kind, d, depth, at)

工具ソリッド(Simple=円柱、Counterbore=大小2円柱の合併)を配置面法線の
**逆方向(−n = 掘り込み方向)**に生成し、カットする。述語は**工具側**で評価し、
カットのHistoryで結果面に写す。

| provides | 述語(工具側) |
|---|---|
| face: wall | 径dの円筒側面の像(Counterboreでは小径側) |
| face: bottom | Blind時のみ: 工具遠端(掘り込み方向側)平面の像 |
| axis | 工具円柱の軸 |
| edge: rim | wall面の円エッジのうち、工具軸に沿って**配置面に最も近い側**のもの(開口円) |

Through の工具は板厚を両側に貫通させる(結果に工具端面の像は残らない)。
Counterbore の座ぐり部(cb_d/cb_depth)、Countersink の皿もみ円錐面は現状providesに
含めない(§4.1の表どおり。必要になれば cb_wall / cs_face 等を本書とともに追加する)。
Counterbore/Countersink の小径工具は座ぐり底/皿もみ底から**0.5mmだけ上側工具に
食い込ませて**構成する(全通しにすると工具フューズで小径側面が2分割され
Ambiguousになるため)。Tapped はねじ山を形状モデル化せず Simple と同一幾何(MVP)。

### Pocket (profile, depth, corner_r, at)

工具プリズム(profile断面を−n方向にdepth押し出し)でカット。

| provides | 述語(工具側) |
|---|---|
| face: floor | 工具遠端(掘り込み方向側)平面の像 |
| face: walls | **集合**: 工具側面(押し出しの側面)の像の集合(Rect=4面+corner_r>0で丸め面、Circ=1面) |

### Boss (profile, height, at)

工具プリズム(profile断面を+n方向にheight押し出し)をフューズ。

| provides | 述語(工具側) |
|---|---|
| face: top | 工具遠端(+n側)平面の像 |
| face: side | **集合**: 工具側面の像の集合 |

### Fillet / Chamfer (M1-3)

providesなし(§4.1)。ただし既存providesの前送りに BRepFilletAPI 系の
Modified/Generated を用いる。述語の追加が必要になった場合は本書を更新する。

### Pattern (M1-4実装済み)

各インスタンスのprovidesは**添字付きフィーチャーID** `<pattern_id>[i]`
(Linear2Dは `<pattern_id>[i][j]`)で参照する:
`feature("bolts[0][1]").face("wall")`。静的検証は添字の範囲をcountで検査する。

- **展開規則**: グリッドは `at`(必須)のフレーム原点を中心に**センタリング**される。
  オフセット = (i − (n−1)/2)·pitch。Linear/Linear2Dの添字はフレームx/y軸方向に昇順
- **Circular**: `axis`(axis要素のprovides参照)まわりに基準配置から
  **反時計回り(右手系)**に pitch度 × k 回転。添字は k=0..count−1
- Pattern内側フィーチャーのid自体は参照可能名ではない(添字IDのみ)。
  内側はHole/Pocket/Bossに対応
- **未決**: §9サンプルのPatternは `at` を持たない。既定規則(ルート天面中心等)を
  導入するかサンプルを修正するか、M1-6までに要決定

## E-ANCHOR-BIND との対応

- 前送りで対応が消滅(IsRemoved) → cause: **Deleted**
- Modified/Generatedとも空かつ結果に元形状が残存しない → cause: **Untracked**
- 単一面providesが複数面に対応 → cause: **Ambiguous**(修復ヒント必須)

## エッジ解決の方針(2026-07-12決定、M1-3)

EdgeSelector(`edges_of` / `edges_between`)は**遅延解決**とする:
Fillet / Chamfer / `from_edge` のコンパイル時点で、**前送り済みの束縛面の境界辺**
から導出する。永続的なエッジ台帳は作らない(エッジのHistory追跡は面より弱いため、
辺を長期参照で運ばない)。

- `edges_of(<binding>)` = 束縛面(集合可)の境界辺全部。**外周と内周(穴のリム等)を
  区別しない**ことに注意 — 内周を除きたい場合は `edges_between` でより特定的に選ぶ
- `edges_between(<a>, <b>)` = 両面グループの共有辺(TopoDS IsSame、向きの違いは無視)。
  集合provides(boss.side等)も面グループとして受け付ける
- 例外: Holeの `rim` は§4.1のprovides要素なので台帳に載り、History前送りされる
