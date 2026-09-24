# Orbit

🚀 开源一体化 API 测试工具

[English](README.md) · [简体中文](README.zh-CN.md)

[![License: AGPL v3](https://img.shields.io/badge/License-AGPL_v3-blue.svg)](https://www.gnu.org/licenses/agpl-3.0)
[![CI](https://github.com/chenx-io/orbit/actions/workflows/ci.yml/badge.svg)](https://github.com/chenx-io/orbit/actions/workflows/ci.yml)

Orbit 是现代化的 API 测试工具，集 **API 调试**、**场景自动化** 与 **性能压测** 于一体。你可以设计请求、编排复杂业务场景、模拟高并发流量、校验接口响应，一站式完成。

无重客户端依赖，面向开发者与 QA 工程师。

## 功能特性

- **多协议 API 调试**：HTTP、gRPC、WebSocket、TCP、UDP、SSE、GraphQL。
- **场景自动化**：通过循环、条件、分组、变量串接多个接口，还原真实业务流程。
- **性能压测**：恒定 / 阶梯 / 到达率等多种 VU 模型，支持阈值与 SLA 门禁。
- **分布式压测**：Controller + Agent 架构，通过 gRPC 横向扩展。
- **Mock 服务**：基于测试计划快速启动本地 Mock。
- **导入导出**：Postman、OpenAPI、cURL、HAR、JMeter、k6 等。
- **Postman 兼容脚本**：基于 QuickJS 的 `pm.*` 前置 / 后置脚本。

## 安装

一行安装（默认安装 `orbit-cli`，含分布式 agent 启动）：

```bash
# Linux / macOS
curl -fsSL https://raw.githubusercontent.com/chenx-io/orbit/main/install.sh | bash

# Windows（PowerShell）
irm https://raw.githubusercontent.com/chenx-io/orbit/main/install.ps1 | iex
```

同时安装桌面 App：

```bash
# Linux / macOS
curl -fsSL https://raw.githubusercontent.com/chenx-io/orbit/main/install.sh | bash -s -- --app

# Windows（PowerShell）
irm https://raw.githubusercontent.com/chenx-io/orbit/main/install.ps1 -OutFile install.ps1; ./install.ps1 -All
```

指定版本 / 自定义 CLI 安装目录：

```bash
curl -fsSL https://raw.githubusercontent.com/chenx-io/orbit/main/install.sh | bash -s -- --version v0.1.0 --dir ~/tools/orbit
irm https://raw.githubusercontent.com/chenx-io/orbit/main/install.ps1 -OutFile install.ps1; ./install.ps1 -Version v0.1.0 -Dir "$env:LOCALAPPDATA\Orbit\bin"
```

> 下载的压缩包会通过 GitHub Release 中的 `SHA256SUMS` 自动校验完整性。
> 仓库地址变更时，通过环境变量 `ORBIT_REPO` 覆盖即可，脚本本身无需改动。

## 快速开始

创建测试计划 `plan.yaml`：

```yaml
name: 快速开始
scenarios:
  - name: Get 示例
    executor:
      type: sequential
      iterations: 1
    steps:
      - type: request
        name: GET /get
        request:
          method: GET
          url: https://httpbin.org/get
        checks:
          - type: status
            value: 200
```

运行：

```bash
orbit run plan.yaml
```

## 从源码构建

### 前置要求

| 依赖 | 说明 |
| --- | --- |
| [Rust](https://rustup.rs/) | stable 工具链（`rustup default stable`） |
| [Node.js](https://nodejs.org/) 18+ 与 [pnpm](https://pnpm.io/) | 桌面端前端构建（`corepack enable` 或 `npm i -g pnpm`） |
| Linux | `libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev patchelf` |
| macOS | Xcode Command Line Tools（`xcode-select --install`） |
| Windows | WebView2（Win10/11 自带）+ [Microsoft C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) |

### 构建 orbit-cli

```bash
git clone https://github.com/chenx-io/orbit.git
cd orbit
cargo build --release -p orbit-cli
```

产物为 `target/release/orbit`（Windows 为 `orbit.exe`）。执行 `orbit --help` 查看全部命令（含 `orbit run` / `orbit agent` 等）。

### 构建桌面 App

```bash
cd web
pnpm install
pnpm tauri:build
```

安装包输出到 `web/src-tauri/target/release/bundle/`（Linux 为 `.deb` / `.rpm`，macOS 为 `.dmg`，Windows 为 NSIS `.exe`）。

## 许可证

本项目采用 [GNU Affero General Public License v3.0 或更高版本](LICENSE)（AGPL-3.0-or-later）。
