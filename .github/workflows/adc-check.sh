#!/usr/bin/env bash
# devcontainer内で実行される検証本体 (M4-4)。
# 設計リポジトリに転用する場合は DESIGN を差し替える。
set -uo pipefail

DESIGN="examples/motor_bracket/design.ron"

cargo build --release -p adc-cli || exit 2
BIN="${CARGO_TARGET_DIR:-target}/release/adc"

# check: exit 0=全Pass / 1=Fail≥1 / 2=Inconclusive≥1またはE-* (07-cli.md)。
# Fail/Inconclusiveでもreportは生成してからそのexit codeでCIを落とす
code=0
"$BIN" check --design "$DESIGN" --format=jsonl > results.jsonl || code=$?

"$BIN" report results.jsonl > report.md
cat report.md

exit "$code"
