# M5-1 設計メモ: 板金フィーチャーのソリッド構築方式(レビュー用)

2026-07-12。M5-1着手前のレビュー対象(承認後に実装、provides表は
docs/provides-predicates.md へ転記する)。

## 1. フランジのソリッド構築方式 — 3案と推奨

| 案 | 方式 | 新規FFI | 評価 |
|---|---|---|---|
| A | スイープ(BRepOffsetAPI_MakePipe)で断面を曲げ経路に沿わせる | 必要 | 一般性は高いがT2語彙(直線曲げのみ)には過剰。History挙動が未実測の新API |
| B | **プリミティブ合成**: 曲げ部=円筒環セグメント(外筒−内筒を角度ウェッジで切出し)、平坦部=プリズム、fuseで接合 | **不要** | 曲げは常に「円筒面+平面」で正確に表現できる。既存のTry網・History前送りの実績範囲に閉じる |
| C | 展開形状を作って曲げ変換 | 非現実的 | OCCTに非剛体変形はない |

**推奨: 案B**。理由: (1) T2のFlangeは angle+bend_r+length の直線曲げのみで、
円筒環セグメント+平板の合成で**厳密に**表現できる(近似ではない)。
(2) make_cylinder_dir / make_prism / ブーリアン / transformed / History前送りは
M1〜M3で実測済みのAPI面であり、新規のOCCT既知の穴を持ち込まない。

## 2. 各フィーチャーの構築(案B詳細)

- **BaseFlange { profile, at }**: profile(Rect等)を `process: SheetMetal.thickness`
  で押出し(既存プリズム機構)。板金Partのルート専用(静的検証)。
- **Flange { edge, angle, length, bend_r }**:
  1. edge解決は既存EdgeSelector(遅延解決)。**直線エッジ1本**に限定 —
     複数本・円弧エッジは E-FEATURE-FAIL {hint: エッジを特定せよ}
  2. 曲げ部: エッジ方向を軸とする円筒環セグメント。内半径 bend_r、
     外半径 bend_r+t、角度 angle、軸長=エッジ長。「外筒 − 内筒」に
     角度ウェッジ(プリズム)を交差して切出す
  3. 平坦部: 曲げ終端の接平面上に length×エッジ長×t のブロック
  4. base ∪ 曲げ部 ∪ 平坦部 を fuse(各プリミティブのHistoryでprovides前送り)
  - 接線接続のfuseはgotcha #4(同軸円柱の面分割)類の分割リスクがある。
    束縛は各ソースプリミティブのHistoryで写すため分割自体では壊れない想定だが、
    **実測してocct-gotchas.mdに記録**する(M1以来の運用)
- **Cutout { profile, at }**: フランジ/ベース面上の貫通ポケット(既存Pocket機構の
  再利用、深さ=板厚方向Through)
- **Relief { kind: Rect|Round, at }**: 曲げ根元の逃げ切欠き。Cutoutの特殊形として
  実装(寸法は明示指定、根元配置は at 必須)

## 3. provides案(承認後 provides-predicates.md へ)

| feature | provides |
|---|---|
| BaseFlange | face: top / bottom / ±x / ±y(Blockと同一規約) |
| Flange | face: **bend_inner / bend_outer**(曲げの円筒面)、**inner / outer**(平坦部の内外面)、**tip**(先端小口) |
| Cutout | face: wall(Pocket踏襲) |
| Relief | —(参照点にしない) |

Flange平坦部の inner/outer は板の内側(曲げ中心側)/外側。
アンカーは全てface束縛でM3のmate幾何(Plane抽出)にそのまま乗る。

## 4. 展開長とK-factor

- 曲げ補正 BA = angle_rad × (bend_r + k_factor × t)(k_factorは
  `process: SheetMetal` から)
- 展開長 L_flat = ベース長 + Σ(フランジ平坦長 length) + Σ BA
- テスト固定値(例): t=2, bend_r=3, k=0.44, 90° →
  BA = (π/2)×(3+0.88) = 6.094689747964198
- 展開長は部品の派生量として保持し、SheetMetalRules(M5-2)と
  explain / report で参照可能にする

## 5. 質量特性の扱い(要判断 — 推奨あり)

曲げ後ソリッドの体積は厳密に angle×t×(bend_r+t/2)×幅 であり、これは
「中立面がt/2」の場合の展開体積に一致します。K-factor(≠0.5)は**中立繊維の
長さ補正**であって体積補正ではない(実際の曲げでも材料体積は保存される)ため:

- **(a) 推奨**: Mass/Cog は従来どおり**OCCTソリッドの体積**で計算(物理的に正)。
  K-factor反映の**展開長**は§4の派生量として提供し、ブランク寸法・DFM情報
  として使う(「展開長の計算式と検証値をテストで固定」はこの派生量で満たす)
- (b) 代替: SheetMetal部品のMassを展開板(L_flat×幅×t)の体積で計上 —
  K-factorが質量に影響する非物理的な結果になるため非推奨

## 6. E-FEATURE-FAIL条件(新規分)

- 曲げエッジが直線でない/複数本ヒット → hint「edges_between等で1本に特定」
- bend_r ≤ 0、length ≤ 0 → 既存の正値検証(e_pos)で遮断
- 幾何破綻(フランジ同士の自己交差等)はブーリアン例外→既存Try網で捕捉

## 7. Angle mate E2E回収(バックログ①)

45°の Flange を持つ板金ブラケット+Block部品のAssyフィクスチャ:
`Coaxial(axis_lock)` + `Angle(45°)` で Flange の `inner` 面(傾斜平面)を
被拘束側の面に合わせる。mate幾何は既存の束縛表→Plane抽出に乗るため、
ソルバ側の変更は不要の見込み。M3で単体テストのみだったAngle補正の
数学がE2Eで固定される。
