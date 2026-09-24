#!/usr/bin/env bash
# 构建 pg-query 插件为 WASM 组件并打包为 orbit zip 插件。
set -euo pipefail

cd "$(dirname "$0")"
TARGET="wasm32-wasip2"
PLUGIN_DIR="com.example.pg"

rustup target add "$TARGET" >/dev/null 2>&1 || true
cargo build --target "$TARGET" --release

rm -rf "$PLUGIN_DIR"
mkdir -p "$PLUGIN_DIR"
cp manifest.json "$PLUGIN_DIR/"
cp "target/${TARGET}/release/pg_query.wasm" "$PLUGIN_DIR/"

if command -v zip >/dev/null 2>&1; then
  zip -r "com.example.pg.zip" "$PLUGIN_DIR"
else
  python3 - <<'PY'
import shutil
shutil.make_archive("com.example.pg", "zip", ".", "com.example.pg")
PY
fi

echo "OK → com.example.pg.zip（可在插件管理页安装）"
