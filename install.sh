#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────
# Orbit 官方安装脚本
#
# 用法（三选一）：
#   curl -fsSL https://raw.githubusercontent.com/chenx-io/orbit/main/install.sh | bash
#   curl -fsSL https://raw.githubusercontent.com/chenx-io/orbit/main/install.sh | bash -s -- --app
#   curl -fsSL https://raw.githubusercontent.com/chenx-io/orbit/main/install.sh | bash -s -- --version v0.1.0
#
# 选项：
#   --cli              只安装 orbit-cli（默认，轻量、无需管理员）
#   --app              同时安装 Orbit App 安装包（并启动安装器）
#   --version <ver>    指定版本（默认 latest，如 v0.1.0）
#   --dir <path>       自定义 CLI 安装目录（默认 ~/.orbit/bin）
#
# 环境变量：ORBIT_REPO / ORBIT_VERSION 可覆盖默认值
# 下载的压缩包会通过 Release 中的 SHA256SUMS 自动校验完整性
# ─────────────────────────────────────────────────────────────
set -euo pipefail

# 发布仓库（仓库迁移后改这里即可，脚本本身无需其他改动）
REPO="${ORBIT_REPO:-chenx-io/orbit}"
VERSION="${ORBIT_VERSION:-latest}"
INSTALL_CLI=true
INSTALL_APP=false
PREFIX="${ORBIT_PREFIX:-$HOME/.orbit}"

while [ $# -gt 0 ]; do
  case "$1" in
    --cli) INSTALL_CLI=true; INSTALL_APP=false ;;
    --app) INSTALL_APP=true ;;
    --all) INSTALL_CLI=true; INSTALL_APP=true ;;
    --version) VERSION="$2"; shift ;;
    --dir) PREFIX="$2"; shift ;;
    -h|--help)
      sed -n '2,18p' "$0" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *) echo "未知参数: $1（用 --help 查看用法）" >&2; exit 1 ;;
  esac
  shift
done

log()  { printf '\033[1;34m%s\033[0m\n' "$*"; }
ok()   { printf '\033[1;32m%s\033[0m\n' "$*"; }
die()  { printf '\033[1;31m%s\033[0m\n' "$*" >&2; exit 1; }

# ── 平台检测 ────────────────────────────────────────────────
detect_platform() {
  case "$(uname -s | tr '[:upper:]' '[:lower:]')" in
    linux) OS="linux" ;;
    darwin) OS="darwin" ;;
    mingw*|msys*|cygwin*) OS="windows" ;;
    *) die "不支持的操作系统: $(uname -s)" ;;
  esac
  case "$(uname -m)" in
    x86_64|amd64) ARCH="x86_64" ;;
    aarch64|arm64) ARCH="aarch64" ;;
    *) die "不支持的 CPU 架构: $(uname -m)" ;;
  esac
  # windows 安装器固定 x86_64（当前发布形态）
  [ "$OS" = "windows" ] && ARCH="x86_64"
  # Intel Mac：GitHub Actions 已无免费 x64 macOS runner，官方不发布该形态
  if [ "$OS" = "darwin" ] && [ "$ARCH" = "x86_64" ]; then
    die "Orbit 暂不提供 Intel Mac 版本，请在 Apple Silicon Mac 上安装（x86_64 程序可经 Rosetta 运行）"
  fi
}

# ── GitHub 下载地址 ─────────────────────────────────────────
asset_url() { # <asset-name> → https://github.com/<repo>/releases/(latest/download|download/<ver>)/<asset>
  if [ "$VERSION" = "latest" ]; then
    echo "https://github.com/$REPO/releases/latest/download/$1"
  else
    echo "https://github.com/$REPO/releases/download/$VERSION/$1"
  fi
}

# 按正则从 Release 元数据匹配资产名（App 资产名带版本号，无法硬编码）
# 用法：latest_asset_name '<pattern>'  → 输出资产名（未找到输出空）
latest_asset_name() {
  local pattern="$1" api url
  if [ "$VERSION" = "latest" ]; then
    api="https://api.github.com/repos/$REPO/releases/latest"
  else
    api="https://api.github.com/repos/$REPO/releases/tags/$VERSION"
  fi
  url="$(curl -fsSL "$api" | grep -oE '"browser_download_url": *"[^"]+"' | head -n 60)"
  printf '%s\n' "$url" | grep -oE '"[^"]*'"$pattern"'[^"]*"' | head -n 1 | tr -d '"'
}

download() { # <url> <dest>
  log "⬇️  下载 $2"
  curl -fsSL --retry 3 "$1" -o "$2"
}

# 用 Release 里的 SHA256SUMS 校验下载的压缩包
verify_checksum() { # <file> <asset-name>
  local file="$1" name="$2" line expected
  line="$(curl -fsSL --retry 3 "$(asset_url SHA256SUMS)" | grep -F "$name" | head -n 1 || true)"
  if [ -z "$line" ]; then
    die "未找到 $name 的 SHA256SUMS 校验值，请确认版本 $VERSION 的 Release 已包含校验文件"
  fi
  expected="${line%% *}"
  echo "$expected  $file" | sha256sum -c - >/dev/null 2>&1 \
    || die "校验失败: $name（下载可能损坏，请重试或更换网络）"
  ok "🔐 校验通过: ${expected:0:12}…"
}

# ── 安装 orbit-cli ──────────────────────────────────────────
install_cli() {
  local bin_dir="$PREFIX/bin"
  mkdir -p "$bin_dir"
  local asset="orbit-cli-$OS-$ARCH.tar.gz"
  [ "$OS" = "windows" ] && asset="orbit-cli-windows-$ARCH.zip"

  local tmp
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' RETURN
  download "$(asset_url "$asset")" "$tmp/pkg"
  verify_checksum "$tmp/pkg" "$asset"

  log "📦 解压到 $bin_dir"
  mkdir -p "$tmp/out"
  if [ "$OS" = "windows" ]; then
    unzip -o "$tmp/pkg" -d "$tmp/out" >/dev/null
  else
    tar -xzf "$tmp/pkg" -C "$tmp/out"
  fi
  cp -f "$tmp/out/orbit"* "$bin_dir/" 2>/dev/null || cp -f "$tmp/out/orbit" "$bin_dir/"
  chmod +x "$bin_dir"/orbit* 2>/dev/null || true
  ok "✅ orbit-cli 已安装: $bin_dir/orbit"
  "$bin_dir/orbit" --version
  ensure_path "$bin_dir"
}

# 把 CLI 目录写入 shell 配置（幂等，仅 bash/zsh）
ensure_path() {
  local dir="$1" rc
  case "${SHELL:-}" in
    *zsh) rc="$HOME/.zshrc" ;;
    *)    rc="$HOME/.bashrc" ;;
  esac
  if ! echo ":$PATH:" | grep -qF ":$dir:"; then
    if [ -f "$rc" ] && ! grep -qF "$dir" "$rc" 2>/dev/null; then
      printf '\nexport PATH="%s:$PATH"\n' "$dir" >> "$rc"
      ok "🔧 已追加 PATH 到 $rc（重新打开终端生效）"
    else
      log "💡 将 CLI 加入 PATH: export PATH=\"$dir:\$PATH\""
    fi
  fi
}

# ── 安装 Orbit App ──────────────────────────────────────────
install_app() {
  case "$OS" in
    linux)
      # 发行版检测：Debian/Ubuntu 系用 dpkg 装 deb，Fedora/RHEL/openSUSE 系用 rpm
      local os_id
      os_id="$(grep -E '^ID=' /etc/os-release 2>/dev/null | cut -d= -f2 | tr -d '"' | tr '[:upper:]' '[:lower:]' || true)"
      case "$os_id" in
        debian|ubuntu|linuxmint|pop|elementary|zorin|kali|raspbian|deepin|uos)
          local deb tmp
          deb="$(latest_asset_name '\.deb$')"
          [ -n "$deb" ] || die "未找到 Debian/Ubuntu 安装包（.deb）"
          tmp="$(mktemp -d)"
          download "$(asset_url "$deb")" "$tmp/orbit.deb"
          verify_checksum "$tmp/orbit.deb" "$deb"
          log "📦 安装 $deb（需要 sudo）"
          sudo dpkg -i "$tmp/orbit.deb" || sudo apt-get install -f -y
          rm -rf "$tmp"
          ok "✅ Orbit 已安装，从应用菜单启动"
          ;;
        fedora|rhel|centos|rocky|almalinux|opensuse*|suse*)
          local rpm tmp
          rpm="$(latest_asset_name '\.rpm$')"
          [ -n "$rpm" ] || die "未找到 RPM 安装包（.rpm）"
          tmp="$(mktemp -d)"
          download "$(asset_url "$rpm")" "$tmp/orbit.rpm"
          verify_checksum "$tmp/orbit.rpm" "$rpm"
          log "📦 安装 $rpm（需要 sudo）"
          if command -v dnf >/dev/null 2>&1; then
            sudo dnf install -y "$tmp/orbit.rpm"
          elif command -v zypper >/dev/null 2>&1; then
            sudo zypper install -y "$tmp/orbit.rpm"
          else
            sudo rpm -Uvh "$tmp/orbit.rpm"
          fi
          rm -rf "$tmp"
          ok "✅ Orbit 已安装，从应用菜单启动"
          ;;
        *)
          die "暂不支持该 Linux 发行版（ID: ${os_id:-unknown}），请从源码构建：https://github.com/$REPO#build-from-source"
          ;;
      esac
      ;;
    darwin)
      local dmg
      dmg="$(latest_asset_name '\.dmg$')"
      [ -n "$dmg" ] || die "未找到 macOS 安装包（.dmg）"
      local tmp; tmp="$(mktemp -d)"
      download "$(asset_url "$dmg")" "$tmp/Orbit.dmg"
      verify_checksum "$tmp/Orbit.dmg" "$dmg"
      log "🍎 挂载 DMG 并拷贝到 /Applications"
      hdiutil attach "$tmp/Orbit.dmg" -nobrowse -quiet -mountpoint "$tmp/mnt"
      if [ -d /Applications ] && [ -w /Applications ]; then
        rm -rf "/Applications/Orbit.app"
        cp -R "$tmp/mnt/Orbit.app" /Applications/
        ok "✅ Orbit 已安装到 /Applications"
      else
        cp -R "$tmp/mnt/Orbit.app" "$HOME/Applications/" 2>/dev/null || die "无法写入 /Applications，请手动拷贝"
        ok "✅ Orbit 已安装到 ~/Applications"
      fi
      hdiutil detach "$tmp/mnt" -quiet || true
      rm -rf "$tmp"
      ;;
    windows)
      local exe
      exe="$(latest_asset_name 'setup\.exe$')"
      [ -n "$exe" ] || die "未找到 Windows 安装包（-setup.exe）"
      local tmp; tmp="$(mktemp -d)"
      download "$(asset_url "$exe")" "$tmp/setup.exe"
      verify_checksum "$tmp/setup.exe" "$exe"
      log "🚀 启动安装器（请跟随向导完成安装）"
      "$tmp/setup.exe" &
      ;;
  esac
}

# ── 主流程 ──────────────────────────────────────────────────
main() {
  detect_platform
  log "🪐 Orbit installer | $OS/$ARCH | repo=$REPO | version=$VERSION"
  [ "$INSTALL_CLI" = true ] && install_cli
  [ "$INSTALL_APP" = true ] && install_app
  ok "🎉 安装完成！更多信息: https://github.com/$REPO"
}

main
