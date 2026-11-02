// แก้ไขเอกสารบนเว็บด้วย ONLYOFFICE Docs (รันเป็น container iso-office บน server เดียวกัน)
//   1) ผู้ใช้กด "แก้ไขเอกสาร" → สร้างรอบแก้ไข (สำเนาทำงานของฉบับล่าสุด หรือฉบับร่างที่ถูกตีกลับ) + token ลับ
//   2) หน้าเว็บฝังตัวแก้ไข ONLYOFFICE ด้วยค่าตั้งที่ API ลงนาม JWT ให้ — ONLYOFFICE ดึงไฟล์จาก /api/public/edit/<token>
//   3) กดบันทึก (Ctrl+S) → ONLYOFFICE เรียก callback /api/public/office/<token> → API ดึงไฟล์ที่แก้มาทับสำเนาทำงาน
//   4) หน้าเว็บดู PDF ของสำเนาทำงาน แล้วกด "ส่งขออนุมัติ" (ลงนามด้วย PIN) → กลายเป็นคำขอ DCR ตามขั้นตอนเดิม
// token เก็บเป็น SHA-256 เท่านั้น ใช้ได้กับไฟล์รอบนี้ไฟล์เดียว และหมดอายุเมื่อส่ง/ยกเลิก หรือครบ 12 ชั่วโมง
// ตั้งค่า: ONLYOFFICE_URL (ที่อยู่ที่เบราว์เซอร์เปิด), ONLYOFFICE_INTERNAL_URL (ที่อยู่ภายใน network ของ container),
//          ONLYOFFICE_JWT_SECRET (ตรงกับ JWT_SECRET ของ iso-office), ISO_INTERNAL_URL (ที่อยู่ API ที่ iso-office เรียกถึง)
use crate::eform::{self, AuthUser};
use crate::{doccontrol, err, mime_of, now_iso, pct, pdfview, sha256_hex, S};
use axum::{
    body::{Body, Bytes},
    http::HeaderMap,
    extract::{Extension, Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::fs;

pub const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS edit_sessions(
  id BIGSERIAL PRIMARY KEY,
  token_hash TEXT NOT NULL UNIQUE,
  emp_code TEXT NOT NULL,
  emp_name TEXT NOT NULL,
  doc_code TEXT NOT NULL,
  change_id BIGINT NOT NULL DEFAULT 0,
  ext TEXT NOT NULL,
  filename TEXT NOT NULL,
  stored_name TEXT NOT NULL,
  base_sha TEXT NOT NULL,
  sha256 TEXT NOT NULL,
  size BIGINT NOT NULL DEFAULT 0,
  saves INT NOT NULL DEFAULT 0,
  status TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  saved_at TEXT NOT NULL DEFAULT '',
  expires_at TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS idx_es_doc ON edit_sessions(doc_code, emp_code) WHERE status='open';
-- ไฟล์ในคลังภาพรวมองค์กร (ไม่อยู่ในทะเบียน): บันทึกเป็นเวอร์ชันใหม่ได้เลย ไม่ผ่าน DCR
ALTER TABLE edit_sessions ADD COLUMN IF NOT EXISTS ov_path TEXT NOT NULL DEFAULT '';
";

const TTL_HOURS: i64 = 12;
const MAX_BYTES: usize = 40 * 1024 * 1024;
const COLS: &str = "id,emp_code,emp_name,doc_code,change_id,ext,filename,stored_name,base_sha,sha256,size,saves,status,created_at,updated_at,saved_at,expires_at,ov_path";

struct Sess {
    id: i64, emp_code: String, doc_code: String, change_id: i64, ext: String, filename: String,
    stored_name: String, base_sha: String, sha256: String, status: String, expires_at: String, ov_path: String, json: Value,
}

fn row_sess(r: &tokio_postgres::Row) -> Sess {
    let base_sha: String = r.get(8);
    let sha: String = r.get(9);
    let json = json!({
        "id": r.get::<_, i64>(0), "empCode": r.get::<_, String>(1), "empName": r.get::<_, String>(2),
        "docCode": r.get::<_, String>(3), "changeId": r.get::<_, i64>(4), "ext": r.get::<_, String>(5),
        "filename": r.get::<_, String>(6), "sha256": sha, "size": r.get::<_, i64>(10), "saves": r.get::<_, i32>(11),
        "changed": sha != base_sha, "status": r.get::<_, String>(12),
        "createdAt": r.get::<_, String>(13), "updatedAt": r.get::<_, String>(14), "savedAt": r.get::<_, String>(15),
        "expiresAt": r.get::<_, String>(16), "ovPath": r.get::<_, String>(17),
    });
    Sess {
        id: r.get(0), emp_code: r.get(1), doc_code: r.get(3), change_id: r.get(4), ext: r.get(5), filename: r.get(6),
        stored_name: r.get(7), base_sha, sha256: sha, status: r.get(12), expires_at: r.get(16), ov_path: r.get(17), json,
    }
}

fn db_err() -> Response { err(StatusCode::INTERNAL_SERVER_ERROR, "เชื่อมต่อฐานข้อมูลไม่ได้") }

fn edits_dir(st: &S) -> std::path::PathBuf {
    let d = st.storage.join("edits");
    let _ = fs::create_dir_all(&d);
    d
}

fn expiry() -> String { (chrono::Utc::now() + chrono::Duration::hours(TTL_HOURS)).to_rfc3339() }

/// ชนิดไฟล์ที่แก้บนเว็บ (ONLYOFFICE) ได้
pub(crate) const EDIT_EXTS: [&str; 7] = ["docx", "doc", "xlsx", "xls", "pptx", "ppt", "pdf"];

/// ไฟล์ Office/PDF จริงหรือไม่ — docx/xlsx/pptx เป็น zip, doc/xls/ppt เป็น OLE (กันไฟล์อื่นถูกส่งมาแทน)
fn looks_like(ext: &str, b: &[u8]) -> bool {
    match ext {
        "docx" | "xlsx" | "xlsm" | "pptx" => b.starts_with(b"PK\x03\x04"),
        "doc" | "xls" | "ppt" => b.starts_with(&[0xD0, 0xCF, 0x11, 0xE0]),
        "pdf" => b.starts_with(b"%PDF"),
        _ => false,
    }
}

/// รูปแบบใหม่ที่ ONLYOFFICE บันทึกกลับมาแทนรูปแบบเก่า (.doc → .docx ฯลฯ)
fn modern_of(ext: &str) -> Option<&'static str> {
    match ext { "doc" => Some("docx"), "xls" => Some("xlsx"), "ppt" => Some("pptx"), _ => None }
}

fn write_atomic(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("part");
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path)
}

async fn load(c: &impl deadpool_postgres::GenericClient, id: i64) -> Option<Sess> {
    c.query_opt(&format!("SELECT {COLS} FROM edit_sessions WHERE id=$1"), &[&id]).await.ok()?.map(|r| row_sess(&r))
}

/// รอบแก้ไขของผู้ใช้คนนี้เท่านั้น
async fn own(st: &S, u: &AuthUser, id: i64) -> Result<Sess, Response> {
    let Ok(client) = st.db.get().await else { return Err(db_err()) };
    match load(&client, id).await {
        Some(s) if s.emp_code == u.emp_code => Ok(s),
        Some(_) => Err(err(StatusCode::FORBIDDEN, "รอบแก้ไขนี้เป็นของผู้ใช้อื่น")),
        None => Err(err(StatusCode::NOT_FOUND, "ไม่พบรอบแก้ไข")),
    }
}

/// ฉบับที่ใช้เป็นจุดเริ่มแก้: ฉบับร่างของ DCR ที่ถูกตีกลับ หรือฉบับล่าสุดที่ใช้งานอยู่
async fn source_file(st: &S, u: &AuthUser, client: &impl deadpool_postgres::GenericClient, code: &str, change_id: i64)
    -> Result<(Vec<u8>, String, String), Response> {
    if change_id > 0 {
        let r = client.query_opt("SELECT doc_code,status,prepared_by,stored_name,ext,filename FROM doc_changes WHERE id=$1", &[&change_id])
            .await.ok().flatten().ok_or_else(|| err(StatusCode::NOT_FOUND, "ไม่พบคำขอเปลี่ยนแปลง"))?;
        let (dc, status, by): (String, String, String) = (r.get(0), r.get(1), r.get(2));
        if dc != code { return Err(err(StatusCode::BAD_REQUEST, "คำขอนี้ไม่ใช่ของเอกสารนี้")); }
        if status != "returned" || by != u.emp_code {
            return Err(err(StatusCode::FORBIDDEN, "แก้ฉบับร่างได้เฉพาะผู้จัดทำ เมื่อคำขอถูกตีกลับ"));
        }
        let stored: String = r.get(3);
        let bytes = fs::read(st.storage.join("changes").join(&stored)).map_err(|_| err(StatusCode::NOT_FOUND, "ไม่พบไฟล์ฉบับร่างในระบบ"))?;
        return Ok((bytes, r.get(4), r.get(5)));
    }
    if u.role == "viewer" { return Err(err(StatusCode::FORBIDDEN, "บัญชีผู้อ่านอย่างเดียวแก้ไขเอกสารไม่ได้")); }
    if doccontrol::is_obsolete(client, code).await { return Err(err(StatusCode::CONFLICT, "เอกสารนี้ถูกยกเลิกใช้แล้ว — ต้องนำกลับมาใช้ก่อนจึงจะแก้ไขได้")); }
    if let Ok(Some(r)) = client.query_opt("SELECT ref_no FROM doc_changes WHERE doc_code=$1 AND status IN ('approve','returned')", &[&code]).await {
        return Err(err(StatusCode::CONFLICT, &format!("มีคำขอ {} ที่ยังไม่จบอยู่แล้ว — ดำเนินการให้เสร็จหรือยกเลิกก่อน", r.get::<_, String>(0))));
    }
    let (_, abs, ext, filename) = crate::resolve(st, code, None).await?;
    let bytes = fs::read(&abs).map_err(|_| err(StatusCode::NOT_FOUND, "อ่านไฟล์ไม่ได้"))?;
    Ok((bytes, ext.to_lowercase(), filename))
}

#[derive(Deserialize)]
#[allow(non_snake_case)]
pub struct StartReq { changeId: Option<i64> }

/// POST /api/docs/:code/edit — เริ่ม (หรือเปิดต่อ) รอบแก้ไข → คืนค่าตั้งของตัวแก้ไข ONLYOFFICE
/// เปิดซ้ำ = ใช้สำเนาทำงานเดิม (งานที่บันทึกไว้ไม่หาย) แต่ออก token ใหม่ ตัวเก่าใช้ไม่ได้อีก
pub async fn start(Path(code): Path<String>, State(st): State<S>, Extension(u): Extension<AuthUser>, Json(b): Json<StartReq>) -> Response {
    if !st.by_code.contains_key(&code) { return err(StatusCode::NOT_FOUND, "ไม่พบเอกสาร"); }
    let change_id = b.changeId.unwrap_or(0);
    let Ok(client) = st.db.get().await else { return db_err() };
    let src = source_file(&st, &u, &client, &code, change_id);
    open_session(&st, &u, &client, &code, change_id, "", src).await
}

/// เปิดรอบแก้ไขต่อ (ถ้ามีของผู้ใช้คนนี้ค้างอยู่) หรือสร้างใหม่จากไฟล์ต้นทาง `src`
/// `src` ตรวจสิทธิ์ด้วย — เรียกทุกครั้งแม้เปิดต่อ (ระหว่างนี้อาจมีคนส่ง DCR ไปแล้ว หรือ DCR ถูกยกเลิก)
async fn open_session(
    st: &S, u: &AuthUser, client: &deadpool_postgres::Object, code: &str, change_id: i64, ov_path: &str,
    src: impl std::future::Future<Output = Result<(Vec<u8>, String, String), Response>>,
) -> Response {
    let token = eform::new_token("edit");
    let hash = sha256_hex(token.as_bytes());
    let now = now_iso();
    let exp = expiry();

    let existing = client.query_opt(
        &format!("SELECT {COLS} FROM edit_sessions WHERE doc_code=$1 AND emp_code=$2 AND change_id=$3 AND ov_path=$4 AND status='open' ORDER BY id DESC LIMIT 1"),
        &[&code, &u.emp_code, &change_id, &ov_path],
    ).await.ok().flatten().map(|r| row_sess(&r));
    let source = src.await;
    if let Some(s) = existing.filter(|s| edits_dir(st).join(&s.stored_name).exists()) {
        if let Err(r) = source { return r; }
        let _ = client.execute("UPDATE edit_sessions SET token_hash=$2,expires_at=$3,updated_at=$4 WHERE id=$1", &[&s.id, &hash, &exp, &now]).await;
        let Some(s) = load(client, s.id).await else { return db_err() };
        let others = others_editing(client, code, &u.emp_code).await;
        return started(st, u, &s, &token, others);
    }

    let (bytes, ext, filename) = match source { Ok(x) => x, Err(r) => return r };
    if !EDIT_EXTS.contains(&ext.as_str()) || !looks_like(&ext, &bytes) {
        return err(StatusCode::BAD_REQUEST, "แก้ไขบนเว็บได้เฉพาะไฟล์ Word, Excel, PowerPoint และ PDF");
    }
    let sha = sha256_hex(&bytes);
    let size = bytes.len() as i64;
    let row = client.query_one(
        "INSERT INTO edit_sessions(token_hash,emp_code,emp_name,doc_code,change_id,ext,filename,stored_name,base_sha,sha256,size,saves,status,created_at,updated_at,saved_at,expires_at,ov_path)
         VALUES($1,$2,$3,$4,$5,$6,$7,'',$8,$8,$9,0,'open',$10,$10,'',$11,$12) RETURNING id",
        &[&hash, &u.emp_code, &u.name, &code, &change_id, &ext, &filename, &sha, &size, &now, &exp, &ov_path],
    ).await;
    let id: i64 = match row { Ok(r) => r.get(0), Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()) };
    let stored = format!("{id}.{ext}");
    if let Err(e) = write_atomic(&edits_dir(st).join(&stored), &bytes) {
        let _ = client.execute("DELETE FROM edit_sessions WHERE id=$1", &[&id]).await;
        return err(StatusCode::INTERNAL_SERVER_ERROR, &format!("สร้างสำเนาสำหรับแก้ไขไม่ได้: {e}"));
    }
    let _ = client.execute("UPDATE edit_sessions SET stored_name=$2 WHERE id=$1", &[&id, &stored]).await;
    let _ = eform::audit(client, u, "edit_start", "document", code, &format!("รอบแก้ไข #{id}")).await;
    let Some(s) = load(client, id).await else { return db_err() };
    let others = others_editing(client, code, &u.emp_code).await;
    started(st, u, &s, &token, others)
}

#[derive(Deserialize)]
pub struct OvStartReq { path: Option<String> }

/// POST /api/overview/edit — แก้ไฟล์ภาพรวมองค์กร (ไม่อยู่ในทะเบียน) ด้วยตัวแก้ไขบนเว็บ
/// บันทึกแล้วกด "บันทึกเป็นเวอร์ชันใหม่" (commit) — ไม่ผ่าน DCR เหมือน /api/overview/save เดิม
pub async fn start_overview(State(st): State<S>, Extension(u): Extension<AuthUser>, Json(b): Json<OvStartReq>) -> Response {
    let rel = b.path.unwrap_or_default();
    if u.role == "viewer" { return err(StatusCode::FORBIDDEN, "บัญชีผู้อ่านอย่างเดียวแก้ไขไฟล์ไม่ได้"); }
    if let Some(code) = crate::find_code_by_path(&st, &rel) {
        return err(StatusCode::CONFLICT, &format!("{code} เป็นเอกสารควบคุมในทะเบียน — แก้จากหน้าเอกสารในทะเบียน (ผ่าน DCR)"));
    }
    let (abs, ext, name) = match crate::resolve_overview_path(&st, &rel) { Ok(x) => x, Err(r) => return r };
    let code = match crate::overview_doc_code(&st, &rel, &abs, &name, &ext).await { Ok(c) => c, Err(r) => return r };
    let Ok(client) = st.db.get().await else { return db_err() };
    let src = async {
        let (live, vext, filename, _, _) = crate::overview_live_file(&st, &rel, None).await?;
        let bytes = fs::read(&live).map_err(|_| err(StatusCode::NOT_FOUND, "อ่านไฟล์ไม่ได้"))?;
        Ok::<_, Response>((bytes, vext.to_lowercase(), filename))
    };
    open_session(&st, &u, &client, &code, 0, &rel, src).await
}

/// GET /api/overview/edit?path= — รอบแก้ไขที่ยังเปิดอยู่ของผู้ใช้กับไฟล์ภาพรวมนี้ (null = ไม่มี)
pub async fn mine_overview(axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>, State(st): State<S>, Extension(u): Extension<AuthUser>) -> Response {
    let rel = q.get("path").cloned().unwrap_or_default();
    let Ok(client) = st.db.get().await else { return db_err() };
    let s = client.query_opt(
        &format!("SELECT {COLS} FROM edit_sessions WHERE ov_path=$1 AND emp_code=$2 AND status='open' ORDER BY id DESC LIMIT 1"),
        &[&rel, &u.emp_code],
    ).await.ok().flatten().map(|r| row_sess(&r).json);
    Json(s.unwrap_or(Value::Null)).into_response()
}

#[derive(Deserialize)]
pub struct CommitReq { note: Option<String> }

/// POST /api/edit/:id/commit — ไฟล์ภาพรวม: บันทึกสำเนาที่แก้เป็นเวอร์ชันใหม่ (เวอร์ชันเดิมยังเก็บไว้)
pub async fn commit(Path(id): Path<i64>, State(st): State<S>, Extension(u): Extension<AuthUser>, Json(b): Json<CommitReq>) -> Response {
    let Ok(client) = st.db.get().await else { return db_err() };
    let Some(s) = load(&client, id).await else { return err(StatusCode::NOT_FOUND, "ไม่พบรอบแก้ไข") };
    if s.ov_path.is_empty() { return err(StatusCode::BAD_REQUEST, "เอกสารควบคุมต้องส่งขออนุมัติ (DCR)"); }
    if crate::find_code_by_path(&st, &s.ov_path).is_some() {
        return err(StatusCode::CONFLICT, "ไฟล์นี้ขึ้นทะเบียนเป็นเอกสารควบคุมแล้ว — ต้องส่งคำขอเปลี่ยนแปลง (DCR)");
    }
    let (bytes, ext, filename) = match take(&st, &client, &u, id, &s.doc_code, 0).await { Ok(x) => x, Err(r) => return r };
    let (abs, _, _) = match crate::resolve_overview_path(&st, &s.ov_path) { Ok(x) => x, Err(r) => return r };
    let note = b.note.unwrap_or_default().trim().to_string();
    let nv = crate::NewVersion {
        code: &s.doc_code, filename: &filename, ext: &ext, source: "edit", note: &note,
        signer_code: &u.emp_code, signer_name: &u.name, signer_role: &u.position,
        rev: "", effective_date: "", change_id: 0, approved_by: "", approved_at: "",
    };
    let (version, v) = match crate::save_new_version(&st, &nv, &bytes, Some(abs.as_path())).await { Ok(x) => x, Err(r) => return r };
    finish(&st, &client, id).await;
    let _ = eform::audit(&client, &u, "overview_save", "overview_file", &s.ov_path, &format!("v{version} {filename} (แก้บนเว็บ)")).await;
    Json(json!({ "ok": true, "latestVersion": version, "version": v })).into_response()
}


fn started(st: &S, u: &AuthUser, s: &Sess, token: &str, others: Value) -> Response {
    let office = office_config(st, u, s, token);
    Json(json!({ "session": s.json, "office": office, "others": others })).into_response()
}

// ---------- ONLYOFFICE ----------

pub(crate) fn env_url(k: &str) -> String { std::env::var(k).unwrap_or_default().trim_end_matches('/').to_string() }

fn b64url(b: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b)
}

/// HMAC-SHA256 (RFC 2104) — ใช้ลงนาม/ตรวจ JWT กับ ONLYOFFICE
fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut k = [0u8; 64];
    if key.len() > 64 { k[..32].copy_from_slice(&Sha256::digest(key)); } else { k[..key.len()].copy_from_slice(key); }
    let (mut ipad, mut opad) = ([0x36u8; 64], [0x5cu8; 64]);
    for i in 0..64 { ipad[i] ^= k[i]; opad[i] ^= k[i]; }
    let inner = Sha256::new().chain_update(ipad).chain_update(msg).finalize();
    Sha256::new().chain_update(opad).chain_update(inner).finalize().into()
}

pub(crate) fn jwt_sign(secret: &str, payload: &Value) -> String {
    let head = b64url(br#"{"alg":"HS256","typ":"JWT"}"#);
    let body = b64url(payload.to_string().as_bytes());
    let sig = b64url(&hmac_sha256(secret.as_bytes(), format!("{head}.{body}").as_bytes()));
    format!("{head}.{body}.{sig}")
}

/// ตรวจลายเซ็น HS256 แล้วคืน payload (ไม่ผ่าน = None)
fn jwt_verify(secret: &str, jwt: &str) -> Option<Value> {
    use base64::Engine;
    let mut it = jwt.split('.');
    let (h, b, sig) = (it.next()?, it.next()?, it.next()?);
    if it.next().is_some() { return None; }
    let want = b64url(&hmac_sha256(secret.as_bytes(), format!("{h}.{b}").as_bytes()));
    // เทียบแบบเวลาคงที่
    if want.len() != sig.len() || want.bytes().zip(sig.bytes()).fold(0u8, |a, (x, y)| a | (x ^ y)) != 0 { return None; }
    let header: Value = serde_json::from_slice(&base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(h).ok()?).ok()?;
    if header["alg"] != "HS256" { return None; }
    serde_json::from_slice(&base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(b).ok()?).ok()
}

/// ค่าตั้งของ DocsAPI.DocEditor (null = ยังไม่ได้ตั้งค่า ONLYOFFICE บน server นี้)
fn office_config(st: &S, u: &AuthUser, s: &Sess, token: &str) -> Value {
    let (public, internal_api) = (env_url("ONLYOFFICE_URL"), env_url("ISO_INTERNAL_URL"));
    let secret = std::env::var("ONLYOFFICE_JWT_SECRET").unwrap_or_default();
    if public.is_empty() || internal_api.is_empty() || secret.is_empty() { return Value::Null; }
    let doc_type = match s.ext.as_str() { "xls" | "xlsx" => "cell", "ppt" | "pptx" => "slide", "pdf" => "pdf", _ => "word" };
    let title = st.by_code.get(&s.doc_code).map(|d| format!("{} {}", s.doc_code, d["name"].as_str().unwrap_or(""))).unwrap_or_else(|| s.filename.clone());
    // key เปลี่ยนเมื่อไฟล์เปลี่ยน — ONLYOFFICE ใช้ key แคชเอกสาร เปิดใหม่หลังบันทึกต้องได้ฉบับล่าสุด
    let key = format!("iso-{}-{}", s.id, &s.sha256[..16]);
    let mut config = json!({
        "document": {
            "fileType": s.ext, "key": key, "title": s.filename,
            "url": format!("{internal_api}/api/public/edit/{token}"),
            "permissions": { "edit": true, "download": true, "print": true, "comment": true, "review": true },
        },
        "documentType": doc_type,
        "editorConfig": {
            "callbackUrl": format!("{internal_api}/api/public/office/{token}"),
            "lang": "th", "region": "th-TH",
            "user": { "id": u.emp_code, "name": u.name },
            // กดบันทึก = ส่งไฟล์เข้าสำเนาทำงานทันที (ไม่ต้องรอปิดตัวแก้ไข)
            "customization": { "forcesave": true, "autosave": true, "compactHeader": true, "feedback": false, "help": false, "uiTheme": "theme-light" },
        },
        "height": "100%", "width": "100%", "type": "desktop",
    });
    config["token"] = json!(jwt_sign(&secret, &config));
    json!({ "url": public, "config": config, "title": title.trim() })
}

/// GET /api/docs/:code/edit — รอบแก้ไขที่ยังเปิดอยู่ของผู้ใช้กับเอกสารนี้ (null = ไม่มี)
pub async fn mine(Path(code): Path<String>, State(st): State<S>, Extension(u): Extension<AuthUser>) -> Response {
    let Ok(client) = st.db.get().await else { return db_err() };
    let s = client.query_opt(
        &format!("SELECT {COLS} FROM edit_sessions WHERE doc_code=$1 AND emp_code=$2 AND status='open' ORDER BY id DESC LIMIT 1"),
        &[&code, &u.emp_code],
    ).await.ok().flatten().map(|r| row_sess(&r).json);
    Json(s.unwrap_or(Value::Null)).into_response()
}

/// GET /api/edit/:id — สถานะ (หน้าเว็บถามซ้ำเพื่อรู้ว่ากดบันทึกในตัวแก้ไขแล้ว)
pub async fn status(Path(id): Path<i64>, State(st): State<S>, Extension(u): Extension<AuthUser>) -> Response {
    match own(&st, &u, id).await { Ok(s) => Json(s.json).into_response(), Err(r) => r }
}

/// GET /api/edit/:id/pdf — ดูสำเนาทำงานเป็น PDF (แคชตาม SHA ของไฟล์ บันทึกใหม่ = PDF ใหม่)
pub async fn pdf(Path(id): Path<i64>, State(st): State<S>, Extension(u): Extension<AuthUser>) -> Response {
    let s = match own(&st, &u, id).await { Ok(s) => s, Err(r) => return r };
    if s.ext == "pdf" {
        return match fs::read(edits_dir(&st).join(&s.stored_name)) {
            Ok(b) => pdfview::pdf_response(b, &format!("ร่างแก้ไข - {}", s.filename)),
            Err(_) => err(StatusCode::NOT_FOUND, "ไม่พบไฟล์ที่แก้ไขในระบบ"),
        };
    }
    if !pdfview::convertible(&s.ext) { return err(StatusCode::BAD_REQUEST, "ไฟล์ชนิดนี้แปลงเป็น PDF ไม่ได้"); }
    let title = st.by_code.get(&s.doc_code).map(|d| format!("{} {}", s.doc_code, d["name"].as_str().unwrap_or(""))).unwrap_or_else(|| s.doc_code.clone());
    match pdfview::pdf_bytes(&st, &edits_dir(&st).join(&s.stored_name), &s.ext, title.trim()).await {
        Ok(b) => pdfview::pdf_response(b, &format!("ร่างแก้ไข - {}", s.filename)),
        Err(e) => err(StatusCode::BAD_GATEWAY, &e),
    }
}

/// POST /api/edit/:id/cancel — ทิ้งรอบแก้ไข (ไฟล์ที่บันทึกไว้ไม่ถูกส่งเป็นคำขอ)
pub async fn cancel(Path(id): Path<i64>, State(st): State<S>, Extension(u): Extension<AuthUser>) -> Response {
    let s = match own(&st, &u, id).await { Ok(s) => s, Err(r) => return r };
    let Ok(client) = st.db.get().await else { return db_err() };
    let n = client.execute("UPDATE edit_sessions SET status='cancelled',updated_at=$2 WHERE id=$1 AND status='open'", &[&id, &now_iso()]).await.unwrap_or(0);
    if n == 0 { return err(StatusCode::CONFLICT, "รอบแก้ไขนี้ปิดไปแล้ว"); }
    let _ = fs::remove_file(edits_dir(&st).join(&s.stored_name));
    let _ = eform::audit(&client, &u, "edit_cancel", "document", &s.doc_code, &format!("รอบแก้ไข #{id}")).await;
    Json(json!({ "ok": true })).into_response()
}

/// ใช้ตอนส่งคำขอ DCR: ไฟล์จากรอบแก้ไข (ต้องเป็นของผู้ส่ง ยังเปิดอยู่ ตรงเอกสาร/คำขอ และมีการแก้จริง)
pub(crate) async fn take(st: &S, client: &impl deadpool_postgres::GenericClient, u: &AuthUser, id: i64, code: &str, change_id: i64)
    -> Result<(Vec<u8>, String, String), Response> {
    let s = load(client, id).await.ok_or_else(|| err(StatusCode::NOT_FOUND, "ไม่พบรอบแก้ไข"))?;
    if s.emp_code != u.emp_code { return Err(err(StatusCode::FORBIDDEN, "รอบแก้ไขนี้เป็นของผู้ใช้อื่น")); }
    if s.status != "open" { return Err(err(StatusCode::CONFLICT, "รอบแก้ไขนี้ปิดไปแล้ว")); }
    if s.doc_code != code || s.change_id != change_id { return Err(err(StatusCode::BAD_REQUEST, "รอบแก้ไขไม่ตรงกับเอกสาร/คำขอนี้")); }
    if s.sha256 == s.base_sha { return Err(err(StatusCode::BAD_REQUEST, "ยังไม่มีการแก้ไข — กดบันทึกในตัวแก้ไขก่อนส่งขออนุมัติ")); }
    let bytes = fs::read(edits_dir(st).join(&s.stored_name)).map_err(|_| err(StatusCode::NOT_FOUND, "ไม่พบไฟล์ที่แก้ไขในระบบ"))?;
    if sha256_hex(&bytes) != s.sha256 { return Err(err(StatusCode::CONFLICT, "ไฟล์กำลังถูกบันทึก — ลองใหม่อีกครั้ง")); }
    Ok((bytes, s.ext, s.filename))
}

/// ปิดรอบแก้ไขหลังส่งคำขอสำเร็จ — token ใช้ไม่ได้อีก ไฟล์ที่ลงนามถูกเก็บแยกไว้ใน storage/changes แล้ว
pub(crate) async fn finish(st: &S, client: &impl deadpool_postgres::GenericClient, id: i64) {
    if let Some(s) = load(client, id).await {
        let _ = client.execute("UPDATE edit_sessions SET status='submitted',updated_at=$2 WHERE id=$1", &[&id, &now_iso()]).await;
        let _ = fs::remove_file(edits_dir(st).join(&s.stored_name));
    }
}

// ---------- ฝั่ง ONLYOFFICE (ไม่มี login — ใช้ token ใน URL) ----------

async fn by_token(st: &S, token: &str) -> Result<Sess, Response> {
    let deny = || err(StatusCode::UNAUTHORIZED, "ลิงก์แก้ไขหมดอายุหรือถูกปิดแล้ว — เปิดจากหน้าเอกสารในระบบ ISO อีกครั้ง");
    if token.len() != 64 || !token.chars().all(|c| c.is_ascii_hexdigit()) { return Err(deny()); }
    let Ok(client) = st.db.get().await else { return Err(db_err()) };
    let s = client.query_opt(&format!("SELECT {COLS} FROM edit_sessions WHERE token_hash=$1"), &[&sha256_hex(token.as_bytes())])
        .await.ok().flatten().map(|r| row_sess(&r)).ok_or_else(deny)?;
    if s.status != "open" || s.expires_at < now_iso() { return Err(deny()); }
    Ok(s)
}

/// GET /api/public/edit/:token — ONLYOFFICE ดาวน์โหลดสำเนาทำงาน
pub async fn public_get(Path(token): Path<String>, State(st): State<S>) -> Response {
    let s = match by_token(&st, &token).await { Ok(s) => s, Err(r) => return r };
    let Ok(bytes) = fs::read(edits_dir(&st).join(&s.stored_name)) else { return err(StatusCode::NOT_FOUND, "ไม่พบไฟล์") };
    Response::builder()
        .header(header::CONTENT_TYPE, mime_of(&s.ext))
        .header(header::CONTENT_DISPOSITION, format!("attachment; filename*=UTF-8''{}", pct(&s.filename)))
        .header(header::CACHE_CONTROL, "no-store")
        .header(header::ETAG, format!("\"{}\"", s.sha256))
        .body(Body::from(bytes))
        .unwrap()
}

/// รอบแก้ไขที่ไม่มีใครแตะเกิน 7 วันถือว่าถูกทิ้ง — ปิดรอบและลบสำเนาทำงาน
/// (token หมดอายุใน 12 ชม. แต่เปิดต่อได้ — ร่างที่บันทึกไว้จึงอยู่ได้ข้ามคืน)
const ABANDON_DAYS: i64 = 7;

/// ล้างรอบแก้ไขที่ถูกทิ้ง และไฟล์ใน storage/edits ที่ไม่มีรอบเปิดอ้างถึง — รันตอนเริ่มและวันละครั้ง
pub async fn sweep(st: &S) {
    let Ok(client) = st.db.get().await else { return };
    let now = now_iso();
    let stale = (chrono::Utc::now() - chrono::Duration::days(ABANDON_DAYS)).to_rfc3339();
    let n = client.execute(
        "UPDATE edit_sessions SET status='expired',updated_at=$1 WHERE status='open' AND updated_at<$2", &[&now, &stale],
    ).await.unwrap_or(0);
    let keep: std::collections::HashSet<String> = match client.query("SELECT stored_name FROM edit_sessions WHERE status='open'", &[]).await {
        Ok(rows) => rows.iter().map(|r| r.get::<_, String>(0)).collect(),
        Err(_) => return, // ไม่รู้ว่าไฟล์ไหนยังใช้อยู่ — ไม่ลบอะไร
    };
    let mut removed = 0;
    if let Ok(rd) = fs::read_dir(edits_dir(st)) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            // ไฟล์อายุน้อยกว่า 1 ชม. อาจเป็นรอบที่กำลังสร้าง (ยังไม่ได้บันทึก stored_name)
            let young = e.metadata().and_then(|m| m.modified()).ok()
                .and_then(|t| t.elapsed().ok()).map(|d| d.as_secs() < 3600).unwrap_or(true);
            if !keep.contains(&name) && !young && fs::remove_file(e.path()).is_ok() { removed += 1; }
        }
    }
    if n > 0 || removed > 0 { println!("  🧹 รอบแก้ไขที่ถูกทิ้ง: ปิด {n} รอบ · ลบไฟล์ {removed} ไฟล์"); }
}

/// คนอื่นที่เปิดแก้เอกสารนี้อยู่ (มีความเคลื่อนไหวใน 12 ชม.) — แสดงเตือนก่อนแก้ซ้อนกัน
async fn others_editing(client: &impl deadpool_postgres::GenericClient, code: &str, me: &str) -> Value {
    let since = (chrono::Utc::now() - chrono::Duration::hours(TTL_HOURS)).to_rfc3339();
    let rows = client.query(
        "SELECT emp_name,updated_at,saves FROM edit_sessions WHERE doc_code=$1 AND emp_code<>$2 AND status='open' AND updated_at>$3 ORDER BY updated_at DESC",
        &[&code, &me, &since],
    ).await.unwrap_or_default();
    Value::Array(rows.iter().map(|r| json!({ "name": r.get::<_, String>(0), "updatedAt": r.get::<_, String>(1), "saves": r.get::<_, i32>(2) })).collect())
}

/// สถานะตัวแก้ไข ONLYOFFICE สำหรับ /api/health ("off" = ยังไม่ได้ตั้งค่า)
pub async fn office_health() -> &'static str {
    let internal = env_url("ONLYOFFICE_INTERNAL_URL");
    if internal.is_empty() { return "off"; }
    let Ok(c) = reqwest::Client::builder().timeout(std::time::Duration::from_millis(1500)).build() else { return "down" };
    match c.get(format!("{internal}/healthcheck")).send().await {
        Ok(r) if r.status().is_success() => if r.text().await.map(|t| t.trim() == "true").unwrap_or(false) { "ok" } else { "down" },
        _ => "down",
    }
}

// ---------- ฟอนต์มาตรฐานเอกสาร ----------

/// ฟอนต์มาตรฐานของเอกสาร ISO — ข้อความที่วาง/พิมพ์ในตัวแก้ไขบนเว็บถูกปรับเป็นฟอนต์นี้ทุกครั้งที่บันทึก
const DOC_FONT: &str = "Angsana New";

/// ฟอนต์สัญลักษณ์ (☐ ✓ • ฯลฯ) — เปลี่ยนแล้วสัญลักษณ์จะกลายเป็นตัวอักษรอื่น จึงคงไว้
fn symbol_font(name: &str) -> bool {
    let n = name.trim().to_ascii_lowercase();
    n == "symbol" || n.starts_with("wingdings") || n == "webdings" || n == "marlett" || n.starts_with("segoe ui symbol")
        || n.starts_with("segoe ui emoji") || n == "ms gothic" || n == "ms mincho" || n.starts_with("zapf")
        // สมการ (OMML) ต้องใช้ฟอนต์คณิตศาสตร์ — เปลี่ยนแล้ว Word แสดงสมการผิด
        || n == "cambria math" || n.ends_with(" math")
}

/// ค่าของ attribute ในแท็ก XML เช่น attr(r#"<w:rFonts w:ascii="X"/>"#, "w:ascii") = Some("X")
fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let i = tag.find(&format!(" {name}=\""))? + name.len() + 3;
    tag[i..].find('"').map(|e| &tag[i..i + e])
}

/// เปลี่ยน <w:rFonts> ทุกตัวเป็น DOC_FONT (ทั้ง ascii/hAnsi/eastAsia/cs และตัด theme font ที่จะทับค่าที่ตั้ง) — เว้นฟอนต์สัญลักษณ์
fn force_fonts_xml(xml: &str) -> String {
    let mut out = String::with_capacity(xml.len());
    let mut rest = xml;
    while let Some(i) = rest.find("<w:rFonts") {
        let Some(len) = rest[i..].find('>') else { break };
        let tag = &rest[i..=i + len];
        // <w:rFontsX ...> ไม่ใช่แท็กนี้
        let real = matches!(tag.as_bytes().get(9), Some(b' ' | b'/' | b'>'));
        let symbol = ["w:ascii", "w:hAnsi", "w:eastAsia", "w:cs"].iter().any(|a| attr(tag, a).is_some_and(symbol_font));
        out.push_str(&rest[..i]);
        if real && !symbol {
            let hint = attr(tag, "w:hint").map(|h| format!(" w:hint=\"{h}\"")).unwrap_or_default();
            out.push_str(&format!("<w:rFonts w:ascii=\"{DOC_FONT}\" w:hAnsi=\"{DOC_FONT}\" w:eastAsia=\"{DOC_FONT}\" w:cs=\"{DOC_FONT}\"{hint}{}>",
                if tag.ends_with("/>") { "/" } else { "" }));
        } else {
            out.push_str(tag);
        }
        rest = &rest[i + len + 1..];
    }
    out.push_str(rest);
    out
}

/// ค่าเริ่มต้นของเอกสาร (docDefaults) ใช้ DOC_FONT — ข้อความที่ไม่ได้ระบุฟอนต์ (เช่นวางแบบข้อความล้วน) จะได้ฟอนต์นี้
fn default_font_styles(xml: &str) -> String {
    let fonts = format!("<w:rFonts w:ascii=\"{DOC_FONT}\" w:hAnsi=\"{DOC_FONT}\" w:eastAsia=\"{DOC_FONT}\" w:cs=\"{DOC_FONT}\"/>");
    let Some(d) = xml.find("<w:rPrDefault") else {
        // ไม่มี rPrDefault — เพิ่มไว้ใน docDefaults (หรือสร้าง docDefaults ต่อจากแท็กเปิด <w:styles>)
        // <w:docDefaults/> ว่าง — แทนที่ตรงนั้น (ห้ามสร้าง docDefaults ซ้ำ ไม่งั้น Word เปิดแบบซ่อมไฟล์)
        if xml.contains("<w:docDefaults/>") {
            return xml.replacen("<w:docDefaults/>", &format!("<w:docDefaults><w:rPrDefault><w:rPr>{fonts}</w:rPr></w:rPrDefault></w:docDefaults>"), 1);
        }
        if let Some(i) = xml.find("<w:docDefaults>") {
            let at = i + "<w:docDefaults>".len();
            return format!("{}<w:rPrDefault><w:rPr>{fonts}</w:rPr></w:rPrDefault>{}", &xml[..at], &xml[at..]);
        }
        let Some(s) = xml.find("<w:styles") else { return xml.to_string() };
        let Some(e) = xml[s..].find('>') else { return xml.to_string() };
        let at = s + e + 1;
        return format!("{}<w:docDefaults><w:rPrDefault><w:rPr>{fonts}</w:rPr></w:rPrDefault></w:docDefaults>{}", &xml[..at], &xml[at..]);
    };
    let end = xml[d..].find("</w:rPrDefault>").map(|e| d + e);
    let block = end.map(|e| &xml[d..e]).unwrap_or("");
    if block.contains("<w:rFonts") { return xml.to_string(); } // มีแล้ว — force_fonts_xml เปลี่ยนให้
    match (end, block.find("<w:rPr>")) {
        (Some(_), Some(r)) => { let at = d + r + "<w:rPr>".len(); format!("{}{fonts}{}", &xml[..at], &xml[at..]) }
        (Some(e), None) => format!("{}><w:rPr>{fonts}</w:rPr>{}", &xml[..d + "<w:rPrDefault".len()], &xml[e..]),
        // <w:rPrDefault/>
        (None, _) => xml.replacen("<w:rPrDefault/>", &format!("<w:rPrDefault><w:rPr>{fonts}</w:rPr></w:rPrDefault>"), 1),
    }
}

/// ปรับฟอนต์ใน .docx ทั้งไฟล์ (เนื้อหา สไตล์ หัว/ท้ายกระดาษ เชิงอรรถ) เป็น DOC_FONT — None = ไม่มีอะไรเปลี่ยนหรืออ่านไฟล์ไม่ได้
fn force_doc_font(bytes: &[u8]) -> Option<Vec<u8>> {
    use std::io::{Cursor, Read, Write};
    let fixed = crate::revscan::repair_eocd(bytes);
    let mut zin = zip::ZipArchive::new(Cursor::new(fixed.as_slice())).ok()?;
    let targets: Vec<String> = zin.file_names().filter(|n| {
        matches!(*n, "word/document.xml" | "word/styles.xml" | "word/footnotes.xml" | "word/endnotes.xml")
            || ((n.starts_with("word/header") || n.starts_with("word/footer")) && n.ends_with(".xml"))
    }).map(String::from).collect();
    let mut changed = std::collections::HashMap::new();
    for name in targets {
        let mut xml = String::new();
        zin.by_name(&name).ok()?.read_to_string(&mut xml).ok()?;
        let mut out = force_fonts_xml(&xml);
        if name == "word/styles.xml" { out = default_font_styles(&out); }
        if out != xml { changed.insert(name, out); }
    }
    if changed.is_empty() { return None; }
    let mut zout = zip::ZipWriter::new(Cursor::new(Vec::with_capacity(bytes.len() + 4096)));
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for i in 0..zin.len() {
        let name = zin.by_index_raw(i).ok()?.name().to_string();
        if let Some(xml) = changed.get(&name) {
            zout.start_file(name.as_str(), opts).ok()?;
            zout.write_all(xml.as_bytes()).ok()?;
        } else {
            zout.raw_copy_file(zin.by_index_raw(i).ok()?).ok()?;
        }
    }
    Some(zout.finish().ok()?.into_inner())
}

/// บันทึกไฟล์ที่แก้เป็นสำเนาทำงานของรอบแก้ไข (ONLYOFFICE ส่ง .doc/.xls/.ppt กลับมาเป็น .docx/.xlsx/.pptx — รับได้ และเปลี่ยนชนิดฉบับร่างตาม)
/// Word: ข้อความที่วาง/คัดลอกมาจากที่อื่นถูกปรับเป็นฟอนต์มาตรฐาน (Angsana New) ทุกครั้งที่บันทึก
async fn store_saved(st: &S, s: &Sess, body: &[u8]) -> Result<(), String> {
    if body.is_empty() { return Err("ไฟล์ว่าง".into()); }
    if body.len() > MAX_BYTES { return Err("ไฟล์ใหญ่เกิน 40 MB".into()); }
    let ext = match modern_of(&s.ext) { Some(m) if looks_like(m, body) => m.to_string(), _ => s.ext.clone() };
    if !looks_like(&ext, body) { return Err(format!("ไฟล์ไม่ใช่ .{}", s.ext)); }
    let normalized = if ext == "docx" { force_doc_font(body) } else { None };
    let body = normalized.as_deref().unwrap_or(body);
    let (stored, filename) = if ext == s.ext {
        (s.stored_name.clone(), s.filename.clone())
    } else {
        let stem = s.filename.rsplit_once('.').map(|(a, _)| a).unwrap_or(&s.filename).to_string();
        (format!("{}.{ext}", s.id), format!("{stem}.{ext}"))
    };
    let sha = sha256_hex(body);
    if sha != s.sha256 || stored != s.stored_name {
        write_atomic(&edits_dir(st).join(&stored), body).map_err(|e| format!("บันทึกไฟล์ไม่ได้: {e}"))?;
    }
    let client = st.db.get().await.map_err(|_| "เชื่อมต่อฐานข้อมูลไม่ได้".to_string())?;
    let now = now_iso();
    let size = body.len() as i64;
    let _ = client.execute(
        "UPDATE edit_sessions SET sha256=$2,size=$3,saves=saves+1,saved_at=$4,updated_at=$4,ext=$5,filename=$6,stored_name=$7 WHERE id=$1 AND status='open'",
        &[&s.id, &sha, &size, &now, &ext, &filename, &stored],
    ).await;
    if stored != s.stored_name { let _ = fs::remove_file(edits_dir(st).join(&s.stored_name)); }
    Ok(())
}

/// POST /api/public/office/:token — callback ของ ONLYOFFICE (ต้องตอบ {"error":0} ไม่งั้นตัวแก้ไขขึ้นว่าบันทึกไม่ได้)
///   status 2 = ปิดตัวแก้ไขแล้วมีการแก้, 6 = กดบันทึก (forcesave) → ดึงไฟล์จาก url มาเป็นสำเนาทำงาน
pub async fn office_callback(Path(token): Path<String>, State(st): State<S>, headers: HeaderMap, body: Bytes) -> Response {
    let reply = |code: i32| Json(json!({ "error": code })).into_response();
    let s = match by_token(&st, &token).await { Ok(s) => s, Err(_) => return reply(1) };
    let secret = std::env::var("ONLYOFFICE_JWT_SECRET").unwrap_or_default();
    if secret.is_empty() { return reply(1); }
    // ข้อมูลที่เชื่อได้คือ payload ใน JWT เท่านั้น (ใน body หรือ header Authorization: Bearer)
    let raw: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let jwt = raw["token"].as_str().map(String::from).or_else(|| {
        headers.get("authorization").and_then(|h| h.to_str().ok()).and_then(|h| h.strip_prefix("Bearer ")).map(String::from)
    });
    let Some(claims) = jwt.as_deref().and_then(|t| jwt_verify(&secret, t)) else { return reply(1) };
    let cb = if claims.get("payload").is_some() { claims["payload"].clone() } else { claims };
    let status = cb["status"].as_i64().unwrap_or(0);
    if status != 2 && status != 6 { return reply(0); }
    let Some(url) = cb["url"].as_str() else { return reply(1) };
    // url ชี้ไปที่อยู่สาธารณะของ ONLYOFFICE — ดึงผ่านที่อยู่ภายในแทน และรับเฉพาะไฟล์จาก ONLYOFFICE เท่านั้น
    let (public, internal) = (env_url("ONLYOFFICE_URL"), env_url("ONLYOFFICE_INTERNAL_URL"));
    let under = |base: &str| !base.is_empty() && url.starts_with(&format!("{base}/"));
    let fetch_url = if under(&public) && !internal.is_empty() {
        format!("{internal}{}", &url[public.len()..])
    } else if under(&internal) || under(&public) {
        url.to_string()
    } else {
        eprintln!("  ⚠ ONLYOFFICE callback: url นอกเหนือที่อนุญาต {url}");
        return reply(1);
    };
    let bytes = match reqwest::Client::new().get(&fetch_url).timeout(std::time::Duration::from_secs(60)).send().await {
        Ok(r) if r.status().is_success() => match r.bytes().await { Ok(b) => b, Err(_) => return reply(1) },
        _ => return reply(1),
    };
    match store_saved(&st, &s, &bytes).await {
        Ok(()) => reply(0),
        Err(e) => { eprintln!("  ⚠ บันทึกจาก ONLYOFFICE ไม่สำเร็จ: {e}"); reply(1) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hmac_matches_rfc4231_case2() {
        let mac = hmac_sha256(b"Jefe", b"what do ya want for nothing?");
        let hex: String = mac.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(hex, "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843");
    }

    #[test]
    fn jwt_roundtrip_and_tamper() {
        let t = jwt_sign("secret", &json!({ "status": 6, "url": "http://iso-office/cache/x.docx" }));
        assert_eq!(jwt_verify("secret", &t).unwrap()["status"], 6);
        assert!(jwt_verify("other", &t).is_none());
        let mut parts: Vec<&str> = t.split('.').collect();
        let forged = b64url(br#"{"status":6,"url":"http://evil/x.docx"}"#);
        parts[1] = &forged;
        assert!(jwt_verify("secret", &parts.join(".")).is_none());
        let none_alg = format!("{}.{}.", b64url(br#"{"alg":"none"}"#), b64url(b"{}"));
        assert!(jwt_verify("secret", &none_alg).is_none());
    }

    #[test]
    fn forces_angsana_but_keeps_symbol_fonts() {
        let xml = r#"<w:r><w:rPr><w:rFonts w:asciiTheme="minorHAnsi" w:ascii="Calibri" w:cs="Tahoma" w:hint="cs"/></w:rPr><w:t>ก</w:t></w:r><w:r><w:rPr><w:rFonts w:ascii="Wingdings" w:hAnsi="Wingdings"/></w:rPr></w:r><w:rFontsX/>"#;
        let out = force_fonts_xml(xml);
        assert!(out.contains(r#"<w:rFonts w:ascii="Angsana New" w:hAnsi="Angsana New" w:eastAsia="Angsana New" w:cs="Angsana New" w:hint="cs"/>"#));
        assert!(!out.contains("Calibri") && !out.contains("minorHAnsi"));
        assert!(out.contains(r#"<w:rFonts w:ascii="Wingdings" w:hAnsi="Wingdings"/>"#) && out.contains("<w:rFontsX/>"));
    }

    #[test]
    fn sets_document_default_font() {
        let f = r#"<w:rFonts w:ascii="Angsana New""#;
        assert!(default_font_styles(r#"<w:styles x="1"><w:docDefaults><w:rPrDefault><w:rPr><w:sz w:val="28"/></w:rPr></w:rPrDefault></w:docDefaults></w:styles>"#).contains(&format!("<w:rPr>{f}")));
        assert!(default_font_styles(r#"<w:styles><w:docDefaults><w:rPrDefault/></w:docDefaults></w:styles>"#).contains(f));
        let self_closing = default_font_styles(r#"<w:styles x="1"><w:docDefaults/><w:style/></w:styles>"#);
        assert_eq!(self_closing.matches("<w:docDefaults").count(), 1);
        assert!(self_closing.contains(f));
        assert!(symbol_font("Cambria Math"));
        assert!(default_font_styles(r#"<w:styles x="1"><w:style/></w:styles>"#).starts_with(&format!(r#"<w:styles x="1"><w:docDefaults><w:rPrDefault><w:rPr>{f}"#)));
    }
}
