# 成功基準4 予備実験(M2 Exit条件)— 一次証跡

- **実施日**: 2026-07-12
- **成功基準4 (01-intent.md)**: チェッカーのEvidence出力のみを材料に、LLMが違反を
  1〜3回の修正ループで解消できること(修復可能な粒度の検証)
- **形式**: ADCがFail Evidence(results.jsonl)を提示し、人間レビュアがLLM役として
  Evidence文字列**のみ**から違反箇所の特定と修復案を返答する

## フィクスチャ

§9ブラケット相当+シャフトの2体Assy。シャフト外径 `shaft_d = 56.0`
(basis: Assumption「シャフト外径の仮決め」)、ボア `bore_d = 55.0`、
要求: 半径クリアランス1.0mm以上(REQ-021)+全ペア非干渉。

## 提示したEvidence(全文)

```json
{"assert_id":"a_clearance","checker":"clearance","status":"fail","measured":0.5,"threshold":1.0,"margin":-0.5,"evidence":[{"anchors":["bracket_i.bearing_bore","shaft_i.od"],"points":[[12.5,30.0,0.0],[12.0,30.0,0.0]],"note":"最小距離 0.5(要求 1 以上)"}]}
{"assert_id":"b_no_interference","checker":"no_interference","status":"fail","measured":348.716784548,"threshold":0.0,"margin":-0.035962485,"evidence":[{"anchors":["bracket","shaft"],"points":[[40.0,30.0,2.0]],"note":"交差体積 348.716784548 mm^3"}]}
```

## LLM役(人間レビュア)の応答

**shaft_d ≤ 53.0 への変更に到達**(修正ループ1回)。
アンカー帰属(bearing_bore / od)と実測距離0.5から、シャフト外径がボアに対して
過大であることを特定。クリアランス1.0の要求から φ55 − 2×1.0 = φ53 以下と導出。

## 判定

**成立**。Evidence(アンカーID+最近接点座標+実測値+しきい値)のみで、
(1) 違反箇所の特定、(2) 定量的な修復案の導出、が1ループで完了した。

## 備考

- 本実験の後、交差時のclearance measuredは負の貫入指標(交差体積の立方根)に
  規約統一した(docs/checkers.md)。本実験のEvidenceは非交差(d=0.5)のため影響なし
- 本番の成功基準4検証(実LLM・自動ループ)はADR-006どおりMCPクライアント側で
  M6にて実施する。CI必須テストには含めない(非決定的なため)
