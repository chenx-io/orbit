//! The **single authoritative description** of Orbit's own syntax: the prompt body and the "anti-drift tests" share the same data.
//!
//! ## Why this module exists
//!
//! When writing request definitions, the model's most common mistake is not "can't think of content" but **using another product's syntax**:
//! Postman's `{{$uuid.v4}}` / `{{$randomInt}}`, other tools' `{{$timestamp.ms}}`,
//! instinctively writing `comparator:"eq"` in assertions (our official name is `equal`),
//! or inventing APIs like `pm.response.to.have.status(200)` that this sandbox does not have.
//! These mistakes **raise no error** — a wrong dynamic value is sent verbatim, a wrong comparator quietly degrades to `equal` —
//! so the only defense is "feeding the real syntax to the model".
//!
//! Writing the syntax into the prompt alone is not enough: the prompt is hand-written and drifts the moment the implementation changes (this project already got burned:
//! the prompt taught `{{$uuid.v4}}` and `comparator:"eq"`). So every list here is
//! **pinned to the real implementation** by tests (see `tests` below):
//!
//! | list | what proves it correct |
//! |---|---|
//! | [`DYNAMIC_CATALOG`] | `orbit_dynamic::resolve` must resolve every token |
//! | [`COMPARATOR_NAMES`] | `orbit_assertion::types::Comparator::from_name` must recognize every name |
//! | [`ASSERTION_EXAMPLES_ZH`] / [`ASSERTION_EXAMPLES_EN`] | must deserialize into `orbit_config::Check` |
//! | `PM_API_*_ZH` / `PM_API_*_EN` / [`SCRIPT_EXAMPLES_ZH`] / [`SCRIPT_EXAMPLES_EN`] | must be reachable / runnable in the real QuickJS sandbox |
//!
//! Change the implementation without updating this and the tests go red — the mechanical guarantee that "the model writes our syntax".

use crate::prompt::Language;

/// Dynamic-value catalog: `category` → **all** method names under that category.
///
/// Authoritative implementation: `orbit-dynamic` (`{{$category.method}}` / `{{$category.method(k=v)}}` / `{{$...|pipe}}`).
/// The test assembles each entry into `{{$cat.method}}` and runs it through the real resolver to confirm it is not made up.
pub const DYNAMIC_CATALOG: &[(&str, &[&str])] = &[
    (
        "string",
        &[
            "uuid",
            "alpha",
            "alphanumeric",
            "numeric",
            "hexadecimal",
            "symbol",
            "sample",
        ],
    ),
    ("number", &["int", "float", "hex", "binary", "octal"]),
    ("datatype", &["boolean"]),
    (
        "date",
        &[
            "now",
            "time",
            "timestamp",
            "timestampMs",
            "today",
            "iso",
            "isoDate",
            "year",
            "month",
            "day",
            "hour",
            "minute",
            "second",
            "timeZone",
            "weekday",
            "monthName",
            "offset",
            "past",
            "future",
            "random",
            "between",
            "pastRandom",
            "futureRandom",
        ],
    ),
    (
        "person",
        &[
            "fullName",
            "firstName",
            "lastName",
            "idCard",
            "gender",
            "age",
        ],
    ),
    (
        "internet",
        &[
            "email",
            "url",
            "ip",
            "ipv6",
            "userName",
            "password",
            "domainName",
            "port",
            "httpMethod",
            "userAgent",
        ],
    ),
    ("phone", &["mobile", "number"]),
    (
        "location",
        &[
            "city",
            "state",
            "street",
            "streetAddress",
            "county",
            "secondaryAddress",
            "address",
            "zipCode",
            "country",
            "latitude",
            "longitude",
        ],
    ),
    (
        "commerce",
        &[
            "price",
            "productName",
            "productNameEn",
            "department",
            "productDescription",
            "sku",
        ],
    ),
    ("company", &["name", "catchPhrase", "bs"]),
    (
        "finance",
        &[
            "accountNumber",
            "amount",
            "currencyCode",
            "currencyName",
            "creditCardCVV",
            "transactionType",
        ],
    ),
    (
        "helpers",
        &[
            "arrayElement",
            "fromRegExp",
            "replaceSymbols",
            "slugify",
            "rangeToNumber",
        ],
    ),
    ("image", &["url", "avatar"]),
    ("lorem", &["word", "words", "sentence", "paragraph"]),
    ("color", &["name", "hex", "rgb"]),
    ("food", &["dish", "name", "vegetable", "fruit", "meat"]),
    (
        "vehicle",
        &["manufacturer", "brand", "model", "type", "vin"],
    ),
    ("music", &["artist", "album", "songName", "genre"]),
];

/// Dynamic-value argument and pipe description, Chinese (the usage convention written into the prompt).
pub const DYNAMIC_ARG_HINT_ZH: &str =
    "参数写在括号里、`key=value` 逗号分隔（如 `{{$number.int(min=1,max=9)}}`、\
`{{$string.alphanumeric(length=16)}}`、`{{$date.now(format=yyyy-MM-dd)}}`）；\
`date` 类可用 `tz=` 指定时区；支持多语言的类别可加 `locale=zh|en|ja`；\
`|` 接管道：`{{$person.fullName|toUpperCase}}`（可用管道见下）。";

/// Dynamic-value argument description, English (kept 1:1 with [`DYNAMIC_ARG_HINT_ZH`]).
pub const DYNAMIC_ARG_HINT_EN: &str =
    "Args go in parentheses as `key=value`, comma separated (e.g. `{{$number.int(min=1,max=9)}}`, \
`{{$string.alphanumeric(length=16)}}`, `{{$date.now(format=yyyy-MM-dd)}}`); \
date accepts `tz=`; localized categories accept `locale=zh|en|ja`; \
`|` applies a pipe: `{{$person.fullName|toUpperCase}}` (pipes listed below).";

/// Dynamic-value pipes (`|`) — consistent with `orbit-dynamic`'s `apply_pipe`.
pub const DYNAMIC_PIPES: &[&str] = &["toUpperCase", "toLowerCase", "trim"];

/// The most common complete forms (the prompt gives "copy-paste" examples directly; the test really resolves each one).
pub const DYNAMIC_EXAMPLES: &[&str] = &[
    "{{$string.uuid}}",
    "{{$string.alphanumeric(length=16)}}",
    "{{$number.int(min=1,max=100)}}",
    "{{$date.timestampMs}}",
    "{{$date.now(format=yyyy-MM-dd HH:mm:ss)}}",
    "{{$person.fullName}}",
    "{{$internet.email}}",
    "{{$location.city}}",
    "{{$commerce.price}}",
    "{{$color.hex}}",
];

/// Assertion-comparator **official names** (the `comparator` field may only use these; empty = `equal`).
///
/// Authoritative implementation: `orbit_assertion::types::Comparator::from_name`.
/// Note that forms like `eq` / `contain` are **not** official names (the engine falls back to `equal`, silently changing the assertion's meaning).
pub const COMPARATOR_NAMES: &[&str] = &[
    "equal",
    "not_equal",
    "contains",
    "not_contains",
    "exists",
    "matches",
    "regex",
    "gt",
    "lt",
];

/// Assertion examples, Chinese: both the templates in the prompt and the **input to the deserialization test**.
///
/// Covers all built-in types + the two data assertions db/redis (including `retry` / `extract_var` / `meta`).
pub const ASSERTION_EXAMPLES_ZH: &[&str] = &[
    r#"{"type":"status","value":200}"#,
    r#"{"type":"body_contains","value":"success"}"#,
    r#"{"type":"duration_lt","value":"2s"}"#,
    r#"{"type":"jsonpath","path":"$.data.token","comparator":"not_equal","expected":""}"#,
    r#"{"type":"jmespath","expression":"data.token","comparator":"exists","expected":""}"#,
    r#"{"type":"header","name":"content-type","comparator":"contains","expected":"json"}"#,
    r#"{"type":"regex","pattern":"\"code\":\\s*0"}"#,
    r#"{"type":"size_lt","value":1024}"#,
    r#"{"type":"xpath","path":"//item/id","comparator":"exists","expected":""}"#,
    r#"{"type":"jsonschema","schema":"{\"type\":\"object\"}"}"#,
    r#"{"type":"css_selector","selector":".price","comparator":"exists","expected":""}"#,
    r#"{"type":"db","datasource":"pg-test","sql":"select status from orders where id=${orderId}","target":{"type":"scalar"},"comparator":"equal","expected":"PAID","extract_var":"dbStatus","retry":{"interval_ms":500,"max_attempts":5}}"#,
    r#"{"type":"redis","datasource":"redis-test","command":"GET","args":["order:${orderId}"],"comparator":"exists","expected":""}"#,
    r#"{"type":"status","value":200,"meta":{"name":"登录成功","enabled":true}}"#,
];

/// Assertion examples, English: identical to [`ASSERTION_EXAMPLES_ZH`] except for the sample `meta.name`.
pub const ASSERTION_EXAMPLES_EN: &[&str] = &[
    r#"{"type":"status","value":200}"#,
    r#"{"type":"body_contains","value":"success"}"#,
    r#"{"type":"duration_lt","value":"2s"}"#,
    r#"{"type":"jsonpath","path":"$.data.token","comparator":"not_equal","expected":""}"#,
    r#"{"type":"jmespath","expression":"data.token","comparator":"exists","expected":""}"#,
    r#"{"type":"header","name":"content-type","comparator":"contains","expected":"json"}"#,
    r#"{"type":"regex","pattern":"\"code\":\\s*0"}"#,
    r#"{"type":"size_lt","value":1024}"#,
    r#"{"type":"xpath","path":"//item/id","comparator":"exists","expected":""}"#,
    r#"{"type":"jsonschema","schema":"{\"type\":\"object\"}"}"#,
    r#"{"type":"css_selector","selector":".price","comparator":"exists","expected":""}"#,
    r#"{"type":"db","datasource":"pg-test","sql":"select status from orders where id=${orderId}","target":{"type":"scalar"},"comparator":"equal","expected":"PAID","extract_var":"dbStatus","retry":{"interval_ms":500,"max_attempts":5}}"#,
    r#"{"type":"redis","datasource":"redis-test","command":"GET","args":["order:${orderId}"],"comparator":"exists","expected":""}"#,
    r#"{"type":"status","value":200,"meta":{"name":"login ok","enabled":true}}"#,
];

/// Value extraction for `db` assertions (`target.type`) and a description of `redis` commands, Chinese.
pub const DATA_ASSERTION_HINT_ZH: &str = "`db` 的 `target` 取 `{\"type\":\"row_count\"|\"scalar\"|\"cell\"|\"row\"|\"json_path\"}`\
（`cell` 带 `row`/`column`，`json_path` 带 `row`/`path`）；`sql` 支持 `${var}` 插值。\
`redis` 的 `command` 如 `GET`/`HGET`/`EXISTS`/`TTL`/`LLEN`，`args` 为字符串数组。\
两者都支持 `retry{interval_ms,max_attempts,timeout_ms}`（等待异步落库）、`extract_var`（把实际值写进变量）、`hard`。";

/// Value extraction for `db` assertions and `redis` command notes, English (1:1 with [`DATA_ASSERTION_HINT_ZH`]).
pub const DATA_ASSERTION_HINT_EN: &str = "`db.target` is one of `{\"type\":\"row_count\"|\"scalar\"|\"cell\"|\"row\"|\"json_path\"}` \
(`cell` carries `row`/`column`, `json_path` carries `row`/`path`); `sql` supports `${var}` interpolation. \
`redis.command` is e.g. `GET`/`HGET`/`EXISTS`/`TTL`/`LLEN` and `args` is a string array. \
Both support `retry{interval_ms,max_attempts,timeout_ms}` (waiting for async persistence), `extract_var` (write the actual value into a variable) and `hard`.";

/// `pm.*` member list (**available to pre-request scripts**), Chinese: `path` → description.
/// The test probes each one for existence in the real sandbox.
pub const PM_API_PRE_ZH: &[(&str, &str)] = &[
    ("pm.request.url", "当前请求 URL（可改写）"),
    ("pm.request.method", "当前请求方法（可改写）"),
    (
        "pm.request.headers",
        "请求头，支持 get/set/upsert/add/remove/keys",
    ),
    ("pm.request.body", "当前请求体字符串（只读）"),
];

/// `pm.*` member list (**available to pre-request scripts**), English (same paths as [`PM_API_PRE_ZH`]).
pub const PM_API_PRE_EN: &[(&str, &str)] = &[
    ("pm.request.url", "current request URL (writable)"),
    ("pm.request.method", "current request method (writable)"),
    (
        "pm.request.headers",
        "request headers; supports get/set/upsert/add/remove/keys",
    ),
    ("pm.request.body", "current request body string (read-only)"),
];

/// `pm.*` member list (**available to post-response scripts**), Chinese.
pub const PM_API_POST_ZH: &[(&str, &str)] = &[
    ("pm.response.code", "响应状态码（数字）"),
    ("pm.response.status", "同 code"),
    ("pm.response.responseTime", "耗时（毫秒）"),
    ("pm.response.body", "响应体字符串"),
    ("pm.response.raw", "原始响应体"),
    ("pm.response.headers", "响应头，按头名取值"),
    ("pm.response.json", "解析响应体为对象（解析失败返回 null）"),
    ("pm.response.text", "同 body"),
];

/// `pm.*` member list (**available to post-response scripts**), English (same paths as [`PM_API_POST_ZH`]).
pub const PM_API_POST_EN: &[(&str, &str)] = &[
    ("pm.response.code", "response status code (number)"),
    ("pm.response.status", "same as code"),
    ("pm.response.responseTime", "elapsed time (ms)"),
    ("pm.response.body", "response body string"),
    ("pm.response.raw", "raw response body"),
    (
        "pm.response.headers",
        "response headers; looked up by header name",
    ),
    (
        "pm.response.json",
        "parse the response body into an object (null on parse failure)",
    ),
    ("pm.response.text", "same as body"),
];

/// Members available to **both** pre and post, Chinese.
pub const PM_API_BOTH_ZH: &[(&str, &str)] = &[
    ("pm.environment.get", "读环境变量"),
    ("pm.environment.set", "写环境变量（本次请求的 {{}} 插值即可用到）"),
    ("pm.variables.get", "读临时变量（本次请求生命周期）"),
    ("pm.variables.set", "写临时变量（优先于环境变量）"),
    ("pm.globals.get", "Postman 兼容别名，等同 environment"),
    ("pm.collectionVariables.get", "Postman 兼容别名，等同 environment"),
    ("pm.secret.get", "只读环境密钥（只给值，不给名字以外的信息）"),
    ("pm.test", "记录一条脚本侧断言 `pm.test(name, fn)`"),
    (
        "pm.expect",
        "断言表达式：`to.equal/eql/contain`（`include` 同义）、`be.true/false/null/undefined/ok`、\
`be.a(type)` / `be.an(type)`（`'array'` / `'null'` 可判）、`have.property`；取反写 `to.not.xxx(...)` \
或 `not.to.xxx(...)`",
    ),
    ("pm.crypto.md5", "哈希：md5/sha1/sha224/sha256/sha384/sha512/sha3/ripemd160"),
    ("pm.crypto.hmac", "HMAC：`pm.crypto.hmac('sha256', secret, data)`"),
    ("pm.crypto.hmacBase64", "HMAC 并输出 Base64"),
    ("pm.crypto.base64Encode", "Base64 编码"),
    ("pm.crypto.base64Decode", "Base64 解码"),
    ("pm.crypto.aesEncrypt", "AES：`{data, key, iv, mode, outputType}`（outputType: hex/base64/string）"),
    ("pm.crypto.aesDecrypt", "AES 解密（参数同 aesEncrypt）"),
    ("pm.crypto.getRandomValues", "随机字节（CryptoJS WordArray）"),
    ("pm.utf8.encode", "字符串 → 字节串（decode 反向）"),
    ("pm.hex.encode", "字节串 ↔ hex"),
    ("pm.b64.encode", "字节串 ↔ Base64"),
    ("pm.info.requestName", "当前接口名"),
    ("pm.iterationData", "数据驱动行（当前始终为空串，Postman 兼容占位）"),
];

/// Members available to **both** pre and post, English (same paths as [`PM_API_BOTH_ZH`]).
pub const PM_API_BOTH_EN: &[(&str, &str)] = &[
    ("pm.environment.get", "read an environment variable"),
    ("pm.environment.set", "write an environment variable (usable by this request's {{}} interpolation right away)"),
    ("pm.variables.get", "read a temporary variable (lifetime of this request)"),
    ("pm.variables.set", "write a temporary variable (takes precedence over environment variables)"),
    ("pm.globals.get", "Postman-compatible alias, equivalent to environment"),
    ("pm.collectionVariables.get", "Postman-compatible alias, equivalent to environment"),
    ("pm.secret.get", "read-only environment secret (value only, no information beyond the name)"),
    ("pm.test", "record a script-side assertion `pm.test(name, fn)`"),
    (
        "pm.expect",
        "assertion expressions: `to.equal/eql/contain` (`include` is synonymous), `be.true/false/null/undefined/ok`, \
`be.a(type)` / `be.an(type)` (`'array'` / `'null'` testable), `have.property`; negation is `to.not.xxx(...)` \
or `not.to.xxx(...)`",
    ),
    ("pm.crypto.md5", "hash: md5/sha1/sha224/sha256/sha384/sha512/sha3/ripemd160"),
    ("pm.crypto.hmac", "HMAC: `pm.crypto.hmac('sha256', secret, data)`"),
    ("pm.crypto.hmacBase64", "HMAC with Base64 output"),
    ("pm.crypto.base64Encode", "Base64 encode"),
    ("pm.crypto.base64Decode", "Base64 decode"),
    ("pm.crypto.aesEncrypt", "AES: `{data, key, iv, mode, outputType}` (outputType: hex/base64/string)"),
    ("pm.crypto.aesDecrypt", "AES decrypt (same arguments as aesEncrypt)"),
    ("pm.crypto.getRandomValues", "random bytes (CryptoJS WordArray)"),
    ("pm.utf8.encode", "string -> byte string (decode is the inverse)"),
    ("pm.hex.encode", "byte string <-> hex"),
    ("pm.b64.encode", "byte string <-> Base64"),
    ("pm.info.requestName", "current request name"),
    ("pm.iterationData", "data-driven row (currently always an empty string; Postman-compatible placeholder)"),
];

/// Modules a script may `require()` (a Postman-compatible whitelist); `CryptoJS` is available globally,
/// plus Node-style `url` / `querystring`.
pub const REQUIRE_LIBS: &[&str] = &[
    "crypto-js",
    "lodash",
    "moment",
    "uuid",
    "atob",
    "btoa",
    "url",
    "querystring",
];

/// Runnable script examples: `(phase, code)`, where `phase` is `"preresolve"` (before interpolation) / `"pre"` (after interpolation) / `"post"`.
///
/// Both the templates in the prompt and something the test **really executes once** (a wrong API name gets caught).
pub const SCRIPT_EXAMPLES_ZH: &[(&str, &str)] = &[
    (
        "preresolve",
        "// 插值前：生成随机数据写入环境变量，本次请求的 {{var}} 立刻能用\n\
pm.environment.set('orderNo', 'ORD-' + Date.now() + '-' + Math.floor(Math.random() * 1000));\n\
pm.environment.set('traceId', require('uuid').v4());\n\
pm.environment.set('amount', (Math.random() * 1000).toFixed(2));",
    ),
    (
        "pre",
        "// 插值后：此时 pm.request.body 已经是最终报文，可以安全地算签名\n\
var sign = pm.crypto.hmac('sha256', pm.secret.get('APP_SECRET'), pm.request.body);\n\
pm.request.headers.upsert({ key: 'X-Signature', value: sign });\n\
pm.request.headers.set('X-Trace-Id', pm.variables.get('traceId') || '');",
    ),
    (
        "post",
        "// 断言 + 提取变量供后续请求使用\n\
pm.test('status 200', function () { pm.expect(pm.response.code).to.equal(200); });\n\
var data = pm.response.json();\n\
pm.test('业务码为 0', function () { pm.expect(data.code).to.equal(0); });\n\
pm.environment.set('token', data.data.token);",
    ),
];

/// Runnable script examples, English: the JavaScript bodies are identical to [`SCRIPT_EXAMPLES_ZH`];
/// only their comments (and one `pm.test` label) are translated.
pub const SCRIPT_EXAMPLES_EN: &[(&str, &str)] = &[
    (
        "preresolve",
        "// before interpolation: write random data into environment variables; this request's {{var}} is usable immediately\n\
pm.environment.set('orderNo', 'ORD-' + Date.now() + '-' + Math.floor(Math.random() * 1000));\n\
pm.environment.set('traceId', require('uuid').v4());\n\
pm.environment.set('amount', (Math.random() * 1000).toFixed(2));",
    ),
    (
        "pre",
        "// after interpolation: pm.request.body now holds the final payload, so it is safe to compute the signature\n\
var sign = pm.crypto.hmac('sha256', pm.secret.get('APP_SECRET'), pm.request.body);\n\
pm.request.headers.upsert({ key: 'X-Signature', value: sign });\n\
pm.request.headers.set('X-Trace-Id', pm.variables.get('traceId') || '');",
    ),
    (
        "post",
        "// assertions + extract a variable for later requests\n\
pm.test('status 200', function () { pm.expect(pm.response.code).to.equal(200); });\n\
var data = pm.response.json();\n\
pm.test('business code is 0', function () { pm.expect(data.code).to.equal(0); });\n\
pm.environment.set('token', data.data.token);",
    ),
];

/// Render the "syntax reference" block (assembled into the system prompt).
///
/// All lists come from this module's constants (rather than a second hand-written copy), so the prompt and the tests are inherently consistent.
pub fn syntax_reference(lang: Language) -> String {
    match lang {
        Language::Zh => zh(),
        Language::En => en(),
    }
}

fn catalog_lines(sep: &str) -> String {
    DYNAMIC_CATALOG
        .iter()
        .map(|(cat, methods)| format!("- `{cat}`{sep}{}", methods.join(" / ")))
        .collect::<Vec<_>>()
        .join("\n")
}

fn zh() -> String {
    let mut out = String::from("## 语法参考（Orbit 自有语法，必须照此书写）\n");
    out.push_str(
        "以下每条都与引擎实现一一对应。**不要**套用 Postman / Apifox / JMeter 的写法——\
名字不同的地方一律以本表为准；写错的动态值会被**原样发出**，写错的比较器会**悄悄退化**。\n\n",
    );

    out.push_str("### 1. 变量与插值（都在执行时由引擎统一插值，不要自己预替换）\n");
    out.push_str("- `{{name}}`：环境/全局变量。**缺失时原样保留**（不会变成空串）。\n");
    out.push_str("- `${name}` / `${env:name}`：同上（脚本与导入场景）。\n");
    out.push_str(
        "- `${=expr}`：算术表达式（`+ - * / %`、括号、变量），如 `${=loop.index * 10}`。\n",
    );
    out.push_str("- `{{$category.method}}`：动态值，见下一节。\n");
    out.push_str("- 前置脚本只有**一个有序列表** `preActions`，里面有一个**内置「插值」节点**\
（`{\"type\":\"interpolate\"}`，不可删除/修改）：**排在它之前**的动作＝**插值前**\
（`pm.environment.set` / `pm.variables.set` 写的变量**本次请求的 URL/Header/Body 立刻能用**，适合造随机数据）；\
**排在它之后**的动作＝**插值后**（`pm.request.*` 拿到的就是**最终报文**，适合签名/加密，\
它写入的内容不会再被插值）。\
改**单个**动作用 `update_action` / `insert_action` / `delete_action` / `move_action`\
（`list` + `index` 定位，先用 `get_request` 拿到下标），**不要整表重发 `preActions`**；\
要复用公共脚本可以引用脚本库库项 `{\"type\":\"ref\",\"library_id\":\"…\"}`\
（id 先用 `list_action_templates` 取；改库项 → 所有引用处一起生效）。\n\n");

    out.push_str("### 2. 动态值（真实目录，共 ");
    out.push_str(&DYNAMIC_CATALOG.len().to_string());
    out.push_str(" 类）\n");
    out.push_str(&catalog_lines(": "));
    out.push('\n');
    out.push_str(DYNAMIC_ARG_HINT_ZH);
    out.push('\n');
    out.push_str(&format!(
        "可用管道：{}。\n",
        DYNAMIC_PIPES
            .iter()
            .map(|p| format!("`{p}`"))
            .collect::<Vec<_>>()
            .join("、")
    ));
    out.push_str("常用写法（可直接照抄）：\n");
    for example in DYNAMIC_EXAMPLES {
        out.push_str(&format!("- `{example}`\n"));
    }
    out.push_str(
        "**反例（这些写法不存在，写了会被原样发出）**：`{{$uuid.v4}}`、`{{$timestamp.ms}}`、\
`{{$randomInt}}`、`{{$randomUUID}}`、`{{$guid}}`。\n\n",
    );

    out.push_str("### 3. 断言（`assertions` 数组，一条一个对象）\n");
    out.push_str(
        "内置类型：`status` / `body_contains` / `duration_lt` / `jsonpath` / `jmespath` / \
`regex` / `size_lt` / `xpath` / `jsonschema` / `header` / `css_selector`；\
数据断言：`db` / `redis`。\n",
    );
    out.push_str(
        "**时长必须带单位**（`\"2s\"` / `\"800ms\"`）：`duration_lt` 的纯数字会被当成**秒**。\n",
    );
    out.push_str(&format!(
        "`comparator` 只能取：{}（留空 = `equal`）；`status` / `size_lt` 的 `value` 是数字，\
其余字段都是字符串。可选 `meta{{\"name\",\"enabled\"}}`。\n",
        COMPARATOR_NAMES
            .iter()
            .map(|c| format!("`{c}`"))
            .collect::<Vec<_>>()
            .join(" / ")
    ));
    out.push_str(DATA_ASSERTION_HINT_ZH);
    out.push('\n');
    out.push_str("样板（照抄形状即可）：\n");
    for ex in ASSERTION_EXAMPLES_ZH {
        out.push_str(&format!("- `{ex}`\n"));
    }
    out.push('\n');

    out.push_str("### 4. 脚本（Postman 兼容沙箱，但**只有下列成员存在**）\n");
    out.push_str(
        "内置插值节点**之前 / 之后**的请求侧脚本共用同一套成员（`pm.request` 两处都可读写，\
区别只是拿到的是模板还是最终报文）：\n",
    );
    for (path, note) in PM_API_PRE_ZH {
        out.push_str(&format!("- `{path}`：{note}\n"));
    }
    out.push_str("后置脚本常用：\n");
    for (path, note) in PM_API_POST_ZH {
        out.push_str(&format!("- `{path}`：{note}\n"));
    }
    out.push_str("两者皆可：\n");
    for (path, note) in PM_API_BOTH_ZH {
        out.push_str(&format!("- `{path}`：{note}\n"));
    }
    out.push_str(&format!(
        "`require()` 白名单：{}（`CryptoJS` 全局直接可用）。\
`console.log/error/warn` 可打日志。脚本顶层用 `const` / `let` / `class` 没问题（每次运行独立作用域）。\n",
        REQUIRE_LIBS
            .iter()
            .map(|l| format!("`{l}`"))
            .collect::<Vec<_>>()
            .join("、")
    ));
    out.push_str("示例：\n");
    for (phase, code) in SCRIPT_EXAMPLES_ZH {
        out.push_str(&format!(
            "```js\n// {}脚本\n{code}\n```\n",
            phase_label_zh(phase)
        ));
    }
    out
}

/// Chinese phase labels for the script examples (1:1 with the `phase` values in [`SCRIPT_EXAMPLES_ZH`])
fn phase_label_zh(phase: &str) -> &'static str {
    match phase {
        "preresolve" => "插值前（放在内置插值节点之前）",
        "pre" => "插值后（放在内置插值节点之后）",
        _ => "后置",
    }
}

/// English phase labels for the script examples (1:1 with the `phase` values in [`SCRIPT_EXAMPLES_EN`])
fn phase_label_en(phase: &str) -> &'static str {
    match phase {
        "preresolve" => "before the built-in interpolation node",
        "pre" => "after the built-in interpolation node",
        _ => "post-response",
    }
}

fn en() -> String {
    let mut out = String::from("## Syntax reference (Orbit's own syntax — follow it exactly)\n");
    out.push_str("Every item below maps 1:1 to the engine implementation. Do **not** use Postman / Apifox / \
JMeter spellings: an unknown dynamic value is sent **literally**, and an unknown comparator **silently \
degrades**. \n\n");

    out.push_str("### 1. Variables & interpolation (always done by the engine at run time — never pre-substitute)\n");
    out.push_str("- `{{name}}`: environment/global variable. **Missing ones stay literal** (never become an empty string).\n");
    out.push_str("- `${name}` / `${env:name}`: same (script/import style).\n");
    out.push_str(
        "- `${=expr}`: arithmetic (`+ - * / %`, parens, variables), e.g. `${=loop.index * 10}`.\n",
    );
    out.push_str("- `{{$category.method}}`: dynamic value (next section).\n");
    out.push_str("- Pre-request scripts live in **one ordered list** `preActions` that contains a **built-in interpolation node** (`{\"type\":\"interpolate\"}`, not deletable/editable): actions placed **above it** run **before interpolation**, so the variables they set (`pm.environment.set` / `pm.variables.set`) **are usable in this very request's URL/headers/body** (good for random data); actions placed **below it** run once interpolation is done, so `pm.request.*` already holds the **final payload** (good for signing/encryption) and anything they write is sent verbatim. Edit a **single** action with `update_action` / `insert_action` / `delete_action` / `move_action` (`list` + `index`, read `get_request` first) instead of resending the whole list. Reusable scripts can be referenced from the script library (`{\"type\":\"ref\",\"library_id\":\"…\"}`, ids from `list_action_templates`) — editing a library item updates every request that uses it.\n");

    out.push_str(&format!(
        "### 2. Dynamic values (real catalog, {} categories)\n",
        DYNAMIC_CATALOG.len()
    ));
    out.push_str(&catalog_lines(": "));
    out.push('\n');
    out.push_str(DYNAMIC_ARG_HINT_EN);
    out.push('\n');
    out.push_str(&format!(
        "Pipes: {}.\n",
        DYNAMIC_PIPES
            .iter()
            .map(|p| format!("`{p}`"))
            .collect::<Vec<_>>()
            .join(", ")
    ));
    out.push_str("Common forms (copy as-is):\n");
    for example in DYNAMIC_EXAMPLES {
        out.push_str(&format!("- `{example}`\n"));
    }
    out.push_str(
        "**Invalid (do not use)**: `{{$uuid.v4}}`, `{{$timestamp.ms}}`, `{{$randomInt}}`, \
`{{$randomUUID}}`, `{{$guid}}` — they are sent literally.\n\n",
    );

    out.push_str("### 3. Assertions (`assertions` array, one object each)\n");
    out.push_str("Built-ins: `status` / `body_contains` / `duration_lt` / `jsonpath` / `jmespath` / `regex` / \
`size_lt` / `xpath` / `jsonschema` / `header` / `css_selector`; data assertions: `db` / `redis`.\n");
    out.push_str("**Durations need an explicit unit** (`\"2s\"` / `\"800ms\"`) — a bare number means **seconds**.\n");
    out.push_str(&format!(
        "`comparator` accepts only: {} (empty = `equal`); `status` / `size_lt` `value` is a number, \
everything else is a string. Optional `meta{{\"name\",\"enabled\"}}`.\n",
        COMPARATOR_NAMES
            .iter()
            .map(|c| format!("`{c}`"))
            .collect::<Vec<_>>()
            .join(" / ")
    ));
    out.push_str(DATA_ASSERTION_HINT_EN);
    out.push('\n');
    out.push_str("Templates:\n");
    for ex in ASSERTION_EXAMPLES_EN {
        out.push_str(&format!("- `{ex}`\n"));
    }
    out.push('\n');

    out.push_str("### 4. Scripts (Postman-compatible sandbox, but **only these members exist**)\n");
    out.push_str(
        "Scripts above/below the built-in interpolation node share the same members \
(`pm.request` is read/write in both; only the payload they see differs — template vs final):\n",
    );
    for (path, note) in PM_API_PRE_EN {
        out.push_str(&format!("- `{path}`: {note}\n"));
    }
    out.push_str("Post-response:\n");
    for (path, note) in PM_API_POST_EN {
        out.push_str(&format!("- `{path}`: {note}\n"));
    }
    out.push_str("Both:\n");
    for (path, note) in PM_API_BOTH_EN {
        out.push_str(&format!("- `{path}`: {note}\n"));
    }
    out.push_str(&format!(
        "`require()` whitelist: {} (global `CryptoJS` is available without require). \
`console.log/error/warn` are available; top-level `const` / `let` / `class` are fine (fresh scope per run).\n",
        REQUIRE_LIBS
            .iter()
            .map(|l| format!("`{l}`"))
            .collect::<Vec<_>>()
            .join(", ")
    ));
    out.push_str("Examples:\n");
    for (phase, code) in SCRIPT_EXAMPLES_EN {
        out.push_str(&format!(
            "```js\n// {} script\n{code}\n```\n",
            phase_label_en(phase)
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every dynamic-value token written in the prompt must really be resolvable by the engine.
    ///
    /// This pins "the prompt" to `orbit-dynamic`: remove/rename a method from the catalog and the test goes red immediately.
    #[test]
    fn every_documented_dynamic_token_resolves() {
        for (category, methods) in DYNAMIC_CATALOG {
            assert!(!methods.is_empty(), "{category} category must not be empty");
            for method in *methods {
                let token = format!("{{{{${category}.{method}}}}}");
                let resolved = orbit_dynamic::resolve(&token).unwrap_or_else(|e| {
                    panic!("the {token} in the prompt cannot be resolved: {e}")
                });
                assert!(
                    !resolved.contains("{{$"),
                    "{token} was not substituted (meaning it is absent from the catalog): {resolved}"
                );
            }
        }
    }

    /// The "copy-paste" complete examples in the prompt must also really resolve.
    #[test]
    fn every_documented_dynamic_example_resolves() {
        for example in DYNAMIC_EXAMPLES {
            let resolved = orbit_dynamic::resolve(example)
                .unwrap_or_else(|e| panic!("the prompt example {example} cannot be resolved: {e}"));
            assert!(
                !resolved.contains("{{$"),
                "{example} was not substituted: {resolved}"
            );
        }
    }

    /// Pipe names must also really exist.
    #[test]
    fn every_documented_pipe_works() {
        for pipe in DYNAMIC_PIPES {
            let token = format!("{{{{$string.alpha|{pipe}}}}}");
            orbit_dynamic::resolve(&token)
                .unwrap_or_else(|e| panic!("{token} cannot be resolved: {e}"));
        }
    }

    /// The official comparator names must be recognized by `Comparator::from_name`; and no name from the implementation may be missing.
    #[test]
    fn every_documented_comparator_is_recognized() {
        use orbit_assertion::types::Comparator;
        for name in COMPARATOR_NAMES {
            assert!(
                Comparator::from_name(name).is_some(),
                "the implementation does not recognize the prompt's comparator `{name}` (it would silently degrade to equal)"
            );
        }
        for name in Comparator::NAMES {
            assert!(
                COMPARATOR_NAMES.contains(name),
                "the implementation supports comparator `{name}`, but the prompt does not mention it (the model can only guess)"
            );
        }
    }

    /// The assertion templates must deserialize into the domain model — otherwise even copying them verbatim fails.
    #[test]
    fn every_documented_assertion_example_deserializes() {
        let examples = ASSERTION_EXAMPLES_ZH
            .iter()
            .chain(ASSERTION_EXAMPLES_EN.iter());
        for example in examples.clone() {
            serde_json::from_str::<orbit_config::Check>(example).unwrap_or_else(|e| {
                panic!("the assertion template cannot be parsed: {example}\nerror: {e}")
            });
        }
        // The zh/en templates must stay in lockstep (only the sample `meta.name` differs)
        assert_eq!(
            ASSERTION_EXAMPLES_ZH.len(),
            ASSERTION_EXAMPLES_EN.len(),
            "the zh/en assertion template lists must have the same length"
        );
        // Check the comparator field too: the names used in the templates must be official names
        for example in examples {
            let value: serde_json::Value = serde_json::from_str(example).unwrap();
            if let Some(c) = value.get("comparator").and_then(|v| v.as_str()) {
                assert!(
                    !c.is_empty() && COMPARATOR_NAMES.contains(&c),
                    "the comparator `{c}` in the template is not an official name"
                );
            }
        }
    }

    fn probe(paths: &[&str], pre: bool) -> Result<(), String> {
        let sandbox = orbit_js::JsSandbox::new().map_err(|e| e.to_string())?;
        let list = paths
            .iter()
            .map(|p| format!("\"{p}\""))
            .collect::<Vec<_>>()
            .join(",");
        let script = format!(
            "var __paths = [{list}];\n\
             var __missing = [];\n\
             __paths.forEach(function (p) {{\n\
               var cur = globalThis, parts = p.split('.');\n\
               for (var i = 0; i < parts.length; i++) {{\n\
                 if (cur == null || typeof cur[parts[i]] === 'undefined') {{ __missing.push(p); return; }}\n\
                 cur = cur[parts[i]];\n\
               }}\n\
             }});\n\
             if (__missing.length) {{ throw new Error('missing: ' + __missing.join(', ')); }}"
        );
        let mut req = orbit_js::RequestContext {
            url: "https://probe.test/x".into(),
            method: "POST".into(),
            headers: std::collections::HashMap::new(),
            body: "{}".into(),
            raw: "{}".into(),
        };
        if pre {
            let outcome = sandbox.run_pre_request(&script, &mut req, None, None);
            return if outcome.success {
                Ok(())
            } else {
                Err(outcome.error.unwrap_or_default())
            };
        }
        let resp = orbit_js::ResponseContext {
            status: 200,
            body: "{}".into(),
            headers: std::collections::HashMap::new(),
            duration_ms: 1,
            raw: "{}".into(),
            decoded: None,
        };
        let outcome = sandbox.run_post_response(&script, &resp, None, None);
        if outcome.success {
            Ok(())
        } else {
            Err(outcome.error.unwrap_or_default())
        }
    }

    /// The `pm.*` members listed in the prompt must exist in the real sandbox
    /// (guarding against "writing Postman forms like `pm.response.to.have.status` that this sandbox does not have").
    #[test]
    fn every_documented_pm_member_exists() {
        let pre_zh: Vec<&str> = PM_API_PRE_ZH
            .iter()
            .chain(PM_API_BOTH_ZH.iter())
            .map(|(p, _)| *p)
            .collect();
        let pre_en: Vec<&str> = PM_API_PRE_EN
            .iter()
            .chain(PM_API_BOTH_EN.iter())
            .map(|(p, _)| *p)
            .collect();
        assert_eq!(
            pre_zh, pre_en,
            "the zh/en pre-request member lists must stay in sync"
        );
        probe(&pre_zh, true)
            .unwrap_or_else(|e| panic!("probing pre-request-side pm members failed: {e}"));
        let post_zh: Vec<&str> = PM_API_POST_ZH
            .iter()
            .chain(PM_API_BOTH_ZH.iter())
            .map(|(p, _)| *p)
            .collect();
        let post_en: Vec<&str> = PM_API_POST_EN
            .iter()
            .chain(PM_API_BOTH_EN.iter())
            .map(|(p, _)| *p)
            .collect();
        assert_eq!(
            post_zh, post_en,
            "the zh/en post-response member lists must stay in sync"
        );
        probe(&post_zh, false)
            .unwrap_or_else(|e| panic!("probing post-response-side pm members failed: {e}"));
    }

    /// The script examples in the prompt must really run (a wrong API name / argument shape gets caught).
    #[test]
    fn every_documented_script_example_runs() {
        let sandbox = orbit_js::JsSandbox::new().unwrap();
        for (phase, code) in SCRIPT_EXAMPLES_ZH
            .iter()
            .chain(SCRIPT_EXAMPLES_EN.iter())
            .copied()
        {
            // Before- and after-interpolation share the same sandbox entry point (`run_pre_request`); only the data source differs
            let outcome = if phase == "pre" || phase == "preresolve" {
                let mut req = orbit_js::RequestContext {
                    url: "https://probe.test/x".into(),
                    method: "POST".into(),
                    headers: std::collections::HashMap::new(),
                    body: "{\"a\":1}".into(),
                    raw: "{\"a\":1}".into(),
                };
                sandbox.run_pre_request(code, &mut req, None, None)
            } else {
                let resp = orbit_js::ResponseContext {
                    status: 200,
                    body: "{\"code\":0,\"data\":{\"token\":\"t\"}}".into(),
                    headers: std::collections::HashMap::new(),
                    duration_ms: 1,
                    raw: String::new(),
                    decoded: None,
                };
                sandbox.run_post_response(code, &resp, None, None)
            };
            assert!(
                outcome.success,
                "the {phase} example script failed to run: {:?}\n{code}",
                outcome.error
            );
        }
    }

    /// Modules in the `require()` whitelist must really load.
    #[test]
    fn every_documented_require_lib_loads() {
        let sandbox = orbit_js::JsSandbox::new().unwrap();
        for lib in REQUIRE_LIBS {
            let code = format!("require('{lib}');");
            let mut req = orbit_js::RequestContext {
                url: "https://probe.test/x".into(),
                method: "GET".into(),
                headers: std::collections::HashMap::new(),
                body: String::new(),
                raw: String::new(),
            };
            let outcome = sandbox.run_pre_request(&code, &mut req, None, None);
            assert!(
                outcome.success,
                "require('{lib}') failed to load: {:?}",
                outcome.error
            );
        }
    }

    /// The syntax reference grows with the catalog — set a cap to keep the system prompt from quietly bloating (exceeding it means trim or make it on-demand).
    #[test]
    fn syntax_reference_stays_within_budget() {
        // The cap is language-specific on purpose: English runs roughly 30% longer than Chinese for the
        // same content, so a single cap would either be too loose for zh or trip on en.
        let cap = |lang: Language| match lang {
            Language::Zh => 9_000,
            Language::En => 11_000,
        };
        for lang in [Language::Zh, Language::En] {
            let len = syntax_reference(lang).chars().count();
            let cap = cap(lang);
            // `cargo test -- --nocapture` shows the real size, useful for estimating token cost
            println!("syntax_reference({lang:?}) = {len} chars (cap {cap})");
            assert!(
                len <= cap,
                "the syntax reference for {lang:?} is already {len} chars, over budget {cap} (time to trim or make it on-demand)"
            );
        }
    }

    /// Both prompt languages must carry the syntax reference (and include the key lists).
    #[test]
    fn prompt_embeds_syntax_reference() {
        for section in [zh(), en()] {
            assert!(section.contains("$string.uuid"));
            assert!(section.contains("not_equal"));
            assert!(section.contains("pm.crypto.hmac"));
            assert!(section.contains("crypto-js"));
        }
        let zh_text = zh();
        assert!(
            zh_text.contains("语法参考"),
            "the Chinese block must keep its title"
        );
        assert!(
            zh_text.contains("断言"),
            "the Chinese block must cover assertions"
        );
        assert!(
            zh_text.contains("插值前"),
            "the Chinese block must explain when scripts run"
        );
        assert!(
            zh_text.contains("preActions"),
            "the Chinese block must name the pre-action list field"
        );
        assert!(
            zh_text.contains("interpolate"),
            "the Chinese block must name the built-in interpolation node"
        );
        assert!(
            en().contains("preActions"),
            "the English block must name the pre-action list field"
        );
        assert!(
            en().contains("interpolate"),
            "the English block must name the built-in interpolation node"
        );
        // The English block must not leak Chinese labels or full-width punctuation. A plain `is_ascii()`
        // check would be too strict (the block legitimately uses an em dash), so test the CJK ranges.
        let has_cjk = |s: &str| {
            s.chars()
                .any(|c| matches!(c as u32, 0x3000..=0x303F | 0x4E00..=0x9FFF | 0xFF00..=0xFFEF))
        };
        assert!(
            !has_cjk(&en()),
            "the English syntax reference must contain no Chinese label or full-width punctuation"
        );
        let en_text = en();
        assert!(en_text.contains("Syntax reference"));
    }
}
