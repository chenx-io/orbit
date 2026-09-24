# pg-native（动态库网络协议插件示例 · PostgreSQL）

Orbit **动态库（native）协议插件**示例：直接用 **sqlx** 实现 PostgreSQL 查询。

## 与 wasm 插件的定位差异

| | wasm 插件（如 `pg-query`） | native 插件（本目录） |
|---|---|---|
| 形态 | `wasm32-wasip2` 组件 | 宿主平台 `cdylib`（`.dll`/`.so`） |
| 生态复用 | 受限（无 IO，需纯协议库） | **完整**（sqlx、tokio-postgres 全可用） |
| 连接池 | 宿主/插件自实现 | **sqlx `PgPool` 自带** |
| 隔离 | 强（wasm 沙箱） | 进程内（后期可迁移子进程） |
| 适用 | 编解码/断言/提取/输出（纯逻辑） | **网络协议**（DB/消息队列等重生态） |

## 架构

插件是独立 `cdylib`，经 **abi_stable** 稳定 FFI ABI 导出 3 个 `extern "C"` 函数：

```
orbit_native_api_version() -> u32
orbit_native_info()        -> JSON 字符串（能力声明 + 连接/消息 schema）
orbit_native_execute(req)  -> JSON 字符串（执行网络请求，返回结果）
```

- 出入参为 JSON 字符串，跨 ABI 边界用 abi_stable 稳定类型（`RString`）
- 插件内部自由使用生态库（此处用 sqlx `PgPool`，自带连接池/TLS/SCRAM）
- 宿主不介入网络/加密/连接池，仅负责 `dlopen` 加载与调用

## 构建

```bash
# 产物：target/release/pg_native.dll（Windows）/ libpg_native.so（Linux）
cargo build --release
```

## 验证

```bash
# 用真实 PG 连接做端到端（连接池复用 + 查询）
ORBIT_TEST_PG_URL="postgresql://postgres:postgres@localhost:5432/mydb" \
  cargo test -p orbit-plugin-native
```

覆盖：
1. `native_pg_plugin_loads_and_infos`：dlopen 加载 + ABI 版本校验 + `info()` 能力声明
2. `native_pg_plugin_execute_via_real_db`：真实 PG 查询（`SELECT 1 AS one, 'orbit' AS tag`）
3. `native_pg_plugin_connection_pool_reused`：同一 url 多次 execute，验证 sqlx 连接池复用

## 关键文件

- 插件 SDK：`crates/orbit-plugin-api-native`（`NativePlugin` trait + `export_plugin!` 宏）
- 宿主加载器：`crates/orbit-plugin-native`（`NativePluginHandle::load` → dlopen + 调用）
- 本示例：`src/lib.rs`
