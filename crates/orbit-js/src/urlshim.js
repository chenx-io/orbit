// orbit-js built-in URL / URLSearchParams compatibility layer.
//
// QuickJS only implements ECMAScript built-in objects and lacks the browser's URL / URLSearchParams.
// This provides, in pure JS, capabilities matching a common subset of WHATWG URL / Node's `url` module,
// for pre/post scripts to handle query parameters (a common Apifox / Postman scripting use).
//
// Injected once by the Rust side before bootstrap and re-injected on every script run reset (to prevent tampering).
// Exposes:
//   - globalThis.URL            — new URL(input[, base])
//   - globalThis.URLSearchParams
//   - globalThis.__orbitUrlModule        — require('url')
//   - globalThis.__orbitQuerystringModule — require('querystring')
//   - globalThis.__orbitDecorateRequest(req) - attaches query convenience methods to pm.request
(function () {
  "use strict";

  // ── application/x-www-form-urlencoded encode/decode (aligned with URLSearchParams semantics) ──
  function encodePart(s) {
    return encodeURIComponent(String(s))
      .replace(/%20/g, "+")
      .replace(/[!'()~]/g, function (c) {
        return "%" + c.charCodeAt(0).toString(16).toUpperCase();
      });
  }
  function decodePart(s) {
    var t = String(s).replace(/\+/g, " ");
    try {
      return decodeURIComponent(t);
    } catch (e) {
      return t;
    }
  }

  // ════════════════════════════════════════════════════════════════
  //  URLSearchParams
  // ════════════════════════════════════════════════════════════════
  function URLSearchParams(init) {
    if (!(this instanceof URLSearchParams)) return new URLSearchParams(init);
    this._entries = [];
    if (init === undefined || init === null) return;
    var self = this;
    var i;
    if (typeof init === "string") {
      var s = init.charAt(0) === "?" ? init.slice(1) : init;
      if (s.length) {
        var parts = s.split("&");
        for (i = 0; i < parts.length; i++) {
          if (parts[i] === "") continue;
          var eq = parts[i].indexOf("=");
          if (eq < 0) self._entries.push([decodePart(parts[i]), ""]);
          else
            self._entries.push([
              decodePart(parts[i].slice(0, eq)),
              decodePart(parts[i].slice(eq + 1)),
            ]);
        }
      }
    } else if (typeof init === "object") {
      if (typeof init.forEach === "function" && typeof init.length !== "number") {
        // Iterable objects such as Map / URLSearchParams
        init.forEach(function (value, key) {
          self._entries.push([String(key), String(value)]);
        });
      } else if (typeof init.length === "number") {
        // Array or array-like: [ [k, v], ... ] or a single [k, v] pair
        for (i = 0; i < init.length; i++) {
          var pair = init[i];
          if (pair && typeof pair.length === "number" && pair.length >= 2) {
            self._entries.push([String(pair[0]), String(pair[1])]);
          } else {
            self._entries.push([String(pair), ""]);
          }
        }
      } else {
        // Plain object
        for (var k in init) {
          if (Object.prototype.hasOwnProperty.call(init, k)) {
            self._entries.push([String(k), String(init[k])]);
          }
        }
      }
    }
  }

  URLSearchParams.prototype.append = function (name, value) {
    this._entries.push([String(name), String(value)]);
  };
  URLSearchParams.prototype["delete"] = function (name, value) {
    var n = String(name);
    if (arguments.length > 1) {
      var v = String(value);
      this._entries = this._entries.filter(function (e) {
        return !(e[0] === n && e[1] === v);
      });
    } else {
      this._entries = this._entries.filter(function (e) {
        return e[0] !== n;
      });
    }
  };
  URLSearchParams.prototype.get = function (name) {
    var n = String(name);
    for (var i = 0; i < this._entries.length; i++) {
      if (this._entries[i][0] === n) return this._entries[i][1];
    }
    return null;
  };
  URLSearchParams.prototype.getAll = function (name) {
    var n = String(name);
    var out = [];
    for (var i = 0; i < this._entries.length; i++) {
      if (this._entries[i][0] === n) out.push(this._entries[i][1]);
    }
    return out;
  };
  URLSearchParams.prototype.has = function (name, value) {
    var n = String(name);
    for (var i = 0; i < this._entries.length; i++) {
      if (this._entries[i][0] === n) {
        if (arguments.length > 1 && this._entries[i][1] !== String(value)) continue;
        return true;
      }
    }
    return false;
  };
  URLSearchParams.prototype.set = function (name, value) {
    var n = String(name),
      v = String(value),
      found = false,
      out = [];
    for (var i = 0; i < this._entries.length; i++) {
      if (this._entries[i][0] === n) {
        if (!found) {
          out.push([n, v]);
          found = true;
        }
      } else {
        out.push(this._entries[i]);
      }
    }
    if (!found) out.push([n, v]);
    this._entries = out;
  };
  URLSearchParams.prototype.sort = function () {
    this._entries.sort(function (a, b) {
      if (a[0] < b[0]) return -1;
      if (a[0] > b[0]) return 1;
      return 0;
    });
  };
  URLSearchParams.prototype.toString = function () {
    var out = [];
    for (var i = 0; i < this._entries.length; i++) {
      out.push(encodePart(this._entries[i][0]) + "=" + encodePart(this._entries[i][1]));
    }
    return out.join("&");
  };
  URLSearchParams.prototype.forEach = function (cb, thisArg) {
    for (var i = 0; i < this._entries.length; i++) {
      cb.call(thisArg, this._entries[i][1], this._entries[i][0], this);
    }
  };
  URLSearchParams.prototype.entries = function () {
    return this._entries.slice();
  };
  URLSearchParams.prototype.keys = function () {
    var out = [];
    for (var i = 0; i < this._entries.length; i++) out.push(this._entries[i][0]);
    return out;
  };
  URLSearchParams.prototype.values = function () {
    var out = [];
    for (var i = 0; i < this._entries.length; i++) out.push(this._entries[i][1]);
    return out;
  };

  // ════════════════════════════════════════════════════════════════
  //  URL
  // ════════════════════════════════════════════════════════════════
  // Groups: 1=scheme 2=userinfo 3=hostname 4=port 5=path 6=search 7=hash
  var URL_RE =
    /^(?:([A-Za-z][A-Za-z0-9+.\-]*):)?(?:\/\/(?:([^:@\/?#]*)@)?([^:\/?#]*)(?::(\d*))?)?([^?#]*)(?:\?([^#]*))?(?:#([\s\S]*))?$/;

  function formatUrl(u) {
    var auth = "";
    if (u.username) {
      auth = u.username;
      if (u.password) auth += ":" + u.password;
      auth += "@";
    }
    var host = u.hostname || "";
    if (u.port) host += ":" + u.port;
    var slash = u._authority || host ? "//" : "";
    var search = u._search ? "?" + u._search : "";
    return (u.protocol || "") + slash + auth + host + (u.pathname || "") + search + (u.hash || "");
  }

  function resolveRelative(base, rel) {
    var b = new URL(base);
    if (/^[A-Za-z][A-Za-z0-9+.\-]*:/.test(rel)) return rel;
    var origin =
      (b.protocol || "") + (b._authority || b.hostname ? "//" + b.host : "");
    if (rel.charAt(0) === "/") return origin + rel;
    if (rel.charAt(0) === "#") {
      return origin + b.pathname + (b._search ? "?" + b._search : "") + rel;
    }
    if (rel.charAt(0) === "?") return origin + b.pathname + rel;
    var dir = b.pathname.replace(/[^/]*$/, "");
    return origin + dir + rel;
  }

  function URL(input, base) {
    if (!(this instanceof URL)) return new URL(input, base);
    if (input instanceof URL) input = input.href;
    input = String(input);
    if (base !== undefined && base !== null && !/^[A-Za-z][A-Za-z0-9+.\-]*:/.test(input)) {
      input = resolveRelative(base, input);
    }
    var m = URL_RE.exec(input);
    if (!m) throw new TypeError("Invalid URL: " + input);
    this.protocol = m[1] ? m[1].toLowerCase() + ":" : "";
    var userinfo = m[2] || "";
    var ci = userinfo.indexOf(":");
    if (ci >= 0) {
      this.username = userinfo.slice(0, ci);
      this.password = userinfo.slice(ci + 1);
    } else {
      this.username = userinfo;
      this.password = "";
    }
    this.hostname = m[3] || "";
    this.port = m[4] || "";
    this.pathname = m[5] || "";
    this._search = m[6] !== undefined ? m[6] : "";
    this.hash = m[7] !== undefined ? "#" + m[7] : "";
    this._authority = m[3] !== undefined;
  }

  Object.defineProperties(URL.prototype, {
    href: {
      enumerable: true,
      get: function () {
        return formatUrl(this);
      },
      set: function (v) {
        var u = new URL(v);
        this.protocol = u.protocol;
        this.username = u.username;
        this.password = u.password;
        this.hostname = u.hostname;
        this.port = u.port;
        this.pathname = u.pathname;
        this._search = u._search;
        this.hash = u.hash;
        this._authority = u._authority;
      },
    },
    host: {
      enumerable: true,
      get: function () {
        return (this.hostname || "") + (this.port ? ":" + this.port : "");
      },
      set: function (v) {
        v = String(v);
        var ci = v.lastIndexOf(":");
        if (ci >= 0) {
          this.hostname = v.slice(0, ci);
          this.port = v.slice(ci + 1);
        } else {
          this.hostname = v;
          this.port = "";
        }
      },
    },
    origin: {
      enumerable: true,
      get: function () {
        return (this.protocol || "") + "//" + this.host;
      },
    },
    search: {
      enumerable: true,
      get: function () {
        return this._search ? "?" + this._search : "";
      },
      set: function (v) {
        this._search = v ? String(v).replace(/^\?/, "") : "";
      },
    },
    searchParams: {
      enumerable: true,
      get: function () {
        var self = this;
        var sp = new URLSearchParams(this._search);
        var toStr = sp.toString;
        function sync() {
          self._search = String(toStr.call(sp));
        }
        // Mutating method calls write back to the URL immediately, so u.toString() reflects u.searchParams.set(...)
        ["append", "delete", "set", "sort"].forEach(function (name) {
          var orig = sp[name];
          sp[name] = function () {
            var r = orig.apply(sp, arguments);
            sync();
            return r;
          };
        });
        return sp;
      },
      set: function (v) {
        this._search =
          v instanceof URLSearchParams ? v.toString() : String(v).replace(/^\?/, "");
      },
    },
  });
  URL.prototype.toString = function () {
    return this.href;
  };
  URL.prototype.toJSON = function () {
    return this.href;
  };

  // ════════════════════════════════════════════════════════════════
  //  Global and require module exports
  // ════════════════════════════════════════════════════════════════
  globalThis.URLSearchParams = URLSearchParams;
  globalThis.URL = URL;

  globalThis.__orbitUrlModule = {
    URL: URL,
    URLSearchParams: URLSearchParams,
    parse: function (input, parseQueryString) {
      var u = new URL(String(input));
      var q = u._search;
      var out = {
        protocol: u.protocol || null,
        host: u.host || null,
        hostname: u.hostname || null,
        port: u.port || null,
        pathname: u.pathname || null,
        search: u.search || null,
        query: q === "" ? null : q,
        hash: u.hash || null,
        href: u.href,
        auth: u.username ? (u.password ? u.username + ":" + u.password : u.username) : null,
        path: (u.pathname || "") + u.search,
      };
      // Node semantics: parse(str, true) returns query as a plain object (duplicate keys aggregated into an array)
      if (parseQueryString) out.query = globalThis.__orbitQuerystringModule.parse(q);
      return out;
    },
    format: function (obj) {
      if (obj === undefined || obj === null) return "";
      if (typeof obj === "string") return obj;
      if (obj instanceof URL) return obj.href;
      var proto = obj.protocol || "";
      var auth = obj.auth || "";
      if (auth && auth.charAt(auth.length - 1) !== "@") auth += "@";
      var host = obj.host || (obj.hostname || "") + (obj.port ? ":" + obj.port : "");
      var path = obj.pathname || "";
      var search = obj.search;
      if (search === undefined || search === null) {
        if (obj.query !== undefined && obj.query !== null) {
          search =
            "?" +
            (typeof obj.query === "string"
              ? obj.query
              : new URLSearchParams(obj.query).toString());
        }
      }
      return proto + (host ? "//" : "") + auth + host + path + (search || "") + (obj.hash || "");
    },
    resolve: function (from, to) {
      return resolveRelative(String(from), String(to));
    },
  };

  globalThis.__orbitQuerystringModule = {
    parse: function (str, sep, eq) {
      sep = sep || "&";
      eq = eq || "=";
      var out = {};
      var s = String(str === undefined || str === null ? "" : str);
      if (s.charAt(0) === "?") s = s.slice(1);
      if (!s) return out;
      var parts = s.split(sep);
      for (var i = 0; i < parts.length; i++) {
        if (parts[i] === "") continue;
        var idx = parts[i].indexOf(eq);
        var k = idx < 0 ? decodePart(parts[i]) : decodePart(parts[i].slice(0, idx));
        var v = idx < 0 ? "" : decodePart(parts[i].slice(idx + eq.length));
        if (Object.prototype.hasOwnProperty.call(out, k)) {
          if (Object.prototype.toString.call(out[k]) === "[object Array]") out[k].push(v);
          else out[k] = [out[k], v];
        } else {
          out[k] = v;
        }
      }
      return out;
    },
    stringify: function (obj, sep, eq) {
      sep = sep || "&";
      eq = eq || "=";
      var out = [];
      for (var k in obj) {
        if (!Object.prototype.hasOwnProperty.call(obj, k)) continue;
        var v = obj[k];
        if (Object.prototype.toString.call(v) === "[object Array]") {
          for (var i = 0; i < v.length; i++) out.push(encodePart(k) + eq + encodePart(v[i]));
        } else if (v !== undefined) {
          out.push(encodePart(k) + eq + encodePart(v === null ? "" : v));
        }
      }
      return out.join(sep);
    },
    escape: encodePart,
    unescape: decodePart,
  };

  // ════════════════════════════════════════════════════════════════
  //  pm.request query convenience methods
  //  Attached to the pm.request object newly created by Rust on each run (see lib.rs run_pre_request).
  // ════════════════════════════════════════════════════════════════
  globalThis.__orbitDecorateRequest = function (req) {
    function splitUrl(url) {
      var s = String(url === undefined || url === null ? "" : url);
      var hash = "";
      var hi = s.indexOf("#");
      if (hi >= 0) {
        hash = s.slice(hi);
        s = s.slice(0, hi);
      }
      var qi = s.indexOf("?");
      var base = qi >= 0 ? s.slice(0, qi) : s;
      var query = qi >= 0 ? s.slice(qi + 1) : "";
      return { base: base, query: query, hash: hash };
    }
    function params() {
      return new URLSearchParams(splitUrl(req.url).query);
    }
    function apply(mutate) {
      var p = splitUrl(req.url);
      var sp = new URLSearchParams(p.query);
      mutate(sp);
      var qs = sp.toString();
      req.url = p.base + (qs ? "?" + qs : "") + p.hash;
      return req.url;
    }

    // Read: returns null when absent (consistent with URLSearchParams.get)
    req.getQueryParam = function (name) {
      return params().get(name);
    };
    req.getQueryParams = function (name) {
      return params().getAll(name);
    };
    req.hasQueryParam = function (name) {
      return params().has(name);
    };
    // Write: set overwrites same-name (keeps the first, removes the rest); add appends; remove deletes all same-name
    req.setQueryParam = function (name, value) {
      return apply(function (sp) {
        sp.set(name, value);
      });
    };
    req.addQueryParam = function (name, value) {
      return apply(function (sp) {
        sp.append(name, value);
      });
    };
    req.removeQueryParam = function (name) {
      return apply(function (sp) {
        sp["delete"](name);
      });
    };

    req.query = {
      get: req.getQueryParam,
      getAll: req.getQueryParams,
      has: req.hasQueryParam,
      set: req.setQueryParam,
      add: req.addQueryParam,
      remove: req.removeQueryParam,
      delete: req.removeQueryParam,
      keys: function () {
        return params().keys();
      },
      toObject: function () {
        var o = {};
        params().forEach(function (v, k) {
          if (!Object.prototype.hasOwnProperty.call(o, k)) o[k] = v;
        });
        return o;
      },
      toString: function () {
        return params().toString();
      },
    };
    return req;
  };
})();
