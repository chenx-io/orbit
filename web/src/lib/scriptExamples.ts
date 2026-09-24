import type { Locale } from "@/data/types";

/** Script kind: pre-request / post-response */
export type ScriptKind = "pre" | "post";

export interface ScriptExample {
  /** Stable id */
  id: string;
  /** Example name (per locale) */
  label: Record<Locale, string>;
  /**
   * Script code to insert (per locale).
   *
   * The snippets are inserted straight into the user's editor, so their comments are user-visible
   * content and are localized as well — Chinese users keep Chinese comments.
   */
  code: Record<Locale, string>;
}

export interface ScriptExampleCategory {
  /** Stable id */
  id: string;
  /** Category name (per locale) */
  label: Record<Locale, string>;
  /** Applicable script kinds: pre / post / both */
  scope: ScriptKind[];
  examples: ScriptExample[];
}

/**
 * Common pre-request / post-response script examples.
 *
 * `scope` splits the pre/post lists; clicking an example appends it to the script editor.
 */
export const SCRIPT_EXAMPLE_CATEGORIES: ScriptExampleCategory[] = [
  {
    id: "variable",
    label: { "zh-CN": "环境变量与密钥", "en-US": "Environment & Secrets" },
    scope: ["pre", "post"],
    examples: [
      {
        id: "get-env",
        label: {
          "zh-CN": "读取环境变量 / 密钥",
          "en-US": "Read env var / secret",
        },
        code: {
          "zh-CN": `// 读取环境变量（解析顺序：脚本写入 → 环境变量 → 密钥）
var token = pm.environment.get('token');

// 显式只读取密钥（api_key / token 等，密钥只读、不可写入）
var apiKey = pm.secret.get('API_KEY');`,
          "en-US": `// Read an environment variable (resolution order: script writes → environment → secrets)
var token = pm.environment.get('token');

// Read a secret explicitly (api_key / token etc.; secrets are read-only and cannot be written)
var apiKey = pm.secret.get('API_KEY');`,
        },
      },
      {
        id: "set-env",
        label: { "zh-CN": "设置环境变量", "en-US": "Set env variable" },
        code: {
          "zh-CN": `// 写入环境变量，运行结束后自动合并回当前激活环境，后续可用 {{name}} 引用
pm.environment.set('token', pm.response.json().token);`,
          "en-US": `// Write an environment variable; it is merged back into the active environment after the run and can be referenced later via {{name}}
pm.environment.set('token', pm.response.json().token);`,
        },
      },
    ],
  },
  {
    id: "tempvar",
    label: { "zh-CN": "临时变量", "en-US": "Temp variables" },
    scope: ["pre", "post"],
    examples: [
      {
        id: "get-temp",
        label: { "zh-CN": "读取临时变量", "en-US": "Read temp variable" },
        code: {
          "zh-CN": `// pm.variables 是当前请求作用域的临时变量（不落盘、不共享到环境）
var v = pm.variables.get('myVar');`,
          "en-US": `// pm.variables holds temporary variables scoped to the current request (never persisted, never shared with the environment)
var v = pm.variables.get('myVar');`,
        },
      },
      {
        id: "set-temp",
        label: { "zh-CN": "设置临时变量", "en-US": "Set temp variable" },
        code: {
          "zh-CN": `// 仅本次请求有效，常用于前置脚本计算后给后置脚本使用
pm.variables.set('nonce', 'abc123');`,
          "en-US": `// Valid for this request only; commonly used to hand a value computed in the pre-request script to the post-response script
pm.variables.set('nonce', 'abc123');`,
        },
      },
    ],
  },
  {
    id: "sign",
    label: { "zh-CN": "签名", "en-US": "Signing" },
    scope: ["pre"],
    examples: [
      {
        id: "hmac-header",
        label: {
          "zh-CN": "HMAC 签名并写入 Header",
          "en-US": "HMAC sign & write header",
        },
        code: {
          "zh-CN": `// 对 body 计算 HMAC-SHA256，写入请求头（前置脚本）
var body = pm.request.body.raw;
var secret = pm.secret.get('SECRET');
var sign = pm.crypto.hmac('sha256', secret, body);
pm.request.headers.upsert({ key: 'X-Signature', value: sign });
console.log('签名:', sign);`,
          "en-US": `// Compute an HMAC-SHA256 over the body and write it into a request header (pre-request script)
var body = pm.request.body.raw;
var secret = pm.secret.get('SECRET');
var sign = pm.crypto.hmac('sha256', secret, body);
pm.request.headers.upsert({ key: 'X-Signature', value: sign });
console.log('signature:', sign);`,
        },
      },
      {
        id: "hash",
        label: { "zh-CN": "MD5 / SHA 哈希", "en-US": "MD5 / SHA hash" },
        code: {
          "zh-CN": `var md5 = pm.crypto.md5('hello');                  // 32 位 hex
var sha256 = pm.crypto.sha256('hello');            // 64 位 hex
var base64 = pm.crypto.hmacBase64('sha256', secret, body); // base64`,
          "en-US": `var md5 = pm.crypto.md5('hello');                  // 32-char hex
var sha256 = pm.crypto.sha256('hello');            // 64-char hex
var base64 = pm.crypto.hmacBase64('sha256', secret, body); // base64`,
        },
      },
    ],
  },
  {
    id: "query",
    label: { "zh-CN": "URL / Query 参数", "en-US": "URL / Query params" },
    scope: ["pre"],
    examples: [
      {
        id: "query-params",
        label: {
          "zh-CN": "读取 / 修改 query 参数",
          "en-US": "Read / modify query params",
        },
        code: {
          "zh-CN": `// 读取（不存在返回 null）
var uid = pm.request.getQueryParam('uid');

// 覆盖 / 追加 / 删除（就地改写 pm.request.url）
pm.request.setQueryParam('page', 2);
pm.request.addQueryParam('tag', 'a');
pm.request.removeQueryParam('debug');

// 等价写法：pm.request.query.get/set/add/remove/toObject/toString
console.log('uid =', uid, 'url =', pm.request.url);`,
          "en-US": `// Read (returns null when absent)
var uid = pm.request.getQueryParam('uid');

// Overwrite / append / remove (rewrites pm.request.url in place)
pm.request.setQueryParam('page', 2);
pm.request.addQueryParam('tag', 'a');
pm.request.removeQueryParam('debug');

// Equivalent form: pm.request.query.get/set/add/remove/toObject/toString
console.log('uid =', uid, 'url =', pm.request.url);`,
        },
      },
      {
        id: "url-api",
        label: {
          "zh-CN": "URL / URLSearchParams（Node 风格）",
          "en-US": "URL / URLSearchParams (Node-style)",
        },
        code: {
          "zh-CN": `// 浏览器同款 API（QuickJS 内置兼容层提供）
var u = new URL(pm.request.url);
u.searchParams.set('page', '2');
pm.request.url = u.toString();

// Node 风格模块
var url = require('url');
var parsed = url.parse(pm.request.url, true);  // true：query 解析为对象
console.log(parsed.query);
require('querystring').stringify({ a: 1, b: 'x y' }); // 'a=1&b=x+y'`,
          "en-US": `// The same API as in the browser (provided by the QuickJS compatibility layer)
var u = new URL(pm.request.url);
u.searchParams.set('page', '2');
pm.request.url = u.toString();

// Node-style modules
var url = require('url');
var parsed = url.parse(pm.request.url, true);  // true: parse query into an object
console.log(parsed.query);
require('querystring').stringify({ a: 1, b: 'x y' }); // 'a=1&b=x+y'`,
        },
      },
    ],
  },
  {
    id: "response",
    label: { "zh-CN": "读取响应体", "en-US": "Read response" },
    scope: ["post"],
    examples: [
      {
        id: "json",
        label: { "zh-CN": "读取响应 JSON", "en-US": "Read response JSON" },
        code: {
          "zh-CN": `// 后置脚本：解析 JSON（失败返回 null）
var data = pm.response.json();
console.log('code:', pm.response.code, 'time(ms):', pm.response.responseTime);`,
          "en-US": `// Post-response script: parse JSON (returns null on failure)
var data = pm.response.json();
console.log('code:', pm.response.code, 'time(ms):', pm.response.responseTime);`,
        },
      },
      {
        id: "text",
        label: { "zh-CN": "读取响应文本", "en-US": "Read response text" },
        code: {
          "zh-CN": `var text = pm.response.text();
console.log('body:', text);`,
          "en-US": `var text = pm.response.text();
console.log('body:', text);`,
        },
      },
    ],
  },
  {
    id: "assert",
    label: { "zh-CN": "断言", "en-US": "Assertions" },
    scope: ["post"],
    examples: [
      {
        id: "status",
        label: { "zh-CN": "断言状态码", "en-US": "Assert status code" },
        code: {
          "zh-CN": `pm.test('状态码为 200', function () {
  pm.expect(pm.response.code).to.equal(200);
});`,
          "en-US": `pm.test('status code is 200', function () {
  pm.expect(pm.response.code).to.equal(200);
});`,
        },
      },
      {
        id: "field",
        label: { "zh-CN": "断言响应字段", "en-US": "Assert response field" },
        code: {
          "zh-CN": `pm.test('返回含 token', function () {
  pm.expect(pm.response.json().token).to.equal('abc123');
});

pm.test('字段类型', function () {
  pm.expect(pm.response.json().name).to.be.a('string');
});`,
          "en-US": `pm.test('response contains a token', function () {
  pm.expect(pm.response.json().token).to.equal('abc123');
});

pm.test('field type', function () {
  pm.expect(pm.response.json().name).to.be.a('string');
});`,
        },
      },
      {
        id: "array",
        label: { "zh-CN": "断言数组", "en-US": "Assert array" },
        code: {
          "zh-CN": `pm.test('数组长度', function () {
  pm.expect(pm.response.json().list).to.have.property('2');
});`,
          "en-US": `pm.test('array length', function () {
  pm.expect(pm.response.json().list).to.have.property('2');
});`,
        },
      },
    ],
  },
  {
    id: "log",
    label: { "zh-CN": "日志", "en-US": "Logging" },
    scope: ["pre", "post"],
    examples: [
      {
        id: "log",
        label: { "zh-CN": "打印日志", "en-US": "Print log" },
        code: {
          "zh-CN": `console.log('当前 body:', pm.request.body.raw);
console.log({ a: 1, b: [1, 2, 3] }); // 对象美化输出
console.warn('注意'); console.error('出错');`,
          "en-US": `console.log('current body:', pm.request.body.raw);
console.log({ a: 1, b: [1, 2, 3] }); // pretty-printed object output
console.warn('warning'); console.error('error');`,
        },
      },
    ],
  },
];

/** Get the example categories applicable to a script kind (definition order preserved) */
export function getScriptExampleCategories(
  kind: ScriptKind,
): ScriptExampleCategory[] {
  return SCRIPT_EXAMPLE_CATEGORIES.filter((c) => c.scope.includes(kind));
}
