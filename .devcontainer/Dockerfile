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

# OCCT 7.8.1 プリビルド焼き込み: opencascade-rs (rev固定) をwarmupビルド。
# rev は M1-1 の fork 作成時に fork の rev に差し替えること。
ARG OPENCASCADE_RS_REV=b6374512949727609335fa8258a25b5c94f426cb
ENV CARGO_TARGET_DIR=/opt/cargo-target
RUN mkdir -p /opt/occt-warmup/src && cd /opt/occt-warmup \
    && printf '[package]\nname = "occt-warmup"\nversion = "0.0.0"\nedition = "2021"\n\n[dependencies]\nopencascade = { git = "https://github.com/bschwind/opencascade-rs", rev = "%s" }\n' "$OPENCASCADE_RS_REV" > Cargo.toml \
    && echo 'fn main() {}' > src/main.rs \
    && cargo build --release \
    && chmod -R a+rw /opt/cargo-target "$CARGO_HOME"
