// Training matrix — ความสามารถของบุคลากร (ISO 9001:2015 ข้อ 7.2, 7.3)
//   ข้อกำหนดการอบรม ต่อ แผนก / ตำแหน่ง / บุคคล / ทุกคน:
//     course — จับคู่ประวัติการอบรม (training_log จาก FM-HR-008 หรือบันทึกตรง) ด้วยคำค้น หรือผูกกับข้อกำหนดโดยตรง; อายุใบรับรอง (เดือน) ได้
//     doc    — ต้องรับทราบเอกสารฉบับใช้งานปัจจุบัน (doc_acks)
//   ผลแต่ละช่อง: ผ่าน · หมดอายุ · รอรับทราบ · ต้องรับทราบฉบับใหม่ · ยังขาด
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
use std::collections::HashMap;

pub const SCHEMA: &str = "
ALTER TABLE IF EXISTS training_log ADD COLUMN IF NOT EXISTS source TEXT NOT NULL DEFAULT '';
ALTER TABLE IF EXISTS training_log ADD COLUMN IF NOT EXISTS req_id BIGINT NOT NULL DEFAULT 0;
ALTER TABLE IF EXISTS training_log ADD COLUMN IF NOT EXISTS recorded_by TEXT NOT NULL DEFAULT '';
ALTER TABLE IF EXISTS training_log ADD COLUMN IF NOT EXISTS note TEXT NOT NULL DEFAULT '';
CREATE TABLE IF NOT EXISTS training_reqs(
  id BIGSERIAL PRIMARY KEY,
  title TEXT NOT NULL,
  kind TEXT NOT NULL,
  keywords TEXT NOT NULL DEFAULT '',
  doc_code TEXT NOT NULL DEFAULT '',
  target_type TEXT NOT NULL,
  target TEXT NOT NULL DEFAULT '',
  validity_months BIGINT NOT NULL DEFAULT 0,
  clause TEXT NOT NULL DEFAULT '7.2',
  active BOOLEAN NOT NULL DEFAULT true,
  created_by TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL);
-- ข้อกำหนดรับทราบเอกสารที่ถูกปิดเพราะเอกสารยกเลิกใช้ (เปิดคืนเมื่อนำเอกสารกลับมาใช้)
ALTER TABLE training_reqs ADD COLUMN IF NOT EXISTS off_by_obsolete BOOLEAN NOT NULL DEFAULT false;
";

fn db_err() -> Response { err(StatusCode::INTERNAL_SERVER_ERROR, "เชื่อมต่อฐานข้อมูลไม่ได้") }
fn bad(m: &str) -> Response { err(StatusCode::BAD_REQUEST, m) }
fn forbid(m: &str) -> Response { err(StatusCode::FORBIDDEN, m) }
fn s(v: &Value, k: &str) -> String { v[k].as_str().unwrap_or("").to_string() }
fn clip(t: &str, n: usize) -> String { t.trim().chars().take(n).collect() }
fn today() -> String { crate::capa::today() }

const RCOLS: &str = "id,title,kind,keywords,doc_code,target_type,target,validity_months,clause,active,created_by,created_at,updated_at";
fn req_json(r: &tokio_postgres::Row) -> Value {
    json!({
        "id": r.get::<_, i64>(0), "title": r.get::<_, String>(1), "kind": r.get::<_, String>(2), "keywords": r.get::<_, String>(3),
        "docCode": r.get::<_, String>(4), "targetType": r.get::<_, String>(5), "target": r.get::<_, String>(6),
        "validityMonths": r.get::<_, i64>(7), "clause": r.get::<_, String>(8), "active": r.get::<_, bool>(9),
        "createdBy": r.get::<_, String>(10), "createdAt": r.get::<_, String>(11), "updatedAt": r.get::<_, String>(12),
    })
}

struct Emp { code: String, name: String, dept: String, position: String }

fn applies(req: &Value, e: &Emp) -> bool {
    let t = s(req, "target");
    match s(req, "targetType").as_str() {
        "all" => true,
        "dept" => t == e.dept,
        "position" => !t.is_empty() && t.trim() == e.position.trim(),
        "emp" => t == e.code,
        _ => false,
    }
}

/// ผลการอบรมที่ไม่นับว่าผ่าน
fn failed(result: &str) -> bool { let r = result.to_lowercase(); r.contains("ไม่ผ่าน") || r.contains("fail") || r.contains("ขาด") }

fn add_months(date: &str, months: i64) -> Option<String> {
    let d = chrono::NaiveDate::parse_from_str(date.get(..10).unwrap_or(date), "%Y-%m-%d").ok()?;
    let m = d.checked_add_months(chrono::Months::new(months.max(0) as u32))?;
    Some(m.format("%Y-%m-%d").to_string())
}

/// ข้อมูลที่ใช้คำนวณ matrix ทั้งหมดในครั้งเดียว
struct Facts {
    logs: HashMap<String, Vec<(String, String, String, String, i64)>>, // emp → (course, date, result, ref, req_id)
    acks: HashMap<(String, String), Vec<(i64, String, String)>>,      // (emp, doc) → (version, status, acked_at)
    current: HashMap<String, (i64, String)>,                           // doc → (version, rev)
}

async fn facts(c: &impl deadpool_postgres::GenericClient) -> Facts {
    let mut f = Facts { logs: HashMap::new(), acks: HashMap::new(), current: HashMap::new() };
    if let Ok(rows) = c.query("SELECT emp_code,course,train_date,result,ref_no,req_id FROM training_log WHERE emp_code<>''", &[]).await {
        for r in rows { f.logs.entry(r.get(0)).or_default().push((r.get(1), r.get(2), r.get(3), r.get(4), r.get(5))); }
    }
    if let Ok(rows) = c.query("SELECT emp_code,doc_code,version,status,acked_at FROM doc_acks", &[]).await {
        for r in rows { f.acks.entry((r.get(0), r.get(1))).or_default().push((r.get(2), r.get(3), r.get(4))); }
    }
    if let Ok(rows) = c.query("SELECT DISTINCT ON (doc_code) doc_code,version,rev FROM versions ORDER BY doc_code, version DESC", &[]).await {
        for r in rows { f.current.insert(r.get(0), (r.get(1), r.get(2))); }
    }
    f
}

/// สถานะหนึ่งช่องของ matrix
fn cell(req: &Value, e: &Emp, f: &Facts, st: &S) -> Value {
    let now = today();
    let rid = req["id"].as_i64().unwrap_or(0);
    if s(req, "kind") == "doc" {
        let code = s(req, "docCode");
        let Some((cur_v, cur_rev)) = f.current.get(&code).cloned() else { return json!({ "status": "missing", "detail": "ไม่พบเอกสาร" }) };
        let rev = if cur_rev.is_empty() { st.by_code.get(&code).and_then(|d| d["rev"].as_str()).unwrap_or("").to_string() } else { cur_rev };
        let list = f.acks.get(&(e.code.clone(), code.clone())).cloned().unwrap_or_default();
        if let Some((_, _, at)) = list.iter().find(|(v, stt, _)| *v == cur_v && stt == "acknowledged") {
            return json!({ "status": "met", "date": at.get(..10).unwrap_or(""), "detail": format!("รับทราบ {rev}") });
        }
        if list.iter().any(|(v, stt, _)| *v == cur_v && stt == "pending") {
            return json!({ "status": "pending", "detail": format!("รอรับทราบ {rev}") });
        }
        if list.iter().any(|(_, stt, _)| stt == "acknowledged") {
            return json!({ "status": "outdated", "detail": format!("รับทราบฉบับเก่า — ต้องรับทราบ {rev}") });
        }
        return json!({ "status": "missing", "detail": format!("ยังไม่ได้รับทราบ {rev}") });
    }
    let kws: Vec<String> = s(req, "keywords").split([',', '\n']).map(|k| k.trim().to_lowercase()).filter(|k| !k.is_empty()).collect();
    let mut best: Option<(String, String, String)> = None; // (date, course, ref)
    for (course, date, result, refno, req_id) in f.logs.get(&e.code).cloned().unwrap_or_default() {
        if failed(&result) { continue; }
        let hit = req_id == rid || kws.iter().any(|k| course.to_lowercase().contains(k));
        if !hit { continue; }
        if best.as_ref().map(|b| date > b.0).unwrap_or(true) { best = Some((date, course, refno)); }
    }
    let Some((date, course, refno)) = best else { return json!({ "status": "missing", "detail": "ยังไม่มีบันทึกการอบรม" }) };
    let months = req["validityMonths"].as_i64().unwrap_or(0);
    let expires = if months > 0 { add_months(&date, months) } else { None };
    if let Some(x) = &expires {
        if *x < now { return json!({ "status": "expired", "date": date, "expires": x, "detail": format!("หมดอายุ {x} — ต้องอบรมทบทวน"), "course": course, "ref": refno }); }
    }
    json!({ "status": "met", "date": date, "expires": expires, "detail": course, "course": course, "ref": refno })
}

async fn employees(c: &impl deadpool_postgres::GenericClient, dept: &str) -> Vec<Emp> {
    c.query("SELECT emp_code,name_th,dept,position FROM employees WHERE status='active' AND ($1='' OR dept=$1) ORDER BY dept, emp_code", &[&dept]).await
        .map(|rows| rows.iter().map(|r| Emp { code: r.get(0), name: r.get(1), dept: r.get(2), position: r.get(3) }).collect()).unwrap_or_default()
}
async fn active_reqs(c: &impl deadpool_postgres::GenericClient) -> Vec<Value> {
    c.query(&format!("SELECT {RCOLS} FROM training_reqs WHERE active ORDER BY kind, title"), &[]).await.map(|r| r.iter().map(req_json).collect()).unwrap_or_default()
}

/// สรุปทั้งบริษัท (ใช้ในเตรียม Audit / ทบทวนฝ่ายบริหาร)
pub async fn summary(st: &S, c: &impl deadpool_postgres::GenericClient) -> Value {
    let reqs = active_reqs(c).await;
    let emps = employees(c, "").await;
    let f = facts(c).await;
    let (mut total, mut met, mut gaps_people) = (0, 0, 0);
    let mut expired = 0;
    for e in &emps {
        let mut gap = false;
        for r in reqs.iter().filter(|r| applies(r, e)) {
            total += 1;
            let c = cell(r, e, &f, st);
            match c["status"].as_str() { Some("met") => met += 1, Some("expired") => { expired += 1; gap = true } _ => gap = true }
        }
        if gap { gaps_people += 1; }
    }
    json!({ "requirements": reqs.len(), "cells": total, "met": met, "expired": expired, "peopleWithGaps": gaps_people,
            "rate": if total == 0 { Value::Null } else { json!(((met as f64) / (total as f64) * 100.0).round() as i64) } })
}

/// หลักสูตรที่พนักงานคนนี้ยังขาด/หมดอายุ (ข้อกำหนดชนิดรับทราบเอกสารไปอยู่ในงาน "รับทราบเอกสาร" แล้ว) — ใช้ในกล่องงานของฉัน
pub async fn my_gaps(st: &S, c: &impl deadpool_postgres::GenericClient, emp_code: &str) -> Vec<(String, String)> {
    let Some(e) = employees(c, "").await.into_iter().find(|e| e.code == emp_code) else { return vec![] };
    let f = facts(c).await;
    active_reqs(c).await.iter().filter(|r| s(r, "kind") != "doc" && applies(r, &e)).filter_map(|r| {
        let cl = cell(r, &e, &f, st);
        matches!(cl["status"].as_str(), Some("missing" | "expired")).then(|| (s(r, "title"), s(&cl, "detail")))
    }).collect()
}

// ---------- handlers ----------

/// Training matrix (ผู้อนุมัติขึ้นไปเห็นทุกคน; คนอื่นเห็นเฉพาะของตนเอง)
pub async fn matrix(State(st): State<S>, Extension(u): Extension<AuthUser>, Query(q): Query<HashMap<String, String>>) -> Response {
    let Ok(c) = st.db.get().await else { return db_err() };
    let manager = eform::can_approve(&u.role);
    let dept = if manager { q.get("dept").cloned().unwrap_or_default() } else { String::new() };
    let reqs = active_reqs(&c).await;
    let mut emps = employees(&c, &dept).await;
    if !manager { emps.retain(|e| e.code == u.emp_code); }
    let f = facts(&c).await;
    let rows: Vec<Value> = emps.iter().map(|e| {
        let mut cells = serde_json::Map::new();
        let (mut need, mut ok) = (0, 0);
        for r in reqs.iter().filter(|r| applies(r, e)) {
            let x = cell(r, e, &f, &st);
            need += 1;
            if x["status"] == json!("met") { ok += 1; }
            cells.insert(r["id"].to_string(), x);
        }
        json!({ "code": e.code, "name": e.name, "dept": e.dept, "position": e.position, "cells": cells, "required": need, "met": ok })
    }).collect();
    let positions: Vec<String> = c.query("SELECT DISTINCT position FROM employees WHERE status='active' AND position<>'' ORDER BY 1", &[]).await
        .map(|r| r.iter().map(|x| x.get(0)).collect()).unwrap_or_default();
    Json(json!({ "requirements": reqs, "employees": rows, "positions": positions, "canManage": manager, "summary": summary(&st, &c).await })).into_response()
}

/// แหล่งที่มาของบันทึก: form (FM-HR-008) · ack (รับทราบเอกสาร) · manual (บันทึกตรง)
fn source_of(source: String, record_id: i64) -> String {
    if !source.is_empty() { source } else if record_id > 0 { "form".into() } else { "ack".into() }
}

/// ประวัติการอบรมของพนักงานหนึ่งคน (ตนเอง หรือผู้อนุมัติขึ้นไป)
pub async fn history(Path(code): Path<String>, State(st): State<S>, Extension(u): Extension<AuthUser>) -> Response {
    if code != u.emp_code && !eform::can_approve(&u.role) { return forbid("ดูประวัติของผู้อื่นได้เฉพาะผู้อนุมัติขึ้นไป"); }
    let Ok(c) = st.db.get().await else { return db_err() };
    let rows = c.query("SELECT course,train_date,result,ref_no,source,recorded_by,note,record_id FROM training_log WHERE emp_code=$1 ORDER BY train_date DESC", &[&code]).await.unwrap_or_default();
    Json(Value::Array(rows.iter().map(|r| json!({
        "course": r.get::<_, String>(0), "date": r.get::<_, String>(1), "result": r.get::<_, String>(2), "ref": r.get::<_, String>(3),
        "source": source_of(r.get::<_, String>(4), r.get::<_, i64>(7)),
        "recordedBy": r.get::<_, String>(5), "note": r.get::<_, String>(6),
    })).collect())).into_response()
}

#[derive(Deserialize)]
#[allow(non_snake_case)]
pub struct ReqFields {
    title: Option<String>, kind: Option<String>, keywords: Option<String>, docCode: Option<String>,
    targetType: Option<String>, target: Option<String>, validityMonths: Option<i64>, clause: Option<String>, active: Option<bool>,
}

fn validate(st: &S, v: &mut Value, f: ReqFields) -> Result<(), Response> {
    if let Some(t) = f.title { v["title"] = json!(clip(&t, 200)); }
    if let Some(k) = f.kind { if !matches!(k.as_str(), "course" | "doc") { return Err(bad("ชนิดต้องเป็นหลักสูตรอบรมหรือเอกสารที่ต้องรับทราบ")); } v["kind"] = json!(k); }
    if let Some(k) = f.keywords { v["keywords"] = json!(clip(&k, 500)); }
    if let Some(d) = f.docCode { let d = clip(&d, 60); if !d.is_empty() && !st.by_code.contains_key(&d) { return Err(bad("ไม่พบรหัสเอกสารในทะเบียน")); } v["docCode"] = json!(d); }
    if let Some(t) = f.targetType { if !matches!(t.as_str(), "all" | "dept" | "position" | "emp") { return Err(bad("กลุ่มเป้าหมายไม่ถูกต้อง")); } v["targetType"] = json!(t); }
    if let Some(t) = f.target { v["target"] = json!(clip(&t, 120)); }
    if let Some(m) = f.validityMonths { v["validityMonths"] = json!(m.clamp(0, 120)); }
    if let Some(c) = f.clause { v["clause"] = json!(clip(&c, 10)); }
    if let Some(a) = f.active { v["active"] = json!(a); }
    if s(v, "title").is_empty() { return Err(bad("ต้องระบุชื่อข้อกำหนด")); }
    if s(v, "kind") == "doc" && s(v, "docCode").is_empty() { return Err(bad("ต้องเลือกเอกสารที่ต้องรับทราบ")); }
    if s(v, "targetType") != "all" && s(v, "target").is_empty() { return Err(bad("ต้องระบุแผนก/ตำแหน่ง/พนักงาน")); }
    Ok(())
}

pub async fn create_req(State(st): State<S>, Extension(u): Extension<AuthUser>, Json(f): Json<ReqFields>) -> Response {
    if !eform::can_approve(&u.role) { return forbid("กำหนดข้อกำหนดการอบรมได้เฉพาะผู้อนุมัติขึ้นไป"); }
    let mut v = json!({ "kind": "course", "targetType": "all", "target": "", "validityMonths": 0, "clause": "7.2", "keywords": "", "docCode": "" });
    if let Err(r) = validate(&st, &mut v, f) { return r; }
    let Ok(c) = st.db.get().await else { return db_err() };
    let now = now_iso();
    let id: i64 = match c.query_one(
        "INSERT INTO training_reqs(title,kind,keywords,doc_code,target_type,target,validity_months,clause,active,created_by,created_at,updated_at)
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,true,$9,$10,$10) RETURNING id",
        &[&s(&v, "title"), &s(&v, "kind"), &s(&v, "keywords"), &s(&v, "docCode"), &s(&v, "targetType"), &s(&v, "target"), &v["validityMonths"].as_i64().unwrap_or(0), &s(&v, "clause"), &u.name, &now],
    ).await { Ok(r) => r.get(0), Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()) };
    let _ = eform::audit(&c, &u, "training_req", "training_req", &id.to_string(), &s(&v, "title")).await;
    if s(&v, "kind") == "doc" { crate::acks::on_training_req(&st, &c, &s(&v, "docCode"), &u.name).await; }
    Json(json!({ "ok": true, "id": id })).into_response()
}

pub async fn update_req(Path(id): Path<i64>, State(st): State<S>, Extension(u): Extension<AuthUser>, Json(f): Json<ReqFields>) -> Response {
    if !eform::can_approve(&u.role) { return forbid("แก้ข้อกำหนดการอบรมได้เฉพาะผู้อนุมัติขึ้นไป"); }
    let Ok(c) = st.db.get().await else { return db_err() };
    let Some(mut v) = c.query_opt(&format!("SELECT {RCOLS} FROM training_reqs WHERE id=$1"), &[&id]).await.ok().flatten().map(|r| req_json(&r)) else { return err(StatusCode::NOT_FOUND, "ไม่พบข้อกำหนด") };
    if let Err(r) = validate(&st, &mut v, f) { return r; }
    let _ = c.execute(
        "UPDATE training_reqs SET title=$2,kind=$3,keywords=$4,doc_code=$5,target_type=$6,target=$7,validity_months=$8,clause=$9,active=$10,updated_at=$11 WHERE id=$1",
        &[&id, &s(&v, "title"), &s(&v, "kind"), &s(&v, "keywords"), &s(&v, "docCode"), &s(&v, "targetType"), &s(&v, "target"), &v["validityMonths"].as_i64().unwrap_or(0), &s(&v, "clause"), &v["active"].as_bool().unwrap_or(true), &now_iso()],
    ).await;
    let _ = eform::audit(&c, &u, if v["active"] == json!(false) { "training_req_off" } else { "training_req_edit" }, "training_req", &id.to_string(), &s(&v, "title")).await;
    if s(&v, "kind") == "doc" && v["active"] != json!(false) { crate::acks::on_training_req(&st, &c, &s(&v, "docCode"), &u.name).await; }
    Json(json!({ "ok": true })).into_response()
}

#[derive(Deserialize)]
#[allow(non_snake_case)]
pub struct RecordReq { empCodes: Vec<String>, reqId: Option<i64>, course: Option<String>, date: String, result: Option<String>, refNo: Option<String>, note: Option<String> }

/// บันทึกการอบรมที่ไม่ได้มาจาก FM-HR-008 (อบรมภายนอก, OJT, ใบรับรอง) — ผู้อนุมัติขึ้นไป
pub async fn record(State(st): State<S>, Extension(u): Extension<AuthUser>, Json(b): Json<RecordReq>) -> Response {
    if !eform::can_approve(&u.role) { return forbid("บันทึกการอบรมได้เฉพาะผู้อนุมัติขึ้นไป"); }
    if b.empCodes.is_empty() { return bad("ต้องเลือกพนักงานอย่างน้อย 1 คน"); }
    if chrono::NaiveDate::parse_from_str(&b.date, "%Y-%m-%d").is_err() { return bad("รูปแบบวันที่ไม่ถูกต้อง"); }
    if b.date > today() { return bad("วันที่อบรมต้องไม่อยู่ในอนาคต"); }
    let Ok(c) = st.db.get().await else { return db_err() };
    let req_id = b.reqId.unwrap_or(0);
    let mut course = clip(&b.course.unwrap_or_default(), 300);
    if req_id > 0 {
        let Some(r) = c.query_opt("SELECT title,kind FROM training_reqs WHERE id=$1", &[&req_id]).await.ok().flatten() else { return bad("ไม่พบข้อกำหนด") };
        if r.get::<_, String>(1) == "doc" { return bad("ข้อกำหนดชนิดรับทราบเอกสาร ต้องรับทราบผ่านระบบ ไม่บันทึกแทน"); }
        if course.is_empty() { course = r.get(0); }
    }
    if course.is_empty() { return bad("ต้องระบุหลักสูตร"); }
    let result = clip(&b.result.unwrap_or_else(|| "ผ่าน".into()), 60);
    let (refno, note) = (clip(&b.refNo.unwrap_or_default(), 120), clip(&b.note.unwrap_or_default(), 1000));
    let mut n = 0;
    for code in b.empCodes.iter().take(500) {
        let Some(e) = c.query_opt("SELECT name_th,dept FROM employees WHERE emp_code=$1", &[code]).await.ok().flatten() else { continue };
        let ok = c.execute(
            "INSERT INTO training_log(record_id,emp_code,name,course,train_date,result,ref_no,dept,source,req_id,recorded_by,note) VALUES(0,$1,$2,$3,$4,$5,$6,$7,'manual',$8,$9,$10)",
            &[code, &e.get::<_, String>(0), &course, &b.date, &result, &refno, &e.get::<_, String>(1), &req_id, &u.name, &note],
        ).await.is_ok();
        if ok { n += 1; }
    }
    let _ = eform::audit(&c, &u, "training_record", "training", &refno, &format!("{course} {} · {n} คน", b.date)).await;
    Json(json!({ "ok": true, "count": n })).into_response()
}
