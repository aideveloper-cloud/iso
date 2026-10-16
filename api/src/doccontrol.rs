// ควบคุมการเปลี่ยนแปลงเอกสาร (Document Change Control, ISO 9001:2015 ข้อ 7.5.2–7.5.3)
//   คำขอเปลี่ยนแปลง (DCR): ผู้จัดทำส่งไฟล์ฉบับร่าง → ผู้ตรวจสอบ → ผู้อนุมัติ → มีผลบังคับใช้ (สร้างเวอร์ชันใหม่ + Rev. ใหม่)
//   ทุกขั้นลงนามด้วยผู้ใช้ที่ login + กรอก PIN ซ้ำ และบันทึก SHA-256 ของไฟล์ที่ลงนาม
//   ยกเลิกใช้ (Obsolete) / นำกลับมาใช้ — กันการนำเอกสารที่ไม่ใช้แล้วไปใช้โดยไม่ตั้งใจ
use crate::eform::{self, AuthUser};
use crate::{decode_upload_bytes, ensure_original, err, ext_of, mime_of, now_iso, pct, preview_abs, safe_stem, sha256_hex, NewVersion, S};
use axum::{
    body::Body,
    extract::{Extension, Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{collections::HashMap, fs};

pub const SCHEMA: &str = "
ALTER TABLE versions ADD COLUMN IF NOT EXISTS rev TEXT NOT NULL DEFAULT '';
ALTER TABLE versions ADD COLUMN IF NOT EXISTS effective_date TEXT NOT NULL DEFAULT '';
ALTER TABLE versions ADD COLUMN IF NOT EXISTS change_id BIGINT NOT NULL DEFAULT 0;
ALTER TABLE versions ADD COLUMN IF NOT EXISTS sha256 TEXT NOT NULL DEFAULT '';
ALTER TABLE versions ADD COLUMN IF NOT EXISTS signer_code TEXT NOT NULL DEFAULT '';
ALTER TABLE versions ADD COLUMN IF NOT EXISTS approved_by TEXT NOT NULL DEFAULT '';
ALTER TABLE versions ADD COLUMN IF NOT EXISTS approved_at TEXT NOT NULL DEFAULT '';
CREATE TABLE IF NOT EXISTS doc_changes(
  id BIGSERIAL PRIMARY KEY,
  ref_no TEXT NOT NULL,
  doc_code TEXT NOT NULL,
  status TEXT NOT NULL,
  reason TEXT NOT NULL DEFAULT '',
  note TEXT NOT NULL DEFAULT '',
  from_rev TEXT NOT NULL DEFAULT '',
  new_rev TEXT NOT NULL,
  filename TEXT NOT NULL,
  ext TEXT NOT NULL,
  stored_name TEXT NOT NULL,
  size BIGINT NOT NULL DEFAULT 0,
  sha256 TEXT NOT NULL,
  source TEXT NOT NULL,
  base_version BIGINT NOT NULL DEFAULT 0,
  prepared_by TEXT NOT NULL,
  prepared_name TEXT NOT NULL,
  prepared_position TEXT NOT NULL DEFAULT '',
  version BIGINT NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  closed_at TEXT NOT NULL DEFAULT '');
CREATE INDEX IF NOT EXISTS idx_dc_doc ON doc_changes(doc_code);
CREATE UNIQUE INDEX IF NOT EXISTS uq_dc_open ON doc_changes(doc_code) WHERE status IN ('review','approve','returned');
CREATE TABLE IF NOT EXISTS doc_change_steps(
  id BIGSERIAL PRIMARY KEY,
  change_id BIGINT NOT NULL,
  step TEXT NOT NULL,
  action TEXT NOT NULL,
  actor_code TEXT NOT NULL,
  actor_name TEXT NOT NULL,
  actor_position TEXT NOT NULL DEFAULT '',
  comment TEXT NOT NULL DEFAULT '',
  sha256 TEXT NOT NULL DEFAULT '',
  at TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS idx_dcs_change ON doc_change_steps(change_id);
CREATE TABLE IF NOT EXISTS doc_status(
  doc_code TEXT PRIMARY KEY,
  status TEXT NOT NULL,
  reason TEXT NOT NULL DEFAULT '',
  actor_code TEXT NOT NULL,
  actor_name TEXT NOT NULL,
  at TEXT NOT NULL);
";

/// สถานะที่ยังไม่จบ — เอกสารหนึ่งฉบับมีคำขอค้างได้ครั้งละ 1 ใบ (บังคับด้วย uq_dc_open)
const OPEN: [&str; 3] = ["review", "approve", "returned"];
const DCOLS: &str = "id,ref_no,doc_code,status,reason,note,from_rev,new_rev,filename,ext,stored_name,size,sha256,source,base_version,prepared_by,prepared_name,prepared_position,version,created_at,updated_at,closed_at";

fn db_err() -> Response { err(StatusCode::INTERNAL_SERVER_ERROR, "เชื่อมต่อฐานข้อมูลไม่ได้") }
fn is_open(status: &str) -> bool { OPEN.contains(&status) }

fn change_json(r: &tokio_postgres::Row) -> Value {
    json!({
        "id": r.get::<_, i64>(0), "refNo": r.get::<_, String>(1), "docCode": r.get::<_, String>(2),
        "status": r.get::<_, String>(3), "reason": r.get::<_, String>(4), "note": r.get::<_, String>(5),
        "fromRev": r.get::<_, String>(6), "newRev": r.get::<_, String>(7),
        "filename": r.get::<_, String>(8), "ext": r.get::<_, String>(9),
        "size": r.get::<_, i64>(11), "sha256": r.get::<_, String>(12), "source": r.get::<_, String>(13),
        "baseVersion": r.get::<_, i64>(14),
        "preparedBy": r.get::<_, String>(15), "preparedName": r.get::<_, String>(16), "preparedPosition": r.get::<_, String>(17),
        "version": r.get::<_, i64>(18),
        "createdAt": r.get::<_, String>(19), "updatedAt": r.get::<_, String>(20), "closedAt": r.get::<_, String>(21),
    })
}

/// สิทธิ์ของผู้ใช้ต่อคำขอนี้ — ผู้จัดทำตรวจ/อนุมัติคำขอของตนเองไม่ได้ (ยกเว้นผู้ดูแลระบบ ตามกติกาเดียวกับ e-form)
fn can_flags(u: &AuthUser, ch: &Value) -> Value {
    let status = ch["status"].as_str().unwrap_or("");
    let own = ch["preparedBy"].as_str() == Some(u.emp_code.as_str());
    let other = eform::is_admin(&u.role) || !own;
    json!({
        "review": status == "review" && eform::can_review(&u.role) && other,
        "approve": status == "approve" && eform::can_approve(&u.role) && other,
        "resubmit": status == "returned" && own,
        "cancel": is_open(status) && (own || eform::is_admin(&u.role)),
    })
}

fn with_doc(st: &S, u: &AuthUser, mut ch: Value) -> Value {
    let code = ch["docCode"].as_str().unwrap_or("").to_string();
    if let Some(d) = st.by_code.get(&code) {
        ch["docName"] = d["name"].clone();
        ch["docType"] = d["type"].clone();
        ch["deptName"] = d["deptName"].clone();
    }
    ch["can"] = can_flags(u, &ch);
    ch
}

async fn load_change(c: &impl deadpool_postgres::GenericClient, id: i64) -> Option<(Value, String)> {
    let r = c.query_opt(&format!("SELECT {DCOLS} FROM doc_changes WHERE id=$1"), &[&id]).await.ok()??;
    let stored: String = r.get(10);
    Some((change_json(&r), stored))
}

async fn add_step(c: &impl deadpool_postgres::GenericClient, id: i64, step: &str, action: &str, u: &AuthUser, comment: &str, sha: &str) {
    let _ = c.execute(
        "INSERT INTO doc_change_steps(change_id,step,action,actor_code,actor_name,actor_position,comment,sha256,at) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)",
        &[&id, &step.to_string(), &action.to_string(), &u.emp_code, &u.name, &u.position, &comment.to_string(), &sha.to_string(), &now_iso()],
    ).await;
}

async fn steps_of(c: &impl deadpool_postgres::GenericClient, id: i64) -> Vec<Value> {
    let Ok(rows) = c.query(
        "SELECT step,action,actor_code,actor_name,actor_position,comment,sha256,at FROM doc_change_steps WHERE change_id=$1 ORDER BY id",
        &[&id],
    ).await else { return vec![] };
    rows.iter().map(|r| json!({
        "step": r.get::<_, String>(0), "action": r.get::<_, String>(1), "actorCode": r.get::<_, String>(2),
        "actorName": r.get::<_, String>(3), "actorPosition": r.get::<_, String>(4), "comment": r.get::<_, String>(5),
        "sha256": r.get::<_, String>(6), "at": r.get::<_, String>(7),
    })).collect()
}

async fn detail_resp(st: &S, u: &AuthUser, id: i64) -> Response {
    let Ok(client) = st.db.get().await else { return db_err() };
    let Some((ch, _)) = load_change(&client, id).await else { return err(StatusCode::NOT_FOUND, "ไม่พบคำขอเปลี่ยนแปลง") };
    let mut ch = with_doc(st, u, ch);
    ch["steps"] = json!(steps_of(&client, id).await);
    Json(ch).into_response()
}

// ---------- Rev. / วันที่บังคับใช้ ----------

/// Rev. ถัดไป: "Rev.00" → "Rev.01", "Rev.9" → "Rev.10", ว่าง → "Rev.01"
pub fn bump_rev(rev: &str) -> String {
    let t = rev.trim();
    let digits = t.chars().rev().take_while(|c| c.is_ascii_digit()).count();
    let (pre, num) = t.split_at(t.len() - digits);
    if num.is_empty() {
        return format!("{}01", if pre.is_empty() { "Rev." } else { pre });
    }
    let n: u64 = num.parse::<u64>().unwrap_or(0) + 1;
    format!("{pre}{:0width$}", n, width = num.len())
}

/// วันที่วันนี้ในรูปแบบทะเบียนเอกสาร: วว/ดด/พ.ศ. (เวลาไทย)
fn today_th() -> String {
    let tz = chrono::FixedOffset::east_opt(7 * 3600).unwrap();
    let d = chrono::Utc::now().with_timezone(&tz);
    let y: i64 = d.format("%Y").to_string().parse().unwrap_or(2026) + 543;
    format!("{}/{y}", d.format("%d/%m"))
}

/// สถานะควบคุมของทุกเอกสาร (Rev./วันที่บังคับใช้ล่าสุด, ยกเลิกใช้, คำขอที่ค้าง) — อ่านครั้งเดียวต่อคำขอ
pub struct ControlMap {
    revs: HashMap<String, (String, String)>,
    status: HashMap<String, Value>,
    open: HashMap<String, Value>,
}

impl ControlMap {
    /// เขียนทับ rev/eff จาก manifest ด้วยค่าจากเวอร์ชันที่อนุมัติล่าสุด และเติม docStatus / openChange
    pub fn apply(&self, code: &str, o: &mut Value) {
        if let Some((rev, eff)) = self.revs.get(code) {
            o["rev"] = json!(rev);
            if !eff.is_empty() { o["eff"] = json!(eff); }
        }
        match self.status.get(code) {
            Some(s) => { o["docStatus"] = s["status"].clone(); o["obsolete"] = s.clone(); }
            None => { o["docStatus"] = json!("active"); o["obsolete"] = Value::Null; }
        }
        o["openChange"] = self.open.get(code).cloned().unwrap_or(Value::Null);
    }
}

pub async fn control_map(st: &S) -> ControlMap {
    let mut m = ControlMap { revs: HashMap::new(), status: HashMap::new(), open: HashMap::new() };
    let Ok(client) = st.db.get().await else { return m };
    if let Ok(rows) = client.query(
        "SELECT DISTINCT ON (doc_code) doc_code,rev,effective_date FROM versions WHERE rev<>'' ORDER BY doc_code, version DESC", &[],
    ).await {
        for r in rows { m.revs.insert(r.get(0), (r.get(1), r.get(2))); }
    }
    if let Ok(rows) = client.query("SELECT doc_code,status,reason,actor_name,at FROM doc_status WHERE status='obsolete'", &[]).await {
        for r in rows {
            m.status.insert(r.get(0), json!({ "status": r.get::<_, String>(1), "reason": r.get::<_, String>(2), "by": r.get::<_, String>(3), "at": r.get::<_, String>(4) }));
        }
    }
    if let Ok(rows) = client.query("SELECT id,ref_no,doc_code,status,new_rev,prepared_name FROM doc_changes WHERE status IN ('review','approve','returned')", &[]).await {
        for r in rows {
            m.open.insert(r.get(2), json!({ "id": r.get::<_, i64>(0), "refNo": r.get::<_, String>(1), "status": r.get::<_, String>(3), "newRev": r.get::<_, String>(4), "preparedName": r.get::<_, String>(5) }));
        }
    }
    m
}

async fn current_rev(st: &S, c: &impl deadpool_postgres::GenericClient, code: &str) -> String {
    let r = c.query_opt("SELECT rev FROM versions WHERE doc_code=$1 AND rev<>'' ORDER BY version DESC LIMIT 1", &[&code]).await.ok().flatten();
    match r {
        Some(r) => r.get(0),
        None => st.by_code.get(code).and_then(|d| d["rev"].as_str()).unwrap_or("").to_string(),
    }
}

pub(crate) async fn is_obsolete(c: &impl deadpool_postgres::GenericClient, code: &str) -> bool {
    c.query_opt("SELECT 1 FROM doc_status WHERE doc_code=$1 AND status='obsolete'", &[&code]).await.ok().flatten().is_some()
}

/// ชื่อไฟล์ตอนดาวน์โหลด — ติดป้ายถ้าเป็นเวอร์ชันเก่าหรือเอกสารยกเลิกใช้ (สำเนาไม่ควบคุม)
pub async fn download_name(st: &S, code: &str, v: i64, filename: &str) -> String {
    let Ok(c) = st.db.get().await else { return filename.to_string() };
    if is_obsolete(&c, code).await { return format!("OBSOLETE - {filename}"); }
    let latest: i64 = c.query_one("SELECT COALESCE(MAX(version),0) FROM versions WHERE doc_code=$1", &[&code]).await.map(|r| r.get(0)).unwrap_or(0);
    if v < latest { format!("UNCONTROLLED v{v} - {filename}") } else { filename.to_string() }
}

/// จำนวนคำขอที่รอผู้ใช้คนนี้ตรวจสอบ/อนุมัติ
pub async fn pending_count(st: &S, u: &AuthUser) -> i64 {
    let Ok(c) = st.db.get().await else { return 0 };
    let Ok(rows) = c.query(&format!("SELECT {DCOLS} FROM doc_changes WHERE status IN ('review','approve')"), &[]).await else { return 0 };
    rows.iter().filter(|r| {
        let f = can_flags(u, &change_json(r));
        f["review"] == json!(true) || f["approve"] == json!(true)
    }).count() as i64
}

// ---------- ไฟล์ฉบับร่าง ----------

struct Draft { bytes: Vec<u8>, ext: String, filename: String, source: &'static str }

fn read_draft(doc: &Value, data: Option<String>, ext: Option<String>, filename: Option<String>, source: Option<String>) -> Result<Draft, Response> {
    let bytes = decode_upload_bytes(data)?;
    let ext: String = ext.unwrap_or_else(|| ext_of(doc)).to_lowercase().chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    if ext.is_empty() { return Err(err(StatusCode::BAD_REQUEST, "ไม่ทราบชนิดไฟล์")); }
    let filename = filename.filter(|f| !f.trim().is_empty()).unwrap_or_else(|| doc["file"].as_str().unwrap_or("").to_string());
    let source = if source.as_deref() == Some("edit") { "edit" } else { "upload" };
    Ok(Draft { bytes, ext, filename, source })
}

fn changes_dir(st: &S) -> std::path::PathBuf {
    let d = st.storage.join("changes");
    let _ = fs::create_dir_all(&d);
    d
}

/// เก็บไฟล์ฉบับร่างแยกจากเวอร์ชันที่ใช้งาน — ชื่อไม่ซ้ำกันทุกครั้งที่ส่ง เพื่อให้ไฟล์ที่ลงนามแต่ละขั้นยังอยู่ครบ
fn store_draft(st: &S, ref_no: &str, round: i64, ext: &str, bytes: &[u8]) -> Result<String, Response> {
    let name = format!("{}_r{round}.{ext}", safe_stem(ref_no));
    fs::write(changes_dir(st).join(&name), bytes)
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &format!("บันทึกไฟล์ฉบับร่างไม่ได้: {e}")))?;
    Ok(name)
}

fn validate_rev(new_rev: &str, from_rev: &str) -> Result<(), Response> {
    if new_rev.is_empty() { return Err(err(StatusCode::BAD_REQUEST, "กรุณาระบุ Rev. ใหม่")); }
    if new_rev.eq_ignore_ascii_case(from_rev.trim()) {
        return Err(err(StatusCode::BAD_REQUEST, &format!("Rev. ใหม่ต้องไม่ซ้ำกับฉบับปัจจุบัน ({from_rev})")));
    }
    Ok(())
}

async fn rev_used(c: &impl deadpool_postgres::GenericClient, code: &str, rev: &str) -> bool {
    c.query_opt("SELECT 1 FROM versions WHERE doc_code=$1 AND lower(rev)=lower($2)", &[&code, &rev]).await.ok().flatten().is_some()
}

// ---------- handlers ----------

#[derive(Deserialize)]
#[allow(non_snake_case)]
pub struct ChangeReq {
    dataBase64: Option<String>,
    ext: Option<String>,
    filename: Option<String>,
    source: Option<String>,
    note: Option<String>,
    reason: Option<String>,
    newRev: Option<String>,
    pin: Option<String>,
    /// ไฟล์จากรอบแก้ไขบนเว็บ (editsession, ONLYOFFICE) แทน dataBase64
    editId: Option<i64>,
}

/// ไฟล์ฉบับร่าง: จากรอบแก้ไขบนเว็บ ถ้าระบุ editId ไม่งั้นจาก dataBase64
async fn draft_of(st: &S, client: &impl deadpool_postgres::GenericClient, u: &AuthUser, doc: &Value, code: &str, change_id: i64, b: &mut ChangeReq)
    -> Result<Draft, Response> {
    match b.editId {
        Some(eid) => {
            let (bytes, ext, filename) = crate::editsession::take(st, client, u, eid, code, change_id).await?;
            Ok(Draft { bytes, ext, filename, source: "edit" })
        }
        None => read_draft(doc, b.dataBase64.take(), b.ext.take(), b.filename.take(), b.source.take()),
    }
}

/// ส่งคำขอเปลี่ยนแปลงเอกสาร (ผู้จัดทำลงนาม) → รอตรวจสอบ
pub async fn create(Path(code): Path<String>, State(st): State<S>, Extension(u): Extension<AuthUser>, Json(mut b): Json<ChangeReq>) -> Response {
    let Some(doc) = st.by_code.get(&code).cloned() else { return err(StatusCode::NOT_FOUND, "ไม่พบเอกสาร") };
    if u.role == "viewer" { return err(StatusCode::FORBIDDEN, "บัญชีผู้อ่านอย่างเดียวส่งคำขอแก้ไขไม่ได้"); }
    let reason = b.reason.take().unwrap_or_default().trim().to_string();
    if reason.is_empty() { return err(StatusCode::BAD_REQUEST, "กรุณาระบุเหตุผลการเปลี่ยนแปลง"); }
    let Ok(client) = st.db.get().await else { return db_err() };
    if is_obsolete(&client, &code).await { return err(StatusCode::CONFLICT, "เอกสารนี้ถูกยกเลิกใช้แล้ว — ต้องนำกลับมาใช้ก่อนจึงจะแก้ไขได้"); }
    if let Ok(Some(r)) = client.query_opt("SELECT ref_no FROM doc_changes WHERE doc_code=$1 AND status IN ('review','approve','returned')", &[&code]).await {
        return err(StatusCode::CONFLICT, &format!("มีคำขอ {} ที่ยังไม่จบอยู่แล้ว — ดำเนินการให้เสร็จหรือยกเลิกก่อน", r.get::<_, String>(0)));
    }
    if let Err(r) = eform::verify_pin(&st, &client, &u, b.pin.as_deref().unwrap_or("")).await { return r; }
    let draft = match draft_of(&st, &client, &u, &doc, &code, 0, &mut b).await { Ok(d) => d, Err(r) => return r };

    ensure_original(&st, &code).await;
    let base_version: i64 = client.query_one("SELECT COALESCE(MAX(version),0) FROM versions WHERE doc_code=$1", &[&code])
        .await.map(|r| r.get(0)).unwrap_or(0);
    let from_rev = current_rev(&st, &client, &code).await;
    let new_rev = b.newRev.take().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).unwrap_or_else(|| bump_rev(&from_rev));
    if let Err(r) = validate_rev(&new_rev, &from_rev) { return r; }
    if rev_used(&client, &code, &new_rev).await { return err(StatusCode::BAD_REQUEST, &format!("{new_rev} เคยใช้กับเอกสารนี้แล้ว")); }

    let year = eform::be_year_now();
    let seq: i64 = match client.query_one(
        "INSERT INTO form_seq(form_code,year_be,last_seq) VALUES('DCR',$1,1) ON CONFLICT(form_code,year_be) DO UPDATE SET last_seq=form_seq.last_seq+1 RETURNING last_seq",
        &[&year],
    ).await { Ok(r) => r.get(0), Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()) };
    let ref_no = format!("DCR-{year}-{seq:04}");
    let stored = match store_draft(&st, &ref_no, 1, &draft.ext, &draft.bytes) { Ok(s) => s, Err(r) => return r };
    let sha = sha256_hex(&draft.bytes);
    let size = draft.bytes.len() as i64;
    let note = b.note.take().unwrap_or_default().trim().to_string();
    let now = now_iso();
    let row = client.query_one(
        &format!("INSERT INTO doc_changes(ref_no,doc_code,status,reason,note,from_rev,new_rev,filename,ext,stored_name,size,sha256,source,base_version,prepared_by,prepared_name,prepared_position,version,created_at,updated_at,closed_at)
                  VALUES($1,$2,'review',$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,0,$17,$17,'') RETURNING {DCOLS}"),
        &[&ref_no, &code, &reason, &note, &from_rev, &new_rev, &draft.filename, &draft.ext, &stored, &size, &sha, &draft.source, &base_version,
          &u.emp_code, &u.name, &u.position, &now],
    ).await;
    let id: i64 = match row {
        Ok(r) => r.get(0),
        Err(e) => {
            let _ = fs::remove_file(changes_dir(&st).join(&stored));
            // ชน uq_dc_open = มีคนส่งคำขอของเอกสารเดียวกันพร้อมกัน
            let msg = if e.code() == Some(&tokio_postgres::error::SqlState::UNIQUE_VIOLATION) {
                "มีคำขอแก้ไขเอกสารนี้ที่ยังไม่จบอยู่แล้ว".to_string()
            } else { format!("บันทึกคำขอไม่สำเร็จ: {e}") };
            return err(StatusCode::CONFLICT, &msg);
        }
    };
    add_step(&client, id, "submit", "submit", &u, &reason, &sha).await;
    if let Some(eid) = b.editId { crate::editsession::finish(&st, &client, eid).await; }
    let _ = eform::audit(&client, &u, "dcr_submit", "doc_change", &id.to_string(), &format!("{ref_no} {code} {from_rev}→{new_rev}")).await;
    detail_resp(&st, &u, id).await
}

#[derive(Deserialize)]
pub struct ActReq { action: String, comment: Option<String>, pin: Option<String> }

/// ตรวจสอบ / อนุมัติ / ตีกลับ — อนุมัติขั้นสุดท้ายแล้วเอกสารมีผลบังคับใช้ทันที
pub async fn act(Path(id): Path<i64>, State(st): State<S>, Extension(u): Extension<AuthUser>, Json(b): Json<ActReq>) -> Response {
    let pass = b.action == "pass";
    if !pass && b.action != "return" { return err(StatusCode::BAD_REQUEST, "การดำเนินการไม่ถูกต้อง"); }
    let comment = b.comment.unwrap_or_default().trim().to_string();
    if !pass && comment.is_empty() { return err(StatusCode::BAD_REQUEST, "กรุณาระบุเหตุผลที่ตีกลับ"); }
    let Ok(client) = st.db.get().await else { return db_err() };
    let Some((ch, stored)) = load_change(&client, id).await else { return err(StatusCode::NOT_FOUND, "ไม่พบคำขอเปลี่ยนแปลง") };
    let status = ch["status"].as_str().unwrap_or("").to_string();
    let can = can_flags(&u, &ch);
    let step = match status.as_str() {
        "review" if can["review"] == json!(true) => "review",
        "approve" if can["approve"] == json!(true) => "approve",
        "review" | "approve" => return err(StatusCode::FORBIDDEN, "ไม่มีสิทธิ์ในขั้นนี้ (ผู้จัดทำตรวจ/อนุมัติคำขอของตนเองไม่ได้)"),
        _ => return err(StatusCode::BAD_REQUEST, "คำขอนี้ไม่ได้อยู่ในขั้นรอตรวจสอบ/อนุมัติ"),
    };
    if let Err(r) = eform::verify_pin(&st, &client, &u, b.pin.as_deref().unwrap_or("")).await { return r; }

    // ตรวจว่าไฟล์ฉบับร่างไม่ถูกแก้หลังผู้จัดทำลงนาม
    let bytes = match fs::read(changes_dir(&st).join(&stored)) {
        Ok(x) => x,
        Err(_) => return err(StatusCode::INTERNAL_SERVER_ERROR, "ไม่พบไฟล์ฉบับร่างในระบบ"),
    };
    let sha = sha256_hex(&bytes);
    if sha != ch["sha256"].as_str().unwrap_or("") {
        return err(StatusCode::CONFLICT, "ไฟล์ฉบับร่างไม่ตรงกับที่ผู้จัดทำลงนาม (SHA-256 ไม่ตรง) — ตีกลับให้ส่งใหม่");
    }

    let now = now_iso();
    let next = if !pass { "returned" } else if step == "review" { "approve" } else { "effective" };
    // เปลี่ยนสถานะแบบมีเงื่อนไข กันกดซ้ำ/สองคนอนุมัติพร้อมกัน
    let n = client.execute("UPDATE doc_changes SET status=$2,updated_at=$3 WHERE id=$1 AND status=$4", &[&id, &next, &now, &status]).await.unwrap_or(0);
    if n == 0 { return err(StatusCode::CONFLICT, "คำขอนี้ถูกดำเนินการไปแล้ว — โหลดหน้าใหม่"); }

    if next == "effective" {
        let code = ch["docCode"].as_str().unwrap_or("").to_string();
        let origin = st.by_code.get(&code).and_then(|d| d["path"].as_str()).filter(|p| !p.is_empty()).map(|p| st.docs_root.join(p));
        let eff = today_th();
        let nv = NewVersion {
            code: &code,
            filename: ch["filename"].as_str().unwrap_or(""),
            ext: ch["ext"].as_str().unwrap_or(""),
            source: ch["source"].as_str().unwrap_or("upload"),
            note: ch["note"].as_str().unwrap_or(""),
            signer_code: ch["preparedBy"].as_str().unwrap_or(""),
            signer_name: ch["preparedName"].as_str().unwrap_or(""),
            signer_role: ch["preparedPosition"].as_str().unwrap_or(""),
            rev: ch["newRev"].as_str().unwrap_or(""),
            effective_date: &eff,
            change_id: id,
            approved_by: &u.name,
            approved_at: &now,
        };
        match crate::save_new_version(&st, &nv, &bytes, origin.as_deref()).await {
            Ok((version, _)) => {
                let _ = client.execute("UPDATE doc_changes SET version=$2,closed_at=$3 WHERE id=$1", &[&id, &version, &now]).await;
                // แจกจ่าย Rev. ใหม่: ส่งคำขอรับทราบให้ทุกคนในรายชื่อ (7.5.3)
                crate::acks::on_effective(&st, &client, &code, version, ch["newRev"].as_str().unwrap_or(""), &u.name).await;
            }
            Err(r) => {
                let _ = client.execute("UPDATE doc_changes SET status='approve',updated_at=$2 WHERE id=$1", &[&id, &now]).await;
                return r;
            }
        }
    }
    let action = if pass { "pass" } else { "return" };
    add_step(&client, id, step, action, &u, &comment, &sha).await;
    let ref_no = ch["refNo"].as_str().unwrap_or("");
    let _ = eform::audit(&client, &u, &format!("dcr_{step}_{action}"), "doc_change", &id.to_string(), &format!("{ref_no} {comment}")).await;
    detail_resp(&st, &u, id).await
}

/// ผู้จัดทำแก้ตามที่ถูกตีกลับแล้วส่งใหม่ (แนบไฟล์ใหม่ได้ หรือใช้ไฟล์เดิม)
pub async fn resubmit(Path(id): Path<i64>, State(st): State<S>, Extension(u): Extension<AuthUser>, Json(mut b): Json<ChangeReq>) -> Response {
    let Ok(client) = st.db.get().await else { return db_err() };
    let Some((ch, stored)) = load_change(&client, id).await else { return err(StatusCode::NOT_FOUND, "ไม่พบคำขอเปลี่ยนแปลง") };
    if can_flags(&u, &ch)["resubmit"] != json!(true) { return err(StatusCode::FORBIDDEN, "ส่งใหม่ได้เฉพาะผู้จัดทำ เมื่อคำขอถูกตีกลับ"); }
    let code = ch["docCode"].as_str().unwrap_or("").to_string();
    let Some(doc) = st.by_code.get(&code).cloned() else { return err(StatusCode::NOT_FOUND, "ไม่พบเอกสาร") };
    let reason = b.reason.take().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).unwrap_or_else(|| ch["reason"].as_str().unwrap_or("").to_string());
    let note = b.note.take().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).unwrap_or_else(|| ch["note"].as_str().unwrap_or("").to_string());
    let from_rev = ch["fromRev"].as_str().unwrap_or("").to_string();
    let new_rev = b.newRev.take().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).unwrap_or_else(|| ch["newRev"].as_str().unwrap_or("").to_string());
    if let Err(r) = validate_rev(&new_rev, &from_rev) { return r; }
    if rev_used(&client, &code, &new_rev).await { return err(StatusCode::BAD_REQUEST, &format!("{new_rev} เคยใช้กับเอกสารนี้แล้ว")); }
    if let Err(r) = eform::verify_pin(&st, &client, &u, b.pin.as_deref().unwrap_or("")).await { return r; }

    let (mut filename, mut ext, mut stored_name, mut size, mut sha, mut source) = (
        ch["filename"].as_str().unwrap_or("").to_string(), ch["ext"].as_str().unwrap_or("").to_string(), stored,
        ch["size"].as_i64().unwrap_or(0), ch["sha256"].as_str().unwrap_or("").to_string(), ch["source"].as_str().unwrap_or("").to_string(),
    );
    if b.editId.is_some() || b.dataBase64.as_deref().map(|s| !s.is_empty()).unwrap_or(false) {
        let draft = match draft_of(&st, &client, &u, &doc, &code, id, &mut b).await { Ok(d) => d, Err(r) => return r };
        let rounds: i64 = client.query_one("SELECT COUNT(*) FROM doc_change_steps WHERE change_id=$1 AND action IN ('submit','resubmit')", &[&id])
            .await.map(|r| r.get(0)).unwrap_or(1);
        stored_name = match store_draft(&st, ch["refNo"].as_str().unwrap_or("DCR"), rounds + 1, &draft.ext, &draft.bytes) { Ok(s) => s, Err(r) => return r };
        sha = sha256_hex(&draft.bytes);
        size = draft.bytes.len() as i64;
        filename = draft.filename;
        ext = draft.ext;
        source = draft.source.to_string();
    }
    let now = now_iso();
    let n = client.execute(
        "UPDATE doc_changes SET status='review',reason=$2,note=$3,new_rev=$4,filename=$5,ext=$6,stored_name=$7,size=$8,sha256=$9,source=$10,updated_at=$11
         WHERE id=$1 AND status='returned'",
        &[&id, &reason, &note, &new_rev, &filename, &ext, &stored_name, &size, &sha, &source, &now],
    ).await.unwrap_or(0);
    if n == 0 { return err(StatusCode::CONFLICT, "คำขอนี้ถูกดำเนินการไปแล้ว — โหลดหน้าใหม่"); }
    add_step(&client, id, "submit", "resubmit", &u, &reason, &sha).await;
    if let Some(eid) = b.editId { crate::editsession::finish(&st, &client, eid).await; }
    let _ = eform::audit(&client, &u, "dcr_resubmit", "doc_change", &id.to_string(), ch["refNo"].as_str().unwrap_or("")).await;
    detail_resp(&st, &u, id).await
}

#[derive(Deserialize)]
pub struct ReasonReq { reason: Option<String>, pin: Option<String> }

pub async fn cancel(Path(id): Path<i64>, State(st): State<S>, Extension(u): Extension<AuthUser>, Json(b): Json<ReasonReq>) -> Response {
    let reason = b.reason.unwrap_or_default().trim().to_string();
    if reason.is_empty() { return err(StatusCode::BAD_REQUEST, "กรุณาระบุเหตุผลการยกเลิกคำขอ"); }
    let Ok(client) = st.db.get().await else { return db_err() };
    let Some((ch, _)) = load_change(&client, id).await else { return err(StatusCode::NOT_FOUND, "ไม่พบคำขอเปลี่ยนแปลง") };
    if can_flags(&u, &ch)["cancel"] != json!(true) { return err(StatusCode::FORBIDDEN, "ยกเลิกได้เฉพาะผู้จัดทำหรือผู้ดูแลระบบ ขณะคำขอยังไม่จบ"); }
    let now = now_iso();
    let n = client.execute(
        "UPDATE doc_changes SET status='cancelled',updated_at=$2,closed_at=$2 WHERE id=$1 AND status IN ('review','approve','returned')", &[&id, &now],
    ).await.unwrap_or(0);
    if n == 0 { return err(StatusCode::CONFLICT, "คำขอนี้ถูกดำเนินการไปแล้ว — โหลดหน้าใหม่"); }
    add_step(&client, id, "cancel", "cancel", &u, &reason, "").await;
    let _ = eform::audit(&client, &u, "dcr_cancel", "doc_change", &id.to_string(), &format!("{} {reason}", ch["refNo"].as_str().unwrap_or(""))).await;
    detail_resp(&st, &u, id).await
}

pub async fn detail(Path(id): Path<i64>, State(st): State<S>, Extension(u): Extension<AuthUser>) -> Response {
    detail_resp(&st, &u, id).await
}

/// คำขอทั้งหมดของเอกสารหนึ่งฉบับ (ใหม่ → เก่า) พร้อมขั้นตอนการลงนาม
pub async fn doc_changes(Path(code): Path<String>, State(st): State<S>, Extension(u): Extension<AuthUser>) -> Response {
    let Ok(client) = st.db.get().await else { return db_err() };
    let rows = match client.query(&format!("SELECT {DCOLS} FROM doc_changes WHERE doc_code=$1 ORDER BY id DESC"), &[&code]).await {
        Ok(r) => r, Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };
    let mut out = Vec::with_capacity(rows.len());
    for r in &rows {
        let mut ch = with_doc(&st, &u, change_json(r));
        ch["steps"] = json!(steps_of(&client, ch["id"].as_i64().unwrap_or(0)).await);
        out.push(ch);
    }
    Json(Value::Array(out)).into_response()
}

/// ทะเบียนคำขอเปลี่ยนแปลงทั้งหมด (หน้า "คำขอเปลี่ยนแปลงเอกสาร")
pub async fn list(State(st): State<S>, Extension(u): Extension<AuthUser>) -> Response {
    let Ok(client) = st.db.get().await else { return db_err() };
    match client.query(&format!("SELECT {DCOLS} FROM doc_changes ORDER BY id DESC LIMIT 1000"), &[]).await {
        Ok(rows) => Json(Value::Array(rows.iter().map(|r| with_doc(&st, &u, change_json(r))).collect())).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

async fn draft_path(st: &S, id: i64) -> Result<(std::path::PathBuf, Value), Response> {
    let Ok(client) = st.db.get().await else { return Err(db_err()) };
    let Some((ch, stored)) = load_change(&client, id).await else { return Err(err(StatusCode::NOT_FOUND, "ไม่พบคำขอเปลี่ยนแปลง")) };
    let p = changes_dir(st).join(stored);
    if !p.exists() { return Err(err(StatusCode::NOT_FOUND, "ไม่พบไฟล์ฉบับร่างในระบบ")); }
    Ok((p, ch))
}

pub async fn content(Path(id): Path<i64>, State(st): State<S>) -> Response {
    let (p, ch) = match draft_path(&st, id).await { Ok(x) => x, Err(r) => return r };
    let mut body = preview_abs(&st, &p, ch["ext"].as_str().unwrap_or("")).await;
    body["changeId"] = json!(id);
    Json(body).into_response()
}

pub async fn file(Path(id): Path<i64>, Query(q): Query<HashMap<String, String>>, State(st): State<S>) -> Response {
    let (p, ch) = match draft_path(&st, id).await { Ok(x) => x, Err(r) => return r };
    let Ok(bytes) = fs::read(&p) else { return err(StatusCode::NOT_FOUND, "อ่านไฟล์ไม่ได้") };
    let ext = ch["ext"].as_str().unwrap_or("");
    let status = ch["status"].as_str().unwrap_or("");
    let filename = ch["filename"].as_str().unwrap_or("");
    let name = if status == "effective" { filename.to_string() } else { format!("DRAFT {} - {filename}", ch["refNo"].as_str().unwrap_or("")) };
    let disp = if q.get("dl").map(|s| s == "1").unwrap_or(false) { "attachment" } else { "inline" };
    Response::builder()
        .header(header::CONTENT_TYPE, mime_of(ext))
        .header(header::CONTENT_DISPOSITION, format!("{disp}; filename*=UTF-8''{}", pct(&name)))
        .body(Body::from(bytes))
        .unwrap()
}

/// ประกาศยกเลิกใช้เอกสาร (ผู้อนุมัติ) — ยังเปิดดูย้อนหลังได้ แต่แก้ไข/ส่งคำขอไม่ได้ และไฟล์ดาวน์โหลดติดป้าย OBSOLETE
pub async fn obsolete(Path(code): Path<String>, State(st): State<S>, Extension(u): Extension<AuthUser>, Json(b): Json<ReasonReq>) -> Response {
    if !st.by_code.contains_key(&code) { return err(StatusCode::NOT_FOUND, "ไม่พบเอกสาร"); }
    if !eform::can_approve(&u.role) { return err(StatusCode::FORBIDDEN, "เฉพาะผู้อนุมัติหรือผู้ดูแลระบบ"); }
    let reason = b.reason.unwrap_or_default().trim().to_string();
    if reason.is_empty() { return err(StatusCode::BAD_REQUEST, "กรุณาระบุเหตุผลการยกเลิกใช้"); }
    let Ok(client) = st.db.get().await else { return db_err() };
    if let Ok(Some(r)) = client.query_opt("SELECT ref_no FROM doc_changes WHERE doc_code=$1 AND status IN ('review','approve','returned')", &[&code]).await {
        return err(StatusCode::CONFLICT, &format!("มีคำขอ {} ที่ยังไม่จบ — ยกเลิกคำขอก่อน", r.get::<_, String>(0)));
    }
    if let Err(r) = eform::verify_pin(&st, &client, &u, b.pin.as_deref().unwrap_or("")).await { return r; }
    set_status(&client, &code, "obsolete", &reason, &u).await;
    crate::acks::on_obsolete(&client, &code).await;
    let _ = eform::audit(&client, &u, "doc_obsolete", "document", &code, &reason).await;
    Json(json!({ "ok": true })).into_response()
}

/// นำเอกสารที่ยกเลิกใช้กลับมาใช้ (ผู้ดูแลระบบ)
pub async fn reinstate(Path(code): Path<String>, State(st): State<S>, Extension(u): Extension<AuthUser>, Json(b): Json<ReasonReq>) -> Response {
    if !st.by_code.contains_key(&code) { return err(StatusCode::NOT_FOUND, "ไม่พบเอกสาร"); }
    if !eform::is_admin(&u.role) { return err(StatusCode::FORBIDDEN, "เฉพาะผู้ดูแลระบบ"); }
    let reason = b.reason.unwrap_or_default().trim().to_string();
    if reason.is_empty() { return err(StatusCode::BAD_REQUEST, "กรุณาระบุเหตุผลการนำกลับมาใช้"); }
    let Ok(client) = st.db.get().await else { return db_err() };
    if let Err(r) = eform::verify_pin(&st, &client, &u, b.pin.as_deref().unwrap_or("")).await { return r; }
    set_status(&client, &code, "active", &reason, &u).await;
    crate::acks::on_reinstate(&st, &client, &code, &u.name).await;
    let _ = eform::audit(&client, &u, "doc_reinstate", "document", &code, &reason).await;
    Json(json!({ "ok": true })).into_response()
}

async fn set_status(c: &impl deadpool_postgres::GenericClient, code: &str, status: &str, reason: &str, u: &AuthUser) {
    let _ = c.execute(
        "INSERT INTO doc_status(doc_code,status,reason,actor_code,actor_name,at) VALUES($1,$2,$3,$4,$5,$6)
         ON CONFLICT(doc_code) DO UPDATE SET status=$2,reason=$3,actor_code=$4,actor_name=$5,at=$6",
        &[&code, &status, &reason, &u.emp_code, &u.name, &now_iso()],
    ).await;
}
