# ADC設計リポジトリ・スターター

新しい設計リポジトリの雛形。中身は3点:

- `design.ron` — 正典の雛形(そのまま `adc check` が通る最小構成)
- `CLAUDE.md` — Claude CodeがADCを駆動するための運用知識(パターンA/B、CLI、修復手順)
- `.github/workflows/` — PRごとに全検証を回しmargin表をStep Summaryに出すCI

導入手順は ADC本体リポジトリの `docs/quickstart.md` を参照。
このディレクトリを新リポジトリにコピーして `git init` すれば開始できる。
