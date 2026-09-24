# echo-protocol（示例插件）

Orbit 插件系统的最小可运行示例：`protocol-plugin` world 的完整桩实现。

- 声明协议 id：`echo`
- `execute`：把入参 metadata + payload 回显（支持 `prefix` / `separator` 参数）
- 连接参数 / 消息参数由 `manifest.json` 中的 `connectionConfigSchema` / `requestConfigSchema` 驱动前端动态表单

## 目录结构

```
echo-protocol/
├── Cargo.toml        # cdylib，构建为 WASM 组件
├── manifest.json     # 插件元数据（含动态表单 schema）
├── src/lib.rs        # wit-bindgen 生成绑定 + 协议实现
└── README.md
```

## 构建

```bash
# 需已安装 wasm32-wasip2 target
rustup target add wasm32-wasip2
cargo build --target wasm32-wasip2 --release
```

产物：`target/wasm32-wasip2/release/echo_protocol.wasm`

## 打包并安装

Orbit 插件以 **zip** 分发，zip 顶层即插件目录：

```text
com.example.echo/          ← 目录名必须与 manifest.id 一致
├── manifest.json
└── echo.wasm              ← 将编译产物重命名为 entry 指定的文件名
```

将 `echo_protocol.wasm` 复制为 `echo.wasm`，与 `manifest.json` 一起放入 `com.example.echo/`，
压缩为 `com.example.echo.zip`。在插件管理页「安装 zip 包」上传即可。

> 本仓库提供 `build.ps1`（Windows）与 `build.sh`（macOS/Linux）自动化构建 + 打包。

## 校验项

- manifest.id 与顶层目录名一致 ✅
- protocol 插件必须声明 `connectionConfigSchema` ✅
- entry 文件名需以 `.wasm` 结尾 ✅
