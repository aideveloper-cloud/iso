// การทบทวนของฝ่ายบริหาร (Management Review, ISO 9001:2015 ข้อ 9.3) — ต่อยอดจาก management-review-dashboard
//   ข้อมูลนำเข้า 13 หัวข้อ (9.3.2) ดึงจากข้อมูลจริงในระบบ (DCR, CAR/DAR/PAR, ผลความพร้อม, การอบรม, งานจากครั้งก่อน)
//   + บันทึกเพิ่มเองพร้อมหลักฐาน · มติและงานติดตาม (9.3.3)
//   ขั้นตอน: ร่าง → รอตรวจสอบ → รออนุมัติ → ติดตามงาน → ปิด  ทุกขั้นลงนามด้วยบัญชีพนักงาน + PIN
//   แยกหน้าที่: ผู้จัดทำ ≠ ผู้ตรวจสอบ ≠ ผู้อนุมัติ · เจ้าของงานทวนสอบประสิทธิผลงานของตนเองไม่ได้ (ยกเว้น admin)
use crate::eform::{self, AuthUser};
use crate::{err, now_iso, sha256_hex, S};
use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashSet;

pub const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS mr_meetings(
  id BIGSERIAL PRIMARY KEY,
  ref_no TEXT NOT NULL,
  revision BIGINT NOT NULL DEFAULT 0,
  parent_id BIGINT NOT NULL DEFAULT 0,
  version BIGINT NOT NULL DEFAULT 1,
  status TEXT NOT NULL,
  period TEXT NOT NULL DEFAULT '',
  meet_date TEXT NOT NULL DEFAULT '',
  meet_time TEXT NOT NULL DEFAULT '',
  location TEXT NOT NULL DEFAULT '',
  chair TEXT NOT NULL DEFAULT '',
  secretary TEXT NOT NULL DEFAULT '',
  attendees TEXT NOT NULL DEFAULT '',
  next_due TEXT NOT NULL DEFAULT '',
  inputs TEXT NOT NULL,
  synced_at TEXT NOT NULL DEFAULT '',
  approved_snapshot TEXT NOT NULL DEFAULT '',
  created_by TEXT NOT NULL,
  created_name TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS mr_actions(
  id BIGSERIAL PRIMARY KEY,
  meeting_id BIGINT NOT NULL,
  version BIGINT NOT NULL DEFAULT 1,
  ref_no TEXT NOT NULL,
  title TEXT NOT NULL,
  topic TEXT NOT NULL DEFAULT '',
  owner_code TEXT NOT NULL DEFAULT '',
  owner_name TEXT NOT NULL DEFAULT '',
  dept TEXT NOT NULL DEFAULT '',
  due_date TEXT NOT NULL DEFAULT '',
  priority TEXT NOT NULL DEFAULT 'medium',
  status TEXT NOT NULL DEFAULT 'not_started',
  reason TEXT NOT NULL DEFAULT '',
  expected TEXT NOT NULL DEFAULT '',
  budget DOUBLE PRECISION NOT NULL DEFAULT 0,
  resources TEXT NOT NULL DEFAULT '',
  approval TEXT NOT NULL DEFAULT 'not_required',
  approved_by TEXT NOT NULL DEFAULT '',
  evidence TEXT NOT NULL DEFAULT '[]',
  comment TEXT NOT NULL DEFAULT '',
  effectiveness TEXT NOT NULL DEFAULT 'pending',
  verified_by TEXT NOT NULL DEFAULT '',
  verified_at TEXT NOT NULL DEFAULT '',
  completed_at TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS idx_mra_meeting ON mr_actions(meeting_id);
CREATE TABLE IF NOT EXISTS mr_signatures(
  id BIGSERIAL PRIMARY KEY,
  meeting_id BIGINT NOT NULL,
  stage TEXT NOT NULL,
  action TEXT NOT NULL,
  emp_code TEXT NOT NULL,
  name TEXT NOT NULL,
  position TEXT NOT NULL DEFAULT '',
  comment TEXT NOT NULL DEFAULT '',
  sha256 TEXT NOT NULL DEFAULT '',
  at TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS idx_mrs_meeting ON mr_signatures(meeting_id);
";

/// ข้อมูลนำเข้าตาม 9.3.2 — (key, ชื่อหัวข้อ, ข้อย่อย)
pub const TOPICS: [(&str, &str, &str); 13] = [
    ("previous", "งานติดตามจากการทบทวนครั้งก่อน", "9.3.2 a"),
    ("changes", "การเปลี่ยนแปลงประเด็นภายในและภายนอก", "9.3.2 b"),
    ("customers", "ความพึงพอใจและข้อร้องเรียนของลูกค้า", "9.3.2 c1"),
    ("objectives", "ผลการบรรลุวัตถุประสงค์คุณภาพ (KPI)", "9.3.2 c2"),
    ("processes", "สมรรถนะของกระบวนการ", "9.3.2 c3"),
    ("conformity", "ความสอดคล้องของผลิตภัณฑ์และบริการ", "9.3.2 c3"),
    ("capa", "ความไม่สอดคล้องและการแก้ไข (CAR/DAR/PAR)", "9.3.2 c4"),
    ("measurement", "ผลการติดตามและการวัด", "9.3.2 c5"),
    ("audits", "ผลการตรวจติดตาม/ตรวจประเมิน", "9.3.2 c6"),
    ("suppliers", "สมรรถนะของผู้ให้บริการภายนอก", "9.3.2 c7"),
    ("resources", "ความเพียงพอของทรัพยากร", "9.3.2 d"),
    ("risks", "ประสิทธิผลของการจัดการความเสี่ยงและโอกาส", "9.3.2 e"),
    ("improvements", "โอกาสในการปรับปรุง", "9.3.2 f"),
];
const OPEN_ACTION: [&str; 2] = ["not_started", "in_progress"];

const MCOLS: &str = "id,ref_no,revision,parent_id,version,status,period,meet_date,meet_time,location,chair,secretary,attendees,next_due,inputs,synced_at,approved_snapshot,created_by,created_name,created_at,updated_at";
const ACOLS: &str = "id,meeting_id,version,ref_no,title,topic,owner_code,owner_name,dept,due_date,priority,status,reason,expected,budget,resources,approval,approved_by,evidence,comment,effectiveness,verified_by,verified_at,completed_at,created_at,updated_at";

fn db_err() -> Response { err(StatusCode::INTERNAL_SERVER_ERROR, "เชื่อมต่อฐานข้อมูลไม่ได้") }
fn bad(m: &str) -> Response { err(StatusCode::BAD_REQUEST, m) }
fn forbid(m: &str) -> Response { err(StatusCode::FORBIDDEN, m) }
fn stale() -> Response { err(StatusCode::CONFLICT, "ข้อมูลถูกแก้ไขโดยผู้อื่นแล้ว — โหลดหน้าล่าสุดก่อนบันทึก") }

fn can_write(u: &AuthUser) -> bool { u.role != "viewer" }
fn admin(u: &AuthUser) -> bool { eform::is_admin(&u.role) }
fn s(v: &Value, k: &str) -> String { v[k].as_str().unwrap_or("").to_string() }
fn clip(t: &str, n: usize) -> String { t.trim().chars().take(n).collect() }

fn today() -> String {
    let tz = chrono::FixedOffset::east_opt(7 * 3600).unwrap();
    chrono::Utc::now().with_timezone(&tz).format("%Y-%m-%d").to_string()
}
fn valid_date(d: &str) -> bool { d.is_empty() || chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").is_ok() }

fn meeting_json(r: &tokio_postgres::Row) -> Value {
    let parse = |i: usize| serde_json::from_str::<Value>(&r.get::<_, String>(i)).unwrap_or(Value::Null);
    json!({
        "id": r.get::<_, i64>(0), "refNo": r.get::<_, String>(1), "revision": r.get::<_, i64>(2), "parentId": r.get::<_, i64>(3),
        "version": r.get::<_, i64>(4), "status": r.get::<_, String>(5), "period": r.get::<_, String>(6),
        "meetDate": r.get::<_, String>(7), "meetTime": r.get::<_, String>(8), "location": r.get::<_, String>(9),
        "chair": r.get::<_, String>(10), "secretary": r.get::<_, String>(11), "attendees": r.get::<_, String>(12),
        "nextDue": r.get::<_, String>(13), "inputs": parse(14), "syncedAt": r.get::<_, String>(15),
        "hasSnapshot": !r.get::<_, String>(16).is_empty(),
        "createdBy": r.get::<_, String>(17), "createdName": r.get::<_, String>(18),
        "createdAt": r.get::<_, String>(19), "updatedAt": r.get::<_, String>(20),
    })
}
fn action_json(r: &tokio_postgres::Row) -> Value {
    json!({
        "id": r.get::<_, i64>(0), "meetingId": r.get::<_, i64>(1), "version": r.get::<_, i64>(2), "refNo": r.get::<_, String>(3),
        "title": r.get::<_, String>(4), "topic": r.get::<_, String>(5), "ownerCode": r.get::<_, String>(6), "ownerName": r.get::<_, String>(7),
        "dept": r.get::<_, String>(8), "dueDate": r.get::<_, String>(9), "priority": r.get::<_, String>(10), "status": r.get::<_, String>(11),
        "reason": r.get::<_, String>(12), "expected": r.get::<_, String>(13), "budget": r.get::<_, f64>(14), "resources": r.get::<_, String>(15),
        "approval": r.get::<_, String>(16), "approvedBy": r.get::<_, String>(17),
        "evidence": serde_json::from_str::<Value>(&r.get::<_, String>(18)).unwrap_or(json!([])),
        "comment": r.get::<_, String>(19), "effectiveness": r.get::<_, String>(20), "verifiedBy": r.get::<_, String>(21),
        "verifiedAt": r.get::<_, String>(22), "completedAt": r.get::<_, String>(23), "createdAt": r.get::<_, String>(24), "updatedAt": r.get::<_, String>(25),
    })
}

async fn load_meeting(c: &impl deadpool_postgres::GenericClient, id: i64) -> Option<Value> {
    let r = c.query_opt(&format!("SELECT {MCOLS} FROM mr_meetings WHERE id=$1"), &[&id]).await.ok()??;
    Some(meeting_json(&r))
}
async fn load_actions(c: &impl deadpool_postgres::GenericClient, id: i64) -> Vec<Value> {
    c.query(&format!("SELECT {ACOLS} FROM mr_actions WHERE meeting_id=$1 ORDER BY id"), &[&id]).await
        .map(|rows| rows.iter().map(action_json).collect()).unwrap_or_default()
}
async fn signatures(c: &impl deadpool_postgres::GenericClient, id: i64) -> Vec<Value> {
    c.query("SELECT stage,action,emp_code,name,position,comment,sha256,at FROM mr_signatures WHERE meeting_id=$1 ORDER BY id", &[&id]).await
        .map(|rows| rows.iter().map(|r| json!({
            "stage": r.get::<_, String>(0), "action": r.get::<_, String>(1), "empCode": r.get::<_, String>(2), "name": r.get::<_, String>(3),
            "position": r.get::<_, String>(4), "comment": r.get::<_, String>(5), "sha256": r.get::<_, String>(6), "at": r.get::<_, String>(7),
        })).collect()).unwrap_or_default()
}

/// ผู้ลงนามขั้นล่าสุดของแต่ละ stage ในรอบปัจจุบัน (นับหลังการตีกลับครั้งล่าสุด)
fn current_signers(sigs: &[Value]) -> (String, String) {
    let start = sigs.iter().rposition(|x| x["action"] == json!("return")).map(|i| i + 1).unwrap_or(0);
    let cur = &sigs[start..];
    let who = |stage: &str| cur.iter().rev().find(|x| x["stage"] == json!(stage) && x["action"] != json!("return")).map(|x| s(x, "empCode")).unwrap_or_default();
    (who("prepared"), who("reviewed"))
}

// ---------- ความครบถ้วน ----------

fn input_complete(i: &Value) -> bool {
    let reviewed = i["reviewed"].as_bool().unwrap_or(false);
    let na = !s(i, "notApplicable").trim().is_empty();
    let recs = i["records"].as_array().cloned().unwrap_or_default();
    reviewed && (na || (!recs.is_empty() && recs.iter().all(|r| r["evidence"].as_array().map(|e| !e.is_empty()).unwrap_or(false))))
}

pub fn completeness(m: &Value, actions: &[Value]) -> Value {
    let inputs = m["inputs"].as_array().cloned().unwrap_or_default();
    let mut checks = vec![];
    for (key, title, _) in TOPICS {
        let i = inputs.iter().find(|x| x["key"] == json!(key)).cloned().unwrap_or(json!({}));
        let pass = input_complete(&i);
        let reason = if !i["reviewed"].as_bool().unwrap_or(false) { "ยังไม่ได้ทบทวน".to_string() }
            else if !pass { "ข้อมูลหรือหลักฐานยังไม่ครบ".to_string() }
            else if !s(&i, "notApplicable").trim().is_empty() { format!("ไม่เกี่ยวข้อง: {}", s(&i, "notApplicable")) }
            else { "ทบทวนแล้ว · มีหลักฐานอ้างอิง".to_string() };
        checks.push(json!({ "key": key, "title": title, "pass": pass, "reason": reason }));
    }
    let live: Vec<&Value> = actions.iter().filter(|a| a["status"] != json!("cancelled")).collect();
    checks.push(json!({ "key": "decisions", "title": "มติของฝ่ายบริหาร", "pass": inputs.iter().any(|i| !s(i, "decision").trim().is_empty()), "reason": "บันทึกมติอย่างน้อยหนึ่งหัวข้อ" }));
    checks.push(json!({ "key": "actions", "title": "งานติดตามจากมติ", "pass": !live.is_empty(), "reason": "บันทึกงานติดตามอย่างน้อยหนึ่งงาน" }));
    checks.push(json!({ "key": "owners", "title": "ผู้รับผิดชอบงาน", "pass": !live.is_empty() && live.iter().all(|a| !s(a, "ownerCode").is_empty()), "reason": "ระบุผู้รับผิดชอบ (พนักงานในระบบ) ทุกงาน" }));
    checks.push(json!({ "key": "dueDates", "title": "กำหนดเสร็จ", "pass": !live.is_empty() && live.iter().all(|a| !s(a, "dueDate").is_empty()), "reason": "ระบุกำหนดเสร็จทุกงาน" }));
    let n = checks.len();
    let passed = checks.iter().filter(|c| c["pass"] == json!(true)).count();
    let reviewed = checks[..13].iter().filter(|c| c["pass"] == json!(true)).count();
    json!({ "checks": checks, "percent": ((passed as f64) / (n as f64) * 100.0).round() as i64, "reviewed": reviewed, "passed": passed, "total": n })
}

/// ประเด็นที่ควรหยิบยกในที่ประชุม — KPI ไม่ถึงเป้า, สถานะวิกฤต, ไม่มีหลักฐาน, งานเลยกำหนด
fn insights(m: &Value, actions: &[Value]) -> Vec<Value> {
    let mut out = vec![];
    let now = today();
    for i in m["inputs"].as_array().cloned().unwrap_or_default() {
        let key = s(&i, "key");
        let recs = i["records"].as_array().cloned().unwrap_or_default();
        if recs.is_empty() && s(&i, "notApplicable").trim().is_empty() {
            out.push(json!({ "topic": key, "level": "unreviewed", "text": "ยังไม่มีข้อมูลหรือหลักฐานเพียงพอ", "sources": [] }));
        }
        for r in recs {
            let mut notes = vec![];
            let mut level = s(&r, "status");
            if r["evidence"].as_array().map(|e| e.is_empty()).unwrap_or(true) { notes.push("ไม่มีหลักฐานอ้างอิง".to_string()); level = "unreviewed".into(); }
            let mt = &r["metric"];
            if let (Some(a), Some(t)) = (mt["actual"].as_f64(), mt["target"].as_f64()) {
                let lte = mt["direction"] == json!("lte");
                let unit = s(mt, "unit");
                if (lte && a > t) || (!lte && a < t) { notes.push(format!("{a}{unit} ไม่ถึงเป้า {}{t}{unit}", if lte { "≤" } else { "≥" })); if level == "good" { level = "attention".into(); } }
                if let Some(p) = mt["previous"].as_f64() { if (lte && a > p) || (!lte && a < p) { notes.push("แนวโน้มแย่ลงจากรอบก่อน".into()); } }
            }
            if level == "critical" || level == "attention" || !notes.is_empty() {
                out.push(json!({ "topic": key, "level": level, "text": format!("{}{}", s(&r, "title"), if notes.is_empty() { String::new() } else { format!(" — {}", notes.join(" · ")) }), "sources": [s(&r, "id")] }));
            }
        }
    }
    for a in actions {
        if OPEN_ACTION.contains(&a["status"].as_str().unwrap_or("")) && !s(a, "dueDate").is_empty() && s(a, "dueDate") < now {
            out.push(json!({ "topic": "actions", "level": "critical", "text": format!("{} {} — เลยกำหนด {}", s(a, "refNo"), s(a, "title"), s(a, "dueDate")), "sources": [s(a, "refNo")] }));
        }
    }
    let rank = |l: &str| match l { "critical" => 0, "attention" => 1, "unreviewed" => 2, _ => 3 };
    out.sort_by_key(|x| rank(x["level"].as_str().unwrap_or("")));
    out
}

// ---------- สิทธิ์ ----------

fn can_flags(u: &AuthUser, m: &Value, sigs: &[Value], actions: &[Value]) -> Value {
    let status = m["status"].as_str().unwrap_or("");
    let (preparer, reviewer) = current_signers(sigs);
    let me = u.emp_code.as_str();
    let pct = completeness(m, actions)["percent"].as_i64().unwrap_or(0);
    let open = actions.iter().filter(|a| OPEN_ACTION.contains(&a["status"].as_str().unwrap_or(""))).count();
    let unverified = actions.iter().filter(|a| a["status"] == json!("completed") && a["effectiveness"] != json!("effective")).count();
    json!({
        "edit": status == "draft" && can_write(u),
        "submit": status == "draft" && can_write(u) && pct == 100,
        "review": status == "review" && eform::can_review(&u.role) && (admin(u) || preparer != me),
        "approve": status == "approve" && eform::can_approve(&u.role) && (admin(u) || (preparer != me && reviewer != me)),
        "close": status == "monitoring" && eform::can_approve(&u.role) && open == 0 && unverified == 0,
        "revise": matches!(status, "monitoring" | "closed") && can_write(u),
        "verify": eform::can_review(&u.role),
        "approveBudget": eform::can_approve(&u.role),
        "percent": pct, "openActions": open, "unverified": unverified,
        "preparer": preparer, "reviewer": reviewer,
    })
}

async fn detail_resp(st: &S, u: &AuthUser, id: i64) -> Response {
    let Ok(c) = st.db.get().await else { return db_err() };
    let Some(mut m) = load_meeting(&c, id).await else { return err(StatusCode::NOT_FOUND, "ไม่พบการประชุม") };
    let actions = load_actions(&c, id).await;
    let sigs = signatures(&c, id).await;
    m["can"] = can_flags(u, &m, &sigs, &actions);
    m["completeness"] = completeness(&m, &actions);
    m["insights"] = json!(insights(&m, &actions));
    m["actions"] = json!(actions);
    m["signatures"] = json!(sigs);
    m["topics"] = json!(TOPICS.iter().map(|(k, t, c)| json!({ "key": k, "title": t, "clause": c })).collect::<Vec<_>>());
    if let Ok(rows) = c.query("SELECT id,ref_no,revision,status FROM mr_meetings WHERE ref_no=$1 ORDER BY revision", &[&s(&m, "refNo")]).await {
        m["revisions"] = json!(rows.iter().map(|r| json!({ "id": r.get::<_, i64>(0), "refNo": r.get::<_, String>(1), "revision": r.get::<_, i64>(2), "status": r.get::<_, String>(3) })).collect::<Vec<_>>());
    }
    Json(m).into_response()
}

// ---------- ข้อมูลจากระบบ (sync) ----------

fn rec(id: String, title: String, owner: String, status: &str, note: String, evidence: Vec<Value>) -> Value {
    json!({ "id": id, "title": title, "owner": owner, "status": status, "note": note, "evidence": evidence, "source": "system" })
}
fn ev(code: &str, title: &str) -> Value { json!({ "code": code, "title": title }) }

/// สร้างระเบียนข้อมูลนำเข้าจากข้อมูลจริง — คีย์คือ topic
async fn system_records(st: &S, c: &impl deadpool_postgres::GenericClient, meeting_id: i64, parent_id: i64) -> Vec<(String, Vec<Value>)> {
    let now = today();
    let since = (chrono::NaiveDate::parse_from_str(&now, "%Y-%m-%d").unwrap() - chrono::Duration::days(365)).format("%Y-%m-%d").to_string();
    let mut out: Vec<(String, Vec<Value>)> = vec![];

    // a) งานจากการทบทวนครั้งก่อน: การประชุมที่อนุมัติแล้วล่าสุด (ไม่ใช่ฉบับนี้/ฉบับต้นทางของ revision)
    let prev = c.query_opt(
        "SELECT id,ref_no FROM mr_meetings WHERE id<>$1 AND id<>$2 AND ref_no<>(SELECT ref_no FROM mr_meetings WHERE id=$1) AND status IN ('monitoring','closed','superseded') ORDER BY meet_date DESC, id DESC LIMIT 1",
        &[&meeting_id, &parent_id],
    ).await.ok().flatten();
    let mut previous = vec![];
    if let Some(p) = prev {
        // งานของการประชุมครั้งก่อนทั้งชุด (ทุกฉบับแก้ไขของเลขที่เดียวกัน — งานที่เปิดอยู่ถูกย้ายไปฉบับล่าสุด)
        let ref_prev: String = p.get(1);
        let ids: Vec<i64> = c.query("SELECT id FROM mr_meetings WHERE ref_no=$1", &[&ref_prev]).await.map(|r| r.iter().map(|x| x.get(0)).collect()).unwrap_or_default();
        let mut all = vec![];
        for pid in ids { all.extend(load_actions(c, pid).await); }
        for a in all {
            let st_ = s(&a, "status");
            let late = OPEN_ACTION.contains(&st_.as_str()) && !s(&a, "dueDate").is_empty() && s(&a, "dueDate") < now;
            let status = if late { "critical" } else if st_ == "completed" && a["effectiveness"] == json!("effective") { "good" } else if st_ == "cancelled" { "good" } else { "attention" };
            let mut evs = vec![ev(&s(&a, "refNo"), &format!("งาน {} ({})", s(&a, "refNo"), p.get::<_, String>(1)))];
            evs.extend(a["evidence"].as_array().cloned().unwrap_or_default());
            previous.push(rec(format!("SYS-{}", s(&a, "refNo")), s(&a, "title"), s(&a, "ownerName"), status,
                format!("สถานะ {} · กำหนด {} · ประสิทธิผล {}", st_, s(&a, "dueDate"), s(&a, "effectiveness")), evs));
        }
    }
    out.push(("previous".into(), previous));

    // b) การเปลี่ยนแปลง: DCR ในรอบ 12 เดือน + เอกสารที่ยกเลิกใช้
    let mut changes = vec![];
    if let Ok(rows) = c.query("SELECT ref_no,doc_code,status,from_rev,new_rev,reason,updated_at FROM doc_changes WHERE status<>'cancelled' AND updated_at>=$1 ORDER BY id", &[&since]).await {
        for r in rows {
            let (ref_no, code, status): (String, String, String) = (r.get(0), r.get(1), r.get(2));
            let name = st.by_code.get(&code).and_then(|d| d["name"].as_str()).unwrap_or("").to_string();
            changes.push(rec(format!("SYS-{ref_no}"), format!("{ref_no} {code} {name}: {} → {}", r.get::<_, String>(3), r.get::<_, String>(4)), String::new(),
                if status == "effective" { "good" } else { "attention" }, format!("เหตุผล: {} · สถานะ {status}", r.get::<_, String>(5)),
                vec![ev(&ref_no, "คำขอเปลี่ยนแปลงเอกสาร"), ev(&code, &name)]));
        }
    }
    if let Ok(rows) = c.query("SELECT doc_code,reason,at FROM doc_status WHERE status='obsolete'", &[]).await {
        for r in rows {
            let code: String = r.get(0);
            changes.push(rec(format!("SYS-OBS-{code}"), format!("ยกเลิกใช้ {code}"), String::new(), "good", format!("เหตุผล: {}", r.get::<_, String>(1)), vec![ev(&code, "เอกสารยกเลิกใช้")]));
        }
    }
    out.push(("changes".into(), changes));

    // c4) CAR/DAR/PAR ที่ยังเปิด หรือปิดในรอบ 12 เดือน
    let mut capa = vec![];
    if let Ok(rows) = c.query("SELECT ref_no,kind,title,status,due_date,assignee,doc_code,action_taken,closed_at,root_cause,effectiveness FROM actions WHERE status<>'cancelled' ORDER BY id", &[]).await {
        for r in rows {
            let (ref_no, status, due, closed): (String, String, String, String) = (r.get(0), r.get(3), r.get(4), r.get(8));
            if status == "closed" && closed.get(..10).unwrap_or("") < since.as_str() { continue; }
            let late = status != "closed" && !due.is_empty() && due < now;
            let mut evs = vec![ev(&ref_no, &format!("ใบ {}", r.get::<_, String>(1)))];
            let doc: String = r.get(6);
            if !doc.is_empty() { evs.push(ev(&doc, "เอกสารอ้างอิง")); }
            capa.push(rec(format!("SYS-{ref_no}"), format!("{ref_no} {}", r.get::<_, String>(2)), r.get(5),
                if late { "critical" } else if status == "closed" { "good" } else { "attention" },
                format!("สถานะ {status} · กำหนด {due}{}{}", if r.get::<_, String>(9).is_empty() { String::new() } else { format!(" · สาเหตุ: {}", r.get::<_, String>(9)) },
                    if status == "closed" { format!(" · การดำเนินการ: {} · ประสิทธิผล: {}", r.get::<_, String>(7), r.get::<_, String>(10)) } else { String::new() }), evs));
        }
    }
    out.push(("capa".into(), capa));

    // c6) ผลคัดกรองความพร้อมรับการตรวจ (ข้อมูลจริง) — ภาพรวม + ประเด็นที่ยังไม่ผ่าน
    let rd = crate::readiness::evaluate(st).await;
    let mut audits = vec![rec("SYS-READINESS".into(), format!("ผลคัดกรองความพร้อมรับการตรวจ {}%", rd["score"]), String::new(),
        if rd["critical"].as_i64().unwrap_or(0) > 0 { "critical" } else if rd["warnings"].as_i64().unwrap_or(0) > 0 { "attention" } else { "good" },
        format!("ผ่าน {} · ต้องติดตาม {} · วิกฤต {} จาก {} รายการ", rd["passed"], rd["warnings"], rd["critical"], rd["total"]),
        vec![ev("READINESS", "เตรียม Audit → ความพร้อมรับการตรวจ")])];
    for ctl in rd["controls"].as_array().cloned().unwrap_or_default() {
        if ctl["status"] == json!("pass") || ctl["category"] == json!("ข้อกำหนด") { continue; }
        let codes: Vec<Value> = ctl["evidence"].as_array().cloned().unwrap_or_default().iter().take(5).filter_map(|x| x.as_str().map(|c| ev(c, ""))).collect();
        let evs = if codes.is_empty() { vec![ev("READINESS", &s(&ctl, "id"))] } else { codes };
        audits.push(rec(format!("SYS-RD-{}", s(&ctl, "id")), s(&ctl, "title"), String::new(), if ctl["status"] == json!("critical") { "critical" } else { "attention" }, s(&ctl, "detail"), evs));
    }
    // ผลการตรวจติดตามภายในที่ลงนามรายงานแล้วในรอบ 12 เดือน
    let ia = crate::internalaudit::facts(c).await;
    for d in ia["done"].as_array().cloned().unwrap_or_default() {
        let k = &d["counts"];
        let (major, minor, ofi) = (k["major"].as_i64().unwrap_or(0), k["minor"].as_i64().unwrap_or(0), k["ofi"].as_i64().unwrap_or(0));
        let open = d["openCars"].as_i64().unwrap_or(0);
        audits.push(rec(format!("SYS-{}", s(&d, "refNo")), format!("ตรวจติดตามภายใน {} {}", s(&d, "refNo"), s(&d, "title")), String::new(),
            if major > 0 && open > 0 { "critical" } else if open > 0 || minor > 0 { "attention" } else { "good" },
            format!("NC สำคัญ {major} · NC เล็กน้อย {minor} · ข้อสังเกต {ofi} · CAR ที่ยังเปิด {open} · สถานะ {}", s(&d, "status")),
            vec![ev(&s(&d, "refNo"), "รายงานการตรวจติดตามภายใน")]));
    }
    out.push(("audits".into(), audits));

    // d) ทรัพยากร: การอบรมที่อนุมัติแล้วในรอบ 12 เดือน (จาก FM-HR-008)
    let mut resources = vec![];
    if let Ok(rows) = c.query(
        "SELECT course, COUNT(*), MIN(ref_no), MAX(train_date) FROM training_log WHERE train_date='' OR train_date>=$1 GROUP BY course ORDER BY MAX(train_date) DESC LIMIT 50",
        &[&since],
    ).await {
        for r in rows {
            let course: String = r.get(0);
            let n: i64 = r.get(1);
            let ref_no: String = r.get(2);
            resources.push(rec(format!("SYS-TR-{}", sha256_hex(course.as_bytes()).get(..10).unwrap_or("")), format!("อบรม: {course}"), String::new(), "good",
                format!("ผู้เข้าอบรม {n} คน · ล่าสุด {}", r.get::<_, String>(3)), vec![ev(&ref_no, "ใบรายชื่อผู้เข้าอบรม (FM-HR-008)")]));
        }
    }
    out.push(("resources".into(), resources));
    out
}

/// รวมข้อมูลจากระบบเข้ากับข้อมูลนำเข้าเดิม: เก็บระเบียนที่บันทึกเอง; รีเซ็ต "ทบทวนแล้ว" เฉพาะหัวข้อที่ข้อมูลระบบเปลี่ยน
fn merge_system(inputs: &mut Vec<Value>, sys: Vec<(String, Vec<Value>)>) {
    for (key, records) in sys {
        let Some(i) = inputs.iter_mut().find(|x| x["key"] == json!(key)) else { continue };
        let old: Vec<Value> = i["records"].as_array().cloned().unwrap_or_default();
        let old_sys: Vec<&Value> = old.iter().filter(|r| r["source"] == json!("system")).collect();
        let canon = |v: Vec<&Value>| { let mut x: Vec<String> = v.iter().map(|r| r.to_string()).collect(); x.sort(); x };
        let changed = canon(old_sys) != canon(records.iter().collect());
        let mut merged = records;
        merged.extend(old.into_iter().filter(|r| r["source"] != json!("system")));
        i["records"] = json!(merged);
        if changed { i["reviewed"] = json!(false); i["reviewedBy"] = json!(""); }
    }
}

fn empty_inputs() -> Vec<Value> {
    TOPICS.iter().map(|(k, _, _)| json!({ "key": k, "records": [], "reviewed": false, "reviewedBy": "", "reviewedAt": "", "discussion": "", "decision": "", "notApplicable": "" })).collect()
}

// ---------- handlers ----------

/// รายการประชุม + งานที่ฉันรับผิดชอบ
pub async fn list(State(st): State<S>, Extension(u): Extension<AuthUser>) -> Response {
    let Ok(c) = st.db.get().await else { return db_err() };
    let rows = c.query(&format!("SELECT {MCOLS} FROM mr_meetings ORDER BY meet_date DESC, id DESC LIMIT 200"), &[]).await.unwrap_or_default();
    let mut meetings = vec![];
    for r in &rows {
        let m = meeting_json(r);
        let id = m["id"].as_i64().unwrap_or(0);
        let actions = load_actions(&c, id).await;
        let comp = completeness(&m, &actions);
        let open = actions.iter().filter(|a| OPEN_ACTION.contains(&a["status"].as_str().unwrap_or(""))).count();
        let late = actions.iter().filter(|a| OPEN_ACTION.contains(&a["status"].as_str().unwrap_or("")) && !s(a, "dueDate").is_empty() && s(a, "dueDate") < today()).count();
        meetings.push(json!({
            "id": id, "refNo": m["refNo"], "revision": m["revision"], "status": m["status"], "period": m["period"], "meetDate": m["meetDate"],
            "chair": m["chair"], "percent": comp["percent"], "actions": actions.len(), "openActions": open, "lateActions": late, "updatedAt": m["updatedAt"],
        }));
    }
    let mine = c.query(
        &format!("SELECT {ACOLS} FROM mr_actions WHERE owner_code=$1 AND status IN ('not_started','in_progress') AND meeting_id IN (SELECT id FROM mr_meetings WHERE status='monitoring') ORDER BY due_date"),
        &[&u.emp_code],
    ).await.map(|r| r.iter().map(action_json).collect::<Vec<_>>()).unwrap_or_default();
    Json(json!({ "meetings": meetings, "myActions": mine, "canCreate": can_write(&u) })).into_response()
}

pub async fn detail(Path(id): Path<i64>, State(st): State<S>, Extension(u): Extension<AuthUser>) -> Response {
    detail_resp(&st, &u, id).await
}

#[derive(Deserialize)]
#[allow(non_snake_case)]
pub struct MeetingFields {
    period: Option<String>, meetDate: Option<String>, meetTime: Option<String>, location: Option<String>,
    chair: Option<String>, secretary: Option<String>, attendees: Option<String>, nextDue: Option<String>,
}

fn field_values(cur: &Value, f: MeetingFields) -> Result<[String; 8], Response> {
    let pick = |v: Option<String>, k: &str, n: usize| clip(&v.unwrap_or_else(|| s(cur, k)), n);
    let vals = [
        pick(f.period, "period", 60), pick(f.meetDate, "meetDate", 10), pick(f.meetTime, "meetTime", 20), pick(f.location, "location", 120),
        pick(f.chair, "chair", 120), pick(f.secretary, "secretary", 120), pick(f.attendees, "attendees", 2000), pick(f.nextDue, "nextDue", 10),
    ];
    if !valid_date(&vals[1]) || !valid_date(&vals[7]) { return Err(bad("รูปแบบวันที่ไม่ถูกต้อง (YYYY-MM-DD)")); }
    Ok(vals)
}

/// สร้างการประชุมใหม่ (ผู้จัดทำขึ้นไป) — ดึงข้อมูลนำเข้าจากระบบทันที
pub async fn create(State(st): State<S>, Extension(u): Extension<AuthUser>, Json(b): Json<MeetingFields>) -> Response {
    if !can_write(&u) { return forbid("บัญชีผู้อ่านอย่างเดียวสร้างการประชุมไม่ได้"); }
    let vals = match field_values(&json!({}), b) { Ok(v) => v, Err(r) => return r };
    if vals[1].is_empty() { return bad("กรุณาระบุวันที่ประชุม"); }
    let Ok(c) = st.db.get().await else { return db_err() };
    // เลขที่ใช้ปี พ.ศ. ของวันที่ประชุม ไม่ใช่วันที่สร้าง
    let year: i64 = vals[1][..4].parse::<i64>().unwrap_or(2026) + 543;
    let seq: i64 = match c.query_one(
        "INSERT INTO form_seq(form_code,year_be,last_seq) VALUES('MR',$1,1) ON CONFLICT(form_code,year_be) DO UPDATE SET last_seq=form_seq.last_seq+1 RETURNING last_seq", &[&year],
    ).await { Ok(r) => r.get(0), Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()) };
    let ref_no = format!("MR-{year}-{seq:03}");
    let now = now_iso();
    let row = c.query_one(
        "INSERT INTO mr_meetings(ref_no,revision,parent_id,version,status,period,meet_date,meet_time,location,chair,secretary,attendees,next_due,inputs,created_by,created_name,created_at,updated_at)
         VALUES($1,0,0,1,'draft',$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$13) RETURNING id",
        &[&ref_no, &vals[0], &vals[1], &vals[2], &vals[3], &vals[4], &vals[5], &vals[6], &vals[7], &json!(empty_inputs()).to_string(), &u.emp_code, &u.name, &now],
    ).await;
    let id: i64 = match row { Ok(r) => r.get(0), Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()) };
    let mut inputs = empty_inputs();
    merge_system(&mut inputs, system_records(&st, &c, id, 0).await);
    let _ = c.execute("UPDATE mr_meetings SET inputs=$2,synced_at=$3 WHERE id=$1", &[&id, &json!(inputs).to_string(), &now]).await;
    let _ = eform::audit(&c, &u, "mr_create", "mr_meeting", &id.to_string(), &ref_no).await;
    detail_resp(&st, &u, id).await
}

#[derive(Deserialize)]
pub struct VersionOnly { version: i64 }

/// ใช้ร่วม: โหลดการประชุมสถานะร่าง + ตรวจสิทธิ์/version
async fn draft_for_edit(c: &impl deadpool_postgres::GenericClient, u: &AuthUser, id: i64, version: i64) -> Result<Value, Response> {
    if !can_write(u) { return Err(forbid("บัญชีผู้อ่านอย่างเดียวแก้ไขไม่ได้")); }
    let Some(m) = load_meeting(c, id).await else { return Err(err(StatusCode::NOT_FOUND, "ไม่พบการประชุม")) };
    if m["status"] != json!("draft") { return Err(bad("แก้ไขได้เฉพาะสถานะร่าง — ส่งกลับเป็นร่างก่อน หรือสร้างฉบับแก้ไข (revision)")); }
    if m["version"].as_i64() != Some(version) { return Err(stale()); }
    Ok(m)
}

async fn bump(c: &impl deadpool_postgres::GenericClient, id: i64, version: i64, set: &str, params: &[&(dyn tokio_postgres::types::ToSql + Sync)]) -> Result<(), Response> {
    let mut all: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = vec![&id, &version];
    all.extend_from_slice(params);
    let sql = format!("UPDATE mr_meetings SET {set}, version=version+1, updated_at='{}' WHERE id=$1 AND version=$2", now_iso());
    match c.execute(&sql, &all).await {
        Ok(1) => Ok(()),
        Ok(_) => Err(stale()),
        Err(e) => Err(err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string())),
    }
}

#[derive(Deserialize)]
pub struct FieldsReq { version: i64, fields: MeetingFields }

pub async fn update(Path(id): Path<i64>, State(st): State<S>, Extension(u): Extension<AuthUser>, Json(b): Json<FieldsReq>) -> Response {
    let Ok(c) = st.db.get().await else { return db_err() };
    let m = match draft_for_edit(&c, &u, id, b.version).await { Ok(m) => m, Err(r) => return r };
    let v = match field_values(&m, b.fields) { Ok(v) => v, Err(r) => return r };
    if v[1].is_empty() { return bad("กรุณาระบุวันที่ประชุม"); }
    if let Err(r) = bump(&c, id, b.version, "period=$3,meet_date=$4,meet_time=$5,location=$6,chair=$7,secretary=$8,attendees=$9,next_due=$10",
        &[&v[0], &v[1], &v[2], &v[3], &v[4], &v[5], &v[6], &v[7]]).await { return r; }
    detail_resp(&st, &u, id).await
}

#[derive(Deserialize)]
pub struct InputReq { version: i64, input: Value }

/// บันทึกหนึ่งหัวข้อ: ข้ออภิปราย มติ เหตุผลไม่เกี่ยวข้อง ระเบียนที่บันทึกเอง และการทำเครื่องหมายทบทวนแล้ว
pub async fn save_input(Path(id): Path<i64>, State(st): State<S>, Extension(u): Extension<AuthUser>, Json(b): Json<InputReq>) -> Response {
    let Ok(c) = st.db.get().await else { return db_err() };
    let m = match draft_for_edit(&c, &u, id, b.version).await { Ok(m) => m, Err(r) => return r };
    let key = s(&b.input, "key");
    if !TOPICS.iter().any(|(k, _, _)| *k == key) { return bad("หัวข้อไม่ถูกต้อง"); }
    let mut inputs = m["inputs"].as_array().cloned().unwrap_or_else(empty_inputs);
    let Some(cur) = inputs.iter_mut().find(|x| x["key"] == json!(key)) else { return bad("หัวข้อไม่ถูกต้อง") };

    // ระเบียนจากระบบแก้ไม่ได้ ใช้ของเดิม; ระเบียนที่บันทึกเองต้องมีชื่อเรื่อง และหลักฐานต้องเป็นรหัส/ชื่อ หรือ http(s)
    let sys: Vec<Value> = cur["records"].as_array().cloned().unwrap_or_default().into_iter().filter(|r| r["source"] == json!("system")).collect();
    let old_manual: Vec<Value> = cur["records"].as_array().cloned().unwrap_or_default().into_iter().filter(|r| r["source"] != json!("system")).collect();
    let mut manual = vec![];
    let mut ids: HashSet<String> = sys.iter().map(|r| s(r, "id")).collect();
    for r in b.input["records"].as_array().cloned().unwrap_or_default() {
        if r["source"] == json!("system") { continue; }
        let title = clip(&s(&r, "title"), 300);
        if title.is_empty() { return bad("ระเบียนที่บันทึกเองต้องมีชื่อเรื่อง"); }
        let mut rid = clip(&s(&r, "id"), 40);
        if rid.is_empty() || rid.starts_with("SYS-") || ids.contains(&rid) { rid = format!("M-{}", &sha256_hex(format!("{title}{}{}", ids.len(), now_iso()).as_bytes())[..10]); }
        ids.insert(rid.clone());
        let status = match s(&r, "status").as_str() { "good" | "attention" | "critical" => s(&r, "status"), _ => "attention".into() };
        let mut evidence = vec![];
        for e in r["evidence"].as_array().cloned().unwrap_or_default().into_iter().take(20) {
            let url = clip(&s(&e, "url"), 500);
            if !url.is_empty() && !(url.starts_with("https://") || url.starts_with("http://")) { return bad("ลิงก์หลักฐานต้องขึ้นต้นด้วย http:// หรือ https://"); }
            let code = clip(&s(&e, "code"), 60);
            let et = clip(&s(&e, "title"), 200);
            if code.is_empty() && et.is_empty() && url.is_empty() { continue; }
            let et = if et.is_empty() { st.by_code.get(&code).and_then(|d| d["name"].as_str()).unwrap_or("").to_string() } else { et };
            evidence.push(json!({ "code": code, "title": et, "url": url }));
        }
        // คงผู้บันทึกเดิมไว้ ถ้าเป็นระเบียนเดิมที่ไม่ได้เปลี่ยน
        let author = old_manual.iter().find(|o| s(o, "id") == rid).map(|o| s(o, "author")).filter(|a| !a.is_empty()).unwrap_or_else(|| u.name.clone());
        let metric = &r["metric"];
        let num = |k: &str| metric[k].as_f64().map(Value::from).unwrap_or(Value::Null);
        manual.push(json!({
            "id": rid, "title": title, "owner": clip(&s(&r, "owner"), 120), "status": status, "note": clip(&s(&r, "note"), 2000),
            "evidence": evidence, "source": "manual", "author": author,
            "metric": { "actual": num("actual"), "target": num("target"), "previous": num("previous"), "unit": clip(&s(metric, "unit"), 12),
                        "direction": if metric["direction"] == json!("lte") { "lte" } else { "gte" } },
        }));
    }
    let mut records = sys;
    records.extend(manual);
    cur["records"] = json!(records);
    cur["discussion"] = json!(clip(&s(&b.input, "discussion"), 5000));
    cur["decision"] = json!(clip(&s(&b.input, "decision"), 5000));
    cur["notApplicable"] = json!(clip(&s(&b.input, "notApplicable"), 1000));
    let reviewed = b.input["reviewed"].as_bool().unwrap_or(false);
    if reviewed && !cur["reviewed"].as_bool().unwrap_or(false) { cur["reviewedBy"] = json!(u.name); cur["reviewedAt"] = json!(now_iso()); }
    if !reviewed { cur["reviewedBy"] = json!(""); cur["reviewedAt"] = json!(""); }
    cur["reviewed"] = json!(reviewed);
    let text = json!(inputs).to_string();
    if let Err(r) = bump(&c, id, b.version, "inputs=$3", &[&text]).await { return r; }
    detail_resp(&st, &u, id).await
}

/// ดึงข้อมูลนำเข้าจากระบบล่าสุด (สถานะร่าง)
pub async fn sync(Path(id): Path<i64>, State(st): State<S>, Extension(u): Extension<AuthUser>, Json(b): Json<VersionOnly>) -> Response {
    let Ok(c) = st.db.get().await else { return db_err() };
    let m = match draft_for_edit(&c, &u, id, b.version).await { Ok(m) => m, Err(r) => return r };
    let mut inputs = m["inputs"].as_array().cloned().unwrap_or_else(empty_inputs);
    merge_system(&mut inputs, system_records(&st, &c, id, m["parentId"].as_i64().unwrap_or(0)).await);
    let text = json!(inputs).to_string();
    let now = now_iso();
    if let Err(r) = bump(&c, id, b.version, "inputs=$3,synced_at=$4", &[&text, &now]).await { return r; }
    detail_resp(&st, &u, id).await
}

#[derive(Deserialize)]
pub struct ActReq { version: i64, action: String, comment: Option<String>, pin: Option<String> }

/// เปลี่ยนขั้นตอน: submit (ผู้จัดทำ) · pass/return (ผู้ตรวจสอบ) · approve/return (ผู้อนุมัติ) · close (ผู้อนุมัติ)
pub async fn act(Path(id): Path<i64>, State(st): State<S>, Extension(u): Extension<AuthUser>, Json(b): Json<ActReq>) -> Response {
    let Ok(c) = st.db.get().await else { return db_err() };
    let Some(m) = load_meeting(&c, id).await else { return err(StatusCode::NOT_FOUND, "ไม่พบการประชุม") };
    if m["version"].as_i64() != Some(b.version) { return stale(); }
    let actions = load_actions(&c, id).await;
    let sigs = signatures(&c, id).await;
    let can = can_flags(&u, &m, &sigs, &actions);
    let status = s(&m, "status");
    let comment = clip(&b.comment.unwrap_or_default(), 2000);
    let (stage, next) = match (status.as_str(), b.action.as_str()) {
        ("draft", "submit") if can["submit"] == json!(true) => ("prepared", "review"),
        ("draft", "submit") => return bad(if can_write(&u) { "ยังทบทวนข้อมูลนำเข้า มติ หรืองานติดตามไม่ครบ 100%" } else { "บัญชีผู้อ่านอย่างเดียวส่งไม่ได้" }),
        ("review", "pass") if can["review"] == json!(true) => ("reviewed", "approve"),
        ("review", "return") if can["review"] == json!(true) => ("reviewed", "draft"),
        ("approve", "approve") if can["approve"] == json!(true) => ("approved", "monitoring"),
        ("approve", "return") if can["approve"] == json!(true) => ("approved", "draft"),
        ("monitoring", "close") if can["close"] == json!(true) => ("closed", "closed"),
        ("monitoring", "close") => return bad("ปิดได้เมื่อทุกงานเสร็จและทวนสอบประสิทธิผลแล้ว (หรือยกเลิกพร้อมเหตุผล) โดยผู้อนุมัติ"),
        ("review" | "approve", _) => return forbid("ไม่มีสิทธิ์ในขั้นนี้ — ผู้จัดทำ ผู้ตรวจสอบ และผู้อนุมัติต้องเป็นคนละคนกัน"),
        _ => return bad("เปลี่ยนสถานะตามลำดับนี้ไม่ได้"),
    };
    if b.action == "return" && comment.is_empty() { return bad("กรุณาระบุเหตุผลที่ตีกลับ"); }
    if let Err(r) = eform::verify_pin(&st, &c, &u, b.pin.as_deref().unwrap_or("")).await { return r; }

    // ลายเซ็นผูกกับเนื้อหา ณ ขณะลงนาม (ข้อมูลนำเข้า + งานติดตาม)
    let snapshot = json!({ "meeting": { "refNo": m["refNo"], "revision": m["revision"], "period": m["period"], "meetDate": m["meetDate"], "chair": m["chair"], "attendees": m["attendees"] },
                           "inputs": m["inputs"], "actions": actions });
    let snap_text = snapshot.to_string();
    let sha = sha256_hex(snap_text.as_bytes());
    let res = if next == "monitoring" {
        bump(&c, id, b.version, "status=$3,approved_snapshot=$4", &[&next, &snap_text]).await
    } else {
        bump(&c, id, b.version, "status=$3", &[&next]).await
    };
    if let Err(r) = res { return r; }
    let action = if b.action == "return" { "return" } else { b.action.as_str() };
    let _ = c.execute(
        "INSERT INTO mr_signatures(meeting_id,stage,action,emp_code,name,position,comment,sha256,at) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)",
        &[&id, &stage, &action, &u.emp_code, &u.name, &u.position, &comment, &sha, &now_iso()],
    ).await;
    let _ = eform::audit(&c, &u, &format!("mr_{action}"), "mr_meeting", &id.to_string(), &format!("{} {comment}", s(&m, "refNo"))).await;
    detail_resp(&st, &u, id).await
}

#[derive(Deserialize)]
pub struct ReviseReq { version: i64, reason: Option<String> }

/// ฉบับแก้ไข: สร้างร่างใหม่ (revision+1) จากฉบับที่อนุมัติล่าสุด — ฉบับเดิมที่ยังติดตามงานกลายเป็น "ถูกแทนที่" และงานที่ยังเปิดย้ายมาฉบับใหม่
pub async fn revise(Path(id): Path<i64>, State(st): State<S>, Extension(u): Extension<AuthUser>, Json(b): Json<ReviseReq>) -> Response {
    if !can_write(&u) { return forbid("บัญชีผู้อ่านอย่างเดียวสร้างฉบับแก้ไขไม่ได้"); }
    let reason = clip(&b.reason.unwrap_or_default(), 1000);
    if reason.is_empty() { return bad("กรุณาระบุเหตุผลการแก้ไข"); }
    let Ok(mut c) = st.db.get().await else { return db_err() };
    let Some(m) = load_meeting(&c, id).await else { return err(StatusCode::NOT_FOUND, "ไม่พบการประชุม") };
    if !matches!(m["status"].as_str(), Some("monitoring" | "closed")) { return bad("สร้างฉบับแก้ไขได้จากฉบับที่อนุมัติแล้วเท่านั้น"); }
    if m["version"].as_i64() != Some(b.version) { return stale(); }
    let ref_no = s(&m, "refNo");
    let latest: i64 = c.query_one("SELECT MAX(revision) FROM mr_meetings WHERE ref_no=$1", &[&ref_no]).await.map(|r| r.get(0)).unwrap_or(0);
    if m["revision"].as_i64() != Some(latest) { return bad("ต้องสร้างฉบับแก้ไขจากฉบับล่าสุดเท่านั้น"); }
    let now = now_iso();
    let Ok(tx) = c.transaction().await else { return db_err() };
    let new_id: i64 = match tx.query_one(
        "INSERT INTO mr_meetings(ref_no,revision,parent_id,version,status,period,meet_date,meet_time,location,chair,secretary,attendees,next_due,inputs,created_by,created_name,created_at,updated_at)
         SELECT ref_no,revision+1,id,1,'draft',period,meet_date,meet_time,location,chair,secretary,attendees,next_due,inputs,$2,$3,$4,$4 FROM mr_meetings WHERE id=$1 RETURNING id",
        &[&id, &u.emp_code, &u.name, &now],
    ).await { Ok(r) => r.get(0), Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()) };
    // ทุกหัวข้อต้องทบทวนใหม่ในฉบับแก้ไข
    let mut inputs = m["inputs"].as_array().cloned().unwrap_or_else(empty_inputs);
    for i in inputs.iter_mut() { i["reviewed"] = json!(false); i["reviewedBy"] = json!(""); i["reviewedAt"] = json!(""); }
    let _ = tx.execute("UPDATE mr_meetings SET inputs=$2 WHERE id=$1", &[&new_id, &json!(inputs).to_string()]).await;
    let _ = tx.execute("UPDATE mr_actions SET meeting_id=$2, version=version+1, updated_at=$3 WHERE meeting_id=$1 AND status IN ('not_started','in_progress')", &[&id, &new_id, &now]).await;
    if m["status"] == json!("monitoring") {
        let _ = tx.execute("UPDATE mr_meetings SET status='superseded', version=version+1, updated_at=$2 WHERE id=$1", &[&id, &now]).await;
    }
    let _ = tx.execute(
        "INSERT INTO mr_signatures(meeting_id,stage,action,emp_code,name,position,comment,sha256,at) VALUES($1,'revision','revise',$2,$3,$4,$5,'',$6)",
        &[&id, &u.emp_code, &u.name, &u.position, &reason, &now],
    ).await;
    if tx.commit().await.is_err() { return err(StatusCode::INTERNAL_SERVER_ERROR, "สร้างฉบับแก้ไขไม่สำเร็จ"); }
    let _ = eform::audit(&c, &u, "mr_revise", "mr_meeting", &new_id.to_string(), &format!("{ref_no} {reason}")).await;
    detail_resp(&st, &u, new_id).await
}

// ---------- งานติดตาม ----------

#[derive(Deserialize)]
#[allow(non_snake_case)]
pub struct ActionFields {
    title: Option<String>, topic: Option<String>, ownerCode: Option<String>, dueDate: Option<String>, priority: Option<String>,
    status: Option<String>, reason: Option<String>, expected: Option<String>, budget: Option<f64>, resources: Option<String>,
    approval: Option<String>, evidence: Option<Value>, comment: Option<String>, effectiveness: Option<String>,
}

#[derive(Deserialize)]
#[allow(non_snake_case)]
pub struct ActionReq { version: Option<i64>, fields: ActionFields }

async fn employee(c: &impl deadpool_postgres::GenericClient, code: &str) -> Option<(String, String)> {
    let r = c.query_opt("SELECT name_th,dept FROM employees WHERE emp_code=$1 AND status='active'", &[&code]).await.ok()??;
    Some((r.get(0), r.get(1)))
}

fn clean_evidence(v: Option<Value>, cur: &Value) -> Result<Value, Response> {
    let Some(v) = v else { return Ok(cur["evidence"].clone()) };
    let mut out = vec![];
    for e in v.as_array().cloned().unwrap_or_default().into_iter().take(20) {
        let url = clip(&s(&e, "url"), 500);
        if !url.is_empty() && !(url.starts_with("https://") || url.starts_with("http://")) { return Err(bad("ลิงก์หลักฐานต้องขึ้นต้นด้วย http:// หรือ https://")); }
        let (code, title) = (clip(&s(&e, "code"), 60), clip(&s(&e, "title"), 200));
        if code.is_empty() && title.is_empty() && url.is_empty() { continue; }
        out.push(json!({ "code": code, "title": title, "url": url }));
    }
    Ok(json!(out))
}

/// เพิ่มงานติดตาม (สถานะร่างเท่านั้น)
pub async fn action_create(Path(id): Path<i64>, State(st): State<S>, Extension(u): Extension<AuthUser>, Json(b): Json<ActionReq>) -> Response {
    let Ok(c) = st.db.get().await else { return db_err() };
    let m = match draft_for_edit(&c, &u, id, b.version.unwrap_or(-1)).await { Ok(m) => m, Err(r) => return r };
    let f = b.fields;
    let title = clip(&f.title.unwrap_or_default(), 300);
    if title.is_empty() { return bad("กรุณาระบุชื่องาน"); }
    let topic = f.topic.unwrap_or_default();
    if !topic.is_empty() && !TOPICS.iter().any(|(k, _, _)| *k == topic) { return bad("หัวข้อไม่ถูกต้อง"); }
    let owner_code = clip(&f.ownerCode.unwrap_or_default(), 20);
    let (owner_name, dept) = if owner_code.is_empty() { (String::new(), String::new()) } else {
        match employee(&c, &owner_code).await { Some(x) => x, None => return bad("ไม่พบผู้รับผิดชอบในรายชื่อพนักงาน") }
    };
    let due = clip(&f.dueDate.unwrap_or_default(), 10);
    if !valid_date(&due) { return bad("รูปแบบกำหนดเสร็จไม่ถูกต้อง"); }
    let priority = match f.priority.as_deref() { Some("high") => "high", Some("low") => "low", _ => "medium" };
    let budget = f.budget.unwrap_or(0.0).max(0.0);
    let approval = if budget > 0.0 || !f.resources.as_deref().unwrap_or("").trim().is_empty() { "pending" } else { "not_required" };
    let evidence = match clean_evidence(f.evidence, &json!({ "evidence": [] })) { Ok(e) => e, Err(r) => return r };
    let n: i64 = c.query_one("SELECT COUNT(*) FROM mr_actions WHERE ref_no LIKE $1", &[&format!("{}-A%", s(&m, "refNo"))]).await.map(|r| r.get(0)).unwrap_or(0) + 1;
    let ref_no = format!("{}-A{n:02}", s(&m, "refNo"));
    let now = now_iso();
    // งานเปลี่ยน = เนื้อหาการประชุมเปลี่ยน → เลื่อน version ก่อน (กันการแก้ทับ/ลงนามบนข้อมูลเก่า)
    if let Err(r) = bump(&c, id, b.version.unwrap_or(-1), "status=status", &[]).await { return r; }
    let res = c.execute(
        "INSERT INTO mr_actions(meeting_id,version,ref_no,title,topic,owner_code,owner_name,dept,due_date,priority,status,reason,expected,budget,resources,approval,evidence,comment,created_at,updated_at)
         VALUES($1,1,$2,$3,$4,$5,$6,$7,$8,$9,'not_started',$10,$11,$12,$13,$14,$15,$16,$17,$17)",
        &[&id, &ref_no, &title, &topic, &owner_code, &owner_name, &dept, &due, &priority, &clip(&f.reason.unwrap_or_default(), 2000), &clip(&f.expected.unwrap_or_default(), 2000),
          &budget, &clip(&f.resources.unwrap_or_default(), 1000), &approval, &evidence.to_string(), &clip(&f.comment.unwrap_or_default(), 2000), &now],
    ).await;
    if let Err(e) = res { return err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()); }
    let _ = eform::audit(&c, &u, "mr_action_create", "mr_action", &ref_no, &title).await;
    detail_resp(&st, &u, id).await
}

/// แก้ไข/ติดตามงาน
///  - ร่าง: ผู้จัดทำแก้รายละเอียดทั้งหมด (อนุมัติงบได้เฉพาะผู้อนุมัติ)
///  - ติดตามงาน: รายละเอียดล็อก; เจ้าของงานหรือผู้ตรวจสอบขึ้นไปอัปเดตสถานะ/หลักฐาน/ความเห็น
///    เสร็จแล้วต้องมีหลักฐาน · ยกเลิกต้องมีเหตุผลและเป็นผู้ตรวจสอบขึ้นไป · ทวนสอบประสิทธิผลโดยผู้ตรวจสอบที่ไม่ใช่เจ้าของงาน
pub async fn action_update(Path(aid): Path<i64>, State(st): State<S>, Extension(u): Extension<AuthUser>, Json(b): Json<ActionReq>) -> Response {
    let Ok(c) = st.db.get().await else { return db_err() };
    let Some(cur) = c.query_opt(&format!("SELECT {ACOLS} FROM mr_actions WHERE id=$1"), &[&aid]).await.ok().flatten().map(|r| action_json(&r))
        else { return err(StatusCode::NOT_FOUND, "ไม่พบงาน") };
    if b.version != cur["version"].as_i64() { return stale(); }
    let mid = cur["meetingId"].as_i64().unwrap_or(0);
    let Some(m) = load_meeting(&c, mid).await else { return err(StatusCode::NOT_FOUND, "ไม่พบการประชุม") };
    let mstatus = s(&m, "status");
    let f = b.fields;
    let owner = s(&cur, "ownerCode") == u.emp_code;
    let reviewer = eform::can_review(&u.role);
    let now = now_iso();
    let mut v: Value = cur.clone();

    match mstatus.as_str() {
        "draft" => {
            if !can_write(&u) { return forbid("บัญชีผู้อ่านอย่างเดียวแก้ไขไม่ได้"); }
            if let Some(t) = f.title { let t = clip(&t, 300); if t.is_empty() { return bad("กรุณาระบุชื่องาน"); } v["title"] = json!(t); }
            if let Some(t) = f.topic { if !t.is_empty() && !TOPICS.iter().any(|(k, _, _)| *k == t) { return bad("หัวข้อไม่ถูกต้อง"); } v["topic"] = json!(t); }
            if let Some(code) = f.ownerCode {
                let code = clip(&code, 20);
                let (n, d) = if code.is_empty() { (String::new(), String::new()) } else { match employee(&c, &code).await { Some(x) => x, None => return bad("ไม่พบผู้รับผิดชอบในรายชื่อพนักงาน") } };
                v["ownerCode"] = json!(code); v["ownerName"] = json!(n); v["dept"] = json!(d);
            }
            if let Some(d) = f.dueDate { let d = clip(&d, 10); if !valid_date(&d) { return bad("รูปแบบกำหนดเสร็จไม่ถูกต้อง"); } v["dueDate"] = json!(d); }
            if let Some(p) = f.priority { v["priority"] = json!(match p.as_str() { "high" => "high", "low" => "low", _ => "medium" }); }
            if let Some(x) = f.reason { v["reason"] = json!(clip(&x, 2000)); }
            if let Some(x) = f.expected { v["expected"] = json!(clip(&x, 2000)); }
            let money_changed = f.budget.map(|x| x.max(0.0) != cur["budget"].as_f64().unwrap_or(0.0)).unwrap_or(false)
                || f.resources.as_deref().map(|x| x.trim() != s(&cur, "resources")).unwrap_or(false);
            if let Some(x) = f.budget { v["budget"] = json!(x.max(0.0)); }
            if let Some(x) = f.resources { v["resources"] = json!(clip(&x, 1000)); }
            let needs = v["budget"].as_f64().unwrap_or(0.0) > 0.0 || !s(&v, "resources").trim().is_empty();
            // เปลี่ยนงบ/ทรัพยากร → ต้องอนุมัติใหม่
            if money_changed || s(&v, "approval") == "not_required" { v["approval"] = json!(if needs { "pending" } else { "not_required" }); v["approvedBy"] = json!(""); }
            if let Some(a) = f.approval.as_deref() {
                match a {
                    "approved" if needs => { if !eform::can_approve(&u.role) { return forbid("อนุมัติงบ/ทรัพยากรได้เฉพาะผู้อนุมัติ"); } v["approval"] = json!("approved"); v["approvedBy"] = json!(u.name); }
                    "pending" if needs => { v["approval"] = json!("pending"); v["approvedBy"] = json!(""); }
                    _ => {}
                }
            }
            if let Some(x) = f.comment { v["comment"] = json!(clip(&x, 2000)); }
            match clean_evidence(f.evidence, &cur) { Ok(e) => v["evidence"] = e, Err(r) => return r }
        }
        "monitoring" => {
            for (name, given) in [("title", f.title.is_some()), ("topic", f.topic.is_some()), ("ownerCode", f.ownerCode.is_some()), ("dueDate", f.dueDate.is_some()),
                                  ("priority", f.priority.is_some()), ("reason", f.reason.is_some()), ("expected", f.expected.is_some()),
                                  ("budget", f.budget.is_some()), ("resources", f.resources.is_some()), ("approval", f.approval.is_some())] {
                if given { return bad(&format!("รายละเอียดงานที่อนุมัติแล้ว ({name}) แก้โดยตรงไม่ได้ — สร้างฉบับแก้ไข (revision)")); }
            }
            if !owner && !reviewer { return forbid("อัปเดตได้เฉพาะเจ้าของงาน หรือผู้ตรวจสอบขึ้นไป"); }
            if let Some(x) = f.comment { v["comment"] = json!(clip(&x, 2000)); }
            match clean_evidence(f.evidence, &cur) { Ok(e) => v["evidence"] = e, Err(r) => return r }
            if let Some(stt) = f.status.as_deref() {
                match stt {
                    "not_started" | "in_progress" => { v["status"] = json!(stt); v["completedAt"] = json!(""); v["effectiveness"] = json!("pending"); v["verifiedBy"] = json!(""); v["verifiedAt"] = json!(""); }
                    "completed" => {
                        if v["evidence"].as_array().map(|e| e.is_empty()).unwrap_or(true) { return bad("ปิดงานต้องแนบหลักฐานอย่างน้อย 1 รายการ"); }
                        if s(&cur, "status") != "completed" { v["completedAt"] = json!(now); }
                        v["status"] = json!("completed");
                    }
                    "cancelled" => {
                        if !reviewer { return forbid("ยกเลิกงานได้เฉพาะผู้ตรวจสอบขึ้นไป"); }
                        if s(&v, "comment").trim().is_empty() { return bad("ยกเลิกงานต้องระบุเหตุผลในช่องความเห็น"); }
                        v["status"] = json!("cancelled");
                    }
                    _ => return bad("สถานะไม่ถูกต้อง"),
                }
            }
            if let Some(eff) = f.effectiveness.as_deref() {
                if eff != s(&cur, "effectiveness") {
                    if !reviewer { return forbid("ทวนสอบประสิทธิผลได้เฉพาะผู้ตรวจสอบขึ้นไป"); }
                    if owner && !admin(&u) { return forbid("เจ้าของงานทวนสอบประสิทธิผลงานของตนเองไม่ได้"); }
                    if s(&v, "status") != "completed" { return bad("ทวนสอบได้เมื่องานเสร็จแล้ว"); }
                    match eff {
                        "effective" | "not_effective" => { v["effectiveness"] = json!(eff); v["verifiedBy"] = json!(u.name); v["verifiedAt"] = json!(now); }
                        _ => { v["effectiveness"] = json!("pending"); v["verifiedBy"] = json!(""); v["verifiedAt"] = json!(""); }
                    }
                    // ไม่มีประสิทธิผล → กลับไปดำเนินการต่อ
                    if eff == "not_effective" { v["status"] = json!("in_progress"); v["completedAt"] = json!(""); }
                }
            }
        }
        "review" | "approve" => return bad("ระหว่างรอตรวจสอบ/อนุมัติแก้งานไม่ได้ — ส่งกลับเป็นร่างก่อน"),
        _ => return bad("การประชุมนี้ปิดหรือถูกแทนที่แล้ว แก้งานไม่ได้"),
    }

    let n = c.execute(
        "UPDATE mr_actions SET version=version+1,title=$3,topic=$4,owner_code=$5,owner_name=$6,dept=$7,due_date=$8,priority=$9,status=$10,reason=$11,expected=$12,
           budget=$13,resources=$14,approval=$15,approved_by=$16,evidence=$17,comment=$18,effectiveness=$19,verified_by=$20,verified_at=$21,completed_at=$22,updated_at=$23
         WHERE id=$1 AND version=$2",
        &[&aid, &b.version.unwrap_or(-1), &s(&v, "title"), &s(&v, "topic"), &s(&v, "ownerCode"), &s(&v, "ownerName"), &s(&v, "dept"), &s(&v, "dueDate"),
          &s(&v, "priority"), &s(&v, "status"), &s(&v, "reason"), &s(&v, "expected"), &v["budget"].as_f64().unwrap_or(0.0), &s(&v, "resources"),
          &s(&v, "approval"), &s(&v, "approvedBy"), &v["evidence"].to_string(), &s(&v, "comment"), &s(&v, "effectiveness"), &s(&v, "verifiedBy"),
          &s(&v, "verifiedAt"), &s(&v, "completedAt"), &now],
    ).await.unwrap_or(0);
    if n == 0 { return stale(); }
    if mstatus == "draft" {
        let _ = c.execute("UPDATE mr_meetings SET version=version+1, updated_at=$2 WHERE id=$1", &[&mid, &now]).await;
    }
    let _ = eform::audit(&c, &u, "mr_action_update", "mr_action", &s(&cur, "refNo"), &format!("{} {}", s(&v, "status"), s(&v, "effectiveness"))).await;
    detail_resp(&st, &u, mid).await
}

/// ส่งออกครบชุด (ข้อมูลนำเข้า งาน ลายเซ็น และ snapshot ที่อนุมัติ)
pub async fn export(Path(id): Path<i64>, State(st): State<S>, Extension(u): Extension<AuthUser>) -> Response {
    let Ok(c) = st.db.get().await else { return db_err() };
    let Some(m) = load_meeting(&c, id).await else { return err(StatusCode::NOT_FOUND, "ไม่พบการประชุม") };
    let snap: Value = c.query_one("SELECT approved_snapshot FROM mr_meetings WHERE id=$1", &[&id]).await.ok()
        .and_then(|r| serde_json::from_str(&r.get::<_, String>(0)).ok()).unwrap_or(Value::Null);
    let actions = load_actions(&c, id).await;
    let _ = eform::audit(&c, &u, "mr_export", "mr_meeting", &id.to_string(), &s(&m, "refNo")).await;
    Json(json!({ "exportedAt": now_iso(), "exportedBy": u.name, "meeting": m, "completeness": completeness(&m, &actions),
                 "actions": actions, "signatures": signatures(&c, id).await, "approvedSnapshot": snap })).into_response()
}

/// สำหรับหน้าเตรียม Audit: การทบทวนที่อนุมัติล่าสุด และงานจากการทบทวนที่เลยกำหนด
pub async fn readiness_facts(c: &impl deadpool_postgres::GenericClient) -> (Option<(String, String)>, Vec<String>) {
    let last = c.query_opt("SELECT ref_no,meet_date FROM mr_meetings WHERE status IN ('monitoring','closed','superseded') ORDER BY meet_date DESC LIMIT 1", &[]).await
        .ok().flatten().map(|r| (r.get::<_, String>(0), r.get::<_, String>(1)));
    let late = c.query("SELECT ref_no FROM mr_actions WHERE status IN ('not_started','in_progress') AND due_date<>'' AND due_date<$1 AND meeting_id IN (SELECT id FROM mr_meetings WHERE status='monitoring')", &[&today()]).await
        .map(|rows| rows.iter().map(|r| r.get::<_, String>(0)).collect()).unwrap_or_default();
    (last, late)
}
