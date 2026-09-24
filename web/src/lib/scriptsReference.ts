import type { Locale } from "@/data/types";

export interface RefSection {
  /** Section heading */
  title: string;
  /** Paragraphs (may contain `inline code`) */
  body?: string[];
  /** Bullet list (may contain `inline code`) */
  bullets?: string[];
  /** Code block (rendered verbatim, no inline parsing) */
  code?: string;
}

export interface ScriptsReference {
  title: string;
  intro: string;
  sections: RefSection[];
}

const ZH: ScriptsReference = {
  title: "前置 / 后置脚本参考",
  intro:
    "脚本引擎基于 Rust + QuickJS（rquickjs）运行，在请求发送前（前置）或收到响应后（后置）执行 JavaScript（标准 ES5+）。所有能力通过全局对象 `pm` 提供，无 DOM / fetch / setTimeout 等浏览器 API。",
  sections: [
    {
      title: "执行顺序",
      body: [
        "一次请求的完整链路如下：",
        "1. 运行【插值前脚本】——在变量解析之前执行：可写入变量（本次请求的插值立刻能用，适合造随机数据），也可改写请求模板（改写结果仍会被插值）",
        "2. 解析 `{{$动态值}}`（时间戳、UUID、随机值等内建动态函数）",
        "3. 解析 `{{变量}}`（环境变量 / 集合变量 / 密钥 Secrets）",
        "4. 组装请求——URL 的 path / query 编码、请求体编码、Content-Type 等",
        "5. 运行【插值后脚本】——此时 `pm.request.*` 就是**最终报文**，这里改写的 url / method / headers / body 会原样发出（不再插值），适合签名 / 加密",
        "6. 发送请求",
        "7. 运行【后置脚本】——可读取响应、提取变量、写断言",
      ],
    },
    {
      title: "全局对象 pm",
      body: ["所有能力通过全局对象 `pm` 提供："],
      bullets: [
        "`pm.request`（前置可写、后置只读）：`url` / `method` / `headers` / `body.raw` / `body.mode`",
        "`pm.request` query 便捷方法（前置，就地改写 url 并保留 hash）：`getQueryParam(name)` / `getQueryParams(name)` / `hasQueryParam(name)` / `setQueryParam(name, value)` / `addQueryParam(name, value)` / `removeQueryParam(name)`；等价写法 `pm.request.query.get/set/add/remove/toObject/toString`",
        "全局 `URL` / `URLSearchParams`（浏览器同款 API）与 `require('url')` / `require('querystring')`（Node 风格模块），用于解析 / 构造 URL 与 query",
        "`pm.response`（仅后置、只读）：`code` / `status` / `responseTime`(ms) / `headers` / `body` / `text()` / `json()`",
        "`pm.environment`：持久变量（`set` 会合并回当前激活环境）；`pm.variables`：本次请求的**临时变量**（不落盘）",
        "`pm.secret`：只读密钥（api_key 等）；`pm.globals` / `pm.collectionVariables`：Postman 别名（映射到环境变量）",
        "`pm.crypto`：哈希 / HMAC / AES / Base64（见「计算签名 / 哈希」）",
        "`pm.test(name, fn)` + `pm.expect(...)`：断言（见「断言」）",
        "`pm.utf8` / `pm.hex` / `pm.b64`：字节串编解码工具（`encode` / `decode`）",
        "`CryptoJS`：**官方 crypto-js 完整库**（见「CryptoJS 官方库」）",
        "`require(name)`：加载内置库（见「require 内置库」）",
        "全局 `btoa` / `atob`（Latin-1 语义，见「Base64 编码」）与旧式扁平 `request` / `response` / `env`",
      ],
    },
    {
      title: "变量与密钥（Secrets）",
      body: [
        "变量（Variables）与密钥（Secrets）是两个独立的作用域，均在「环境管理」中维护：变量用于普通占位值，密钥用于 api_key、token 等敏感信息（UI 中默认掩码显示）。",
        "`pm.environment.get(name)` 的解析顺序为：脚本本次写入的变量 → 环境变量 → 密钥（密钥作为只读兜底）。即同名时脚本写入优先，其次变量，最后密钥。",
        "`pm.variables` 是**本次请求作用域的临时变量**：`set(name, value)` 只对本请求内后续读取生效，不落盘、不影响其它请求；读取优先级为 临时变量 → 环境变量。",
        "`pm.globals` / `pm.collectionVariables` 为 Postman 兼容别名（`get` / `set` / `upsert` / `remove` 等），内部映射到 `pm.environment`，便于导入的 Postman 脚本零修改运行。",
        "密钥合并进了变量快照，因此也能直接在模板里用 `{{secret_name}}` 引用（如请求头、签名密钥、鉴权字段）。",
      ],
      code: `// ── 读取密钥（推荐：语义清晰，且密钥只读）──
var apiKey = pm.secret.get('API_KEY');
var apiKey2 = pm.environment.get('API_KEY');   // 等价：会回退到密钥

// ── 写 / 读变量的等价写法 ──
pm.environment.set('user', 'admin');           // 持久：合并回环境
var a = pm.environment.get('user');

pm.globals.set('user', 'admin');               // Postman 别名：即 environment.set
var b = pm.globals.get('user');                // == a

pm.collectionVariables.set('scope', 's');      // 别名同上

pm.variables.set('temp', 'x');                 // 临时：仅本次请求，不落盘
var t = pm.variables.get('temp');

// ── 模板引用（脚本外）──
// Authorization: Bearer {{API_KEY}}   —— 变量 / 密钥均可在 URL、Header、Body 用 {{name}}`,
    },
    {
      title: "修改请求（插值后脚本）",
      body: [
        "插值后脚本可改写即将发出的请求。变量与动态值均已在前面阶段解析完成，`pm.request.body.raw` 即为最终 body；此处改写的内容不再被插值。",
      ],
      code: `// 读取最终 body（已解析变量）
var body = pm.request.body.raw;

// ── 写入 / 修改 header：以下三种写法等价 ──
pm.request.headers.upsert({ key: 'X-Signature', value: sign });
pm.request.headers.set('X-Token', token);
pm.request.headers['X-Foo'] = 'bar';
pm.request.headers.remove('X-Old');

// ── 改写 url / method / body ──
pm.request.url = pm.request.url + '?v=2';
pm.request.method = 'POST';
pm.request.body.raw = JSON.stringify({ a: 1 });

// 旧式扁平写法（兼容历史脚本）
request.url = request.url + '?v=3';
request.headers['X-Legacy'] = '1';`,
    },
    {
      title: "处理 URL / query 参数（前置脚本）",
      body: [
        "处理 query 参数有两种方式：`pm.request` 便捷方法（就地改写 URL，自动保留 `#hash`），或全局 `URL` / `URLSearchParams`（浏览器同款 API，QuickJS 原生不含，由内置兼容层提供）。",
        "便捷方法：`getQueryParam(name)`（不存在返回 `null`）、`getQueryParams(name)`（同名多值）、`hasQueryParam(name)`、`setQueryParam(name, value)`（覆盖同名，保留首个）、`addQueryParam(name, value)`（追加同名）、`removeQueryParam(name)`（删除全部同名）。",
        "也支持 `require('url')` / `require('querystring')` 等 Node 风格模块。",
      ],
      code: `// ── 方式一：pm.request 便捷方法（推荐，最简）──
var uid = pm.request.getQueryParam('uid');        // 不存在返回 null
pm.request.setQueryParam('page', 2);              // 覆盖（自动 URL 编码）
pm.request.addQueryParam('tag', 'a');             // 追加
pm.request.addQueryParam('tag', 'b');             // 同名多值 → tag=a&tag=b
pm.request.removeQueryParam('debug');             // 删除全部同名
// 等价写法：
// pm.request.query.get('uid') / .set('page', 2) / .add(...) / .remove(...) / .toObject()

// ── 方式二：URL / URLSearchParams（浏览器同款 API）──
var u = new URL(pm.request.url);
u.searchParams.set('page', '2');
u.searchParams.append('tag', 'x');
pm.request.url = u.toString();                    // 写回（自动保留 #hash）

var sp = new URLSearchParams('a=1&a=2');
sp.get('a');                                      // '1'
sp.getAll('a');                                   // ['1', '2']
sp.toString();                                    // 'a=1&a=2'

// ── 方式三：Node 风格模块 ──
var url = require('url');
var parsed = url.parse(pm.request.url, true);     // true → query 解析为对象
parsed.query;                                     // { a: '1', b: '2' }
url.format({ protocol: 'https:', host: 'a.com', pathname: '/p', query: 'x=1' });
require('querystring').stringify({ a: 1, b: 'x y' }); // 'a=1&b=x+y'`,
    },
    {
      title: "require 内置库",
      body: [
        "`require(name)` 同步返回内置库（与 Postman sandbox 同款机制），首次加载后本次运行内缓存；**未内置的模块会抛错**（可用：crypto-js, lodash, moment, uuid, atob, btoa, url, querystring）。",
        "内置**官方 crypto-js 4.2.0 完整库**——导入的 Postman 脚本写什么就是什么，**无需修改**即可运行。",
      ],
      code: `const CryptoJS = require('crypto-js');   // 与全局 CryptoJS 同一实例
const _ = require('lodash');            // 4.17.21
const moment = require('moment');       // 2.30.1
const { v4: uuidv4 } = require('uuid'); // 7.0.3
const atobFn = require('atob');         // 与全局 atob 同一函数
const btoaFn = require('btoa');
const url = require('url');             // Node 风格：URL / URLSearchParams / parse / format / resolve
const qs = require('querystring');      // Node 风格：parse / stringify

var arr = _.chunk([1, 2, 3, 4], 2);     // [[1,2],[3,4]]
var u = uuidv4();                       // xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx`,
    },
    {
      title: "Base64 编码（btoa / atob）",
      body: [
        "全局 `btoa` / `atob` 为 **Latin-1 语义**（与浏览器 / Postman 一致）：每个字符取低 8 位，字符码位超过 255（如中文）会抛异常——中文等需先 `unescape(encodeURIComponent(s))` 编码。",
        "**Postman 兼容**：`btoa` 可直接接收 CryptoJS 摘要（WordArray），`btoa(CryptoJS.MD5(s))` 直接得到摘要字节的 Base64（与 `.toString(CryptoJS.enc.Base64)` 等价）。",
        "中文更推荐 crypto-js 方式：`CryptoJS.enc.Base64.stringify(CryptoJS.enc.Utf8.parse(s))`（自动 UTF-8，天然支持中文 / emoji）。",
        "JWT 等场景需要 URL 安全的 **Base64url**：`CryptoJS.enc.Base64url.stringify(...)`（自动去 `=`、`+`→`-`、`/`→`_`）。",
      ],
      code: `// ── 标准 Base64：同一结果的三种写法 ──
btoa('hello');                              // aGVsbG8=
pm.crypto.base64Encode('hello');            // aGVsbG8=（UTF-8 字节的 base64）
CryptoJS.enc.Base64.stringify(CryptoJS.enc.Utf8.parse('hello'));  // aGVsbG8=

// 解码
atob('aGVsbG8=');                           // hello
pm.crypto.base64Decode('aGVsbG8=');         // hello（字节串）
CryptoJS.enc.Utf8.stringify(CryptoJS.enc.Base64.parse('aGVsbG8=')); // hello

// ── Postman 兼容：btoa 直接吃 CryptoJS 摘要对象 ──
btoa(CryptoJS.MD5('abc'));                  // kAFQmDzST7DWlj99KOF/cg==（摘要字节 base64）
btoa(CryptoJS.HmacSHA256(msg, key));        // 等价于 .toString(CryptoJS.enc.Base64)

// ── 中文：btoa 需先编码；crypto-js 自动 UTF-8 ──
btoa(unescape(encodeURIComponent('你好'))); // 5L2g5aW9
CryptoJS.enc.Base64.stringify(CryptoJS.enc.Utf8.parse('你好'));    // 5L2g5aW9
CryptoJS.enc.Utf8.stringify(CryptoJS.enc.Base64.parse('5L2g5aW9'));// 你好

// ── Base64url（JWT 用）：URL 安全、无填充 ──
CryptoJS.enc.Base64url.stringify(CryptoJS.enc.Utf8.parse('hello'));
CryptoJS.enc.Base64url.parse('aGVsbG8');    // 解码（自动补 =）`,
    },
    {
      title: "字节串工具（pm.utf8 / pm.hex / pm.b64）",
      body: [
        "Orbit 提供字节串编解码工具（字节串 = 每个字符 0-255 的 JS 字符串），适合在自定义协议、二进制 body、签名场景中与 `pm.crypto` 配合使用。",
        "`pm.utf8.encode(str)` → UTF-8 字节串；`pm.utf8.decode(bytes)` → 原字符串。",
        "`pm.hex.encode(bytes)` → hex 字符串；`pm.hex.decode(hexStr)` → 字节串。",
        "`pm.b64.encode(bytes)` → base64 字符串；`pm.b64.decode(b64)` → 字节串。",
      ],
      code: `// 字节串 → hex / base64（三者都基于同一字节串）
var raw = pm.request.body.raw;
var bytes = pm.utf8.encode(raw);   // UTF-8 字节串
var hex = pm.hex.encode(bytes);
var b64 = pm.b64.encode(bytes);

// 反向：hex / base64 → 原字符串
var back = pm.utf8.decode(pm.hex.decode(hex));
var back2 = pm.utf8.decode(pm.b64.decode(b64));

// pm.crypto 的 AES 接口直接吃 hex
var keyHex = pm.hex.encode(pm.utf8.encode('16byte-secret!!'));
var ivHex = '000102030405060708090a0b0c0d0e0f';
var ct = pm.crypto.aesEncrypt({ data: hex, key: keyHex, iv: ivHex, mode: CryptoJS.mode.CBC, outputType: 'base64' });`,
    },
    {
      title: "计算签名 / 哈希（pm.crypto）",
      body: [
        "`pm.crypto` 提供 md5 / sha1 / sha224 / sha256 / sha384 / sha512 / sha3 / ripemd160 / hmac / hmacBase64 / base64Encode / base64Decode / aesEncrypt / aesDecrypt / getRandomValues，除注明 base64 外均返回十六进制字符串。",
        "`hmac(algo, key, msg)` 的 algo 支持 `md5` / `sha1` / `sha224` / `sha256` / `sha384` / `sha512` / `ripemd160` / `sha3-224/256/384/512`。底层由 Rust 实现，可审计。",
        "`aesEncrypt({ data, key, iv, mode, outputType })` 对象参数（Postman 官方签名），data/key/iv 为 hex 字符串；`mode` 为 `CryptoJS.mode.CBC/ECB`（Rust 实现支持 CBC/ECB），`outputType` 为 `hex` / `base64` / `string`。",
        "`getRandomValues(nBytes)` 返回 n 字节随机 hex（WordArray 兼容，可 `toString(CryptoJS.enc.Hex)`）。",
      ],
      code: `// ── 各哈希算法 ──
pm.crypto.md5('hello')                       // 32 位 hex
pm.crypto.sha1('hello')                       // 40 位 hex
pm.crypto.sha224('hello')                     // 56 位 hex
pm.crypto.sha256('hello')                     // 64 位 hex
pm.crypto.sha384('hello')                     // 96 位 hex
pm.crypto.sha512('hello')                     // 128 位 hex
pm.crypto.sha3('hello', 512)                  // SHA3-512
pm.crypto.sha3('hello', 256)                  // SHA3-256
pm.crypto.ripemd160('hello')                  // 40 位 hex

// ── HMAC ──
pm.crypto.hmac('sha256', secret, body)        // hex
pm.crypto.hmacBase64('sha256', secret, body)  // base64
pm.crypto.hmac('sha512', secret, body)        // 换算法同理
pm.crypto.hmac('ripemd160', secret, body)

// ── 随机字节 ──
pm.crypto.getRandomValues(16).toString(CryptoJS.enc.Hex)   // 16 字节随机 hex
pm.crypto.getRandomValues(32)                              // 直接就是 hex 字符串`,
    },
    {
      title: "签名写法对照（pm.crypto vs CryptoJS）",
      body: [
        "同一个 HMAC-SHA256 任务，各 API 的**输出完全一致**（集成测试保证），可按习惯任选：",
        "- hex 输出：`pm.crypto.hmac(...)` 与 `CryptoJS.HmacSHA256(...).toString()` 等价",
        "- base64 输出：`pm.crypto.hmacBase64(...)` 与 `CryptoJS.HmacSHA256(...).toString(CryptoJS.enc.Base64)` 等价",
        "消息含中文时：pm.crypto 直接传字符串即可（内部按 UTF-8）；CryptoJS 的 msg 直接传字符串同样按 UTF-8 处理。",
      ],
      code: `// ── 任务：HMAC-SHA256(body, secret)，输出 hex ──
var sign1 = pm.crypto.hmac('sha256', secret, body);          // (A) pm.crypto
var sign2 = CryptoJS.HmacSHA256(body, secret).toString();    // (B) CryptoJS
// sign1 === sign2

// ── 任务：输出 base64（常用于 Authorization 头）──
var b1 = pm.crypto.hmacBase64('sha256', secret, body);
var b2 = CryptoJS.HmacSHA256(body, secret).toString(CryptoJS.enc.Base64);
var b3 = CryptoJS.enc.Base64.stringify(CryptoJS.HmacSHA256(body, secret));
// b1 === b2 === b3

// ── 任务：SHA-256 摘要 ──
pm.crypto.sha256(body)                        // hex
CryptoJS.SHA256(body).toString()              // hex，等价
CryptoJS.SHA256(body).toString(CryptoJS.enc.Base64)   // base64 版

// ── 任务：中文消息签名（两种写法都按 UTF-8，结果一致）──
pm.crypto.hmac('sha256', secret, '订单号A001');
CryptoJS.HmacSHA256('订单号A001', secret).toString();`,
    },
    {
      title: "AES 加解密写法对照",
      body: [
        "`CryptoJS.AES`（官方库）支持口令模式与原始密钥模式；`pm.crypto.aesEncrypt`（Rust）使用 hex 交换、仅 CBC/ECB。",
        "口令模式输出 OpenSSL `Salted__` 格式，可与 `openssl enc -aes-256-cbc -pass pass:xxx` 互相解密。",
      ],
      code: `// ── 方式一：CryptoJS 口令模式（最常用，Salted__ 格式）──
const ct1 = CryptoJS.AES.encrypt('hello world', 'mySecret').toString();
const pt1 = CryptoJS.AES.decrypt(ct1, 'mySecret').toString(CryptoJS.enc.Utf8);  // hello world

// ── 方式二：CryptoJS 原始密钥模式（key/iv 为 WordArray）──
const key = CryptoJS.enc.Hex.parse('2b7e151628aed2a6abf7158809cf4f3c');  // AES-128
const iv  = CryptoJS.enc.Hex.parse('000102030405060708090a0b0c0d0e0f');
const ct2 = CryptoJS.AES.encrypt('data', key, { iv }).toString(CryptoJS.enc.Base64);
const pt2 = CryptoJS.AES.decrypt(ct2, key, { iv }).toString(CryptoJS.enc.Utf8); // data

// ── 方式三：pm.crypto（Rust，hex 交换，CBC/ECB）──
var keyHex = '2b7e151628aed2a6abf7158809cf4f3c';          // 16/24/32 字节 hex
var ivHex  = '000102030405060708090a0b0c0d0e0f';          // 16 字节 hex
var dataHex = CryptoJS.enc.Hex.stringify(CryptoJS.enc.Utf8.parse('data'));
var ct3 = pm.crypto.aesEncrypt({ data: dataHex, key: keyHex, iv: ivHex, mode: CryptoJS.mode.CBC, outputType: 'hex' });
var pt3 = pm.crypto.aesDecrypt({ data: ct3, key: keyHex, iv: ivHex, mode: CryptoJS.mode.CBC, outputType: 'string' }); // data

// ── 方式四：DES / TripleDES（crypto-js 同构）──
var d = CryptoJS.DES.encrypt('msg', 'secret').toString();
CryptoJS.DES.decrypt(d, 'secret').toString(CryptoJS.enc.Utf8);
var t = CryptoJS.TripleDES.encrypt('msg', 'secret').toString();
CryptoJS.TripleDES.decrypt(t, 'secret').toString(CryptoJS.enc.Utf8);`,
    },
    {
      title: "JWT 签名完整示例",
      body: [
        "综合演练：Base64url 编码 header/payload + HMAC-SHA256 签名，生成 JWT 并写入 Authorization 头（前置脚本）。",
        "也可用 `require('uuid')` 生成 jti。",
      ],
      code: `// 前置脚本：生成 JWT（HS256）
var secret = pm.secret.get('JWT_SECRET');        // 密钥放「环境管理 → 密钥」

function b64url(s) {
  return CryptoJS.enc.Base64url.stringify(CryptoJS.enc.Utf8.parse(s));
}

var header = b64url(JSON.stringify({ alg: 'HS256', typ: 'JWT' }));
var payload = b64url(JSON.stringify({
  sub: 'user_001',
  name: '管理员',
  iat: Math.floor(Date.now() / 1000),
  exp: Math.floor(Date.now() / 1000) + 3600,
  jti: require('uuid').v4(),
}));

var signingInput = header + '.' + payload;
var signature = CryptoJS.enc.Base64url.stringify(
  CryptoJS.HmacSHA256(signingInput, secret)
);

var token = signingInput + '.' + signature;
pm.request.headers.upsert({ key: 'Authorization', value: 'Bearer ' + token });
console.log('JWT:', token);`,
    },
    {
      title: "UUID / 随机数",
      body: [
        "三种获取随机 / UUID 的方式：动态模板、`require('uuid')`、`pm.crypto.getRandomValues`。",
      ],
      code: `// ── 方式一：模板动态值（脚本外，URL/Header/Body 均可）──
// {{$uuid}}  {{$timestamp}}  {{$randomInt}}  {{$guid}}

// ── 方式二：require('uuid')（v4）──
var u = require('uuid').v4();      // xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx

// ── 方式三：pm.crypto.getRandomValues ──
var hex16 = pm.crypto.getRandomValues(16);         // 32 位 hex 随机
var salt = CryptoJS.enc.Hex.parse(pm.crypto.getRandomValues(8)); // 8 字节 salt（WordArray）`,
    },
    {
      title: "CryptoJS 官方库",
      body: [
        "内置**官方 crypto-js 4.2.0 完整库**（与 Postman 完全一致），`require('crypto-js')` 与全局 `CryptoJS` 是**同一实例**。",
        "哈希：`MD5` / `SHA1` / `SHA224` / `SHA256` / `SHA384` / `SHA512` / `SHA3(msg, {outputLength})` / `RIPEMD160`；HMAC 变体：`HmacMD5` / `HmacSHA1` / `HmacSHA224` / `HmacSHA256` / `HmacSHA384` / `HmacSHA512` / `HmacSHA3` / `HmacRIPEMD160`。",
        "对称加密：`AES` / `DES` / `TripleDES` / `Rabbit` / `RabbitLegacy` / `RC4` / `RC4Drop`（`encrypt(message, key, cfg)` / `decrypt(ciphertext, key, cfg)`）。key 为字符串时走**口令模式**（OpenSSL `Salted__` 格式，可与 openssl CLI 互解）；key 为 WordArray 时走原始密钥模式。",
        "编码器 `enc.*`：`Hex` / `Latin1` / `Utf8` / `Utf16` / `Utf16LE` / `Base64` / `Base64url`（`parse` / `stringify`）；模式 `mode.*`：`CBC`（默认）/ `ECB` / `CFB` / `OFB` / `CTR`；填充 `pad.*`：`Pkcs7`（默认）/ `NoPadding` / `ZeroPadding` / `AnsiX923` / `Iso10126` / `Iso97971`。",
        "`.toString()` 默认 hex；`.toString(CryptoJS.enc.Base64)` 得 base64。AES 加密返回 CipherParams 对象，`.toString()` 即 OpenSSL 格式字符串。",
      ],
      code: `const CryptoJS = require('crypto-js');
// 或直接用全局 CryptoJS（同一实例）

// ── 哈希 / HMAC ──
CryptoJS.MD5('x').toString();
CryptoJS.SHA3('x', { outputLength: 256 }).toString();
CryptoJS.HmacSHA256(msg, key).toString(CryptoJS.enc.Base64);

// ── 高级模式：AES-CBC + ZeroPadding + 原始 key ──
var key = CryptoJS.enc.Hex.parse('2b7e151628aed2a6abf7158809cf4f3c');
var iv  = CryptoJS.enc.Hex.parse('000102030405060708090a0b0c0d0e0f');
var ct = CryptoJS.AES.encrypt('data', key, {
  mode: CryptoJS.mode.CBC,
  padding: CryptoJS.pad.ZeroPadding,
  iv: iv,
}).toString(CryptoJS.enc.Base64);
CryptoJS.AES.decrypt(ct, key, { mode: CryptoJS.mode.CBC, padding: CryptoJS.pad.ZeroPadding, iv: iv })
  .toString(CryptoJS.enc.Utf8);

// ── 口令模式（OpenSSL 兼容，可与 CLI 互解）──
const ct2 = CryptoJS.AES.encrypt('hello', 'secret').toString();
const pt2 = CryptoJS.AES.decrypt(ct2, 'secret').toString(CryptoJS.enc.Utf8);

// ── 中文 Base64（官方推荐）──
const b64 = CryptoJS.enc.Base64.stringify(CryptoJS.enc.Utf8.parse('你好'));`,
    },
    {
      title: "读取响应（后置脚本）",
      body: [
        "后置脚本可读响应。`pm.response.json()` 解析 JSON，失败返回 `null`；`pm.response.text()` 返回原始文本。",
      ],
      code: `var data = pm.response.json();      // 解析 JSON，失败返回 null
var text = pm.response.text();      // 原始文本
pm.response.code;                   // 状态码，如 200
pm.response.responseTime;           // 耗时(ms)
pm.response.headers['Content-Type'];

// 响应也可参与签名 / 断言
pm.test('响应体是 JSON 数组', function () {
  pm.expect(pm.response.json()).to.be.a('array');
});`,
    },
    {
      title: "提取变量",
      body: [
        "`pm.environment.set(name, value)` 写入的变量，会在本次运行结束后自动合并回当前激活环境（与 `pm.environment.get` 同名时覆盖）。",
        "因此可在后置脚本里把响应中的 token 等提取到环境，供后续请求通过 `{{token}}` 引用。",
        "仅需本请求内使用的中间值，用 `pm.variables.set` 更合适（不落盘、无副作用）。",
      ],
      code: `var token = pm.response.json().token;
pm.environment.set('token', token);   // 之后可用 {{token}} 引用

// 本请求内临时使用
pm.variables.set('page', pm.response.json().page);

// Postman 别名同样可用
pm.globals.set('last_status', pm.response.code);`,
    },
    {
      title: "断言（pm.test / pm.expect）",
      body: [
        "用 `pm.test(name, fn)` 定义断言，`fn` 内用 `pm.expect` 校验。断言失败会被捕获并记录，不会中断脚本。",
        "所有断言结果显示在响应面板的「脚本」→ 断言区，绿色通过、红色失败。",
      ],
      code: `pm.test('状态码为 200', function () {
  pm.expect(pm.response.code).to.equal(200);
});
pm.test('返回含 token', function () {
  pm.expect(pm.response.json().token).to.equal('abc123');
});
pm.test('数组长度', function () {
  pm.expect(pm.response.json().arr).to.have.property('2');
});
pm.test('类型校验', function () {
  pm.expect('hi').to.be.a('string');
});
// 断言失败示例（记录但不中断）
pm.test('字段非空', function () {
  pm.expect(pm.response.json().name).to.be.ok;
});`,
      bullets: [
        "`to.equal(expected)` —— 宽松相等（==）",
        "`to.eql(expected)` —— 深度相等（JSON 比对）",
        "`to.contain(sub)` —— 字符串包含",
        "`to.be.true` / `to.be.false` / `to.be.null` / `to.be.undefined` / `to.be.ok`（真值）",
        "`to.be.a('string'|'number'|'boolean'|'object'|'array')`",
        "`to.have.property('key')`",
      ],
    },
    {
      title: "调试输出（console）",
      body: [
        "使用 `console.log` / `console.warn` / `console.error` 打印调试信息，显示在响应面板的「脚本」日志区。",
        "对象会被自动美化为多行 JSON，每条日志可点击展开 / 收起，长 JSON 不再需要滚动原始文本。",
      ],
      code: `console.log('当前 body:', pm.request.body.raw);
console.log({ a: 1, b: [1, 2, 3] });   // 美化输出
console.warn('注意'); console.error('出错');`,
    },
    {
      title: "完整示例：签名并写入 Header",
      body: [
        "经典的「body + 密钥 生成 HMAC，写入 header」场景。因为动态值已在阶段 1 解析，此处 `pm.request.body.raw` 已是最终内容。",
      ],
      code: `// 前置脚本
var body = pm.request.body.raw;
var secret = pm.secret.get('SECRET');   // 读取密钥（在「环境管理 → 密钥」配置）

// 写法 A：pm.crypto（hex）
var sign = pm.crypto.hmac('sha256', secret, body + '|' + 'k1');
// 写法 B：CryptoJS（等价）
var sign2 = CryptoJS.HmacSHA256(body + '|' + 'k1', secret).toString();

pm.request.headers.upsert({ key: 'X-Signature', value: sign });
console.log('签名:', sign);`,
    },
    {
      title: "注意事项",
      bullets: [
        "前置脚本中 `pm.response` 不可用；后置脚本中 `pm.request` 为只读快照。",
        "密钥（Secrets）只读：脚本只能用 `pm.secret.get` / `pm.environment.get` 读取，不能用 `set` 写入密钥。",
        "`pm.variables` 是临时变量（本次请求有效，不落盘）；`pm.environment` 写入会持久化回环境。",
        "`pm.globals` / `pm.collectionVariables` 是兼容别名，映射到环境变量，非独立作用域。",
        "`require(name)` 只支持内置库（crypto-js, lodash, moment, uuid, atob, btoa, url, querystring），其它模块会抛错；脚本在 Rust 沙箱中执行，无 DOM / fetch / 网络 / 文件系统。",
        "`btoa` / `atob` 为 Latin-1 语义：直接传中文会抛异常，中文请用 `unescape(encodeURIComponent(s))` 或 crypto-js 方式。",
        "`pm.crypto.aesEncrypt/aesDecrypt` 仅支持 CBC/ECB；`CryptoJS.AES`（官方库）支持 CBC/ECB/CFB/OFB/CTR。",
        "断言失败仅记录，不会让请求本身失败。",
        "避免在 console 打印超大字符串；日志按条目展开，但仍有体积成本。",
      ],
    },
  ],
};

const EN: ScriptsReference = {
  title: "Pre-request / Post-response Script Reference",
  intro:
    "The script engine runs on Rust + QuickJS (rquickjs), executing JavaScript (standard ES5+) before the request is sent (pre-request) or after the response is received (post-response). All capabilities are exposed via the global `pm` object. There is no DOM / fetch / setTimeout browser API.",
  sections: [
    {
      title: "Execution Order",
      body: [
        "The full lifecycle of a single request:",
        "1. Run the [Before-interpolation Script] — runs before variable resolution: it can set variables (usable by this very request's interpolation, ideal for random data) and rewrite the request template (the result is still interpolated)",
        "2. Resolve `{{$dynamic}}` (built-in dynamic functions: timestamp, UUID, random, ...)",
        "3. Resolve `{{variables}}` (environment / collection variables / secrets)",
        "4. Assemble the request — encode URL path / query, encode the body, derive Content-Type, ...",
        "5. Run the [After-interpolation Script] — `pm.request.*` is now the **final payload**; anything it rewrites (url / method / headers / body) is sent verbatim (no further interpolation), ideal for signing / encryption",
        "6. Send the request",
        "7. Run the [Post-response Script] — may read the response, extract variables, and assert",
      ],
    },
    {
      title: "Global object: pm",
      body: ["All capabilities are exposed via the global `pm` object:"],
      bullets: [
        "`pm.request` (writable in pre-request, read-only in post): `url` / `method` / `headers` / `body.raw` / `body.mode`",
        "`pm.request` query helpers (pre-request, rewrites the url in place and keeps the hash): `getQueryParam(name)` / `getQueryParams(name)` / `hasQueryParam(name)` / `setQueryParam(name, value)` / `addQueryParam(name, value)` / `removeQueryParam(name)`; equivalent form `pm.request.query.get/set/add/remove/toObject/toString`",
        "Global `URL` / `URLSearchParams` (browser-compatible API) and `require('url')` / `require('querystring')` (Node-style modules) for parsing / building URLs and queries",
        "`pm.response` (post-only, read-only): `code` / `status` / `responseTime`(ms) / `headers` / `body` / `text()` / `json()`",
        "`pm.environment`: persistent variables (`set` merges back into the active environment); `pm.variables`: **temporary** per-request variables (not persisted)",
        "`pm.secret`: read-only secrets (api_key, etc.); `pm.globals` / `pm.collectionVariables`: Postman aliases (mapped to environment)",
        '`pm.crypto`: hashing / HMAC / AES / Base64 (see "Compute signature / hash")',
        '`pm.test(name, fn)` + `pm.expect(...)`: assertions (see "Assertions")',
        "`pm.utf8` / `pm.hex` / `pm.b64`: byte-string codec helpers (`encode` / `decode`)",
        '`CryptoJS`: the **full official crypto-js library** (see "CryptoJS official library")',
        '`require(name)`: load built-in libraries (see "require built-ins")',
        'Global `btoa` / `atob` (Latin-1 semantics, see "Base64 encoding") and legacy flat `request` / `response` / `env`',
      ],
    },
    {
      title: "Variables & Secrets",
      body: [
        'Variables and Secrets are two separate scopes, both managed in "Environment Manager": variables hold ordinary placeholders, while Secrets hold sensitive values like api_key / token (masked by default in the UI).',
        "`pm.environment.get(name)` resolves in this order: variables written by the script this run → environment variables → secrets (secrets act as a read-only fallback). Same name: script writes win, then variables, then secrets.",
        "`pm.variables` are **per-request temporary variables**: `set(name, value)` only affects subsequent reads within this request — never persisted, never leaks to other requests. Read priority: temporary → environment.",
        "`pm.globals` / `pm.collectionVariables` are Postman-compatible aliases (`get` / `set` / `upsert` / `remove`, etc.) mapped to `pm.environment`, so imported Postman scripts run unmodified.",
        "Secrets are merged into the variable snapshot, so you can also reference them directly in templates via `{{secret_name}}` (e.g. headers, signing keys, auth fields).",
      ],
      code: `// ── Read a secret (preferred: clear intent, and secrets are read-only) ──
var apiKey = pm.secret.get('API_KEY');
var apiKey2 = pm.environment.get('API_KEY');   // equivalent: falls back to secrets

// ── Equivalent ways to write / read variables ──
pm.environment.set('user', 'admin');           // persistent: merged back into env
var a = pm.environment.get('user');

pm.globals.set('user', 'admin');               // Postman alias: == environment.set
var b = pm.globals.get('user');                // == a

pm.collectionVariables.set('scope', 's');      // alias, same as above

pm.variables.set('temp', 'x');                 // temporary: this request only, not persisted
var t = pm.variables.get('temp');

// ── Template reference (outside scripts) ──
// Authorization: Bearer {{API_KEY}}   — variables / secrets work in URL, Header, Body via {{name}}`,
    },
    {
      title: "Modify the request (after interpolation)",
      body: [
        "The after-interpolation script can rewrite the outgoing request. Variables and dynamic values are already resolved, so `pm.request.body.raw` is the final body; whatever you write here is no longer interpolated.",
      ],
      code: `// Read the final body (variables resolved)
var body = pm.request.body.raw;

// ── Write / modify headers: all three are equivalent ──
pm.request.headers.upsert({ key: 'X-Signature', value: sign });
pm.request.headers.set('X-Token', token);
pm.request.headers['X-Foo'] = 'bar';
pm.request.headers.remove('X-Old');

// ── Rewrite url / method / body ──
pm.request.url = pm.request.url + '?v=2';
pm.request.method = 'POST';
pm.request.body.raw = JSON.stringify({ a: 1 });

// Legacy flat style (compat with old scripts)
request.url = request.url + '?v=3';
request.headers['X-Legacy'] = '1';`,
    },
    {
      title: "Work with URL / query params (pre-request)",
      body: [
        "Two ways to handle query params: the `pm.request` helpers (rewrite the URL in place, keeping `#hash`), or the global `URL` / `URLSearchParams` (browser-compatible API; QuickJS has no native URL, provided by a built-in shim).",
        "Helpers: `getQueryParam(name)` (returns `null` when absent), `getQueryParams(name)` (all values), `hasQueryParam(name)`, `setQueryParam(name, value)` (replace, keep first), `addQueryParam(name, value)` (append), `removeQueryParam(name)` (delete all with that name).",
        "Node-style modules are also available via `require('url')` / `require('querystring')`.",
      ],
      code: `// ── Way 1: pm.request helpers (recommended, simplest) ──
var uid = pm.request.getQueryParam('uid');        // null when absent
pm.request.setQueryParam('page', 2);              // replace (auto URL-encoded)
pm.request.addQueryParam('tag', 'a');             // append
pm.request.addQueryParam('tag', 'b');             // duplicate → tag=a&tag=b
pm.request.removeQueryParam('debug');             // delete all with this name
// Equivalent form:
// pm.request.query.get('uid') / .set('page', 2) / .add(...) / .remove(...) / .toObject()

// ── Way 2: URL / URLSearchParams (browser-compatible API) ──
var u = new URL(pm.request.url);
u.searchParams.set('page', '2');
u.searchParams.append('tag', 'x');
pm.request.url = u.toString();                    // write back (keeps #hash)

var sp = new URLSearchParams('a=1&a=2');
sp.get('a');                                      // '1'
sp.getAll('a');                                   // ['1', '2']
sp.toString();                                    // 'a=1&a=2'

// ── Way 3: Node-style modules ──
var url = require('url');
var parsed = url.parse(pm.request.url, true);     // true → query parsed into an object
parsed.query;                                     // { a: '1', b: '2' }
url.format({ protocol: 'https:', host: 'a.com', pathname: '/p', query: 'x=1' });
require('querystring').stringify({ a: 1, b: 'x y' }); // 'a=1&b=x+y'`,
    },
    {
      title: "require built-ins",
      body: [
        "`require(name)` synchronously returns a built-in library (same mechanism as the Postman sandbox), cached for the rest of this run; **unknown modules throw an error** (available: crypto-js, lodash, moment, uuid, atob, btoa, url, querystring).",
        "Ships the **full official crypto-js 4.2.0** — imported Postman scripts run exactly as written, **without modification**.",
      ],
      code: `const CryptoJS = require('crypto-js');   // same instance as global CryptoJS
const _ = require('lodash');            // 4.17.21
const moment = require('moment');       // 2.30.1
const { v4: uuidv4 } = require('uuid'); // 7.0.3
const atobFn = require('atob');         // same function as global atob
const btoaFn = require('btoa');
const url = require('url');             // Node-style: URL / URLSearchParams / parse / format / resolve
const qs = require('querystring');      // Node-style: parse / stringify

var arr = _.chunk([1, 2, 3, 4], 2);     // [[1,2],[3,4]]
var u = uuidv4();                       // xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx`,
    },
    {
      title: "Base64 encoding (btoa / atob)",
      body: [
        "Global `btoa` / `atob` use **Latin-1 semantics** (same as browsers / Postman): each character is truncated to its low 8 bits, and characters above U+00FF (e.g. Chinese) throw — encode Chinese first with `unescape(encodeURIComponent(s))`.",
        "**Postman-compatible**: `btoa` also accepts a CryptoJS digest (WordArray) directly — `btoa(CryptoJS.MD5(s))` returns the Base64 of the digest bytes (equivalent to `.toString(CryptoJS.enc.Base64)`).",
        "For Chinese, the crypto-js approach is preferred: `CryptoJS.enc.Base64.stringify(CryptoJS.enc.Utf8.parse(s))` (automatic UTF-8, safe for Chinese / emoji).",
        "For URL-safe **Base64url** (JWT, etc.): `CryptoJS.enc.Base64url.stringify(...)` (strips `=`, `+`→`-`, `/`→`_`).",
      ],
      code: `// ── Standard Base64: three equivalent ways, same result ──
btoa('hello');                              // aGVsbG8=
pm.crypto.base64Encode('hello');            // aGVsbG8= (base64 of UTF-8 bytes)
CryptoJS.enc.Base64.stringify(CryptoJS.enc.Utf8.parse('hello'));  // aGVsbG8=

// Decode
atob('aGVsbG8=');                           // hello
pm.crypto.base64Decode('aGVsbG8=');         // hello (byte string)
CryptoJS.enc.Utf8.stringify(CryptoJS.enc.Base64.parse('aGVsbG8=')); // hello

// ── Postman-compatible: btoa accepts a CryptoJS digest object ──
btoa(CryptoJS.MD5('abc'));                  // kAFQmDzST7DWlj99KOF/cg== (base64 of digest bytes)
btoa(CryptoJS.HmacSHA256(msg, key));        // equivalent to .toString(CryptoJS.enc.Base64)

// ── Chinese: btoa needs pre-encoding; crypto-js is auto UTF-8 ──
btoa(unescape(encodeURIComponent('你好'))); // 5L2g5aW9
CryptoJS.enc.Base64.stringify(CryptoJS.enc.Utf8.parse('你好'));    // 5L2g5aW9
CryptoJS.enc.Utf8.stringify(CryptoJS.enc.Base64.parse('5L2g5aW9'));// 你好

// ── Base64url (for JWT): URL-safe, no padding ──
CryptoJS.enc.Base64url.stringify(CryptoJS.enc.Utf8.parse('hello'));
CryptoJS.enc.Base64url.parse('aGVsbG8');    // decode (auto pads =)`,
    },
    {
      title: "Byte-string helpers (pm.utf8 / pm.hex / pm.b64)",
      body: [
        "Orbit provides byte-string codec helpers (a byte string is a JS string where every char is 0-255), handy for custom protocols, binary bodies and signing flows combined with `pm.crypto`.",
        "`pm.utf8.encode(str)` → UTF-8 byte string; `pm.utf8.decode(bytes)` → original string.",
        "`pm.hex.encode(bytes)` → hex string; `pm.hex.decode(hexStr)` → byte string.",
        "`pm.b64.encode(bytes)` → base64 string; `pm.b64.decode(b64)` → byte string.",
      ],
      code: `// byte string → hex / base64 (all based on the same byte string)
var raw = pm.request.body.raw;
var bytes = pm.utf8.encode(raw);   // UTF-8 byte string
var hex = pm.hex.encode(bytes);
var b64 = pm.b64.encode(bytes);

// Reverse: hex / base64 → original string
var back = pm.utf8.decode(pm.hex.decode(hex));
var back2 = pm.utf8.decode(pm.b64.decode(b64));

// pm.crypto AES takes hex directly
var keyHex = pm.hex.encode(pm.utf8.encode('16byte-secret!!'));
var ivHex = '000102030405060708090a0b0c0d0e0f';
var ct = pm.crypto.aesEncrypt({ data: hex, key: keyHex, iv: ivHex, mode: CryptoJS.mode.CBC, outputType: 'base64' });`,
    },
    {
      title: "Compute signature / hash (pm.crypto)",
      body: [
        "`pm.crypto` provides md5 / sha1 / sha224 / sha256 / sha384 / sha512 / sha3 / ripemd160 / hmac / hmacBase64 / base64Encode / base64Decode / aesEncrypt / aesDecrypt / getRandomValues. All return hex except where base64 is noted.",
        "`hmac(algo, key, msg)` accepts `md5` / `sha1` / `sha224` / `sha256` / `sha384` / `sha512` / `ripemd160` / `sha3-224/256/384/512`. Implemented in Rust — auditable.",
        "`aesEncrypt({ data, key, iv, mode, outputType })` takes an object (Postman official signature); data/key/iv are hex strings; `mode` is `CryptoJS.mode.CBC/ECB` (Rust impl supports CBC/ECB); `outputType` is `hex` / `base64` / `string`.",
        "`getRandomValues(nBytes)` returns n random bytes as hex (WordArray-compatible, `toString(CryptoJS.enc.Hex)`).",
      ],
      code: `// ── Hashing algorithms ──
pm.crypto.md5('hello')                       // 32-char hex
pm.crypto.sha1('hello')                       // 40-char hex
pm.crypto.sha224('hello')                     // 56-char hex
pm.crypto.sha256('hello')                     // 64-char hex
pm.crypto.sha384('hello')                     // 96-char hex
pm.crypto.sha512('hello')                     // 128-char hex
pm.crypto.sha3('hello', 512)                  // SHA3-512
pm.crypto.sha3('hello', 256)                  // SHA3-256
pm.crypto.ripemd160('hello')                  // 40-char hex

// ── HMAC ──
pm.crypto.hmac('sha256', secret, body)        // hex
pm.crypto.hmacBase64('sha256', secret, body)  // base64
pm.crypto.hmac('sha512', secret, body)        // same pattern for other algos
pm.crypto.hmac('ripemd160', secret, body)

// ── Random bytes ──
pm.crypto.getRandomValues(16).toString(CryptoJS.enc.Hex)   // 16 random bytes as hex
pm.crypto.getRandomValues(32)                              // already a hex string`,
    },
    {
      title: "Signing cheat-sheet (pm.crypto vs CryptoJS)",
      body: [
        "For the same HMAC-SHA256 task, all APIs produce **identical output** (guaranteed by integration tests) — pick whichever you like:",
        "- hex output: `pm.crypto.hmac(...)` ≡ `CryptoJS.HmacSHA256(...).toString()`",
        "- base64 output: `pm.crypto.hmacBase64(...)` ≡ `CryptoJS.HmacSHA256(...).toString(CryptoJS.enc.Base64)`",
        "For Chinese messages: `pm.crypto` treats the string as UTF-8 internally; `CryptoJS` does the same for string inputs.",
      ],
      code: `// ── Task: HMAC-SHA256(body, secret), output hex ──
var sign1 = pm.crypto.hmac('sha256', secret, body);          // (A) pm.crypto
var sign2 = CryptoJS.HmacSHA256(body, secret).toString();    // (B) CryptoJS
// sign1 === sign2

// ── Task: output base64 (commonly for Authorization headers) ──
var b1 = pm.crypto.hmacBase64('sha256', secret, body);
var b2 = CryptoJS.HmacSHA256(body, secret).toString(CryptoJS.enc.Base64);
var b3 = CryptoJS.enc.Base64.stringify(CryptoJS.HmacSHA256(body, secret));
// b1 === b2 === b3

// ── Task: SHA-256 digest ──
pm.crypto.sha256(body)                        // hex
CryptoJS.SHA256(body).toString()              // hex, equivalent
CryptoJS.SHA256(body).toString(CryptoJS.enc.Base64)   // base64 variant

// ── Task: Chinese message signing (both treat as UTF-8, same result) ──
pm.crypto.hmac('sha256', secret, '订单号A001');
CryptoJS.HmacSHA256('订单号A001', secret).toString();`,
    },
    {
      title: "AES encryption cheat-sheet",
      body: [
        "`CryptoJS.AES` (official library) supports passphrase mode and raw-key mode; `pm.crypto.aesEncrypt` (Rust) uses hex exchange, CBC/ECB only.",
        "Passphrase mode emits the OpenSSL `Salted__` format, mutually decryptable with `openssl enc -aes-256-cbc -pass pass:xxx`.",
      ],
      code: `// ── Way 1: CryptoJS passphrase mode (most common, Salted__ format) ──
const ct1 = CryptoJS.AES.encrypt('hello world', 'mySecret').toString();
const pt1 = CryptoJS.AES.decrypt(ct1, 'mySecret').toString(CryptoJS.enc.Utf8);  // hello world

// ── Way 2: CryptoJS raw-key mode (key/iv as WordArray) ──
const key = CryptoJS.enc.Hex.parse('2b7e151628aed2a6abf7158809cf4f3c');  // AES-128
const iv  = CryptoJS.enc.Hex.parse('000102030405060708090a0b0c0d0e0f');
const ct2 = CryptoJS.AES.encrypt('data', key, { iv }).toString(CryptoJS.enc.Base64);
const pt2 = CryptoJS.AES.decrypt(ct2, key, { iv }).toString(CryptoJS.enc.Utf8); // data

// ── Way 3: pm.crypto (Rust, hex exchange, CBC/ECB) ──
var keyHex = '2b7e151628aed2a6abf7158809cf4f3c';          // 16/24/32-byte hex
var ivHex  = '000102030405060708090a0b0c0d0e0f';          // 16-byte hex
var dataHex = CryptoJS.enc.Hex.stringify(CryptoJS.enc.Utf8.parse('data'));
var ct3 = pm.crypto.aesEncrypt({ data: dataHex, key: keyHex, iv: ivHex, mode: CryptoJS.mode.CBC, outputType: 'hex' });
var pt3 = pm.crypto.aesDecrypt({ data: ct3, key: keyHex, iv: ivHex, mode: CryptoJS.mode.CBC, outputType: 'string' }); // data

// ── Way 4: DES / TripleDES (same shape as crypto-js) ──
var d = CryptoJS.DES.encrypt('msg', 'secret').toString();
CryptoJS.DES.decrypt(d, 'secret').toString(CryptoJS.enc.Utf8);
var t = CryptoJS.TripleDES.encrypt('msg', 'secret').toString();
CryptoJS.TripleDES.decrypt(t, 'secret').toString(CryptoJS.enc.Utf8);`,
    },
    {
      title: "JWT signing: complete example",
      body: [
        "Full walkthrough: Base64url-encode header/payload + HMAC-SHA256 signature, build a JWT and write it to the Authorization header (pre-request script).",
        "Optionally use `require('uuid')` for the jti claim.",
      ],
      code: `// Pre-request: build a JWT (HS256)
var secret = pm.secret.get('JWT_SECRET');        // put the secret in Environment Manager → Secrets

function b64url(s) {
  return CryptoJS.enc.Base64url.stringify(CryptoJS.enc.Utf8.parse(s));
}

var header = b64url(JSON.stringify({ alg: 'HS256', typ: 'JWT' }));
var payload = b64url(JSON.stringify({
  sub: 'user_001',
  name: '管理员',
  iat: Math.floor(Date.now() / 1000),
  exp: Math.floor(Date.now() / 1000) + 3600,
  jti: require('uuid').v4(),
}));

var signingInput = header + '.' + payload;
var signature = CryptoJS.enc.Base64url.stringify(
  CryptoJS.HmacSHA256(signingInput, secret)
);

var token = signingInput + '.' + signature;
pm.request.headers.upsert({ key: 'Authorization', value: 'Bearer ' + token });
console.log('JWT:', token);`,
    },
    {
      title: "UUID / random values",
      body: [
        "Three ways to get random / UUID values: template dynamic values, `require('uuid')`, and `pm.crypto.getRandomValues`.",
      ],
      code: `// ── Way 1: template dynamic values (outside scripts, URL/Header/Body all work) ──
// {{$uuid}}  {{$timestamp}}  {{$randomInt}}  {{$guid}}

// ── Way 2: require('uuid') (v4) ──
var u = require('uuid').v4();      // xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx

// ── Way 3: pm.crypto.getRandomValues ──
var hex16 = pm.crypto.getRandomValues(16);         // 32-char random hex
var salt = CryptoJS.enc.Hex.parse(pm.crypto.getRandomValues(8)); // 8-byte salt (WordArray)`,
    },
    {
      title: "CryptoJS official library",
      body: [
        "Ships the **full official crypto-js 4.2.0** (identical to Postman); `require('crypto-js')` and the global `CryptoJS` are the **same instance**.",
        "Hashing: `MD5` / `SHA1` / `SHA224` / `SHA256` / `SHA384` / `SHA512` / `SHA3(msg, {outputLength})` / `RIPEMD160`; HMAC variants: `HmacMD5` / `HmacSHA1` / `HmacSHA224` / `HmacSHA256` / `HmacSHA384` / `HmacSHA512` / `HmacSHA3` / `HmacRIPEMD160`.",
        "Symmetric ciphers: `AES` / `DES` / `TripleDES` / `Rabbit` / `RabbitLegacy` / `RC4` / `RC4Drop` (`encrypt(message, key, cfg)` / `decrypt(ciphertext, key, cfg)`). A string key uses **passphrase mode** (OpenSSL `Salted__` format, mutually decryptable with the openssl CLI); a WordArray key uses raw-key mode.",
        "Encoders `enc.*`: `Hex` / `Latin1` / `Utf8` / `Utf16` / `Utf16LE` / `Base64` / `Base64url` (`parse` / `stringify`); modes `mode.*`: `CBC` (default) / `ECB` / `CFB` / `OFB` / `CTR`; paddings `pad.*`: `Pkcs7` (default) / `NoPadding` / `ZeroPadding` / `AnsiX923` / `Iso10126` / `Iso97971`.",
        "`.toString()` returns hex by default; `.toString(CryptoJS.enc.Base64)` returns base64. AES.encrypt returns a CipherParams object whose `.toString()` is the OpenSSL-format string.",
      ],
      code: `const CryptoJS = require('crypto-js');
// or use the global CryptoJS directly (same instance)

// ── Hashing / HMAC ──
CryptoJS.MD5('x').toString();
CryptoJS.SHA3('x', { outputLength: 256 }).toString();
CryptoJS.HmacSHA256(msg, key).toString(CryptoJS.enc.Base64);

// ── Advanced: AES-CBC + ZeroPadding + raw key ──
var key = CryptoJS.enc.Hex.parse('2b7e151628aed2a6abf7158809cf4f3c');
var iv  = CryptoJS.enc.Hex.parse('000102030405060708090a0b0c0d0e0f');
var ct = CryptoJS.AES.encrypt('data', key, {
  mode: CryptoJS.mode.CBC,
  padding: CryptoJS.pad.ZeroPadding,
  iv: iv,
}).toString(CryptoJS.enc.Base64);
CryptoJS.AES.decrypt(ct, key, { mode: CryptoJS.mode.CBC, padding: CryptoJS.pad.ZeroPadding, iv: iv })
  .toString(CryptoJS.enc.Utf8);

// ── Passphrase mode (OpenSSL compatible, CLI-interoperable) ──
const ct2 = CryptoJS.AES.encrypt('hello', 'secret').toString();
const pt2 = CryptoJS.AES.decrypt(ct2, 'secret').toString(CryptoJS.enc.Utf8);

// ── Chinese-safe Base64 (official approach) ──
const b64 = CryptoJS.enc.Base64.stringify(CryptoJS.enc.Utf8.parse('你好'));`,
    },
    {
      title: "Read the response (post-response)",
      body: [
        "The post-response script can read the response. `pm.response.json()` parses JSON (returns `null` on failure); `pm.response.text()` returns the raw text.",
      ],
      code: `var data = pm.response.json();      // parse JSON, null on failure
var text = pm.response.text();      // raw text
pm.response.code;                   // status code, e.g. 200
pm.response.responseTime;           // elapsed(ms)
pm.response.headers['Content-Type'];

// The response can also feed assertions
pm.test('body is a JSON array', function () {
  pm.expect(pm.response.json()).to.be.a('array');
});`,
    },
    {
      title: "Extract variables",
      body: [
        "Variables written via `pm.environment.set(name, value)` are merged back into the active environment after the run (overriding same-named reads from `pm.environment.get`).",
        "This lets you extract a token from the response into the environment for later requests via `{{token}}`.",
        "For intermediate values only needed within this request, prefer `pm.variables.set` (not persisted, no side effects).",
      ],
      code: `var token = pm.response.json().token;
pm.environment.set('token', token);   // later use {{token}}

// temporary within this request
pm.variables.set('page', pm.response.json().page);

// Postman aliases work too
pm.globals.set('last_status', pm.response.code);`,
    },
    {
      title: "Assertions (pm.test / pm.expect)",
      body: [
        "Use `pm.test(name, fn)` to define an assertion and `pm.expect` inside `fn` to verify. A failed assertion is caught and recorded without aborting the script.",
        'All assertions are shown in the response panel under "Script" → Tests, green for passed, red for failed.',
      ],
      code: `pm.test('status is 200', function () {
  pm.expect(pm.response.code).to.equal(200);
});
pm.test('has token', function () {
  pm.expect(pm.response.json().token).to.equal('abc123');
});
pm.test('array length', function () {
  pm.expect(pm.response.json().arr).to.have.property('2');
});
pm.test('type check', function () {
  pm.expect('hi').to.be.a('string');
});
// A failing assertion is recorded but does not abort the script
pm.test('name is non-empty', function () {
  pm.expect(pm.response.json().name).to.be.ok;
});`,
      bullets: [
        "`to.equal(expected)` — loose equality (==)",
        "`to.eql(expected)` — deep equality (JSON compare)",
        "`to.contain(sub)` — string contains",
        "`to.be.true` / `to.be.false` / `to.be.null` / `to.be.undefined` / `to.be.ok` (truthy)",
        "`to.be.a('string'|'number'|'boolean'|'object'|'array')`",
        "`to.have.property('key')`",
      ],
    },
    {
      title: "Debug output (console)",
      body: [
        'Use `console.log` / `console.warn` / `console.error` to print debug info, shown in the response panel\'s "Script" logs area.',
        "Objects are auto-pretty-printed as multi-line JSON. Each log line can be expanded / collapsed, so long JSON no longer requires scrolling raw text.",
      ],
      code: `console.log('current body:', pm.request.body.raw);
console.log({ a: 1, b: [1, 2, 3] });   // pretty-printed
console.warn('note'); console.error('oops');`,
    },
    {
      title: "Full example: sign and write to Header",
      body: [
        'The classic "body + secret → HMAC, write to header" scenario. Because dynamic values are resolved in step 1, `pm.request.body.raw` here is already the final content.',
      ],
      code: `// Pre-request script
var body = pm.request.body.raw;
var secret = pm.secret.get('SECRET');   // read a secret (configured in Environment Manager → Secrets)

// Way A: pm.crypto (hex)
var sign = pm.crypto.hmac('sha256', secret, body + '|' + 'k1');
// Way B: CryptoJS (equivalent)
var sign2 = CryptoJS.HmacSHA256(body + '|' + 'k1', secret).toString();

pm.request.headers.upsert({ key: 'X-Signature', value: sign });
console.log('signature:', sign);`,
    },
    {
      title: "Notes",
      bullets: [
        "`pm.response` is unavailable in pre-request; `pm.request` is a read-only snapshot in post-response.",
        "Secrets are read-only: scripts may read them via `pm.secret.get` / `pm.environment.get`, but cannot write secrets via `set`.",
        "`pm.variables` are temporary (this request only, not persisted); `pm.environment` writes persist back to the environment.",
        "`pm.globals` / `pm.collectionVariables` are compatibility aliases mapped to environment variables, not separate scopes.",
        "`require(name)` supports only the built-in libraries (crypto-js, lodash, moment, uuid, atob, btoa, url, querystring); other modules throw. Scripts run in a Rust sandbox with no DOM / fetch / network / filesystem.",
        "`btoa` / `atob` are Latin-1: passing Chinese directly throws; use `unescape(encodeURIComponent(s))` or the crypto-js approach for Chinese.",
        "`pm.crypto.aesEncrypt/aesDecrypt` support only CBC/ECB; `CryptoJS.AES` (official library) supports CBC/ECB/CFB/OFB/CTR.",
        "A failed assertion is only recorded — it does not fail the request itself.",
        "Avoid printing very large strings to console; logs expand per line but still cost space.",
      ],
    },
  ],
};

export function getScriptsReference(locale: Locale): ScriptsReference {
  return locale === "en-US" ? EN : ZH;
}
