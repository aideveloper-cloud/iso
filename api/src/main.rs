// iso-api — API หลัก (Rust + Axum)
//   จัดการเอกสาร ISO: ทะเบียน, เวอร์ชัน+ลายเซ็น (PostgreSQL), เนื้อหา (xlsx/docx/pdf), บันทึกเวอร์ชัน,
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
use deadpool_postgres::{ManagerConfig, Pool, RecyclingMethod};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{collections::HashMap, fs, net::TcpStream, path::PathBuf, sync::Arc, time::Duration};
use tokio_postgres::NoTls;

struct AppState {
    manifest: Vec<Value>,
    by_code: HashMap<String, Value>,
    overview: Value,
    docs_root: PathBuf,
    storage: PathBuf,
    conv_port: String,
    db: Pool,
}
type S = Arc<AppState>;

// ---------- helpers ----------
fn ext_of(d: &Value) -> String { d["ext"].as_str().unwrap_or("").to_lowercase() }
fn editable(e: &str) -> bool { matches!(e, "xls" | "xlsx" | "docx") }
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

/// ชื่อไฟล์ใน storage ที่ปลอดภัย (ตัดอักขระพิเศษ)
fn safe_stem(s: &str) -> String {
    s.replace(|c: char| !c.is_alphanumeric() && c != '-' && c != '_' && c != '.', "_")
}

/// ถอดรหัส base64 จากคำขออัปโหลด/อัปเดท — ตรวจว่างและขนาด ≤ 40 MB
fn decode_upload_bytes(data: Option<String>) -> Result<Vec<u8>, Response> {
    let data = match data {
        Some(d) if !d.is_empty() => d,
        _ => return Err(err(StatusCode::BAD_REQUEST, "ไม่พบข้อมูลไฟล์")),
    };
    let bytes = match B64.decode(data.as_bytes()) {
        Ok(x) => x,
        Err(_) => return Err(err(StatusCode::BAD_REQUEST, "ข้อมูลไฟล์ไม่ถูกต้อง")),
    };
    if bytes.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "ไฟล์ว่าง"));
    }
    if bytes.len() > 40 * 1024 * 1024 {
        return Err(err(StatusCode::PAYLOAD_TOO_LARGE, "ไฟล์ใหญ่เกิน 40 MB"));
    }
    Ok(bytes)
}

fn require_signer(name: Option<String>, empty_msg: &str) -> Result<String, Response> {
    let name = name.unwrap_or_default();
    if name.trim().is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, empty_msg));
    }
    Ok(name.trim().to_string())
}

fn norm_path(p: &str) -> String {
    p.replace('\\', "/")
}

/// รหัสเอกสารสังเคราะห์สำหรับไฟล์ภาพรวมที่ยังไม่ขึ้นทะเบียน (เก็บเวอร์ชันในระบบ)
fn ov_doc_code(rel: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    rel.hash(&mut h);
    format!("OV:{:016x}", h.finish())
}

fn find_code_by_path(st: &S, rel: &str) -> Option<String> {
    let want = norm_path(rel);
    st.by_code.iter().find_map(|(code, d)| {
        let path = d["path"].as_str().map(norm_path)?;
        (path == want).then(|| code.clone())
    })
}

async fn version_exists(st: &S, code: &str) -> bool {
    let Ok(client) = st.db.get().await else { return false };
    client
        .query_one("SELECT COUNT(*) FROM versions WHERE doc_code=$1", &[&code])
        .await
        .map(|r| r.get::<_, i64>(0) > 0)
        .unwrap_or(false)
}
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

// ---------- database (PostgreSQL) ----------
const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS versions(
        id BIGSERIAL PRIMARY KEY,
        doc_code TEXT NOT NULL, version BIGINT NOT NULL,
        filename TEXT NOT NULL, ext TEXT NOT NULL,
        stored_name TEXT, origin_path TEXT, size BIGINT NOT NULL DEFAULT 0,
        source TEXT NOT NULL, note TEXT NOT NULL DEFAULT '',
        signer_name TEXT NOT NULL, signer_role TEXT NOT NULL DEFAULT '',
        signed_at TEXT NOT NULL, created_at TEXT NOT NULL,
        UNIQUE(doc_code, version));
     CREATE INDEX IF NOT EXISTS idx_v_doc ON versions(doc_code);
     CREATE TABLE IF NOT EXISTS actions(
        id BIGSERIAL PRIMARY KEY,
        ref_no TEXT NOT NULL,
        kind TEXT NOT NULL,
        title TEXT NOT NULL,
        detail TEXT NOT NULL DEFAULT '',
        dept TEXT NOT NULL DEFAULT '',
        doc_code TEXT NOT NULL DEFAULT '',
        raised_by TEXT NOT NULL DEFAULT '',
        assignee TEXT NOT NULL DEFAULT '',
        priority TEXT NOT NULL DEFAULT 'medium',
        status TEXT NOT NULL DEFAULT 'open',
        progress BIGINT NOT NULL DEFAULT 0,
        due_date TEXT NOT NULL DEFAULT '',
        action_taken TEXT NOT NULL DEFAULT '',
        created_at TEXT NOT NULL, updated_at TEXT NOT NULL, closed_at TEXT NOT NULL DEFAULT '');
     CREATE INDEX IF NOT EXISTS idx_a_kind ON actions(kind);";

const VCOLS: &str = "version,filename,ext,stored_name,origin_path,size,source,note,signer_name,signer_role,signed_at,created_at";
const ACOLS: &str = "id,ref_no,kind,title,detail,dept,doc_code,raised_by,assignee,priority,status,progress,due_date,action_taken,created_at,updated_at,closed_at";

fn make_pool() -> Pool {
    let url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "host=localhost port=5432 user=iso password=iso dbname=iso".into());
    let pg_config: tokio_postgres::Config = url.parse().expect("DATABASE_URL ไม่ถูกต้อง");
    let mgr = deadpool_postgres::Manager::from_config(
        pg_config,
        NoTls,
        ManagerConfig { recycling_method: RecyclingMethod::Fast },
    );
    Pool::builder(mgr).max_size(16).build().expect("สร้าง pool ไม่ได้")
}

// รอ Postgres พร้อม (docker อาจยังบูตไม่เสร็จ) แล้วสร้างตาราง
async fn init_db(pool: &Pool) {
    for attempt in 1..=30 {
        match pool.get().await {
            Ok(client) => {
                client.batch_execute(SCHEMA).await.expect("สร้างตารางไม่สำเร็จ");
                return;
            }
            Err(e) => {
                if attempt == 30 { panic!("เชื่อมต่อ PostgreSQL ไม่ได้: {e}"); }
                println!("  ⏳ รอ PostgreSQL... (ครั้งที่ {attempt})");
                tokio::time::sleep(Duration::from_millis(1000)).await;
            }
        }
    }
}

// migrate ข้อมูลเวอร์ชันเก่าจาก SQLite (versions.db) ครั้งเดียว ถ้าตาราง Postgres ยังว่าง
async fn migrate_from_sqlite(pool: &Pool, sqlite_path: &PathBuf) {
    if !sqlite_path.exists() { return; }
    let client = match pool.get().await { Ok(c) => c, Err(_) => return };
    let existing: i64 = client
        .query_one("SELECT COUNT(*) FROM versions", &[])
        .await
        .map(|r| r.get(0))
        .unwrap_or(0);
    if existing > 0 { return; } // มีข้อมูลแล้ว ไม่ต้อง migrate

    // อ่านทุกแถวจาก SQLite
    type Row = (String, i64, String, String, Option<String>, Option<String>, i64, String, String, String, String, String, String);
    let rows: Vec<Row> = match rusqlite::Connection::open(sqlite_path) {
        Ok(c) => {
            let mut stmt = match c.prepare(&format!("SELECT doc_code,{VCOLS} FROM versions")) {
                Ok(s) => s,
                Err(_) => return,
            };
            let it = stmt.query_map([], |r| {
                Ok((
                    r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?,
                    r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?, r.get(10)?, r.get(11)?, r.get(12)?,
                ))
            });
            match it { Ok(m) => m.filter_map(|x| x.ok()).collect(), Err(_) => return }
        }
        Err(_) => return,
    };
    if rows.is_empty() { return; }
    let n = rows.len();
    for r in rows {
        let _ = client.execute(
            "INSERT INTO versions(doc_code,version,filename,ext,stored_name,origin_path,size,source,note,signer_name,signer_role,signed_at,created_at)
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13) ON CONFLICT (doc_code,version) DO NOTHING",
            &[&r.0,&r.1,&r.2,&r.3,&r.4,&r.5,&r.6,&r.7,&r.8,&r.9,&r.10,&r.11,&r.12],
        ).await;
    }
    println!("  📦 migrate ข้อมูลจาก SQLite สำเร็จ: {n} เวอร์ชัน");
}

async fn ensure_original(st: &S, code: &str) {
    let Some(doc) = st.by_code.get(code) else { return };
    let rel = doc["path"].as_str().unwrap_or("");
    let filename = doc["file"].as_str().unwrap_or("");
    let ext = ext_of(doc);
    ensure_original_row(st, code, rel, filename, &ext).await;
}

async fn ensure_original_row(st: &S, code: &str, rel: &str, filename: &str, ext: &str) {
    let client = match st.db.get().await { Ok(c) => c, Err(_) => return };
    let maxv: i64 = client
        .query_one("SELECT COALESCE(MAX(version),0) FROM versions WHERE doc_code=$1", &[&code])
        .await
        .map(|r| r.get(0))
        .unwrap_or(0);
    if maxv > 0 { return; }
    let size = fs::metadata(st.docs_root.join(rel)).map(|m| m.len() as i64).unwrap_or(0);
    let now = now_iso();
    let _ = client.execute(
        "INSERT INTO versions(doc_code,version,filename,ext,stored_name,origin_path,size,source,note,signer_name,signer_role,signed_at,created_at)
         VALUES($1,1,$2,$3,NULL,$4,$5,'original','เวอร์ชันต้นฉบับจากแฟ้มเอกสาร','ระบบ (ต้นฉบับ)','',$6,$6)
         ON CONFLICT (doc_code,version) DO NOTHING",
        &[&code, &filename, &ext, &rel, &size, &now],
    ).await;
}

/// บันทึกไฟล์เป็นเวอร์ชันใหม่ใน storage + PostgreSQL
/// ถ้ามี `origin_sync` จะเขียนทับต้นฉบับด้วย (ให้เปิดจากดิสก์แล้วเห็นเวอร์ชันล่าสุด)
async fn save_new_version(
    st: &S,
    code: &str,
    bytes: &[u8],
    filename: &str,
    ext: &str,
    source: &str,
    note: &str,
    signer: &str,
    role: &str,
    origin_sync: Option<&std::path::Path>,
) -> Result<(i64, Value), Response> {
    let client = match st.db.get().await {
        Ok(c) => c,
        Err(_) => return Err(err(StatusCode::INTERNAL_SERVER_ERROR, "เชื่อมต่อฐานข้อมูลไม่ได้")),
    };
    let version: i64 = client
        .query_one("SELECT COALESCE(MAX(version),0) FROM versions WHERE doc_code=$1", &[&code])
        .await
        .map(|r| r.get::<_, i64>(0))
        .unwrap_or(0)
        + 1;
    let stored = format!("{}__v{}.{}", safe_stem(code), version, ext);
    if let Err(e) = fs::write(st.storage.join(&stored), bytes) {
        return Err(err(StatusCode::INTERNAL_SERVER_ERROR, &format!("บันทึกไฟล์ในระบบไม่ได้: {e}")));
    }
    if let Some(origin) = origin_sync {
        if let Some(parent) = origin.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Err(e) = fs::write(origin, bytes) {
            // เวอร์ชันในระบบบันทึกแล้ว — แจ้งแต่ไม่ rollback
            eprintln!("  ⚠ ซิงก์ต้นฉบับไม่ได้ ({}): {e}", origin.display());
        }
    }
    let now = now_iso();
    let size = bytes.len() as i64;
    if let Err(e) = client.execute(
        "INSERT INTO versions(doc_code,version,filename,ext,stored_name,origin_path,size,source,note,signer_name,signer_role,signed_at,created_at)
         VALUES($1,$2,$3,$4,$5,NULL,$6,$7,$8,$9,$10,$11,$11)",
        &[&code, &version, &filename, &ext, &stored, &size, &source, &note, &signer, &role, &now],
    ).await {
        return Err(err(StatusCode::INTERNAL_SERVER_ERROR, &format!("บันทึกเวอร์ชันในระบบไม่ได้: {e}")));
    }
    let sql = format!("SELECT {VCOLS} FROM versions WHERE doc_code=$1 AND version=$2");
    let v = match client.query_one(&sql, &[&code, &version]).await {
        Ok(r) => row_to_json(&r),
        Err(_) => json!({ "version": version }),
    };
    Ok((version, v))
}

fn row_to_json(r: &tokio_postgres::Row) -> Value {
    json!({
        "version": r.get::<_, i64>(0), "filename": r.get::<_, String>(1), "ext": r.get::<_, String>(2),
        "storedName": r.get::<_, Option<String>>(3), "originPath": r.get::<_, Option<String>>(4),
        "size": r.get::<_, i64>(5), "source": r.get::<_, String>(6), "note": r.get::<_, String>(7),
        "signerName": r.get::<_, String>(8), "signerRole": r.get::<_, String>(9),
        "signedAt": r.get::<_, String>(10), "createdAt": r.get::<_, String>(11),
    })
}

async fn list_versions(st: &S, code: &str) -> Vec<Value> {
    let client = match st.db.get().await { Ok(c) => c, Err(_) => return vec![] };
    let sql = format!("SELECT {VCOLS} FROM versions WHERE doc_code=$1 ORDER BY version DESC");
    match client.query(&sql, &[&code]).await {
        Ok(rows) => rows.iter().map(row_to_json).collect(),
        Err(_) => vec![],
    }
}

async fn counts_by_doc(st: &S) -> HashMap<String, (i64, i64)> {
    let mut m = HashMap::new();
    let client = match st.db.get().await { Ok(c) => c, Err(_) => return m };
    if let Ok(rows) = client
        .query("SELECT doc_code,MAX(version),COUNT(*) FROM versions GROUP BY doc_code", &[])
        .await
    {
        for r in rows {
            m.insert(r.get::<_, String>(0), (r.get::<_, i64>(1), r.get::<_, i64>(2)));
        }
    }
    m
}

// (version, absolute path, ext, filename)
async fn resolve(st: &S, code: &str, v: Option<&String>) -> Result<(i64, PathBuf, String, String), Response> {
    if !st.by_code.contains_key(code) { return Err(err(StatusCode::NOT_FOUND, "ไม่พบเอกสาร")); }
    ensure_original(st, code).await;
    let client = match st.db.get().await { Ok(c) => c, Err(_) => return Err(err(StatusCode::INTERNAL_SERVER_ERROR, "เชื่อมต่อฐานข้อมูลไม่ได้")) };
    if let Some(vv) = v {
        let vn: i64 = vv.parse().unwrap_or(0);
        let row = client.query_opt(
            "SELECT version,ext,stored_name,origin_path,filename FROM versions WHERE doc_code=$1 AND version=$2",
            &[&code, &vn],
        ).await;
        return match row {
            Ok(Some(r)) => {
                let ver: i64 = r.get(0);
                let ext: String = r.get(1);
                let stored: Option<String> = r.get(2);
                let origin: Option<String> = r.get(3);
                let filename: String = r.get(4);
                let abs = version_abs(st, stored, origin, "");
                if !abs.exists() {
                    return Err(err(StatusCode::NOT_FOUND, "ไม่พบไฟล์ในระบบ — ต้นฉบับอาจหายจากดิสก์ หรือยังไม่ได้อัปโหลดเวอร์ชันเข้าสู่ระบบ"));
                }
                Ok((ver, abs, ext, filename))
            }
            _ => Err(err(StatusCode::NOT_FOUND, "ไม่พบเวอร์ชัน")),
        };
    }
    // ล่าสุด: ไล่จากเวอร์ชันใหม่ → เก่า จนเจอไฟล์ที่มีจริง
    let rows = match client
        .query(
            "SELECT version,ext,stored_name,origin_path,filename FROM versions WHERE doc_code=$1 ORDER BY version DESC",
            &[&code],
        )
        .await
    {
        Ok(r) => r,
        Err(_) => return Err(err(StatusCode::NOT_FOUND, "ไม่พบเวอร์ชัน")),
    };
    for r in rows {
        let ver: i64 = r.get(0);
        let ext: String = r.get(1);
        let stored: Option<String> = r.get(2);
        let origin: Option<String> = r.get(3);
        let filename: String = r.get(4);
        let abs = version_abs(st, stored, origin, "");
        if abs.exists() {
            return Ok((ver, abs, ext, filename));
        }
    }
    Err(err(StatusCode::NOT_FOUND, "ไม่พบไฟล์ในระบบ — ต้นฉบับอาจหายจากดิสก์ หรือยังไม่ได้อัปโหลดเวอร์ชันเข้าสู่ระบบ"))
}

fn version_abs(st: &S, stored: Option<String>, origin: Option<String>, fallback_rel: &str) -> PathBuf {
    match stored {
        Some(s) => st.storage.join(s),
        None => st.docs_root.join(origin.unwrap_or_else(|| fallback_rel.to_string())),
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
    let db_ok = st.db.get().await.is_ok();
    Json(json!({ "service": "iso-api (Rust/Axum)", "api": "ok", "db": if db_ok {"ok"} else {"down"}, "converter": if conv {"ok"} else {"down"} }))
}

async fn docs_list(State(st): State<S>) -> Json<Value> {
    let counts = counts_by_doc(&st).await;
    let arr: Vec<Value> = st
        .manifest
        .iter()
        .map(|d| {
            let code = d["code"].as_str().unwrap_or("");
            let e = ext_of(d);
            let (latest, count) = counts.get(code).cloned().unwrap_or((1, 1));
            let rel = d["path"].as_str().unwrap_or("");
            let file_exists = !rel.is_empty() && st.docs_root.join(rel).exists();
            let marked_missing = d["missing"].as_bool().unwrap_or(false);
            let mut o = d.clone();
            o["editable"] = json!(editable(&e));
            o["previewable"] = json!(previewable(&e));
            o["latestVersion"] = json!(latest);
            o["versionCount"] = json!(count);
            o["fileExists"] = json!(file_exists);
            o["missing"] = json!(marked_missing || !file_exists);
            o
        })
        .collect();
    Json(Value::Array(arr))
}

async fn versions(Path(code): Path<String>, State(st): State<S>) -> Response {
    let doc = match st.by_code.get(&code) { Some(d) => d.clone(), None => return err(StatusCode::NOT_FOUND, "ไม่พบเอกสาร") };
    ensure_original(&st, &code).await;
    let e = ext_of(&doc);
    let rel = doc["path"].as_str().unwrap_or("");
    let file_exists = !rel.is_empty() && st.docs_root.join(rel).exists();
    Json(json!({
        "code": code, "name": doc["name"], "ext": e,
        "editable": editable(&e), "previewable": previewable(&e),
        "fileExists": file_exists,
        "missing": doc["missing"].as_bool().unwrap_or(false) || !file_exists,
        "versions": list_versions(&st, &code).await,
    }))
    .into_response()
}

async fn preview_abs(st: &S, abs: &PathBuf, ext: &str) -> Value {
    match ext {
        "xls" | "xlsx" => match fs::read(abs) {
            Ok(b) => json!({ "kind": "sheet", "ext": ext, "data": B64.encode(b) }),
            Err(e) => json!({ "kind": "download", "ext": ext, "reason": format!("อ่านไฟล์ไม่ได้: {e}") }),
        },
        "docx" => {
            let body = json!({ "path": abs.to_string_lossy(), "ext": "docx" }).to_string();
            match call_converter(&st.conv_port, "/convert", &body).await {
                Ok(v) if v["ok"] == json!(true) => json!({ "kind": "html", "ext": ext, "html": v["html"] }),
                Ok(v) => json!({ "kind": "download", "ext": ext, "reason": format!("แปลงเอกสารไม่สำเร็จ: {}", v["error"].as_str().unwrap_or("ไม่ทราบสาเหตุ")) }),
                Err(e) => json!({ "kind": "download", "ext": ext, "reason": format!("converter ไม่ตอบสนอง: {e}") }),
            }
        }
        "pdf" | "jpg" | "jpeg" | "png" => json!({ "kind": "embed", "ext": ext }),
        _ => json!({ "kind": "download", "ext": ext, "reason": "ไฟล์ชนิดนี้เปิดอ่านบนหน้าเว็บไม่ได้ — กดดาวน์โหลดเพื่อเปิดด้วยโปรแกรมในเครื่อง" }),
    }
}

fn resolve_overview_path(st: &S, rel: &str) -> Result<(PathBuf, String, String), Response> {
    if rel.is_empty() { return Err(err(StatusCode::BAD_REQUEST, "ไม่ระบุไฟล์")); }
    if rel.contains("..") { return Err(err(StatusCode::BAD_REQUEST, "พาธไม่ถูกต้อง")); }
    let abs = st.docs_root.join(rel);
    let base = norm_path(&st.docs_root.to_string_lossy()).to_lowercase();
    let target = norm_path(&abs.to_string_lossy()).to_lowercase();
    if !target.starts_with(&base) { return Err(err(StatusCode::BAD_REQUEST, "พาธไม่ถูกต้อง")); }
    let ext = abs.extension().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
    let name = abs.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    Ok((abs, ext, name))
}

/// รหัสเอกสารสำหรับพาธภาพรวม: ใช้รหัสในทะเบียนถ้ามี ไม่งั้นใช้ OV:<hash>
async fn overview_doc_code(st: &S, rel: &str, abs: &PathBuf, name: &str, ext: &str) -> Result<String, Response> {
    if let Some(c) = find_code_by_path(st, rel) {
        ensure_original(st, &c).await;
        return Ok(c);
    }
    let c = ov_doc_code(rel);
    if !abs.exists() && !version_exists(st, &c).await {
        return Err(err(StatusCode::NOT_FOUND, "ไม่พบไฟล์"));
    }
    ensure_original_row(st, &c, rel, name, ext).await;
    Ok(c)
}

/// เลือกไฟล์ล่าสุดในระบบสำหรับพาธภาพรวม (เวอร์ชันใน storage มาก่อน ต้นฉบับบนดิสก์เป็นรอง)
async fn overview_live_file(st: &S, rel: &str) -> Result<(PathBuf, String, String, Option<String>, Option<i64>), Response> {
    let (abs, ext, name) = resolve_overview_path(st, rel)?;
    let code = match find_code_by_path(st, rel) {
        Some(c) => {
            ensure_original(st, &c).await;
            c
        }
        None => ov_doc_code(rel),
    };
    let client = match st.db.get().await {
        Ok(c) => c,
        Err(_) => return Err(err(StatusCode::INTERNAL_SERVER_ERROR, "เชื่อมต่อฐานข้อมูลไม่ได้")),
    };
    if let Ok(rows) = client
        .query(
            "SELECT version,ext,stored_name,origin_path,filename FROM versions WHERE doc_code=$1 ORDER BY version DESC",
            &[&code],
        )
        .await
    {
        for r in rows {
            let ver: i64 = r.get(0);
            let vext: String = r.get(1);
            let stored: Option<String> = r.get(2);
            let origin: Option<String> = r.get(3);
            let filename: String = r.get(4);
            let live = version_abs(st, stored, origin, rel);
            if live.exists() {
                return Ok((live, vext, filename, Some(code), Some(ver)));
            }
        }
    }
    if !abs.exists() {
        return Err(err(StatusCode::NOT_FOUND, "ไม่พบไฟล์"));
    }
    Ok((abs, ext, name, Some(code), None))
}

async fn overview_file(Query(q): Query<HashMap<String, String>>, State(st): State<S>) -> Response {
    let rel = q.get("path").cloned().unwrap_or_default();
    let (abs, ext, name, _, _) = match overview_live_file(&st, &rel).await { Ok(x) => x, Err(r) => return r };
    let bytes = match fs::read(&abs) { Ok(b) => b, Err(_) => return err(StatusCode::NOT_FOUND, "อ่านไฟล์ไม่ได้") };
    let disp = if q.get("dl").map(|s| s == "1").unwrap_or(false) { "attachment" } else { "inline" };
    Response::builder()
        .header(header::CONTENT_TYPE, mime_of(&ext))
        .header(header::CONTENT_DISPOSITION, format!("{disp}; filename*=UTF-8''{}", pct(&name)))
        .body(Body::from(bytes))
        .unwrap()
}

async fn overview_content(Query(q): Query<HashMap<String, String>>, State(st): State<S>) -> Response {
    let rel = q.get("path").cloned().unwrap_or_default();
    let (abs, ext, name, code, ver) = match overview_live_file(&st, &rel).await { Ok(x) => x, Err(r) => return r };
    let mut body = preview_abs(&st, &abs, &ext).await;
    body["name"] = json!(name);
    body["path"] = json!(rel);
    body["editable"] = json!(editable(&ext));
    if let Some(c) = code { body["docCode"] = json!(c); }
    if let Some(v) = ver { body["version"] = json!(v); }
    Json(body).into_response()
}

#[derive(Deserialize)]
#[allow(non_snake_case)]
struct OverviewSaveReq {
    path: Option<String>,
    dataBase64: Option<String>,
    signerName: Option<String>,
    signerRole: Option<String>,
    note: Option<String>,
}

async fn overview_save(State(st): State<S>, Json(b): Json<OverviewSaveReq>) -> Response {
    let rel = b.path.unwrap_or_default();
    let (abs, ext, name) = match resolve_overview_path(&st, &rel) {
        Ok(x) => x,
        Err(r) => return r,
    };
    if !editable(&ext) {
        return err(StatusCode::BAD_REQUEST, "ไฟล์ชนิดนี้แก้ไขบนหน้าเว็บไม่ได้");
    }
    let code = match overview_doc_code(&st, &rel, &abs, &name, &ext).await {
        Ok(c) => c,
        Err(r) => return r,
    };
    let signer = match require_signer(b.signerName, "ต้องลงชื่อผู้แก้ไขก่อนอัปเดท") {
        Ok(n) => n,
        Err(r) => return r,
    };
    let bytes = match decode_upload_bytes(b.dataBase64) {
        Ok(x) => x,
        Err(r) => return r,
    };
    let note = b.note.unwrap_or_default().trim().to_string();
    let role = b.signerRole.unwrap_or_default().trim().to_string();
    let filename = st
        .by_code
        .get(&code)
        .and_then(|d| d["file"].as_str())
        .unwrap_or(&name)
        .to_string();

    // บันทึกเวอร์ชันในระบบ + ซิงก์ต้นฉบับบนดิสก์
    let (version, v) = match save_new_version(
        &st, &code, &bytes, &filename, &ext, "edit", &note, &signer, &role, Some(abs.as_path()),
    ).await {
        Ok(x) => x,
        Err(r) => return r,
    };

    Json(json!({
        "ok": true,
        "inSystem": true,
        "path": rel,
        "name": name,
        "ext": ext,
        "size": bytes.len(),
        "code": code,
        "version": v,
        "latestVersion": version,
    })).into_response()
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
    let doc = match st.by_code.get(&code) {
        Some(d) => d.clone(),
        None => return err(StatusCode::NOT_FOUND, "ไม่พบเอกสาร"),
    };
    ensure_original(&st, &code).await;
    let name = match require_signer(b.signerName, "ต้องลงชื่อผู้แก้ไขก่อนบันทึก") {
        Ok(n) => n,
        Err(r) => return r,
    };
    let bytes = match decode_upload_bytes(b.dataBase64) {
        Ok(x) => x,
        Err(r) => return r,
    };
    let safe_ext: String = b
        .ext
        .unwrap_or_else(|| ext_of(&doc))
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    let filename = b.filename.unwrap_or_else(|| doc["file"].as_str().unwrap_or("").to_string());
    let source = if b.source.as_deref() == Some("edit") { "edit" } else { "upload" };
    let note = b.note.unwrap_or_default().trim().to_string();
    let role = b.signerRole.unwrap_or_default().trim().to_string();

    let origin = {
        let rel = doc["path"].as_str().unwrap_or("");
        let p = st.docs_root.join(rel);
        if rel.is_empty() { None } else { Some(p) }
    };
    match save_new_version(
        &st, &code, &bytes, &filename, &safe_ext, source, &note, &name, &role,
        origin.as_deref(),
    ).await {
        Ok((_, v)) => Json(json!({ "ok": true, "version": v, "inSystem": true })).into_response(),
        Err(r) => r,
    }
}

// ---------- CAR / DAR / PAR (ใบร้องขอดำเนินการ) ----------
fn action_to_json(r: &tokio_postgres::Row) -> Value {
    json!({
        "id": r.get::<_, i64>(0), "refNo": r.get::<_, String>(1), "kind": r.get::<_, String>(2),
        "title": r.get::<_, String>(3), "detail": r.get::<_, String>(4), "dept": r.get::<_, String>(5),
        "docCode": r.get::<_, String>(6), "raisedBy": r.get::<_, String>(7), "assignee": r.get::<_, String>(8),
        "priority": r.get::<_, String>(9), "status": r.get::<_, String>(10), "progress": r.get::<_, i64>(11),
        "dueDate": r.get::<_, String>(12), "actionTaken": r.get::<_, String>(13),
        "createdAt": r.get::<_, String>(14), "updatedAt": r.get::<_, String>(15), "closedAt": r.get::<_, String>(16),
    })
}

#[derive(Deserialize)]
#[allow(non_snake_case)]
struct ActionCreate {
    kind: String, title: String,
    detail: Option<String>, dept: Option<String>, docCode: Option<String>,
    raisedBy: Option<String>, assignee: Option<String>, priority: Option<String>, dueDate: Option<String>,
}

#[derive(Deserialize)]
#[allow(non_snake_case)]
struct ActionUpdate {
    status: Option<String>, progress: Option<i64>, actionTaken: Option<String>,
    assignee: Option<String>, dueDate: Option<String>, priority: Option<String>, title: Option<String>, detail: Option<String>,
}

async fn actions_list(State(st): State<S>) -> Response {
    let client = match st.db.get().await { Ok(c) => c, Err(_) => return err(StatusCode::INTERNAL_SERVER_ERROR, "เชื่อมต่อฐานข้อมูลไม่ได้") };
    let sql = format!("SELECT {ACOLS} FROM actions ORDER BY created_at DESC");
    match client.query(&sql, &[]).await {
        Ok(rows) => Json(Value::Array(rows.iter().map(action_to_json).collect())).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, &format!("อ่านรายการไม่ได้: {e}")),
    }
}

async fn action_create(State(st): State<S>, Json(b): Json<ActionCreate>) -> Response {
    let kind = b.kind.to_uppercase();
    if !matches!(kind.as_str(), "CAR" | "DAR" | "PAR") { return err(StatusCode::BAD_REQUEST, "ประเภทต้องเป็น CAR/DAR/PAR"); }
    if b.title.trim().is_empty() { return err(StatusCode::BAD_REQUEST, "ต้องระบุเรื่อง"); }
    let client = match st.db.get().await { Ok(c) => c, Err(_) => return err(StatusCode::INTERNAL_SERVER_ERROR, "เชื่อมต่อฐานข้อมูลไม่ได้") };
    // เลขที่ใบ: KIND-<พ.ศ.>-<ลำดับ 3 หลัก>
    let year_be = chrono::Utc::now().format("%Y").to_string().parse::<i64>().unwrap_or(2025) + 543;
    let prefix = format!("{}-{}-", kind, year_be);
    let seq: i64 = client
        .query_one("SELECT COUNT(*) FROM actions WHERE ref_no LIKE $1", &[&format!("{}%", prefix)])
        .await.map(|r| r.get::<_, i64>(0)).unwrap_or(0) + 1;
    let ref_no = format!("{}{:03}", prefix, seq);
    let now = now_iso();
    let detail = b.detail.unwrap_or_default();
    let dept = b.dept.unwrap_or_default();
    let doc_code = b.docCode.unwrap_or_default();
    let raised_by = b.raisedBy.unwrap_or_default();
    let assignee = b.assignee.unwrap_or_default();
    let priority = b.priority.unwrap_or_else(|| "medium".into());
    let due = b.dueDate.unwrap_or_default();
    let sql = format!(
        "INSERT INTO actions(ref_no,kind,title,detail,dept,doc_code,raised_by,assignee,priority,status,progress,due_date,action_taken,created_at,updated_at,closed_at)
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,'open',0,$10,'',$11,$11,'') RETURNING {ACOLS}");
    match client.query_one(&sql, &[&ref_no, &kind, &b.title.trim().to_string(), &detail, &dept, &doc_code, &raised_by, &assignee, &priority, &due, &now]).await {
        Ok(r) => Json(json!({ "ok": true, "action": action_to_json(&r) })).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, &format!("เปิดใบไม่สำเร็จ: {e}")),
    }
}

async fn action_update(Path(id): Path<i64>, State(st): State<S>, Json(b): Json<ActionUpdate>) -> Response {
    let client = match st.db.get().await { Ok(c) => c, Err(_) => return err(StatusCode::INTERNAL_SERVER_ERROR, "เชื่อมต่อฐานข้อมูลไม่ได้") };
    let cur = match client.query_opt(&format!("SELECT {ACOLS} FROM actions WHERE id=$1"), &[&id]).await {
        Ok(Some(r)) => action_to_json(&r), _ => return err(StatusCode::NOT_FOUND, "ไม่พบใบนี้"),
    };
    let status = b.status.unwrap_or_else(|| cur["status"].as_str().unwrap_or("open").to_string());
    if !matches!(status.as_str(), "open" | "in_progress" | "closed") { return err(StatusCode::BAD_REQUEST, "สถานะไม่ถูกต้อง"); }
    let mut progress = b.progress.unwrap_or_else(|| cur["progress"].as_i64().unwrap_or(0));
    if status == "closed" { progress = 100; }
    progress = progress.clamp(0, 100);
    let action_taken = b.actionTaken.unwrap_or_else(|| cur["actionTaken"].as_str().unwrap_or("").to_string());
    let assignee = b.assignee.unwrap_or_else(|| cur["assignee"].as_str().unwrap_or("").to_string());
    let due = b.dueDate.unwrap_or_else(|| cur["dueDate"].as_str().unwrap_or("").to_string());
    let priority = b.priority.unwrap_or_else(|| cur["priority"].as_str().unwrap_or("medium").to_string());
    let title = b.title.unwrap_or_else(|| cur["title"].as_str().unwrap_or("").to_string());
    let detail = b.detail.unwrap_or_else(|| cur["detail"].as_str().unwrap_or("").to_string());
    let now = now_iso();
    let closed_at = if status == "closed" {
        let prev = cur["closedAt"].as_str().unwrap_or("");
        if prev.is_empty() { now.clone() } else { prev.to_string() }
    } else { String::new() };
    let sql = format!(
        "UPDATE actions SET status=$2,progress=$3,action_taken=$4,assignee=$5,due_date=$6,priority=$7,title=$8,detail=$9,updated_at=$10,closed_at=$11 WHERE id=$1 RETURNING {ACOLS}");
    match client.query_one(&sql, &[&id, &status, &progress, &action_taken, &assignee, &due, &priority, &title, &detail, &now, &closed_at]).await {
        Ok(r) => Json(json!({ "ok": true, "action": action_to_json(&r) })).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, &format!("อัปเดตไม่สำเร็จ: {e}")),
    }
}

async fn stats(State(st): State<S>) -> Json<Value> {
    let counts = counts_by_doc(&st).await;
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
        match st.db.get().await {
            Ok(client) => match client
                .query("SELECT signer_name,signer_role,signed_at,doc_code,note,source,version FROM versions WHERE source!='original' ORDER BY created_at DESC", &[])
                .await
            {
                Ok(rows) => rows.iter().map(|r| (
                    r.get::<_, String>(0), r.get::<_, String>(1), r.get::<_, String>(2),
                    r.get::<_, String>(3), r.get::<_, String>(4), r.get::<_, String>(5), r.get::<_, i64>(6),
                )).collect(),
                Err(_) => vec![],
            },
            Err(_) => vec![],
        }
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

async fn content(Path(code): Path<String>, Query(q): Query<HashMap<String, String>>, State(st): State<S>) -> Response {
    let (ver, abs, ext, _fn) = match resolve(&st, &code, q.get("v")).await { Ok(x) => x, Err(r) => return r };
    let mut body = preview_abs(&st, &abs, &ext).await;
    body["version"] = json!(ver);
    Json(body).into_response()
}

async fn file(Path(code): Path<String>, Query(q): Query<HashMap<String, String>>, State(st): State<S>) -> Response {
    let (_v, abs, ext, filename) = match resolve(&st, &code, q.get("v")).await { Ok(x) => x, Err(r) => return r };
    let bytes = match fs::read(&abs) { Ok(b) => b, Err(_) => return err(StatusCode::NOT_FOUND, "อ่านไฟล์ไม่ได้") };
    let disp = if q.get("dl").map(|s| s == "1").unwrap_or(false) { "attachment" } else { "inline" };
    Response::builder()
        .header(header::CONTENT_TYPE, mime_of(&ext))
        .header(header::CONTENT_DISPOSITION, format!("{disp}; filename*=UTF-8''{}", pct(&filename)))
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

    let pool = make_pool();
    init_db(&pool).await;
    migrate_from_sqlite(&pool, &docs_root.join("versions.db")).await;

    let state: S = Arc::new(AppState {
        manifest, by_code, overview: overview_json, docs_root, storage,
        conv_port: std::env::var("CONVERTER_PORT").unwrap_or_else(|_| "8081".into()),
        db: pool,
    });

    let app = Router::new()
        .route("/api/health", get(health))
        .route("/api/docs", get(docs_list))
        .route("/api/docs/:code/versions", get(versions).post(save))
        .route("/api/docs/:code/content", get(content))
        .route("/api/docs/:code/file", get(file))
        .route("/api/stats", get(stats))
        .route("/api/actions", get(actions_list).post(action_create))
        .route("/api/actions/:id", axum::routing::put(action_update))
        .route("/api/overview", get(overview))
        .route("/api/overview/file", get(overview_file))
        .route("/api/overview/content", get(overview_content))
        .route("/api/overview/save", axum::routing::post(overview_save))
        .layer(DefaultBodyLimit::max(64 * 1024 * 1024))
        .with_state(state);

    let port = std::env::var("API_PORT").unwrap_or_else(|_| "8080".into());
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await.unwrap();
    println!("\n  ✅ iso-api (Rust/Axum + PostgreSQL) พร้อมที่ http://localhost:{port}\n");
    axum::serve(listener, app).await.unwrap();
}
