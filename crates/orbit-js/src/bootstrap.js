// orbit-js script bootstrap - Postman sandbox compatibility layer.
// Runs after the globals `pm` / `__orbit_crypto` (Rust crypto bridge) / `__orbitRequireLib` (Rust lib source bridge) are ready.
// require() supports Postman built-in libs: crypto-js / lodash / moment / uuid / atob / btoa,
// plus the Node-style modules url / querystring (provided by urlshim.js).
(function () {
  // ── console prettifying ──
  // Log writing is registered by Rust on each run (__orbit_console dispatches to this run's buffer),
  // wrapped here: objects/arrays are pretty-printed as multi-line via JSON.stringify(..., 2), strings as-is.
  function __fmt(args) {
    var out = [];
    for (var i = 0; i < args.length; i++) {
      var a = args[i];
      if (typeof a === "string") out.push(a);
      else if (a === undefined) out.push("undefined");
      else if (a === null) out.push("null");
      else {
        try {
          out.push(JSON.stringify(a, null, 2));
        } catch (e) {
          out.push(String(a));
        }
      }
    }
    return out.join(" ");
  }
  globalThis.console = {
    log: function () {
      __orbit_console("log", __fmt(arguments));
    },
    error: function () {
      __orbit_console("error", __fmt(arguments));
    },
    warn: function () {
      __orbit_console("warn", __fmt(arguments));
    },
  };
  // Save the pristine wrapper methods and restore them before each run (to prevent scripts overwriting/tampering with console)
  globalThis.__orbitConsoleMethods = {
    log: globalThis.console.log,
    error: globalThis.console.error,
    warn: globalThis.console.warn,
  };

  // -- Rust bridge error detection: Rust closures return a "!!__orbit_err__::msg" marker, converted here into a JS exception --
  var __ORBIT_ERR = "!!__orbit_err__::";
  function __checkErr(v) {
    if (typeof v === "string" && v.indexOf(__ORBIT_ERR) === 0) {
      throw new Error(v.slice(__ORBIT_ERR.length));
    }
    return v;
  }
  // Wrap the global btoa/atob
  // Postman compatible: when btoa receives a CryptoJS WordArray it converts directly to byte Base64 (e.g. btoa(CryptoJS.MD5(x)))
  var __rBtoa = globalThis.btoa,
    __rAtob = globalThis.atob;
  globalThis.btoa = function (s) {
    if (typeof s === "string") return __checkErr(__rBtoa(s));
    // CryptoJS WordArray (words + sigBytes) -> byte Base64
    if (
      s &&
      typeof s === "object" &&
      s.words &&
      typeof s.sigBytes === "number"
    ) {
      var C = globalThis.CryptoJS;
      if (C && C.enc && C.enc.Base64) return C.enc.Base64.stringify(s);
      throw new TypeError("btoa: unrecognized object type");
    }
    throw new TypeError(
      "btoa: argument must be a string or a CryptoJS WordArray (a hash digest object may also be passed directly, e.g. btoa(CryptoJS.MD5(x)))",
    );
  };
  globalThis.atob = function (s) {
    if (typeof s !== "string") {
      throw new TypeError("atob: argument must be a string");
    }
    return __checkErr(__rAtob(s));
  };
  // Save the pristine wrapper references and restore them before each run (to prevent script overwrites)
  globalThis.__orbitBtoa = globalThis.btoa;
  globalThis.__orbitAtob = globalThis.atob;

  // ════════════════════════════════════════════════════════════════
  //  require system (Postman sandbox compatible)
  //  Built-in official libs are embedded by the Rust side via include_str! (__orbitRequireLib returns the source),
  //  executed and cached by a UMD loader on first require; atob/btoa return the Rust global functions.
  // ════════════════════════════════════════════════════════════════
  var __orbitRequireCache = {};
  function __loadCjsModule(src) {
    var module = { exports: {} };
    var fn = new Function("module", "exports", "require", src);
    fn(module, module.exports, globalThis.require);
    return module.exports;
  }
  globalThis.require = function (name) {
    if (Object.prototype.hasOwnProperty.call(__orbitRequireCache, name)) {
      return __orbitRequireCache[name];
    }
    if (name === "atob" || name === "btoa") {
      __orbitRequireCache[name] = globalThis[name];
      return __orbitRequireCache[name];
    }
    // Node-style built-in modules (provided by the URL compatibility layer, see urlshim.js)
    if (name === "url" || name === "node:url") {
      __orbitRequireCache[name] = globalThis.__orbitUrlModule;
      return __orbitRequireCache[name];
    }
    if (name === "querystring" || name === "node:querystring") {
      __orbitRequireCache[name] = globalThis.__orbitQuerystringModule;
      return __orbitRequireCache[name];
    }
    var src = __checkErr(globalThis.__orbitRequireLib(name)); // Rust: returns the lib source; throws for non-built-in
    var mod = __loadCjsModule(src);
    __orbitRequireCache[name] = mod;
    return mod;
  };

  // -- Variable value type conversion (Postman semantics, shared by pm.environment/pm.variables.set) --
  // string as-is / number·bool to string / null·undefined empty string / object·array JSON.stringify
  function __orbitToStorageString(value) {
    if (typeof value === "string") return value;
    if (value === null || value === undefined) return "";
    if (typeof value === "object") return JSON.stringify(value);
    return String(value);
  }
  // The set backend is a per-run Rust write buffer; pm.environment/pm.variables are rebuilt each run,
  // and the Rust side binds set to them via this factory (single source of truth for type conversion)
  function __orbitStorageSet(backend) {
    return function (name, value) {
      backend(name, __orbitToStorageString(value));
    };
  }
  globalThis.__orbitStorageSet = __orbitStorageSet;

  // -- Web Crypto compatibility: the QuickJS sandbox has no native random source; getRandomValues delegates to Rust --
  // crypto-js's WordArray.random / uuid v4 both depend on it.
  var __cryptoObj =
    typeof globalThis.crypto === "object" && globalThis.crypto
      ? globalThis.crypto
      : {};
  __cryptoObj.getRandomValues = function (arr) {
    var bytes = __hexToBytes(B.randBytes(Math.max(0, arr.length)));
    for (var i = 0; i < arr.length; i++) arr[i] = bytes.charCodeAt(i) & 0xff;
    return arr;
  };
  globalThis.crypto = __cryptoObj;

  // Postman global CryptoJS: usable without require, and the same instance as require('crypto-js')
  var CryptoJS = (globalThis.CryptoJS = require("crypto-js"));
  var B = globalThis.__orbit_crypto; // Rust crypto bridge (pm.crypto backend)

  // -- Byte-string encode/decode helper (byte string = a JS string where each char is 0-255) --
  var __b64Chars =
    "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
  function __bytesToB64(b) {
    var out = "",
      i;
    for (i = 0; i + 2 < b.length; i += 3) {
      var n =
        (b.charCodeAt(i) << 16) |
        (b.charCodeAt(i + 1) << 8) |
        b.charCodeAt(i + 2);
      out +=
        __b64Chars.charAt((n >> 18) & 63) +
        __b64Chars.charAt((n >> 12) & 63) +
        __b64Chars.charAt((n >> 6) & 63) +
        __b64Chars.charAt(n & 63);
    }
    var rem = b.length - i;
    if (rem === 1) {
      var n = b.charCodeAt(i) << 16;
      out +=
        __b64Chars.charAt((n >> 18) & 63) +
        __b64Chars.charAt((n >> 12) & 63) +
        "==";
    } else if (rem === 2) {
      var n = (b.charCodeAt(i) << 16) | (b.charCodeAt(i + 1) << 8);
      out +=
        __b64Chars.charAt((n >> 18) & 63) +
        __b64Chars.charAt((n >> 12) & 63) +
        __b64Chars.charAt((n >> 6) & 63) +
        "=";
    }
    return out;
  }
  function __b64ToBytes(s) {
    var out = "",
      buf = 0,
      bits = 0;
    for (var i = 0; i < s.length; i++) {
      var ch = s.charAt(i);
      if (ch === "=" || ch === "\n" || ch === "\r" || ch === " " || ch === "\t")
        continue;
      var v = __b64Chars.indexOf(ch);
      if (v < 0) continue;
      buf = (buf << 6) | v;
      bits += 6;
      if (bits >= 8) {
        bits -= 8;
        out += String.fromCharCode((buf >> bits) & 255);
        buf &= (1 << bits) - 1;
      }
    }
    return out;
  }
  function __bytesToHex(b) {
    var out = "";
    for (var i = 0; i < b.length; i++) {
      var h = b.charCodeAt(i).toString(16);
      out += (h.length < 2 ? "0" : "") + h;
    }
    return out;
  }
  function __hexToBytes(s) {
    var str = String(s).replace(/\s+/g, ""),
      out = "";
    for (var i = 0; i + 1 < str.length; i += 2) {
      out += String.fromCharCode(parseInt(str.substr(i, 2), 16));
    }
    return out;
  }
  function __utf8ToBytes(s) {
    var out = "";
    for (var i = 0; i < s.length; i++) {
      var c = s.charCodeAt(i);
      if (c < 0x80) {
        out += String.fromCharCode(c);
      } else if (c < 0x800) {
        out += String.fromCharCode(0xc0 | (c >> 6), 0x80 | (c & 0x3f));
      } else {
        out += String.fromCharCode(
          0xe0 | (c >> 12),
          0x80 | ((c >> 6) & 0x3f),
          0x80 | (c & 0x3f),
        );
      }
    }
    return out;
  }
  function __bytesToUtf8(b) {
    var out = "";
    for (var i = 0; i < b.length; i++) {
      var c = b.charCodeAt(i);
      if (c < 0x80) {
        out += String.fromCharCode(c);
      } else if (c < 0xe0 && i + 1 < b.length) {
        out += String.fromCharCode(
          ((c & 0x1f) << 6) | (b.charCodeAt(++i) & 0x3f),
        );
      } else if (i + 2 < b.length) {
        out += String.fromCharCode(
          ((c & 0x0f) << 12) |
            ((b.charCodeAt(i + 1) & 0x3f) << 6) |
            (b.charCodeAt(i + 2) & 0x3f),
        );
        i += 2;
      }
    }
    return out;
  }

  // -- Input normalization: string -> UTF-8 -> hex; WordArray -> hex --
  function hexOf(x) {
    if (x === undefined || x === null) return "";
    if (typeof x === "string")
      return CryptoJS.enc.Hex.stringify(CryptoJS.enc.Utf8.parse(x));
    return CryptoJS.enc.Hex.stringify(x);
  }

  // -- Legacy flat object proxies (request / response injected by Rust) --
  globalThis.__makeHeaderProxy = function (backing, readOnly) {
    return new Proxy(backing, {
      get: function (t, k) {
        if (k === "get")
          return function (key) {
            return key in t ? t[key] : "";
          };
        if (!readOnly && (k === "set" || k === "upsert" || k === "add"))
          return function (o, v) {
            var key = typeof o === "object" ? o.key : o;
            var val = typeof o === "object" ? o.value : v !== undefined ? v : o;
            t[key] = val;
          };
        if (!readOnly && k === "remove")
          return function (key) {
            delete t[key];
          };
        if (k === "toJSON")
          return function () {
            return t;
          };
        if (k === "keys")
          return function () {
            return Object.keys(t);
          };
        return t[k];
      },
      set: function (t, k, v) {
        if (readOnly) return true;
        t[k] = v;
        return true;
      },
      deleteProperty: function (t, k) {
        if (readOnly) return true;
        delete t[k];
        return true;
      },
      has: function (t, k) {
        return k in t;
      },
    });
  };
  pm.utf8 = { encode: __utf8ToBytes, decode: __bytesToUtf8 };
  pm.hex = { encode: __bytesToHex, decode: __hexToBytes };
  pm.b64 = { encode: __bytesToB64, decode: __b64ToBytes };

  // ── pm.test / pm.expect ──
  pm._tests = [];
  pm.test = function (name, fn) {
    try {
      if (typeof fn === "function") {
        fn();
      }
      pm._tests.push({ name: name, passed: true, message: "" });
    } catch (e) {
      pm._tests.push({
        name: name,
        passed: false,
        message: e && e.message ? e.message : String(e),
      });
    }
  };
  pm.expect = function (actual) {
    // Postman-style negation: both `.to.not.xxx(...)` and `.not.to.xxx(...)` forms are supported
    var negate = false;
    function fail(msg) {
      throw new Error(msg);
    }
    // Assertion convergence point: `ok` = whether the positive expectation holds; on the negate chain the meaning is inverted
    function assert(ok, msg) {
      if (negate) {
        if (ok) fail("not (" + msg + ")");
      } else if (!ok) {
        fail(msg);
      }
    }
    // chai-style `a/an(type)`: arrays and null can't be checked with typeof
    function typeOk(type) {
      if (type === "array") return Array.isArray(actual);
      if (type === "null") return actual === null;
      return typeof actual === type;
    }
    function checkType(type) {
      assert(typeOk(type), "expected type " + type + " but got " + typeof actual);
    }
    function contains(e) {
      return !!(
        actual &&
        (actual.indexOf(e) >= 0 || (actual.includes && actual.includes(e)))
      );
    }
    var beHandler = {
      get: function (t, k) {
        if (k === "true") {
          assert(actual === true, "expected true");
          return undefined;
        }
        if (k === "false") {
          assert(actual === false, "expected false");
          return undefined;
        }
        if (k === "null") {
          assert(actual === null, "expected null");
          return undefined;
        }
        if (k === "undefined") {
          assert(actual === undefined, "expected undefined");
          return undefined;
        }
        if (k === "ok") {
          assert(!!actual, "expected truthy value");
          return undefined;
        }
        // `be.a(type)` / `be.an(type)` are equivalent (both forms are common in Postman)
        if (k === "a" || k === "an") {
          return function (type) {
            checkType(type);
          };
        }
        return undefined;
      },
    };
    var to = {
      equal: function (e) {
        assert(actual == e, "expected " + actual + " to equal " + e);
      },
      eql: function (e) {
        assert(
          JSON.stringify(actual) === JSON.stringify(e),
          "expected " + JSON.stringify(actual) + " to deep equal " + JSON.stringify(e)
        );
      },
      contain: function (e) {
        assert(contains(e), "expected " + actual + " to contain " + e);
      },
      be: new Proxy({}, beHandler),
      have: {
        property: function (p) {
          assert(actual && actual[p] !== undefined, "expected to have property " + p);
        },
      },
    };
    // `include` is Postman's spelling of `contain` (equivalent)
    to.include = to.contain;
    var api = { to: to };
    Object.defineProperty(api, "not", {
      get: function () {
        negate = true;
        return api;
      },
    });
    Object.defineProperty(to, "not", {
      get: function () {
        negate = true;
        return to;
      },
    });
    return api;
  };

  // -- pm.crypto: Postman semantics (all backed by Rust) --
  function __pmc(rustFn) {
    return function () {
      return __checkErr(rustFn.apply(null, arguments));
    };
  }
  pm.crypto.md5 = __pmc(function (data) {
    return B.md5(hexOf(data));
  });
  pm.crypto.sha1 = __pmc(function (data) {
    return B.sha1(hexOf(data));
  });
  pm.crypto.sha224 = __pmc(function (data) {
    return B.sha224(hexOf(data));
  });
  pm.crypto.sha256 = __pmc(function (data) {
    return B.sha256(hexOf(data));
  });
  pm.crypto.sha384 = __pmc(function (data) {
    return B.sha384(hexOf(data));
  });
  pm.crypto.sha512 = __pmc(function (data) {
    return B.sha512(hexOf(data));
  });
  pm.crypto.sha3 = __pmc(function (data, outputLength) {
    return B.sha3(hexOf(data), outputLength || 512);
  });
  pm.crypto.ripemd160 = __pmc(function (data) {
    return B.ripemd160(hexOf(data));
  });
  pm.crypto.hmac = __pmc(function (algo, secret, data) {
    return B.hmac(String(algo), hexOf(secret), hexOf(data));
  });
  pm.crypto.hmacBase64 = __pmc(function (algo, secret, data) {
    return B.hmacBase64(String(algo), hexOf(secret), hexOf(data));
  });
  pm.crypto.base64Encode = __pmc(function (data) {
    return B.b64Encode(hexOf(data));
  });
  pm.crypto.base64Decode = __pmc(function (s) {
    return __hexToBytes(B.b64Decode(String(s)));
  });
  function pmcAes(encrypt, opts) {
    if (!opts || typeof opts !== "object") {
      throw new Error(
        "pm.crypto.aes" +
          (encrypt ? "Encrypt" : "Decrypt") +
          " requires an object argument {data, key, iv, mode, outputType}",
      );
    }
    var dataHex = hexOf(
      opts.data === undefined || opts.data === null ? "" : opts.data,
    );
    var keyHex = hexOf(
      opts.key === undefined || opts.key === null ? "" : opts.key,
    );
    var ivHex = opts.iv === undefined || opts.iv === null ? "" : hexOf(opts.iv);
    var m = opts.mode;
    var modeStr = "cbc";
    if (typeof m === "string") modeStr = m.toLowerCase();
    else if (m && m === CryptoJS.mode.ECB) modeStr = "ecb";
    var out = encrypt
      ? B.aesEncrypt(dataHex, keyHex, ivHex, modeStr, "pkcs7")
      : B.aesDecrypt(dataHex, keyHex, ivHex, modeStr, "pkcs7");
    out = __checkErr(out);
    var ot = (opts.outputType || "hex").toLowerCase();
    if (ot === "base64" || ot === "b64") return B.b64Encode(out);
    if (ot === "string") return __bytesToUtf8(__hexToBytes(out));
    return out; // hex (default)
  }
  pm.crypto.aesEncrypt = function (opts) {
    return pmcAes(true, opts);
  };
  pm.crypto.aesDecrypt = function (opts) {
    return pmcAes(false, opts);
  };
  pm.crypto.getRandomValues = function (nBytes) {
    return CryptoJS.enc.Hex.parse(B.randBytes(Math.max(0, nBytes | 0)));
  };

  // -- Postman compatibility aliases (Orbit has no separate globals / collectionVariables; mapped to environment variables) --
  pm.globals = pm.environment;
  pm.collectionVariables = pm.environment;
  pm.iterationData = new Proxy(
    {},
    {
      get: function () {
        return "";
      },
    },
  );
  var __reqName = "";
  try {
    __reqName = (pm.request && (pm.request.name || "")) || "";
  } catch (e) {}
  pm.info = {
    eventName: "script",
    iteration: 1,
    iterationCount: 1,
    requestName: __reqName,
  };
})();
