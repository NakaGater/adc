# ADR-002: カーネルはOCCT、Rustから opencascade-rs を土台にFFI拡張

**Status:** Accepted

## Context

B-repカーネルの自作は非現実的(ブーリアン・フィレットの堅牢性は数十年分の資産)。選択肢は OCCT(LGPL)、純Rustのtruck、商用カーネル。実装言語はRust(長期資産・型安全・決定性の観点で確定済み)。

## Decision

1. **カーネルはOCCT 7.x一択。** truckはブーリアン/フィレット/STEP対応がOCCTに遠く及ばない
2. **バインディングは `opencascade-rs`(bschwind)を土台に採用。** 不足APIは同クレートのcxxベースFFI層に追加実装する(fork/vendorし、上流にPR可能な形を保つ)
3. **FFI境界を極小化する。** OCCTに触れるのは `adc-kernel` クレートのみ。上位層(スキーマ、チェッカーの代数計算、CLI)はOCCT非依存とし、カーネル差し替え可能性を残す
4. **M1の最初のユニットはAPI被覆調査。** 必要API(プリミティブ、ブーリアン、フィレット/面取り、BRepExtrema、BRepGProp、BRepTools_History、STEP AP242書き出し)の被覆表を作り、不足分のFFI追加工数を見積もってから本実装に入る

## Consequences

- (+) 実績あるカーネルの堅牢性+Rustの型安全・並列性
- (−) FFI追加はC++理解を要する。cxxブリッジの雛形をユニット化して繰り返しコストを下げる
- (−) OCCTのHistory APIの網羅性には既知の穴がある。追跡不能ケースはE-ANCHOR-BINDに落とす(ADR-001の保証を優先)
- (−) ビルド環境が重い(OCCTのC++ビルド)。devcontainerでOCCTプリビルドを配布する
