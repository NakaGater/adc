#!/usr/bin/env bash
# devcontainer内で実行される検証本体(スターター版)。
# ADC本体は .adc-src/ にチェックアウト済み(adc-check.yml)。
set -uo pipefail

DESIGN="design.ron"

(cd .adc-src && cargo build --release -p adc-cli) || exit 2
BIN="${CARGO_TARGET_DIR:-.adc-src/target}/release/adc"

# check: exit 0=全Pass / 1=Fail≥1 / 2=Inconclusive≥1またはE-*。
# Fail/Inconclusiveでもreportは生成してからそのexit codeでCIを落とす
code=0
"$BIN" check --design "$DESIGN" --format=jsonl > results.jsonl || code=$?

"$BIN" report results.jsonl > report.md
cat report.md

exit "$code"
