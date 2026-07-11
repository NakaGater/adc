# FreeCADヘッドレス(freecadcmd)でSTEPを開き、体積と面数をJSONで出力する。
# 使い方: freecadcmd scripts/step_volume.py <file.step>
import json
import sys

import Part  # noqa: E402 (FreeCAD module)

path = sys.argv[-1]
shape = Part.Shape()
shape.read(path)
print(json.dumps({"volume_mm3": shape.Volume, "faces": len(shape.Faces)}))
