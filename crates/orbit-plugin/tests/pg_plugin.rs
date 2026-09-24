//! End-to-end verification of the PG query plugin:
//! 1) The host loads `pg_query.wasm` and recognizes the `pg` protocol
//! 2) Verify the handshake + auth + query + result-parsing chain via a mock PG server
//!
//! Prerequisite: build the example plugin first
//!   cargo build -p pg-query --target wasm32-wasip2 --release   (under examples/plugins/pg-query)
//! If the wasm artifact is missing, the test is skipped automatically.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};

use hmac::{Hmac, Mac};
use orbit_plugin::PluginManager;
use pbkdf2::pbkdf2_hmac;
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

const MOCK_PASSWORD: &str = "postgres";

// ─── SCRAM-SHA-256 server utilities (verify the plugin client implementation) ──────────────

fn b64_encode(data: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(data)
}

fn b64_decode(s: &str) -> Vec<u8> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode(s).unwrap()
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("hmac");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn sha256(data: &[u8]) -> Vec<u8> {
    let mut h = Sha256::new();
    h.update(data);
    h.finalize().to_vec()
}

fn attrs(msg: &str) -> std::collections::HashMap<&str, &str> {
    let mut map = std::collections::HashMap::new();
    for part in msg.split(',') {
        if let Some(eq) = part.find('=') {
            map.insert(&part[..eq], &part[eq + 1..]);
        }
    }
    map
}

fn wasm_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/plugins/pg-query/target/wasm32-wasip2/release/pg_query.wasm")
}

fn find_free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

// ─── Minimal PG wire server (trust auth + one fixed query result) ──────────────

fn be32(n: i32) -> [u8; 4] {
    n.to_be_bytes()
}

fn read_exact_n(stream: &mut TcpStream, n: usize) -> Vec<u8> {
    let mut buf = vec![0u8; n];
    stream.read_exact(&mut buf).expect("read");
    buf
}

/// Read one frontend message (len-prefixed, no type byte) or a message with a type byte
fn read_frame(stream: &mut TcpStream) -> Vec<u8> {
    let len_buf = read_exact_n(stream, 4);
    let len = i32::from_be_bytes([len_buf[0], len_buf[1], len_buf[2], len_buf[3]]);
    let mut body = vec![0u8; (len - 4) as usize];
    stream.read_exact(&mut body).expect("read body");
    body
}

fn send_backend(stream: &mut TcpStream, ty: u8, body: &[u8]) {
    let mut frame = vec![ty];
    frame.extend_from_slice(&be32(body.len() as i32 + 4));
    frame.extend_from_slice(body);
    stream.write_all(&frame).expect("write");
}

/// Read the startup/query message the client sends and return the body (may carry a type prefix for queries)
fn handle_client(mut stream: TcpStream) {
    // Add a timeout to avoid an infinite wait if the protocol does not match
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .ok();
    stream
        .set_write_timeout(Some(std::time::Duration::from_secs(5)))
        .ok();
    eprintln!("[mock] accepted client");

    // 1) StartupMessage: len + protocol version + parameters (no type)
    let startup = read_frame(&mut stream);
    eprintln!("[mock] startup len={}", startup.len());
    assert!(startup.len() >= 4, "startup too short");
    let version = i32::from_be_bytes([startup[0], startup[1], startup[2], startup[3]]);
    assert_eq!(version, 196608, "protocol version should be 3.0");

    // 2) SCRAM-SHA-256 authentication (SASL)
    // 2a) AuthenticationSASL: mechanism list
    let mut sasl_body = Vec::new();
    sasl_body.extend_from_slice(b"SCRAM-SHA-256\x00\x00");
    let mut body = Vec::new();
    body.extend_from_slice(&be32(10)); // AUTH_SASL
    body.extend_from_slice(&sasl_body);
    send_backend(&mut stream, b'R', &body);
    eprintln!("[mock] sent AuthenticationSASL");

    // 2b) Read SASLInitialResponse (`p`): mechanism + \0 + len + client-first
    let first = read_exact_n(&mut stream, 1)[0];
    assert_eq!(first, b'p', "client should send SASLInitialResponse");
    let len_buf = read_exact_n(&mut stream, 4);
    let len = i32::from_be_bytes([len_buf[0], len_buf[1], len_buf[2], len_buf[3]]);
    let mut init = vec![0u8; (len - 4) as usize];
    stream.read_exact(&mut init).expect("read sasl init");
    // init: "SCRAM-SHA-256\0" + int32 len + client-first-message
    let mech_end = init.iter().position(|&b| b == 0).unwrap();
    let mech = String::from_utf8_lossy(&init[..mech_end]).into_owned();
    assert_eq!(mech, "SCRAM-SHA-256");
    let cfm_len = i32::from_be_bytes([
        init[mech_end + 1],
        init[mech_end + 2],
        init[mech_end + 3],
        init[mech_end + 4],
    ]) as usize;
    let client_first =
        String::from_utf8_lossy(&init[mech_end + 5..mech_end + 5 + cfm_len]).into_owned();
    eprintln!("[mock] client-first='{}'", client_first);
    // client-first = "n,," + bare；bare = "n=<user>,r=<nonce>"
    let client_first_bare = client_first
        .strip_prefix("n,,")
        .unwrap_or(&client_first)
        .to_string();

    // Server parameters: fixed salt/iterations; server nonce = client nonce + fixed suffix
    const SALT: &[u8] = b"0123456789abcdef";
    const ITER: u32 = 4096;
    let c_r = attrs(&client_first_bare).get("r").unwrap().to_string();
    let s_r = format!("{}srvsalt1234", c_r);
    let server_first = format!("r={},s={},i={}", s_r, b64_encode(SALT), ITER);

    // 2c) AuthenticationSASLContinue (11) + server-first-message
    let mut sf = Vec::new();
    sf.extend_from_slice(&be32(11)); // AUTH_SASL_CONTINUE
    sf.extend_from_slice(server_first.as_bytes());
    send_backend(&mut stream, b'R', &sf);
    eprintln!("[mock] sent SASLContinue");

    // 2d) Read SASLResponse (`p`): client-final-message
    let first = read_exact_n(&mut stream, 1)[0];
    assert_eq!(first, b'p', "client should send SASLResponse");
    let len_buf = read_exact_n(&mut stream, 4);
    let len = i32::from_be_bytes([len_buf[0], len_buf[1], len_buf[2], len_buf[3]]);
    let mut cfin = vec![0u8; (len - 4) as usize];
    stream.read_exact(&mut cfin).expect("read sasl response");
    let client_final = String::from_utf8_lossy(&cfin).into_owned();
    eprintln!("[mock] client-final='{}'", client_final);

    // 2e) Verify ClientProof (computed with the same salt/iter as scram_server)
    let cf = attrs(&client_final);
    let proof_b64 = cf.get("p").expect("client-final missing p");
    let proof = b64_decode(proof_b64);
    let c_r = attrs(&client_first_bare).get("r").unwrap().to_string();
    let mut s_r = c_r.clone();
    s_r.push_str("srvsalt1234");
    let cf_without = format!("c={},r={}", cf.get("c").unwrap(), s_r);
    let server_first_for_auth =
        format!("r={},s={},i={}", s_r, b64_encode(b"0123456789abcdef"), 4096);
    let auth_message = format!(
        "{},{},{}",
        client_first_bare, server_first_for_auth, cf_without
    );
    let mut salted = [0u8; 32];
    pbkdf2_hmac::<Sha256>(
        MOCK_PASSWORD.as_bytes(),
        b"0123456789abcdef",
        4096,
        &mut salted,
    );
    let client_key = hmac_sha256(&salted, b"Client Key");
    let stored_key = sha256(&client_key);
    let client_signature = hmac_sha256(&stored_key, auth_message.as_bytes());
    let expected_proof: Vec<u8> = client_key
        .iter()
        .zip(client_signature.iter())
        .map(|(a, b)| a ^ b)
        .collect();
    assert_eq!(
        proof, expected_proof,
        "ClientProof verification failed (the plugin's SCRAM client implementation is wrong)"
    );

    // 2f) AuthenticationSASLFinal (12) + server-final（v=ServerSignature）
    let server_signature = hmac_sha256(
        &hmac_sha256(&salted, b"Server Key"),
        auth_message.as_bytes(),
    );
    let mut ff = Vec::new();
    ff.extend_from_slice(&be32(12)); // AUTH_SASL_FINAL
    ff.extend_from_slice(format!("v={}", b64_encode(&server_signature)).as_bytes());
    send_backend(&mut stream, b'R', &ff);
    eprintln!("[mock] sent SASLFinal, SCRAM auth succeeded");

    // 3) ParameterStatus ×2 + BackendKeyData
    send_backend(&mut stream, b'S', b"server_version\x0015.0\x00");
    send_backend(&mut stream, b'S', b"client_encoding\x00UTF8\x00");
    let mut kd = Vec::new();
    kd.extend_from_slice(&be32(1));
    kd.extend_from_slice(&be32(2));
    send_backend(&mut stream, b'K', &kd);

    // 4) ReadyForQuery（idle）
    send_backend(&mut stream, b'Z', b"I");

    // 5) Loop to handle queries
    loop {
        let first = read_exact_n(&mut stream, 1)[0];
        let len_buf = read_exact_n(&mut stream, 4);
        let len = i32::from_be_bytes([len_buf[0], len_buf[1], len_buf[2], len_buf[3]]);
        let mut body = vec![0u8; (len - 4) as usize];
        stream.read_exact(&mut body).expect("read query body");

        match first {
            b'Q' => {
                let sql_end = body.iter().position(|&b| b == 0).unwrap_or(body.len());
                let sql = String::from_utf8_lossy(&body[..sql_end]).into_owned();
                eprintln!("[mock] query sql='{}'", sql);
                if sql.to_lowercase().contains("select 1") {
                    // RowDescription: 1 column "?column?" int4
                    let mut rd = Vec::new();
                    rd.extend_from_slice(&1i16.to_be_bytes()); // field count
                    rd.extend_from_slice(b"?column?\x00"); // name
                    rd.extend_from_slice(&be32(0)); // table oid
                    rd.extend_from_slice(&0i16.to_be_bytes()); // attr
                    rd.extend_from_slice(&be32(23)); // int4
                    rd.extend_from_slice(&4i16.to_be_bytes()); // size
                    rd.extend_from_slice(&be32(-1)); // type mod
                    rd.extend_from_slice(&0i16.to_be_bytes()); // format
                    send_backend(&mut stream, b'T', &rd);

                    // DataRow: 1 value = 1
                    let mut dr = Vec::new();
                    dr.extend_from_slice(&1i16.to_be_bytes());
                    dr.extend_from_slice(&be32(1));
                    dr.push(b'1');
                    send_backend(&mut stream, b'D', &dr);

                    // CommandComplete
                    send_backend(&mut stream, b'C', b"SELECT 1\x00");
                } else {
                    // Other SQL -> error
                    let mut er = Vec::new();
                    er.extend_from_slice(b"SERROR\x00");
                    er.extend_from_slice(b"C42501\x00");
                    er.extend_from_slice("Mmock unknown query\x00".as_bytes());
                    er.push(0);
                    send_backend(&mut stream, b'E', &er);
                    send_backend(&mut stream, b'Z', b"I");
                }
                send_backend(&mut stream, b'Z', b"I");
            }
            b'X' => break, // Terminate
            _ => { /* ignore other frontend messages */ }
        }
    }
}

#[tokio::test]
async fn pg_plugin_loads_and_queries() {
    let wasm = wasm_path();
    if !wasm.exists() {
        eprintln!(
            "SKIP: {} not found (build the example plugin first)",
            wasm.display()
        );
        return;
    }
    let bytes = std::fs::read(&wasm).unwrap();

    // 1) The host loads and recognizes the pg protocol
    let mut mgr = PluginManager::new().expect("plugin manager");
    let (kind, caps) = mgr
        .load_wasm("com.example.pg", &bytes, None)
        .await
        .expect("load_wasm");
    assert_eq!(kind, "protocol");
    assert!(
        caps.contains(&"pg".to_string()),
        "capabilities should contain pg, got {:?}",
        caps
    );

    // 2) Start the mock PG server (separate OS thread to avoid contending for the runtime with the host's block_on)
    let port = find_free_port();
    std::thread::spawn(move || {
        let listener = TcpListener::bind(("127.0.0.1", port)).unwrap();
        for stream in listener.incoming() {
            match stream {
                Ok(s) => handle_client(s),
                Err(_) => break,
            }
        }
    });
    // Wait for the server to be ready
    std::thread::sleep(std::time::Duration::from_millis(100));

    // 3) Get a client from the dynamic registry and run the query
    let mut client = orbit_protocol::registry::build_client_by_id("pg").expect("pg client");
    let req = orbit_protocol::types::ProtocolRequest {
        target: format!("127.0.0.1:{}", port),
        operation: String::new(),
        metadata: vec![],
        payload: b"SELECT 1".to_vec(),
        timeout: Some(std::time::Duration::from_secs(5)),
        streaming_mode: None,
        payload_format: None,
        response_format: None,
        options: Default::default(),
        connection: Some(
            serde_json::json!({
                "host": "127.0.0.1",
                "port": port,
                "username": "postgres",
                "password": "postgres",
                "database": "postgres",
            })
            .to_string(),
        ),
    };
    let resp = tokio::time::timeout(std::time::Duration::from_secs(15), client.execute(req))
        .await
        .expect("execute 15s timeout")
        .expect("execute returned an error");
    eprintln!("[test] execute ok");
    assert_eq!(resp.status_code, 200);

    let body = String::from_utf8_lossy(&resp.payload).into_owned();
    let json: serde_json::Value = serde_json::from_str(&body).expect("response should be JSON");
    assert_eq!(json["columns"][0], "?column?");
    // In the PG text protocol, the int4 value "1" is parsed by the plugin as a JSON number
    assert_eq!(json["rows"][0]["?column?"], 1);
    assert_eq!(json["rowCount"], 1);
}
