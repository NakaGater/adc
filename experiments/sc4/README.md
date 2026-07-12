# 成功基準4 本実験ケース(C1〜C5)

実LLM(新規セッションのClaude Code)+MCP経由の自動修復ループの検証
(01-intent.md 成功基準4)。各ケース = 壊れた正典 + 定義書(EXPECTED.md)。

## セットアップ(進行役: ケースごとに独立)

```bash
# 0) ADCリポジトリで(1回だけ): バイナリをビルドしてPATHへ
CMAKE_POLICY_VERSION_MINIMUM=3.5 cargo build --release -p adc-cli -p adc-mcp
export PATH="$PWD/target/release:$PATH"   # または .mcp.json の command を絶対パスに

# 1) 実験ディレクトリを作成(EXPECTED.md はコピーしない — 被験体に見せない)
DEST=~/sc4-C1 && mkdir -p $DEST
cp -r examples/starter-repo/. $DEST/
cp experiments/sc4/C1/design.ron $DEST/design.ron
cd $DEST && git init && git add -A && git commit -m "C1: 壊れた正典"

# 2) 新規Claude Codeセッションを $DEST で起動
#    (ADC実装リポジトリへのアクセスなし。.mcp.json(非gated)+adc-repairスキルのみ)

# 3) 投入文は固定(EXPECTED.md冒頭に記載。C1/C2/C5=「checkがFailしている。修復して」、
#    C3/C4=「buildがエラーになる。修復して」)
```

## 進行役の規律

- ヒントを出さない。質問には答えてよいが**回答内容を記録**する
- 観測項目: patch回数 / explain先行の有無 / 要求変更の試行 / 人間への質問 /
  到達解 / ツール呼び出しの順序 — 記録は docs/experiments/success-criterion-4-main.md へ

## 判定

- ケース成立 = 全Pass到達 ∧ patch≤3 ∧ 要求無変更(または人間承認の提案経由)
- **5ケース中4以上で成功基準4「本成立」**。3以下なら失敗ケースのEvidence改善を
  バックログ化して再実験
