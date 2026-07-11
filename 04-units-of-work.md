# Units of Work — マイルストーンと実装順序

Cargoワークスペース構成:

```
adc/
├── crates/
│   ├── adc-schema     # IR型定義・serde・静的検証 (OCCT非依存)
│   ├── adc-kernel     # OCCT FFI境界 (唯一OCCTに触れる層)
│   ├── adc-compile    # フィーチャー→B-rep、アンカー束縛、Assy解決
│   ├── adc-check      # Checkerトレイト+実装群
│   └── adc-cli        # adcバイナリ
└── examples/           # サンプルdesign.ron + ゴールデンテスト
```

各ユニットは「テスト先行(受入基準をテスト化)→実装」の順で進めること。

---

## M0: スキーマ基盤(ジオメトリなし・OCCT不要)

| Unit | 内容 | US |
|---|---|---|
| M0-1 | adc-schema: 全型定義+serde round-tripテスト | US-01 |
| M0-2 | 静的検証: 参照解決、循環検出、rationale必須、単位 (E-SCHEMA-*) | US-02, 04 |
| M0-3 | Expr評価器 (param参照+四則、Open伝播) | US-01, 25 |
| M0-4 | `adc explain <id>` JSON出力 | US-03 |
| M0-5 | エラーコード体系と構造化エラー出力の共通基盤 | US-08前提 |

**Exit条件:** 05-schema.md §9のサンプルがparse→検証→explainまで通る。

## M1: 単品コンパイル(OCCT導入)

| Unit | 内容 | US |
|---|---|---|
| M1-0 | **opencascade-rs API被覆調査**(ADR-002)。不足API一覧と FFI追加見積もり。devcontainer(OCCTプリビルド)整備 | — |
| M1-1 | adc-kernel: プリミティブ+ブーリアン+History取得のラッパー | US-05 |
| M1-2 | フィーチャーT1コンパイル (Block/Cylinder/Hole/Pocket/Boss) | US-05 |
| M1-3 | Fillet/Chamfer + EdgeSelector解決 | US-05 |
| M1-4 | Pattern展開(provides添字) | US-05 |
| M1-5 | アンカー2段束縛+再束縛テスト(定義変更→再buildで束縛維持/E-ANCHOR-BIND) | US-06 |
| M1-6 | STEPエクスポート+FreeCADでの開閲ゴールデンテスト。初期は既定スキーマ(AP214)で可、AP242切替はInterface_Static露出後(M3目安)にフォローアップ | US-07 |
| M1-7 | E-FEATURE-FAIL構造化(フィレット失敗ケースの再現テスト含む) | US-08 |

**Exit条件:** サンプルブラケットがbuild→STEP出力でき、`wall_t`変更→再buildで全アンカーが再束縛される。

## M2: 検証ハーネス

| Unit | 内容 | US |
|---|---|---|
| M2-1 | Checkerトレイト+results.jsonl+exit code+決定性テスト(バイト再現) | US-11, 15, 17 |
| M2-2 | Clearance/NoInterference (BRepExtrema+ブーリアン) | US-12, 16 |
| M2-3 | Mass/Cog (BRepGProp+材料) | US-13 |
| M2-4 | WallThickness (レイキャスト、density設定、限界のdoc化) | US-14 |
| M2-5 | BoundingBox / DatumValidity | US-11 |
| M2-6 | 結果キャッシュ (SHA-256、部分再計算) | US-20 |

**Exit条件:** 成功基準4の予備実験 — Evidence文字列のみからLLM(手動プロンプト)が違反箇所を特定できることを1ケースで確認。

## M3: アセンブリ

| Unit | 内容 | US |
|---|---|---|
| M3-1 | Assembly/Mate解決 (DAG位相ソート、剛体変換合成) | US-22 |
| M3-2 | E-MATE-UNSOLVED / 残自由度レポート | US-24 |
| M3-3 | Assy干渉マップ (全ペアclearance一括+margin表) | US-23 |
| M3-4 | Assy再生成テスト (部品変更→mate再束縛) | US-22 |

**Exit条件:** 3部品の実例Assy(ブラケット+シャフト+ハウジング)で干渉マップが出る。

## M4: 未確定パラメータ+CI

| Unit | 内容 | US |
|---|---|---|
| M4-1 | Open 3点ビルド&チェック | US-25 |
| M4-2 | `--narrow` 二分探索+suggested_range | US-26 |
| M4-3 | `adc diff` (制約差分+体積差分) | US-10 |
| M4-4 | `adc report` (jsonl→Markdownテーブル) + GitHub Actionsテンプレート | US-27 |

**Exit条件(=MVPコア完成):** Intent成功基準1、2、4の検証。実在部品でのドッグフーディング開始。

## M5: 板金+T2チェッカー(MVP拡張)

| Unit | 内容 | US |
|---|---|---|
| M5-1 | 板金フィーチャー+展開長 | US-09 |
| M5-2 | SheetMetalRules(代数チェック) | US-18 |
| M5-3 | ToleranceStack1D (worst-case/RSS) | US-19 |

**Exit条件(=MVP完成):** Intentに記載のMVPスコープ全充足。成功基準3(実務者テスト)の実施。

## M6 (Phase 2): エージェント統合+高度チェッカー

| Unit | 内容 | US |
|---|---|---|
| M6-1 | MCPサーバー (design_read/patch/build_and_check/evidence_query/narrow_param、--gatedモード)。LLM非搭載原則に従う(ADR-006): LLMクライアント・APIキーをリポジトリに持ち込まない。修復用エージェントSkill(`agent-skills/adc-repair`)を同梱 | US-28 |
| M6-2 | ToolAccess / MinCornerRadius | US-21 |

---

## 依存関係の要点

- M0はOCCT不要。環境構築(M1-0)と完全並行可能
- M2-1(契約)はM2の他ユニットの前提。最初に固めること
- M5-3(公差スタック)はジオメトリ不要の代数計算であり、実はM0直後にも前倒し可能。自動車設計者への訴求が最も高いため、デモ都合で前倒しを許可する

## 実装上の全体規律

1. 本プロジェクト自体をTDDで作る。各Unitの受入基準を統合テストとして先に書く
2. examples/にゴールデンファイル(design.ron→期待STEP/期待results.jsonl)を置き、リグレッションを機械検知する
3. OCCT依存はadc-kernelに閉じ込める。他クレートからのocct型のリークはCIで禁止(依存グラフチェック)
4. 全公開エラーは 05-schema.md §8 のコード体系に従い、JSONシリアライズ可能であること
