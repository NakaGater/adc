# ADC devcontainer — Debian trixie + OCCT 7.8.1 プリビルド焼き込み
# (docs/M1-0-api-coverage.md §4 の推奨構成)
#
# 二段構え:
#   1. 基本: opencascade-rs builtin (OCCT 7.8.1 ソースビルド・静的リンク) を
#      イメージ層で1回だけビルドし、共有 CARGO_TARGET_DIR に焼き込む。
#      開発者の初回ビルドからOCCTコンパイルを消し、OCCTバージョンを
#      サブモジュール(7.8.1)と厳密一致させる(ゴールデンテストの数値再現性)
#   2. イテレーション用: apt の libocct-*-dev (trixie = 7.8.1、同一版) +
#      --no-default-features の動的リンクも選択可能にしておく

FROM debian:trixie-slim

# OCCTソースビルドに必要なツールチェーン
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential cmake ninja-build git curl ca-certificates pkg-config \
    libfontconfig1-dev libfreetype6-dev libx11-dev libxi-dev libxext-dev \
    && rm -rf /var/lib/apt/lists/*

# tier2: 動的リンク用のシステムOCCT 7.8.1 (builtinのサブモジュールと同一版)
RUN apt-get update && apt-get install -y --no-install-recommends \
    libocct-foundation-dev libocct-modeling-data-dev \
    libocct-modeling-algorithms-dev libocct-data-exchange-dev \
    libocct-ocaf-dev \
    && rm -rf /var/lib/apt/lists/*

# FreeCADヘッドレス (M1-6 STEPゴールデンテスト用 — scripts/golden-step-check.sh)
RUN apt-get update && apt-get install -y --no-install-recommends \
    freecad-python3 freecad-common python3 \
    && rm -rf /var/lib/apt/lists/*

# Rust (stable固定)
ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --default-toolchain 1.97.0 --profile minimal --component clippy,rustfmt

# OCCT 7.8.1 プリビルド焼き込み: fork (NakaGater/opencascade-rs, branch adc) を
# warmupビルド。依存指定はワークスペース(crates/adc-kernel/Cargo.toml)と
# **完全に同一のspec(git+branch)**にすること — specが異なるとcargoが別ソース扱いし
# 焼き込みが再利用されない。ビルド時点のadc HEADで焼くため、fork更新後は
# イメージを再ビルドする(古いままでも差分再ビルドが走るだけで壊れない)。
ENV CARGO_TARGET_DIR=/opt/cargo-target
RUN mkdir -p /opt/occt-warmup/src && cd /opt/occt-warmup \
    && printf '[package]\nname = "occt-warmup"\nversion = "0.0.0"\nedition = "2021"\n\n[dependencies]\nopencascade = { git = "https://github.com/NakaGater/opencascade-rs.git", branch = "adc" }\n' > Cargo.toml \
    && echo 'fn main() {}' > src/main.rs \
    && cargo build --release \
    && chmod -R a+rw /opt/cargo-target "$CARGO_HOME"
