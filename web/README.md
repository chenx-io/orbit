# Orbit Web

Orbit 桌面应用前端：React + TypeScript + Vite，桌面壳为 Tauri（`src-tauri/`）。

## 开发

```bash
pnpm install      # 安装依赖
pnpm dev          # 浏览器预览（需本地 orbit-server，见 server:dev）
pnpm tauri:dev    # Tauri 桌面开发模式（自动启动 Rust 后端）
pnpm build        # 类型检查 + 产物构建
pnpm lint         # oxlint 检查
```

## 架构要点

- 前端只做 UI 与交互；数据管理（接口/插值/环境/场景）与网络协议执行均在后端 Rust 层（`../crates/`）
- 数据通道统一走后端：Tauri 桌面用命令（`src-tauri/src/commands/`），浏览器预览走 `orbit-server` HTTP API
- 快照（`orbit_data.json`）为权威存储，前端经快照同步 + 模块级命令读写
