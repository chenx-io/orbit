#!/usr/bin/env bash
# 构建 echo-protocol 插件为 WASM 组件并打包为 orbit zip 插件。
set -euo pipefail

cd "$(dirname "$0")"
TARGET="wasm32-wasip2"
PLUGIN_DIR="com.example.echo"

rustup target add "$TARGET" >/dev/null 2>&1 || true
cargo build --target "$TARGET" --release

# 组装插件目录
rm -rf "$PLUGIN_DIR"
mkdir -p "$PLUGIN_DIR"
cp manifest.json "$PLUGIN_DIR/"
cp "target/${TARGET}/release/echo_protocol.wasm" "$PLUGIN_DIR/echo.wasm"

# 打包 zip（顶层即插件目录）
if command -v zip >/dev/null 2>&1; then
  zip -r "com.example.echo.zip" "$PLUGIN_DIR"
else
  python3 - <<'PY'
import shutil
shutil.make_archive("com.example.echo", "zip", ".", "com.example.echo")
PY
fi

echo "OK → com.example.echo.zip（可在插件管理页安装）"
