// iso-api — API หลัก (Rust + Axum)
//   จัดการเอกสาร ISO: ทะเบียน, เวอร์ชัน+ลายเซ็น (PostgreSQL), เนื้อหา (xlsx/docx/pdf), บันทึกเวอร์ชัน,
//   สรุปแดชบอร์ด, คลังภาพรวมองค์กร  ·  แปลง docx->HTML โดยเรียก Go converter
use axum::{
    body::Body,
    extract::{DefaultBodyLimit, Extension, Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use deadpool_postgres::{ManagerConfig, Pool, RecyclingMethod};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{collections::HashMap, fs, net::TcpStream, path::PathBuf, sync::{Arc, Mutex}, time::Duration};
use tokio_postgres::NoTls;

mod acks;
mod audittrail;
mod capa;
mod doccontrol;
mod eform;
mod internalaudit;
mod mreview;
mod readiness;
mod training;

pub(crate) struct AppState {
    pub(crate) manifest: Vec<Value>,
    pub(crate) by_code: HashMap<String, Value>,
    pub(crate) overview: Value,
    /// แผนผังข้อกำหนด ISO → เอกสารหลักฐาน (data/clauses.json)
    pub(crate) clauses: Value,
    pub(crate) docs_root: PathBuf,
    pub(crate) storage: PathBuf,
    pub(crate) conv_port: String,
    pub(crate) db: Pool,
    pub(crate) login_hits: Mutex<HashMap<String, Vec<i64>>>,
}
pub(crate) type S = Arc<AppState>;

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
pub(crate) fn now_iso() -> String { chrono::Utc::now().to_rfc3339() }
pub(crate) fn err(c: StatusCode, m: &str) -> Response { (c, Json(json!({ "error": m }))).into_response() }

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

const VCOLS: &str = "version,filename,ext,stored_name,origin_path,size,source,note,signer_name,signer_role,signed_at,created_at,rev,effective_date,change_id,sha256,signer_code,approved_by,approved_at";
/// คอลัมน์ของตาราง versions ในไฟล์ SQLite รุ่นเก่า (ใช้ตอน migrate เท่านั้น)
const LEGACY_VCOLS: &str = "version,filename,ext,stored_name,origin_path,size,source,note,signer_name,signer_role,signed_at,created_at";

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
                client.batch_execute(eform::SCHEMA).await.expect("สร้างตารางแบบฟอร์มไม่สำเร็จ");
                client.batch_execute(doccontrol::SCHEMA).await.expect("สร้างตารางควบคุมการเปลี่ยนแปลงเอกสารไม่สำเร็จ");
                client.batch_execute(readiness::SCHEMA).await.expect("สร้างตารางผลความพร้อม Audit ไม่สำเร็จ");
                client.batch_execute(mreview::SCHEMA).await.expect("สร้างตารางทบทวนฝ่ายบริหารไม่สำเร็จ");
                client.batch_execute(capa::SCHEMA).await.expect("ปรับตาราง CAR/DAR/PAR ไม่สำเร็จ");
                client.batch_execute(internalaudit::SCHEMA).await.expect("สร้างตารางตรวจติดตามภายในไม่สำเร็จ");
                client.batch_execute(acks::SCHEMA).await.expect("สร้างตารางการรับทราบเอกสารไม่สำเร็จ");
                client.batch_execute(training::SCHEMA).await.expect("สร้างตาราง training matrix ไม่สำเร็จ");
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
            let mut stmt = match c.prepare(&format!("SELECT doc_code,{LEGACY_VCOLS} FROM versions")) {
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
    let abs = st.docs_root.join(rel);
    // แช่แข็งต้นฉบับเป็นไฟล์ในคลังเวอร์ชันทันที — การแก้/อัปโหลด/แก้ในแฟ้มภายหลังจะไม่ทับเวอร์ชัน 1
    let (stored, size, sha) = match fs::read(&abs) {
        Ok(bytes) => {
            let name = format!("{}__v1.{}", safe_stem(code), ext);
            if fs::write(st.storage.join(&name), &bytes).is_ok() { (Some(name), bytes.len() as i64, sha256_hex(&bytes)) } else { (None, bytes.len() as i64, String::new()) }
        }
        Err(_) => (None, 0, String::new()),
    };
    let now = now_iso();
    let _ = client.execute(
        "INSERT INTO versions(doc_code,version,filename,ext,stored_name,origin_path,size,source,note,signer_name,signer_role,signed_at,created_at,sha256)
         VALUES($1,1,$2,$3,$4,$5,$6,'original','เวอร์ชันต้นฉบับจากแฟ้มเอกสาร','ระบบ (ต้นฉบับ)','',$7,$7,$8)
         ON CONFLICT (doc_code,version) DO NOTHING",
        &[&code, &filename, &ext, &stored, &rel, &size, &now, &sha],
    ).await;
}

/// ไฟล์ต้นฉบับที่ถูกเขียนทับไปแล้วก่อนมีการแช่แข็ง (กู้ไม่ได้จากเครื่อง)
const LOST: &str = "LOST";

/// ตอนเริ่มระบบ: สร้างเวอร์ชัน 1 ให้ทุกเอกสารในทะเบียน และแช่แข็งต้นฉบับที่ยังอ้างไฟล์ในแฟ้ม (stored_name ว่าง) เข้าคลังเวอร์ชัน
/// ถ้าไฟล์ในแฟ้มตรงกับเวอร์ชันที่ใหม่กว่า แปลว่าต้นฉบับถูกเขียนทับแล้ว → ทำเครื่องหมายสูญหาย แทนการแสดงเนื้อหาผิดฉบับ
async fn freeze_originals(st: &S) {
    for code in st.by_code.keys().cloned().collect::<Vec<_>>() { ensure_original(st, &code).await; }
    let Ok(c) = st.db.get().await else { return };
    let Ok(rows) = c.query("SELECT doc_code,ext,origin_path,note FROM versions WHERE version=1 AND stored_name IS NULL AND origin_path IS NOT NULL", &[]).await else { return };
    let (mut frozen, mut lost) = (0, 0);
    for r in rows {
        let (code, ext, rel, note): (String, String, String, String) = (r.get(0), r.get(1), r.get(2), r.get(3));
        let Ok(bytes) = fs::read(st.docs_root.join(&rel)) else { continue };
        let sha = sha256_hex(&bytes);
        let later = c.query("SELECT version,stored_name FROM versions WHERE doc_code=$1 AND version>1 AND stored_name IS NOT NULL ORDER BY version", &[&code]).await.unwrap_or_default();
        let overwritten_by = later.iter().find_map(|v| {
            let name: String = v.get(1);
            fs::read(st.storage.join(&name)).ok().filter(|b| sha256_hex(b) == sha).map(|_| v.get::<_, i64>(0))
        });
        if let Some(v) = overwritten_by {
            let msg = format!("{note} · ⚠ ไฟล์ต้นฉบับในแฟ้มถูกเขียนทับด้วย v{v} ก่อนระบบแช่แข็งต้นฉบับ — กู้ได้จากประวัติเวอร์ชันของ OneDrive");
            let _ = c.execute("UPDATE versions SET stored_name=$2, note=$3 WHERE doc_code=$1 AND version=1", &[&code, &LOST, &msg]).await;
            lost += 1;
        } else {
            let name = format!("{}__v1.{}", safe_stem(&code), ext);
            if fs::write(st.storage.join(&name), &bytes).is_ok() {
                let size = bytes.len() as i64;
                let _ = c.execute("UPDATE versions SET stored_name=$2, sha256=$3, size=$4 WHERE doc_code=$1 AND version=1", &[&code, &name, &sha, &size]).await;
                frozen += 1;
            }
        }
    }
    if frozen + lost > 0 { println!("  🧊 แช่แข็งต้นฉบับเข้าคลังเวอร์ชัน {frozen} ฉบับ · ต้นฉบับสูญหาย (ถูกเขียนทับก่อนหน้า) {lost} ฉบับ"); }
}

/// ข้อมูลเวอร์ชันใหม่ที่จะบันทึก — ผู้ลงนามมาจากเซสชันที่ login เสมอ (ไม่รับชื่อที่พิมพ์เอง)
pub(crate) struct NewVersion<'a> {
    pub code: &'a str,
    pub filename: &'a str,
    pub ext: &'a str,
    pub source: &'a str,
    pub note: &'a str,
    pub signer_code: &'a str,
    pub signer_name: &'a str,
    pub signer_role: &'a str,
    /// ฉบับแก้ไข (เช่น Rev.01) และวันที่บังคับใช้ (วว/ดด/พ.ศ.) — ว่างได้สำหรับไฟล์ภาพรวมที่ไม่อยู่ในทะเบียน
    pub rev: &'a str,
    pub effective_date: &'a str,
    /// คำขอเปลี่ยนแปลง (DCR) ที่อนุมัติเวอร์ชันนี้ — 0 = ไม่ผ่าน DCR
    pub change_id: i64,
    pub approved_by: &'a str,
    pub approved_at: &'a str,
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// บันทึกไฟล์เป็นเวอร์ชันใหม่ใน storage + PostgreSQL
/// ถ้ามี `origin_sync` จะเขียนทับต้นฉบับด้วย (ให้เปิดจากดิสก์แล้วเห็นเวอร์ชันล่าสุด)
pub(crate) async fn save_new_version(
    st: &S,
    nv: &NewVersion<'_>,
    bytes: &[u8],
    origin_sync: Option<&std::path::Path>,
) -> Result<(i64, Value), Response> {
    let client = match st.db.get().await {
        Ok(c) => c,
        Err(_) => return Err(err(StatusCode::INTERNAL_SERVER_ERROR, "เชื่อมต่อฐานข้อมูลไม่ได้")),
    };
    let code = nv.code;
    let version: i64 = client
        .query_one("SELECT COALESCE(MAX(version),0) FROM versions WHERE doc_code=$1", &[&code])
        .await
        .map(|r| r.get::<_, i64>(0))
        .unwrap_or(0)
        + 1;
    let stored = format!("{}__v{}.{}", safe_stem(code), version, nv.ext);
    if let Err(e) = fs::write(st.storage.join(&stored), bytes) {
        return Err(err(StatusCode::INTERNAL_SERVER_ERROR, &format!("บันทึกไฟล์ในระบบไม่ได้: {e}")));
    }
    // ซิงก์ไฟล์ในแฟ้มให้เป็นฉบับล่าสุดเฉพาะเมื่อชนิดไฟล์ตรงกัน (อัปโหลด .pdf แทน .docx จะไม่เขียนทับไฟล์ .docx เดิม)
    let same_ext = |p: &&std::path::Path| p.extension().map(|e| e.to_string_lossy().eq_ignore_ascii_case(nv.ext)).unwrap_or(false);
    if let Some(origin) = origin_sync.filter(same_ext) {
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
    let sha = sha256_hex(bytes);
    if let Err(e) = client.execute(
        "INSERT INTO versions(doc_code,version,filename,ext,stored_name,origin_path,size,source,note,signer_name,signer_role,signed_at,created_at,
                              rev,effective_date,change_id,sha256,signer_code,approved_by,approved_at)
         VALUES($1,$2,$3,$4,$5,NULL,$6,$7,$8,$9,$10,$11,$11,$12,$13,$14,$15,$16,$17,$18)",
        &[&code, &version, &nv.filename, &nv.ext, &stored, &size, &nv.source, &nv.note, &nv.signer_name, &nv.signer_role, &now,
          &nv.rev, &nv.effective_date, &nv.change_id, &sha, &nv.signer_code, &nv.approved_by, &nv.approved_at],
    ).await {
        let _ = fs::remove_file(st.storage.join(&stored));
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
        "rev": r.get::<_, String>(12), "effectiveDate": r.get::<_, String>(13), "changeId": r.get::<_, i64>(14),
        "sha256": r.get::<_, String>(15), "signerCode": r.get::<_, String>(16),
        "approvedBy": r.get::<_, String>(17), "approvedAt": r.get::<_, String>(18),
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
                if stored.as_deref() == Some(LOST) {
                    return Err(err(StatusCode::NOT_FOUND, "ไฟล์ต้นฉบับเวอร์ชันนี้สูญหาย (ถูกเขียนทับก่อนระบบแช่แข็งต้นฉบับ) — กู้ได้จากประวัติเวอร์ชันของ OneDrive"));
                }
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
        Some(s) if s == LOST => st.storage.join("__lost__").join("original"),
        Some(s) => st.storage.join(s),
        None => st.docs_root.join(origin.unwrap_or_else(|| fallback_rel.to_string())),
    }
}

// ---------- เรียก Go converter (HTTP ง่าย ๆ ผ่าน TCP) ----------
pub(crate) async fn call_converter(port: &str, path_route: &str, body: &str) -> Result<Value, String> {
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
    let ctl = doccontrol::control_map(&st).await;
    let last = latest_by_doc(&st).await;
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
            ctl.apply(code, &mut o);
            if let Some(l) = last.get(code) { o["lastBy"] = l["by"].clone(); o["lastAt"] = l["at"].clone(); }
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
    let mut o = json!({
        "code": code, "name": doc["name"], "ext": e,
        "editable": editable(&e), "previewable": previewable(&e),
        "fileExists": file_exists,
        "missing": doc["missing"].as_bool().unwrap_or(false) || !file_exists,
        "rev": doc["rev"], "eff": doc["eff"],
        "versions": list_versions(&st, &code).await,
    });
    doccontrol::control_map(&st).await.apply(&code, &mut o);
    o["nextRev"] = json!(doccontrol::bump_rev(o["rev"].as_str().unwrap_or("")));
    Json(o).into_response()
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
async fn overview_live_file(st: &S, rel: &str, want: Option<i64>) -> Result<(PathBuf, String, String, Option<String>, Option<i64>), Response> {
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
            if want.map(|w| w != ver).unwrap_or(false) { continue; }
            let vext: String = r.get(1);
            let stored: Option<String> = r.get(2);
            let origin: Option<String> = r.get(3);
            let filename: String = r.get(4);
            let live = version_abs(st, stored, origin, rel);
            if live.exists() {
                return Ok((live, vext, filename, Some(code), Some(ver)));
            }
            if want.is_some() { return Err(err(StatusCode::NOT_FOUND, "ไฟล์ของเวอร์ชันนี้ไม่มีในระบบ")); }
        }
    }
    if want.is_some() { return Err(err(StatusCode::NOT_FOUND, "ไม่พบเวอร์ชัน")); }
    if !abs.exists() {
        return Err(err(StatusCode::NOT_FOUND, "ไม่พบไฟล์"));
    }
    Ok((abs, ext, name, Some(code), None))
}

async fn overview_file(Query(q): Query<HashMap<String, String>>, State(st): State<S>) -> Response {
    let rel = q.get("path").cloned().unwrap_or_default();
    let (abs, ext, name, _, _) = match overview_live_file(&st, &rel, q.get("v").and_then(|v| v.parse().ok())).await { Ok(x) => x, Err(r) => return r };
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
    let (abs, ext, name, code, ver) = match overview_live_file(&st, &rel, q.get("v").and_then(|v| v.parse().ok())).await { Ok(x) => x, Err(r) => return r };
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
    note: Option<String>,
    /// อัปโหลดไฟล์ใหม่ (ชื่อไฟล์ที่เลือก) — ไม่ระบุ = แก้ไขบนเว็บ
    filename: Option<String>,
}

/// บันทึกไฟล์ภาพรวมองค์กรที่ไม่อยู่ในทะเบียน — เอกสารควบคุมต้องผ่านคำขอเปลี่ยนแปลง (DCR) เท่านั้น
async fn overview_save(State(st): State<S>, Extension(u): Extension<eform::AuthUser>, Json(b): Json<OverviewSaveReq>) -> Response {
    let rel = b.path.unwrap_or_default();
    let (abs, ext, name) = match resolve_overview_path(&st, &rel) {
        Ok(x) => x,
        Err(r) => return r,
    };
    let upload = b.filename.as_deref().map(|f| !f.trim().is_empty()).unwrap_or(false);
    if !upload && !editable(&ext) {
        return err(StatusCode::BAD_REQUEST, "ไฟล์ชนิดนี้แก้ไขบนเว็บไม่ได้ — ใช้อัปโหลดไฟล์เวอร์ชันใหม่");
    }
    if let Some(code) = find_code_by_path(&st, &rel) {
        return err(StatusCode::CONFLICT, &format!("{code} เป็นเอกสารควบคุมในทะเบียน — ต้องส่งคำขอเปลี่ยนแปลงเอกสาร (DCR) เพื่อตรวจสอบและอนุมัติก่อน"));
    }
    if u.role == "viewer" {
        return err(StatusCode::FORBIDDEN, "บัญชีผู้อ่านอย่างเดียวแก้ไขไฟล์ไม่ได้");
    }
    let code = match overview_doc_code(&st, &rel, &abs, &name, &ext).await {
        Ok(c) => c,
        Err(r) => return r,
    };
    let bytes = match decode_upload_bytes(b.dataBase64) {
        Ok(x) => x,
        Err(r) => return r,
    };
    let note = b.note.unwrap_or_default().trim().to_string();
    let (vname, vext) = if upload {
        let f: String = b.filename.unwrap_or_default().chars().filter(|c| *c != '/' && *c != '\\' && !c.is_control()).take(200).collect();
        let e: String = f.rsplit('.').next().unwrap_or("").to_lowercase().chars().filter(|c| c.is_ascii_alphanumeric()).take(8).collect();
        if e.is_empty() { return err(StatusCode::BAD_REQUEST, "ไม่ทราบชนิดไฟล์ที่อัปโหลด"); }
        (f, e)
    } else { (name.clone(), ext.clone()) };
    let nv = NewVersion {
        code: &code, filename: &vname, ext: &vext, source: if upload { "upload" } else { "edit" }, note: &note,
        signer_code: &u.emp_code, signer_name: &u.name, signer_role: &u.position,
        rev: "", effective_date: "", change_id: 0, approved_by: "", approved_at: "",
    };

    // บันทึกเวอร์ชันในระบบ + ซิงก์ต้นฉบับบนดิสก์
    let (version, v) = match save_new_version(&st, &nv, &bytes, Some(abs.as_path())).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    if let Ok(client) = st.db.get().await {
        let _ = eform::audit(&client, &u, if upload { "overview_upload" } else { "overview_save" }, "overview_file", &rel, &format!("v{version} {vname}")).await;
    }

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

/// ประวัติเวอร์ชันของไฟล์ภาพรวม (รวมไฟล์ที่ไม่อยู่ในทะเบียน) — GET /api/overview/history?path=
async fn overview_history(Query(q): Query<HashMap<String, String>>, State(st): State<S>) -> Response {
    let rel = q.get("path").cloned().unwrap_or_default();
    if let Err(r) = resolve_overview_path(&st, &rel) { return r; }
    let code = find_code_by_path(&st, &rel).unwrap_or_else(|| ov_doc_code(&rel));
    Json(json!({ "code": code, "registered": st.by_code.contains_key(&code), "versions": list_versions(&st, &code).await })).into_response()
}

/// สรุปเวอร์ชันล่าสุดของทุกไฟล์ภาพรวม: path → { version, count, by, at }
async fn overview_latest(State(st): State<S>) -> Response {
    let latest = latest_by_doc(&st).await;
    let mut out = serde_json::Map::new();
    for sec in st.overview.as_array().cloned().unwrap_or_default() {
        for g in sec["groups"].as_array().cloned().unwrap_or_default() {
            for it in g["items"].as_array().cloned().unwrap_or_default() {
                let rel = it["path"].as_str().unwrap_or("").to_string();
                let code = find_code_by_path(&st, &rel).unwrap_or_else(|| ov_doc_code(&rel));
                if let Some(v) = latest.get(&code) { out.insert(rel, v.clone()); }
            }
        }
    }
    Json(Value::Object(out)).into_response()
}

/// เวอร์ชันล่าสุดต่อเอกสาร: { version, count, by, at, source } (เฉพาะที่มีการแก้/อัปโหลดแล้ว)
async fn latest_by_doc(st: &S) -> HashMap<String, Value> {
    let mut m = HashMap::new();
    let Ok(c) = st.db.get().await else { return m };
    if let Ok(rows) = c.query(
        "SELECT DISTINCT ON (doc_code) doc_code,version,signer_name,signed_at,source,(SELECT COUNT(*) FROM versions x WHERE x.doc_code=v.doc_code)
         FROM versions v ORDER BY doc_code, version DESC", &[],
    ).await {
        for r in rows {
            let v: i64 = r.get(1);
            if v < 2 { continue; }
            m.insert(r.get::<_, String>(0), json!({ "version": v, "by": r.get::<_, String>(2), "at": r.get::<_, String>(3), "source": r.get::<_, String>(4), "count": r.get::<_, i64>(5) }));
        }
    }
    m
}

async fn content(Path(code): Path<String>, Query(q): Query<HashMap<String, String>>, State(st): State<S>, Extension(u): Extension<eform::AuthUser>) -> Response {
    let (ver, abs, ext, _fn) = match resolve(&st, &code, q.get("v")).await { Ok(x) => x, Err(r) => return r };
    acks::mark_opened(&st, &u, &code, ver).await;
    let mut body = preview_abs(&st, &abs, &ext).await;
    body["version"] = json!(ver);
    Json(body).into_response()
}

async fn file(Path(code): Path<String>, Query(q): Query<HashMap<String, String>>, State(st): State<S>, Extension(u): Extension<eform::AuthUser>) -> Response {
    let (v, abs, ext, filename) = match resolve(&st, &code, q.get("v")).await { Ok(x) => x, Err(r) => return r };
    acks::mark_opened(&st, &u, &code, v).await;
    let bytes = match fs::read(&abs) { Ok(b) => b, Err(_) => return err(StatusCode::NOT_FOUND, "อ่านไฟล์ไม่ได้") };
    // กันการนำไปใช้โดยไม่ตั้งใจ (7.5.3.2 d): ไฟล์ที่ไม่ใช่ฉบับใช้งานปัจจุบันติดป้ายไว้ที่ชื่อไฟล์
    let filename = doccontrol::download_name(&st, &code, v, &filename).await;
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
    let clauses: Value = fs::read_to_string(docs_root.join("clauses.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(json!({ "clauses": [] }));
    let by_code: HashMap<String, Value> = manifest.iter().filter_map(|d| d["code"].as_str().map(|c| (c.to_string(), d.clone()))).collect();

    let pool = make_pool();
    init_db(&pool).await;
    migrate_from_sqlite(&pool, &docs_root.join("versions.db")).await;

    let state: S = Arc::new(AppState {
        manifest, by_code, overview: overview_json, clauses, docs_root, storage,
        conv_port: std::env::var("CONVERTER_PORT").unwrap_or_else(|_| "8081".into()),
        db: pool.clone(),
        login_hits: Mutex::new(HashMap::new()),
    });
    eform::seed(&pool, &state.docs_root).await;
    freeze_originals(&state).await;
    {
        let pool_b = pool.clone();
        let dir = state.docs_root.join("backups");
        tokio::spawn(async move {
            loop {
                eform::backup_once(&pool_b, &dir).await;
                audittrail::db_backup(&dir).await;
                tokio::time::sleep(Duration::from_secs(86_400)).await;
            }
        });
    }

    let app = Router::new()
        .route("/api/health", get(health))
        .route("/api/docs", get(docs_list))
        .route("/api/docs/:code/versions", get(versions))
        .route("/api/docs/:code/changes", get(doccontrol::doc_changes).post(doccontrol::create))
        .route("/api/docs/:code/obsolete", axum::routing::post(doccontrol::obsolete))
        .route("/api/docs/:code/reinstate", axum::routing::post(doccontrol::reinstate))
        .route("/api/changes", get(doccontrol::list))
        .route("/api/changes/:id", get(doccontrol::detail))
        .route("/api/changes/:id/content", get(doccontrol::content))
        .route("/api/changes/:id/file", get(doccontrol::file))
        .route("/api/changes/:id/act", axum::routing::post(doccontrol::act))
        .route("/api/changes/:id/resubmit", axum::routing::post(doccontrol::resubmit))
        .route("/api/changes/:id/cancel", axum::routing::post(doccontrol::cancel))
        .route("/api/docs/:code/content", get(content))
        .route("/api/docs/:code/file", get(file))
        .route("/api/stats", get(stats))
        .route("/api/readiness", get(readiness::get))
        .route("/api/readiness/runs", get(readiness::runs).post(readiness::save_run))
        .route("/api/mr", get(mreview::list).post(mreview::create))
        .route("/api/mr/:id", get(mreview::detail).put(mreview::update))
        .route("/api/mr/:id/input", axum::routing::put(mreview::save_input))
        .route("/api/mr/:id/sync", axum::routing::post(mreview::sync))
        .route("/api/mr/:id/act", axum::routing::post(mreview::act))
        .route("/api/mr/:id/revision", axum::routing::post(mreview::revise))
        .route("/api/mr/:id/actions", axum::routing::post(mreview::action_create))
        .route("/api/mr/:id/export", get(mreview::export))
        .route("/api/mr/actions/:aid", axum::routing::put(mreview::action_update))
        .route("/api/actions", get(capa::list).post(capa::create))
        .route("/api/actions/:id", get(capa::get).put(capa::update))
        .route("/api/actions/:id/act", axum::routing::post(capa::act))
        .route("/api/audit-log", get(audittrail::list))
        .route("/api/training/matrix", get(training::matrix))
        .route("/api/training/history/:code", get(training::history))
        .route("/api/training/reqs", axum::routing::post(training::create_req))
        .route("/api/training/reqs/:id", axum::routing::put(training::update_req))
        .route("/api/training/records", axum::routing::post(training::record))
        .route("/api/acks", get(acks::overview))
        .route("/api/acks/mine", get(acks::mine))
        .route("/api/acks/bulk", axum::routing::post(acks::request_bulk))
        .route("/api/acks/:id/ack", axum::routing::post(acks::acknowledge))
        .route("/api/docs/:code/acks", get(acks::doc))
        .route("/api/docs/:code/acks/request", axum::routing::post(acks::request))
        .route("/api/docs/:code/distribution", axum::routing::put(acks::set_distribution))
        .route("/api/ia", get(internalaudit::list).post(internalaudit::create))
        .route("/api/ia/auditors", axum::routing::post(internalaudit::set_auditor))
        .route("/api/ia/:id", get(internalaudit::get).put(internalaudit::update))
        .route("/api/ia/:id/act", axum::routing::post(internalaudit::act))
        .route("/api/ia/:id/items", axum::routing::post(internalaudit::save_item))
        .route("/api/ia/:id/items/:iid/car", axum::routing::post(internalaudit::raise_car))
        .route("/api/overview", get(overview))
        .route("/api/overview/file", get(overview_file))
        .route("/api/overview/content", get(overview_content))
        .route("/api/overview/save", axum::routing::post(overview_save))
        .route("/api/overview/history", get(overview_history))
        .route("/api/overview/latest", get(overview_latest))
        .route("/api/auth/login", axum::routing::post(eform::login))
        .route("/api/auth/logout", axum::routing::post(eform::logout))
        .route("/api/auth/me", get(eform::me))
        .route("/api/auth/pin", axum::routing::post(eform::change_pin))
        .route("/api/employees", get(eform::employees_list).post(eform::employees_save))
        .route("/api/forms/schemas", get(eform::schemas_list).post(eform::schema_save))
        .route("/api/forms/schemas/:code", get(eform::schema_get))
        .route("/api/forms/records", get(eform::records_list).post(eform::record_create))
        .route("/api/forms/records/:id", get(eform::record_get).put(eform::record_save))
        .route("/api/forms/records/:id/submit", axum::routing::post(eform::record_submit))
        .route("/api/forms/records/:id/act", axum::routing::post(eform::record_act))
        .route("/api/forms/records/:id/cancel", axum::routing::post(eform::record_cancel))
        .route("/api/forms/records/:id/amend", axum::routing::post(eform::record_amend))
        .route("/api/forms/records/:id/files", axum::routing::post(eform::file_add))
        .route("/api/forms/records/:id/print", get(eform::record_print))
        .route("/api/forms/files/:fid", get(eform::file_get))
        .route("/api/forms/pending", get(eform::pending))
        .route("/api/forms/training", get(eform::training_list))
        .route("/api/forms/stats", get(eform::form_stats))
        .route("/api/forms/places", get(eform::places))
        .route("/api/forms/previous", get(eform::copy_previous))
        .route("/api/public/checkin/:token", get(eform::checkin_info))
        .route("/api/public/checkin", axum::routing::post(eform::checkin))
        .layer(DefaultBodyLimit::max(64 * 1024 * 1024))
        .layer(axum::middleware::from_fn_with_state(state.clone(), eform::require_auth))
        .with_state(state);

    let port = std::env::var("API_PORT").unwrap_or_else(|_| "8085".into());
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await.unwrap();
    println!("\n  ✅ iso-api (Rust/Axum + PostgreSQL) พร้อมที่ http://localhost:{port}\n");
    axum::serve(listener, app).await.unwrap();
}
