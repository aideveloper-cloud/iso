// ผู้ให้บริการภายนอก / ผู้ขาย (ISO 9001:2015 ข้อ 8.4)
//   ทะเบียนผู้ขายที่อนุมัติ SUP-xxx: ประเภท สินค้า/บริการ ผู้รับผิดชอบประเมิน · สถานะ อนุมัติ / มีเงื่อนไข / ระงับ
//   ประเมินตามรอบ (SEV-ปี-xxx): คุณภาพ 40% · การส่งมอบ 30% · ราคา 15% · บริการ 15% (คะแนน 1–5) → % และเกรด A–D → สถานะที่แนะนำ
//   กำหนดประเมินซ้ำ: อนุมัติ = ตามรอบของผู้ขาย (ค่าเริ่มต้น 12 เดือน) · มีเงื่อนไข = 3 เดือน · ระงับ = ไม่มีกำหนด
//   ผลประเมินต่ำ (เกรด C/D หรือมีหัวข้อได้ 1) เปิด CAR แหล่งที่มา "ผู้ขาย" ได้ — ผูกเลขที่ใบกับผลประเมิน
use crate::capa;
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
CREATE TABLE IF NOT EXISTS suppliers(
  id BIGSERIAL PRIMARY KEY,
  code TEXT NOT NULL UNIQUE,
  version BIGINT NOT NULL DEFAULT 1,
  name TEXT NOT NULL,
  vendor_code TEXT NOT NULL DEFAULT '',
  category TEXT NOT NULL DEFAULT 'material',
  products TEXT NOT NULL DEFAULT '',
  contact TEXT NOT NULL DEFAULT '',
  phone TEXT NOT NULL DEFAULT '',
  email TEXT NOT NULL DEFAULT '',
  owner_code TEXT NOT NULL DEFAULT '',
  owner_name TEXT NOT NULL DEFAULT '',
  eval_months BIGINT NOT NULL DEFAULT 12,
  status TEXT NOT NULL DEFAULT 'conditional',
  status_reason TEXT NOT NULL DEFAULT '',
  next_eval TEXT NOT NULL DEFAULT '',
  last_eval TEXT NOT NULL DEFAULT '',
  last_score BIGINT NOT NULL DEFAULT 0,
  last_grade TEXT NOT NULL DEFAULT '',
  created_by TEXT NOT NULL,
  created_name TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS supplier_evals(
  id BIGSERIAL PRIMARY KEY,
  supplier_id BIGINT NOT NULL,
  ref_no TEXT NOT NULL UNIQUE,
  period TEXT NOT NULL DEFAULT '',
  eval_date TEXT NOT NULL,
  quality BIGINT NOT NULL,
  delivery BIGINT NOT NULL,
  price BIGINT NOT NULL,
  service BIGINT NOT NULL,
  score BIGINT NOT NULL,
  grade TEXT NOT NULL,
  status_before TEXT NOT NULL DEFAULT '',
  status_after TEXT NOT NULL,
  next_eval TEXT NOT NULL DEFAULT '',
  note TEXT NOT NULL DEFAULT '',
  evaluator_code TEXT NOT NULL,
  evaluator_name TEXT NOT NULL,
  car_ref TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS idx_sev_sup ON supplier_evals(supplier_id);
CREATE TABLE IF NOT EXISTS supplier_events(
  id BIGSERIAL PRIMARY KEY,
  supplier_id BIGINT NOT NULL,
  event TEXT NOT NULL,
  actor_code TEXT NOT NULL,
  actor_name TEXT NOT NULL,
  note TEXT NOT NULL DEFAULT '',
  at TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS idx_spe_sup ON supplier_events(supplier_id);
";

const COLS: &str = "p.id,p.code,p.version,p.name,p.vendor_code,p.category,p.products,p.contact,p.phone,p.email,p.owner_code,p.owner_name,p.eval_months,
  p.status,p.status_reason,p.next_eval,p.last_eval,p.last_score,p.last_grade,p.created_by,p.created_name,p.created_at,p.updated_at,
  (SELECT COUNT(*) FROM supplier_evals v WHERE v.supplier_id=p.id AND v.grade IN ('C','D') AND v.car_ref='')";
const VCOLS: &str = "id,supplier_id,ref_no,period,eval_date,quality,delivery,price,service,score,grade,status_before,status_after,next_eval,note,evaluator_code,evaluator_name,car_ref,created_at";
const CATEGORIES: [&str; 4] = ["material", "outsource", "service", "other"];
const STATUSES: [&str; 3] = ["approved", "conditional", "blocked"];
/// น้ำหนักหัวข้อประเมิน (%): คุณภาพ · การส่งมอบ · ราคา · บริการ
pub const WEIGHTS: [i64; 4] = [40, 30, 15, 15];
/// ผู้ขายแบบมีเงื่อนไขต้องประเมินซ้ำภายในกี่เดือน
const CONDITIONAL_MONTHS: i64 = 3;
/// ผู้ขายใหม่ต้องประเมินครั้งแรกภายในกี่วัน (ค่าเริ่มต้นของกำหนดประเมิน)
const FIRST_EVAL_DAYS: i64 = 30;

fn db_err() -> Response { err(StatusCode::INTERNAL_SERVER_ERROR, "เชื่อมต่อฐานข้อมูลไม่ได้") }
fn bad(m: &str) -> Response { err(StatusCode::BAD_REQUEST, m) }
fn forbid(m: &str) -> Response { err(StatusCode::FORBIDDEN, m) }
fn stale() -> Response { err(StatusCode::CONFLICT, "ข้อมูลถูกแก้ไขโดยผู้อื่นแล้ว — โหลดใหม่ก่อนบันทึก") }
fn s(v: &Value, k: &str) -> String { v[k].as_str().unwrap_or("").to_string() }
fn i(v: &Value, k: &str) -> i64 { v[k].as_i64().unwrap_or(0) }
fn clip(t: &str, n: usize) -> String { t.trim().chars().take(n).collect() }
fn parse(d: &str) -> Option<chrono::NaiveDate> { chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok() }
fn valid_date(d: &str) -> bool { d.is_empty() || parse(d).is_some() }
fn plus_days(d: &str, n: i64) -> String { parse(d).map(|x| (x + chrono::Duration::days(n)).format("%Y-%m-%d").to_string()).unwrap_or_default() }

/// คะแนนรวม (%) จากคะแนน 1–5 ของแต่ละหัวข้อ ถ่วงน้ำหนัก — เต็ม 100
pub fn score(q: i64, d: i64, p: i64, sv: i64) -> i64 {
    let sum: i64 = [q, d, p, sv].iter().zip(WEIGHTS).map(|(x, w)| x * w).sum();
    (sum as f64 / 5.0).round() as i64
}
/// เกรด: A ≥ 85 · B 70–84 · C 50–69 · D < 50
pub fn grade(score: i64) -> &'static str {
    if score >= 85 { "A" } else if score >= 70 { "B" } else if score >= 50 { "C" } else { "D" }
}
/// ผลประเมินต่ำ = เกรด C/D หรือมีหัวข้อใดได้ 1 คะแนน → ควรเปิด CAR
pub fn poor(grade: &str, min_item: i64) -> bool { matches!(grade, "C" | "D") || min_item <= 1 }
/// สถานะที่แนะนำจากผลประเมิน
pub fn recommended(grade: &str, min_item: i64) -> &'static str {
    match grade { "D" => "blocked", "C" => "conditional", _ if min_item <= 1 => "conditional", _ => "approved" }
}
fn rank(status: &str) -> i64 { match status { "approved" => 2, "conditional" => 1, _ => 0 } }
/// กำหนดประเมินครั้งถัดไปตามสถานะ
pub fn next_eval_for(status: &str, from: &str, months: i64) -> String {
    let m = match status { "approved" => months, "conditional" => CONDITIONAL_MONTHS, _ => return String::new() };
    crate::calibration::add_months(from, m)
}
pub fn status_th(st: &str) -> &'static str {
    match st { "approved" => "อนุมัติ", "conditional" => "มีเงื่อนไข", "blocked" => "ระงับ", _ => "-" }
}

fn row_json(r: &tokio_postgres::Row) -> Value {
    let g = |k: usize| r.get::<_, String>(k);
    let n = |k: usize| r.get::<_, i64>(k);
    let (status, next_eval) = (g(13), g(15));
    let today = capa::today();
    let state = if status == "blocked" { "none" } else { crate::calibration::due_state(&next_eval, &today) };
    json!({
        "id": n(0), "code": g(1), "version": n(2), "name": g(3), "vendorCode": g(4), "category": g(5), "products": g(6), "contact": g(7), "phone": g(8),
        "email": g(9), "ownerCode": g(10), "ownerName": g(11), "evalMonths": n(12), "status": status, "statusReason": g(14), "nextEval": next_eval,
        "lastEval": g(16), "lastScore": n(17), "lastGrade": g(18), "createdBy": g(19), "createdName": g(20), "createdAt": g(21), "updatedAt": g(22),
        "poorNoCar": n(23), "dueState": state, "daysLeft": crate::calibration::days_left(&next_eval, &today),
        "evalOverdue": state == "overdue", "neverEvaluated": g(16).is_empty(),
    })
}
fn eval_json(r: &tokio_postgres::Row) -> Value {
    let g = |k: usize| r.get::<_, String>(k);
    let n = |k: usize| r.get::<_, i64>(k);
    let (q, d, p, sv) = (n(5), n(6), n(7), n(8));
    let gr = g(10);
    json!({
        "id": n(0), "supplierId": n(1), "refNo": g(2), "period": g(3), "evalDate": g(4), "quality": q, "delivery": d, "price": p, "service": sv,
        "score": n(9), "grade": gr, "statusBefore": g(11), "statusAfter": g(12), "nextEval": g(13), "note": g(14), "evaluatorCode": g(15),
        "evaluatorName": g(16), "carRef": g(17), "createdAt": g(18), "poor": poor(&gr, q.min(d).min(p).min(sv)),
    })
}

fn can_flags(u: &AuthUser, p: &Value) -> Value {
    let approver = eform::can_approve(&u.role);
    let owner = s(p, "ownerCode") == u.emp_code;
    let write = u.role != "viewer";
    json!({
        "edit": write && (owner || approver || s(p, "createdBy") == u.emp_code),
        "evaluate": write && (owner || approver),
        "car": write && (owner || approver),
        "setStatus": write && approver,
        // ผู้อนุมัติขึ้นไปกำหนดสถานะต่างจากที่แนะนำได้ (พร้อมเหตุผล) — ผู้รับผิดชอบทั่วไปใช้สถานะตามผลประเมิน
        "override": write && approver,
    })
}

fn gaps(p: &Value) -> Vec<&'static str> {
    let mut g = vec![];
    if s(p, "name").trim().is_empty() { g.push("ชื่อผู้ขาย"); }
    if s(p, "products").trim().is_empty() { g.push("สินค้า/บริการที่ซื้อ"); }
    if s(p, "ownerCode").is_empty() { g.push("ผู้รับผิดชอบประเมิน"); }
    if !(1..=36).contains(&i(p, "evalMonths")) { g.push("รอบประเมิน"); }
    g
}

async fn load(c: &impl deadpool_postgres::GenericClient, id: i64) -> Option<Value> {
    let r = c.query_opt(&format!("SELECT {COLS} FROM suppliers p WHERE p.id=$1"), &[&id]).await.ok()??;
    Some(row_json(&r))
}
async fn event(c: &impl deadpool_postgres::GenericClient, id: i64, ev: &str, u: &AuthUser, note: &str) {
    let _ = c.execute("INSERT INTO supplier_events(supplier_id,event,actor_code,actor_name,note,at) VALUES($1,$2,$3,$4,$5,$6)",
        &[&id, &ev.to_string(), &u.emp_code, &u.name, &note.to_string(), &now_iso()]).await;
}

async fn detail_resp(st: &S, u: &AuthUser, id: i64) -> Response {
    let Ok(c) = st.db.get().await else { return db_err() };
    let Some(mut p) = load(&c, id).await else { return err(StatusCode::NOT_FOUND, "ไม่พบผู้ขายนี้") };
    let can = can_flags(u, &p);
    let evals: Vec<Value> = c.query(&format!("SELECT {VCOLS} FROM supplier_evals WHERE supplier_id=$1 ORDER BY eval_date DESC, id DESC"), &[&id]).await
        .map(|rows| rows.iter().map(|r| {
            let mut v = eval_json(r);
            v["can"] = json!({ "car": can["car"] == json!(true) && v["poor"] == json!(true) && s(&v, "carRef").is_empty() });
            v
        }).collect()).unwrap_or_default();
    p["can"] = can;
    p["gaps"] = json!(gaps(&p));
    p["evaluations"] = json!(evals);
    p["history"] = json!(c.query("SELECT event,actor_name,note,at FROM supplier_events WHERE supplier_id=$1 ORDER BY id", &[&id]).await
        .map(|rows| rows.iter().map(|x| json!({ "event": x.get::<_, String>(0), "actorName": x.get::<_, String>(1), "note": x.get::<_, String>(2), "at": x.get::<_, String>(3) })).collect::<Vec<_>>())
        .unwrap_or_default());
    Json(json!({ "ok": true, "supplier": p })).into_response()
}

// ---------- handlers ----------

pub async fn list(State(st): State<S>, Extension(u): Extension<AuthUser>) -> Response {
    let Ok(c) = st.db.get().await else { return db_err() };
    match c.query(&format!("SELECT {COLS} FROM suppliers p ORDER BY CASE p.status WHEN 'blocked' THEN 1 ELSE 0 END, p.code"), &[]).await {
        Ok(rows) => Json(Value::Array(rows.iter().map(|r| { let mut x = row_json(r); x["can"] = can_flags(&u, &x); x["gaps"] = json!(gaps(&x)); x }).collect())).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, &format!("อ่านทะเบียนไม่ได้: {e}")),
    }
}

pub async fn get(Path(id): Path<i64>, State(st): State<S>, Extension(u): Extension<AuthUser>) -> Response { detail_resp(&st, &u, id).await }

#[derive(Deserialize, Default)]
#[allow(non_snake_case)]
pub struct Fields {
    name: Option<String>, vendorCode: Option<String>, category: Option<String>, products: Option<String>, contact: Option<String>,
    phone: Option<String>, email: Option<String>, ownerCode: Option<String>, evalMonths: Option<i64>, nextEval: Option<String>,
}

async fn apply(c: &impl deadpool_postgres::GenericClient, p: &mut Value, f: Fields) -> Result<(), Response> {
    if let Some(t) = f.name { let t = clip(&t, 200); if t.is_empty() { return Err(bad("ต้องระบุชื่อผู้ขาย")); } p["name"] = json!(t); }
    for (k, v, n) in [("vendorCode", f.vendorCode, 40), ("products", f.products, 1000), ("contact", f.contact, 200), ("phone", f.phone, 60), ("email", f.email, 200)] {
        if let Some(x) = v { p[k] = json!(clip(&x, n)); }
    }
    if let Some(x) = f.category { if !CATEGORIES.contains(&x.as_str()) { return Err(bad("ประเภทผู้ขายไม่ถูกต้อง")); } p["category"] = json!(x); }
    if let Some(n) = f.evalMonths { if !(1..=36).contains(&n) { return Err(bad("รอบประเมินต้องอยู่ระหว่าง 1–36 เดือน")); } p["evalMonths"] = json!(n); }
    if let Some(d) = f.nextEval { let d = clip(&d, 10); if !valid_date(&d) { return Err(bad("รูปแบบวันที่ไม่ถูกต้อง (YYYY-MM-DD)")); } p["nextEval"] = json!(d); }
    if let Some(code) = f.ownerCode {
        let code = clip(&code, 20);
        let name: String = if code.is_empty() { String::new() } else {
            c.query_opt("SELECT name_th FROM employees WHERE emp_code=$1 AND status='active'", &[&code]).await.ok().flatten()
                .map(|x| x.get(0)).ok_or_else(|| bad("ไม่พบผู้รับผิดชอบในรายชื่อพนักงาน"))?
        };
        p["ownerCode"] = json!(code); p["ownerName"] = json!(name);
    }
    Ok(())
}

#[derive(Deserialize)]
pub struct CreateReq {
    #[serde(flatten)]
    fields: Fields,
    /// สถานะเริ่มต้น — อนุมัติ/ระงับ ต้องเป็นผู้อนุมัติขึ้นไป (ผู้ขายเดิมที่อนุมัติอยู่แล้ว) · อื่น ๆ = มีเงื่อนไขจนกว่าจะประเมินครั้งแรก
    status: Option<String>,
    reason: Option<String>,
}

/// ขึ้นทะเบียนผู้ขาย (ผู้จัดทำขึ้นไป) — รหัส SUP-xxx ต่อเนื่อง
pub async fn create(State(st): State<S>, Extension(u): Extension<AuthUser>, Json(b): Json<CreateReq>) -> Response {
    if u.role == "viewer" { return forbid("บัญชีผู้อ่านอย่างเดียวบันทึกไม่ได้"); }
    let Ok(c) = st.db.get().await else { return db_err() };
    if b.fields.name.as_deref().map(|t| t.trim().is_empty()).unwrap_or(true) { return bad("ต้องระบุชื่อผู้ขาย"); }
    let mut p = json!({ "category": "material", "evalMonths": 12, "nextEval": "" });
    if let Err(x) = apply(&c, &mut p, b.fields).await { return x; }
    let g = gaps(&p);
    if !g.is_empty() { return bad(&format!("ยังขาด: {}", g.join(", "))); }
    let status = b.status.unwrap_or_else(|| "conditional".into());
    if !STATUSES.contains(&status.as_str()) { return bad("สถานะไม่ถูกต้อง"); }
    if status != "conditional" && !eform::can_approve(&u.role) { return forbid("กำหนดสถานะอนุมัติ/ระงับได้เฉพาะผู้อนุมัติขึ้นไป — ผู้ขายใหม่เริ่มที่ มีเงื่อนไข จนกว่าจะประเมิน"); }
    let reason = clip(&b.reason.unwrap_or_default(), 2000);
    if status != "conditional" && reason.is_empty() { return bad("กรุณาระบุเหตุผล/หลักฐานการอนุมัติหรือระงับ"); }
    let today = capa::today();
    if s(&p, "nextEval").is_empty() && status != "blocked" {
        p["nextEval"] = json!(if status == "approved" { crate::calibration::add_months(&today, i(&p, "evalMonths")) } else { plus_days(&today, FIRST_EVAL_DAYS) });
    }
    if status == "blocked" { p["nextEval"] = json!(""); }
    let n: i64 = match c.query_one(
        "INSERT INTO form_seq(form_code,year_be,last_seq) VALUES('SUP',0,1) ON CONFLICT(form_code,year_be) DO UPDATE SET last_seq=form_seq.last_seq+1 RETURNING last_seq", &[],
    ).await { Ok(x) => x.get(0), Err(x) => return err(StatusCode::INTERNAL_SERVER_ERROR, &x.to_string()) };
    let code = format!("SUP-{n:03}");
    let now = now_iso();
    let res = c.query_one(
        "INSERT INTO suppliers(code,name,vendor_code,category,products,contact,phone,email,owner_code,owner_name,eval_months,status,status_reason,next_eval,
           created_by,created_name,created_at,updated_at) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$17) RETURNING id",
        &[&code, &s(&p, "name"), &s(&p, "vendorCode"), &s(&p, "category"), &s(&p, "products"), &s(&p, "contact"), &s(&p, "phone"), &s(&p, "email"),
          &s(&p, "ownerCode"), &s(&p, "ownerName"), &i(&p, "evalMonths"), &status, &if reason.is_empty() { "ผู้ขายใหม่ — รอประเมินครั้งแรก".to_string() } else { reason.clone() },
          &s(&p, "nextEval"), &u.emp_code, &u.name, &now],
    ).await;
    let id: i64 = match res { Ok(x) => x.get(0), Err(x) => return err(StatusCode::INTERNAL_SERVER_ERROR, &format!("บันทึกไม่สำเร็จ: {x}")) };
    event(&c, id, "create", &u, &format!("{} · {}", s(&p, "name"), status_th(&status))).await;
    let _ = eform::audit(&c, &u, "supplier_create", "supplier", &code, &format!("{} · {}", s(&p, "name"), status_th(&status))).await;
    detail_resp(&st, &u, id).await
}

#[derive(Deserialize)]
pub struct UpdateReq { version: i64, fields: Fields }

pub async fn update(Path(id): Path<i64>, State(st): State<S>, Extension(u): Extension<AuthUser>, Json(b): Json<UpdateReq>) -> Response {
    let Ok(c) = st.db.get().await else { return db_err() };
    let Some(mut p) = load(&c, id).await else { return err(StatusCode::NOT_FOUND, "ไม่พบผู้ขายนี้") };
    if can_flags(&u, &p)["edit"] != json!(true) { return forbid("แก้ไขได้เฉพาะผู้บันทึก ผู้รับผิดชอบ หรือผู้อนุมัติขึ้นไป"); }
    let before = p.clone();
    if let Err(x) = apply(&c, &mut p, b.fields).await { return x; }
    let g = gaps(&p);
    if !g.is_empty() { return bad(&format!("ยังขาด: {}", g.join(", "))); }
    if s(&p, "status") == "blocked" { p["nextEval"] = json!(""); }
    let n = c.execute(
        "UPDATE suppliers SET version=version+1,name=$3,vendor_code=$4,category=$5,products=$6,contact=$7,phone=$8,email=$9,owner_code=$10,owner_name=$11,
           eval_months=$12,next_eval=$13,updated_at=$14 WHERE id=$1 AND version=$2",
        &[&id, &b.version, &s(&p, "name"), &s(&p, "vendorCode"), &s(&p, "category"), &s(&p, "products"), &s(&p, "contact"), &s(&p, "phone"), &s(&p, "email"),
          &s(&p, "ownerCode"), &s(&p, "ownerName"), &i(&p, "evalMonths"), &s(&p, "nextEval"), &now_iso()],
    ).await.unwrap_or(0);
    if n == 0 { return stale(); }
    let changed: Vec<String> = ["name", "vendorCode", "category", "products", "contact", "phone", "email", "ownerCode", "evalMonths", "nextEval"]
        .into_iter().filter(|k| before[*k] != p[*k])
        .map(|k| if k == "nextEval" { format!("nextEval {}→{}", s(&before, k), s(&p, k)) } else { k.to_string() }).collect();
    if !changed.is_empty() {
        event(&c, id, "edit", &u, &changed.join(", ")).await;
        let _ = eform::audit(&c, &u, "supplier_edit", "supplier", &s(&p, "code"), &changed.join(", ")).await;
    }
    detail_resp(&st, &u, id).await
}

#[derive(Deserialize)]
#[allow(non_snake_case)]
pub struct EvalReq {
    version: i64, period: String, evalDate: String, quality: i64, delivery: i64, price: i64, service: i64,
    note: Option<String>, status: Option<String>, nextEval: Option<String>,
}

/// บันทึกผลประเมิน — คำนวณคะแนน/เกรด/สถานะที่แนะนำฝั่ง server · ผู้อนุมัติขึ้นไปกำหนดสถานะต่างจากที่แนะนำได้พร้อมเหตุผล
pub async fn evaluate(Path(id): Path<i64>, State(st): State<S>, Extension(u): Extension<AuthUser>, Json(b): Json<EvalReq>) -> Response {
    let Ok(c) = st.db.get().await else { return db_err() };
    let Some(p) = load(&c, id).await else { return err(StatusCode::NOT_FOUND, "ไม่พบผู้ขายนี้") };
    let can = can_flags(&u, &p);
    if can["evaluate"] != json!(true) { return forbid("ประเมินได้เฉพาะผู้รับผิดชอบประเมิน หรือผู้อนุมัติขึ้นไป"); }
    let items = [b.quality, b.delivery, b.price, b.service];
    if items.iter().any(|x| !(1..=5).contains(x)) { return bad("คะแนนแต่ละหัวข้อต้องอยู่ระหว่าง 1–5"); }
    let today = capa::today();
    let date = clip(&b.evalDate, 10);
    if parse(&date).is_none() || date > today { return bad("วันที่ประเมินไม่ถูกต้อง (ต้องไม่เกินวันนี้)"); }
    let period = clip(&b.period, 40);
    if period.is_empty() { return bad("กรุณาระบุรอบ/ช่วงที่ประเมิน (เช่น ม.ค.–มิ.ย. 2569)"); }
    let min_item = *items.iter().min().unwrap();
    let sc = score(b.quality, b.delivery, b.price, b.service);
    let gr = grade(sc);
    let rec = recommended(gr, min_item);
    let status = b.status.filter(|x| !x.is_empty()).unwrap_or_else(|| rec.to_string());
    if !STATUSES.contains(&status.as_str()) { return bad("สถานะไม่ถูกต้อง"); }
    let note = clip(&b.note.unwrap_or_default(), 3000);
    if status != rec {
        if can["override"] != json!(true) { return forbid(&format!("สถานะตามผลประเมินคือ {} — กำหนดต่างจากนี้ได้เฉพาะผู้อนุมัติขึ้นไป", status_th(rec))); }
        if note.is_empty() { return bad("กำหนดสถานะต่างจากที่แนะนำ ต้องระบุเหตุผลในหมายเหตุ"); }
    }
    if poor(gr, min_item) && note.is_empty() { return bad("ผลประเมินต่ำ — กรุณาบันทึกข้อมูลประกอบ/ปัญหาที่พบในหมายเหตุ"); }
    let next = match b.nextEval.map(|d| clip(&d, 10)).filter(|d| !d.is_empty()) {
        Some(_) if status == "blocked" => String::new(),
        Some(d) => { if parse(&d).is_none() || d <= date { return bad("กำหนดประเมินครั้งถัดไปต้องหลังวันที่ประเมิน"); } d }
        None => next_eval_for(&status, &date, i(&p, "evalMonths")),
    };
    let year = eform::be_year_now();
    let seq: i64 = match c.query_one(
        "INSERT INTO form_seq(form_code,year_be,last_seq) VALUES('SEV',$1,1) ON CONFLICT(form_code,year_be) DO UPDATE SET last_seq=form_seq.last_seq+1 RETURNING last_seq", &[&year],
    ).await { Ok(x) => x.get(0), Err(x) => return err(StatusCode::INTERNAL_SERVER_ERROR, &x.to_string()) };
    let ref_no = format!("SEV-{year}-{seq:03}");
    let now = now_iso();
    // ประเมินย้อนหลัง (ก่อนครั้งล่าสุด) เก็บเป็นประวัติ ไม่เปลี่ยนสถานะปัจจุบัน
    let latest = s(&p, "lastEval").is_empty() || date >= s(&p, "lastEval");
    if latest {
        let reason = format!("ผลประเมิน {ref_no} เกรด {gr} ({sc}%)");
        let n = c.execute(
            "UPDATE suppliers SET version=version+1,status=$3,status_reason=$4,next_eval=$5,last_eval=$6,last_score=$7,last_grade=$8,updated_at=$9 WHERE id=$1 AND version=$2",
            &[&id, &b.version, &status, &reason, &next, &date, &sc, &gr.to_string(), &now],
        ).await.unwrap_or(0);
        if n == 0 { return stale(); }
    }
    let res = c.execute(
        "INSERT INTO supplier_evals(supplier_id,ref_no,period,eval_date,quality,delivery,price,service,score,grade,status_before,status_after,next_eval,note,
           evaluator_code,evaluator_name,created_at) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17)",
        &[&id, &ref_no, &period, &date, &b.quality, &b.delivery, &b.price, &b.service, &sc, &gr.to_string(), &s(&p, "status"),
          &if latest { status.clone() } else { s(&p, "status") }, &if latest { next.clone() } else { String::new() }, &note, &u.emp_code, &u.name, &now],
    ).await;
    if let Err(x) = res { return err(StatusCode::INTERNAL_SERVER_ERROR, &format!("บันทึกผลประเมินไม่สำเร็จ: {x}")); }
    let summary = format!("{ref_no} {period} · คุณภาพ {} ส่งมอบ {} ราคา {} บริการ {} = {sc}% เกรด {gr}{}", b.quality, b.delivery, b.price, b.service,
        if latest { format!(" · {} → {}", status_th(&s(&p, "status")), status_th(&status)) } else { " · ประเมินย้อนหลัง".into() });
    event(&c, id, "evaluate", &u, &summary).await;
    let _ = eform::audit(&c, &u, "supplier_evaluate", "supplier", &s(&p, "code"), &summary).await;
    detail_resp(&st, &u, id).await
}

#[derive(Deserialize)]
pub struct StatusReq { version: i64, status: String, reason: Option<String> }

/// เปลี่ยนสถานะโดยตรง (ผู้อนุมัติขึ้นไป พร้อมเหตุผล) — เช่น ระงับทันทีเมื่อเกิดปัญหาร้ายแรง หรืออนุมัติจากการตรวจสถานที่
pub async fn set_status(Path(id): Path<i64>, State(st): State<S>, Extension(u): Extension<AuthUser>, Json(b): Json<StatusReq>) -> Response {
    let Ok(c) = st.db.get().await else { return db_err() };
    let Some(p) = load(&c, id).await else { return err(StatusCode::NOT_FOUND, "ไม่พบผู้ขายนี้") };
    if can_flags(&u, &p)["setStatus"] != json!(true) { return forbid("เปลี่ยนสถานะได้เฉพาะผู้อนุมัติขึ้นไป"); }
    if !STATUSES.contains(&b.status.as_str()) { return bad("สถานะไม่ถูกต้อง"); }
    if b.status == s(&p, "status") { return bad("สถานะเดิมอยู่แล้ว"); }
    let reason = clip(&b.reason.unwrap_or_default(), 2000);
    if reason.is_empty() { return bad("กรุณาระบุเหตุผล"); }
    let today = capa::today();
    // ปรับกำหนดประเมิน: ระงับ = ไม่มี · ยกระดับจากระงับ/มีเงื่อนไข = เริ่มรอบใหม่ · ลดระดับเป็นมีเงื่อนไข = ไม่เกิน 3 เดือน
    let next = match b.status.as_str() {
        "blocked" => String::new(),
        st if rank(st) > rank(&s(&p, "status")) || s(&p, "nextEval").is_empty() => next_eval_for(st, &today, i(&p, "evalMonths")),
        st => { let cap = next_eval_for(st, &today, i(&p, "evalMonths")); let cur = s(&p, "nextEval"); if cur < cap { cur } else { cap } }
    };
    let n = c.execute("UPDATE suppliers SET version=version+1,status=$3,status_reason=$4,next_eval=$5,updated_at=$6 WHERE id=$1 AND version=$2",
        &[&id, &b.version, &b.status, &reason, &next, &now_iso()]).await.unwrap_or(0);
    if n == 0 { return stale(); }
    let note = format!("{} → {} · {reason}", status_th(&s(&p, "status")), status_th(&b.status));
    event(&c, id, "status", &u, &note).await;
    let _ = eform::audit(&c, &u, "supplier_status", "supplier", &s(&p, "code"), &note).await;
    detail_resp(&st, &u, id).await
}

/// เปิด CAR จากผลประเมินต่ำ (แหล่งที่มา "ผู้ขาย") — ผู้รับผิดชอบประเมินเป็นผู้รับผิดชอบใบ
pub async fn raise_car(Path((id, vid)): Path<(i64, i64)>, State(st): State<S>, Extension(u): Extension<AuthUser>) -> Response {
    let Ok(c) = st.db.get().await else { return db_err() };
    let Some(p) = load(&c, id).await else { return err(StatusCode::NOT_FOUND, "ไม่พบผู้ขายนี้") };
    if can_flags(&u, &p)["car"] != json!(true) { return forbid("เปิด CAR ได้เฉพาะผู้รับผิดชอบประเมิน หรือผู้อนุมัติขึ้นไป"); }
    let Some(v) = c.query_opt(&format!("SELECT {VCOLS} FROM supplier_evals WHERE id=$1 AND supplier_id=$2"), &[&vid, &id]).await.ok().flatten().map(|r| eval_json(&r))
        else { return err(StatusCode::NOT_FOUND, "ไม่พบผลประเมินนี้") };
    if v["poor"] != json!(true) { return bad("เปิด CAR ได้เฉพาะผลประเมินต่ำ (เกรด C/D หรือมีหัวข้อได้ 1 คะแนน)"); }
    if !s(&v, "carRef").is_empty() { return bad(&format!("ผลประเมินนี้เปิด {} ไว้แล้ว", s(&v, "carRef"))); }
    let f = capa::Fields {
        kind: Some("CAR".into()),
        title: Some(format!("ผลประเมินผู้ขายต่ำ: {} {} (เกรด {})", s(&p, "code"), s(&p, "name"), s(&v, "grade"))),
        detail: Some(format!("{} รอบ {} · คุณภาพ {}/5 · ส่งมอบ {}/5 · ราคา {}/5 · บริการ {}/5 = {}% · สินค้า/บริการ: {}{}", s(&v, "refNo"), s(&v, "period"),
            v["quality"], v["delivery"], v["price"], v["service"], v["score"], s(&p, "products"),
            if s(&v, "note").is_empty() { String::new() } else { format!(" · ปัญหาที่พบ: {}", s(&v, "note")) })),
        dept: Some("PU".into()),
        source: Some("supplier".into()),
        assigneeCode: Some(s(&p, "ownerCode")),
        priority: Some(if s(&v, "grade") == "D" { "high".into() } else { "medium".into() }),
        linkedRef: Some(s(&v, "refNo")),
        ..Default::default()
    };
    let (_, ref_no) = match capa::open_action(&st, &c, &u, f).await { Ok(x) => x, Err(x) => return x };
    let _ = c.execute("UPDATE supplier_evals SET car_ref=$2 WHERE id=$1", &[&vid, &ref_no]).await;
    let _ = c.execute("UPDATE suppliers SET version=version+1,updated_at=$2 WHERE id=$1", &[&id, &now_iso()]).await;
    event(&c, id, "car", &u, &format!("{ref_no} จาก {}", s(&v, "refNo"))).await;
    let _ = eform::audit(&c, &u, "supplier_car", "supplier", &s(&p, "code"), &format!("{} → {ref_no}", s(&v, "refNo"))).await;
    detail_resp(&st, &u, id).await
}

// ---------- ใช้ร่วมกับโมดูลอื่น ----------

/// ผู้ขายทั้งหมดในทะเบียน (หน้าเตรียม Audit · ทบทวนฝ่ายบริหาร · แจ้งเตือน)
pub async fn all(c: &impl deadpool_postgres::GenericClient) -> Vec<Value> {
    c.query(&format!("SELECT {COLS} FROM suppliers p ORDER BY p.code"), &[]).await
        .map(|rows| rows.iter().map(|r| { let mut x = row_json(r); x["gaps"] = json!(gaps(&x)); x }).collect()).unwrap_or_default()
}

/// ผลประเมินตั้งแต่วันที่กำหนด (ทบทวนฝ่ายบริหาร)
pub async fn evals_since(c: &impl deadpool_postgres::GenericClient, since: &str) -> Vec<Value> {
    c.query(&format!("SELECT {VCOLS} FROM supplier_evals WHERE eval_date>=$1 ORDER BY eval_date DESC, id DESC"), &[&since.to_string()]).await
        .map(|rows| rows.iter().map(eval_json).collect()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scoring() {
        assert_eq!(score(5, 5, 5, 5), 100);
        assert_eq!(score(1, 1, 1, 1), 20);
        assert_eq!(score(4, 4, 3, 3), 74); // (160+120+45+45)/5
        assert_eq!(score(5, 3, 4, 4), 82); // (200+90+60+60)/5
        assert_eq!(WEIGHTS.iter().sum::<i64>(), 100);
    }
    #[test]
    fn grades_and_status() {
        assert_eq!(grade(85), "A");
        assert_eq!(grade(84), "B");
        assert_eq!(grade(70), "B");
        assert_eq!(grade(69), "C");
        assert_eq!(grade(50), "C");
        assert_eq!(grade(49), "D");
        assert_eq!(recommended("A", 4), "approved");
        assert_eq!(recommended("B", 1), "conditional");
        assert_eq!(recommended("C", 3), "conditional");
        assert_eq!(recommended("D", 2), "blocked");
        assert!(poor("C", 3) && poor("A", 1) && !poor("B", 2));
    }
    #[test]
    fn next_due() {
        assert_eq!(next_eval_for("approved", "2026-10-16", 12), "2027-10-16");
        assert_eq!(next_eval_for("conditional", "2026-10-16", 12), "2027-01-16");
        assert_eq!(next_eval_for("blocked", "2026-10-16", 12), "");
    }
}
