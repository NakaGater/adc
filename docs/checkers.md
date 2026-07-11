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
- **measured / threshold**: スカラー距離 / min
- **margin** = (measured − min) / |min|。min≈0(|min|<1e-12)のときは measured そのもの
- **Evidence(Pass/Failとも)**: anchors=[参照ラベルa, b]、points=[最近接点1, 最近接点2]

## no_interference (M2-2)

- **判定**: 対象ペアのブーリアン積の体積 ≤ 1e-9 mm³
- **対象ペア**: scope=All → Assyの全インスタンスペア / Pairs → 明示部品ペア。
  ペアが空(単部品・Assyなし)は **Inconclusive「対象ペアなし」**(単部品内は対象外)
- **measured / threshold**: 交差体積合計 / 0
- **margin**: Fail = −max_pair(交差体積 / min(体積a, 体積b))(最悪ペアの体積比)/
  Pass = min_pair(最小距離) / 全体AABB対角(スケール正規化した余裕)
- **Evidence(Failのペアごと)**: anchors=[part_a, part_b]、points=[交差部の体積重心]、
  note=交差体積

## 未実装(M2後続)

Mass/Cog(M2-3)、WallThickness(M2-4)、DatumValidity(M2-5)、
SheetMetalRules/ToleranceStack1D(M5)、ToolAccess/MinCornerRadius(M6)
— 現状は Inconclusive{"チェッカー未実装"} を返す。
