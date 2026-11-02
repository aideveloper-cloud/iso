// การแจกจ่ายและรับทราบเอกสาร (ISO 9001:2015 ข้อ 7.5.3.1–7.5.3.2, 7.3 การตระหนัก, 7.2 หลักฐานความสามารถ)
//   รายชื่อแจกจ่ายต่อเอกสาร (แผนก / บุคคล / บทบาท) — ค่าเริ่มต้น: แผนกเจ้าของเอกสาร, คู่มือคุณภาพ (QM) = ทุกคน
//   DCR มีผล (Rev. ใหม่) → ส่งคำขอรับทราบให้ทุกคนในรายชื่อ อัตโนมัติ (ครบกำหนด 14 วัน) · คำขอของ Rev. เก่าที่ค้าง → ถูกแทนที่
//   ผู้รับต้องเปิดอ่านฉบับนั้นก่อน แล้วลงนาม "อ่านและเข้าใจแล้ว" ด้วย PIN → บันทึกใน training_log เป็นหลักฐาน
use crate::eform::{self, AuthUser};
use crate::{err, now_iso, S};
use axum::{
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};

pub const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS doc_distribution(
  id BIGSERIAL PRIMARY KEY,
  doc_code TEXT NOT NULL,
  target_type TEXT NOT NULL,
  target TEXT NOT NULL,
  added_by TEXT NOT NULL,
  added_at TEXT NOT NULL,
  UNIQUE(doc_code, target_type, target));
CREATE TABLE IF NOT EXISTS doc_acks(
  id BIGSERIAL PRIMARY KEY,
  doc_code TEXT NOT NULL,
  version BIGINT NOT NULL,
  rev TEXT NOT NULL DEFAULT '',
  emp_code TEXT NOT NULL,
  emp_name TEXT NOT NULL,
  dept TEXT NOT NULL DEFAULT '',
  status TEXT NOT NULL DEFAULT 'pending',
  requested_by TEXT NOT NULL DEFAULT '',
  requested_at TEXT NOT NULL,
  due_date TEXT NOT NULL DEFAULT '',
  opened_at TEXT NOT NULL DEFAULT '',
  acked_at TEXT NOT NULL DEFAULT '',
  comment TEXT NOT NULL DEFAULT '',
  sha256 TEXT NOT NULL DEFAULT '',
  UNIQUE(doc_code, version, emp_code));
CREATE INDEX IF NOT EXISTS idx_acks_emp ON doc_acks(emp_code, status);
CREATE INDEX IF NOT EXISTS idx_acks_doc ON doc_acks(doc_code, version);
";

const COLS: &str = "id,doc_code,version,rev,emp_code,emp_name,dept,status,requested_by,requested_at,due_date,opened_at,acked_at,comment,sha256";
const DUE_DAYS: i64 = 14;

fn db_err() -> Response { err(StatusCode::INTERNAL_SERVER_ERROR, "เชื่อมต่อฐานข้อมูลไม่ได้") }
fn bad(m: &str) -> Response { err(StatusCode::BAD_REQUEST, m) }
fn forbid(m: &str) -> Response { err(StatusCode::FORBIDDEN, m) }
fn s(v: &Value, k: &str) -> String { v[k].as_str().unwrap_or("").to_string() }
fn today() -> String { crate::capa::today() }

fn ack_json(st: &S, r: &tokio_postgres::Row) -> Value {
    let g = |i: usize| r.get::<_, String>(i);
    let code = g(1);
    let status = g(7);
    let due = g(10);
    json!({
        "id": r.get::<_, i64>(0), "docCode": code, "docName": st.by_code.get(&code).and_then(|d| d["name"].as_str()).unwrap_or(""),
        "version": r.get::<_, i64>(2), "rev": g(3), "empCode": g(4), "empName": g(5), "dept": g(6), "status": status,
        "requestedBy": g(8), "requestedAt": g(9), "dueDate": due, "openedAt": g(11), "ackedAt": g(12), "comment": g(13), "sha256": g(14),
        "overdue": status == "pending" && !due.is_empty() && due < today(),
    })
}

/// รายชื่อแจกจ่ายที่ตั้งไว้ (ว่าง = ใช้ค่าเริ่มต้น)
async fn targets(c: &impl deadpool_postgres::GenericClient, code: &str) -> Vec<(String, String)> {
    c.query("SELECT target_type,target FROM doc_distribution WHERE doc_code=$1 ORDER BY target_type,target", &[&code]).await
        .map(|r| r.iter().map(|x| (x.get(0), x.get(1))).collect()).unwrap_or_default()
}
fn default_targets(st: &S, code: &str) -> Vec<(String, String)> {
    let d = st.by_code.get(code);
    if d.and_then(|d| d["type"].as_str()) == Some("QM") { return vec![("all".into(), "*".into())]; }
    d.and_then(|d| d["dept"].as_str()).filter(|x| !x.is_empty()).map(|x| vec![("dept".to_string(), x.to_string())]).unwrap_or_default()
}

/// แปลงรายชื่อแจกจ่าย → พนักงานที่ใช้งานอยู่ (รหัส, ชื่อ, แผนก)
async fn resolve(c: &impl deadpool_postgres::GenericClient, ts: &[(String, String)]) -> Vec<(String, String, String)> {
    let Ok(rows) = c.query("SELECT emp_code,name_th,dept,role,position FROM employees WHERE status='active' ORDER BY emp_code", &[]).await else { return vec![] };
    rows.iter().filter(|r| {
        let (code, dept, role, position): (String, String, String, String) = (r.get(0), r.get(2), r.get(3), r.get(4));
        ts.iter().any(|(t, v)| match t.as_str() {
            "all" => true, "dept" => *v == dept, "emp" => *v == code, "role" => *v == role, "position" => !v.trim().is_empty() && v.trim() == position.trim(), _ => false,
        })
    }).map(|r| (r.get(0), r.get(1), r.get(2))).collect()
}

async fn effective_targets(st: &S, c: &impl deadpool_postgres::GenericClient, code: &str) -> (Vec<(String, String)>, bool) {
    let t = targets(c, code).await;
    if t.is_empty() { (default_targets(st, code), true) } else { (t, false) }
}

/// เวอร์ชันใช้งานปัจจุบัน (version, rev, sha256)
async fn current_version(st: &S, c: &impl deadpool_postgres::GenericClient, code: &str) -> Option<(i64, String, String)> {
    let r = c.query_opt("SELECT version,rev,sha256 FROM versions WHERE doc_code=$1 ORDER BY version DESC LIMIT 1", &[&code]).await.ok()??;
    let rev: String = r.get(1);
    let rev = if rev.is_empty() { st.by_code.get(code).and_then(|d| d["rev"].as_str()).unwrap_or("").to_string() } else { rev };
    Some((r.get(0), rev, r.get(2)))
}

/// สร้างคำขอรับทราบให้ทุกคนในรายชื่อที่ยังไม่มีคำขอของเวอร์ชันนี้ — คืนจำนวนที่สร้าง
async fn request_for(st: &S, c: &impl deadpool_postgres::GenericClient, code: &str, version: i64, rev: &str, sha: &str, by: &str) -> usize {
    let (mut ts, _) = effective_targets(st, c, code).await;
    // ผู้ที่ Training matrix กำหนดให้ต้องรับทราบเอกสารนี้ (7.2) ได้คำขอด้วย แม้ไม่อยู่ในรายชื่อแจกจ่าย
    if let Ok(rows) = c.query("SELECT target_type,target FROM training_reqs WHERE active AND kind='doc' AND doc_code=$1", &[&code]).await {
        ts.extend(rows.iter().map(|r| (r.get::<_, String>(0), r.get::<_, String>(1))));
    }
    let people = resolve(c, &ts).await;
    let now = now_iso();
    let due = chrono::NaiveDate::parse_from_str(&today(), "%Y-%m-%d").map(|d| (d + chrono::Duration::days(DUE_DAYS)).format("%Y-%m-%d").to_string()).unwrap_or_default();
    // คำขอของเวอร์ชันเก่าที่ยังค้าง → ถูกแทนที่ด้วยฉบับใหม่
    let _ = c.execute("UPDATE doc_acks SET status='superseded' WHERE doc_code=$1 AND version<$2 AND status='pending'", &[&code, &version]).await;
    let mut n = 0;
    for (emp, name, dept) in people {
        let r = c.execute(
            "INSERT INTO doc_acks(doc_code,version,rev,emp_code,emp_name,dept,status,requested_by,requested_at,due_date,sha256)
             VALUES($1,$2,$3,$4,$5,$6,'pending',$7,$8,$9,$10)
             ON CONFLICT(doc_code,version,emp_code) DO UPDATE SET status='pending', requested_by=EXCLUDED.requested_by,
               requested_at=EXCLUDED.requested_at, due_date=EXCLUDED.due_date
             WHERE doc_acks.status='superseded'",
            &[&code, &version, &rev, &emp, &name, &dept, &by, &now, &due, &sha],
        ).await.unwrap_or(0);
        n += r as usize;
    }
    n
}

// ---------- จุดเชื่อมจากโมดูลอื่น ----------

/// เรียกเมื่อ DCR มีผล — ส่งคำขอรับทราบ Rev. ใหม่อัตโนมัติ
pub async fn on_effective(st: &S, c: &impl deadpool_postgres::GenericClient, code: &str, version: i64, rev: &str, approver: &str) {
    let sha = c.query_opt("SELECT sha256 FROM versions WHERE doc_code=$1 AND version=$2", &[&code, &version]).await.ok().flatten().map(|r| r.get::<_, String>(0)).unwrap_or_default();
    let n = request_for(st, c, code, version, rev, &sha, &format!("ระบบ (DCR อนุมัติโดย {approver})")).await;
    if n > 0 { println!("  📨 ส่งคำขอรับทราบ {code} {rev} ถึง {n} คน"); }
}

/// เรียกเมื่อเอกสารถูกยกเลิกใช้ — คำขอที่ค้างไม่ต้องรับทราบแล้ว
pub async fn on_obsolete(c: &impl deadpool_postgres::GenericClient, code: &str) {
    let _ = c.execute("UPDATE doc_acks SET status='superseded' WHERE doc_code=$1 AND status='pending'", &[&code]).await;
}

/// เรียกเมื่อนำเอกสารที่ยกเลิกใช้กลับมาใช้ — ส่งคำขอรับทราบฉบับปัจจุบันอีกครั้ง (คำขอที่ถูกแทนที่ของฉบับเดียวกันกลับเป็นรอรับทราบ)
pub async fn on_reinstate(st: &S, c: &impl deadpool_postgres::GenericClient, code: &str, by: &str) {
    if let Some((v, rev, sha)) = current_version(st, c, code).await {
        request_for(st, c, code, v, &rev, &sha, &format!("ระบบ (นำกลับมาใช้โดย {by})")).await;
    }
}

/// เรียกเมื่อ Training matrix กำหนด/แก้ข้อกำหนด "รับทราบเอกสาร" — ส่งคำขอฉบับปัจจุบันให้ผู้ที่ต้องรับทราบทันที (คนที่มีคำขอแล้วไม่ถูกส่งซ้ำ)
pub async fn on_training_req(st: &S, c: &impl deadpool_postgres::GenericClient, code: &str, by: &str) {
    if c.query_opt("SELECT 1 FROM doc_status WHERE doc_code=$1 AND status='obsolete'", &[&code]).await.ok().flatten().is_some() { return; }
    if let Some((v, rev, sha)) = current_version(st, c, code).await {
        request_for(st, c, code, v, &rev, &sha, &format!("ระบบ (Training matrix โดย {by})")).await;
    }
}

/// บันทึกว่าผู้ใช้เปิดอ่านเวอร์ชันนี้แล้ว (เงื่อนไขก่อนกดรับทราบ)
pub async fn mark_opened(st: &S, u: &AuthUser, code: &str, version: i64) {
    if let Ok(c) = st.db.get().await {
        let _ = c.execute("UPDATE doc_acks SET opened_at=$4 WHERE doc_code=$1 AND version=$2 AND emp_code=$3 AND opened_at=''", &[&code, &version, &u.emp_code, &now_iso()]).await;
    }
}

/// จำนวนคำขอที่รอผู้ใช้รับทราบ (ตัวเลขบนเมนู)
pub async fn pending_count(st: &S, u: &AuthUser) -> i64 {
    let Ok(c) = st.db.get().await else { return 0 };
    c.query_one("SELECT COUNT(*) FROM doc_acks WHERE emp_code=$1 AND status='pending'", &[&u.emp_code]).await.map(|r| r.get(0)).unwrap_or(0)
}

/// สำหรับหน้าเตรียม Audit / ทบทวนฝ่ายบริหาร: อัตรารับทราบของคำขอที่ยังมีผล + รายการเลยกำหนด
pub async fn facts(st: &S, c: &impl deadpool_postgres::GenericClient) -> Value {
    // นับเฉพาะคำขอของฉบับใช้งานปัจจุบัน (การรับทราบ Rev. เก่าเป็นประวัติ ไม่นับในอัตรา)
    let rows = c.query(&format!("SELECT {COLS} FROM doc_acks WHERE status IN ('pending','acknowledged') AND version=(SELECT MAX(v.version) FROM versions v WHERE v.doc_code=doc_acks.doc_code)"), &[]).await.unwrap_or_default();
    let all: Vec<Value> = rows.iter().map(|r| ack_json(st, r)).collect();
    let done = all.iter().filter(|a| a["status"] == json!("acknowledged")).count();
    let overdue: Vec<&Value> = all.iter().filter(|a| a["overdue"] == json!(true)).collect();
    let mut docs: Vec<String> = overdue.iter().map(|a| s(a, "docCode")).collect::<HashSet<_>>().into_iter().collect();
    docs.sort();
    json!({ "total": all.len(), "acknowledged": done, "pending": all.len() - done, "overdue": overdue.len(), "overdueDocs": docs,
            "rate": if all.is_empty() { Value::Null } else { json!(((done as f64) / (all.len() as f64) * 100.0).round() as i64) } })
}

// ---------- handlers ----------

/// เอกสารที่ฉันต้องรับทราบ + ประวัติ
pub async fn mine(State(st): State<S>, Extension(u): Extension<AuthUser>) -> Response {
    let Ok(c) = st.db.get().await else { return db_err() };
    let rows = c.query(&format!("SELECT {COLS} FROM doc_acks WHERE emp_code=$1 ORDER BY (status='pending') DESC, requested_at DESC LIMIT 300"), &[&u.emp_code]).await.unwrap_or_default();
    Json(Value::Array(rows.iter().map(|r| ack_json(&st, r)).collect())).into_response()
}

/// การแจกจ่ายและการรับทราบของเอกสารหนึ่งฉบับ
pub async fn doc(Path(code): Path<String>, State(st): State<S>, Extension(u): Extension<AuthUser>) -> Response {
    if !st.by_code.contains_key(&code) { return err(StatusCode::NOT_FOUND, "ไม่พบเอกสาร"); }
    let Ok(c) = st.db.get().await else { return db_err() };
    let (ts, is_default) = effective_targets(&st, &c, &code).await;
    let people = resolve(&c, &ts).await;
    let cur = current_version(&st, &c, &code).await;
    let rows = c.query(&format!("SELECT {COLS} FROM doc_acks WHERE doc_code=$1 ORDER BY version DESC, emp_code"), &[&code]).await.unwrap_or_default();
    let acks: Vec<Value> = rows.iter().map(|r| ack_json(&st, r)).collect();
    let mine = acks.iter().find(|a| s(a, "empCode") == u.emp_code && a["status"] == json!("pending")).cloned();
    Json(json!({
        "code": code, "targets": ts.iter().map(|(t, v)| json!({ "type": t, "value": v })).collect::<Vec<_>>(), "isDefault": is_default,
        "people": people.iter().map(|(c, n, d)| json!({ "code": c, "name": n, "dept": d })).collect::<Vec<_>>(),
        "currentVersion": cur.as_ref().map(|x| x.0), "currentRev": cur.as_ref().map(|x| x.1.clone()),
        "acks": acks, "mine": mine,
        "canManage": eform::can_approve(&u.role),
    })).into_response()
}

#[derive(Deserialize)]
pub struct Target { r#type: String, value: String }
#[derive(Deserialize)]
pub struct DistReq { targets: Vec<Target> }

/// ตั้งรายชื่อแจกจ่าย (ผู้อนุมัติขึ้นไป) — ส่งรายการว่าง = กลับไปใช้ค่าเริ่มต้น
pub async fn set_distribution(Path(code): Path<String>, State(st): State<S>, Extension(u): Extension<AuthUser>, Json(b): Json<DistReq>) -> Response {
    if !st.by_code.contains_key(&code) { return err(StatusCode::NOT_FOUND, "ไม่พบเอกสาร"); }
    if !eform::can_approve(&u.role) { return forbid("กำหนดการแจกจ่ายได้เฉพาะผู้อนุมัติ หรือผู้ดูแลระบบ"); }
    let Ok(mut c) = st.db.get().await else { return db_err() };
    let mut clean = vec![];
    for t in b.targets.into_iter().take(200) {
        let v = t.value.trim().chars().take(40).collect::<String>();
        match t.r#type.as_str() {
            "all" => clean.push(("all".to_string(), "*".to_string())),
            "dept" | "emp" | "role" if !v.is_empty() => clean.push((t.r#type.clone(), v)),
            _ => return bad("รายชื่อแจกจ่ายไม่ถูกต้อง"),
        }
    }
    let Ok(tx) = c.transaction().await else { return db_err() };
    let _ = tx.execute("DELETE FROM doc_distribution WHERE doc_code=$1", &[&code]).await;
    let now = now_iso();
    for (t, v) in &clean {
        let _ = tx.execute("INSERT INTO doc_distribution(doc_code,target_type,target,added_by,added_at) VALUES($1,$2,$3,$4,$5) ON CONFLICT DO NOTHING", &[&code, t, v, &u.name, &now]).await;
    }
    if tx.commit().await.is_err() { return db_err(); }
    let _ = eform::audit(&c, &u, "ack_distribution", "document", &code, &clean.iter().map(|(t, v)| format!("{t}:{v}")).collect::<Vec<_>>().join(", ")).await;
    doc(Path(code), State(st), Extension(u)).await
}

/// ส่งคำขอรับทราบฉบับปัจจุบันให้ทุกคนในรายชื่อที่ยังไม่ได้รับ (ผู้อนุมัติขึ้นไป — เช่นตอนเริ่มใช้ระบบ หรือเพิ่มคนใหม่)
pub async fn request(Path(code): Path<String>, State(st): State<S>, Extension(u): Extension<AuthUser>) -> Response {
    if !st.by_code.contains_key(&code) { return err(StatusCode::NOT_FOUND, "ไม่พบเอกสาร"); }
    if !eform::can_approve(&u.role) { return forbid("ส่งคำขอรับทราบได้เฉพาะผู้อนุมัติขึ้นไป"); }
    let Ok(c) = st.db.get().await else { return db_err() };
    if c.query_opt("SELECT 1 FROM doc_status WHERE doc_code=$1 AND status='obsolete'", &[&code]).await.ok().flatten().is_some() { return bad("เอกสารนี้ยกเลิกใช้แล้ว"); }
    let Some((v, rev, sha)) = current_version(&st, &c, &code).await else { return bad("ยังไม่มีเวอร์ชันในระบบ") };
    let n = request_for(&st, &c, &code, v, &rev, &sha, &u.name).await;
    let _ = eform::audit(&c, &u, "ack_request", "document", &code, &format!("{rev} #{v} ส่ง {n} คน")).await;
    let mut resp = doc(Path(code), State(st), Extension(u)).await;
    resp.headers_mut().insert("x-requested", n.to_string().parse().unwrap());
    resp
}

/// ส่งคำขอรับทราบฉบับปัจจุบันของเอกสารหลายฉบับพร้อมกัน (ผู้อนุมัติขึ้นไป) — เช่น ทุกฉบับของแผนก
#[derive(Deserialize)]
pub struct BulkReq { codes: Vec<String> }
pub async fn request_bulk(State(st): State<S>, Extension(u): Extension<AuthUser>, Json(b): Json<BulkReq>) -> Response {
    if !eform::can_approve(&u.role) { return forbid("ส่งคำขอรับทราบได้เฉพาะผู้อนุมัติขึ้นไป"); }
    let Ok(c) = st.db.get().await else { return db_err() };
    let (mut docs, mut people) = (0, 0);
    for code in b.codes.iter().filter(|x| st.by_code.contains_key(*x)).take(500) {
        if c.query_opt("SELECT 1 FROM doc_status WHERE doc_code=$1 AND status='obsolete'", &[code]).await.ok().flatten().is_some() { continue; }
        let Some((v, rev, sha)) = current_version(&st, &c, code).await else { continue };
        let n = request_for(&st, &c, code, v, &rev, &sha, &u.name).await;
        if n > 0 { docs += 1; people += n; }
    }
    let _ = eform::audit(&c, &u, "ack_request", "document", "", &format!("ส่ง {docs} ฉบับ รวม {people} คำขอ")).await;
    Json(json!({ "ok": true, "docs": docs, "requests": people })).into_response()
}

#[derive(Deserialize)]
pub struct AckReq { pin: Option<String>, comment: Option<String> }

/// ลงนาม "อ่านและเข้าใจแล้ว" (เฉพาะเจ้าของคำขอ, ต้องเปิดอ่านฉบับนั้นแล้ว, PIN) → บันทึกใน training_log
pub async fn acknowledge(Path(id): Path<i64>, State(st): State<S>, Extension(u): Extension<AuthUser>, Json(b): Json<AckReq>) -> Response {
    let Ok(c) = st.db.get().await else { return db_err() };
    let Some(row) = c.query_opt(&format!("SELECT {COLS} FROM doc_acks WHERE id=$1"), &[&id]).await.ok().flatten() else { return err(StatusCode::NOT_FOUND, "ไม่พบคำขอ") };
    let a = ack_json(&st, &row);
    if s(&a, "empCode") != u.emp_code { return forbid("รับทราบแทนผู้อื่นไม่ได้"); }
    if a["status"] != json!("pending") { return bad("คำขอนี้รับทราบแล้วหรือถูกแทนที่ด้วยฉบับใหม่"); }
    if s(&a, "openedAt").is_empty() { return bad("กรุณาเปิดอ่านเอกสารฉบับนี้ก่อนกดรับทราบ"); }
    if let Err(r) = eform::verify_pin(&st, &c, &u, b.pin.as_deref().unwrap_or("")).await { return r; }
    let comment: String = b.comment.unwrap_or_default().trim().chars().take(1000).collect();
    let now = now_iso();
    let n = c.execute("UPDATE doc_acks SET status='acknowledged', acked_at=$2, comment=$3 WHERE id=$1 AND status='pending'", &[&id, &now, &comment]).await.unwrap_or(0);
    if n == 0 { return bad("คำขอนี้ถูกดำเนินการไปแล้ว"); }
    // หลักฐานการรับรู้/ความสามารถ (7.2, 7.3) ในประวัติการอบรม
    let code = s(&a, "docCode");
    let course = format!("รับทราบเอกสาร {code} {} {}", s(&a, "docName"), s(&a, "rev"));
    let _ = c.execute(
        "INSERT INTO training_log(record_id,emp_code,name,course,train_date,result,ref_no,dept) VALUES(0,$1,$2,$3,$4,'อ่านและเข้าใจแล้ว',$5,$6)",
        &[&u.emp_code, &u.name, &course, &today(), &format!("{code} {}", s(&a, "rev")), &u.dept],
    ).await;
    let _ = eform::audit(&c, &u, "ack_sign", "document", &code, &format!("{} #{}", s(&a, "rev"), a["version"])).await;
    Json(json!({ "ok": true })).into_response()
}

/// ภาพรวมการรับทราบ (ผู้อนุมัติขึ้นไป): ต่อเอกสาร (ฉบับปัจจุบัน) และรายการค้าง/เลยกำหนด
pub async fn overview(State(st): State<S>, Extension(u): Extension<AuthUser>, Query(q): Query<HashMap<String, String>>) -> Response {
    if !eform::can_approve(&u.role) { return forbid("ดูภาพรวมได้เฉพาะผู้อนุมัติขึ้นไป"); }
    let Ok(c) = st.db.get().await else { return db_err() };
    let rows = c.query(&format!("SELECT {COLS} FROM doc_acks WHERE status IN ('pending','acknowledged') AND version=(SELECT MAX(v.version) FROM versions v WHERE v.doc_code=doc_acks.doc_code) ORDER BY doc_code, emp_code"), &[]).await.unwrap_or_default();
    let all: Vec<Value> = rows.iter().map(|r| ack_json(&st, r)).collect();
    let mut per: BTreeMap<String, (String, String, i64, usize, usize, usize)> = BTreeMap::new();
    for a in &all {
        let e = per.entry(s(a, "docCode")).or_insert((s(a, "docName"), s(a, "rev"), a["version"].as_i64().unwrap_or(0), 0, 0, 0));
        e.3 += 1;
        if a["status"] == json!("acknowledged") { e.4 += 1; }
        if a["overdue"] == json!(true) { e.5 += 1; }
    }
    let docs: Vec<Value> = per.into_iter().map(|(code, (name, rev, v, total, done, late))| json!({
        "docCode": code, "docName": name, "rev": rev, "version": v, "total": total, "acknowledged": done, "overdue": late,
        "rate": ((done as f64) / (total.max(1) as f64) * 100.0).round() as i64,
    })).collect();
    let only = q.get("status").cloned().unwrap_or_default();
    let list: Vec<&Value> = all.iter().filter(|a| match only.as_str() { "overdue" => a["overdue"] == json!(true), "pending" => a["status"] == json!("pending"), "" => true, x => a["status"] == json!(x) }).collect();
    Json(json!({ "summary": facts(&st, &c).await, "docs": docs, "acks": list })).into_response()
}
