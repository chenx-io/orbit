use crate::db::Database;
use serde::{Deserialize, Serialize};
use tauri::State;
use uuid::Uuid;

// ─── Types ────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct CollectionData {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub sort_order: i32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RequestData {
    pub id: String,
    pub collection_id: String,
    pub name: String,
    pub protocol: String,
    pub method: String,
    pub url: String,
    pub headers: String,
    pub body: String,
    pub content_type: String,
    pub cookies: String,
    pub request_body_model_id: Option<String>,
    pub responses: String,
    pub sort_order: i32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EnvironmentData {
    pub id: String,
    pub name: String,
    pub variables: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HistoryEntryData {
    pub id: String,
    pub method: String,
    pub url: String,
    pub status: Option<i32>,
    pub duration_ms: Option<i32>,
    pub size_bytes: Option<i32>,
    pub request_headers: Option<String>,
    pub request_body: Option<String>,
    pub response_headers: Option<String>,
    pub response_body: Option<String>,
    pub timestamp: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ModelData {
    pub id: String,
    pub name: String,
    pub description: String,
    pub fields: String,
}

// ─── Helper ───────────────────────────────────────────────

fn now() -> String {
    jiff::Timestamp::now()
        .strftime("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string()
}

fn gen_id() -> String {
    Uuid::new_v4().to_string()
}

// ─── Collections ─────────────────────────────────────────

#[tauri::command]
pub fn db_get_collections(db: State<'_, Database>) -> Result<Vec<CollectionData>, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare("SELECT id, name, parent_id, sort_order FROM collections ORDER BY sort_order")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok(CollectionData {
                id: row.get(0)?,
                name: row.get(1)?,
                parent_id: row.get(2)?,
                sort_order: row.get(3)?,
            })
        })
        .map_err(|e| e.to_string())?;
    let mut collections = Vec::new();
    for row in rows {
        collections.push(row.map_err(|e| e.to_string())?);
    }
    Ok(collections)
}

#[tauri::command]
pub fn db_create_collection(
    db: State<'_, Database>,
    name: String,
    parent_id: Option<String>,
) -> Result<CollectionData, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let id = gen_id();
    let ts = now();
    conn.execute(
        "INSERT INTO collections (id, name, parent_id, sort_order, created_at, updated_at) VALUES (?1, ?2, ?3, 0, ?4, ?5)",
        rusqlite::params![id, name, parent_id, ts, ts],
    ).map_err(|e| e.to_string())?;
    Ok(CollectionData {
        id,
        name,
        parent_id,
        sort_order: 0,
    })
}

#[tauri::command]
pub fn db_update_collection(
    db: State<'_, Database>,
    id: String,
    name: String,
) -> Result<CollectionData, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let ts = now();
    conn.execute(
        "UPDATE collections SET name = ?1, updated_at = ?2 WHERE id = ?3",
        rusqlite::params![name, ts, id],
    )
    .map_err(|e| e.to_string())?;
    Ok(CollectionData {
        id,
        name,
        parent_id: None,
        sort_order: 0,
    })
}

#[tauri::command]
pub fn db_delete_collection(db: State<'_, Database>, id: String) -> Result<bool, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "DELETE FROM requests WHERE collection_id = ?1",
        rusqlite::params![id],
    )
    .map_err(|e| e.to_string())?;
    conn.execute(
        "DELETE FROM collections WHERE id = ?1",
        rusqlite::params![id],
    )
    .map_err(|e| e.to_string())?;
    Ok(true)
}

// ─── Requests ────────────────────────────────────────────

#[tauri::command]
pub fn db_get_requests(
    db: State<'_, Database>,
    collection_id: String,
) -> Result<Vec<RequestData>, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT id, collection_id, name, protocol, method, url, headers, body, content_type, \
             cookies, request_body_model_id, responses, sort_order FROM requests WHERE collection_id = ?1 ORDER BY sort_order",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(rusqlite::params![collection_id], |row| {
            Ok(RequestData {
                id: row.get(0)?,
                collection_id: row.get(1)?,
                name: row.get(2)?,
                protocol: row.get(3)?,
                method: row.get(4)?,
                url: row.get(5)?,
                headers: row.get(6)?,
                body: row.get(7)?,
                content_type: row.get(8)?,
                cookies: row.get(9)?,
                request_body_model_id: row.get(10)?,
                responses: row.get(11)?,
                sort_order: row.get(12)?,
            })
        })
        .map_err(|e| e.to_string())?;
    let mut requests = Vec::new();
    for row in rows {
        requests.push(row.map_err(|e| e.to_string())?);
    }
    Ok(requests)
}

#[tauri::command]
pub fn db_create_request(
    db: State<'_, Database>,
    collection_id: String,
    name: Option<String>,
) -> Result<RequestData, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let id = gen_id();
    let ts = now();
    let req_name = name.unwrap_or_else(|| "Untitled".into());
    conn.execute(
        "INSERT INTO requests (id, collection_id, name, protocol, method, url, headers, body, content_type, \
         cookies, responses, sort_order, created_at, updated_at) VALUES (?1,?2,?3,'http','GET','','{}','','application/json','[]','[]',0,?4,?5)",
        rusqlite::params![id, collection_id, req_name, ts, ts],
    ).map_err(|e| e.to_string())?;
    Ok(RequestData {
        id,
        collection_id,
        name: req_name,
        protocol: "http".into(),
        method: "GET".into(),
        url: String::new(),
        headers: "{}".into(),
        body: String::new(),
        content_type: "application/json".into(),
        cookies: "[]".into(),
        request_body_model_id: None,
        responses: "[]".into(),
        sort_order: 0,
    })
}

#[tauri::command]
#[allow(clippy::too_many_arguments)] // Tauri command signature: fields map one-to-one to frontend args; a struct would break the invoke protocol
pub fn db_update_request(
    db: State<'_, Database>,
    id: String,
    name: Option<String>,
    method: Option<String>,
    url: Option<String>,
    headers: Option<String>,
    body: Option<String>,
    content_type: Option<String>,
    request_body_model_id: Option<String>,
) -> Result<bool, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let ts = now();

    if let Some(name) = name {
        conn.execute(
            "UPDATE requests SET name = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![name, ts, id],
        )
        .map_err(|e| e.to_string())?;
    }
    if let Some(method) = method {
        conn.execute(
            "UPDATE requests SET method = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![method, ts, id],
        )
        .map_err(|e| e.to_string())?;
    }
    if let Some(url) = url {
        conn.execute(
            "UPDATE requests SET url = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![url, ts, id],
        )
        .map_err(|e| e.to_string())?;
    }
    if let Some(headers) = headers {
        conn.execute(
            "UPDATE requests SET headers = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![headers, ts, id],
        )
        .map_err(|e| e.to_string())?;
    }
    if let Some(body) = body {
        conn.execute(
            "UPDATE requests SET body = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![body, ts, id],
        )
        .map_err(|e| e.to_string())?;
    }
    if let Some(content_type) = content_type {
        conn.execute(
            "UPDATE requests SET content_type = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![content_type, ts, id],
        )
        .map_err(|e| e.to_string())?;
    }
    if let Some(model_id) = request_body_model_id {
        conn.execute(
            "UPDATE requests SET request_body_model_id = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![model_id, ts, id],
        )
        .map_err(|e| e.to_string())?;
    }

    Ok(true)
}

#[tauri::command]
pub fn db_delete_request(db: State<'_, Database>, id: String) -> Result<bool, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM requests WHERE id = ?1", rusqlite::params![id])
        .map_err(|e| e.to_string())?;
    Ok(true)
}

// ─── Environments ────────────────────────────────────────

#[tauri::command]
pub fn db_get_environments(db: State<'_, Database>) -> Result<Vec<EnvironmentData>, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare("SELECT id, name, variables FROM environments ORDER BY name")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok(EnvironmentData {
                id: row.get(0)?,
                name: row.get(1)?,
                variables: row.get(2)?,
            })
        })
        .map_err(|e| e.to_string())?;
    let mut envs = Vec::new();
    for row in rows {
        envs.push(row.map_err(|e| e.to_string())?);
    }
    Ok(envs)
}

#[tauri::command]
pub fn db_create_environment(
    db: State<'_, Database>,
    name: String,
) -> Result<EnvironmentData, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let id = gen_id();
    let ts = now();
    conn.execute(
        "INSERT INTO environments (id, name, variables, created_at, updated_at) VALUES (?1,?2,'{}',?3,?4)",
        rusqlite::params![id, name, ts, ts],
    ).map_err(|e| e.to_string())?;
    Ok(EnvironmentData {
        id,
        name,
        variables: "{}".into(),
    })
}

#[tauri::command]
pub fn db_update_environment(
    db: State<'_, Database>,
    id: String,
    name: Option<String>,
    variables: Option<String>,
) -> Result<bool, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let ts = now();
    if let Some(name) = name {
        conn.execute(
            "UPDATE environments SET name = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![name, ts, id],
        )
        .map_err(|e| e.to_string())?;
    }
    if let Some(variables) = variables {
        conn.execute(
            "UPDATE environments SET variables = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![variables, ts, id],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(true)
}

#[tauri::command]
pub fn db_delete_environment(db: State<'_, Database>, id: String) -> Result<bool, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "DELETE FROM environments WHERE id = ?1",
        rusqlite::params![id],
    )
    .map_err(|e| e.to_string())?;
    Ok(true)
}

// ─── History ─────────────────────────────────────────────

#[tauri::command]
pub fn db_get_history(
    db: State<'_, Database>,
    limit: Option<i32>,
    offset: Option<i32>,
) -> Result<Vec<HistoryEntryData>, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let limit = limit.unwrap_or(100);
    let offset = offset.unwrap_or(0);
    let mut stmt = conn.prepare(
        "SELECT id, method, url, status, duration_ms, size_bytes, request_headers, request_body, \
         response_headers, response_body, timestamp FROM history ORDER BY timestamp DESC LIMIT ?1 OFFSET ?2",
    ).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(rusqlite::params![limit, offset], |row| {
            Ok(HistoryEntryData {
                id: row.get(0)?,
                method: row.get(1)?,
                url: row.get(2)?,
                status: row.get(3)?,
                duration_ms: row.get(4)?,
                size_bytes: row.get(5)?,
                request_headers: row.get(6)?,
                request_body: row.get(7)?,
                response_headers: row.get(8)?,
                response_body: row.get(9)?,
                timestamp: row.get(10)?,
            })
        })
        .map_err(|e| e.to_string())?;
    let mut entries = Vec::new();
    for row in rows {
        entries.push(row.map_err(|e| e.to_string())?);
    }
    Ok(entries)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)] // Tauri command signature: fields map one-to-one to frontend args; a struct would break the invoke protocol
pub fn db_add_history_entry(
    db: State<'_, Database>,
    method: String,
    url: String,
    status: Option<i32>,
    duration_ms: Option<i32>,
    size_bytes: Option<i32>,
    request_headers: Option<String>,
    request_body: Option<String>,
    response_headers: Option<String>,
    response_body: Option<String>,
) -> Result<HistoryEntryData, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let id = gen_id();
    let ts = now();
    conn.execute(
        "INSERT INTO history (id, method, url, status, duration_ms, size_bytes, request_headers, request_body, \
         response_headers, response_body, timestamp) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        rusqlite::params![id, method, url, status, duration_ms, size_bytes, request_headers, request_body,
            response_headers, response_body, ts],
    ).map_err(|e| e.to_string())?;
    Ok(HistoryEntryData {
        id,
        method,
        url,
        status,
        duration_ms,
        size_bytes,
        request_headers,
        request_body,
        response_headers,
        response_body,
        timestamp: ts,
    })
}

#[tauri::command]
pub fn db_clear_history(db: State<'_, Database>) -> Result<bool, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM history", [])
        .map_err(|e| e.to_string())?;
    Ok(true)
}

#[tauri::command]
pub fn db_delete_history_entry(db: State<'_, Database>, id: String) -> Result<bool, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM history WHERE id = ?1", rusqlite::params![id])
        .map_err(|e| e.to_string())?;
    Ok(true)
}

// ─── Data Models ─────────────────────────────────────────

#[tauri::command]
pub fn db_get_models(db: State<'_, Database>) -> Result<Vec<ModelData>, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare("SELECT id, name, description, fields FROM models ORDER BY name")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok(ModelData {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                fields: row.get(3)?,
            })
        })
        .map_err(|e| e.to_string())?;
    let mut models = Vec::new();
    for row in rows {
        models.push(row.map_err(|e| e.to_string())?);
    }
    Ok(models)
}

#[tauri::command]
pub fn db_create_model(db: State<'_, Database>, name: String) -> Result<ModelData, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let id = gen_id();
    let ts = now();
    conn.execute(
        "INSERT INTO models (id, name, description, fields, created_at, updated_at) VALUES (?1,?2,'','[]',?3,?4)",
        rusqlite::params![id, name, ts, ts],
    ).map_err(|e| e.to_string())?;
    Ok(ModelData {
        id,
        name,
        description: String::new(),
        fields: "[]".into(),
    })
}

#[tauri::command]
pub fn db_update_model(
    db: State<'_, Database>,
    id: String,
    name: Option<String>,
    description: Option<String>,
    fields: Option<String>,
) -> Result<bool, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let ts = now();
    if let Some(name) = name {
        conn.execute(
            "UPDATE models SET name = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![name, ts, id],
        )
        .map_err(|e| e.to_string())?;
    }
    if let Some(description) = description {
        conn.execute(
            "UPDATE models SET description = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![description, ts, id],
        )
        .map_err(|e| e.to_string())?;
    }
    if let Some(fields) = fields {
        conn.execute(
            "UPDATE models SET fields = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![fields, ts, id],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(true)
}

#[tauri::command]
pub fn db_delete_model(db: State<'_, Database>, id: String) -> Result<bool, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM models WHERE id = ?1", rusqlite::params![id])
        .map_err(|e| e.to_string())?;
    Ok(true)
}
