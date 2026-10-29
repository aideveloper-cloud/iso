// โปรแกรมตรวจติดตามคุณภาพภายใน (ISO 9001:2015 ข้อ 9.2)
//   แผนประจำปี → การตรวจแต่ละครั้ง (ขอบเขต: ข้อกำหนด + แผนก, ทีมผู้ตรวจ) → checklist จากแผนผังข้อกำหนด (data/clauses.json)
//   → ผลแต่ละข้อ: สอดคล้อง / ข้อสังเกต (OFI) / NC เล็กน้อย / NC สำคัญ / ไม่เกี่ยวข้อง → NC เปิด CAR (source = internal_audit)
//   → หัวหน้าผู้ตรวจลงนามรายงาน (PIN) → ปิดเมื่อ CAR ทุกใบปิด/ยกเลิก (ผู้อนุมัติขึ้นไป)
//   ความเป็นกลาง (9.2.2 b): ผู้ตรวจต้องขึ้นทะเบียนผู้ตรวจติดตามภายใน และไม่ตรวจแผนกของตนเอง
use crate::capa;
use crate::eform::{self, AuthUser};
use crate::{err, now_iso, sha256_hex, S};
use axum::{
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

pub const SCHEMA: &str = "
ALTER TABLE IF EXISTS employees ADD COLUMN IF NOT EXISTS internal_auditor BOOLEAN NOT NULL DEFAULT false;
CREATE TABLE IF NOT EXISTS ia_audits(
  id BIGSERIAL PRIMARY KEY,
  ref_no TEXT NOT NULL,
  year_be BIGINT NOT NULL,
  version BIGINT NOT NULL DEFAULT 1,
  status TEXT NOT NULL,
  title TEXT NOT NULL,
  clauses TEXT NOT NULL DEFAULT '[]',
  depts TEXT NOT NULL DEFAULT '[]',
  criteria TEXT NOT NULL DEFAULT '',
  planned_date TEXT NOT NULL DEFAULT '',
  actual_date TEXT NOT NULL DEFAULT '',
  lead_code TEXT NOT NULL DEFAULT '',
  lead_name TEXT NOT NULL DEFAULT '',
  team TEXT NOT NULL DEFAULT '[]',
  auditee_code TEXT NOT NULL DEFAULT '',
  auditee_name TEXT NOT NULL DEFAULT '',
  summary TEXT NOT NULL DEFAULT '',
  report_by TEXT NOT NULL DEFAULT '',
  report_at TEXT NOT NULL DEFAULT '',
  report_sha TEXT NOT NULL DEFAULT '',
  closed_by TEXT NOT NULL DEFAULT '',
  closed_at TEXT NOT NULL DEFAULT '',
  cancel_reason TEXT NOT NULL DEFAULT '',
  created_by TEXT NOT NULL,
  created_name TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS idx_ia_year ON ia_audits(year_be);
CREATE TABLE IF NOT EXISTS ia_items(
  id BIGSERIAL PRIMARY KEY,
  audit_id BIGINT NOT NULL,
  seq BIGINT NOT NULL DEFAULT 0,
  clause TEXT NOT NULL DEFAULT '',
  question TEXT NOT NULL,
  doc_refs TEXT NOT NULL DEFAULT '[]',
  result TEXT NOT NULL DEFAULT 'pending',
  evidence TEXT NOT NULL DEFAULT '',
  finding TEXT NOT NULL DEFAULT '',
  dept TEXT NOT NULL DEFAULT '',
  car_id BIGINT NOT NULL DEFAULT 0,
  car_ref TEXT NOT NULL DEFAULT '',
  updated_by TEXT NOT NULL DEFAULT '',
  updated_at TEXT NOT NULL DEFAULT '');
CREATE INDEX IF NOT EXISTS idx_iai_audit ON ia_items(audit_id);
";

const RESULTS: [&str; 6] = ["pending", "conform", "ofi", "minor", "major", "na"];
const ACOLS: &str = "id,ref_no,year_be,version,status,title,clauses,depts,criteria,planned_date,actual_date,lead_code,lead_name,team,auditee_code,auditee_name,summary,report_by,report_at,report_sha,closed_by,closed_at,cancel_reason,created_by,created_name,created_at,updated_at";
const ICOLS: &str = "id,audit_id,seq,clause,question,doc_refs,result,evidence,finding,dept,car_id,car_ref,updated_by,updated_at";

fn db_err() -> Response { err(StatusCode::INTERNAL_SERVER_ERROR, "เชื่อมต่อฐานข้อมูลไม่ได้") }
fn bad(m: &str) -> Response { err(StatusCode::BAD_REQUEST, m) }
fn forbid(m: &str) -> Response { err(StatusCode::FORBIDDEN, m) }
fn stale() -> Response { err(StatusCode::CONFLICT, "ข้อมูลถูกแก้ไขโดยผู้อื่นแล้ว — โหลดใหม่ก่อนบันทึก") }
fn s(v: &Value, k: &str) -> String { v[k].as_str().unwrap_or("").to_string() }
fn clip(t: &str, n: usize) -> String { t.trim().chars().take(n).collect() }
fn valid_date(d: &str) -> bool { d.is_empty() || chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").is_ok() }
fn arr(v: &Value) -> Vec<String> { v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default() }

fn audit_json(r: &tokio_postgres::Row) -> Value {
    let g = |i: usize| r.get::<_, String>(i);
    let parse = |i: usize| serde_json::from_str::<Value>(&g(i)).unwrap_or(json!([]));
    json!({
        "id": r.get::<_, i64>(0), "refNo": g(1), "year": r.get::<_, i64>(2), "version": r.get::<_, i64>(3), "status": g(4), "title": g(5),
        "clauses": parse(6), "depts": parse(7), "criteria": g(8), "plannedDate": g(9), "actualDate": g(10), "leadCode": g(11), "leadName": g(12),
        "team": parse(13), "auditeeCode": g(14), "auditeeName": g(15), "summary": g(16), "reportBy": g(17), "reportAt": g(18), "reportSha": g(19),
        "closedBy": g(20), "closedAt": g(21), "cancelReason": g(22), "createdBy": g(23), "createdName": g(24), "createdAt": g(25), "updatedAt": g(26),
    })
}
fn item_json(r: &tokio_postgres::Row) -> Value {
    let g = |i: usize| r.get::<_, String>(i);
    json!({
        "id": r.get::<_, i64>(0), "auditId": r.get::<_, i64>(1), "seq": r.get::<_, i64>(2), "clause": g(3), "question": g(4),
        "docRefs": serde_json::from_str::<Value>(&g(5)).unwrap_or(json!([])), "result": g(6), "evidence": g(7), "finding": g(8), "dept": g(9),
        "carId": r.get::<_, i64>(10), "carRef": g(11), "updatedBy": g(12), "updatedAt": g(13),
    })
}

async fn load(c: &impl deadpool_postgres::GenericClient, id: i64) -> Option<Value> {
    c.query_opt(&format!("SELECT {ACOLS} FROM ia_audits WHERE id=$1"), &[&id]).await.ok()?.map(|r| audit_json(&r))
}
async fn items(c: &impl deadpool_postgres::GenericClient, id: i64) -> Vec<Value> {
    c.query(&format!("SELECT {ICOLS} FROM ia_items WHERE audit_id=$1 ORDER BY seq,id"), &[&id]).await
        .map(|rows| rows.iter().map(item_json).collect()).unwrap_or_default()
}

/// ทีมผู้ตรวจ (รหัสพนักงาน) รวมหัวหน้า
fn team_codes(a: &Value) -> Vec<String> {
    let mut v: Vec<String> = a["team"].as_array().map(|t| t.iter().filter_map(|m| m["code"].as_str().map(String::from)).collect()).unwrap_or_default();
    let lead = s(a, "leadCode");
    if !lead.is_empty() && !v.contains(&lead) { v.insert(0, lead); }
    v
}

fn can_flags(u: &AuthUser, a: &Value, its: &[Value], car_open: usize) -> Value {
    let st = s(a, "status");
    // บัญชีอ่านอย่างเดียวไม่นับเป็นทีมผู้ตรวจแม้ถูกใส่ชื่อไว้
    let on_team = u.role != "viewer" && team_codes(a).contains(&u.emp_code);
    let lead = s(a, "leadCode") == u.emp_code;
    let planner = eform::can_approve(&u.role);
    let pending = its.iter().filter(|i| i["result"] == json!("pending")).count();
    let nc_without_car = its.iter().filter(|i| matches!(i["result"].as_str(), Some("minor" | "major")) && i["carId"].as_i64().unwrap_or(0) == 0).count();
    json!({
        "plan": matches!(st.as_str(), "planned") && planner,
        "start": st == "planned" && (on_team || planner),
        "conduct": st == "in_progress" && (on_team || eform::is_admin(&u.role)),
        "report": st == "in_progress" && (lead || eform::is_admin(&u.role)) && pending == 0 && nc_without_car == 0,
        "close": st == "reported" && eform::can_approve(&u.role) && car_open == 0 && (!on_team || eform::is_admin(&u.role)),
        "cancel": matches!(st.as_str(), "planned" | "in_progress") && planner,
        "pending": pending, "ncWithoutCar": nc_without_car, "openCars": car_open,
    })
}

async fn open_cars(c: &impl deadpool_postgres::GenericClient, its: &[Value]) -> usize {
    let ids: Vec<i64> = its.iter().filter_map(|i| i["carId"].as_i64()).filter(|x| *x > 0).collect();
    if ids.is_empty() { return 0; }
    c.query_one("SELECT COUNT(*) FROM actions WHERE id = ANY($1) AND status NOT IN ('closed','cancelled')", &[&ids]).await.map(|r| r.get::<_, i64>(0) as usize).unwrap_or(0)
}

async fn detail_resp(st: &S, u: &AuthUser, id: i64) -> Response {
    let Ok(c) = st.db.get().await else { return db_err() };
    let Some(mut a) = load(&c, id).await else { return err(StatusCode::NOT_FOUND, "ไม่พบการตรวจ") };
    let mut its = items(&c, id).await;
    // สถานะ CAR ปัจจุบันของข้อบกพร่องแต่ละข้อ
    for it in its.iter_mut() {
        let cid = it["carId"].as_i64().unwrap_or(0);
        if cid > 0 {
            if let Ok(Some(r)) = c.query_opt("SELECT status,effectiveness FROM actions WHERE id=$1", &[&cid]).await {
                it["carStatus"] = json!(r.get::<_, String>(0)); it["carEffectiveness"] = json!(r.get::<_, String>(1));
            }
        }
    }
    let oc = open_cars(&c, &its).await;
    a["can"] = can_flags(u, &a, &its, oc);
    a["summaryCounts"] = counts(&its);
    a["items"] = json!(its);
    Json(a).into_response()
}

fn counts(its: &[Value]) -> Value {
    let n = |r: &str| its.iter().filter(|i| i["result"] == json!(r)).count();
    json!({ "total": its.len(), "pending": n("pending"), "conform": n("conform"), "ofi": n("ofi"), "minor": n("minor"), "major": n("major"), "na": n("na") })
}

/// ตรวจทีมผู้ตรวจ: ต้องเป็นผู้ตรวจที่ขึ้นทะเบียน และไม่อยู่แผนกที่ถูกตรวจ (ความเป็นกลาง 9.2.2 b)
async fn check_team(c: &impl deadpool_postgres::GenericClient, codes: &[String], depts: &[String]) -> Result<Vec<Value>, Response> {
    let mut out = vec![];
    for code in codes {
        let Some(r) = c.query_opt("SELECT name_th,dept,internal_auditor,status FROM employees WHERE emp_code=$1", &[code]).await.ok().flatten()
            else { return Err(bad(&format!("ไม่พบพนักงาน {code}"))); };
        let (name, dept, qualified, status): (String, String, bool, String) = (r.get(0), r.get(1), r.get(2), r.get(3));
        if status != "active" { return Err(bad(&format!("{name} ไม่ได้ใช้งานแล้ว"))); }
        if !qualified { return Err(bad(&format!("{name} ยังไม่ขึ้นทะเบียนเป็นผู้ตรวจติดตามภายใน (กำหนดในหน้า ผู้ใช้และสิทธิ์)"))); }
        if !dept.is_empty() && depts.contains(&dept) { return Err(bad(&format!("{name} อยู่แผนก {dept} ที่ถูกตรวจ — ผู้ตรวจต้องไม่ตรวจงานของตนเอง (ข้อ 9.2.2 b)"))); }
        out.push(json!({ "code": code, "name": name, "dept": dept }));
    }
    Ok(out)
}

/// คำถาม checklist เริ่มต้นจากแผนผังข้อกำหนด: ข้อละหนึ่งคำถาม พร้อมเอกสารอ้างอิงที่เกี่ยวข้องกับแผนกที่ถูกตรวจ
fn default_items(st: &S, clauses: &[String], depts: &[String]) -> Vec<(String, String, Vec<String>)> {
    let catalog = st.clauses["clauses"].as_array().cloned().unwrap_or_default();
    let mut out = vec![];
    for c in catalog {
        let cl = s(&c, "clause");
        let hit = clauses.iter().any(|x| cl == *x || cl.starts_with(&format!("{x}.")));
        if !hit { continue; }
        let mut refs: Vec<String> = arr(&c["docs"]);
        if !depts.is_empty() {
            let mine: Vec<String> = refs.iter().filter(|code| st.by_code.get(*code).and_then(|d| d["dept"].as_str()).map(|d| depts.iter().any(|x| x == d)).unwrap_or(false)).cloned().collect();
            if !mine.is_empty() { refs = mine; }
        }
        let q = format!("ข้อ {cl} {} — มีการปฏิบัติตามขั้นตอนและมีบันทึกเป็นหลักฐานครบถ้วน{}", s(&c, "title"),
            if refs.is_empty() { String::new() } else { format!(" (ตรวจตาม {})", refs.join(", ")) });
        out.push((cl, q, refs));
    }
    out
}

// ---------- handlers ----------

/// รายการตรวจของปี + ความครอบคลุมข้อกำหนด + ผู้ตรวจที่ขึ้นทะเบียน
pub async fn list(State(st): State<S>, Extension(u): Extension<AuthUser>, Query(q): Query<HashMap<String, String>>) -> Response {
    let Ok(c) = st.db.get().await else { return db_err() };
    let year: i64 = q.get("year").and_then(|y| y.parse().ok()).unwrap_or_else(eform::be_year_now);
    let rows = c.query(&format!("SELECT {ACOLS} FROM ia_audits WHERE year_be=$1 ORDER BY planned_date, id"), &[&year]).await.unwrap_or_default();
    let mut audits = vec![];
    let mut covered: HashMap<String, Vec<String>> = HashMap::new();
    for r in &rows {
        let mut a = audit_json(r);
        let id = a["id"].as_i64().unwrap_or(0);
        let its = items(&c, id).await;
        a["summaryCounts"] = counts(&its);
        a["openCars"] = json!(open_cars(&c, &its).await);
        if a["status"] != json!("cancelled") {
            for cl in arr(&a["clauses"]) { covered.entry(cl).or_default().push(s(&a, "refNo")); }
        }
        audits.push(a);
    }
    // ความครอบคลุม: ทุกข้อในแผนผังต้องอยู่ในแผนอย่างน้อยหนึ่งการตรวจ (ข้อหลักครอบคลุมข้อย่อย)
    let catalog = st.clauses["clauses"].as_array().cloned().unwrap_or_default();
    let coverage: Vec<Value> = catalog.iter().map(|c| {
        let cl = s(c, "clause");
        let by: Vec<String> = covered.iter().filter(|(k, _)| cl == **k || cl.starts_with(&format!("{k}."))).flat_map(|(_, v)| v.clone()).collect::<HashSet<_>>().into_iter().collect();
        json!({ "clause": cl, "title": s(c, "title"), "audits": by })
    }).collect();
    let auditors = c.query("SELECT emp_code,name_th,dept,position FROM employees WHERE internal_auditor AND status='active' ORDER BY emp_code", &[]).await
        .map(|rows| rows.iter().map(|r| json!({ "code": r.get::<_, String>(0), "name": r.get::<_, String>(1), "dept": r.get::<_, String>(2), "position": r.get::<_, String>(3) })).collect::<Vec<_>>())
        .unwrap_or_default();
    let years: Vec<i64> = c.query("SELECT DISTINCT year_be FROM ia_audits ORDER BY 1 DESC", &[]).await.map(|r| r.iter().map(|x| x.get(0)).collect()).unwrap_or_default();
    Json(json!({ "year": year, "years": years, "audits": audits, "coverage": coverage, "auditors": auditors, "canPlan": eform::can_approve(&u.role) })).into_response()
}

pub async fn get(Path(id): Path<i64>, State(st): State<S>, Extension(u): Extension<AuthUser>) -> Response { detail_resp(&st, &u, id).await }

#[derive(Deserialize, Default)]
#[allow(non_snake_case)]
pub struct PlanFields {
    title: Option<String>, clauses: Option<Vec<String>>, depts: Option<Vec<String>>, criteria: Option<String>, plannedDate: Option<String>,
    leadCode: Option<String>, team: Option<Vec<String>>, auditeeCode: Option<String>, year: Option<i64>,
}

async fn apply_plan(st: &S, c: &impl deadpool_postgres::GenericClient, a: &mut Value, f: PlanFields) -> Result<(), Response> {
    if let Some(t) = f.title { let t = clip(&t, 300); if t.is_empty() { return Err(bad("ต้องระบุชื่อการตรวจ")); } a["title"] = json!(t); }
    if let Some(cls) = f.clauses {
        let known: HashSet<String> = st.clauses["clauses"].as_array().cloned().unwrap_or_default().iter().map(|c| s(c, "clause")).collect();
        let mut v: Vec<String> = cls.into_iter().map(|x| clip(&x, 10)).filter(|x| !x.is_empty()).collect();
        v.sort(); v.dedup();
        for x in &v { if !known.iter().any(|k| k == x || k.starts_with(&format!("{x}."))) { return Err(bad(&format!("ไม่รู้จักข้อกำหนด {x}"))); } }
        a["clauses"] = json!(v);
    }
    if let Some(d) = f.depts { let mut v: Vec<String> = d.into_iter().map(|x| clip(&x, 10)).filter(|x| !x.is_empty()).collect(); v.sort(); v.dedup(); a["depts"] = json!(v); }
    if let Some(x) = f.criteria { a["criteria"] = json!(clip(&x, 1000)); }
    if let Some(d) = f.plannedDate { let d = clip(&d, 10); if !valid_date(&d) { return Err(bad("รูปแบบวันที่ไม่ถูกต้อง")); } a["plannedDate"] = json!(d); }
    if let Some(y) = f.year { a["year"] = json!(y); }
    if let Some(code) = f.auditeeCode {
        let code = clip(&code, 20);
        let name = if code.is_empty() { String::new() } else {
            c.query_opt("SELECT name_th FROM employees WHERE emp_code=$1 AND status='active'", &[&code]).await.ok().flatten().map(|r| r.get::<_, String>(0)).ok_or_else(|| bad("ไม่พบผู้รับการตรวจ"))?
        };
        a["auditeeCode"] = json!(code); a["auditeeName"] = json!(name);
    }
    let depts = arr(&a["depts"]);
    let lead = f.leadCode.map(|x| clip(&x, 20)).unwrap_or_else(|| s(a, "leadCode"));
    let mut codes: Vec<String> = f.team.map(|t| t.into_iter().map(|x| clip(&x, 20)).filter(|x| !x.is_empty()).collect())
        .unwrap_or_else(|| team_codes(a).into_iter().filter(|x| *x != s(a, "leadCode")).collect());
    if !lead.is_empty() { codes.retain(|x| *x != lead); codes.insert(0, lead.clone()); }
    let team = check_team(c, &codes, &depts).await?;
    a["leadCode"] = json!(lead);
    a["leadName"] = json!(team.first().map(|m| s(m, "name")).unwrap_or_default());
    a["team"] = json!(team);
    Ok(())
}

/// วางแผนการตรวจ (ผู้อนุมัติขึ้นไป — เช่น QMR)
pub async fn create(State(st): State<S>, Extension(u): Extension<AuthUser>, Json(f): Json<PlanFields>) -> Response {
    if !eform::can_approve(&u.role) { return forbid("วางแผนการตรวจได้เฉพาะผู้อนุมัติ หรือผู้ดูแลระบบ"); }
    if f.title.as_deref().map(|t| t.trim().is_empty()).unwrap_or(true) { return bad("ต้องระบุชื่อการตรวจ"); }
    if f.clauses.as_ref().map(|c| c.is_empty()).unwrap_or(true) { return bad("ต้องเลือกข้อกำหนดที่จะตรวจอย่างน้อย 1 ข้อ"); }
    if f.leadCode.as_deref().map(|t| t.trim().is_empty()).unwrap_or(true) { return bad("ต้องกำหนดหัวหน้าผู้ตรวจ"); }
    let Ok(c) = st.db.get().await else { return db_err() };
    let mut a = json!({ "year": eform::be_year_now(), "clauses": [], "depts": [], "team": [] });
    if let Err(r) = apply_plan(&st, &c, &mut a, f).await { return r; }
    let year = a["year"].as_i64().unwrap_or_else(eform::be_year_now);
    let seq: i64 = match c.query_one(
        "INSERT INTO form_seq(form_code,year_be,last_seq) VALUES('IA',$1,1) ON CONFLICT(form_code,year_be) DO UPDATE SET last_seq=form_seq.last_seq+1 RETURNING last_seq", &[&year],
    ).await { Ok(r) => r.get(0), Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()) };
    let ref_no = format!("IA-{year}-{seq:02}");
    let now = now_iso();
    let id: i64 = match c.query_one(
        "INSERT INTO ia_audits(ref_no,year_be,version,status,title,clauses,depts,criteria,planned_date,lead_code,lead_name,team,auditee_code,auditee_name,created_by,created_name,created_at,updated_at)
         VALUES($1,$2,1,'planned',$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$15) RETURNING id",
        &[&ref_no, &year, &s(&a, "title"), &a["clauses"].to_string(), &a["depts"].to_string(), &s(&a, "criteria"), &s(&a, "plannedDate"),
          &s(&a, "leadCode"), &s(&a, "leadName"), &a["team"].to_string(), &s(&a, "auditeeCode"), &s(&a, "auditeeName"), &u.emp_code, &u.name, &now],
    ).await { Ok(r) => r.get(0), Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()) };
    let _ = eform::audit(&c, &u, "ia_plan", "internal_audit", &ref_no, &s(&a, "title")).await;
    detail_resp(&st, &u, id).await
}

#[derive(Deserialize)]
pub struct UpdateReq { version: i64, fields: PlanFields }

pub async fn update(Path(id): Path<i64>, State(st): State<S>, Extension(u): Extension<AuthUser>, Json(b): Json<UpdateReq>) -> Response {
    let Ok(c) = st.db.get().await else { return db_err() };
    let Some(mut a) = load(&c, id).await else { return err(StatusCode::NOT_FOUND, "ไม่พบการตรวจ") };
    if a["status"] != json!("planned") { return bad("แก้แผนได้เฉพาะก่อนเริ่มตรวจ"); }
    if !eform::can_approve(&u.role) { return forbid("แก้แผนได้เฉพาะผู้อนุมัติขึ้นไป"); }
    if a["version"].as_i64() != Some(b.version) { return stale(); }
    if let Err(r) = apply_plan(&st, &c, &mut a, b.fields).await { return r; }
    let n = c.execute(
        "UPDATE ia_audits SET version=version+1,title=$3,clauses=$4,depts=$5,criteria=$6,planned_date=$7,lead_code=$8,lead_name=$9,team=$10,auditee_code=$11,auditee_name=$12,year_be=$13,updated_at=$14
         WHERE id=$1 AND version=$2",
        &[&id, &b.version, &s(&a, "title"), &a["clauses"].to_string(), &a["depts"].to_string(), &s(&a, "criteria"), &s(&a, "plannedDate"),
          &s(&a, "leadCode"), &s(&a, "leadName"), &a["team"].to_string(), &s(&a, "auditeeCode"), &s(&a, "auditeeName"), &a["year"].as_i64().unwrap_or(0), &now_iso()],
    ).await.unwrap_or(0);
    if n == 0 { return stale(); }
    let _ = eform::audit(&c, &u, "ia_plan_edit", "internal_audit", &s(&a, "refNo"), "").await;
    detail_resp(&st, &u, id).await
}

#[derive(Deserialize)]
pub struct ActReq { version: i64, action: String, comment: Option<String>, pin: Option<String> }

/// start (สร้าง checklist) · report (หัวหน้าผู้ตรวจลงนาม PIN) · close (ผู้อนุมัติ, CAR ปิดครบ) · cancel
pub async fn act(Path(id): Path<i64>, State(st): State<S>, Extension(u): Extension<AuthUser>, Json(b): Json<ActReq>) -> Response {
    let Ok(c) = st.db.get().await else { return db_err() };
    let Some(a) = load(&c, id).await else { return err(StatusCode::NOT_FOUND, "ไม่พบการตรวจ") };
    if a["version"].as_i64() != Some(b.version) { return stale(); }
    let its = items(&c, id).await;
    let can = can_flags(&u, &a, &its, open_cars(&c, &its).await);
    let comment = clip(&b.comment.unwrap_or_default(), 5000);
    let now = now_iso();
    let ref_no = s(&a, "refNo");
    let (set, params): (&str, Vec<String>) = match b.action.as_str() {
        "start" => {
            if can["start"] != json!(true) { return forbid("เริ่มตรวจได้เฉพาะทีมผู้ตรวจหรือผู้อนุมัติขึ้นไป"); }
            // สร้าง checklist ครั้งแรกจากแผนผังข้อกำหนด
            if its.is_empty() {
                for (i, (cl, q, refs)) in default_items(&st, &arr(&a["clauses"]), &arr(&a["depts"])).into_iter().enumerate() {
                    let _ = c.execute("INSERT INTO ia_items(audit_id,seq,clause,question,doc_refs) VALUES($1,$2,$3,$4,$5)",
                        &[&id, &((i as i64 + 1) * 10), &cl, &q, &json!(refs).to_string()]).await;
                }
            }
            let today = capa::today();
            ("status='in_progress', actual_date=CASE WHEN actual_date='' THEN $3 ELSE actual_date END", vec![today])
        }
        "report" => {
            if can["report"] != json!(true) {
                return bad(&format!("ลงนามรายงานได้เมื่อ: เป็นหัวหน้าผู้ตรวจ · บันทึกผลครบทุกข้อ (ค้าง {}) · ข้อบกพร่องเปิด CAR ครบ (ยังไม่เปิด {})", can["pending"], can["ncWithoutCar"]));
            }
            if comment.is_empty() { return bad("กรุณาสรุปผลการตรวจ"); }
            if let Err(r) = eform::verify_pin(&st, &c, &u, b.pin.as_deref().unwrap_or("")).await { return r; }
            let sha = sha256_hex(json!({ "audit": a, "items": its, "summary": comment }).to_string().as_bytes());
            ("status='reported', summary=$3, report_by=$4, report_at=$5, report_sha=$6", vec![comment.clone(), u.name.clone(), now.clone(), sha])
        }
        "close" => {
            if can["close"] != json!(true) {
                return bad("ปิดได้โดยผู้อนุมัติที่ไม่อยู่ในทีมผู้ตรวจ เมื่อ CAR จากการตรวจนี้ปิด/ยกเลิกครบ");
            }
            if let Err(r) = eform::verify_pin(&st, &c, &u, b.pin.as_deref().unwrap_or("")).await { return r; }
            ("status='closed', closed_by=$3, closed_at=$4", vec![u.name.clone(), now.clone()])
        }
        "cancel" => {
            if can["cancel"] != json!(true) { return forbid("ยกเลิกได้เฉพาะผู้อนุมัติขึ้นไป ก่อนลงนามรายงาน"); }
            if comment.is_empty() { return bad("กรุณาระบุเหตุผลการยกเลิก"); }
            ("status='cancelled', cancel_reason=$3", vec![comment.clone()])
        }
        _ => return bad("การดำเนินการไม่ถูกต้อง"),
    };
    let sql = format!("UPDATE ia_audits SET version=version+1, updated_at='{now}', {set} WHERE id=$1 AND version=$2");
    let mut p: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = vec![&id, &b.version];
    for x in &params { p.push(x); }
    if c.execute(&sql, &p).await.unwrap_or(0) == 0 { return stale(); }
    let _ = eform::audit(&c, &u, &format!("ia_{}", b.action), "internal_audit", &ref_no, &comment).await;
    detail_resp(&st, &u, id).await
}

#[derive(Deserialize)]
#[allow(non_snake_case)]
pub struct ItemReq {
    id: Option<i64>, clause: Option<String>, question: Option<String>, result: Option<String>, evidence: Option<String>,
    finding: Option<String>, dept: Option<String>, docRefs: Option<Vec<String>>, remove: Option<bool>,
}

/// บันทึก/เพิ่ม/ลบข้อ checklist (ทีมผู้ตรวจ ระหว่างตรวจ)
pub async fn save_item(Path(id): Path<i64>, State(st): State<S>, Extension(u): Extension<AuthUser>, Json(b): Json<ItemReq>) -> Response {
    let Ok(c) = st.db.get().await else { return db_err() };
    let Some(a) = load(&c, id).await else { return err(StatusCode::NOT_FOUND, "ไม่พบการตรวจ") };
    let its = items(&c, id).await;
    if can_flags(&u, &a, &its, 0)["conduct"] != json!(true) { return forbid("บันทึกผลได้เฉพาะทีมผู้ตรวจ ขณะกำลังตรวจ"); }
    let now = now_iso();
    match b.id {
        Some(iid) => {
            let Some(cur) = its.iter().find(|i| i["id"].as_i64() == Some(iid)).cloned() else { return err(StatusCode::NOT_FOUND, "ไม่พบข้อนี้") };
            if b.remove == Some(true) {
                if cur["carId"].as_i64().unwrap_or(0) > 0 { return bad("ข้อนี้เปิด CAR แล้ว ลบไม่ได้"); }
                let _ = c.execute("DELETE FROM ia_items WHERE id=$1 AND audit_id=$2", &[&iid, &id]).await;
            } else {
                let result = b.result.unwrap_or_else(|| s(&cur, "result"));
                if !RESULTS.contains(&result.as_str()) { return bad("ผลการตรวจไม่ถูกต้อง"); }
                if cur["carId"].as_i64().unwrap_or(0) > 0 && !matches!(result.as_str(), "minor" | "major") { return bad("ข้อนี้เปิด CAR แล้ว เปลี่ยนเป็นไม่ใช่ข้อบกพร่องไม่ได้"); }
                let evidence = clip(&b.evidence.unwrap_or_else(|| s(&cur, "evidence")), 5000);
                let finding = clip(&b.finding.unwrap_or_else(|| s(&cur, "finding")), 5000);
                if matches!(result.as_str(), "conform" | "ofi" | "minor" | "major") && evidence.is_empty() { return bad("ต้องบันทึกหลักฐานที่สุ่มตรวจ (เอกสาร/บันทึก/การสัมภาษณ์)"); }
                if matches!(result.as_str(), "ofi" | "minor" | "major") && finding.is_empty() { return bad("ต้องบรรยายข้อพบ (สิ่งที่พบ เทียบกับข้อกำหนด)"); }
                let question = clip(&b.question.unwrap_or_else(|| s(&cur, "question")), 2000);
                let dept = clip(&b.dept.unwrap_or_else(|| s(&cur, "dept")), 10);
                let refs = b.docRefs.map(|r| json!(r).to_string()).unwrap_or_else(|| cur["docRefs"].to_string());
                let _ = c.execute("UPDATE ia_items SET result=$3,evidence=$4,finding=$5,question=$6,dept=$7,doc_refs=$8,updated_by=$9,updated_at=$10 WHERE id=$1 AND audit_id=$2",
                    &[&iid, &id, &result, &evidence, &finding, &question, &dept, &refs, &u.name, &now]).await;
            }
        }
        None => {
            let question = clip(&b.question.unwrap_or_default(), 2000);
            if question.is_empty() { return bad("ต้องระบุคำถาม/สิ่งที่ตรวจ"); }
            let seq: i64 = its.iter().filter_map(|i| i["seq"].as_i64()).max().unwrap_or(0) + 10;
            let _ = c.execute("INSERT INTO ia_items(audit_id,seq,clause,question,doc_refs,dept,updated_by,updated_at) VALUES($1,$2,$3,$4,$5,$6,$7,$8)",
                &[&id, &seq, &clip(&b.clause.unwrap_or_default(), 10), &question, &json!(b.docRefs.unwrap_or_default()).to_string(), &clip(&b.dept.unwrap_or_default(), 10), &u.name, &now]).await;
        }
    }
    let _ = c.execute("UPDATE ia_audits SET version=version+1, updated_at=$2 WHERE id=$1", &[&id, &now]).await;
    detail_resp(&st, &u, id).await
}

#[derive(Deserialize)]
#[allow(non_snake_case)]
pub struct CarReq { assigneeCode: Option<String>, dueDate: Option<String> }

/// เปิด CAR จากข้อบกพร่อง (NC) — เชื่อมเลขที่การตรวจไว้ในใบ
pub async fn raise_car(Path((id, iid)): Path<(i64, i64)>, State(st): State<S>, Extension(u): Extension<AuthUser>, Json(b): Json<CarReq>) -> Response {
    let Ok(c) = st.db.get().await else { return db_err() };
    let Some(a) = load(&c, id).await else { return err(StatusCode::NOT_FOUND, "ไม่พบการตรวจ") };
    let its = items(&c, id).await;
    if can_flags(&u, &a, &its, 0)["conduct"] != json!(true) { return forbid("เปิด CAR ได้เฉพาะทีมผู้ตรวจ ขณะกำลังตรวจ"); }
    let Some(it) = its.iter().find(|i| i["id"].as_i64() == Some(iid)).cloned() else { return err(StatusCode::NOT_FOUND, "ไม่พบข้อนี้") };
    if !matches!(it["result"].as_str(), Some("minor" | "major")) { return bad("เปิด CAR ได้เฉพาะข้อบกพร่อง (NC เล็กน้อย/สำคัญ)"); }
    if it["carId"].as_i64().unwrap_or(0) > 0 { return bad(&format!("ข้อนี้เปิด {} แล้ว", s(&it, "carRef"))); }
    let ref_no = s(&a, "refNo");
    let major = it["result"] == json!("major");
    let doc = arr(&it["docRefs"]).into_iter().find(|d| st.by_code.contains_key(d));
    let dept = if s(&it, "dept").is_empty() { arr(&a["depts"]).first().cloned().unwrap_or_default() } else { s(&it, "dept") };
    let f = capa::Fields {
        kind: Some("CAR".into()),
        title: Some(clip(&format!("{ref_no} ข้อ {}: {}", s(&it, "clause"), s(&it, "finding")), 300)),
        detail: Some(format!("ข้อบกพร่อง{} จากการตรวจติดตามภายใน {ref_no}\nข้อพบ: {}\nหลักฐาน: {}", if major { "สำคัญ (Major)" } else { "เล็กน้อย (Minor)" }, s(&it, "finding"), s(&it, "evidence"))),
        source: Some("internal_audit".into()),
        dept: Some(dept),
        docCode: doc,
        assigneeCode: b.assigneeCode.filter(|x| !x.trim().is_empty()).or_else(|| Some(s(&a, "auditeeCode")).filter(|x| !x.is_empty())),
        // ค่าเริ่มต้นกำหนดตอบกลับ: NC สำคัญ 14 วัน · NC เล็กน้อย 30 วัน (แก้ได้ในใบ CAR)
        dueDate: b.dueDate.filter(|x| !x.trim().is_empty()).or_else(|| {
            let today = chrono::NaiveDate::parse_from_str(&capa::today(), "%Y-%m-%d").ok()?;
            Some((today + chrono::Duration::days(if major { 14 } else { 30 })).format("%Y-%m-%d").to_string())
        }),
        priority: Some(if major { "high" } else { "medium" }.into()),
        linkedRef: Some(ref_no.clone()),
        ..Default::default()
    };
    let (car_id, car_ref) = match capa::open_action(&st, &c, &u, f).await { Ok(x) => x, Err(r) => return r };
    let _ = c.execute("UPDATE ia_items SET car_id=$3, car_ref=$4, updated_by=$5, updated_at=$6 WHERE id=$1 AND audit_id=$2", &[&iid, &id, &car_id, &car_ref, &u.name, &now_iso()]).await;
    let _ = c.execute("UPDATE ia_audits SET version=version+1, updated_at=$2 WHERE id=$1", &[&id, &now_iso()]).await;
    let _ = eform::audit(&c, &u, "ia_raise_car", "internal_audit", &ref_no, &car_ref).await;
    detail_resp(&st, &u, id).await
}

#[derive(Deserialize)]
pub struct AuditorReq { code: String, qualified: bool }

/// ทะเบียนผู้ตรวจติดตามภายใน (FM-MR-011 ในระบบ) — admin กำหนด
pub async fn set_auditor(State(st): State<S>, Extension(u): Extension<AuthUser>, Json(b): Json<AuditorReq>) -> Response {
    if !eform::is_admin(&u.role) { return forbid("เฉพาะผู้ดูแลระบบ"); }
    let Ok(c) = st.db.get().await else { return db_err() };
    let n = c.execute("UPDATE employees SET internal_auditor=$2 WHERE emp_code=$1", &[&b.code, &b.qualified]).await.unwrap_or(0);
    if n == 0 { return err(StatusCode::NOT_FOUND, "ไม่พบพนักงาน"); }
    let _ = eform::audit(&c, &u, "ia_auditor", "employee", &b.code, if b.qualified { "ขึ้นทะเบียนผู้ตรวจติดตามภายใน" } else { "ถอนจากทะเบียนผู้ตรวจ" }).await;
    Json(json!({ "ok": true })).into_response()
}

/// สำหรับหน้าเตรียม Audit / ทบทวนฝ่ายบริหาร
pub async fn facts(c: &impl deadpool_postgres::GenericClient) -> Value {
    let year = eform::be_year_now();
    let rows = c.query(&format!("SELECT {ACOLS} FROM ia_audits WHERE year_be=$1 AND status<>'cancelled'"), &[&year]).await.unwrap_or_default();
    let audits: Vec<Value> = rows.iter().map(audit_json).collect();
    let since = (chrono::Utc::now() - chrono::Duration::days(365)).to_rfc3339();
    let done = c.query(&format!("SELECT {ACOLS} FROM ia_audits WHERE status IN ('reported','closed') AND report_at>=$1 ORDER BY report_at DESC"), &[&since]).await
        .map(|r| r.iter().map(audit_json).collect::<Vec<_>>()).unwrap_or_default();
    let mut done_out = vec![];
    for a in done {
        let its = items(c, a["id"].as_i64().unwrap_or(0)).await;
        done_out.push(json!({ "refNo": a["refNo"], "title": a["title"], "status": a["status"], "reportAt": a["reportAt"], "counts": counts(&its), "openCars": open_cars(c, &its).await }));
    }
    let overdue: Vec<String> = audits.iter().filter(|a| a["status"] == json!("planned") && !s(a, "plannedDate").is_empty() && s(a, "plannedDate") < capa::today()).map(|a| s(a, "refNo")).collect();
    json!({ "year": year, "planned": audits.iter().map(|a| json!({ "refNo": a["refNo"], "clauses": a["clauses"], "status": a["status"] })).collect::<Vec<_>>(), "done": done_out, "overdue": overdue })
}
