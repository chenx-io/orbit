//! # orbit-js
//!
//! QuickJS runtime - a lightweight JS sandbox for pre-request/post-response scripts.
//!
//! ## Global objects
//!
//! - `pm` - Postman-compatible scripting API (recommended)
//!   - `pm.request`   readable/modifiable in pre-request scripts: `url` / `method` / `headers` (`get`/`set`/`upsert`/`add`/`remove` or `headers['X']='v'`) / `body` (`raw` / `mode`)
//!     - query convenience methods (rewrite `pm.request.url` in place, automatically preserving the hash):
//!       `getQueryParam(name)` (returns `null` if absent) / `getQueryParams(name)` / `hasQueryParam(name)` /
//!       `setQueryParam(name, value)` (overwrites same-name) / `addQueryParam(name, value)` (appends) / `removeQueryParam(name)` (removes all same-name);
//!       equivalent object form: `pm.request.query.get/set/add/remove/has/toObject/toString`
//!   - `pm.response`  read-only in post-response scripts: `code` / `status` / `responseTime` / `headers` / `body` / `text()` / `json()`
//!   - `pm.environment`: `get(name)` / `set(name, value)`; `set` merges back into the currently active environment (persisted)
//!   - `pm.variables`: `get(name)` / `set(name, value)`; request-scoped **temporary variables**, independent of the environment and not persisted; read priority is temporary variables -> environment variables
//!
//! ## Variable value type semantics (consistent with Postman)
//!
//! Environment variables are stored as **strings**: `set(name, number|bool|object|...)` converts to a string before writing
//! (number/bool via `String(value)`, null/undefined stored as an empty string, objects/arrays via `JSON.stringify`).
//! Therefore `get(name)` always returns a string:
//! - No effect when referenced via the `{{name}}` template in URL / Header / Body (concatenation is string-based anyway);
//! - If a script needs the original type (e.g. numeric arithmetic), convert it yourself:
//!   `const n = Number(pm.environment.get('userId'))`。
//!   - `pm.secret`: `get(name)` reads secrets (api_key, etc.) read-only; secrets cannot be written by scripts
//!   - `pm.crypto`：`md5` / `sha1` / `sha224` / `sha256` / `sha384` / `sha512` / `sha3(data, bits)` / `ripemd160` /
//!     `hmac(algo, key, msg)` / `hmacBase64(...)` / `base64Encode` / `base64Decode` / `aesEncrypt({data,key,iv,mode,outputType})` /
//!     `aesDecrypt(...)` / `getRandomValues(n)`; all backed by Rust implementations
//!   - `pm.test(name, fn)` + `pm.expect(actual)` assertion collection:
//!     `to.equal` / `to.eql` / `to.contain` (`to.include` synonym) / `to.be.true|false|null|undefined|ok` /
//!     `to.be.a|an(type)` (`'array'` / `'null'` detectable) / `to.have.property`;
//!     negation via `.to.not.xxx(...)` or `.not.to.xxx(...)` (Postman supports both forms)
//!   - `pm.globals` / `pm.collectionVariables` - Postman aliases (mapped to environment); `pm.iterationData` / `pm.info` compatibility stubs
//! - `CryptoJS` - **official crypto-js 4.2.0** (complete: MD5/SHA1/SHA2xx/SHA3/RIPEMD160/HMAC/AES/DES/TripleDES/Rabbit/RC4 + enc.* + mode.* + pad.*)
//! - `require(name)` - Postman sandbox compatible: built-in `crypto-js` / `lodash` / `moment` / `uuid` / `atob` / `btoa`,
//!   scripts run unmodified; non-built-in modules throw an error
//! - `URL` / `URLSearchParams` - the same APIs as the browser (not native to QuickJS, implemented as a pure-JS compatibility layer),
//!   convenient for handling query parameters; `require('url')` and `require('querystring')` provide Node-style modules
//!   （`url.parse` / `url.format` / `url.resolve` / `URL` / `URLSearchParams`，`querystring.parse` / `stringify`）
//! - `btoa` / `atob` - global functions (Latin-1 semantics, consistent with the browser/Postman), Rust implementation
//! - `console` — `{ log(...), error(...), warn(...) }`
//! - `env` - read-only environment variable access (legacy, compatible with older scripts)
//! - `request` / `response` - legacy flat objects (compatible with older scripts and existing tests)

mod bridge;
mod cipher;
mod crypto;
mod types;

pub use types::{EnvVars, RequestContext, ResponseContext, ScriptLog, ScriptOutcome, TestResult};

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use rquickjs::{Context, Ctx, Object, Runtime};

/// Script bootstrap: defines __makeHeaderProxy, pm.test / pm.expect, the complete
/// crypto-js compatibility facade (WordArray / enc.* / hash / HMAC / symmetric encryption),
/// the Postman-semantics wrapper for pm.crypto, and variable set type conversion.
/// Must run after the `pm` global, `__orbit_crypto`, and `__orbit_env_set`/`__orbit_temp_set` are ready.
const BOOTSTRAP: &str = include_str!("bootstrap.js");

/// Built-in URL / URLSearchParams compatibility layer + `pm.request` query convenience methods.
///
/// QuickJS only implements ECMAScript built-ins and lacks the browser's `URL` / `URLSearchParams`;
/// this file fills the gap in pure JS and exposes `require('url')` / `require('querystring')`.
/// Injected once before bootstrap and re-injected on every run reset (to prevent script overwrites).
const URL_SHIM: &str = include_str!("urlshim.js");

/// Records a **reference snapshot** of the `pm` facade (members installed by bootstrap) for restoration before each run.
///
/// `pm` itself is in `pristine_keys`, so [`reset_globals`] won't touch it; if a script overwrites / adds
/// `pm` members (`pm.expect = ...`, `pm.helper = ...`), they leak across requests and can only be restored from this snapshot.
const SNAPSHOT_PM: &str = "globalThis.__orbitPmPristine = (function(){ var out = {}; var pm = globalThis.pm; \
     if (pm) { Object.getOwnPropertyNames(pm).forEach(function(k){ try { out[k] = pm[k]; } catch(e){} }); } \
     return out; })();";

/// Restore the `pm` facade before each run: delete members added by scripts and restore overwritten ones (same approach as console / btoa /
/// crypto bridge restoration). Cross-run pollution from top-level `const` / `let` / `class` is handled by
/// [`wrap_user_script`]'s function-scope isolation; this only handles the "script actively rewrites pm members" case.
const RESTORE_PM: &str = "(function(){ var p = globalThis.__orbitPmPristine, pm = globalThis.pm; \
     if (!p || !pm) { return; } \
     Object.getOwnPropertyNames(pm).forEach(function(k){ \
       if (!Object.prototype.hasOwnProperty.call(p, k)) { try { delete pm[k]; } catch(e){} } }); \
     Object.getOwnPropertyNames(p).forEach(function(k){ try { pm[k] = p[k]; } catch(e){} }); })();";

pub struct JsSandbox {
    /// Persistent execution context: bootstrap runs only once, and the sandbox state is reset before each run
    /// (Context holds a reference to the Runtime internally, so the Runtime need not be stored separately)
    ctx: Context,
    /// Snapshot of global keys after bootstrap (baseline for cleaning up user-script pollution)
    pristine_keys: Vec<String>,
}

impl JsSandbox {
    /// Create the sandbox: initialize the QuickJS Runtime + Context and run bootstrap once (built-in libs/facade).
    ///
    /// The sandbox is reused across multiple script runs (`run_pre_request` / `run_post_response`); before each run it
    /// automatically resets global state (deletes globals added by user scripts, rebuilds `pm.environment`/`pm.variables`, etc.),
    /// keeping scripts isolated from each other.
    pub fn new() -> Result<Self, String> {
        let rt = Runtime::new().map_err(|e| format!("QuickJS: {}", e))?;
        let ctx = Context::full(&rt).map_err(|e| format!("QuickJS context: {}", e))?;
        let mut sandbox = Self {
            ctx,
            pristine_keys: Vec::new(),
        };
        sandbox.pristine_keys = sandbox.init()?;
        Ok(sandbox)
    }

    /// One-time initialization: inject static globals (btoa/atob/crypto bridge/require lib sources) -> pm skeleton -> run bootstrap -> snapshot global keys.
    fn init(&self) -> Result<Vec<String>, String> {
        self.ctx.with(|ctx| {
            bridge::inject_sandbox_globals(&ctx);
            // pm skeleton: bootstrap attaches pm.test / pm.expect / pm.crypto facade / pm.utf8, etc.
            let pm = Object::new(ctx.clone()).unwrap();
            let crypto = bridge::build_crypto(&ctx);
            pm.set("crypto", crypto).ok();
            ctx.globals().set("pm", pm).ok();
            // URL / URLSearchParams / require('url') compatibility layer (bootstrap's require references it)
            ctx.eval::<(), _>(URL_SHIM)
                .map_err(|e| bridge::js_exception_info(&ctx, &e).1)?;
            ctx.eval::<(), _>(BOOTSTRAP)
                .map_err(|e| bridge::js_exception_info(&ctx, &e).1)?;
            // pm facade snapshot (see `SNAPSHOT_PM`)
            ctx.eval::<(), _>(SNAPSHOT_PM)
                .map_err(|e| bridge::js_exception_info(&ctx, &e).1)?;
            // Snapshot pristine global keys (including non-enumerable standard built-ins, matching Object.getOwnPropertyNames semantics)
            let keys: Vec<String> = ctx
                .globals()
                .own_keys::<rquickjs::String>(rquickjs::Filter::new().string())
                .filter_map(|k| k.ok().and_then(|s| s.to_string().ok()))
                .collect();
            Ok(keys)
        })
    }

    /// Pre-request script: runs on the "variables resolved" request and may rewrite url/method/headers/body.
    /// Returns the execution result (including logs and variables written by the script) and writes changes back via `request`.
    ///
    /// `temp_vars`: optional seed temporary variables (e.g. from the pre-request script of the same request), used as the initial values for `pm.variables`.
    pub fn run_pre_request(
        &self,
        script: &str,
        request: &mut RequestContext,
        env_vars: Option<&EnvVars>,
        temp_vars: Option<&HashMap<String, String>>,
    ) -> ScriptOutcome {
        self.run_in_sandbox(env_vars, temp_vars, |ctx, pm, logs, outcome| {
            // request (rich pm API)
            let req_obj = Object::new(ctx.clone()).unwrap();
            req_obj.set("url", request.url.clone()).ok();
            req_obj.set("method", request.method.clone()).ok();
            req_obj.set("raw", request.raw.clone()).ok();
            let headers_backing = Object::new(ctx.clone()).unwrap();
            for (k, v) in &request.headers {
                headers_backing.set(k.clone(), v.clone()).ok();
            }
            let mk = ctx
                .globals()
                .get::<_, rquickjs::Function<'_>>("__makeHeaderProxy")
                .ok();
            if let Some(f) = mk {
                if let Ok(hp) = f.call::<_, rquickjs::Object<'_>>((headers_backing.clone(), false))
                {
                    req_obj.set("headers", hp).ok();
                }
            }
            let body_obj = Object::new(ctx.clone()).unwrap();
            body_obj.set("raw", request.body.clone()).ok();
            body_obj.set("mode", "").ok();
            req_obj.set("body", body_obj.clone()).ok();
            pm.set("request", req_obj.clone()).ok();

            // pm.request query convenience methods (getQueryParam / setQueryParam / addQueryParam /
            // removeQueryParam and req.query.*), provided by the URL compatibility layer
            if let Ok(decorate) = ctx
                .globals()
                .get::<_, rquickjs::Function<'_>>("__orbitDecorateRequest")
            {
                decorate.call::<_, ()>((req_obj.clone(),)).ok();
            }

            // request (legacy flat, compatible with older scripts and existing tests)
            let legacy = Object::new(ctx.clone()).unwrap();
            legacy.set("url", request.url.clone()).ok();
            legacy.set("method", request.method.clone()).ok();
            legacy.set("body", request.body.clone()).ok();
            let lh = Object::new(ctx.clone()).unwrap();
            for (k, v) in &request.headers {
                lh.set(k.clone(), v.clone()).ok();
            }
            legacy.set("headers", lh).ok();
            ctx.globals().set("request", legacy).ok();

            // ── Run the user script ──
            eval_user_script(ctx, script, logs, outcome);

            // ── Write back the request ──
            if let Ok(u) = req_obj.get::<_, String>("url") {
                request.url = u;
            }
            if let Ok(m) = req_obj.get::<_, String>("method") {
                request.method = m;
            }
            if let Ok(b) = body_obj.get::<_, String>("raw") {
                request.body = b;
            }
            if let Ok(r) = req_obj.get::<_, String>("raw") {
                request.raw = r;
            }
            let mut nh = HashMap::new();
            for k in headers_backing.keys::<rquickjs::String>().flatten() {
                let ks = k.to_string().unwrap_or_default();
                if let Ok(v) = headers_backing.get::<_, rquickjs::String>(ks.as_str()) {
                    if let Ok(vs) = v.to_string() {
                        nh.insert(ks, vs);
                    }
                }
            }
            request.headers = nh;
        })
    }

    /// Post-response script: runs after receiving the response; can read the response, write variables, and collect assertions.
    ///
    /// `temp_vars`: optional seed temporary variables (e.g. from the pre-request script of the same request), used as the initial values for `pm.variables`.
    pub fn run_post_response(
        &self,
        script: &str,
        response: &ResponseContext,
        env_vars: Option<&EnvVars>,
        temp_vars: Option<&HashMap<String, String>>,
    ) -> ScriptOutcome {
        self.run_in_sandbox(env_vars, temp_vars, |ctx, pm, logs, outcome| {
            // response (rich pm API, read-only)
            let resp_obj = Object::new(ctx.clone()).unwrap();
            resp_obj.set("code", response.status as i64).ok();
            resp_obj.set("status", response.status as i64).ok();
            resp_obj.set("responseTime", response.duration_ms as i64).ok();
            resp_obj.set("raw", response.raw.clone()).ok();
            resp_obj
                .set(
                    "decoded",
                    response
                        .decoded
                        .clone()
                        .unwrap_or_else(|| response.body.clone()),
                )
                .ok();
            let hb = Object::new(ctx.clone()).unwrap();
            for (k, v) in &response.headers {
                hb.set(k.clone(), v.clone()).ok();
            }
            let mk = ctx
                .globals()
                .get::<_, rquickjs::Function<'_>>("__makeHeaderProxy")
                .ok();
            if let Some(f) = mk {
                if let Ok(hp) = f.call::<_, rquickjs::Object<'_>>((hb.clone(), true)) {
                    resp_obj.set("headers", hp).ok();
                }
            }
            resp_obj.set("body", response.body.clone()).ok();
            pm.set("response", resp_obj.clone()).ok();

            // response (legacy flat, compatible with existing tests)
            let legacy = Object::new(ctx.clone()).unwrap();
            legacy.set("status", response.status as i64).ok();
            legacy.set("body", response.body.clone()).ok();
            legacy.set("duration_ms", response.duration_ms as i64).ok();
            legacy.set("raw", response.raw.clone()).ok();
            let lh = Object::new(ctx.clone()).unwrap();
            for (k, v) in &response.headers {
                lh.set(k.clone(), v.clone()).ok();
            }
            legacy.set("headers", lh).ok();
            let escaped_body = response
                .body
                .replace('\\', "\\\\")
                .replace('\'', "\\'")
                .replace('\n', "\\n")
                .replace('\r', "\\r");
            let json_fn = format!(
                "(function(){{ try {{ return JSON.parse('{}'); }} catch(e) {{ return null; }} }})",
                escaped_body
            );
            if let Ok(fn_val) = ctx.eval::<rquickjs::Value<'_>, _>(json_fn) {
                legacy.set("json", fn_val).ok();
            }
            ctx.globals().set("response", legacy).ok();

            // The post-response script may also dynamically inject text()/json() into pm.response
            let resp_helpers = "pm.response.text = function(){ return pm.response.body; }; \
                 pm.response.json = function(){ try { return JSON.parse(pm.response.body); } catch(e) { return null; } };"
                .to_string();
            ctx.eval::<(), _>(resp_helpers).ok();

            // ── Run the user script ──
            eval_user_script(ctx, script, logs, outcome);

            // Read the script decode result (pm.response.decoded)
            if let Ok(d) = resp_obj.get::<_, String>("decoded") {
                outcome.decoded = Some(d);
            }

            // Read the post-response assertions
            if let Ok(tests_arr) = pm.get::<_, Object<'_>>("_tests") {
                if let Ok(len) = tests_arr.get::<_, i32>("length") {
                    for i in 0..len {
                        if let Ok(t) = tests_arr.get::<i32, Object<'_>>(i) {
                            let name = t.get::<_, String>("name").unwrap_or_default();
                            let passed = t.get::<_, bool>("passed").unwrap_or(false);
                            let message = t.get::<_, String>("message").unwrap_or_default();
                            outcome.tests.push(TestResult {
                                name,
                                passed,
                                message,
                            });
                        }
                    }
                }
            }
        })
    }

    /// Shared execution framework: reset sandbox state -> call `body` (inject business objects, run the script, read back) -> collect results.
    ///
    /// `body` receives `(ctx, pm, logs, outcome)` and calls between "inject -> execute -> read back"
    /// [`eval_user_script`] to run the user script (unified error handling), keeping the timing identical to the single-function version.
    fn run_in_sandbox<F>(
        &self,
        env_vars: Option<&EnvVars>,
        temp_vars: Option<&HashMap<String, String>>,
        body: F,
    ) -> ScriptOutcome
    where
        F: for<'a> FnOnce(&Ctx<'a>, &Object<'a>, &Rc<RefCell<Vec<ScriptLog>>>, &mut ScriptOutcome),
    {
        let mut outcome = ScriptOutcome::default();
        let logs = Rc::new(RefCell::new(Vec::<ScriptLog>::new()));
        let vars_set = Rc::new(RefCell::new(HashMap::<String, String>::new()));
        let temp_vars_set = Rc::new(RefCell::new(HashMap::<String, String>::new()));

        self.ctx.with(|ctx| {
            // ── Sandbox reset (keep each run clean on the persistent Context) ──
            // 1. Delete globals created by user scripts in the previous round (top-level var / globalThis.x), keeping bootstrap keys
            reset_globals(&ctx, &self.pristine_keys);
            // 2. Restore the console wrapper (to prevent script overwrite/tampering) and bind this run's log buffer
            let _ = ctx.eval::<(), _>(
                "if (globalThis.__orbitConsoleMethods) { var m = globalThis.__orbitConsoleMethods; \
                 globalThis.console = { log: m.log, error: m.error, warn: m.warn }; }",
            );
            bridge::register_console(&ctx, &logs);
            // 3. Restore Rust static globals (btoa/atob wrappers + crypto bridge/require lib sources)
            bridge::restore_static_globals(&ctx);
            // 3b. Re-inject the URL compatibility layer (scripts may overwrite URL/URLSearchParams; restore each run)
            let _ = ctx.eval::<(), _>(URL_SHIM);
            // 3c. Restore the pm facade: delete members added by the script last round and restore overwritten ones
            let _ = ctx.eval::<(), _>(RESTORE_PM);
            // 4. Rebuild this run's pm.environment / pm.variables / pm.secret and the legacy env global
            let pm = fetch_pm(&ctx);
            let env_obj = bridge::build_env(&ctx, env_vars, &vars_set);
            let vars_obj = bridge::build_temp(&ctx, env_vars, temp_vars, &temp_vars_set);
            let secret_obj = bridge::build_secret(&ctx, env_vars);
            pm.set("environment", env_obj.clone()).ok();
            pm.set("variables", vars_obj).ok();
            pm.set("secret", secret_obj).ok();
            // Postman aliases (globals / collectionVariables point to this run's environment object) and assertion collection reset
            pm.set("globals", env_obj.clone()).ok();
            pm.set("collectionVariables", env_obj).ok();
            pm.set("_tests", rquickjs::Array::new(ctx.clone()).unwrap())
                .ok();
            ctx.globals().set("pm", pm.clone()).ok();
            // env (legacy read-only global; an empty object when there are no env vars, to avoid leftovers from the previous round)
            let legacy_env = Object::new(ctx.clone()).unwrap();
            if let Some(vars) = env_vars {
                for (k, v) in vars {
                    legacy_env.set(k.as_str(), v.as_str()).ok();
                }
            }
            ctx.globals().set("env", legacy_env).ok();

            body(&ctx, &pm, &logs, &mut outcome);
        });

        outcome.logs = bridge::take(logs);
        outcome.vars_set = bridge::take(vars_set);
        outcome.temp_vars_set = bridge::take(temp_vars_set);
        outcome
    }
}

impl Default for JsSandbox {
    fn default() -> Self {
        Self::new().expect("failed to create QuickJS runtime")
    }
}

/// Process-wide shared sandbox: lazily created (QuickJS + bootstrap run once) and reused across requests.
///
/// Background: if a `PipelineRuntime` containing scripts were created per request, the sandbox would be initialized on the spot
/// (bootstrap includes large libs like crypto-js; in a debug build the first request's TTFB can reach several seconds).
/// Once shared, it is created only once (first use or prewarm), and scripts are isolated by the sandbox reset in
/// `run_pre_request` / `run_post_response`; concurrent script execution is serialized via a Mutex (scripts are short synchronous sections, which is acceptable).
pub fn with_sandbox<T>(f: impl FnOnce(Option<&JsSandbox>) -> T) -> T {
    use std::sync::{Mutex, OnceLock};
    static SANDBOX: OnceLock<Mutex<Option<JsSandbox>>> = OnceLock::new();
    let mut guard = SANDBOX
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if guard.is_none() {
        *guard = JsSandbox::new().ok();
    }
    f(guard.as_ref())
}

/// Prewarm the shared sandbox in the background (called at app startup to move expensive initialization off the request hot path).
/// A prewarm failure is only logged (creation is retried on first real use).
pub fn prewarm_sandbox() {
    std::thread::spawn(|| {
        let t0 = std::time::Instant::now();
        with_sandbox(|_| {});
        tracing::info!(
            "[orbit-js] sandbox prewarm done in {} ms",
            t0.elapsed().as_millis()
        );
    });
}

/// Get the persistent pm object; if replaced/broken by a script, rebuild an empty skeleton (the bootstrap facade is lost - an unrecoverable boundary).
fn fetch_pm<'js>(ctx: &Ctx<'js>) -> Object<'js> {
    match ctx.globals().get::<_, Object<'_>>("pm") {
        Ok(pm) => pm,
        Err(_) => {
            let pm = Object::new(ctx.clone()).unwrap();
            ctx.globals().set("pm", pm.clone()).ok();
            pm
        }
    }
}

/// Delete global keys outside the bootstrap snapshot (cleans up the previous round's user-script top-level `var` / function declarations / `globalThis.x` assignments).
/// In QuickJS the global property declared by a top-level `var` is non-configurable and `delete` fails silently,
/// so set the value to `undefined` to block data leakage (`typeof x` matches a fresh sandbox).
///
/// **Note**: only **global object properties** are visible here. Top-level `const` / `let` / `class` create global **lexical**
/// bindings (not on `globalThis`, cannot be deleted), so they must be isolated by [`wrap_user_script`]'s function scope.
fn reset_globals<'js>(ctx: &Ctx<'js>, pristine: &[String]) {
    let known = serde_json::to_string(pristine).unwrap_or_else(|_| "[]".into());
    let snippet = format!(
        "(function(){{ var known = {known}; Object.getOwnPropertyNames(globalThis).forEach(function(k){{ if (known.indexOf(k) < 0) {{ var d = false; try {{ d = delete globalThis[k]; }} catch(e){{}} if (!d) {{ try {{ globalThis[k] = undefined; }} catch(e){{}} }} }} }}); }})()"
    );
    let _ = ctx.eval::<(), _>(snippet);
}

/// Wrap the user script in a **function scope** before handing it to QuickJS.
///
/// Why the wrapping is required: under a persistent Context, top-level `const` / `let` / `class` create **global lexical bindings**,
/// which neither appear on `globalThis` ([`reset_globals`] can't find them) nor can be `delete`d,
/// so running the same script a second time makes QuickJS report `redeclaration of 'res'` (observed by users).
/// Once wrapped in a function these declarations disappear when the call ends, and `var` / `function` no longer land on the global object.
///
/// The wrapper deliberately **does not change line numbers**: the prefix stays on the script's first line and the suffix goes on its own line,
/// so line numbers in syntax errors are still what the user sees; the trailing newline also catches the case where "the script ends with a `//` line comment".
fn wrap_user_script(script: &str) -> String {
    format!("(function(){{{}\n}})()", script)
}

/// Run the user script and record errors to outcome / logs (shared by `run_pre_request` / `run_post_response`).
fn eval_user_script<'js>(
    ctx: &Ctx<'js>,
    script: &str,
    logs: &Rc<RefCell<Vec<ScriptLog>>>,
    outcome: &mut ScriptOutcome,
) {
    if let Err(e) = ctx.eval::<(), _>(wrap_user_script(script)) {
        let (msg, full) = bridge::js_exception_info(ctx, &e);
        logs.borrow_mut().push(ScriptLog {
            level: "error".into(),
            message: format!("script execution error: {}", full),
        });
        outcome.error = Some(msg);
    } else {
        outcome.success = true;
    }
}

// ── Hash / HMAC (delegates to the crypto module; pub signatures kept for existing tests and external use) ──

pub fn crypto_md5(s: &str) -> String {
    crypto::hex_encode(&crypto::md5_bytes(s.as_bytes()))
}

pub fn crypto_sha1(s: &str) -> String {
    crypto::hex_encode(&crypto::sha1_bytes(s.as_bytes()))
}

pub fn crypto_sha256(s: &str) -> String {
    crypto::hex_encode(&crypto::sha256_bytes(s.as_bytes()))
}

pub fn crypto_sha512(s: &str) -> String {
    crypto::hex_encode(&crypto::sha512_bytes(s.as_bytes()))
}

pub fn crypto_hmac(algo: &str, key: &str, msg: &str) -> String {
    crypto::hex_encode(
        &crypto::hmac_bytes(algo, key.as_bytes(), msg.as_bytes()).unwrap_or_default(),
    )
}

/// Base64 of an HMAC digest (fixes the wrong semantics of the old implementation, which double-encoded the hex text).
pub fn crypto_hmac_base64(algo: &str, key: &str, msg: &str) -> String {
    crypto::b64_encode(
        &crypto::hmac_bytes(algo, key.as_bytes(), msg.as_bytes()).unwrap_or_default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Compile-time assertion: the sandbox can be shared across threads (process-wide persistent reuse requires Send + Sync)
    #[test]
    fn sandbox_is_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}
        assert_send::<JsSandbox>();
        assert_sync::<JsSandbox>();
    }

    #[test]
    fn test_create() {
        assert!(JsSandbox::new().is_ok());
    }

    #[test]
    fn test_pre_url() {
        let s = JsSandbox::new().unwrap();
        let mut r = RequestContext {
            url: "https://a.com/u".into(),
            method: "GET".into(),
            headers: HashMap::new(),
            body: String::new(),
            raw: String::new(),
        };
        let o = s.run_pre_request("pm.request.url += '/v2'", &mut r, None, None);
        assert!(o.success, "{:?}", o.error);
        assert_eq!(r.url, "https://a.com/u/v2");
    }

    #[test]
    fn test_pre_headers_upsert() {
        let s = JsSandbox::new().unwrap();
        let mut r = RequestContext {
            url: "https://a.com".into(),
            method: "POST".into(),
            headers: HashMap::from([("CT".into(), "json".into())]),
            body: String::new(),
            raw: String::new(),
        };
        let o = s.run_pre_request(
            "pm.request.headers.upsert({ key: 'X-Signature', value: 'abc' }); pm.request.headers['X']='Y';",
            &mut r,
            None,
            None,
        );
        assert!(o.success, "{:?}", o.error);
        assert_eq!(r.headers.get("X-Signature").unwrap(), "abc");
        assert_eq!(r.headers.get("X").unwrap(), "Y");
    }

    #[test]
    fn test_pre_body_raw() {
        let s = JsSandbox::new().unwrap();
        let mut r = RequestContext {
            url: "https://a.com".into(),
            method: "POST".into(),
            headers: HashMap::new(),
            body: String::new(),
            raw: String::new(),
        };
        let o = s.run_pre_request(
            "pm.request.body.raw = JSON.stringify({a:1})",
            &mut r,
            None,
            None,
        );
        assert!(o.success, "{:?}", o.error);
        assert!(r.body.contains("\"a\""));
    }

    #[test]
    fn test_pre_env_get() {
        let s = JsSandbox::new().unwrap();
        let mut r = RequestContext {
            url: "https://a.com".into(),
            method: "GET".into(),
            headers: HashMap::new(),
            body: String::new(),
            raw: String::new(),
        };
        let env = HashMap::from([("T".into(), "s123".into())]);
        let o = s.run_pre_request(
            "pm.request.headers.upsert({ key: 'A', value: 'Bearer ' + pm.environment.get('T') })",
            &mut r,
            Some(&env),
            None,
        );
        assert!(o.success, "{:?}", o.error);
        assert_eq!(r.headers.get("A").unwrap(), "Bearer s123");
    }

    #[test]
    fn test_pre_signature_hmac() {
        // Reproduce a user scenario: generate HMAC-SHA256 from body + key and write it to a header
        let s = JsSandbox::new().unwrap();
        let mut r = RequestContext {
            url: "https://a.com/sign".into(),
            method: "POST".into(),
            headers: HashMap::new(),
            body: "{\"amount\":100}".into(),
            raw: String::new(),
        };
        let env = HashMap::from([("SECRET".into(), "mykey".into())]);
        let script = r#"
            var body = pm.request.body.raw;
            var secret = pm.environment.get('SECRET');
            var sign = pm.crypto.hmac('sha256', secret, body + '|' + 'k1');
            pm.request.headers.upsert({ key: 'X-Signature', value: sign });
        "#;
        let o = s.run_pre_request(script, &mut r, Some(&env), None);
        assert!(o.success, "{:?}", o.error);
        let sig = r.headers.get("X-Signature").unwrap();
        // Compare with a direct Rust computation
        assert_eq!(sig, &crypto_hmac("sha256", "mykey", "{\"amount\":100}|k1"));
        assert_eq!(sig.len(), 64);
    }

    #[test]
    fn test_pre_secret_get() {
        let s = JsSandbox::new().unwrap();
        let mut r = RequestContext {
            url: "https://a.com".into(),
            method: "POST".into(),
            headers: HashMap::new(),
            body: "data".into(),
            raw: String::new(),
        };
        // Secrets are passed in via the env_vars snapshot (the frontend merges secrets into vars)
        let env = HashMap::from([("API_KEY".into(), "s3cr3t".into())]);
        let script = r#"
            var key = pm.secret.get('API_KEY');
            pm.request.headers.upsert({ key: 'Authorization', value: 'Bearer ' + key });
        "#;
        let o = s.run_pre_request(script, &mut r, Some(&env), None);
        assert!(o.success, "{:?}", o.error);
        assert_eq!(
            r.headers.get("Authorization").unwrap(),
            &"Bearer s3cr3t".to_string()
        );
        // Secret is read-only: set must not affect the secret snapshot or appear in vars_set
        let script2 = "pm.secret.set && pm.secret.set('API_KEY', 'x');";
        let o2 = s.run_pre_request(script2, &mut r, Some(&env), None);
        assert!(o2.success);
        assert_eq!(o2.vars_set.get("API_KEY"), None);
    }

    #[test]
    fn test_pre_cryptojs_compat() {
        let s = JsSandbox::new().unwrap();
        let mut r = RequestContext {
            url: "https://a.com".into(),
            method: "POST".into(),
            headers: HashMap::new(),
            body: "data".into(),
            raw: String::new(),
        };
        let script = r#"
            var sign = CryptoJS.HmacSHA256(pm.request.body.raw, 'k').toString();
            pm.request.headers.upsert({ key: 'X', value: sign });
        "#;
        let o = s.run_pre_request(script, &mut r, None, None);
        assert!(o.success, "{:?}", o.error);
        assert_eq!(
            r.headers.get("X").unwrap(),
            &crypto_hmac("sha256", "k", "data")
        );
    }

    #[test]
    fn test_pre_error_logged() {
        let s = JsSandbox::new().unwrap();
        let mut r = RequestContext {
            url: "https://a.com".into(),
            method: "GET".into(),
            headers: HashMap::new(),
            body: String::new(),
            raw: String::new(),
        };
        let o = s.run_pre_request("this is not valid js >>>", &mut r, None, None);
        assert!(!o.success);
        assert!(o.error.is_some());
    }

    #[test]
    fn test_post_ok() {
        let s = JsSandbox::new().unwrap();
        let resp = ResponseContext {
            status: 200,
            body: "{}".into(),
            headers: HashMap::new(),
            duration_ms: 100,
            raw: String::new(),
            decoded: None,
        };
        let o = s.run_post_response(
            "if(pm.response.code!==200)throw new Error('x')",
            &resp,
            None,
            None,
        );
        assert!(o.success, "{:?}", o.error);
    }

    #[test]
    fn test_post_throw() {
        let s = JsSandbox::new().unwrap();
        let resp = ResponseContext {
            status: 500,
            body: "{}".into(),
            headers: HashMap::new(),
            duration_ms: 100,
            raw: String::new(),
            decoded: None,
        };
        let r = s.run_post_response(
            "if(pm.response.code!==200)throw new Error('fail '+pm.response.code)",
            &resp,
            None,
            None,
        );
        assert!(!r.success);
        assert!(r.error.is_some());
    }

    #[test]
    fn test_post_assertions() {
        let s = JsSandbox::new().unwrap();
        let resp = ResponseContext {
            status: 200,
            body: r#"{"code":0,"token":"abc123","arr":[1,2,3]}"#.into(),
            headers: HashMap::from([("X-Id".into(), "abc".into())]),
            duration_ms: 50,
            raw: String::new(),
            decoded: None,
        };
        let script = r#"
            pm.test('status 200', function(){ pm.expect(pm.response.code).to.equal(200); });
            pm.test('has token', function(){ pm.expect(pm.response.json().token).to.equal('abc123'); });
            pm.test('header present', function(){ pm.expect(pm.response.headers['X-Id']).to.equal('abc'); });
            pm.test('array len', function(){ pm.expect(pm.response.json().arr).to.have.property('2'); });
            pm.environment.set('token', pm.response.json().token);
        "#;
        let r = s.run_post_response(script, &resp, None, None);
        assert!(r.success, "{:?}", r.error);
        assert_eq!(r.tests.len(), 4);
        assert!(r.tests.iter().all(|t| t.passed), "{:?}", r.tests);
        assert_eq!(r.vars_set.get("token").unwrap(), "abc123");
    }

    #[test]
    fn test_env_set_non_string_values() {
        // Regression: pm.environment.set must support non-string values such as number/bool/objects (user scenario: take a number from the response JSON and store it in a variable)
        let s = JsSandbox::new().unwrap();
        let resp = ResponseContext {
            status: 200,
            body: r#"{"userId":12345,"ok":true,"profile":{"name":"x"},"none":null}"#.into(),
            headers: HashMap::new(),
            duration_ms: 10,
            raw: String::new(),
            decoded: None,
        };
        let script = r#"
            var uid = pm.response.json().userId;
            pm.environment.set('userId', uid);
            pm.environment.set('flag', pm.response.json().ok);
            pm.environment.set('profile', pm.response.json().profile);
            pm.environment.set('nothing', pm.response.json().none);
        "#;
        let r = s.run_post_response(script, &resp, None, None);
        assert!(r.success, "{:?}", r.error);
        assert_eq!(r.vars_set.get("userId").unwrap(), "12345");
        assert_eq!(r.vars_set.get("flag").unwrap(), "true");
        assert_eq!(r.vars_set.get("profile").unwrap(), "{\"name\":\"x\"}");
        assert_eq!(r.vars_set.get("nothing").unwrap(), "");
    }

    #[test]
    fn test_post_assertion_fail() {
        let s = JsSandbox::new().unwrap();
        let resp = ResponseContext {
            status: 200,
            body: "{\"code\":1}".into(),
            headers: HashMap::new(),
            duration_ms: 10,
            raw: String::new(),
            decoded: None,
        };
        let script = r#"
            pm.test('code zero', function(){ pm.expect(pm.response.json().code).to.equal(0); });
        "#;
        let r = s.run_post_response(script, &resp, None, None);
        assert!(r.success);
        assert_eq!(r.tests.len(), 1);
        assert!(!r.tests[0].passed);
    }

    #[test]
    fn test_console() {
        let s = JsSandbox::new().unwrap();
        let resp = ResponseContext {
            status: 200,
            body: "{}".into(),
            headers: HashMap::new(),
            duration_ms: 100,
            raw: String::new(),
            decoded: None,
        };
        let r = s.run_post_response(
            "console.log('hi'); console.warn('w'); console.error('e')",
            &resp,
            None,
            None,
        );
        assert!(r.success, "{:?}", r.error);
        assert_eq!(r.logs.len(), 3);
        assert_eq!(r.logs[0].level, "log");
        assert_eq!(r.logs[0].message, "hi");
    }

    #[test]
    fn test_console_object_pretty() {
        let s = JsSandbox::new().unwrap();
        let resp = ResponseContext {
            status: 200,
            body: "{}".into(),
            headers: HashMap::new(),
            duration_ms: 100,
            raw: String::new(),
            decoded: None,
        };
        let r = s.run_post_response(
            "console.log('payload', { a: 1, b: [1, 2, 3], c: { d: true } }); console.log('ok')",
            &resp,
            None,
            None,
        );
        assert!(r.success, "{:?}", r.error);
        assert_eq!(r.logs.len(), 2);
        // The object is pretty-printed as multi-line JSON
        assert!(
            r.logs[0].message.contains('\n'),
            "expected pretty multi-line: {:?}",
            r.logs[0].message
        );
        assert!(r.logs[0].message.contains("\"a\": 1"));
        assert!(r.logs[0].message.contains("\"d\": true"));
        // A plain string is not wrapped
        assert_eq!(r.logs[1].message, "ok");
    }

    #[test]
    fn test_variables_not_persisted_to_environment() {
        // pm.variables.set writes a temporary variable; it must not enter vars_set (environment) or pollute the env variable snapshot
        let s = JsSandbox::new().unwrap();
        let mut r = RequestContext {
            url: "https://a.com".into(),
            method: "GET".into(),
            headers: HashMap::new(),
            body: String::new(),
            raw: String::new(),
        };
        let env = HashMap::from([("EXIST".into(), "v".into())]);
        let o = s.run_pre_request(
            "pm.variables.set('tmp', 'abc'); pm.environment.set('env1', 'x'); var g = pm.variables.get('EXIST'); pm.request.headers.upsert({ key: 'G', value: g });",
            &mut r,
            Some(&env),
            None,
        );
        assert!(o.success, "{:?}", o.error);
        // Temporary variables only go into temp_vars_set
        assert_eq!(o.temp_vars_set.get("tmp").unwrap(), "abc");
        assert_eq!(o.vars_set.get("tmp"), None);
        // environment writes still go through vars_set
        assert_eq!(o.vars_set.get("env1").unwrap(), "x");
        // get falls back to the env variable snapshot
        assert_eq!(r.headers.get("G").unwrap(), "v");
    }

    #[test]
    fn test_variables_local_priority_over_env() {
        // A pm.variables temporary value takes priority over a same-named environment variable
        let s = JsSandbox::new().unwrap();
        let mut r = RequestContext {
            url: "https://a.com".into(),
            method: "GET".into(),
            headers: HashMap::new(),
            body: String::new(),
            raw: String::new(),
        };
        let env = HashMap::from([("K".into(), "fromEnv".into())]);
        let o = s.run_pre_request(
            "pm.variables.set('K', 'local'); pm.request.headers.upsert({ key: 'H', value: pm.variables.get('K') });",
            &mut r,
            Some(&env),
            None,
        );
        assert!(o.success, "{:?}", o.error);
        assert_eq!(r.headers.get("H").unwrap(), "local");
    }

    #[test]
    fn test_temp_vars_shared_pre_to_post() {
        // A temporary variable written by the pre-request script should be readable in the post-response script (passed via the temp_vars seed)
        let s = JsSandbox::new().unwrap();
        // Pre-request
        let mut r = RequestContext {
            url: "https://a.com".into(),
            method: "GET".into(),
            headers: HashMap::new(),
            body: String::new(),
            raw: String::new(),
        };
        let pre = s.run_pre_request("pm.variables.set('nonce', 'N123');", &mut r, None, None);
        assert!(pre.success, "{:?}", pre.error);
        assert_eq!(pre.temp_vars_set.get("nonce").unwrap(), "N123");
        // Post-response (seed = the temporary variables written by the pre-request)
        let resp = ResponseContext {
            status: 200,
            body: "{}".into(),
            headers: HashMap::new(),
            duration_ms: 10,
            raw: String::new(),
            decoded: None,
        };
        let post = s.run_post_response(
            "pm.test('nonce shared', function(){ pm.expect(pm.variables.get('nonce')).to.equal('N123'); });",
            &resp,
            None,
            Some(&pre.temp_vars_set),
        );
        assert!(post.success, "{:?}", post.error);
        assert_eq!(post.tests.len(), 1);
        assert!(post.tests[0].passed, "{:?}", post.tests);
    }

    #[test]
    fn test_variables_set_non_string_values() {
        // pm.variables.set likewise supports non-string values (number/bool/objects), stored as converted strings
        let s = JsSandbox::new().unwrap();
        let resp = ResponseContext {
            status: 200,
            body: r#"{"n":42,"ok":true,"p":{"a":1}}"#.into(),
            headers: HashMap::new(),
            duration_ms: 10,
            raw: String::new(),
            decoded: None,
        };
        let o = s.run_post_response(
            "pm.variables.set('n', pm.response.json().n); pm.variables.set('ok', pm.response.json().ok); pm.variables.set('p', pm.response.json().p);",
            &resp,
            None,
            None,
        );
        assert!(o.success, "{:?}", o.error);
        assert_eq!(o.temp_vars_set.get("n").unwrap(), "42");
        assert_eq!(o.temp_vars_set.get("ok").unwrap(), "true");
        assert_eq!(o.temp_vars_set.get("p").unwrap(), "{\"a\":1}");
    }

    #[test]
    fn test_sandbox_require_crypto_js() {
        // Postman scripts with zero changes: require('crypto-js') + global CryptoJS + AES + btoa/atob + Chinese Base64
        let s = JsSandbox::new().unwrap();
        let mut r = RequestContext {
            url: "https://api.example.com/orders".into(),
            method: "POST".into(),
            headers: HashMap::new(),
            body: "{\"amount\":100}".into(),
            raw: String::new(),
        };
        let script = r#"
            // 1) require the official crypto-js (Postman compatible)
            const CryptoJS2 = require('crypto-js');
            if (CryptoJS2.SHA256('abc').toString() !== 'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad') throw new Error('require crypto-js SHA256 mismatch');
            // 2) the global CryptoJS is the same instance as require'd
            if (CryptoJS !== CryptoJS2) throw new Error('global CryptoJS != require()');
            // 3) AES passphrase-mode roundtrip (OpenSSL Salted__ format)
            const ct = CryptoJS.AES.encrypt('hello world', 'secret key').toString();
            if (ct.indexOf('U2FsdGVkX1') !== 0) throw new Error('AES did not use OpenSSL Salted__ format: ' + ct);
            const pt = CryptoJS.AES.decrypt(ct, 'secret key').toString(CryptoJS.enc.Utf8);
            if (pt !== 'hello world') throw new Error('AES roundtrip failed: ' + pt);
            // 4) HmacSHA256 -> Base64 (the most common signing scenario)
            const sig = CryptoJS.HmacSHA256('msg', 'key').toString(CryptoJS.enc.Base64);
            const expectSig = CryptoJS.enc.Base64.stringify(CryptoJS.SHA256('msg'));
            if (typeof sig !== 'string' || sig.length !== 44) throw new Error('hmac b64 len: ' + sig);
            // 5) btoa / atob (Latin-1 semantics)
            if (btoa('hello') !== 'aGVsbG8=') throw new Error('btoa mismatch');
            if (atob('aGVsbG8=') !== 'hello') throw new Error('atob mismatch');
            if (btoa(unescape(encodeURIComponent('中文'))) !== '5Lit5paH') throw new Error('btoa cn mismatch');
            // 6) Chinese Base64 (official recommendation: enc.Utf8.parse + enc.Base64.stringify)
            const cn64 = CryptoJS.enc.Base64.stringify(CryptoJS.enc.Utf8.parse('你好'));
            if (CryptoJS.enc.Utf8.stringify(CryptoJS.enc.Base64.parse(cn64)) !== '你好') throw new Error('cn base64 roundtrip failed');
            // 7) pm.crypto (Rust backend) + matches the official library's result
            const pmSig = pm.crypto.hmac('sha256', 'key', 'msg');
            if (pmSig !== CryptoJS.HmacSHA256('msg', 'key').toString()) throw new Error('pm.crypto.hmac != CryptoJS');
            pm.environment.set('OK', 'yes');
        "#;
        let env = HashMap::new();
        let o = s.run_pre_request(script, &mut r, Some(&env), None);
        assert!(o.success, "script execution failed: {:?}", o.error);
        assert_eq!(o.vars_set.get("OK").map(|v| v.as_str()), Some("yes"));
    }

    #[test]
    fn test_sandbox_require_builtin_libs() {
        // lodash / moment / uuid / atob / btoa can all be require'd
        let s = JsSandbox::new().unwrap();
        let mut r = RequestContext {
            url: "https://a.com".into(),
            method: "GET".into(),
            headers: HashMap::new(),
            body: String::new(),
            raw: String::new(),
        };
        let script = r#"
            const _ = require('lodash');
            if (_.chunk([1,2,3,4], 2).length !== 2) throw new Error('lodash chunk failed');
            const moment = require('moment');
            if (!moment('2026-01-01').isValid()) throw new Error('moment invalid');
            const { v4: uuidv4 } = require('uuid');
            const u = uuidv4();
            if (u.length !== 36) throw new Error('uuid v4 length: ' + u);
            const atobFn = require('atob');
            if (atobFn('aGVsbG8=') !== 'hello') throw new Error('require atob failed');
            // pm.globals alias (commonly used in Postman scripts)
            pm.globals.set('g', '1');
            pm.variables.set('t', pm.globals.get('g'));
        "#;
        let o = s.run_pre_request(script, &mut r, None, None);
        assert!(o.success, "script execution failed: {:?}", o.error);
        assert_eq!(o.temp_vars_set.get("t").map(|v| v.as_str()), Some("1"));
    }

    #[test]
    fn test_sandbox_require_unknown_module() {
        let s = JsSandbox::new().unwrap();
        let mut r = RequestContext {
            url: "https://a.com".into(),
            method: "GET".into(),
            headers: HashMap::new(),
            body: String::new(),
            raw: String::new(),
        };
        let o = s.run_pre_request("require('nonexistent-lib');", &mut r, None, None);
        assert!(!o.success);
        assert!(o.error.unwrap_or_default().contains("module not built in"));
    }

    #[test]
    fn test_script_error_written_to_logs() {
        // When the script throws: both the error field and the log area should contain the error message
        let s = JsSandbox::new().unwrap();
        let mut r = RequestContext {
            url: "https://a.com".into(),
            method: "GET".into(),
            headers: HashMap::new(),
            body: String::new(),
            raw: String::new(),
        };
        let o = s.run_pre_request(
            "console.log('before'); throw new Error('boom-test');",
            &mut r,
            None,
            None,
        );
        assert!(!o.success);
        assert_eq!(o.error.as_deref(), Some("boom-test"));
        // The log area has the console output first, then the error appended last
        assert_eq!(o.logs[0].level, "log");
        assert_eq!(o.logs[0].message, "before");
        assert_eq!(o.logs[1].level, "error");
        assert!(o.logs[1].message.contains("boom-test"));
        assert!(o.logs[1].message.contains("script execution error"));
    }

    #[test]
    fn test_btoa_accepts_wordarray_like_postman() {
        // Postman compatible: btoa can directly accept a CryptoJS digest WordArray -> byte Base64
        let s = JsSandbox::new().unwrap();
        let mut r = RequestContext {
            url: "https://a.com".into(),
            method: "GET".into(),
            headers: HashMap::new(),
            body: String::new(),
            raw: String::new(),
        };
        // MD5('abc') = 900150983cd24fb0d6963f7d28e17f72 → base64 kAFQmDzST7DWlj99KOF/cg==
        let o = s.run_pre_request(
            r#"
            var b1 = btoa(CryptoJS.MD5('abc'));
            var b2 = CryptoJS.MD5('abc').toString(CryptoJS.enc.Base64);
            if (b1 !== 'kAFQmDzST7DWlj99KOF/cg==') throw new Error('btoa(WordArray) mismatch: ' + b1);
            if (b1 !== b2) throw new Error('btoa(WordArray) != toString(enc.Base64)');
            pm.environment.set('OK', b1);
            "#,
            &mut r,
            None,
            None,
        );
        assert!(o.success, "script execution failed: {:?}", o.error);
        assert_eq!(
            o.vars_set.get("OK").map(|v| v.as_str()),
            Some("kAFQmDzST7DWlj99KOF/cg==")
        );
    }

    #[test]
    fn test_btoa_non_string_gives_readable_hint() {
        // Passing a plain object (non-WordArray) to btoa should give a readable hint instead of an obscure conversion error
        let s = JsSandbox::new().unwrap();
        let mut r = RequestContext {
            url: "https://a.com".into(),
            method: "GET".into(),
            headers: HashMap::new(),
            body: String::new(),
            raw: String::new(),
        };
        let o = s.run_pre_request("btoa({});", &mut r, None, None);
        assert!(!o.success);
        let err = o.error.unwrap_or_default();
        assert!(
            err.contains("argument must be a string"),
            "error should include a readable hint, actual: {}",
            err
        );
        // The log area should also contain this error
        assert!(o
            .logs
            .iter()
            .any(|l| l.message.contains("argument must be a string")));
    }

    #[test]
    fn test_sandbox_clean_between_runs() {
        // Persistent sandbox: global pollution from the previous round must not leak into the next
        let s = JsSandbox::new().unwrap();
        let mut r = RequestContext {
            url: "https://a.com".into(),
            method: "GET".into(),
            headers: HashMap::new(),
            body: String::new(),
            raw: String::new(),
        };
        // Round 1: define global variables/functions, assign to globalThis, write env and temporary variables
        let o1 = s.run_pre_request(
            "var leaked = 's3cret'; function helper(){}; globalThis.x = 42; \
             pm.environment.set('e1','v1'); pm.variables.set('t1','tv');",
            &mut r,
            None,
            None,
        );
        assert!(o1.success, "{:?}", o1.error);
        assert_eq!(o1.vars_set.get("e1").unwrap(), "v1");
        assert_eq!(o1.temp_vars_set.get("t1").unwrap(), "tv");

        // Round 2: the previous round's globals / env / temporary variables are all invisible
        let o2 = s.run_pre_request(
            "if (typeof leaked !== 'undefined') throw new Error('var leaked'); \
             if (typeof helper !== 'undefined') throw new Error('fn leaked'); \
             if (typeof x !== 'undefined') throw new Error('globalThis.x leaked'); \
             if (pm.environment.get('e1') !== '') throw new Error('env leaked: ' + pm.environment.get('e1')); \
             if (pm.variables.get('t1') !== '') throw new Error('temp leaked'); \
             console.log('clean');",
            &mut r,
            None,
            None,
        );
        assert!(o2.success, "{:?}", o2.error);
        assert_eq!(o2.logs.len(), 1);
        assert_eq!(o2.logs[0].message, "clean");
    }

    #[test]
    fn test_console_restored_after_tampering() {
        // After a script tampers with console, the next round restores the pristine wrapper
        let s = JsSandbox::new().unwrap();
        let mut r = RequestContext {
            url: "https://a.com".into(),
            method: "GET".into(),
            headers: HashMap::new(),
            body: String::new(),
            raw: String::new(),
        };
        let o1 = s.run_pre_request(
            "globalThis.console.log = function(){ throw new Error('tampered'); };",
            &mut r,
            None,
            None,
        );
        assert!(o1.success, "{:?}", o1.error);
        let o2 = s.run_pre_request("console.log('ok2');", &mut r, None, None);
        assert!(o2.success, "{:?}", o2.error);
        assert_eq!(o2.logs.len(), 1);
        assert_eq!(o2.logs[0].message, "ok2");
    }

    #[test]
    fn test_pm_request_query_helpers() {
        // Pre-request script query convenience methods: read / overwrite / append / remove, rewriting pm.request.url in place while preserving the hash
        let s = JsSandbox::new().unwrap();
        let mut r = RequestContext {
            url: "https://a.com/api?x=1&y=2#frag".into(),
            method: "GET".into(),
            headers: HashMap::new(),
            body: String::new(),
            raw: String::new(),
        };
        let script = r#"
            if (pm.request.getQueryParam('x') !== '1') throw new Error('get x');
            if (pm.request.getQueryParam('nope') !== null) throw new Error('a missing parameter should return null');
            if (pm.request.getQueryParams('y').length !== 1) throw new Error('getAll y');
            if (pm.request.hasQueryParam('x') !== true) throw new Error('has x');
            pm.request.setQueryParam('x', '9');
            pm.request.addQueryParam('y', '3');
            pm.request.addQueryParam('q', 'hello world');
            pm.request.removeQueryParam('nope');
            if (pm.request.query.get('x') !== '9') throw new Error('query.get x');
        "#;
        let o = s.run_pre_request(script, &mut r, None, None);
        assert!(o.success, "{:?}", o.error);
        assert_eq!(r.url, "https://a.com/api?x=9&y=2&y=3&q=hello+world#frag");
    }

    #[test]
    fn test_url_and_urlsearchparams_globals() {
        // Global URL / URLSearchParams (not native to QuickJS): parsing, searchParams write-back, encoding
        let s = JsSandbox::new().unwrap();
        let mut r = RequestContext {
            url: "https://a.com".into(),
            method: "GET".into(),
            headers: HashMap::new(),
            body: String::new(),
            raw: String::new(),
        };
        let script = r#"
            var u = new URL('https://a.com/api?x=1#f');
            if (u.protocol !== 'https:') throw new Error('protocol: ' + u.protocol);
            if (u.hostname !== 'a.com') throw new Error('hostname');
            if (u.pathname !== '/api') throw new Error('pathname');
            if (u.search !== '?x=1') throw new Error('search: ' + u.search);
            if (u.hash !== '#f') throw new Error('hash');
            u.searchParams.set('x', '2');
            u.searchParams.append('z', '3');
            if (u.toString() !== 'https://a.com/api?x=2&z=3#f') throw new Error('toString: ' + u.toString());
            var sp = new URLSearchParams({ a: 1, b: 'x y' });
            if (sp.toString() !== 'a=1&b=x+y') throw new Error('sp: ' + sp.toString());
            if (new URLSearchParams('a=1&a=2').getAll('a').length !== 2) throw new Error('getAll');
        "#;
        let o = s.run_pre_request(script, &mut r, None, None);
        assert!(o.success, "{:?}", o.error);
    }

    #[test]
    fn test_require_url_and_querystring() {
        // Node-style built-in modules: require('url') / require('querystring')
        let s = JsSandbox::new().unwrap();
        let mut r = RequestContext {
            url: "https://a.com".into(),
            method: "GET".into(),
            headers: HashMap::new(),
            body: String::new(),
            raw: String::new(),
        };
        let script = r#"
            var url = require('url');
            var u = new url.URL('https://a.com/p?x=1');
            if (u.searchParams.get('x') !== '1') throw new Error('url.URL');
            var parsed = url.parse('https://a.com/p?x=1', true);
            if (parsed.hostname !== 'a.com') throw new Error('parse hostname');
            if (parsed.query.x !== '1') throw new Error('parse query');
            if (url.format({ protocol: 'https:', host: 'a.com', pathname: '/p', query: 'x=1' }) !== 'https://a.com/p?x=1') throw new Error('format');
            var qs = require('querystring');
            var o = qs.parse('a=1&b=2&b=3');
            if (o.a !== '1' || o.b[1] !== '3') throw new Error('qs.parse');
            if (qs.stringify({ a: 1, b: 'x y' }) !== 'a=1&b=x+y') throw new Error('qs.stringify');
        "#;
        let o = s.run_pre_request(script, &mut r, None, None);
        assert!(o.success, "{:?}", o.error);
    }

    /// Regression (user-reported error `redeclaration of 'res'`): top-level `const` / `let` / `class` create
    /// **global lexical bindings** - not on `globalThis` ([`reset_globals`] can't find them) and cannot be `delete`d,
    /// so in a persistent sandbox the second run of the same script inevitably redeclares.
    #[test]
    fn test_lexical_declarations_do_not_leak_between_runs() {
        let s = JsSandbox::new().unwrap();
        let resp = ResponseContext {
            status: 200,
            body: r#"{"method":"POST","json":{"name":"n","age":18}}"#.into(),
            headers: HashMap::new(),
            duration_ms: 10,
            raw: String::new(),
            decoded: None,
        };
        // Typical user-script style: const for the response, let, class, referenced again inside a closure
        let script = r#"
            const res = pm.response.json();
            let method = res.method;
            class Box {}
            pm.test('method is POST', function () { pm.expect(method).to.eql('POST'); });
        "#;
        let first = s.run_post_response(script, &resp, None, None);
        assert!(first.success, "{:?}", first.error);
        assert_eq!(first.tests.len(), 1);
        assert!(first.tests[0].passed, "{:?}", first.tests);

        // Second run of the same script: must not report redeclaration
        let second = s.run_post_response(script, &resp, None, None);
        assert!(
            second.success,
            "the second run must not report redeclaration: {:?}",
            second.error
        );
        assert_eq!(second.tests.len(), 1);
        assert!(second.tests[0].passed, "{:?}", second.tests);

        // The next round in the same sandbox (pre-request script) also cannot see these lexical bindings
        let mut r = RequestContext {
            url: "https://a.com".into(),
            method: "GET".into(),
            headers: HashMap::new(),
            body: String::new(),
            raw: String::new(),
        };
        let o = s.run_pre_request(
            "if (typeof res !== 'undefined') throw new Error('const res leaked'); \
             if (typeof method !== 'undefined') throw new Error('let method leaked'); \
             if (typeof Box !== 'undefined') throw new Error('class Box leaked');",
            &mut r,
            None,
            None,
        );
        assert!(o.success, "{:?}", o.error);
    }

    /// Isolation: a script rewriting `pm` members (overwriting existing / adding custom) must not leak into the next run.
    #[test]
    fn test_pm_members_restored_after_tampering() {
        let s = JsSandbox::new().unwrap();
        let resp = ResponseContext {
            status: 200,
            body: "{}".into(),
            headers: HashMap::new(),
            duration_ms: 10,
            raw: String::new(),
            decoded: None,
        };
        let o1 = s.run_post_response(
            "pm.expect = function(){ throw new Error('tampered expect'); }; \
             pm.extraHelper = 42;",
            &resp,
            None,
            None,
        );
        assert!(o1.success, "{:?}", o1.error);

        let o2 = s.run_post_response(
            "if (typeof pm.extraHelper !== 'undefined') throw new Error('pm.extraHelper leaked'); \
             pm.test('expect restored', function(){ pm.expect(1).to.equal(1); });",
            &resp,
            None,
            None,
        );
        assert!(o2.success, "{:?}", o2.error);
        assert_eq!(o2.tests.len(), 1);
        assert!(o2.tests[0].passed, "{:?}", o2.tests);
    }

    /// The wrapper must not change line numbers (the line number in a syntax error = the number the user sees).
    #[test]
    fn test_user_script_wrapper_keeps_line_numbers() {
        let wrapped = wrap_user_script("const a = 1;\nfoo();");
        let lines: Vec<&str> = wrapped.lines().collect();
        assert_eq!(lines[0], "(function(){const a = 1;");
        assert_eq!(lines[1], "foo();");
        assert_eq!(lines[2], "})()");
        // When the script ends with a line comment, the trailing newline keeps the wrapper from being swallowed by the comment
        assert!(wrap_user_script("var x = 1; // trailing").ends_with("\n})()"));
    }

    /// Postman matcher additions: `not` (negation), `include` (`contain` alias), `be.an` (`be.a` alias),
    /// `be.a('array'|'null')` - user scripts use both `to.not.include(...)` / `to.be.an('object')`.
    #[test]
    fn test_expect_postman_matchers() {
        let s = JsSandbox::new().unwrap();
        let resp = ResponseContext {
            status: 200,
            body: r#"{"method":"POST","json":{"name":"n","age":18,"tags":[]},"none":null}"#.into(),
            headers: HashMap::new(),
            duration_ms: 10,
            raw: String::new(),
            decoded: None,
        };
        let script = r#"
            const res = pm.response.json();
            pm.test('eql', function () { pm.expect(res.method).to.eql('POST'); });
            pm.test('an object', function () { pm.expect(res.json).to.be.an('object'); });
            pm.test('a number', function () { pm.expect(res.json.age).to.be.a('number'); });
            pm.test('an array', function () { pm.expect(res.json.tags).to.be.an('array'); });
            pm.test('a null', function () { pm.expect(res.none).to.be.a('null'); });
            pm.test('no placeholder', function () { pm.expect(JSON.stringify(res.json)).to.not.include('{{'); });
            pm.test('not equal', function () { pm.expect(res.method).to.not.equal('GET'); });
            pm.test('be.not? no -> not.to form', function () { pm.expect(res.method).not.to.equal('PUT'); });
            pm.test('negation must fail here', function () { pm.expect('GET').to.not.equal('GET'); });
        "#;
        let o = s.run_post_response(script, &resp, None, None);
        assert!(o.success, "{:?}", o.error);
        assert_eq!(o.tests.len(), 9, "{:?}", o.tests);
        for t in &o.tests {
            if t.name == "negation must fail here" {
                assert!(!t.passed, "a negated assertion must fail when it should");
            } else {
                assert!(t.passed, "`{}` should pass: {:?}", t.name, t);
            }
        }
    }

    /// User-reported failing script (included as-is): `const res = pm.response.json();` + four assertions + console.log.
    /// Before the fix the second round directly reported `redeclaration of 'res'`; here it runs three times in a row to ensure stability.
    #[test]
    fn test_user_reported_script_runs_repeatedly() {
        let s = JsSandbox::new().unwrap();
        let resp = ResponseContext {
            status: 200,
            body:
                r#"{"method":"POST","json":{"name":"n","email":"e@x.dev","age":18,"amount":9.9}}"#
                    .into(),
            headers: HashMap::new(),
            duration_ms: 12,
            raw: String::new(),
            decoded: None,
        };
        let script = r#"
const res = pm.response.json();

            pm.test('echoed method is POST', () => pm.expect(res.method).to.eql('POST'));
            pm.test('echoed the request body json field', () => pm.expect(res.json).to.be.an('object'));
            pm.test('no uninterpolated placeholders', () => pm.expect(JSON.stringify(res.json)).to.not.include('{{'));
            pm.test('age is a number', () => pm.expect(res.json.age).to.be.a('number'));

console.log('echo name =', res.json.name, '| email =', res.json.email, '| age =', res.json.age, '| amount =', res.json.amount);
"#;
        for round in 1..=3 {
            let o = s.run_post_response(script, &resp, None, None);
            assert!(o.success, "round {round} execution failed: {:?}", o.error);
            assert_eq!(o.tests.len(), 4, "round {round}: {:?}", o.tests);
            assert!(
                o.tests.iter().all(|t| t.passed),
                "round {round} assertions: {:?}",
                o.tests
            );
            assert_eq!(o.logs.len(), 1, "round {round} logs: {:?}", o.logs);
            assert!(
                o.logs[0].message.contains("age = 18"),
                "round {round} console output: {}",
                o.logs[0].message
            );
        }
    }
}
