# OCCT既知の穴・実測記録 (M1時点)

M1(fork rev `c5716f6`、OCCT 7.8.1)の実装・テストで実測したOCCTの制約と、
ADC側の対策の正典。**M2チェッカー設計の入力**。新たな実測は本書に追記する。

## 1. Standard_Failure は std::exception を継承しない【致命・対策済み】

OCCTの例外階層は `Standard_Transient` 系であり、cxxの `Result<>` 境界
(catch std::exception)を**素通りして std::terminate → プロセスabort**する。

- **対策**: OCCT APIをResult境界で包むときは必ずC++側で
  `catch (const Standard_Failure &)` → `std::runtime_error` 変換を挟む
  (fork `b_rep_tools_history.hxx` のTryラッパ群)。M1-7で
  フィレット/面取り(Try+Build+IsDone)とブーリアン(TryNew: 構築=Build内包の
  ため**コンストラクタ**を包む)に適用済み
- **残り**: プリミティブ生成(MakeBox/MakeCylinder等のDomainError)は
  FFI前のRust側寸法検証(`e_pos`: 正値要求)で決定的に遮断

## 2. History追跡の実測結果(ADR-002「既知の穴」の具体化)

| 操作 | 面 | エッジ |
|---|---|---|
| BRepAlgoAPI (Cut/Fuse/Common) | ◎ Modified/IsRemoved一貫(M1-1/M1-2/M1-5全テストで矛盾なし) | ◎(rim前送りで確認) |
| BRepFilletAPI (Fillet/Chamfer、BRepTools_History template ctor経由) | ◎ Modified一貫 | **✕ 偽Removed**: 幾何的に無傷のエッジ(例: フィレット対象でない穴リム)を IsRemoved=true と誤報告する(実測: m1_5 measurementテスト) |

- **対策(実装済み)**: エッジの前送りで Removed 報告を受けたら、**結果ソリッド内を
  TopoDS IsSame で再走査**し、実在すれば束縛を維持する(`adc-compile::forward_entry`)。
  真に消費されたエッジ(リム自体をフィレット)は走査でも見つからず
  Deleted{原因フィーチャー} になることを同テストで確認
- **含意(M2)**: エッジ由来のEvidence(最近接点の帰属等)をHistoryに頼る場合は
  同じ実在検証を要する。面は現時点で信頼可(ただし未知の操作を導入したら要実測)
- **Untracked(真の追跡不能)は M1 の全テストで未観測**。E-ANCHOR-BIND{Untracked}の
  発火経路は「Removed報告なし・Modified空・実在もしない」として温存(発火実例が
  出たら本書に記録)

## 3. 曲面への normal_at_center は射影多義で例外【対策済み】

円筒面などの重心は面上に射影が一意でなく(軸上点)、GeomAPI_ProjectPointOnSurf
が Standard_OutOfRange を投げる(→ 穴1によりabort)。

- **対策**: 面分類は `BRepAdaptor_Surface::GetType`(fork追加 `Face::surface_type`)
  ベースに変更。法線は平面確認後のみ問い合わせる(kernel docに明記)

## 4. 同軸円柱のフューズで内側側面が2分割される【対策済み】

Counterbore/Countersinkの工具合成で、小径円柱を上側工具(座ぐり円柱/皿もみ円錐)
に全通しで重ねると、小径側面が上下2面に分割され1対1追跡が壊れる。

- **対策**: 小径工具は上側工具の底から**0.5mmだけ食い込ませて**構成する
  (provides-predicates.md)

## 5. フィレットの実行可能性は近接形状に敏感【仕様側で対応】

§9サンプルの `edges_of(base.top)` が内周ループ(穴リム)を含む解釈だと、
ボルト穴リムのフィレットリング(r5.5+2.0)が外周フィレット帯と干渉して
**MakeFillet NotDone**になる(=スペックのサンプルがOCCTで実行不能だった)。

- **対応(2026-07-12)**: `edges_of` = **外周ワイヤのみ**に意味論を確定。
  内周(穴リム等)は `edges_between(wall面, 対象面)` で明示選択する

## 6. その他の留意

- §9サンプルの cb_depth(6.5) は板厚(3〜6)より深く、座ぐりが実質φ11貫通穴になる
  (幾何的には正当。板金的な意図確認は設計者側の問題でありチェッカー(M2)の
  検出対象候補: 「座ぐり深さ ≥ 板厚」警告)
- OCCTブーリアンの「失敗」はほぼ IsDone=false ではなく退化入力の例外として現れる。
  自然な失敗誘発は未達成(TryNewは防御として設置)。M2の実部品ドッグフーディングで
  実例が出たら本書に記録
