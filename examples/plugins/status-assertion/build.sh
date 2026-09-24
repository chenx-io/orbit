#!/usr/bin/env bash
# 构建 status-assertion 插件为 WASM 组件并打包为 orbit zip 插件。
set -euo pipefail

cd "$(dirname "$0")"
TARGET="wasm32-wasip2"
PLUGIN_DIR="com.example.status-assertion"

rustup target add "$TARGET" >/dev/null 2>&1 || true
cargo build --target "$TARGET" --release

rm -rf "$PLUGIN_DIR"
mkdir -p "$PLUGIN_DIR"
cp manifest.json "$PLUGIN_DIR/"
cp "target/${TARGET}/release/status_assertion.wasm" "$PLUGIN_DIR/"

if command -v zip >/dev/null 2>&1; then
  zip -r "com.example.status-assertion.zip" "$PLUGIN_DIR"
else
  python3 - <<'PY'
import shutil
shutil.make_archive("com.example.status-assertion", "zip", ".", "com.example.status-assertion")
PY
fi

echo "OK → com.example.status-assertion.zip（可在插件管理页安装）"
