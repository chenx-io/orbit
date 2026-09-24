# pg-query（PostgreSQL 查询协议插件）

Orbit 协议插件：通过 PG wire protocol 执行 SQL 查询。

- 声明协议 id：`pg`
- `execute`：PG wire 握手（协议 3.0）+ 认证（trust / cleartext / md5 / **SCRAM-SHA-256**）+ 简单查询 + 结果解析
- 结果以 JSON 返回：`{ "columns": [...], "rows": [...], "rowCount": N, "commandTag": "..." }`
- 网络经宿主 host-transport（宿主统一超时），插件不持有 socket

## 连接与消息参数

`connectionConfigSchema`（新建集合时由动态表单渲染）：

| 字段 | 类型 | 说明 |
|---|---|---|
| `host` | string | 服务器地址，默认 `127.0.0.1` |
| `port` | integer | 端口，默认 `5432` |
| `username` | string | 用户名，默认 `postgres` |
| `password` | string | 密码 |
| `database` | string | 数据库名，默认 `postgres` |

`requestConfigSchema`（消息参数）：`sql`（text，要执行的 SQL）。

> SQL 也可直接放在请求体 payload（UTF-8），payload 优先于 `options.sql`。

## 构建与打包

```bash
rustup target add wasm32-wasip2
cargo build --target wasm32-wasip2 --release   # 产物 target/wasm32-wasip2/release/pg_query.wasm
```

将 `pg_query.wasm` 放入 `com.example.pg/`（与 `manifest.json`），压缩为 zip 后在插件管理页安装。
或运行 `build.sh` / `build.ps1` 一键构建 + 打包。

## 验证

### 1. 宿主加载 + 模拟 PG 端到端（无需真实 PG）

orbit 内置集成测试 `crates/orbit-plugin/tests/pg_plugin.rs`：

```bash
cargo test -p orbit-plugin --test pg_plugin
```

该测试：加载 `pg_query.wasm` → 启动一个最小 PG wire 模拟 server（**SCRAM-SHA-256 认证**，服务端验证 ClientProof + 返回 ServerSignature）→
通过动态注册表 `build_client_by_id("pg")` 执行 `SELECT 1` →
断言返回 `{ "columns":["?column?"], "rows":[{"?column?":1}], "rowCount":1 }`。

它覆盖了：宿主加载识别 → 握手 → **SCRAM-SHA-256 认证（Proof 校验 + ServerSignature 验证）** → 查询 → RowDescription/DataRow 解析 → JSON 结果。

### 2. 真实 PostgreSQL（可选）

若本机有 PostgreSQL，可经 UI 验证：

1. 插件管理页「安装 zip 包」上传 `com.example.pg.zip`
2. 新建集合 → 协议选择 `pg` → 连接表单填 host/port/username/password/database
3. 新建请求 → 消息参数填 `SELECT 1`（或请求体 payload 放 SQL）→ 发送
4. 响应区应返回 JSON：`{"columns":["?column?"],"rows":[{"?column?":1}],"rowCount":1}`

> 认证支持 trust / cleartext / md5 / **SCRAM-SHA-256**（PG 15+ 默认认证方式，已支持）。

## 约束

- 单次 execute 新建连接并关闭（连接复用二期优化）
- 文本协议结果按 JSON 尝试解析（纯数字→number，其余→string），NULL → null
- 默认读写超时 30s（宿主统一）
