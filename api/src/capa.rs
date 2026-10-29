// CAR / DAR / PAR — การแก้ไขและป้องกันครบวงจร (ISO 9001:2015 ข้อ 10.2, 6.1)
//   เปิด → กำลังดำเนินการ → รอทวนสอบ → ปิด (หรือยกเลิกพร้อมเหตุผล)
//   ต้องมี: การแก้ไขเฉพาะหน้า (containment) · วิเคราะห์สาเหตุ (root cause + วิธีวิเคราะห์) · การแก้ไขป้องกัน
//   ปิดได้เมื่อผู้อนุมัติที่ไม่ใช่ผู้รับผิดชอบทวนสอบว่า "มีประสิทธิผล" และลงนามด้วย PIN
//   ผู้เปิดใบ/ผู้รับผิดชอบเป็นบัญชีพนักงานจริง · ทุกขั้นเก็บประวัติ (action_steps) และ audit_log
use crate::eform::{self, AuthUser};
use crate::{err, now_iso, S};
use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

pub const SCHEMA: &str = "
ALTER TABLE actions ADD COLUMN IF NOT EXISTS version BIGINT NOT NULL DEFAULT 1;
ALTER TABLE actions ADD COLUMN IF NOT EXISTS source TEXT NOT NULL DEFAULT '';
ALTER TABLE actions ADD COLUMN IF NOT EXISTS raised_code TEXT NOT NULL DEFAULT '';
ALTER TABLE actions ADD COLUMN IF NOT EXISTS assignee_code TEXT NOT NULL DEFAULT '';
ALTER TABLE actions ADD COLUMN IF NOT EXISTS containment TEXT NOT NULL DEFAULT '';
ALTER TABLE actions ADD COLUMN IF NOT EXISTS rca_method TEXT NOT NULL DEFAULT '';
ALTER TABLE actions ADD COLUMN IF NOT EXISTS root_cause TEXT NOT NULL DEFAULT '';
ALTER TABLE actions ADD COLUMN IF NOT EXISTS verify_due TEXT NOT NULL DEFAULT '';
ALTER TABLE actions ADD COLUMN IF NOT EXISTS effectiveness TEXT NOT NULL DEFAULT 'pending';
ALTER TABLE actions ADD COLUMN IF NOT EXISTS verify_note TEXT NOT NULL DEFAULT '';
ALTER TABLE actions ADD COLUMN IF NOT EXISTS verified_by TEXT NOT NULL DEFAULT '';
ALTER TABLE actions ADD COLUMN IF NOT EXISTS verified_code TEXT NOT NULL DEFAULT '';
ALTER TABLE actions ADD COLUMN IF NOT EXISTS verified_at TEXT NOT NULL DEFAULT '';
ALTER TABLE actions ADD COLUMN IF NOT EXISTS risk_review TEXT NOT NULL DEFAULT '';
ALTER TABLE actions ADD COLUMN IF NOT EXISTS linked_ref TEXT NOT NULL DEFAULT '';
ALTER TABLE actions ADD COLUMN IF NOT EXISTS cancel_reason TEXT NOT NULL DEFAULT '';
CREATE TABLE IF NOT EXISTS action_steps(
  id BIGSERIAL PRIMARY KEY,
  action_id BIGINT NOT NULL,
  event TEXT NOT NULL,
  actor_code TEXT NOT NULL,
  actor_name TEXT NOT NULL,
  detail TEXT NOT NULL DEFAULT '',
  at TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS idx_as_action ON action_steps(action_id);
";

const COLS: &str = "id,ref_no,kind,title,detail,dept,doc_code,raised_by,assignee,priority,status,progress,due_date,action_taken,created_at,updated_at,closed_at,
  version,source,raised_code,assignee_code,containment,rca_method,root_cause,verify_due,effectiveness,verify_note,verified_by,verified_code,verified_at,risk_review,linked_ref,cancel_reason";

pub const KINDS: [&str; 3] = ["CAR", "DAR", "PAR"];
const SOURCES: [&str; 8] = ["internal_audit", "external_audit", "customer", "process", "product", "supplier", "mr", "other"];
const OPEN: [&str; 3] = ["open", "in_progress", "verify"];

fn db_err() -> Response { err(StatusCode::INTERNAL_SERVER_ERROR, "เชื่อมต่อฐานข้อมูลไม่ได้") }
fn bad(m: &str) -> Response { err(StatusCode::BAD_REQUEST, m) }
fn forbid(m: &str) -> Response { err(StatusCode::FORBIDDEN, m) }
fn s(v: &Value, k: &str) -> String { v[k].as_str().unwrap_or("").to_string() }
fn clip(t: &str, n: usize) -> String { t.trim().chars().take(n).collect() }
fn valid_date(d: &str) -> bool { d.is_empty() || chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").is_ok() }
pub fn today() -> String {
    let tz = chrono::FixedOffset::east_opt(7 * 3600).unwrap();
    chrono::Utc::now().with_timezone(&tz).format("%Y-%m-%d").to_string()
}

pub fn row_json(r: &tokio_postgres::Row) -> Value {
    let g = |i: usize| r.get::<_, String>(i);
    json!({
        "id": r.get::<_, i64>(0), "refNo": g(1), "kind": g(2), "title": g(3), "detail": g(4), "dept": g(5), "docCode": g(6),
        "raisedBy": g(7), "assignee": g(8), "priority": g(9), "status": g(10), "progress": r.get::<_, i64>(11), "dueDate": g(12),
        "actionTaken": g(13), "createdAt": g(14), "updatedAt": g(15), "closedAt": g(16),
        "version": r.get::<_, i64>(17), "source": g(18), "raisedCode": g(19), "assigneeCode": g(20), "containment": g(21),
        "rcaMethod": g(22), "rootCause": g(23), "verifyDue": g(24), "effectiveness": g(25), "verifyNote": g(26),
        "verifiedBy": g(27), "verifiedCode": g(28), "verifiedAt": g(29), "riskReview": g(30), "linkedRef": g(31), "cancelReason": g(32),
    })
}

/// สิทธิ์ของผู้ใช้ต่อใบนี้ (คำนวณฝั่ง server ส่งให้หน้าเว็บ)
fn can_flags(u: &AuthUser, a: &Value) -> Value {
    let st = s(a, "status");
    let me = u.emp_code.as_str();
    let assignee = s(a, "assigneeCode") == me;
    let raiser = s(a, "raisedCode") == me;
    let approver = eform::can_approve(&u.role);
    let write = u.role != "viewer";
    let work = write && (assignee || raiser || approver);
    json!({
        "edit": OPEN[..2].contains(&st.as_str()) && work,
        "start": st == "open" && write && (assignee || approver),
        "submit": st == "in_progress" && write && (assignee || approver),
        "verify": st == "verify" && approver && (eform::is_admin(&u.role) || !assignee),
        "cancel": OPEN.contains(&st.as_str()) && approver,
    })
}

/// เงื่อนไขก่อนส่งทวนสอบ — แยกตามชนิดใบ
fn submit_gaps(a: &Value) -> Vec<&'static str> {
    let kind = s(a, "kind");
    let mut gaps = vec![];
    if s(a, "assigneeCode").is_empty() { gaps.push("ผู้รับผิดชอบ"); }
    if s(a, "actionTaken").trim().is_empty() { gaps.push("การดำเนินการแก้ไข/ป้องกัน"); }
    if kind != "DAR" && s(a, "rootCause").trim().is_empty() { gaps.push(if kind == "PAR" { "สาเหตุที่อาจเกิด" } else { "สาเหตุที่แท้จริง (root cause)" }); }
    if kind == "CAR" && s(a, "containment").trim().is_empty() { gaps.push("การแก้ไขเฉพาะหน้า (containment)"); }
    gaps
}

async fn load(c: &impl deadpool_postgres::GenericClient, id: i64) -> Option<Value> {
    let r = c.query_opt(&format!("SELECT {COLS} FROM actions WHERE id=$1"), &[&id]).await.ok()??;
    Some(row_json(&r))
}
async fn step(c: &impl deadpool_postgres::GenericClient, id: i64, event: &str, u: &AuthUser, detail: &str) {
    let _ = c.execute("INSERT INTO action_steps(action_id,event,actor_code,actor_name,detail,at) VALUES($1,$2,$3,$4,$5,$6)",
        &[&id, &event.to_string(), &u.emp_code, &u.name, &detail.to_string(), &now_iso()]).await;
}
async fn employee(c: &impl deadpool_postgres::GenericClient, code: &str) -> Option<String> {
    c.query_opt("SELECT name_th FROM employees WHERE emp_code=$1 AND status='active'", &[&code]).await.ok().flatten().map(|r| r.get(0))
}

async fn detail_resp(st: &S, u: &AuthUser, id: i64) -> Response {
    let Ok(c) = st.db.get().await else { return db_err() };
    let Some(mut a) = load(&c, id).await else { return err(StatusCode::NOT_FOUND, "ไม่พบใบนี้") };
    a["can"] = can_flags(u, &a);
    a["gaps"] = json!(submit_gaps(&a));
    a["steps"] = json!(c.query("SELECT event,actor_code,actor_name,detail,at FROM action_steps WHERE action_id=$1 ORDER BY id", &[&id]).await
        .map(|rows| rows.iter().map(|r| json!({ "event": r.get::<_, String>(0), "actorCode": r.get::<_, String>(1), "actorName": r.get::<_, String>(2),
            "detail": r.get::<_, String>(3), "at": r.get::<_, String>(4) })).collect::<Vec<_>>()).unwrap_or_default());
    Json(json!({ "ok": true, "action": a })).into_response()
}

// ---------- handlers ----------

pub async fn list(State(st): State<S>, Extension(u): Extension<AuthUser>) -> Response {
    let Ok(c) = st.db.get().await else { return db_err() };
    match c.query(&format!("SELECT {COLS} FROM actions ORDER BY created_at DESC"), &[]).await {
        Ok(rows) => Json(Value::Array(rows.iter().map(|r| { let mut a = row_json(r); a["can"] = can_flags(&u, &a); a }).collect())).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, &format!("อ่านรายการไม่ได้: {e}")),
    }
}

pub async fn get(Path(id): Path<i64>, State(st): State<S>, Extension(u): Extension<AuthUser>) -> Response { detail_resp(&st, &u, id).await }

#[derive(Deserialize, Default)]
#[allow(non_snake_case)]
pub struct Fields {
    pub kind: Option<String>, pub title: Option<String>, pub detail: Option<String>, pub dept: Option<String>, pub docCode: Option<String>,
    pub source: Option<String>, pub assigneeCode: Option<String>, pub priority: Option<String>, pub dueDate: Option<String>,
    pub containment: Option<String>, pub rcaMethod: Option<String>, pub rootCause: Option<String>, pub actionTaken: Option<String>,
    pub progress: Option<i64>, pub verifyDue: Option<String>, pub riskReview: Option<String>, pub linkedRef: Option<String>,
}

/// ตรวจและรวมค่าที่ส่งมาเข้ากับค่าเดิม
async fn apply(c: &impl deadpool_postgres::GenericClient, st: &S, cur: &mut Value, f: Fields) -> Result<(), Response> {
    if let Some(t) = f.title { let t = clip(&t, 300); if t.is_empty() { return Err(bad("ต้องระบุเรื่อง")); } cur["title"] = json!(t); }
    if let Some(x) = f.detail { cur["detail"] = json!(clip(&x, 5000)); }
    if let Some(x) = f.dept { cur["dept"] = json!(clip(&x, 20)); }
    if let Some(x) = f.docCode {
        let x = clip(&x, 60);
        if !x.is_empty() && !st.by_code.contains_key(&x) { return Err(bad("ไม่พบรหัสเอกสารอ้างอิงในทะเบียน")); }
        cur["docCode"] = json!(x);
    }
    if let Some(x) = f.source { if !x.is_empty() && !SOURCES.contains(&x.as_str()) { return Err(bad("แหล่งที่มาไม่ถูกต้อง")); } cur["source"] = json!(x); }
    if let Some(code) = f.assigneeCode {
        let code = clip(&code, 20);
        let name = if code.is_empty() { String::new() } else { employee(c, &code).await.ok_or_else(|| bad("ไม่พบผู้รับผิดชอบในรายชื่อพนักงาน"))? };
        cur["assigneeCode"] = json!(code); cur["assignee"] = json!(name);
    }
    if let Some(p) = f.priority { cur["priority"] = json!(match p.as_str() { "high" => "high", "low" => "low", _ => "medium" }); }
    for (k, v) in [("dueDate", f.dueDate), ("verifyDue", f.verifyDue)] {
        if let Some(d) = v { let d = clip(&d, 10); if !valid_date(&d) { return Err(bad("รูปแบบวันที่ไม่ถูกต้อง (YYYY-MM-DD)")); } cur[k] = json!(d); }
    }
    for (k, v, n) in [("containment", f.containment, 5000), ("rcaMethod", f.rcaMethod, 40), ("rootCause", f.rootCause, 5000),
                      ("actionTaken", f.actionTaken, 5000), ("riskReview", f.riskReview, 2000), ("linkedRef", f.linkedRef, 60)] {
        if let Some(x) = v { cur[k] = json!(clip(&x, n)); }
    }
    if let Some(p) = f.progress { cur["progress"] = json!(p.clamp(0, 99)); }
    Ok(())
}

async fn save(c: &impl deadpool_postgres::GenericClient, a: &Value, version: i64) -> Result<(), Response> {
    let n = c.execute(
        "UPDATE actions SET version=version+1,title=$3,detail=$4,dept=$5,doc_code=$6,source=$7,assignee_code=$8,assignee=$9,priority=$10,due_date=$11,
           containment=$12,rca_method=$13,root_cause=$14,action_taken=$15,progress=$16,verify_due=$17,risk_review=$18,linked_ref=$19,
           status=$20,effectiveness=$21,verify_note=$22,verified_by=$23,verified_code=$24,verified_at=$25,cancel_reason=$26,closed_at=$27,updated_at=$28
         WHERE id=$1 AND version=$2",
        &[&a["id"].as_i64().unwrap_or(0), &version, &s(a, "title"), &s(a, "detail"), &s(a, "dept"), &s(a, "docCode"), &s(a, "source"),
          &s(a, "assigneeCode"), &s(a, "assignee"), &s(a, "priority"), &s(a, "dueDate"), &s(a, "containment"), &s(a, "rcaMethod"),
          &s(a, "rootCause"), &s(a, "actionTaken"), &a["progress"].as_i64().unwrap_or(0), &s(a, "verifyDue"), &s(a, "riskReview"),
          &s(a, "linkedRef"), &s(a, "status"), &s(a, "effectiveness"), &s(a, "verifyNote"), &s(a, "verifiedBy"), &s(a, "verifiedCode"),
          &s(a, "verifiedAt"), &s(a, "cancelReason"), &s(a, "closedAt"), &now_iso()],
    ).await.map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    if n == 0 { return Err(err(StatusCode::CONFLICT, "ใบนี้ถูกแก้ไขโดยผู้อื่นแล้ว — โหลดใหม่ก่อนบันทึก")); }
    Ok(())
}

/// เปิดใบใหม่ (ผู้จัดทำขึ้นไป) — ผู้เปิดใบคือผู้ใช้ที่ login
pub async fn create(State(st): State<S>, Extension(u): Extension<AuthUser>, Json(f): Json<Fields>) -> Response {
    if u.role == "viewer" { return forbid("บัญชีผู้อ่านอย่างเดียวเปิดใบไม่ได้"); }
    let Ok(c) = st.db.get().await else { return db_err() };
    match open_action(&st, &c, &u, f).await {
        Ok((id, _)) => detail_resp(&st, &u, id).await,
        Err(r) => r,
    }
}

/// เปิดใบ CAR/DAR/PAR (ใช้ทั้งจากหน้า CAPA และจากข้อบกพร่องของการตรวจติดตามภายใน) — คืน (id, เลขที่ใบ)
pub(crate) async fn open_action(st: &S, c: &impl deadpool_postgres::GenericClient, u: &AuthUser, f: Fields) -> Result<(i64, String), Response> {
    let kind = f.kind.clone().unwrap_or_default().to_uppercase();
    if !KINDS.contains(&kind.as_str()) { return Err(bad("ประเภทต้องเป็น CAR/DAR/PAR")); }
    if f.title.as_deref().map(|t| t.trim().is_empty()).unwrap_or(true) { return Err(bad("ต้องระบุเรื่อง")); }
    let mut a = json!({ "kind": kind, "priority": "medium", "progress": 0, "status": "open", "effectiveness": "pending" });
    apply(c, st, &mut a, f).await?;
    let year = eform::be_year_now();
    // เลขที่ต่อเนื่องต่อประเภท/ปี — เริ่มต่อจากใบที่มีอยู่แล้ว (ระบบเดิมนับจาก COUNT) และกันเลขซ้ำเมื่อเปิดพร้อมกัน
    let existing: i64 = c.query_one("SELECT COUNT(*) FROM actions WHERE ref_no LIKE $1", &[&format!("{kind}-{year}-%")]).await.map(|r| r.get(0)).unwrap_or(0);
    let seq: i64 = c.query_one(
        "INSERT INTO form_seq(form_code,year_be,last_seq) VALUES($1,$2,$3)
         ON CONFLICT(form_code,year_be) DO UPDATE SET last_seq=GREATEST(form_seq.last_seq,$3-1)+1 RETURNING last_seq",
        &[&kind, &year, &(existing + 1)],
    ).await.map(|r| r.get(0)).map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    let ref_no = format!("{kind}-{year}-{seq:03}");
    let now = now_iso();
    let id: i64 = c.query_one(
        "INSERT INTO actions(ref_no,kind,title,detail,dept,doc_code,raised_by,raised_code,assignee,assignee_code,priority,status,progress,due_date,action_taken,
           source,containment,rca_method,root_cause,verify_due,risk_review,linked_ref,created_at,updated_at,closed_at)
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,'open',0,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$21,'') RETURNING id",
        &[&ref_no, &kind, &s(&a, "title"), &s(&a, "detail"), &s(&a, "dept"), &s(&a, "docCode"), &u.name, &u.emp_code, &s(&a, "assignee"),
          &s(&a, "assigneeCode"), &s(&a, "priority"), &s(&a, "dueDate"), &s(&a, "actionTaken"), &s(&a, "source"), &s(&a, "containment"),
          &s(&a, "rcaMethod"), &s(&a, "rootCause"), &s(&a, "verifyDue"), &s(&a, "riskReview"), &s(&a, "linkedRef"), &now],
    ).await.map(|r| r.get(0)).map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &format!("เปิดใบไม่สำเร็จ: {e}")))?;
    step(c, id, "open", u, &s(&a, "title")).await;
    let _ = eform::audit(c, u, "capa_open", "action", &ref_no, &s(&a, "title")).await;
    Ok((id, ref_no))
}

#[derive(Deserialize)]
pub struct UpdateReq { version: i64, fields: Fields }

/// แก้ไขรายละเอียด (สถานะเปิด/กำลังดำเนินการ) — ผู้เปิดใบ ผู้รับผิดชอบ หรือผู้อนุมัติขึ้นไป
pub async fn update(Path(id): Path<i64>, State(st): State<S>, Extension(u): Extension<AuthUser>, Json(b): Json<UpdateReq>) -> Response {
    let Ok(c) = st.db.get().await else { return db_err() };
    let Some(mut a) = load(&c, id).await else { return err(StatusCode::NOT_FOUND, "ไม่พบใบนี้") };
    if can_flags(&u, &a)["edit"] != json!(true) {
        return forbid(if OPEN[..2].contains(&s(&a, "status").as_str()) { "แก้ไขได้เฉพาะผู้เปิดใบ ผู้รับผิดชอบ หรือผู้อนุมัติขึ้นไป" } else { "ใบที่ส่งทวนสอบ/ปิด/ยกเลิกแล้วแก้รายละเอียดไม่ได้" });
    }
    if b.fields.kind.is_some() { return bad("เปลี่ยนประเภทใบไม่ได้"); }
    let before = a.clone();
    if let Err(r) = apply(&c, &st, &mut a, b.fields).await { return r; }
    if let Err(r) = save(&c, &a, b.version).await { return r; }
    let changed: Vec<&str> = ["title", "detail", "dept", "docCode", "source", "assigneeCode", "priority", "dueDate", "containment", "rcaMethod", "rootCause", "actionTaken", "progress", "verifyDue", "riskReview", "linkedRef"]
        .into_iter().filter(|k| before[*k] != a[*k]).collect();
    if !changed.is_empty() {
        step(&c, id, "edit", &u, &changed.join(", ")).await;
        let _ = eform::audit(&c, &u, "capa_edit", "action", &s(&a, "refNo"), &changed.join(", ")).await;
    }
    detail_resp(&st, &u, id).await
}

#[derive(Deserialize)]
pub struct ActReq { version: i64, action: String, comment: Option<String>, effective: Option<bool>, pin: Option<String> }

/// เปลี่ยนขั้น: start · submit (ส่งทวนสอบ, ลงนาม PIN) · verify (ทวนสอบ, ลงนาม PIN) · cancel
pub async fn act(Path(id): Path<i64>, State(st): State<S>, Extension(u): Extension<AuthUser>, Json(b): Json<ActReq>) -> Response {
    let Ok(c) = st.db.get().await else { return db_err() };
    let Some(mut a) = load(&c, id).await else { return err(StatusCode::NOT_FOUND, "ไม่พบใบนี้") };
    if a["version"].as_i64() != Some(b.version) { return err(StatusCode::CONFLICT, "ใบนี้ถูกแก้ไขโดยผู้อื่นแล้ว — โหลดใหม่"); }
    let can = can_flags(&u, &a);
    let comment = clip(&b.comment.unwrap_or_default(), 2000);
    let now = now_iso();
    let event;
    match b.action.as_str() {
        "start" => {
            if can["start"] != json!(true) { return forbid("เริ่มดำเนินการได้เฉพาะผู้รับผิดชอบหรือผู้อนุมัติขึ้นไป"); }
            if s(&a, "assigneeCode").is_empty() || s(&a, "dueDate").is_empty() { return bad("ต้องกำหนดผู้รับผิดชอบและวันแล้วเสร็จก่อนเริ่ม"); }
            a["status"] = json!("in_progress");
            event = "start";
        }
        "submit" => {
            if can["submit"] != json!(true) { return forbid("ส่งทวนสอบได้เฉพาะผู้รับผิดชอบหรือผู้อนุมัติขึ้นไป"); }
            let gaps = submit_gaps(&a);
            if !gaps.is_empty() { return bad(&format!("ยังขาด: {}", gaps.join(", "))); }
            if let Err(r) = eform::verify_pin(&st, &c, &u, b.pin.as_deref().unwrap_or("")).await { return r; }
            a["status"] = json!("verify");
            a["progress"] = json!(100);
            event = "submit";
        }
        "verify" => {
            if can["verify"] != json!(true) {
                return forbid(if s(&a, "assigneeCode") == u.emp_code { "ผู้รับผิดชอบทวนสอบประสิทธิผลงานของตนเองไม่ได้" } else { "ทวนสอบได้เฉพาะผู้อนุมัติขึ้นไป เมื่อใบอยู่ในสถานะรอทวนสอบ" });
            }
            if comment.is_empty() { return bad("กรุณาบันทึกหลักฐาน/ผลการทวนสอบประสิทธิผล"); }
            let Some(effective) = b.effective else { return bad("กรุณาระบุผลการทวนสอบ"); };
            if let Err(r) = eform::verify_pin(&st, &c, &u, b.pin.as_deref().unwrap_or("")).await { return r; }
            a["verifyNote"] = json!(comment);
            a["verifiedBy"] = json!(u.name);
            a["verifiedCode"] = json!(u.emp_code);
            a["verifiedAt"] = json!(now);
            if effective {
                a["effectiveness"] = json!("effective");
                a["status"] = json!("closed");
                a["closedAt"] = json!(now);
                event = "verify_effective";
            } else {
                // ไม่มีประสิทธิผล → กลับไปวิเคราะห์สาเหตุ/แก้ไขใหม่
                a["effectiveness"] = json!("not_effective");
                a["status"] = json!("in_progress");
                a["progress"] = json!(50);
                event = "verify_not_effective";
            }
        }
        "cancel" => {
            if can["cancel"] != json!(true) { return forbid("ยกเลิกได้เฉพาะผู้อนุมัติขึ้นไป ขณะใบยังไม่ปิด"); }
            if comment.is_empty() { return bad("กรุณาระบุเหตุผลการยกเลิก"); }
            a["status"] = json!("cancelled");
            a["cancelReason"] = json!(comment);
            a["closedAt"] = json!(now);
            event = "cancel";
        }
        _ => return bad("การดำเนินการไม่ถูกต้อง"),
    }
    if let Err(r) = save(&c, &a, b.version).await { return r; }
    step(&c, id, event, &u, &comment).await;
    let _ = eform::audit(&c, &u, &format!("capa_{event}"), "action", &s(&a, "refNo"), &comment).await;
    detail_resp(&st, &u, id).await
}
