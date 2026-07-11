# チェッカーカタログとmargin定義 (v0.1) — ADR-003の文書化義務

各チェッカーのmargin定義・Evidence・Inconclusive条件の正典。
チェッカーを追加したら本書の更新が必須。共通契約は 05-schema.md §6
(cost_msは正準出力から除外 — 2026-07-12決定。`--timings`でstderrへ)。

共通事項:
- 出力の浮動小数は**1e-9量子化**(バイト再現の担保)
- results.jsonl は assert_id 昇順
- 部品のコンパイル失敗・式評価失敗(E-SCHEMA-EVAL)・対象解決失敗は **Inconclusive{reason}**
- **インスタンスはM3(mate解決)まで恒等配置**。2体フィクスチャはルートの
  グローバル配置(Offset)で位置決めする(docs/placement-frames.md)

## bounding_box (M2-1)

- **判定**: 部品AABBの各軸サイズ ≤ max(x,y,z)
- **measured / threshold**: 3成分 [dx,dy,dz] / [mx,my,mz]
- **margin** = min_axis (limit − size) / |limit|(最も厳しい軸の相対余裕)
- **Evidence(Fail)**: anchors=[part]、points=[bbox min, bbox max]、超過軸のnote

## clearance (M2-2)

- **判定**: BRepExtrema_DistShapeShape の最小距離 ≥ min
- **対象**: PartRef(ソリッド全体)または Face束縛アンカー(`instance.anchor`)。
  Edge/Axis束縛のアンカーは現状Inconclusive(エッジ由来の帰属はgotcha #2の
  実在検証を経た面providesに限定する方針)
- **measured / threshold**: スカラー距離 / min。**交差時(最小距離≈0)のmeasuredは
  負の貫入指標に統一**(2026-07-12決定): 大きさ = 所属ソリッド同士の交差体積の
  立方根(等価キューブ辺長)。厳密な最小分離距離(MTD)ではない近似指標。
  接触のみ(交差体積≈0)は 0
- **margin** = (measured − min) / |min|。min≈0(|min|<1e-12)のときは measured そのもの
- **Evidence(Pass/Failとも)**: anchors=[参照ラベルa, b]、points=[最近接点1, 最近接点2]。
  貫入時はnoteに「貫入」と貫入指標を明記

## no_interference (M2-2 / M3-3で干渉マップ化)

- **判定**: 対象ペアのブーリアン積の体積 ≤ 1e-9 mm³
- **対象ペア**: scope=All → Assyの全インスタンスペア / Pairs → 明示インスタンスペア。
  ペアが空(単部品・Assyなし)は **Inconclusive「対象ペアなし」**(単部品内は対象外)。
  ペア列挙は **instance id昇順に正準化**(宣言順非依存 — M3受入で固定)
- **measured / threshold**: 交差体積合計 / 0
- **margin**: Fail = −max_pair(交差体積 / min(体積a, 体積b))(最悪ペアの体積比)/
  Pass = min_pair(最小距離) / 全体AABB対角(スケール正規化した余裕)
- **Evidence(干渉マップ、M3-3)**: **全ペアを一覧で載せる**。ラベルは
  anchors=[instance_a, instance_b]。交差ペアは points=[交差部の体積重心]、
  note=「交差体積 X mm^3」。非交差ペアは note=「最小距離 X(非干渉)」で
  margin相当の余裕を個別に読める(修復ループがどのペアが危ないかを特定する材料)

## mass (M2-3)

- **判定**: 質量 [g] = 体積 [mm³] × 密度 [g/cm³] ÷ 1000 が min ≤ m ≤ max
- **measured / threshold**: 質量 / max
- **margin** = min( (max−m)/|max|, (m−min)/|min| )(minがなければ前者のみ)
- **Inconclusive**: 材料(密度)未定義
- **Evidence(Fail)**: 違反した境界(上限/下限)と実測質量・密度

## cog (M2-3)

- **対象**: Assyがあれば全インスタンス、なければ全部品の質量加重合成重心(恒等配置)
- **判定**: 重心 ∈ BoxSpec(各軸 min ≤ c ≤ max)
- **measured**: 重心 [x,y,z] / **threshold**: boxのmax角(min角はEvidence noteに)
- **margin** = min軸 (半幅 − |c − box中心|)/半幅(中心ど真ん中=1、境界=0、外=負)
- **Evidence**: 実測重心座標+逸脱軸の列挙

## wall_thickness (M2-4)

- **sample_density の意味論**: **面上の格子密度 [点/mm²]**。格子間隔 = 1/√density。
  sample_densityは結果キャッシュキーに含まれる(ADR-003)
- **手法**: 各**平面フェイス**上に決定的格子を張り、面法線の逆向きにレイキャスト
  (面の0.1mm外側から照射、最初のヒットが当該サンプル点であるレイのみ採用)。
  最初の材料通過長 = その点の肉厚
- **壁の定義=対向面条件 (M4-1で追加)**: 出口面の法線が入射面の法線と**5°以内で
  (反)平行**のサンプルのみ「壁」として計上する。フィレット/面取りのロールオーバーへ
  抜けるチョード(角の丸め落とし)を壁厚違反として誤検出しないため。
  微小な抜き勾配(≤5°)の壁は従来どおり検出する。5°超で交わる楔状の薄肉は
  見逃しうる(下記の一方向保証の範囲内)
- **シームの二重ヒット併合 (M4-1で追加)**: レイが面の継ぎ目(フィレット接線シーム等)を
  掠めると同一点で二重ヒットし厚み0の偽違反になるため、レイ上1e-6以内の連続ヒットを
  1点に併合する(実§9のフィレット外周で実測した既知の穴)
- **一方向保証(近似手法の保証範囲)**: **検出した違反は真。未検出は薄肉なしを
  保証しない**(格子間隔より小さい薄肉形状・平面以外のフェイス(円筒壁の径方向等)は
  見逃しうる = false negativeあり)。この注記を出力(Evidence note)にも常に含める
- **M2-4時点の制約**: サンプリング対象は平面フェイスのみ。曲面フェイスの
  サンプリングは将来拡張(本書を更新してから実装)
- **measured / threshold**: 実測最小厚 / min
- **margin** = (実測最小厚 − min)/|min|
- **Evidence**: 最悪違反点の座標+実測厚+面法線、違反サンプル数/全サンプル数

## datum_validity (M2-5)

- **判定**: 部品のDatumアンカーが (1)存在しFaceに束縛 (2)平面 (3)相互に直交
  (|cos| < 1e-6)。幾何公差の実測検証はスコープ外(§7の線を維持)
- **measured / threshold**: max|cos(法線間)| / 0
- **margin** = 1 − max|cos|(データム1個なら1.0)
- **Inconclusive**: Datumアンカーなし
- **Evidence(Fail)**: 違反データムの組と理由(非平面/非直交)

## bounding_box 補足 (M2-5正式化)

OCCTのBnd_Boxは既定でgap(1e-7)を含む — kernelで除去済みのため、
量子化(1e-9)後の実測寸法は正確な設計寸法に一致する(occt-gotchas.md)。

## アセンブリ解決との関係 (M3)

- **mate逐次解決 (M3-1)**: ground を根とする mate グラフ(a→b 有向)を Kahn 位相ソート
  (同順位は instance id 昇順タイブレーク)し、各インスタンスの mate 列を宣言順に
  逐次適用 → 最後に全 mate の残差を検証(> 1e-6 で E-MATE-UNSOLVED
  {mate_id, 原因}、逐次適用で先行mateが壊れた場合はそのmate idを報告)
- **チェッカーへの影響**: Assy 解決に失敗した設計では、配置に依存するチェッカー
  (clearance のインスタンスアンカー参照 / no_interference / cog)は
  **Inconclusive{E-MATE-UNSOLVED...}**。部品コンパイル失敗(E-ANCHOR-BIND 等)も
  Assy 経由で同様に伝播する(M3-4)
- **アンカーの配置**: `instance.anchor` 参照は、部品ローカルの束縛表(Face index)を
  解決済み剛体変換で配置した面として測る。部品を変更して再 build しても、
  アンカーが生きていれば mate は再束縛される(M3-4受入)
- **残自由度レポート (M3-2)**: mate種別ごとの近似計上
  (Coaxial=−4 / Coincident=−3 / Distance=−3 / Angle=−1)で各インスタンスの
  残DOFを報告する。**未拘束・部分拘束は正常**(構想段階のモデルを許容)で
  エラーにせず、`--timings` 同様 stderr 側に `dof\t{instance}\t残N\t内訳` で出す。
  厳密な瞬間自由度解析(ヤコビアン階数)ではない近似である旨は出力noteにも明記

## キャッシュとの関係 (M2-6)

- 部品キャッシュ: hash(ADCバージョン+Part正準形+参照param解決値) →
  .brep+束縛表を併存保存(docs/binding-cache.md)。ヒット時も束縛表経由で
  アンカー参照チェッカーが動作する
- 結果キャッシュ: hash(ADCバージョン+Assertion正準形+依存部品キー列)。
  **Checker設定(sample_density等)はAssertion正準形に含まれる**ため
  設定変更は自動的にミスになる。Inconclusiveはキャッシュしない
- `--no-cache` とキャッシュヒットの結果はバイト同一(受入テストで固定)

## 未実装(M5以降)

SheetMetalRules/ToleranceStack1D(M5)、ToolAccess/MinCornerRadius(M6)
— 現状は Inconclusive{"チェッカー未実装"} を返す。
