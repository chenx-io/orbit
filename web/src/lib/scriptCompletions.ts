// Pre/post script editor: autocompletion data source.
// Shares one set of API facts with scriptsReference.ts (engine capabilities live in orbit-js).
// `scope` controls in which script kinds (pre / post) a completion is visible.

import type { Completion } from "@codemirror/autocomplete";
import type { Locale } from "@/data/types";

export type ScriptScope = "pre" | "post";

/** A value that is either locale-independent or carries one variant per locale. */
export type Localizable = string | Record<Locale, string>;

/** Build a localized pair (keeps the tables below compact). */
const L = (zh: string, en: string): Record<Locale, string> => ({
  "zh-CN": zh,
  "en-US": en,
});

/** Resolve a `Localizable` for the active locale. */
function pick(value: Localizable, locale: Locale): string {
  return typeof value === "string" ? value : value[locale];
}

export interface ScriptCompletionDef {
  /**
   * Completion display text, also used as the inserted text when no `apply` is set.
   *
   * This stays a plain string: it drives prefix matching and insertion, so it is functional
   * rather than display-only.
   */
  label: string;
  /**
   * Localized display label overriding `label` in the completion list.
   *
   * Only needed for the human-readable snippet names, whose `label` also drives prefix matching
   * (a typed latin prefix never matches a Chinese label anyway) and insertion when `apply` is absent.
   */
  labelL10n?: Localizable;
  /** Signature / description shown to the right */
  detail: Localizable;
  /** Full description shown at the bottom (optional) */
  info?: Localizable;
  /** Code to insert (used by multi-line snippets) */
  apply?: Localizable;
  scope: ScriptScope[];
  type?: Completion["type"];
}

const ALL: ScriptScope[] = ["pre", "post"];

/** Top-level completions: global objects / keywords / common snippets */
export const TOP_LEVEL_DEFS: ScriptCompletionDef[] = [
  {
    label: "pm",
    detail: L("全局脚本对象", "global script object"),
    info: L(
      "pm.request / pm.response / pm.environment / pm.variables / pm.secret / pm.crypto / pm.test 等",
      "pm.request / pm.response / pm.environment / pm.variables / pm.secret / pm.crypto / pm.test and more",
    ),
    scope: ALL,
    type: "variable",
  },
  {
    label: "CryptoJS",
    detail: L(
      "内置官方 crypto-js 库（与 require('crypto-js') 同一实例）",
      "bundled upstream crypto-js library (the same instance as require('crypto-js'))",
    ),
    scope: ALL,
    type: "variable",
  },
  {
    label: "console.log",
    detail: L(
      "console.log(...) — 输出到脚本日志",
      "console.log(...) — write to the script log",
    ),
    apply: "console.log(",
    scope: ALL,
    type: "function",
  },
  {
    label: "console.warn",
    detail: "console.warn(...)",
    apply: "console.warn(",
    scope: ALL,
    type: "function",
  },
  {
    label: "console.error",
    detail: "console.error(...)",
    apply: "console.error(",
    scope: ALL,
    type: "function",
  },
  {
    label: "require",
    detail: L(
      "require(name) — 内置库：crypto-js / lodash / moment / uuid / atob / btoa / url / querystring",
      "require(name) — bundled modules: crypto-js / lodash / moment / uuid / atob / btoa / url / querystring",
    ),
    apply: "require('')",
    scope: ALL,
    type: "function",
  },
  {
    label: "URL",
    detail: L(
      "全局 URL：new URL(input[, base]) — 解析 / 构造 URL（.searchParams 处理 query）",
      "global URL: new URL(input[, base]) — parse / build a URL (.searchParams handles the query)",
    ),
    apply: "new URL(pm.request.url)",
    scope: ALL,
    type: "class",
  },
  {
    label: "URLSearchParams",
    detail: L(
      "全局 URLSearchParams：get / getAll / set / append / delete / has / toString",
      "global URLSearchParams: get / getAll / set / append / delete / has / toString",
    ),
    apply: "new URLSearchParams('')",
    scope: ALL,
    type: "class",
  },
  {
    label: "var",
    detail: L("var 声明", "var declaration"),
    scope: ALL,
    type: "keyword",
  },
  {
    label: "const",
    detail: L("const 声明", "const declaration"),
    scope: ALL,
    type: "keyword",
  },
  {
    label: "let",
    detail: L("let 声明", "let declaration"),
    scope: ALL,
    type: "keyword",
  },
  {
    label: "function",
    detail: L("function 声明", "function declaration"),
    apply: "function () {\n  \n}",
    scope: ALL,
    type: "keyword",
  },
  {
    label: "return",
    detail: L("return 语句", "return statement"),
    scope: ALL,
    type: "keyword",
  },
  // ── Common snippets ──
  {
    label: "签名并写入 Header（HMAC-SHA256）",
    labelL10n: L(
      "签名并写入 Header（HMAC-SHA256）",
      "Sign & write header (HMAC-SHA256)",
    ),
    detail: L(
      "前置脚本：body + 密钥 → 签名 → upsert header",
      "Pre-request script: body + secret → signature → upsert header",
    ),
    apply: L(
      `var body = pm.request.body.raw;
var secret = pm.secret.get('SECRET');   // 在「环境管理 → 密钥」配置

var sign = pm.crypto.hmac('sha256', secret, body);
pm.request.headers.upsert({ key: 'X-Signature', value: sign });
console.log('签名:', sign);`,
      `var body = pm.request.body.raw;
var secret = pm.secret.get('SECRET');   // configure under "Environments → Secrets"

var sign = pm.crypto.hmac('sha256', secret, body);
pm.request.headers.upsert({ key: 'X-Signature', value: sign });
console.log('signature:', sign);`,
    ),
    scope: ["pre"],
    type: "snippet",
  },
  {
    label: "JWT HS256 签名",
    labelL10n: L("JWT HS256 签名", "JWT HS256 signing"),
    detail: L(
      "前置脚本：生成 JWT 并写入 Authorization 头",
      "Pre-request script: build a JWT and write the Authorization header",
    ),
    apply: L(
      `var secret = pm.secret.get('JWT_SECRET');

function b64url(s) {
  return CryptoJS.enc.Base64url.stringify(CryptoJS.enc.Utf8.parse(s));
}
var header = b64url(JSON.stringify({ alg: 'HS256', typ: 'JWT' }));
var payload = b64url(JSON.stringify({
  sub: 'user_001',
  iat: Math.floor(Date.now() / 1000),
  exp: Math.floor(Date.now() / 1000) + 3600,
}));
var token = header + '.' + payload + '.' + CryptoJS.enc.Base64url.stringify(
  CryptoJS.HmacSHA256(header + '.' + payload, secret)
);
pm.request.headers.upsert({ key: 'Authorization', value: 'Bearer ' + token });
console.log('JWT:', token);`,
      `var secret = pm.secret.get('JWT_SECRET');

function b64url(s) {
  return CryptoJS.enc.Base64url.stringify(CryptoJS.enc.Utf8.parse(s));
}
var header = b64url(JSON.stringify({ alg: 'HS256', typ: 'JWT' }));
var payload = b64url(JSON.stringify({
  sub: 'user_001',
  iat: Math.floor(Date.now() / 1000),
  exp: Math.floor(Date.now() / 1000) + 3600,
}));
var token = header + '.' + payload + '.' + CryptoJS.enc.Base64url.stringify(
  CryptoJS.HmacSHA256(header + '.' + payload, secret)
);
pm.request.headers.upsert({ key: 'Authorization', value: 'Bearer ' + token });
console.log('JWT:', token);`,
    ),
    scope: ["pre"],
    type: "snippet",
  },
  {
    label: "断言示例（pm.test）",
    labelL10n: L("断言示例（pm.test）", "Assertion example (pm.test)"),
    detail: L(
      "后置脚本：状态码 / 字段断言",
      "Post-response script: status code / field assertions",
    ),
    apply: L(
      `pm.test('状态码为 200', function () {
  pm.expect(pm.response.code).to.equal(200);
});
pm.test('返回含 token', function () {
  pm.expect(pm.response.json().token).to.be.ok;
});`,
      `pm.test('status code is 200', function () {
  pm.expect(pm.response.code).to.equal(200);
});
pm.test('response contains a token', function () {
  pm.expect(pm.response.json().token).to.be.ok;
});`,
    ),
    scope: ["post"],
    type: "snippet",
  },
  {
    label: "提取 token 到环境",
    labelL10n: L("提取 token 到环境", "Extract token into environment"),
    detail: L(
      "后置脚本：从响应提取变量，后续请求用 {{token}} 引用",
      "Post-response script: extract a variable from the response and reference it later via {{token}}",
    ),
    apply: L(
      `var data = pm.response.json();
pm.environment.set('token', data.token);
console.log('token:', data.token);`,
      `var data = pm.response.json();
pm.environment.set('token', data.token);
console.log('token:', data.token);`,
    ),
    scope: ["post"],
    type: "snippet",
  },
  {
    label: "断言响应体字段",
    labelL10n: L("断言响应体字段", "Assert response body field"),
    detail: L(
      "后置脚本：通用字段断言",
      "Post-response script: generic field assertion",
    ),
    apply: L(
      `pm.test('响应体字段', function () {
  var data = pm.response.json();
  pm.expect(data).to.be.an('object');
});`,
      `pm.test('response body field', function () {
  var data = pm.response.json();
  pm.expect(data).to.be.an('object');
});`,
    ),
    scope: ["post"],
    type: "snippet",
  },
  {
    label: "处理 query 参数",
    labelL10n: L("处理 query 参数", "Handle query params"),
    detail: L(
      "前置脚本：读取 / 修改 URL query 参数（pm.request 便捷方法）",
      "Pre-request script: read / modify URL query params (pm.request convenience methods)",
    ),
    apply: L(
      `var uid = pm.request.getQueryParam('uid');   // 不存在返回 null
pm.request.setQueryParam('page', 2);             // 覆盖
pm.request.addQueryParam('tag', 'a');            // 追加
pm.request.removeQueryParam('debug');            // 删除全部同名
console.log('uid =', uid, 'url =', pm.request.url);`,
      `var uid = pm.request.getQueryParam('uid');   // returns null when absent
pm.request.setQueryParam('page', 2);             // overwrite
pm.request.addQueryParam('tag', 'a');            // append
pm.request.removeQueryParam('debug');            // remove every occurrence
console.log('uid =', uid, 'url =', pm.request.url);`,
    ),
    scope: ["pre"],
    type: "snippet",
  },
];

/** Member completions grouped by "dot path": keys such as `pm.request.headers`, `CryptoJS.enc` */
export const SCRIPT_COMPLETION_GROUPS: Record<string, ScriptCompletionDef[]> = {
  pm: [
    {
      label: "request",
      detail: L(
        "请求对象（前置可写 / 后置只读）：url / method / headers / body",
        "request object (writable in pre, read-only in post): url / method / headers / body",
      ),
      scope: ALL,
      type: "property",
    },
    {
      label: "response",
      detail: L(
        "响应对象（仅后置、只读）：code / headers / body / json() / text()",
        "response object (post only, read-only): code / headers / body / json() / text()",
      ),
      scope: ["post"],
      type: "property",
    },
    {
      label: "environment",
      detail: L(
        "持久变量：get / set / unset（set 合并回当前环境）",
        "persistent variables: get / set / unset (set merges back into the active environment)",
      ),
      scope: ALL,
      type: "property",
    },
    {
      label: "variables",
      detail: L(
        "本次请求临时变量：get / set（不落盘）",
        "temporary variables for this request: get / set (never persisted)",
      ),
      scope: ALL,
      type: "property",
    },
    {
      label: "secret",
      detail: L("只读密钥：get(name)", "read-only secrets: get(name)"),
      scope: ALL,
      type: "property",
    },
    {
      label: "globals",
      detail: L(
        "Postman 别名 → 映射到环境变量",
        "Postman alias → mapped to environment variables",
      ),
      scope: ALL,
      type: "property",
    },
    {
      label: "collectionVariables",
      detail: L(
        "Postman 别名 → 映射到环境变量",
        "Postman alias → mapped to environment variables",
      ),
      scope: ALL,
      type: "property",
    },
    {
      label: "crypto",
      detail: L(
        "哈希 / HMAC / AES / Base64（Rust 实现）",
        "hash / HMAC / AES / Base64 (Rust implementation)",
      ),
      scope: ALL,
      type: "property",
    },
    {
      label: "test",
      detail: L(
        "pm.test(name, fn) — 定义断言（后置脚本）",
        "pm.test(name, fn) — define an assertion (post-response script)",
      ),
      apply: L(
        "pm.test('名称', function () {\n  \n})",
        "pm.test('name', function () {\n  \n})",
      ),
      scope: ["post"],
      type: "function",
    },
    {
      label: "expect",
      detail: L(
        "pm.expect(value) — 断言匹配器（配合 pm.test）",
        "pm.expect(value) — assertion matchers (used with pm.test)",
      ),
      scope: ["post"],
      type: "function",
    },
    {
      label: "utf8",
      detail: L(
        "字节串工具：encode / decode",
        "byte-string helper: encode / decode",
      ),
      scope: ALL,
      type: "property",
    },
    {
      label: "hex",
      detail: L(
        "字节串工具：encode / decode",
        "byte-string helper: encode / decode",
      ),
      scope: ALL,
      type: "property",
    },
    {
      label: "b64",
      detail: L(
        "字节串工具：encode / decode",
        "byte-string helper: encode / decode",
      ),
      scope: ALL,
      type: "property",
    },
  ],
  "pm.request": [
    {
      label: "url",
      detail: L("请求 URL（可改写）", "request URL (rewritable)"),
      scope: ALL,
      type: "property",
    },
    {
      label: "method",
      detail: L("请求方法（可改写）", "request method (rewritable)"),
      scope: ALL,
      type: "property",
    },
    {
      label: "headers",
      detail: L(
        "请求头：get / set / upsert / add / remove",
        "request headers: get / set / upsert / add / remove",
      ),
      scope: ALL,
      type: "property",
    },
    {
      label: "body",
      detail: L("请求体：raw / mode", "request body: raw / mode"),
      scope: ALL,
      type: "property",
    },
    {
      label: "getQueryParam",
      detail: L(
        "getQueryParam(name) → query 参数值（不存在返回 null）",
        "getQueryParam(name) → query param value (null when absent)",
      ),
      apply: "pm.request.getQueryParam('')",
      scope: ALL,
      type: "function",
    },
    {
      label: "getQueryParams",
      detail: L(
        "getQueryParams(name) → 同名参数值数组",
        "getQueryParams(name) → array of values for the same name",
      ),
      apply: "pm.request.getQueryParams('')",
      scope: ALL,
      type: "function",
    },
    {
      label: "hasQueryParam",
      detail: L(
        "hasQueryParam(name) → 是否存在",
        "hasQueryParam(name) → whether it exists",
      ),
      apply: "pm.request.hasQueryParam('')",
      scope: ALL,
      type: "function",
    },
    {
      label: "setQueryParam",
      detail: L(
        "setQueryParam(name, value) — 覆盖同名（保留首个）",
        "setQueryParam(name, value) — overwrite the same name (keeps the first)",
      ),
      apply: "pm.request.setQueryParam('', )",
      scope: ALL,
      type: "function",
    },
    {
      label: "addQueryParam",
      detail: L(
        "addQueryParam(name, value) — 追加同名",
        "addQueryParam(name, value) — append the same name",
      ),
      apply: "pm.request.addQueryParam('', )",
      scope: ALL,
      type: "function",
    },
    {
      label: "removeQueryParam",
      detail: L(
        "removeQueryParam(name) — 删除全部同名",
        "removeQueryParam(name) — remove every occurrence of the name",
      ),
      apply: "pm.request.removeQueryParam('')",
      scope: ALL,
      type: "function",
    },
    {
      label: "query",
      detail: L(
        "query 便捷对象：get / getAll / has / set / add / remove / toObject / toString",
        "query convenience object: get / getAll / has / set / add / remove / toObject / toString",
      ),
      scope: ALL,
      type: "property",
    },
  ],
  "pm.request.query": [
    {
      label: "get",
      detail: L(
        "get(name) → 参数值（不存在返回 null）",
        "get(name) → param value (null when absent)",
      ),
      apply: "pm.request.query.get('')",
      scope: ALL,
      type: "function",
    },
    {
      label: "getAll",
      detail: L(
        "getAll(name) → 同名参数值数组",
        "getAll(name) → array of values for the same name",
      ),
      apply: "pm.request.query.getAll('')",
      scope: ALL,
      type: "function",
    },
    {
      label: "has",
      detail: L("has(name) → 是否存在", "has(name) → whether it exists"),
      apply: "pm.request.query.has('')",
      scope: ALL,
      type: "function",
    },
    {
      label: "set",
      detail: L("set(name, value) — 覆盖", "set(name, value) — overwrite"),
      apply: "pm.request.query.set('', )",
      scope: ALL,
      type: "function",
    },
    {
      label: "add",
      detail: L("add(name, value) — 追加", "add(name, value) — append"),
      apply: "pm.request.query.add('', )",
      scope: ALL,
      type: "function",
    },
    {
      label: "remove",
      detail: L(
        "remove(name) — 删除全部同名",
        "remove(name) — remove every occurrence of the name",
      ),
      apply: "pm.request.query.remove('')",
      scope: ALL,
      type: "function",
    },
    {
      label: "toObject",
      detail: L("toObject() → 参数对象", "toObject() → param object"),
      apply: "pm.request.query.toObject()",
      scope: ALL,
      type: "function",
    },
    {
      label: "toString",
      detail: L(
        "toString() → query 字符串（不含 ?）",
        "toString() → query string (without ?)",
      ),
      apply: "pm.request.query.toString()",
      scope: ALL,
      type: "function",
    },
  ],
  "pm.request.headers": [
    {
      label: "get",
      detail: L("get(name) → 读取请求头", "get(name) → read a request header"),
      apply: "pm.request.headers.get('')",
      scope: ALL,
      type: "function",
    },
    {
      label: "set",
      detail: L(
        "set(key, value) 或 set({key, value}) — 写入/覆盖",
        "set(key, value) or set({key, value}) — write / overwrite",
      ),
      apply: "pm.request.headers.set('', '')",
      scope: ALL,
      type: "function",
    },
    {
      label: "upsert",
      detail: L(
        "upsert({key, value}) — 写入/覆盖（推荐）",
        "upsert({key, value}) — write / overwrite (recommended)",
      ),
      apply: "pm.request.headers.upsert({ key: '', value: '' })",
      scope: ALL,
      type: "function",
    },
    {
      label: "add",
      detail: L("add({key, value}) — 追加", "add({key, value}) — append"),
      apply: "pm.request.headers.add({ key: '', value: '' })",
      scope: ALL,
      type: "function",
    },
    {
      label: "remove",
      detail: L("remove(name) — 删除", "remove(name) — delete"),
      apply: "pm.request.headers.remove('')",
      scope: ALL,
      type: "function",
    },
  ],
  "pm.request.body": [
    {
      label: "raw",
      detail: L(
        "请求体原始内容（已解析变量，可改写）",
        "raw body content (variables resolved, rewritable)",
      ),
      scope: ALL,
      type: "property",
    },
    {
      label: "mode",
      detail: L(
        "请求体模式（json / xml / raw / ...）",
        "body mode (json / xml / raw / ...)",
      ),
      scope: ALL,
      type: "property",
    },
  ],
  "pm.response": [
    {
      label: "code",
      detail: L("状态码，如 200", "status code, e.g. 200"),
      scope: ["post"],
      type: "property",
    },
    {
      label: "status",
      detail: L("状态文本，如 'OK'", "status text, e.g. 'OK'"),
      scope: ["post"],
      type: "property",
    },
    {
      label: "responseTime",
      detail: L("耗时(ms)", "elapsed time (ms)"),
      scope: ["post"],
      type: "property",
    },
    {
      label: "headers",
      detail: L("响应头（按名索引）", "response headers (indexed by name)"),
      scope: ["post"],
      type: "property",
    },
    {
      label: "body",
      detail: L("原始响应体文本", "raw response body text"),
      scope: ["post"],
      type: "property",
    },
    {
      label: "text",
      detail: L("text() → 原始文本", "text() → raw text"),
      apply: "pm.response.text()",
      scope: ["post"],
      type: "function",
    },
    {
      label: "json",
      detail: L(
        "json() → 解析 JSON（失败返回 null）",
        "json() → parse JSON (null on failure)",
      ),
      apply: "pm.response.json()",
      scope: ["post"],
      type: "function",
    },
  ],
  "pm.environment": [
    {
      label: "get",
      detail: L(
        "get(name) → 读取环境变量（回退密钥）",
        "get(name) → read an env var (falls back to secrets)",
      ),
      apply: "pm.environment.get('')",
      scope: ALL,
      type: "function",
    },
    {
      label: "set",
      detail: L(
        "set(name, value) — 写入（合并回当前环境）",
        "set(name, value) — write (merges back into the active environment)",
      ),
      apply: "pm.environment.set('', )",
      scope: ALL,
      type: "function",
    },
    {
      label: "unset",
      detail: L("unset(name) — 删除", "unset(name) — delete"),
      apply: "pm.environment.unset('')",
      scope: ALL,
      type: "function",
    },
  ],
  "pm.variables": [
    {
      label: "get",
      detail: L(
        "get(name) → 读取临时变量（回退环境变量）",
        "get(name) → read a temp variable (falls back to env vars)",
      ),
      apply: "pm.variables.get('')",
      scope: ALL,
      type: "function",
    },
    {
      label: "set",
      detail: L(
        "set(name, value) — 本次请求内有效，不落盘",
        "set(name, value) — valid for this request only, never persisted",
      ),
      apply: "pm.variables.set('', )",
      scope: ALL,
      type: "function",
    },
  ],
  "pm.secret": [
    {
      label: "get",
      detail: L(
        "get(name) → 读取密钥（只读）",
        "get(name) → read a secret (read-only)",
      ),
      apply: "pm.secret.get('')",
      scope: ALL,
      type: "function",
    },
  ],
  "pm.globals": [
    {
      label: "get",
      detail: L(
        "get(name) — 环境变量别名",
        "get(name) — environment variable alias",
      ),
      apply: "pm.globals.get('')",
      scope: ALL,
      type: "function",
    },
    {
      label: "set",
      detail: L(
        "set(name, value) — 环境变量别名",
        "set(name, value) — environment variable alias",
      ),
      apply: "pm.globals.set('', )",
      scope: ALL,
      type: "function",
    },
    {
      label: "remove",
      detail: "remove(name)",
      apply: "pm.globals.remove('')",
      scope: ALL,
      type: "function",
    },
  ],
  "pm.collectionVariables": [
    {
      label: "get",
      detail: L(
        "get(name) — 环境变量别名",
        "get(name) — environment variable alias",
      ),
      apply: "pm.collectionVariables.get('')",
      scope: ALL,
      type: "function",
    },
    {
      label: "set",
      detail: L(
        "set(name, value) — 环境变量别名",
        "set(name, value) — environment variable alias",
      ),
      apply: "pm.collectionVariables.set('', )",
      scope: ALL,
      type: "function",
    },
  ],
  "pm.crypto": [
    {
      label: "md5",
      detail: L("md5(str) → 32 位 hex", "md5(str) → 32-char hex"),
      apply: "pm.crypto.md5('')",
      scope: ALL,
      type: "function",
    },
    {
      label: "sha1",
      detail: L("sha1(str) → 40 位 hex", "sha1(str) → 40-char hex"),
      apply: "pm.crypto.sha1('')",
      scope: ALL,
      type: "function",
    },
    {
      label: "sha224",
      detail: L("sha224(str) → 56 位 hex", "sha224(str) → 56-char hex"),
      apply: "pm.crypto.sha224('')",
      scope: ALL,
      type: "function",
    },
    {
      label: "sha256",
      detail: L("sha256(str) → 64 位 hex", "sha256(str) → 64-char hex"),
      apply: "pm.crypto.sha256('')",
      scope: ALL,
      type: "function",
    },
    {
      label: "sha384",
      detail: L("sha384(str) → 96 位 hex", "sha384(str) → 96-char hex"),
      apply: "pm.crypto.sha384('')",
      scope: ALL,
      type: "function",
    },
    {
      label: "sha512",
      detail: L("sha512(str) → 128 位 hex", "sha512(str) → 128-char hex"),
      apply: "pm.crypto.sha512('')",
      scope: ALL,
      type: "function",
    },
    {
      label: "sha3",
      detail: L(
        "sha3(str, bits) — SHA3-224/256/384/512",
        "sha3(str, bits) — SHA3-224/256/384/512",
      ),
      apply: "pm.crypto.sha3('', 512)",
      scope: ALL,
      type: "function",
    },
    {
      label: "ripemd160",
      detail: L("ripemd160(str) → 40 位 hex", "ripemd160(str) → 40-char hex"),
      apply: "pm.crypto.ripemd160('')",
      scope: ALL,
      type: "function",
    },
    {
      label: "hmac",
      detail: L(
        "hmac(algo, key, msg) → hex（algo: md5/sha1/sha256/sha512/...）",
        "hmac(algo, key, msg) → hex (algo: md5/sha1/sha256/sha512/...)",
      ),
      apply: "pm.crypto.hmac('sha256', , )",
      scope: ALL,
      type: "function",
    },
    {
      label: "hmacBase64",
      detail: L(
        "hmacBase64(algo, key, msg) → base64",
        "hmacBase64(algo, key, msg) → base64",
      ),
      apply: "pm.crypto.hmacBase64('sha256', , )",
      scope: ALL,
      type: "function",
    },
    {
      label: "base64Encode",
      detail: "base64Encode(str)",
      apply: "pm.crypto.base64Encode('')",
      scope: ALL,
      type: "function",
    },
    {
      label: "base64Decode",
      detail: "base64Decode(b64)",
      apply: "pm.crypto.base64Decode('')",
      scope: ALL,
      type: "function",
    },
    {
      label: "aesEncrypt",
      detail: "aesEncrypt({data, key, iv, mode, outputType})",
      apply:
        "pm.crypto.aesEncrypt({ data: , key: , iv: , mode: CryptoJS.mode.CBC, outputType: 'hex' })",
      scope: ALL,
      type: "function",
    },
    {
      label: "aesDecrypt",
      detail: "aesDecrypt({data, key, iv, mode, outputType})",
      apply:
        "pm.crypto.aesDecrypt({ data: , key: , iv: , mode: CryptoJS.mode.CBC, outputType: 'string' })",
      scope: ALL,
      type: "function",
    },
    {
      label: "getRandomValues",
      detail: L(
        "getRandomValues(nBytes) → n 字节随机 hex",
        "getRandomValues(nBytes) → n bytes of random hex",
      ),
      apply: "pm.crypto.getRandomValues(16)",
      scope: ALL,
      type: "function",
    },
  ],
  "pm.utf8": [
    {
      label: "encode",
      detail: L(
        "encode(str) → UTF-8 字节串",
        "encode(str) → UTF-8 byte string",
      ),
      apply: "pm.utf8.encode('')",
      scope: ALL,
      type: "function",
    },
    {
      label: "decode",
      detail: L(
        "decode(bytes) → 原字符串",
        "decode(bytes) → the original string",
      ),
      apply: "pm.utf8.decode()",
      scope: ALL,
      type: "function",
    },
  ],
  "pm.hex": [
    {
      label: "encode",
      detail: L("encode(bytes) → hex 字符串", "encode(bytes) → hex string"),
      apply: "pm.hex.encode()",
      scope: ALL,
      type: "function",
    },
    {
      label: "decode",
      detail: L("decode(hexStr) → 字节串", "decode(hexStr) → byte string"),
      apply: "pm.hex.decode('')",
      scope: ALL,
      type: "function",
    },
  ],
  "pm.b64": [
    {
      label: "encode",
      detail: L(
        "encode(bytes) → base64 字符串",
        "encode(bytes) → base64 string",
      ),
      apply: "pm.b64.encode()",
      scope: ALL,
      type: "function",
    },
    {
      label: "decode",
      detail: L("decode(b64) → 字节串", "decode(b64) → byte string"),
      apply: "pm.b64.decode('')",
      scope: ALL,
      type: "function",
    },
  ],
  console: [
    {
      label: "log",
      detail: "console.log(...)",
      apply: "console.log()",
      scope: ALL,
      type: "function",
    },
    {
      label: "warn",
      detail: "console.warn(...)",
      apply: "console.warn()",
      scope: ALL,
      type: "function",
    },
    {
      label: "error",
      detail: "console.error(...)",
      apply: "console.error()",
      scope: ALL,
      type: "function",
    },
  ],
  CryptoJS: [
    {
      label: "MD5",
      detail: "CryptoJS.MD5(msg)",
      apply: "CryptoJS.MD5('')",
      scope: ALL,
      type: "function",
    },
    {
      label: "SHA1",
      detail: "CryptoJS.SHA1(msg)",
      apply: "CryptoJS.SHA1('')",
      scope: ALL,
      type: "function",
    },
    {
      label: "SHA256",
      detail: "CryptoJS.SHA256(msg)",
      apply: "CryptoJS.SHA256('')",
      scope: ALL,
      type: "function",
    },
    {
      label: "SHA512",
      detail: "CryptoJS.SHA512(msg)",
      apply: "CryptoJS.SHA512('')",
      scope: ALL,
      type: "function",
    },
    {
      label: "SHA3",
      detail: "CryptoJS.SHA3(msg, {outputLength})",
      apply: "CryptoJS.SHA3('', { outputLength: 256 })",
      scope: ALL,
      type: "function",
    },
    {
      label: "HmacSHA256",
      detail: "CryptoJS.HmacSHA256(msg, key)",
      apply: "CryptoJS.HmacSHA256(, )",
      scope: ALL,
      type: "function",
    },
    {
      label: "HmacMD5",
      detail: "CryptoJS.HmacMD5(msg, key)",
      apply: "CryptoJS.HmacMD5(, )",
      scope: ALL,
      type: "function",
    },
    {
      label: "HmacSHA1",
      detail: "CryptoJS.HmacSHA1(msg, key)",
      apply: "CryptoJS.HmacSHA1(, )",
      scope: ALL,
      type: "function",
    },
    {
      label: "AES",
      detail: L(
        "CryptoJS.AES.encrypt / decrypt（口令或原始 key）",
        "CryptoJS.AES.encrypt / decrypt (passphrase or raw key)",
      ),
      scope: ALL,
      type: "property",
    },
    {
      label: "DES",
      detail: L(
        "CryptoJS.DES.encrypt / decrypt",
        "CryptoJS.DES.encrypt / decrypt",
      ),
      scope: ALL,
      type: "property",
    },
    {
      label: "TripleDES",
      detail: L(
        "CryptoJS.TripleDES.encrypt / decrypt",
        "CryptoJS.TripleDES.encrypt / decrypt",
      ),
      scope: ALL,
      type: "property",
    },
    {
      label: "enc",
      detail: L(
        "编码器：Hex / Utf8 / Latin1 / Base64 / Base64url / Utf16",
        "encoders: Hex / Utf8 / Latin1 / Base64 / Base64url / Utf16",
      ),
      scope: ALL,
      type: "property",
    },
    {
      label: "mode",
      detail: L(
        "模式：CBC / ECB / CFB / OFB / CTR",
        "modes: CBC / ECB / CFB / OFB / CTR",
      ),
      scope: ALL,
      type: "property",
    },
    {
      label: "pad",
      detail: L(
        "填充：Pkcs7 / NoPadding / ZeroPadding / ...",
        "padding: Pkcs7 / NoPadding / ZeroPadding / ...",
      ),
      scope: ALL,
      type: "property",
    },
  ],
  "CryptoJS.enc": [
    {
      label: "Hex",
      detail: L("Hex.parse / stringify", "Hex.parse / stringify"),
      scope: ALL,
      type: "property",
    },
    {
      label: "Utf8",
      detail: L("Utf8.parse / stringify", "Utf8.parse / stringify"),
      scope: ALL,
      type: "property",
    },
    {
      label: "Latin1",
      detail: L("Latin1.parse / stringify", "Latin1.parse / stringify"),
      scope: ALL,
      type: "property",
    },
    {
      label: "Utf16",
      detail: L("Utf16.parse / stringify", "Utf16.parse / stringify"),
      scope: ALL,
      type: "property",
    },
    {
      label: "Utf16LE",
      detail: L("Utf16LE.parse / stringify", "Utf16LE.parse / stringify"),
      scope: ALL,
      type: "property",
    },
    {
      label: "Base64",
      detail: L("Base64.parse / stringify", "Base64.parse / stringify"),
      scope: ALL,
      type: "property",
    },
    {
      label: "Base64url",
      detail: L(
        "Base64url.parse / stringify（JWT）",
        "Base64url.parse / stringify (JWT)",
      ),
      scope: ALL,
      type: "property",
    },
  ],
  "CryptoJS.mode": [
    {
      label: "CBC",
      detail: L("默认模式", "default mode"),
      scope: ALL,
      type: "property",
    },
    {
      label: "ECB",
      detail: L("ECB 模式", "ECB mode"),
      scope: ALL,
      type: "property",
    },
    {
      label: "CFB",
      detail: L("CFB 模式", "CFB mode"),
      scope: ALL,
      type: "property",
    },
    {
      label: "OFB",
      detail: L("OFB 模式", "OFB mode"),
      scope: ALL,
      type: "property",
    },
    {
      label: "CTR",
      detail: L("CTR 模式", "CTR mode"),
      scope: ALL,
      type: "property",
    },
  ],
  "CryptoJS.pad": [
    {
      label: "Pkcs7",
      detail: L("默认填充", "default padding"),
      scope: ALL,
      type: "property",
    },
    {
      label: "NoPadding",
      detail: L("不填充", "no padding"),
      scope: ALL,
      type: "property",
    },
    {
      label: "ZeroPadding",
      detail: L("零填充", "zero padding"),
      scope: ALL,
      type: "property",
    },
    {
      label: "AnsiX923",
      detail: L("AnsiX923 填充", "AnsiX923 padding"),
      scope: ALL,
      type: "property",
    },
    {
      label: "Iso10126",
      detail: L("ISO 10126 填充", "ISO 10126 padding"),
      scope: ALL,
      type: "property",
    },
    {
      label: "Iso97971",
      detail: L("ISO/IEC 9797-1 填充", "ISO/IEC 9797-1 padding"),
      scope: ALL,
      type: "property",
    },
  ],
};

/** Convert a def into a CodeMirror Completion for the given locale */
export function defToCompletion(
  def: ScriptCompletionDef,
  locale: Locale,
): Completion {
  const apply = def.apply === undefined ? undefined : pick(def.apply, locale);
  return {
    label: def.labelL10n ? pick(def.labelL10n, locale) : def.label,
    detail: pick(def.detail, locale),
    ...(def.info ? { info: pick(def.info, locale) } : {}),
    ...(apply ? { apply } : {}),
    type: def.type ?? "property",
  };
}

/** Filter completions by dot path + typed prefix + scope (reused by the editor and tests). */
export function filterCompletions(
  base: string,
  typed: string,
  scope: ScriptScope,
  locale: Locale,
): Completion[] {
  const defs = SCRIPT_COMPLETION_GROUPS[base] ?? [];
  return defs
    .filter((d) => d.scope.includes(scope) && d.label.startsWith(typed))
    .map((d) => {
      // Member completion: when `apply` starts with the current dot path prefix, strip the prefix.
      // Otherwise, selecting `encode` after `pm.b64.` would let CodeMirror replace the text at the
      // cursor with the whole `pm.b64.encode(...)`, producing `pm.b64.pm.b64.encode(...)`.
      const raw = d.apply === undefined ? undefined : pick(d.apply, locale);
      const apply =
        raw && raw.startsWith(`${base}.`) ? raw.slice(base.length + 1) : raw;
      return defToCompletion({ ...d, apply }, locale);
    });
}

/** Top-level completions (after scope filtering). */
export function filterTopLevel(
  typed: string,
  scope: ScriptScope,
  locale: Locale,
): Completion[] {
  return TOP_LEVEL_DEFS.filter(
    (d) => d.scope.includes(scope) && d.label.startsWith(typed),
  ).map((d) => defToCompletion(d, locale));
}
