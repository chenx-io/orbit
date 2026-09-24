# status-assertion（示例断言插件 · 二期）

Orbit 断言插件的最小可运行示例：`assertion-plugin` world 的完整桩实现。

- 声明断言能力：`status-range`
- `run`：解析响应摘要中的 HTTP 状态码，检查是否落在 `options: {min, max}` 区间
- 纯函数、无网络；接入二期断言执行引擎

## 构建与打包

```bash
rustup target add wasm32-wasip2
cargo build --target wasm32-wasip2 --release   # 产物 target/wasm32-wasip2/release/status_assertion.wasm
```

将产物放入 `com.example.status-assertion/`（与 `manifest.json`），压缩为 zip 后在插件管理页安装。
或运行 `build.sh` / `build.ps1` 一键构建 + 打包。

## 说明

二期插件 `manifest.type` 为 `"assertion"` / `"extractor"` / `"output"`，无需 `connectionConfigSchema`。
宿主提供 `PluginManager::run_assertion` 等执行入口，可在单发/压测响应流后调用。
