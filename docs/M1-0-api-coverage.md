# M1-0: opencascade-rs API被覆調査

- 調査日: 2026-07-11
- 調査対象: [bschwind/opencascade-rs](https://github.com/bschwind/opencascade-rs) `main` ブランチ(最終push 2026-06-27、浅clone取得のスナップショット)
- 関連ADR: ADR-002(カーネル=OCCT 7.x、バインディングは opencascade-rs を土台に fork/vendor)

---

## 1. サマリ

**結論: opencascade-rs 土台採用は妥当。** ADCのM1〜M3が要求する12機能領域のうち、**7領域はsafe APIまで整備済み(◎)、3領域はFFI露出済みで薄いラッパー追加のみ(△)、完全に不在なのは2領域(✕)**。リポジトリは2026年6月まで活発にメンテされ、外部コントリビュータのPRが継続的にマージされており、fork+上流PR戦略と相性が良い。cxxブリッジは2026年5月にドメイン別の43ファイルに分割済み(#199)で、追加実装の置き場所が明確。

**最大のギャップは、ADCの意味的アンカー再束縛の要である履歴追跡(BRepTools_History / Modified / Generated / IsRemoved)が完全に不在なこと**。次いで最小距離計算(BRepExtrema_DistShapeShape)も不在。STEP出力はAP203固定相当(スキーマ設定不可)で、AP242指定にはInterface_Staticの露出が必要。PMI出力(XDE/STEPCAFControl)は全く無く、これだけはL級。

**FFI追加の概算合計工数: PMIを除きおよそ4〜6人日**(履歴追跡 M=2〜3日、DistShapeShape S〜M=0.5〜1日、AP242スキーマ設定 S、gp_Trsf合成 S、体積プロパティsafe化 S)。PMI出力(L=3〜5日超)はM3以降に先送り可能。既存ブリッジに `list_to_vector`(NCollection_List→Vec変換)や `Handle_*` パターンなど再利用可能なインフラが揃っており、追加実装の限界コストは低い。

## 2. 被覆表

判定基準: ◎=safe APIあり / △=FFI露出のみ・部分的 / ✕=なし。根拠のファイルパスはすべて opencascade-rs リポジトリ内の相対パス(2026-06-27時点main)。

| 必要機能 | OCCT API | 状況 | 根拠(ファイル/シンボル) | 不足時のFFI追加工数と理由 |
|---|---|---|---|---|
| プリミティブ(box, cylinder, sphere, cone, torus) | BRepPrimAPI_MakeBox / MakeCylinder / MakeSphere / MakeCone / MakeTorus | ◎ | sys: `src/b_rep_prim_api.rs`(7クラス全露出)。safe: `opencascade/src/primitives/shape.rs` の `box_with_dimensions` / `box_centered` / `cube` / `cylinder*`(4種) / `sphere` / `cone` / `torus`(ビルダーパターン) | 不足なし |
| 押し出し・回転体 | BRepPrimAPI_MakePrism / MakeRevol | ◎ | sys: 同上。safe: `primitives/face.rs` の `Face::extrude`(L66) / `Face::revolve`(L127、角度指定可) / `extrude_to_face` / `subtractive_extrude`、`Surface::extrude/revolve`(L450/L466) | 不足なし |
| ブーリアン fuse/cut/common | BRepAlgoAPI_Fuse / Cut / Common(+Section) | ◎ | sys: `src/b_rep_algo_api.rs`(4クラス+BuilderAlgo基底、`SectionEdges`、Fuseの`SetGlue`)。safe: `shape.rs` の `union`(L692) / `subtract`(L534) / `intersect`(L710) → `BooleanShape`(結果形状+`new_edges`) | 不足なし。※新規エッジは取得できるがModified/History系は下記✕参照 |
| フィレット/面取り(エッジ単位) | BRepFilletAPI_MakeFillet / MakeChamfer | ◎ | sys: `src/b_rep_fillet_api.rs`(`add_edge`, `variable_add_edge`、MakeFillet2dも)。safe: `shape.rs` の `fillet_edge`(L451) / `fillet_edges` / `variable_fillet_edge(s)` / `chamfer_edge`(L465) / `chamfer_edges`、`BooleanShape::fillet_new_edges` | 不足なし(片側距離指定の非対称面取り等が要るなら S) |
| 最小距離+最近接点 | BRepExtrema_DistShapeShape | **✕** | リポジトリ全体grepで `BRepExtrema` / `DistShapeShape` に一致0件。`b_rep_extrema.rs` に相当するブリッジファイル自体が無い | **S〜M(0.5〜1日)**。新規ブリッジファイル1本(`Perform`/`Value`/`NbSolution`/`PointOnShape1/2`)。既存の`construct_unique`+hxxヘルパーの定型パターンで書ける。支持要素(SupportOnShape/SupportTypeShape)まで出すならM寄り |
| 質量特性(体積・重心・慣性) | BRepGProp::VolumeProperties + GProp_GProps | **△** | sys: `src/b_rep_g_prop.rs` に `VolumeProperties` / `SurfaceProperties` / `LinearProperties`、`src/g_prop.rs` に `Mass` / `GProp_GProps_CentreOfMass` / `MomentOfInertia(軸指定)` / `StaticMoments` / `RadiusOfGyration`。safe層はFaceの `center_of_mass` / 面積(`face.rs` L278, L394)のみで **Shape::volume() 相当が無い** | **S(0.5日)**。FFIは揃っており safe ラッパーのみ。慣性テンソル全成分(`MatrixOfInertia`→gp_Mat)が要る場合も既存gp.rsパターンでS |
| **履歴追跡(最重要)** | BRepTools_History(Modified / Generated / IsRemoved) | **✕** | リポジトリ全体grepで `BRepTools_History` に一致0件。`src/b_rep_tools.rs` はouter_wire/BRep入出力のみ(615B)。Builder系の履歴メソッドも `BRepAlgoAPI_Cut::Generated`(`b_rep_algo_api.rs` L37)が唯一の例外で、`Modified()` / `IsDeleted()` / `History()` は全クラスで未露出 | **M(2〜3日)**。(a) `BRepAlgoAPI_BuilderAlgo::History()`(Handle(BRepTools_History)返却)+ BRepTools_Historyの `Modified/Generated/IsRemoved` を新規ブリッジで露出、(b) フィレット/面取り等 BRepBuilderAPI_MakeShape 系には `Modified/Generated/IsDeleted` を直接露出、の2系統が必要。Handle型の露出パターン(`Handle_TopTools_HSequenceOfShape` 等)と `list_to_vector`(`include/bindings_common.hxx` L16、`topo_ds.rs` L111)が既存なので定型作業だが、対象オペレーション数が多く配線量で1〜3日。履歴の連結(`BRepTools_History::Merge`)まで含めても M 内に収まる見込み |
| STEP出力(AP242指定) | STEPControl_Writer + Interface_Static("write.step.schema") | **△** | sys: `src/step_control.rs` + `include/step_control.hxx`(`STEPControl_Writer_new` / `transfer_shape`=STEPControl_AsIs固定 / `write_step`)。safe: `shape.rs` の `write_step`(L570) / `write_all_step`(L574、複数形状、2026-06 #226)。**`Interface_Static` はgrep一致0件 → スキーマ指定(AP242)不可、デフォルトスキーマ(AP214系)固定** | **S(0.5日)**。`Interface_Static::SetCVal/SetIVal/CVal` の静的関数露出のみ("write.step.schema"="AP242DIS" 等)。※値の正確な文字列はOCCT 7.8ドキュメントで実装時に要確認 |
| STEP PMI出力 | STEPCAFControl_Writer + XCAF(GD&T) | **✕** | `STEPCAFControl` / `XCAFDoc` はgrep一致0件。ただしリンクライブラリには TKDESTEP / TKXCAF / TKCAF / TKLCAF が既に含まれる(`opencascade-sys/build.rs` OCCT_LIBS) | **L(3〜5日超)**。TDocStd_Document / TDF_Label / XCAFDoc_ShapeTool / DimTolTool 等のXDE型群を一式露出する必要があり型面積が大きい。リンク設定変更は不要なのが救い。M3以降へ先送り推奨 |
| バウンディングボックス | Bnd_Box + BRepBndLib::Add | ◎ | sys: `src/bnd.rs`(Get/CornerMin/CornerMax/Gap) + `src/b_rep_bnd_lib.rs`(`BRepBndLib::Add`)。safe: `opencascade/src/bounding_box.rs` の `aabb(shape)` → min/max(2025-06 #212) | 不足なし(OBBや `BRepBndLib::AddOptimal` が要るなら S) |
| 剛体変換 | gp_Trsf + BRepBuilderAPI_Transform | **△** | sys: `src/gp.rs` L77-87(`SetMirror/SetRotation/SetScale/SetTranslation/Value`、gp_GTrsfも)、`src/b_rep_builder_api.rs` L117(`BRepBuilderAPI_Transform`)。safe: `shape.rs` の `translated/rotated/scaled/mirrored`(2026-06 #224)。**`Multiply` / `PreMultiply` / `Invert` が未露出 → 変換の合成・逆変換が不可** | **S(0.5日)**。gp_Trsfへのメソッド追加3〜4個のみ |
| メッシュ化 | BRepMesh_IncrementalMesh | ◎ | sys: `src/b_rep_mesh.rs`(deflection指定)。safe: `mesh.rs` の `Mesher` / `Shape::mesh()` / `mesh_with_tolerance`(頂点・法線・UV・インデックス取得) | 不足なし(角度偏差等のIMeshTools_Parameters指定が要るなら S) |
| レイと形状の交差(肉厚チェック) | BRepIntCurveSurface_Inter | ◎ | sys: `src/b_rep_int_curve_surface.rs`(Init/More/Next/face/point/U/V/W)。safe: `shape.rs` の `faces_along_line`(L796、ヒット面+t/u/v+交点座標を返す) | 不足なし。ADCの肉厚レイキャスト要件をそのまま満たす |
| STEP入力(優先度低) | STEPControl_Reader | ◎ | sys: `step_control.rs`(`read_step` / `one_shape_step`)。safe: `shape.rs` の `read_step`(L551) | 単一形状(OneShape)取り込みのみ。ゴールデンテスト用途には十分。ルート形状分割が要るなら S |

補足: 上記以外にADCに有用な既存API — `BRepFeat_MakeCylindricalHole`(`Shape::drill_hole`)、`BRepOffsetAPI`(hollow/offset_surface/薄肉化)、`BRepAlgoAPI_Section`(断面)、BRep/STL/IGES入出力、`Shape::clean()`(ShapeUpgrade_UnifySameDomain)。

**被覆率概観: ◎7 / △3 / ✕2(+PMI✕)。M1のコア(形状生成・ブーリアン・フィレット)は追加実装ゼロで着手可能。**

## 3. リポジトリ健全性

| 項目 | 事実 |
|---|---|
| 最終push | 2026-06-27(GitHub API `pushed_at`)。直近コミット: #227 MakeFace::Add(2026-06-27)、#226 複数形状STEP出力(2026-06-09)、#225 safe downcast、#224 transform群(2026-06-06) |
| メンテ活性 | 活発。メンテナ(Brian Schwind)によるブリッジ分割リファクタ(#199, 2026-05)に加え、外部コントリビュータ(Gal Buki、Satoshi Misumi、他)のPRが2025〜2026年に継続マージ。**上流PRを受け入れる文化が実証されている** |
| 規模 | Star 253 / Fork 67 / Open Issues 65 / コミット165 |
| ライセンス | LGPL-2.1(opencascade / opencascade-sys / occt-sys とも)。OCCT本体もLGPL-2.1(例外条項付き) |
| リリース | **GitHubリリースなし。crates.io の opencascade / opencascade-sys は 0.2.0(2023-08-16)で main から大きく乖離** → 採用は必ずgit参照 or vendor。occt-sys のみ 0.6.0(2024-11-30)が比較的新しい |
| OCCTバージョン | サブモジュール `crates/occt-sys/OCCT` はコミット bd2a789(="Update version to 7.8.1"、2024-03-31)に固定 = **OCCT 7.8.1**。`opencascade-sys/build.rs` の互換ゲートは `major==7 && minor>=8`(**7.8/7.9のみ可、8.0は不可**) |
| 構成 | `crates/{occt-sys, opencascade-sys, opencascade, model-api, viewer, kicad-parser, wasm-example}`。adc-kernelが必要なのは前3つのみ |

**fork/vendor戦略への示唆:**
- cxxブリッジがドメイン別43ファイル(`opencascade-sys/src/*.rs` + `include/*.hxx` の対)に分割済みのため、**ADCの追加(b_rep_extrema.rs、b_rep_tools_history.rs、interface_static.rs等)は新規ファイル追加で完結し、上流とのコンフリクト面が最小**。build.rsの`rust_bridges`配列への1行追加のみが共有ファイル変更点。
- 上流のPR受け入れ実績から、履歴追跡・DistShapeShapeは汎用性が高く上流還元しやすい。PMI/XDEは大きいのでfork内先行→分割PRが現実的。
- crates.io公開が止まっているため、Cargo.tomlは `git = "..."`(自forkのURL+rev固定)または vendor ディレクトリ参照とする。
- LGPL-2.1のため、ADC本体を非LGPLにする場合は adc-kernel の静的リンク形態に注意(再リンク可能なオブジェクト提供義務等)。fork自体はLGPL維持で問題なし。
- **OCCTはLGPL-2.1+例外条項(OCCT LGPL Exception)。将来ADCのバイナリを配布する際はライセンス表記義務がある**(同梱ライセンス文書・帰属表示。静的リンク時は上記の再リンク可能性にも留意)。
- **fork対象はopencascade-rsのcxxブリッジ層(crates/opencascade-sys + crates/opencascade)。occt-sysはcrates.io依存として維持する**(OCCT 7.8.1のvendor+cmakeビルドはocct-sys側の責務であり、ADCの追加FFIはブリッジ層で完結する。実測でも依存解決はcrates.ioのocct-sys v0.6.0が使われることを確認済み)。

## 4. devcontainer方針(OCCTプリビルド)

**ビルド構造の事実**: デフォルトの `builtin` フィーチャで occt-sys がOCCT 7.8.1をcmakeでフルソースビルド(静的リンク)する。`--no-default-features` + システムOCCT(cmake検出、`DEP_OCCT_ROOT` で明示可)で動的リンクに切替可能で、READMEも「ビルド時間が大幅に短縮される」と明記。バージョンゲートは 7.8以上・8.0未満。

**プリビルドOCCT 7.xの入手選択肢(2026-07-11時点で確認)**:

| 選択肢 | バージョン | ゲート(7.8≦x<8.0)適合 | 備考 |
|---|---|---|---|
| Debian trixie apt (`libocct-*-dev`) | 7.8.1+dfsg1-3 | **○(サブモジュールと同一の7.8.1)** | devcontainerベースに最適 |
| Ubuntu 24.04 apt | 7.6.3 | ✕(古すぎ) | 使用不可 |
| conda-forge `occt` | 7.8.x / 7.9.x / 8.0.0 | ○(7.8/7.9を明示ピン) | linux-64/aarch64, osx, win対応。CIでの再現性高 |
| Homebrew `opencascade` | 7.9.3(bottleあり: arm64 mac / linux) | ○ | macOSローカル開発用 |
| vcpkg `opencascade` | **8.0.0** | **✕(ゲート不適合)** | 旧ポート固定が必要になるため非推奨 |
| 公式Dockerイメージ | 存在せず(未確認=広く使われる公式OCCTランタイムイメージは見つからなかった) | — | 自前レイヤーで解決 |

**推奨構成**:
1. **devcontainerは Debian trixie ベース + 二段構え**。基本は fork の `builtin`(OCCT 7.8.1ソースビルド)を **Dockerイメージのビルドレイヤーで1回だけ `cargo build -p adc-kernel` して焼き込む**(cargo target/registryキャッシュをイメージに保持)。これで開発者の初回ビルドからOCCTコンパイルが消え、OCCTバージョンがサブモジュールと厳密一致(ゴールデンテストの数値再現性を担保)。
2. イテレーション高速化が欲しい開発ループでは `apt install libocct-*-dev`(7.8.1)+ `--no-default-features` の動的リンクを許可。**同じ7.8.1なので形状演算結果の乖離リスクが最小**。
3. macOSローカルはHomebrew 7.9.3+動的リンクを「便宜的に可」とするが、**ゴールデンテストの基準値は必ずコンテナ(7.8.1)内で生成・検証**する(7.8↔7.9のアルゴリズム差異による微小数値差の混入防止)。
4. CIは同一devcontainerイメージ+`Swatinem/rust-cache`(上流CIと同方式)。
5. ソースビルド所要時間の**実測値(2026-07-11、Apple Silicon Mac上のDocker/arm64、Debian trixie-slimベース)**: warmupビルド(occt-sys v0.6.0のOCCT 7.8.1 cmakeビルド+opencascade cxxブリッジ、release)**5分57秒**、イメージビルド全体(apt+rustup込み)**6分42秒**。イメージサイズ3.44GB(うちcargo target 1.4GB)。事前推定の15〜40分より大幅に速い(ninja+高並列arm64ネイティブ、可視化系モジュール非対象のため)。x86_64ホストやCIランナーでは要再計測。なお依存解決の実体は git の opencascade-sys → **crates.io の occt-sys v0.6.0(OCCT 7.8.1をvendor)** であり、サブモジュール直参照ではない。

## 5. リスクと未確認事項

**リスク(事実ベース)**:
1. **履歴追跡が完全欠落**(最重要ギャップ)。ADCのアンカー再束縛はこのFFI追加(M=2〜3日)が完了するまで実装不能。M1の最初のFFI作業として着手すべき。ブーリアンの`new_edges`(SectionEdges)だけでは Modified/IsRemoved の代替にならない。
2. **上流はOCCT 8.0(2026-05リリース、vcpkgは既に8.0.0)へ未追随**。build.rsゲートが `major==7` 固定のため、上流が8.0へ移行した場合ADC側forkでの追随判断(7.8.1固定継続 or 8.0検証)が必要になる。当面は7.8.1ピンで問題なし。
3. crates.ioリリースが2023年で停止 → バージョン管理はgit rev固定/vendorで自衛する(セマンティックバージョニングに頼れない)。
4. LGPL-2.1の静的リンク義務(ADC本体のライセンス設計と要整合)。
5. STEP出力は現状スキーマ指定不可(AsIs転送+デフォルトスキーマ)。AP242要件はInterface_Static露出(S)まで未達成。

**未確認事項(推測と区別して明記)**:
- ~~occt-sys(builtin)の実ビルド時間は未計測~~ → **実測済み**(§4参照: warmup 5分57秒 / イメージ全体6分42秒、Apple Silicon)。x86_64/CIでは未計測。
- OCCT 7.8.1における `write.step.schema` の**AP242指定値の正確な文字列("AP242DIS"等)と、AP242 ISへの対応範囲は未確認**(実装時にOCCT 7.8ドキュメントで確認)。
- **PMI(GD&T)のセマンティック出力がOCCT 7.8.1のXDEでどこまで書き出せるか**(グラフィカルPMI/セマンティックPMIの範囲)は本調査では未確認。
- Ubuntu 25.04/25.10 のocctパッケージ版は未確認(Debian trixie 7.8.1で代替確認済みのため影響小)。
- `BRepAlgoAPI_Cut::Generated` がsafe層から未使用である理由(将来の履歴対応の布石か)は未確認。
- 上流CIのビルド所要時間の実測値(キャッシュ有無別)は未確認。

**工数合計(再掲)**: M1〜M3必須分 ≈ **4〜6人日**(履歴M + 距離S〜M + AP242設定S + Trsf合成S + 体積S)。PMI出力は別枠L(3〜5日超、M3以降)。

## 6. 承認済み決定事項(2026-07-11 レビュー)

本レポートはレビュー承認済み。以下が確定:

1. **fork/vendor戦略**: M1-1着手時にfork + Cargo git依存のrev固定。vendorはしない(エンタープライズのオフラインビルド要件が現実化した時点で切替)。ADCの追加FFIは新規cxxブリッジファイルで実装し、上流PR可能な形を維持する
2. **FFI実装順**(4〜6人日の消化順):
   1. BRepTools_History — 最優先。ADR-001のアンカー再束縛の要。M1-1と同時着手
   2. gp_Trsf Multiply/Invert — 小粒だがM3(mate変換合成)のブロッカー。fork初回バッチに同梱
   3. BRepExtrema_DistShapeShape — M2-2(Clearance)のブロッカー。M2着手前まで
   4. Interface_Static(AP242指定) — M3以降に先送り
3. **M1-6受入条件の緩和**: 初期は既定スキーマ(AP214)で可 → `04-units-of-work.md` に反映済み(US-07の本旨は既存ビューアで開けること)
