//! QuickJS bridge layer: Rust construction of JS-side objects such as console / env / pm / crypto bridge / require libs.
//!
//! Cross-language interaction conventions:
//! - The crypto bridge uses **hex strings** for input and output, avoiding complex object conversion between QuickJS and Rust;
//! - Errors are returned via the [`ORBIT_ERR_PREFIX`] sentinel string and converted to JS exceptions by bootstrap's `__checkErr`
//!   (rquickjs 0.12 closures don't support returning a Result, and a pending exception is silently dropped after a closure returns normally).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use rquickjs::{Ctx, Function, Object, Value};

use crate::cipher::{BlockMode, Padding};
use crate::crypto::hex_encode;
use crate::types::{EnvVars, ScriptLog};

// ── Built-in library sources (Postman sandbox-compatible require modules, embedded at compile time) ──
const CRYPTO_JS_SRC: &str = include_str!("../assets/crypto-js.min.js");
const LODASH_SRC: &str = include_str!("../assets/lodash.min.js");
const MOMENT_SRC: &str = include_str!("../assets/moment.min.js");
const UUID_SRC: &str = include_str!("../assets/uuid.min.js");

/// Sentinel marker for throwing exceptions into JS: the Rust side returns a marked string,
/// which bootstrap's `__checkErr` detects and turns into `throw new Error(...)`.
pub(crate) const ORBIT_ERR_PREFIX: &str = "!!__orbit_err__::";

pub(crate) fn js_throw<'js>(_ctx: &Ctx<'js>, msg: &str) -> String {
    format!("{}{}", ORBIT_ERR_PREFIX, msg)
}

/// Extract JS exception info, returning `(message, full)`:
/// - `message`: the exception object's `.message` (used for `outcome.error`)
/// - `full`: message + stack (if any), used for the script log
///
/// Note: `ctx.catch()` clears the pending exception and can only be called once, hence the combined handling.
pub(crate) fn js_exception_info<'js>(ctx: &Ctx<'js>, e: &rquickjs::Error) -> (String, String) {
    let mut message = e.to_string();
    let mut stack = String::new();
    if e.is_exception() {
        let exc = ctx.catch();
        if let Some(obj) = exc.as_object() {
            if let Ok(m) = obj.get::<_, String>("message") {
                if !m.is_empty() {
                    message = m;
                }
            }
            if let Ok(s) = obj.get::<_, String>("stack") {
                if !s.is_empty() {
                    stack = s;
                }
            }
        } else if let Some(s) = exc.as_string() {
            if let Ok(s) = s.to_string() {
                message = s;
            }
        }
    }
    let full = if stack.is_empty() {
        message.clone()
    } else {
        format!("{}\n{}", message, stack)
    };
    (message, full)
}

/// Take the inner value out of Rc<RefCell<T>> (clone as fallback).
pub(crate) fn take<T: Clone>(rc: Rc<RefCell<T>>) -> T {
    match Rc::try_unwrap(rc) {
        Ok(c) => c.into_inner(),
        Err(rc) => rc.borrow().clone(),
    }
}

/// Inject Postman sandbox globals: `btoa` / `atob` (Latin-1 semantics, consistent with the browser) and the underlying crypto bridge.
/// Called only once at initialization (before bootstrap); restored before each run via [`restore_static_globals`].
pub(crate) fn inject_sandbox_globals<'js>(ctx: &Ctx<'js>) {
    ctx.globals()
        .set(
            "btoa",
            Function::new(ctx.clone(), |c: Ctx<'js>, s: String| -> String {
                match crate::crypto::btoa(&s) {
                    Ok(v) => v,
                    Err(e) => js_throw(&c, &e),
                }
            }),
        )
        .ok();
    ctx.globals()
        .set(
            "atob",
            Function::new(ctx.clone(), |c: Ctx<'js>, s: String| -> String {
                match crate::crypto::atob(&s) {
                    Ok(v) => v,
                    Err(e) => js_throw(&c, &e),
                }
            }),
        )
        .ok();
    let bridge = build_crypto(ctx);
    ctx.globals().set("__orbit_crypto", bridge).ok();
    ctx.globals()
        .set("__orbitRequireLib", build_require_lib(ctx))
        .ok();
}

/// Restore static globals before each run: btoa/atob restore bootstrap's wrappers (with type checks and WordArray support),
/// and the crypto bridge and require lib sources are re-registered (to prevent script overwrites).
pub(crate) fn restore_static_globals<'js>(ctx: &Ctx<'js>) {
    for (key, pristine) in [("btoa", "__orbitBtoa"), ("atob", "__orbitAtob")] {
        if let Ok(v) = ctx.globals().get::<_, Value<'_>>(pristine) {
            ctx.globals().set(key, v).ok();
        }
    }
    let bridge = build_crypto(ctx);
    ctx.globals().set("__orbit_crypto", bridge).ok();
    ctx.globals()
        .set("__orbitRequireLib", build_require_lib(ctx))
        .ok();
}

/// require lib source bridge: returns built-in lib sources; throws a JS exception for non-built-in modules.
pub(crate) fn build_require_lib<'js>(ctx: &Ctx<'js>) -> Function<'js> {
    Function::new(ctx.clone(), |c: Ctx<'js>, name: String| -> String {
        match name.as_str() {
            "crypto-js" => CRYPTO_JS_SRC.to_string(),
            "lodash" => LODASH_SRC.to_string(),
            "moment" => MOMENT_SRC.to_string(),
            "uuid" => UUID_SRC.to_string(),
            other => js_throw(
                &c,
                &format!(
                    "require: module not built in '{}' (available: crypto-js, lodash, moment, uuid, atob, btoa, url, querystring)",
                    other
                ),
            ),
        }
    })
    .unwrap()
}

pub(crate) fn register_console<'js>(ctx: &Ctx<'js>, logs: &Rc<RefCell<Vec<ScriptLog>>>) {
    let logs = logs.clone();
    let f = Function::new(ctx.clone(), move |level: String, msg: String| {
        logs.borrow_mut().push(ScriptLog {
            level,
            message: msg,
        });
    });
    ctx.globals().set("__orbit_console", f).ok();
}

/// Underlying crypto bridge (the `__orbit_crypto` global; the JS facade calls the Rust implementation through it).
/// All inputs and outputs are **hex strings**, avoiding complex object conversion between QuickJS and Rust.
///
/// Exposes:
/// - Hash: `md5/sha1/sha224/sha256/sha384/sha512/sha3(hex,bits)/ripemd160(hex)`
/// - HMAC：`hmac(algo,keyHex,msgHex)` / `hmacBase64(...)`
/// - Base64: `b64Encode(hex)` / `b64Decode(b64)`; `btoa/atob` see the globals
/// - Passphrase derivation: `evpBytesToKey(ppHex, saltHex, keyLen, ivLen) -> hex(key+iv)`
/// - Block ciphers (hex exchange): `aesEncrypt/aesDecrypt/desEncrypt/desDecrypt/tdesEncrypt/tdesDecrypt`
///   `(dataHex, keyHex, ivHex, mode, padding) -> hex`，mode=cbc|ecb，padding=pkcs7|none|zero
/// - Stream ciphers: `rc4(keyHex, dataHex)` / `rc4Drop(keyHex, dataHex, dropBytes)`
/// - Random bytes: `randBytes(n) -> hex`
pub(crate) fn build_crypto<'js>(ctx: &Ctx<'js>) -> Object<'js> {
    let c = Object::new(ctx.clone()).unwrap();

    macro_rules! hash_fn {
        ($name:literal, $rust:path) => {
            c.set(
                $name,
                Function::new(ctx.clone(), move |c: Ctx<'js>, s: String| -> String {
                    match crate::crypto::hex_decode(&s) {
                        Ok(bytes) => hex_encode(&$rust(&bytes)),
                        Err(e) => js_throw(&c, &e),
                    }
                }),
            )
            .ok();
        };
    }
    hash_fn!("md5", crate::crypto::md5_bytes);
    hash_fn!("sha1", crate::crypto::sha1_bytes);
    hash_fn!("sha224", crate::crypto::sha224_bytes);
    hash_fn!("sha256", crate::crypto::sha256_bytes);
    hash_fn!("sha384", crate::crypto::sha384_bytes);
    hash_fn!("sha512", crate::crypto::sha512_bytes);
    c.set(
        "sha3",
        Function::new(
            ctx.clone(),
            |c: Ctx<'js>, s: String, bits: Option<i64>| -> String {
                let b = bits.unwrap_or(512) as usize;
                let bytes = match crate::crypto::hex_decode(&s) {
                    Ok(v) => v,
                    Err(e) => return js_throw(&c, &e),
                };
                match crate::crypto::sha3_bytes(&bytes, b) {
                    Ok(d) => hex_encode(&d),
                    Err(e) => js_throw(&c, &e),
                }
            },
        ),
    )
    .ok();
    hash_fn!("ripemd160", crate::crypto::ripemd160_bytes);

    c.set(
        "hmac",
        Function::new(
            ctx.clone(),
            |c: Ctx<'js>, algo: String, key: String, msg: String| -> String {
                let kb = match crate::crypto::hex_decode(&key) {
                    Ok(v) => v,
                    Err(e) => return js_throw(&c, &e),
                };
                let mb = match crate::crypto::hex_decode(&msg) {
                    Ok(v) => v,
                    Err(e) => return js_throw(&c, &e),
                };
                match crate::crypto::hmac_bytes(&algo, &kb, &mb) {
                    Ok(d) => hex_encode(&d),
                    Err(e) => js_throw(&c, &e),
                }
            },
        ),
    )
    .ok();
    c.set(
        "hmacBase64",
        Function::new(
            ctx.clone(),
            |c: Ctx<'js>, algo: String, key: String, msg: String| -> String {
                let kb = match crate::crypto::hex_decode(&key) {
                    Ok(v) => v,
                    Err(e) => return js_throw(&c, &e),
                };
                let mb = match crate::crypto::hex_decode(&msg) {
                    Ok(v) => v,
                    Err(e) => return js_throw(&c, &e),
                };
                match crate::crypto::hmac_bytes(&algo, &kb, &mb) {
                    Ok(d) => crate::crypto::b64_encode(&d),
                    Err(e) => js_throw(&c, &e),
                }
            },
        ),
    )
    .ok();

    c.set(
        "b64Encode",
        Function::new(ctx.clone(), |hex: String| -> String {
            crate::crypto::b64_encode(&crate::crypto::hex_decode(&hex).unwrap_or_default())
        }),
    )
    .ok();
    c.set(
        "b64Decode",
        Function::new(ctx.clone(), |c: Ctx<'js>, s: String| -> String {
            match crate::crypto::b64_decode(&s) {
                Ok(bytes) => hex_encode(&bytes),
                Err(e) => js_throw(&c, &e),
            }
        }),
    )
    .ok();

    c.set(
        "evpBytesToKey",
        Function::new(
            ctx.clone(),
            |c: Ctx<'js>, pp_hex: String, salt_hex: String, key_len: i64, iv_len: i64| -> String {
                let pp = match crate::crypto::hex_decode(&pp_hex) {
                    Ok(v) => v,
                    Err(e) => return js_throw(&c, &e),
                };
                let salt = match crate::crypto::hex_decode(&salt_hex) {
                    Ok(v) => v,
                    Err(e) => return js_throw(&c, &e),
                };
                let (mut key, iv) =
                    crate::crypto::evp_bytes_to_key(&pp, &salt, key_len as usize, iv_len as usize);
                key.extend(iv);
                hex_encode(&key)
            },
        ),
    )
    .ok();

    for (name, algo) in [
        ("aesEncrypt", "aes"),
        ("aesDecrypt", "aes"),
        ("desEncrypt", "des"),
        ("desDecrypt", "des"),
        ("tdesEncrypt", "3des"),
        ("tdesDecrypt", "3des"),
    ] {
        let enc = name.ends_with("Encrypt");
        c.set(
            name,
            Function::new(
                ctx.clone(),
                move |c: Ctx<'js>,
                      data: String,
                      key: String,
                      iv: String,
                      mode: String,
                      padding: String|
                      -> String {
                    match block_crypt_hex(algo, enc, &data, &key, &iv, &mode, &padding) {
                        Ok(out) => out,
                        Err(e) => js_throw(&c, &e),
                    }
                },
            ),
        )
        .ok();
    }

    c.set(
        "rc4",
        Function::new(
            ctx.clone(),
            |c: Ctx<'js>, key_hex: String, data_hex: String| -> String {
                let key = match crate::crypto::hex_decode(&key_hex) {
                    Ok(v) => v,
                    Err(e) => return js_throw(&c, &e),
                };
                let data = match crate::crypto::hex_decode(&data_hex) {
                    Ok(v) => v,
                    Err(e) => return js_throw(&c, &e),
                };
                hex_encode(&crate::crypto::rc4_xor(&key, &data))
            },
        ),
    )
    .ok();
    c.set(
        "rc4Drop",
        Function::new(
            ctx.clone(),
            |c: Ctx<'js>, key_hex: String, data_hex: String, drop: i64| -> String {
                let key = match crate::crypto::hex_decode(&key_hex) {
                    Ok(v) => v,
                    Err(e) => return js_throw(&c, &e),
                };
                let data = match crate::crypto::hex_decode(&data_hex) {
                    Ok(v) => v,
                    Err(e) => return js_throw(&c, &e),
                };
                hex_encode(&crate::crypto::rc4_drop(&key, &data, drop.max(0) as usize))
            },
        ),
    )
    .ok();

    c.set(
        "randBytes",
        Function::new(ctx.clone(), |n: i64| -> String {
            random_hex(n.max(0) as usize)
        }),
    )
    .ok();

    c
}

/// Block cipher hex exchange wrapper.
fn block_crypt_hex(
    algo: &str,
    encrypt: bool,
    data_hex: &str,
    key_hex: &str,
    iv_hex: &str,
    mode_str: &str,
    padding_str: &str,
) -> Result<String, String> {
    let data = crate::crypto::hex_decode(data_hex)?;
    let key = crate::crypto::hex_decode(key_hex)?;
    let mode = BlockMode::parse(mode_str)?;
    let padding = Padding::parse(padding_str)?;
    let iv = if iv_hex.trim().is_empty() {
        None
    } else {
        Some(crate::crypto::hex_decode(iv_hex)?)
    };
    let out = match algo {
        "aes" => crate::cipher::aes_crypt(encrypt, &data, &key, iv.as_deref(), mode, padding)?,
        "des" => {
            crate::cipher::des_crypt("des", encrypt, &data, &key, iv.as_deref(), mode, padding)?
        }
        "3des" => {
            crate::cipher::des_crypt("3des", encrypt, &data, &key, iv.as_deref(), mode, padding)?
        }
        other => return Err(format!("block_crypt: unknown algorithm {}", other)),
    };
    Ok(hex_encode(&out))
}

/// Pseudo-random bytes (uuid v4 entropy source, sufficient for salt / IV / getRandomValues).
fn random_hex(n: usize) -> String {
    let mut out = Vec::with_capacity(n);
    while out.len() < n {
        let u = uuid::Uuid::new_v4();
        out.extend_from_slice(u.as_bytes());
    }
    out.truncate(n);
    hex_encode(&out)
}

/// Environment variable object (`pm.environment`):
/// - `get`: this script's writes (`vars_set`) take priority, falling back to the env snapshot;
/// - `set`: bound via bootstrap's [`__orbitStorageSet`] factory (type conversion is done on the JS side).
pub(crate) fn build_env<'js>(
    ctx: &Ctx<'js>,
    env_vars: Option<&EnvVars>,
    vars_set: &Rc<RefCell<HashMap<String, String>>>,
) -> Object<'js> {
    let e = Object::new(ctx.clone()).unwrap();
    let snap: HashMap<String, String> = env_vars.cloned().unwrap_or_default();
    let vs = vars_set.clone();
    e.set(
        "get",
        Function::new(ctx.clone(), move |name: String| -> String {
            vs.borrow()
                .get(&name)
                .cloned()
                .or_else(|| snap.get(&name).cloned())
                .unwrap_or_default()
        }),
    )
    .ok();
    attach_set(ctx, &e, vars_set);
    e
}

/// Temporary variable object (`pm.variables`): independent of `pm.environment`, valid only for this request's lifetime, not persisted.
///
/// Read priority (Postman-style local-first): temporary variables written by this script -> seed temporary variables (e.g. written by the same request's pre-request script) -> env variable snapshot.
/// Writes: go only into the separate `temp_set` and don't affect `vars_set` (environment).
pub(crate) fn build_temp<'js>(
    ctx: &Ctx<'js>,
    env_vars: Option<&EnvVars>,
    temp_init: Option<&HashMap<String, String>>,
    temp_set: &Rc<RefCell<HashMap<String, String>>>,
) -> Object<'js> {
    let e = Object::new(ctx.clone()).unwrap();
    let env_snap: HashMap<String, String> = env_vars.cloned().unwrap_or_default();
    let temp_snap: HashMap<String, String> = temp_init.cloned().unwrap_or_default();
    let ts = temp_set.clone();
    e.set(
        "get",
        Function::new(ctx.clone(), move |name: String| -> String {
            ts.borrow()
                .get(&name)
                .cloned()
                .or_else(|| temp_snap.get(&name).cloned())
                .or_else(|| env_snap.get(&name).cloned())
                .unwrap_or_default()
        }),
    )
    .ok();
    attach_set(ctx, &e, temp_set);
    e
}

/// Bind set to an object via bootstrap's `__orbitStorageSet` factory (backed by `set_backend`).
/// Warning: do not capture Ctx in a Rust closure: it would extend Ctx's lifetime and trigger a QuickJS gc assertion crash when the Runtime is destroyed.
fn attach_set<'js>(
    ctx: &Ctx<'js>,
    obj: &Object<'js>,
    set_backend: &Rc<RefCell<HashMap<String, String>>>,
) {
    let backend = Function::new(ctx.clone(), {
        let vs = set_backend.clone();
        move |name: String, value: String| {
            vs.borrow_mut().insert(name, value);
        }
    });
    if let Ok(factory) = ctx.globals().get::<_, Function<'_>>("__orbitStorageSet") {
        if let Ok(setter) = factory.call::<_, Function<'_>>((backend,)) {
            obj.set("set", setter).ok();
        }
    }
}

/// Read-only secret access: `pm.secret.get(name)`.
/// Secrets are passed in by the caller via the `env_vars` snapshot (merged with variables); scripts can only read, not write.
pub(crate) fn build_secret<'js>(ctx: &Ctx<'js>, env_vars: Option<&EnvVars>) -> Object<'js> {
    let e = Object::new(ctx.clone()).unwrap();
    let snap: HashMap<String, String> = env_vars.cloned().unwrap_or_default();
    e.set(
        "get",
        Function::new(ctx.clone(), move |name: String| -> String {
            snap.get(&name).cloned().unwrap_or_default()
        }),
    )
    .ok();
    e
}
