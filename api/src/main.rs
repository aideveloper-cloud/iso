// iso-api — API หลัก (Rust + Axum)
//   จัดการเอกสาร ISO: ทะเบียน, เวอร์ชัน+ลายเซ็น (SQLite), เนื้อหา (xlsx/docx/pdf), บันทึกเวอร์ชัน,
//   สรุปแดชบอร์ด, คลังภาพรวมองค์กร  ·  แปลง docx->HTML โดยเรียก Go converter
use axum::{
    body::Body,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use rusqlite::{params, Connection};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{collections::HashMap, fs, net::TcpStream, path::PathBuf, sync::Arc, sync::Mutex, time::Duration};

struct AppState {
    manifest: Vec<Value>,
    by_code: HashMap<String, Value>,
    overview: Value,
    docs_root: PathBuf,
    storage: PathBuf,
    conv_port: String,
    db: Mutex<Connection>,
}
type S = Arc<AppState>;

// ---------- helpers ----------
fn ext_of(d: &Value) -> String { d["ext"].as_str().unwrap_or("").to_lowercase() }
fn editable(e: &str) -> bool { e == "xls" || e == "xlsx" }
fn previewable(e: &str) -> bool {
    matches!(e, "pdf" | "jpg" | "jpeg" | "png" | "docx" | "xls" | "xlsx")
}
fn mime_of(e: &str) -> &'static str {
    match e {
        "pdf" => "application/pdf",
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "xls" => "application/vnd.ms-excel",
        "doc" => "application/msword",
        _ => "application/octet-stream",
    }
}
fn now_iso() -> String { chrono::Utc::now().to_rfc3339() }
fn err(c: StatusCode, m: &str) -> Response { (c, Json(json!({ "error": m }))).into_response() }
fn pct(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            o.push(b as char)
        } else {
            o.push_str(&format!("%{:02X}", b))
        }
    }
    o
}

// ---------- database ----------
fn open_db(path: &PathBuf) -> Connection {
    let c = Connection::open(path).expect("open db");
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS versions(
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            doc_code TEXT NOT NULL, version INTEGER NOT NULL,
            filename TEXT NOT NULL, ext TEXT NOT NULL,
            stored_name TEXT, origin_path TEXT, size INTEGER NOT NULL DEFAULT 0,
            source TEXT NOT NULL, note TEXT NOT NULL DEFAULT '',
            signer_name TEXT NOT NULL, signer_role TEXT NOT NULL DEFAULT '',
            signed_at TEXT NOT NULL, created_at TEXT NOT NULL,
            UNIQUE(doc_code, version));
         CREATE INDEX IF NOT EXISTS idx_v_doc ON versions(doc_code);",
    )
    .unwrap();
    c
}

fn ensure_original(st: &S, code: &str) {
    let doc = match st.by_code.get(code) { Some(d) => d.clone(), None => return };
    let db = st.db.lock().unwrap();
    let maxv: i64 = db
        .query_row("SELECT COALESCE(MAX(version),0) FROM versions WHERE doc_code=?1", [code], |r| r.get(0))
        .unwrap_or(0);
    if maxv > 0 { return; }
    let rel = doc["path"].as_str().unwrap_or("");
    let size = fs::metadata(st.docs_root.join(rel)).map(|m| m.len() as i64).unwrap_or(0);
    let now = now_iso();
    let _ = db.execute(
        "INSERT INTO versions(doc_code,version,filename,ext,stored_name,origin_path,size,source,note,signer_name,signer_role,signed_at,created_at)
         VALUES(?1,1,?2,?3,NULL,?4,?5,'original','เวอร์ชันต้นฉบับจากแฟ้มเอกสาร','ระบบ (ต้นฉบับ)','',?6,?6)",
        params![code, doc["file"].as_str().unwrap_or(""), ext_of(&doc), rel, size, now],
    );
}

fn row_to_json(r: &rusqlite::Row) -> rusqlite::Result<Value> {
    Ok(json!({
        "version": r.get::<_, i64>(0)?, "filename": r.get::<_, String>(1)?, "ext": r.get::<_, String>(2)?,
        "storedName": r.get::<_, Option<String>>(3)?, "originPath": r.get::<_, Option<String>>(4)?,
        "size": r.get::<_, i64>(5)?, "source": r.get::<_, String>(6)?, "note": r.get::<_, String>(7)?,
        "signerName": r.get::<_, String>(8)?, "signerRole": r.get::<_, String>(9)?,
        "signedAt": r.get::<_, String>(10)?, "createdAt": r.get::<_, String>(11)?,
    }))
}
const VCOLS: &str = "version,filename,ext,stored_name,origin_path,size,source,note,signer_name,signer_role,signed_at,created_at";

fn list_versions(st: &S, code: &str) -> Vec<Value> {
    let db = st.db.lock().unwrap();
    let mut stmt = db
        .prepare(&format!("SELECT {VCOLS} FROM versions WHERE doc_code=?1 ORDER BY version DESC"))
        .unwrap();
    let rows = stmt.query_map([code], |r| row_to_json(r)).unwrap();
    rows.filter_map(|x| x.ok()).collect()
}

fn counts_by_doc(st: &S) -> HashMap<String, (i64, i64)> {
    let db = st.db.lock().unwrap();
    let mut stmt = db.prepare("SELECT doc_code,MAX(version),COUNT(*) FROM versions GROUP BY doc_code").unwrap();
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?)))
        .unwrap();
    let mut m = HashMap::new();
    for r in rows.flatten() { m.insert(r.0, (r.1, r.2)); }
    m
}

// (version, absolute path, ext, filename)
fn resolve(st: &S, code: &str, v: Option<&String>) -> Result<(i64, PathBuf, String, String), Response> {
    if !st.by_code.contains_key(code) { return Err(err(StatusCode::NOT_FOUND, "ไม่พบเอกสาร")); }
    ensure_original(st, code);
    let db = st.db.lock().unwrap();
    let mapper = |r: &rusqlite::Row| {
        Ok((
            r.get::<_, i64>(0)?, r.get::<_, String>(1)?,
            r.get::<_, Option<String>>(2)?, r.get::<_, Option<String>>(3)?, r.get::<_, String>(4)?,
        ))
    };
    let res = if let Some(vv) = v {
        let vn: i64 = vv.parse().unwrap_or(0);
        db.query_row("SELECT version,ext,stored_name,origin_path,filename FROM versions WHERE doc_code=?1 AND version=?2", params![code, vn], mapper)
    } else {
        db.query_row("SELECT version,ext,stored_name,origin_path,filename FROM versions WHERE doc_code=?1 ORDER BY version DESC LIMIT 1", params![code], mapper)
    };
    match res {
        Ok((ver, ext, stored, origin, filename)) => {
            let abs = match stored {
                Some(s) => st.storage.join(s),
                None => st.docs_root.join(origin.unwrap_or_default()),
            };
            if !abs.exists() { return Err(err(StatusCode::NOT_FOUND, "ไม่พบไฟล์ในระบบ")); }
            Ok((ver, abs, ext, filename))
        }
        Err(_) => Err(err(StatusCode::NOT_FOUND, "ไม่พบเวอร์ชัน")),
    }
}

// ---------- เรียก Go converter (HTTP ง่าย ๆ ผ่าน TCP) ----------
async fn call_converter(port: &str, path_route: &str, body: &str) -> Result<Value, String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut s = tokio::net::TcpStream::connect(format!("127.0.0.1:{port}")).await.map_err(|e| e.to_string())?;
    let req = format!(
        "POST {path_route} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.as_bytes().len()
    );
    s.write_all(req.as_bytes()).await.map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).await.map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&buf);
    let idx = text.find("\r\n\r\n").ok_or("bad response")?;
    serde_json::from_str(&text[idx + 4..]).map_err(|e| format!("parse: {e}"))
}

// ---------- handlers ----------
async fn health(State(st): State<S>) -> Json<Value> {
    let up = "127.0.0.1:".to_string() + &st.conv_port;
    let conv = up
        .parse()
        .ok()
        .and_then(|a| TcpStream::connect_timeout(&a, Duration::from_millis(400)).ok())
        .is_some();
    Json(json!({ "service": "iso-api (Rust/Axum)", "api": "ok", "converter": if conv {"ok"} else {"down"} }))
}

async fn docs_list(State(st): State<S>) -> Json<Value> {
    let counts = counts_by_doc(&st);
    let arr: Vec<Value> = st
        .manifest
        .iter()
        .map(|d| {
            let code = d["code"].as_str().unwrap_or("");
            let e = ext_of(d);
            let (latest, count) = counts.get(code).cloned().unwrap_or((1, 1));
            let mut o = d.clone();
            o["editable"] = json!(editable(&e));
            o["previewable"] = json!(previewable(&e));
            o["latestVersion"] = json!(latest);
            o["versionCount"] = json!(count);
            o
        })
        .collect();
    Json(Value::Array(arr))
}

async fn versions(Path(code): Path<String>, State(st): State<S>) -> Response {
    let doc = match st.by_code.get(&code) { Some(d) => d.clone(), None => return err(StatusCode::NOT_FOUND, "ไม่พบเอกสาร") };
    ensure_original(&st, &code);
    let e = ext_of(&doc);
    Json(json!({
        "code": code, "name": doc["name"], "ext": e,
        "editable": editable(&e), "previewable": previewable(&e),
        "versions": list_versions(&st, &code),
    }))
    .into_response()
}

async fn content(Path(code): Path<String>, Query(q): Query<HashMap<String, String>>, State(st): State<S>) -> Response {
    let (ver, abs, ext, _fn) = match resolve(&st, &code, q.get("v")) { Ok(x) => x, Err(r) => return r };
    if ext == "xls" || ext == "xlsx" {
        match fs::read(&abs) {
            Ok(b) => Json(json!({ "kind": "sheet", "version": ver, "ext": ext, "data": B64.encode(b) })).into_response(),
            Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, &format!("อ่านไฟล์ไม่ได้: {e}")),
        }
    } else if ext == "docx" {
        let body = json!({ "path": abs.to_string_lossy(), "ext": "docx" }).to_string();
        match call_converter(&st.conv_port, "/convert", &body).await {
            Ok(v) if v["ok"] == json!(true) => Json(json!({ "kind": "html", "version": ver, "ext": ext, "html": v["html"] })).into_response(),
            Ok(v) => Json(json!({ "kind": "download", "version": ver, "ext": ext, "reason": format!("แปลงเอกสารไม่สำเร็จ: {}", v["error"]) })).into_response(),
            Err(e) => Json(json!({ "kind": "download", "version": ver, "ext": ext, "reason": format!("converter ไม่ตอบสนอง: {e}") })).into_response(),
        }
    } else if matches!(ext.as_str(), "pdf" | "jpg" | "jpeg" | "png") {
        Json(json!({ "kind": "embed", "version": ver, "ext": ext })).into_response()
    } else {
        Json(json!({ "kind": "download", "version": ver, "ext": ext, "reason": "ไฟล์ชนิดนี้เปิดอ่านบนหน้าเว็บไม่ได้" })).into_response()
    }
}

async fn file(Path(code): Path<String>, Query(q): Query<HashMap<String, String>>, State(st): State<S>) -> Response {
    let (_v, abs, ext, filename) = match resolve(&st, &code, q.get("v")) { Ok(x) => x, Err(r) => return r };
    let bytes = match fs::read(&abs) { Ok(b) => b, Err(_) => return err(StatusCode::NOT_FOUND, "อ่านไฟล์ไม่ได้") };
    let disp = if q.get("dl").map(|s| s == "1").unwrap_or(false) { "attachment" } else { "inline" };
    Response::builder()
        .header(header::CONTENT_TYPE, mime_of(&ext))
        .header(header::CONTENT_DISPOSITION, format!("{disp}; filename*=UTF-8''{}", pct(&filename)))
        .body(Body::from(bytes))
        .unwrap()
}

#[derive(Deserialize)]
#[allow(non_snake_case)]
struct SaveReq {
    dataBase64: Option<String>,
    filename: Option<String>,
    ext: Option<String>,
    source: Option<String>,
    note: Option<String>,
    signerName: Option<String>,
    signerRole: Option<String>,
}

async fn save(Path(code): Path<String>, State(st): State<S>, Json(b): Json<SaveReq>) -> Response {
    let doc = match st.by_code.get(&code) { Some(d) => d.clone(), None => return err(StatusCode::NOT_FOUND, "ไม่พบเอกสาร") };
    ensure_original(&st, &code);
    let name = b.signerName.unwrap_or_default();
    if name.trim().is_empty() { return err(StatusCode::BAD_REQUEST, "ต้องลงชื่อผู้แก้ไขก่อนบันทึก"); }
    let data = match b.dataBase64 { Some(d) if !d.is_empty() => d, _ => return err(StatusCode::BAD_REQUEST, "ไม่พบข้อมูลไฟล์") };
    let bytes = match B64.decode(data.as_bytes()) { Ok(x) => x, Err(_) => return err(StatusCode::BAD_REQUEST, "ข้อมูลไฟล์ไม่ถูกต้อง") };
    if bytes.is_empty() { return err(StatusCode::BAD_REQUEST, "ไฟล์ว่าง"); }
    if bytes.len() > 40 * 1024 * 1024 { return err(StatusCode::PAYLOAD_TOO_LARGE, "ไฟล์ใหญ่เกิน 40 MB"); }
    let safe_ext: String = b
        .ext
        .unwrap_or_else(|| ext_of(&doc))
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    let filename = b.filename.unwrap_or_else(|| doc["file"].as_str().unwrap_or("").to_string());
    let source = if b.source.as_deref() == Some("edit") { "edit" } else { "upload" };
    let note = b.note.unwrap_or_default();
    let role = b.signerRole.unwrap_or_default();

    let db = st.db.lock().unwrap();
    let version: i64 = db.query_row("SELECT COALESCE(MAX(version),0) FROM versions WHERE doc_code=?1", [&code], |r| r.get(0)).unwrap_or(0) + 1;
    let stored: String = format!("{}__v{}.{}", code.replace(|c: char| !c.is_alphanumeric() && c != '-' && c != '_' && c != '.', "_"), version, safe_ext);
    if let Err(e) = fs::write(st.storage.join(&stored), &bytes) {
        return err(StatusCode::INTERNAL_SERVER_ERROR, &format!("บันทึกไฟล์ไม่ได้: {e}"));
    }
    let now = now_iso();
    let _ = db.execute(
        "INSERT INTO versions(doc_code,version,filename,ext,stored_name,origin_path,size,source,note,signer_name,signer_role,signed_at,created_at)
         VALUES(?1,?2,?3,?4,?5,NULL,?6,?7,?8,?9,?10,?11,?11)",
        params![code, version, filename, safe_ext, stored, bytes.len() as i64, source, note.trim(), name.trim(), role.trim(), now],
    );
    let v = db.query_row(&format!("SELECT {VCOLS} FROM versions WHERE doc_code=?1 AND version=?2"), params![code, version], |r| row_to_json(r)).unwrap();
    Json(json!({ "ok": true, "version": v })).into_response()
}

async fn stats(State(st): State<S>) -> Json<Value> {
    let counts = counts_by_doc(&st);
    let (mut by_type, mut by_ext) = (serde_json::Map::new(), serde_json::Map::new());
    let mut by_dept: HashMap<String, (i64, i64, i64)> = HashMap::new(); // total, revised, versions
    for d in &st.manifest {
        let t = d["type"].as_str().unwrap_or("").to_string();
        *by_type.entry(t).or_insert(json!(0)) = json!(by_type.get(d["type"].as_str().unwrap_or("")).and_then(|x| x.as_i64()).unwrap_or(0) + 1);
        let e = ext_of(d);
        *by_ext.entry(e.clone()).or_insert(json!(0)) = json!(by_ext.get(&e).and_then(|x| x.as_i64()).unwrap_or(0) + 1);
        let dept = d["deptName"].as_str().unwrap_or("").to_string();
        let ent = by_dept.entry(dept).or_insert((0, 0, 0));
        ent.0 += 1;
        if let Some((latest, _)) = counts.get(d["code"].as_str().unwrap_or("")) {
            if *latest > 1 { ent.1 += 1; ent.2 += latest - 1; }
        }
    }
    let by_dept_json: serde_json::Map<String, Value> = by_dept
        .into_iter()
        .map(|(k, v)| (k, json!({ "total": v.0, "revised": v.1, "versions": v.2 })))
        .collect();
    let revised_docs = counts.values().filter(|(l, _)| *l > 1).count();

    // signers + recent (จากเวอร์ชันที่ไม่ใช่ original)
    let all: Vec<(String, String, String, String, String, String, i64)> = {
        let db = st.db.lock().unwrap();
        let mut stmt = db.prepare("SELECT signer_name,signer_role,signed_at,doc_code,note,source,version FROM versions WHERE source!='original' ORDER BY created_at DESC").unwrap();
        let v = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)))
            .unwrap()
            .flatten()
            .collect();
        v
    };
    let total_edits = all.len();
    let mut signers: HashMap<String, (String, i64, String)> = HashMap::new();
    for (nm, role, at, ..) in &all {
        let e = signers.entry(nm.clone()).or_insert((role.clone(), 0, at.clone()));
        e.1 += 1;
        if at > &e.2 { e.2 = at.clone(); }
    }
    let mut signers_v: Vec<Value> = signers.iter().map(|(k, v)| json!({ "name": k, "role": v.0, "n": v.1, "last": v.2 })).collect();
    signers_v.sort_by(|a, b| b["n"].as_i64().cmp(&a["n"].as_i64()));
    let recent: Vec<Value> = all.iter().take(15).map(|(nm, role, at, code, note, source, ver)| {
        let dn = st.by_code.get(code).and_then(|d| d["name"].as_str()).unwrap_or(code);
        let dept = st.by_code.get(code).and_then(|d| d["deptName"].as_str()).unwrap_or("");
        json!({ "signerName": nm, "signerRole": role, "signedAt": at, "code": code, "docName": dn, "dept": dept, "note": note, "source": source, "version": ver })
    }).collect();

    Json(json!({
        "totalDocs": st.manifest.len(),
        "revisedDocs": revised_docs,
        "totalEdits": total_edits,
        "editable": st.manifest.iter().filter(|d| editable(&ext_of(d))).count(),
        "previewable": st.manifest.iter().filter(|d| previewable(&ext_of(d))).count(),
        "unregistered": st.manifest.iter().filter(|d| d["unreg"].as_bool().unwrap_or(false)).count(),
        "duplicates": st.manifest.iter().filter(|d| d["dup"].as_i64().unwrap_or(0) > 1).count(),
        "byType": by_type, "byDept": by_dept_json, "byExt": by_ext,
        "signers": signers_v, "recent": recent,
    }))
}

async fn overview(State(st): State<S>) -> Json<Value> { Json(st.overview.clone()) }

async fn overview_file(Query(q): Query<HashMap<String, String>>, State(st): State<S>) -> Response {
    let rel = q.get("path").cloned().unwrap_or_default();
    let abs = st.docs_root.join(&rel);
    // กันหลุดออกนอก docs_root
    let base = st.docs_root.to_string_lossy().replace('\\', "/");
    let target = abs.to_string_lossy().replace('\\', "/");
    if !target.starts_with(&base) || !abs.exists() { return err(StatusCode::NOT_FOUND, "ไม่พบไฟล์"); }
    let ext = abs.extension().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
    let bytes = match fs::read(&abs) { Ok(b) => b, Err(_) => return err(StatusCode::NOT_FOUND, "อ่านไฟล์ไม่ได้") };
    let name = abs.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let disp = if q.get("dl").map(|s| s == "1").unwrap_or(false) { "attachment" } else { "inline" };
    Response::builder()
        .header(header::CONTENT_TYPE, mime_of(&ext))
        .header(header::CONTENT_DISPOSITION, format!("{disp}; filename*=UTF-8''{}", pct(&name)))
        .body(Body::from(bytes))
        .unwrap()
}

#[tokio::main]
async fn main() {
    let base = std::env::current_dir().unwrap();
    let docs_root = base.join(std::env::var("DOCS_ROOT").unwrap_or_else(|_| "../data".into()));
    let storage = docs_root.join("storage");
    fs::create_dir_all(&storage).ok();
    let manifest: Vec<Value> = serde_json::from_str(&fs::read_to_string(docs_root.join("manifest.json")).expect("manifest.json")).expect("parse manifest");
    let overview_json: Value = fs::read_to_string(docs_root.join("overview.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(json!([]));
    let by_code: HashMap<String, Value> = manifest.iter().filter_map(|d| d["code"].as_str().map(|c| (c.to_string(), d.clone()))).collect();
    let db = open_db(&docs_root.join("versions.db"));

    let state: S = Arc::new(AppState {
        manifest, by_code, overview: overview_json, docs_root, storage,
        conv_port: std::env::var("CONVERTER_PORT").unwrap_or_else(|_| "8081".into()),
        db: Mutex::new(db),
    });

    let app = Router::new()
        .route("/api/health", get(health))
        .route("/api/docs", get(docs_list))
        .route("/api/docs/:code/versions", get(versions).post(save))
        .route("/api/docs/:code/content", get(content))
        .route("/api/docs/:code/file", get(file))
        .route("/api/stats", get(stats))
        .route("/api/overview", get(overview))
        .route("/api/overview/file", get(overview_file))
        .layer(DefaultBodyLimit::max(64 * 1024 * 1024))
        .with_state(state);

    let port = std::env::var("API_PORT").unwrap_or_else(|_| "8080".into());
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await.unwrap();
    println!("\n  ✅ iso-api (Rust/Axum) พร้อมที่ http://localhost:{port}\n");
    axum::serve(listener, app).await.unwrap();
}
