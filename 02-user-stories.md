# ユーザーストーリー

7エピック / 28ストーリー。ペルソナ: **設計者**(機械設計者)、**エージェント**(LLMベースの設計AI)、**リード**(設計レビュー承認者)。

優先度: P0=MVP必須、P1=MVP望ましい、P2=Phase 2。

---

## E1: 正典スキーマとコンパイラ基盤

**US-01 (P0)** 設計者として、部品と制約をRONで宣言的に記述したい。手続き的な操作履歴ではなく「何であるべきか」を書きたい。
- 受入: `05-schema.md` の全型がserdeでround-trip(parse→serialize→parse)可能。不正なRONは行番号付きエラー

**US-02 (P0)** 設計者として、スキーマ違反を意味のあるエラーで知りたい。
- 受入: 未定義アンカー参照、単位不整合、rationale欠落、重複IDを静的検証で検出。エラーコード体系(E-SCHEMA-xxx)で返す

**US-03 (P0)** エージェントとして、正典の構造をプログラム的に照会・パッチしたい。
- 受入: `adc explain <id>` がParam/Anchor/Constraintの定義+rationale+参照元一覧をJSONで返す

**US-04 (P0)** 設計者として、全ての制約にrationaleを付けたい。根拠のない制約をコミットさせたくない。
- 受入: rationale欠落はエラー(basis: Assumptionを明示すれば通る)。`adc explain` で根拠の連鎖が辿れる

---

## E2: ジオメトリコンパイル

**US-05 (P0)** 設計者として、`design.ron` から切削部品のB-repをビルドしたい。
- 受入: `adc build` がフィーチャー語彙T1(Block/Cylinder/Hole/Pocket/Boss/Fillet/Chamfer/Pattern)をOCCTでコンパイルし、部品ごとの.brepキャッシュを生成

**US-06 (P0)** 設計者として、意味的アンカーで幾何を参照したい。再生成で参照が黙って壊れることは許容できない。
- 受入: フィーチャーの`provides`宣言とOCCT History APIによりアンカー→B-rep実体を束縛。再束縛失敗はE-ANCHOR-BINDエラー(該当アンカーIDと原因フィーチャーを含む)

**US-07 (P0)** 設計者として、STEP AP242でエクスポートして既存CAD/ビューアで確認したい。
- 受入: `adc export --step` が部品・Assy両方を出力。主要CAD(確認はFreeCAD/CAD Assistant)で開けること

**US-08 (P0)** エージェントとして、フィーチャー操作の失敗を構造化エラーで受け取りたい。
- 受入: フィレット失敗等がE-FEATURE-FAIL{feature_id, occt_error, hint}で返る。プロセス異常終了しない

**US-09 (P1)** 設計者として、板金部品(BaseFlange/Flange/Cutout/Relief)を記述したい。
- 受入: フィーチャー語彙T2(板金)がコンパイル可能。展開長はK-factor計算で質量特性に反映

**US-10 (P1)** 設計者として、ビルドの差分を見たい。
- 受入: `adc diff <rev1> <rev2>` が制約差分(追加/削除/変更+rationale)とジオメトリ差分(体積差、変更フィーチャー一覧)を出力

---

## E3: 検証ハーネス

**US-11 (P0)** 設計者として、要求をアサーションとして正典に書き、`adc check` で決定的に検証したい。
- 受入: Checkerトレイト(05-schema.md §6)を実装。同一入力で結果がバイト再現

**US-12 (P0)** 設計者として、干渉とクリアランスを検証したい。
- 受入: `clearance(a, b) >= x` がBRepExtrema/ブーリアン交差で判定。Evidenceに最近接点座標と両アンカー参照を含む

**US-13 (P0)** 設計者として、質量・重心・慣性を検証したい。
- 受入: `mass <= x` 等がBRepGProp+材料密度で判定。材料未定義はInconclusive

**US-14 (P0)** 設計者として、最小肉厚を検証したい。
- 受入: レイキャストサンプリング方式(密度パラメータ付き)。近似手法である旨と保証範囲をdocに明記。Evidenceに違反点座標

**US-15 (P0)** エージェントとして、Passでも余裕率(margin)を知りたい。ギリギリの設計と余裕のある設計を区別したい。
- 受入: 全CheckResultにmeasured/threshold/marginが含まれる。`--format=jsonl` で機械可読出力

**US-16 (P0)** エージェントとして、Fail時に修復可能な粒度のEvidenceが欲しい。
- 受入: 成功基準4(Intent)のループ実験で、Evidence文字列のみからLLMが違反箇所を特定できる情報量(アンカーID+座標+実測値)

**US-17 (P0)** 設計者として、チェック不能(Fail以外の失敗)を区別したい。
- 受入: Pass/Fail/Inconclusiveの3値。exit code 0/1/2

**US-18 (P1)** 設計者として、板金設計規則を自動検証したい。
- 受入: bend_radius>=k*t、hole_to_bend、flange_length最小値がフィーチャー定義からの代数計算で判定(ジオメトリ不要)

**US-19 (P1)** 設計者として、1D公差スタックアップを検証したい。
- 受入: アンカー列で経路を宣言し、worst-case/RSS両方の結果とmarginを返す

**US-20 (P1)** 設計者として、変更していない部品の再検証を待ちたくない。
- 受入: hash(Part定義+Checker設定)キーの結果キャッシュ。10部品Assyで1部品変更時、再計算が変更部品+関連Assyチェックのみ

**US-21 (P2)** 設計者として、3軸切削の工具アクセス性と最小コーナーRを検証したい。
- 受入: 工具軸レイ可視性近似+凹コーナー検出

---

## E4: アセンブリ

**US-22 (P0)** 設計者として、部品間の関係をアンカー参照のmateで宣言したい。
- 受入: mate語彙(coaxial/coincident/distance/angle)を逐次解決し剛体変換を確定。部品再生成後もmateが再束縛される

**US-23 (P0)** 設計者として、Assy全体の干渉マップが欲しい。
- 受入: 全ペア+指定ペアのclearance一括実行。ペアごとのmargin一覧表

**US-24 (P1)** 設計者として、mateの過拘束・解決不能を検知したい。
- 受入: 解決不能はE-MATE-UNSOLVED{mate_id, 原因}。自由度の残数を報告

---

## E5: 未確定パラメータ

**US-25 (P0)** 設計者として、決まっていない寸法を`Open(range)`のまま保持したい。
- 受入: Openパラメータ含みでbuild/check可能(公称値+区間両端の3点ビルド)

**US-26 (P1)** 設計者として、検証が通る区間を機械に絞ってほしい。
- 受入: 片端Failのとき二分探索(反復上限付き)でsuggested_rangeを返す。`adc check --narrow`

---

## E6: CLIとCI

**US-27 (P0)** 設計者として、GitHub ActionsでPRごとに全検証を回したい。
- 受入: CIテンプレート同梱。results.jsonlからmargin一覧のMarkdownテーブルを生成するサブコマンド(`adc report`)

---

## E7: エージェント統合(Phase 2)

**US-28 (P2)** エージェントとして、MCP経由でADCを操作したい。
- 受入: MCPサーバーがdesign_read / design_patch / build_and_check / evidence_query / narrow_paramを公開。パッチは検証済みの場合のみ適用可能なモード(--gated)を持つ

---

## トレーサビリティ

各ストーリーは `04-units-of-work.md` のユニットに写像される。実装PRはUS-IDを参照すること。
