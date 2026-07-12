# ADC クイックスタート(人間向け・10分)

コンテナ起動からサンプルの検証・レポート・STEP目視まで。
前提: Docker と VS Code(Dev Containers拡張)または devcontainer CLI、git。

> **コンテナを使わない場合(ホスト直インストール)**:
> `CMAKE_POLICY_VERSION_MINIMUM=3.5 cargo install --git https://github.com/NakaGater/adc adc-cli`
> (OCCTのソースビルドを含むため初回30〜60分。cmake 4系では
> `CMAKE_POLICY_VERSION_MINIMUM=3.5` が必須)。
> ただし**ゴールデン値の数値再現性はdevcontainer(OCCT 7.8.1固定)が正**であり、
> CI・チーム運用はコンテナ経由を推奨。

> **初回のみの注意**: devcontainerイメージのビルドにはOCCTのソースビルドが
> 含まれ30分以上かかる。CIが焼いたイメージ
> (`ghcr.io/nakagater/adc-devcontainer`)をpullできる環境なら、
> `.devcontainer/devcontainer.json` の `build` を `"image"` 指定に
> 差し替えることで数分に短縮できる。2回目以降はキャッシュで即起動。

## 1. クローンしてコンテナで開く(〜2分、初回を除く)

```bash
git clone https://github.com/NakaGater/adc.git && cd adc
code .   # VS Code → 右下の「Reopen in Container」
```

## 2. adcをビルド(〜3分。OCCTはイメージに焼き込み済み)

```bash
cargo build --release -p adc-cli
alias adc="$CARGO_TARGET_DIR/release/adc"
```

## 3. サンプルを検証する(〜1分)

```bash
adc check --design examples/motor_bracket/design.ron --format=jsonl > results.jsonl
echo $?   # 0 = 全Pass(1=Fail / 2=Inconclusive)
```

`wall_t` が `Open(3.0, 6.0)` のため自動で3点評価され、各行の `samples` に
lo/nominal/hi の標本別結果が入る。

## 4. margin表を見る(〜1分)

```bash
adc report results.jsonl
```

Fail先頭→margin昇順のMarkdown表。CIではこれがPRのStep Summaryに載る
(.github/workflows/adc-check.yml)。

## 5. STEPを出して目視(〜2分)

```bash
adc export --step --design examples/motor_bracket/design.ron --out ./out
open ./out/bracket.step   # FreeCAD等、任意のSTEPビューアで
```

## 6. 変更を体験する(〜3分)

```bash
adc explain wall_t --design examples/motor_bracket/design.ron --format=json  # 根拠と参照元
# design.ron の nominal を書き換えて再check → 変更部品だけ再計算(キャッシュ)
adc diff HEAD~1 HEAD --design examples/motor_bracket/design.ron             # コミット済みなら差分レポート
```

## 次のステップ

- 新しい設計を始める: `examples/starter-repo/` を新リポジトリにコピーして
  `git init`。Claude Codeで開けば `CLAUDE.md` の運用知識(パターンA/B)で
  対話設計が始められる
- 仕様の入口: `05-schema.md`(正典IR)、`08-workflows.md`(利用手順)、
  `docs/checkers.md`(チェッカーとmarginの意味)
