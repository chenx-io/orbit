# csv-codec（示例编解码插件）

Orbit 编解码插件的最小可运行示例：`codec-plugin` world 的完整桩实现。

- 声明格式名：`tsv`
- `encode`：JSON DataValue → TSV（首行表头，`\t` 分隔）
- `decode`：TSV → JSON 对象数组
- 纯函数、无网络；安装后在 `payload_format` / `response_format` 下拉可见 `tsv`

## 构建与打包

```bash
rustup target add wasm32-wasip2
cargo build --target wasm32-wasip2 --release   # 产物 target/wasm32-wasip2/release/csv_codec.wasm
```

将 `csv_codec.wasm` 重命名为 `tsv.wasm`，与 `manifest.json` 放入 `com.example.tsv-codec/`，压缩为 zip 后在插件管理页「安装 zip 包」。

或直接运行 `build.sh` / `build.ps1` 一键构建 + 打包。

## 说明

codec 插件**无需** `connectionConfigSchema`；`manifest.type` 为 `"codec"`，`capabilities.codecs` 声明格式名。
