#!/usr/bin/env bash
# M1-6 ゴールデンテスト(コンテナ内で実行すること — 基準値はOCCT 7.8.1環境で生成)
#
#   ホスト: docker run --rm -v "$PWD":/work [-e REGEN=1] adc-devcontainer \
#             bash scripts/golden-step-check.sh
#
# §9ブラケットを adc export --step で出力し、FreeCADヘッドレスで開閲して
# 体積・面数を examples/motor_bracket/golden_step.json と比較する。
# REGEN=1 でゴールデンを再生成する(仕様変更時のみ。差分はレビュー対象)。
set -euo pipefail
cd /work

cargo build -q -p adc-cli

OUT=/tmp/adc-step-out
rm -rf "$OUT" && mkdir -p "$OUT"
"${CARGO_TARGET_DIR:-target}/debug/adc" export --step \
    --design examples/motor_bracket/design.ron --out "$OUT"

ACTUAL=$(freecadcmd scripts/step_volume.py "$OUT/bracket.step" 2>/dev/null | grep '^{')
echo "freecad: $ACTUAL"

GOLDEN=examples/motor_bracket/golden_step.json
if [ "${REGEN:-0}" = "1" ]; then
    echo "$ACTUAL" > "$GOLDEN"
    echo "golden regenerated: $GOLDEN"
    exit 0
fi

python3 - "$ACTUAL" "$GOLDEN" <<'PY'
import json, sys
actual = json.loads(sys.argv[1])
golden = json.load(open(sys.argv[2]))
assert golden["faces"] == actual["faces"], f"面数不一致: golden={golden}, actual={actual}"
rel = abs(actual["volume_mm3"] - golden["volume_mm3"]) / golden["volume_mm3"]
assert rel < 1e-9, f"体積不一致: 相対誤差 {rel}"
print("golden STEP check OK:", actual)
PY
