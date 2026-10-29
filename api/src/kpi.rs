// วัตถุประสงค์คุณภาพและตัวชี้วัด (ISO 9001:2015 ข้อ 6.2, 9.1.1, 9.1.3)
//   วัตถุประสงค์ที่วัดได้: เป้าหมาย (≥ / ≤) หน่วย ความถี่ ผู้รับผิดชอบ และแผนบรรลุ (6.2.2: ทำอะไร ทรัพยากร ใคร เมื่อไร ประเมินอย่างไร)
//   ผลรายงวด (เดือน/ไตรมาส/ปี) บันทึกโดยผู้รับผิดชอบหรือผู้อนุมัติขึ้นไป · ไม่ถึงเป้า → เปิด CAR ได้จากหน้าเดียวกัน
//   ข้อมูลนี้ป้อนให้หน้าเตรียม Audit (ความครบของผล) และการทบทวนฝ่ายบริหาร (9.3.2 c2) อัตโนมัติ
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
CREATE TABLE IF NOT EXISTS kpi_objectives(
  id BIGSERIAL PRIMARY KEY,
  code TEXT NOT NULL UNIQUE,
  version BIGINT NOT NULL DEFAULT 1,
  title TEXT NOT NULL,
  dept TEXT NOT NULL DEFAULT '',
  process TEXT NOT NULL DEFAULT '',
  owner_code TEXT NOT NULL DEFAULT '',
  owner_name TEXT NOT NULL DEFAULT '',
  unit TEXT NOT NULL DEFAULT '',
  direction TEXT NOT NULL DEFAULT 'gte',
  target DOUBLE PRECISION NOT NULL DEFAULT 0,
  frequency TEXT NOT NULL DEFAULT 'monthly',
  start_period TEXT NOT NULL DEFAULT '',
  formula TEXT NOT NULL DEFAULT '',
  plan TEXT NOT NULL DEFAULT '',
  resources TEXT NOT NULL DEFAULT '',
  evaluation TEXT NOT NULL DEFAULT '',
  status TEXT NOT NULL DEFAULT 'active',
  retire_reason TEXT NOT NULL DEFAULT '',
  created_by TEXT NOT NULL,
  created_name TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS kpi_results(
  id BIGSERIAL PRIMARY KEY,
  kpi_id BIGINT NOT NULL,
  period TEXT NOT NULL,
  actual DOUBLE PRECISION NOT NULL,
  target DOUBLE PRECISION NOT NULL,
  met BOOLEAN NOT NULL,
  note TEXT NOT NULL DEFAULT '',
  evidence TEXT NOT NULL DEFAULT '',
  car_ref TEXT NOT NULL DEFAULT '',
  recorded_code TEXT NOT NULL,
  recorded_name TEXT NOT NULL,
  recorded_at TEXT NOT NULL,
  UNIQUE(kpi_id, period));
CREATE INDEX IF NOT EXISTS idx_kr_kpi ON kpi_results(kpi_id);
";

const OCOLS: &str = "id,code,version,title,dept,process,owner_code,owner_name,unit,direction,target,frequency,start_period,formula,plan,resources,evaluation,status,retire_reason,created_by,created_name,created_at,updated_at";
const RCOLS: &str = "kpi_id,period,actual,target,met,note,evidence,car_ref,recorded_code,recorded_name,recorded_at";
const FREQS: [&str; 3] = ["monthly", "quarterly", "yearly"];
/// ผลของงวดที่จบแล้วต้องบันทึกภายในกี่วันหลังสิ้นงวด ก่อนนับว่าค้าง
pub const GRACE_DAYS: i64 = 15;

fn db_err() -> Response { err(StatusCode::INTERNAL_SERVER_ERROR, "เชื่อมต่อฐานข้อมูลไม่ได้") }
fn bad(m: &str) -> Response { err(StatusCode::BAD_REQUEST, m) }
fn forbid(m: &str) -> Response { err(StatusCode::FORBIDDEN, m) }
fn s(v: &Value, k: &str) -> String { v[k].as_str().unwrap_or("").to_string() }
fn clip(t: &str, n: usize) -> String { t.trim().chars().take(n).collect() }

fn today() -> chrono::NaiveDate {
    let tz = chrono::FixedOffset::east_opt(7 * 3600).unwrap();
    chrono::Utc::now().with_timezone(&tz).date_naive()
}

// ---------- งวด ----------
//   monthly "2026-03" · quarterly "2026-Q1" · yearly "2026" (ปี ค.ศ. — หน้าเว็บแสดงเป็น พ.ศ.)

use chrono::Datelike;

fn period_of(freq: &str, d: chrono::NaiveDate) -> String {
    match freq {
        "quarterly" => format!("{}-Q{}", d.year(), (d.month() - 1) / 3 + 1),
        "yearly" => format!("{}", d.year()),
        _ => format!("{}-{:02}", d.year(), d.month()),
    }
}

/// วันสุดท้ายของงวด (None = รูปแบบงวดไม่ถูกต้อง)
fn period_end(freq: &str, p: &str) -> Option<chrono::NaiveDate> {
    let first_of = |y: i32, m: u32| chrono::NaiveDate::from_ymd_opt(y, m, 1);
    let (y, last_month) = match freq {
        "quarterly" => {
            let (y, q) = p.split_once("-Q")?;
            let q: u32 = q.parse().ok().filter(|q| (1..=4).contains(q))?;
            (y.parse::<i32>().ok()?, q * 3)
        }
        "yearly" => (p.parse::<i32>().ok().filter(|_| p.len() == 4)?, 12),
        _ => {
            let (y, m) = p.split_once('-')?;
            (y.parse::<i32>().ok()?, m.parse::<u32>().ok().filter(|m| (1..=12).contains(m) && p.len() == 7)?)
        }
    };
    if !(2000..=2200).contains(&y) { return None; }
    let next = if last_month == 12 { first_of(y + 1, 1)? } else { first_of(y, last_month + 1)? };
    Some(next - chrono::Duration::days(1))
}

/// งวดก่อนหน้า
fn prev_period(freq: &str, p: &str) -> Option<String> {
    let end = period_end(freq, p)?;
    let start = match freq {
        "quarterly" => chrono::NaiveDate::from_ymd_opt(end.year(), end.month() - 2, 1)?,
        "yearly" => chrono::NaiveDate::from_ymd_opt(end.year(), 1, 1)?,
        _ => chrono::NaiveDate::from_ymd_opt(end.year(), end.month(), 1)?,
    };
    Some(period_of(freq, start - chrono::Duration::days(1)))
}

/// งวดล่าสุดที่จบแล้ว (งวดที่ควรมีผล)
fn last_closed_period(freq: &str, now: chrono::NaiveDate) -> String {
    prev_period(freq, &period_of(freq, now)).unwrap_or_default()
}

fn met(direction: &str, actual: f64, target: f64) -> bool {
    if direction == "lte" { actual <= target } else { actual >= target }
}

// ---------- JSON ----------

fn obj_json(r: &tokio_postgres::Row) -> Value {
    let g = |i: usize| r.get::<_, String>(i);
    json!({
        "id": r.get::<_, i64>(0), "code": g(1), "version": r.get::<_, i64>(2), "title": g(3), "dept": g(4), "process": g(5),
        "ownerCode": g(6), "ownerName": g(7), "unit": g(8), "direction": g(9), "target": r.get::<_, f64>(10), "frequency": g(11),
        "startPeriod": g(12), "formula": g(13), "plan": g(14), "resources": g(15), "evaluation": g(16), "status": g(17),
        "retireReason": g(18), "createdBy": g(19), "createdName": g(20), "createdAt": g(21), "updatedAt": g(22),
    })
}
fn res_json(r: &tokio_postgres::Row) -> Value {
    let g = |i: usize| r.get::<_, String>(i);
    json!({
        "period": g(1), "actual": r.get::<_, f64>(2), "target": r.get::<_, f64>(3), "met": r.get::<_, bool>(4),
        "note": g(5), "evidence": g(6), "carRef": g(7), "recordedCode": g(8), "recordedName": g(9), "recordedAt": g(10),
    })
}

/// สถานะ ณ วันนี้: งวดที่ควรมีผล, ค้างบันทึกหรือไม่, อัตราบรรลุ, แนวโน้ม
fn enrich(o: &mut Value, results: Vec<Value>, now: chrono::NaiveDate) {
    let freq = s(o, "frequency");
    let expected = last_closed_period(&freq, now);
    let start = s(o, "startPeriod");
    // งวดที่ควรมีผลต้องไม่ก่อนงวดเริ่ม · ค้างเมื่อพ้นสิ้นงวด + GRACE_DAYS แล้วยังไม่บันทึก
    let due = period_end(&freq, &expected).map(|e| e + chrono::Duration::days(GRACE_DAYS));
    let applies = s(o, "status") == "active" && !expected.is_empty() && (start.is_empty() || expected >= start);
    let has_expected = results.iter().any(|r| r["period"] == json!(expected));
    let missing = applies && !has_expected && due.map(|d| now > d).unwrap_or(false);
    let pending = applies && !has_expected && !missing;
    let n = results.len();
    let n_met = results.iter().filter(|r| r["met"] == json!(true)).count();
    let latest = results.last().cloned();
    let previous = if n >= 2 { Some(results[n - 2].clone()) } else { None };
    let lte = s(o, "direction") == "lte";
    let trend = match (&latest, &previous) {
        (Some(l), Some(p)) => {
            let (a, b) = (l["actual"].as_f64().unwrap_or(0.0), p["actual"].as_f64().unwrap_or(0.0));
            if (a - b).abs() < f64::EPSILON { "flat" } else if (a > b) != lte { "better" } else { "worse" }
        }
        _ => "",
    };
    let off_no_car: Vec<String> = results.iter().filter(|r| r["met"] == json!(false) && s(r, "carRef").is_empty()).map(|r| s(r, "period")).collect();
    o["expectedPeriod"] = json!(expected);
    o["dueDate"] = json!(due.map(|d| d.format("%Y-%m-%d").to_string()).unwrap_or_default());
    o["missing"] = json!(missing);
    o["pending"] = json!(pending);
    o["achieved"] = json!(n_met);
    o["measured"] = json!(n);
    o["rate"] = json!(if n == 0 { Value::Null } else { json!(((n_met as f64) / (n as f64) * 100.0).round() as i64) });
    o["latest"] = latest.unwrap_or(Value::Null);
    o["previous"] = previous.unwrap_or(Value::Null);
    o["trend"] = json!(trend);
    o["offTargetNoCar"] = json!(off_no_car);
    o["results"] = json!(results);
}

async fn load_all(c: &impl deadpool_postgres::GenericClient, only: Option<i64>) -> Vec<Value> {
    let rows = match only {
        Some(id) => c.query(&format!("SELECT {OCOLS} FROM kpi_objectives WHERE id=$1"), &[&id]).await,
        None => c.query(&format!("SELECT {OCOLS} FROM kpi_objectives ORDER BY status, dept, code"), &[]).await,
    }.unwrap_or_default();
    let mut objs: Vec<Value> = rows.iter().map(obj_json).collect();
    let res = match only {
        Some(id) => c.query(&format!("SELECT {RCOLS} FROM kpi_results WHERE kpi_id=$1 ORDER BY period"), &[&id]).await,
        None => c.query(&format!("SELECT {RCOLS} FROM kpi_results ORDER BY period"), &[]).await,
    }.unwrap_or_default();
    let now = today();
    for o in objs.iter_mut() {
        let id = o["id"].as_i64().unwrap_or(0);
        let mine: Vec<Value> = res.iter().filter(|r| r.get::<_, i64>(0) == id).map(res_json).collect();
        enrich(o, mine, now);
    }
    objs
}

fn can_flags(u: &AuthUser, o: &Value) -> Value {
    let approver = eform::can_approve(&u.role);
    let owner = s(o, "ownerCode") == u.emp_code;
    let active = s(o, "status") == "active";
    json!({
        "edit": approver && active,
        "record": active && u.role != "viewer" && (owner || approver),
        "car": u.role != "viewer" && (owner || approver),
        "retire": approver && active,
    })
}

// ---------- handlers ----------

pub async fn list(State(st): State<S>, Extension(u): Extension<AuthUser>) -> Response {
    let Ok(c) = st.db.get().await else { return db_err() };
    let mut objs = load_all(&c, None).await;
    for o in objs.iter_mut() { o["can"] = can_flags(&u, o); }
    Json(json!({ "objectives": objs, "canCreate": eform::can_approve(&u.role), "graceDays": GRACE_DAYS })).into_response()
}

async fn one(c: &impl deadpool_postgres::GenericClient, u: &AuthUser, id: i64) -> Response {
    let Some(mut o) = load_all(c, Some(id)).await.into_iter().next() else { return err(StatusCode::NOT_FOUND, "ไม่พบวัตถุประสงค์นี้") };
    o["can"] = can_flags(u, &o);
    Json(json!({ "ok": true, "objective": o })).into_response()
}

#[derive(Deserialize, Default)]
#[allow(non_snake_case)]
pub struct Fields {
    title: Option<String>, dept: Option<String>, process: Option<String>, ownerCode: Option<String>, unit: Option<String>,
    direction: Option<String>, target: Option<f64>, frequency: Option<String>, startPeriod: Option<String>, formula: Option<String>,
    plan: Option<String>, resources: Option<String>, evaluation: Option<String>,
}

async fn apply(c: &impl deadpool_postgres::GenericClient, o: &mut Value, f: Fields) -> Result<(), Response> {
    if let Some(t) = f.title { let t = clip(&t, 300); if t.is_empty() { return Err(bad("ต้องระบุวัตถุประสงค์/ตัวชี้วัด")); } o["title"] = json!(t); }
    if let Some(x) = f.dept { o["dept"] = json!(clip(&x, 20)); }
    if let Some(x) = f.process { o["process"] = json!(clip(&x, 200)); }
    if let Some(code) = f.ownerCode {
        let code = clip(&code, 20);
        let name: String = if code.is_empty() { String::new() } else {
            c.query_opt("SELECT name_th FROM employees WHERE emp_code=$1 AND status='active'", &[&code]).await.ok().flatten()
                .map(|r| r.get(0)).ok_or_else(|| bad("ไม่พบผู้รับผิดชอบในรายชื่อพนักงาน"))?
        };
        o["ownerCode"] = json!(code); o["ownerName"] = json!(name);
    }
    if let Some(x) = f.unit { o["unit"] = json!(clip(&x, 20)); }
    if let Some(x) = f.direction { o["direction"] = json!(if x == "lte" { "lte" } else { "gte" }); }
    if let Some(t) = f.target { if !t.is_finite() { return Err(bad("เป้าหมายต้องเป็นตัวเลข")); } o["target"] = json!(t); }
    if let Some(x) = f.frequency { if !FREQS.contains(&x.as_str()) { return Err(bad("ความถี่ไม่ถูกต้อง")); } o["frequency"] = json!(x); }
    if let Some(x) = f.startPeriod {
        let x = clip(&x, 10);
        if !x.is_empty() && period_end(&s(o, "frequency"), &x).is_none() { return Err(bad("งวดเริ่มต้นไม่ตรงกับความถี่ (เช่น 2026-01, 2026-Q1, 2026)")); }
        o["startPeriod"] = json!(x);
    }
    for (k, v, n) in [("formula", f.formula, 1000), ("plan", f.plan, 5000), ("resources", f.resources, 2000), ("evaluation", f.evaluation, 2000)] {
        if let Some(x) = v { o[k] = json!(clip(&x, n)); }
    }
    Ok(())
}

fn gaps(o: &Value) -> Vec<&'static str> {
    let mut g = vec![];
    if s(o, "ownerCode").is_empty() { g.push("ผู้รับผิดชอบ"); }
    if s(o, "plan").is_empty() { g.push("แผนการบรรลุ (6.2.2)"); }
    g
}

/// สร้างวัตถุประสงค์ใหม่ (ผู้อนุมัติขึ้นไป — ฝ่ายบริหาร/QMR เป็นผู้กำหนด)
pub async fn create(State(st): State<S>, Extension(u): Extension<AuthUser>, Json(f): Json<Fields>) -> Response {
    if !eform::can_approve(&u.role) { return forbid("กำหนดวัตถุประสงค์คุณภาพได้เฉพาะผู้อนุมัติ หรือผู้ดูแลระบบ"); }
    let Ok(c) = st.db.get().await else { return db_err() };
    let mut o = json!({ "direction": "gte", "frequency": "monthly", "target": 0.0, "status": "active" });
    if f.title.as_deref().map(|t| t.trim().is_empty()).unwrap_or(true) { return bad("ต้องระบุวัตถุประสงค์/ตัวชี้วัด"); }
    // ความถี่ต้องตั้งก่อนตรวจงวดเริ่ม
    if let Some(fr) = &f.frequency { o["frequency"] = json!(fr); }
    if let Err(r) = apply(&c, &mut o, f).await { return r; }
    let g = gaps(&o);
    if !g.is_empty() { return bad(&format!("ยังขาด: {}", g.join(", "))); }
    let n: i64 = c.query_one(
        "INSERT INTO form_seq(form_code,year_be,last_seq) VALUES('KPI',0,1) ON CONFLICT(form_code,year_be) DO UPDATE SET last_seq=form_seq.last_seq+1 RETURNING last_seq", &[],
    ).await.map(|r| r.get(0)).unwrap_or(1);
    let code = format!("KPI-{n:03}");
    let now = now_iso();
    let res = c.query_one(
        "INSERT INTO kpi_objectives(code,title,dept,process,owner_code,owner_name,unit,direction,target,frequency,start_period,formula,plan,resources,evaluation,created_by,created_name,created_at,updated_at)
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$18) RETURNING id",
        &[&code, &s(&o, "title"), &s(&o, "dept"), &s(&o, "process"), &s(&o, "ownerCode"), &s(&o, "ownerName"), &s(&o, "unit"), &s(&o, "direction"),
          &o["target"].as_f64().unwrap_or(0.0), &s(&o, "frequency"), &s(&o, "startPeriod"), &s(&o, "formula"), &s(&o, "plan"), &s(&o, "resources"),
          &s(&o, "evaluation"), &u.emp_code, &u.name, &now],
    ).await;
    match res {
        Ok(r) => {
            let id: i64 = r.get(0);
            let _ = eform::audit(&c, &u, "kpi_create", "kpi", &code, &s(&o, "title")).await;
            one(&c, &u, id).await
        }
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, &format!("บันทึกไม่สำเร็จ: {e}")),
    }
}

#[derive(Deserialize)]
pub struct UpdateReq { version: i64, fields: Fields }

/// แก้ไขวัตถุประสงค์ (ผู้อนุมัติขึ้นไป) — เปลี่ยนเป้าหมายไม่กระทบผลงวดที่บันทึกแล้ว (แต่ละงวดเก็บเป้าหมาย ณ ตอนบันทึก)
pub async fn update(Path(id): Path<i64>, State(st): State<S>, Extension(u): Extension<AuthUser>, Json(b): Json<UpdateReq>) -> Response {
    let Ok(c) = st.db.get().await else { return db_err() };
    let Some(mut o) = load_all(&c, Some(id)).await.into_iter().next() else { return err(StatusCode::NOT_FOUND, "ไม่พบวัตถุประสงค์นี้") };
    if can_flags(&u, &o)["edit"] != json!(true) { return forbid("แก้ไขได้เฉพาะผู้อนุมัติขึ้นไป และวัตถุประสงค์ที่ยังใช้งาน"); }
    let before = o.clone();
    if let Some(fr) = &b.fields.frequency { o["frequency"] = json!(fr); }
    if let Err(r) = apply(&c, &mut o, b.fields).await { return r; }
    let g = gaps(&o);
    if !g.is_empty() { return bad(&format!("ยังขาด: {}", g.join(", "))); }
    let n = c.execute(
        "UPDATE kpi_objectives SET version=version+1,title=$3,dept=$4,process=$5,owner_code=$6,owner_name=$7,unit=$8,direction=$9,target=$10,frequency=$11,
           start_period=$12,formula=$13,plan=$14,resources=$15,evaluation=$16,updated_at=$17 WHERE id=$1 AND version=$2",
        &[&id, &b.version, &s(&o, "title"), &s(&o, "dept"), &s(&o, "process"), &s(&o, "ownerCode"), &s(&o, "ownerName"), &s(&o, "unit"),
          &s(&o, "direction"), &o["target"].as_f64().unwrap_or(0.0), &s(&o, "frequency"), &s(&o, "startPeriod"), &s(&o, "formula"),
          &s(&o, "plan"), &s(&o, "resources"), &s(&o, "evaluation"), &now_iso()],
    ).await.unwrap_or(0);
    if n == 0 { return err(StatusCode::CONFLICT, "ข้อมูลถูกแก้ไขโดยผู้อื่นแล้ว — โหลดใหม่ก่อนบันทึก"); }
    let changed: Vec<String> = ["title", "dept", "process", "ownerCode", "unit", "direction", "target", "frequency", "startPeriod", "formula", "plan", "resources", "evaluation"]
        .into_iter().filter(|k| before[*k] != o[*k])
        .map(|k| if k == "target" { format!("target {}→{}", before[k], o[k]) } else { k.to_string() }).collect();
    if !changed.is_empty() { let _ = eform::audit(&c, &u, "kpi_edit", "kpi", &s(&o, "code"), &changed.join(", ")).await; }
    one(&c, &u, id).await
}

#[derive(Deserialize)]
pub struct RetireReq { version: i64, reason: String }

/// เลิกใช้วัตถุประสงค์ (ผลเดิมยังเก็บไว้เป็นหลักฐาน)
pub async fn retire(Path(id): Path<i64>, State(st): State<S>, Extension(u): Extension<AuthUser>, Json(b): Json<RetireReq>) -> Response {
    let Ok(c) = st.db.get().await else { return db_err() };
    let Some(o) = load_all(&c, Some(id)).await.into_iter().next() else { return err(StatusCode::NOT_FOUND, "ไม่พบวัตถุประสงค์นี้") };
    if can_flags(&u, &o)["retire"] != json!(true) { return forbid("เลิกใช้ได้เฉพาะผู้อนุมัติขึ้นไป"); }
    let reason = clip(&b.reason, 1000);
    if reason.is_empty() { return bad("กรุณาระบุเหตุผล"); }
    let n = c.execute("UPDATE kpi_objectives SET version=version+1,status='retired',retire_reason=$3,updated_at=$4 WHERE id=$1 AND version=$2",
        &[&id, &b.version, &reason, &now_iso()]).await.unwrap_or(0);
    if n == 0 { return err(StatusCode::CONFLICT, "ข้อมูลถูกแก้ไขโดยผู้อื่นแล้ว — โหลดใหม่"); }
    let _ = eform::audit(&c, &u, "kpi_retire", "kpi", &s(&o, "code"), &reason).await;
    one(&c, &u, id).await
}

#[derive(Deserialize)]
pub struct ResultReq { period: String, actual: f64, note: Option<String>, evidence: Option<String> }

/// บันทึก/แก้ผลของงวด (ผู้รับผิดชอบหรือผู้อนุมัติขึ้นไป) — ค่าเดิมถูกเก็บใน audit log เมื่อมีการแก้
pub async fn record(Path(id): Path<i64>, State(st): State<S>, Extension(u): Extension<AuthUser>, Json(b): Json<ResultReq>) -> Response {
    let Ok(c) = st.db.get().await else { return db_err() };
    let Some(o) = load_all(&c, Some(id)).await.into_iter().next() else { return err(StatusCode::NOT_FOUND, "ไม่พบวัตถุประสงค์นี้") };
    if can_flags(&u, &o)["record"] != json!(true) { return forbid("บันทึกผลได้เฉพาะผู้รับผิดชอบตัวชี้วัด หรือผู้อนุมัติขึ้นไป"); }
    let freq = s(&o, "frequency");
    let period = clip(&b.period, 10);
    let Some(end) = period_end(&freq, &period) else { return bad("งวดไม่ถูกต้องตามความถี่ของตัวชี้วัด"); };
    if period_end(&freq, &period_of(&freq, today())).map(|cur| end > cur).unwrap_or(true) { return bad("บันทึกผลล่วงหน้าของงวดในอนาคตไม่ได้"); }
    if !b.actual.is_finite() { return bad("ผลต้องเป็นตัวเลข"); }
    let note = clip(&b.note.unwrap_or_default(), 2000);
    let evidence = clip(&b.evidence.unwrap_or_default(), 500);
    let old = o["results"].as_array().and_then(|a| a.iter().find(|r| r["period"] == json!(period)).cloned());
    // แก้ผลเดิม: ใช้เป้าหมายเดิมของงวดนั้น (เป้าหมายที่ประกาศตอนนั้น) และคงเลขที่ CAR ที่เปิดไว้
    let target = old.as_ref().and_then(|r| r["target"].as_f64()).unwrap_or_else(|| o["target"].as_f64().unwrap_or(0.0));
    let is_met = met(&s(&o, "direction"), b.actual, target);
    let now = now_iso();
    let r = c.execute(
        "INSERT INTO kpi_results(kpi_id,period,actual,target,met,note,evidence,recorded_code,recorded_name,recorded_at) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)
         ON CONFLICT(kpi_id,period) DO UPDATE SET actual=$3,met=$5,note=$6,evidence=$7,recorded_code=$8,recorded_name=$9,recorded_at=$10",
        &[&id, &period, &b.actual, &target, &is_met, &note, &evidence, &u.emp_code, &u.name, &now],
    ).await;
    if let Err(e) = r { return err(StatusCode::INTERNAL_SERVER_ERROR, &format!("บันทึกผลไม่สำเร็จ: {e}")); }
    let detail = match &old {
        Some(x) => format!("{period}: {} → {}{}", x["actual"], b.actual, s(&o, "unit")),
        None => format!("{period}: {}{} (เป้า {}{target}) {}", b.actual, s(&o, "unit"), if s(&o, "direction") == "lte" { "≤" } else { "≥" }, if is_met { "ถึงเป้า" } else { "ไม่ถึงเป้า" }),
    };
    let _ = eform::audit(&c, &u, if old.is_some() { "kpi_result_edit" } else { "kpi_result" }, "kpi", &s(&o, "code"), &detail).await;
    one(&c, &u, id).await
}

#[derive(Deserialize)]
pub struct CarReq { period: String }

/// เปิด CAR จากผลที่ไม่ถึงเป้า (10.2) — ผูกเลขที่ใบกลับไปที่ผลของงวด
pub async fn raise_car(Path(id): Path<i64>, State(st): State<S>, Extension(u): Extension<AuthUser>, Json(b): Json<CarReq>) -> Response {
    let Ok(c) = st.db.get().await else { return db_err() };
    let Some(o) = load_all(&c, Some(id)).await.into_iter().next() else { return err(StatusCode::NOT_FOUND, "ไม่พบวัตถุประสงค์นี้") };
    if can_flags(&u, &o)["car"] != json!(true) { return forbid("เปิด CAR ได้เฉพาะผู้รับผิดชอบตัวชี้วัด หรือผู้อนุมัติขึ้นไป"); }
    let Some(r) = o["results"].as_array().and_then(|a| a.iter().find(|r| r["period"] == json!(b.period)).cloned()) else { return bad("ไม่พบผลของงวดนี้"); };
    if r["met"] == json!(true) { return bad("งวดนี้ถึงเป้าแล้ว"); }
    if !s(&r, "carRef").is_empty() { return bad(&format!("งวดนี้เปิด {} ไว้แล้ว", s(&r, "carRef"))); }
    let unit = s(&o, "unit");
    let f = capa::Fields {
        kind: Some("CAR".into()),
        title: Some(format!("{} ไม่ถึงเป้า งวด {}", s(&o, "title"), b.period)),
        detail: Some(format!("ตัวชี้วัด {} · ผล {}{unit} เทียบเป้า {}{}{unit}{}", s(&o, "code"), r["actual"], if s(&o, "direction") == "lte" { "≤" } else { "≥" }, r["target"],
            if s(&r, "note").is_empty() { String::new() } else { format!(" · หมายเหตุ: {}", s(&r, "note")) })),
        dept: Some(s(&o, "dept")),
        source: Some("process".into()),
        assigneeCode: Some(s(&o, "ownerCode")),
        linkedRef: Some(format!("{} {}", s(&o, "code"), b.period)),
        ..Default::default()
    };
    let (_, ref_no) = match capa::open_action(&st, &c, &u, f).await { Ok(x) => x, Err(r) => return r };
    let _ = c.execute("UPDATE kpi_results SET car_ref=$3 WHERE kpi_id=$1 AND period=$2", &[&id, &b.period, &ref_no]).await;
    let _ = eform::audit(&c, &u, "kpi_car", "kpi", &s(&o, "code"), &format!("{} → {ref_no}", b.period)).await;
    one(&c, &u, id).await
}

// ---------- ใช้ร่วมกับโมดูลอื่น ----------

/// ตัวชี้วัดที่ใช้งานทั้งหมดพร้อมสถานะ (หน้าเตรียม Audit · ทบทวนฝ่ายบริหาร · แจ้งเตือน)
pub async fn active(c: &impl deadpool_postgres::GenericClient) -> Vec<Value> {
    load_all(c, None).await.into_iter().filter(|o| o["status"] == json!("active")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn d(y: i32, m: u32, dd: u32) -> chrono::NaiveDate { chrono::NaiveDate::from_ymd_opt(y, m, dd).unwrap() }

    #[test]
    fn periods() {
        assert_eq!(period_of("monthly", d(2026, 3, 9)), "2026-03");
        assert_eq!(period_of("quarterly", d(2026, 3, 9)), "2026-Q1");
        assert_eq!(period_of("yearly", d(2026, 3, 9)), "2026");
        assert_eq!(period_end("monthly", "2026-02"), Some(d(2026, 2, 28)));
        assert_eq!(period_end("monthly", "2026-12"), Some(d(2026, 12, 31)));
        assert_eq!(period_end("quarterly", "2026-Q4"), Some(d(2026, 12, 31)));
        assert_eq!(period_end("yearly", "2026"), Some(d(2026, 12, 31)));
        assert_eq!(period_end("monthly", "2026-13"), None);
        assert_eq!(period_end("monthly", "2026-Q1"), None);
        assert_eq!(period_end("quarterly", "2026-03"), None);
        assert_eq!(last_closed_period("monthly", d(2026, 1, 5)), "2025-12");
        assert_eq!(last_closed_period("quarterly", d(2026, 4, 1)), "2026-Q1");
        assert_eq!(last_closed_period("quarterly", d(2026, 2, 1)), "2025-Q4");
        assert_eq!(last_closed_period("yearly", d(2026, 6, 1)), "2025");
    }

    #[test]
    fn missing_after_grace() {
        let mut o = json!({ "frequency": "monthly", "status": "active", "direction": "gte", "startPeriod": "" });
        enrich(&mut o, vec![], d(2026, 4, 10));
        assert_eq!(o["expectedPeriod"], json!("2026-03"));
        assert_eq!(o["missing"], json!(false));
        assert_eq!(o["pending"], json!(true));
        let mut o2 = o.clone();
        enrich(&mut o2, vec![], d(2026, 4, 16));
        assert_eq!(o2["missing"], json!(true));
        // ก่อนงวดเริ่ม ไม่นับว่าค้าง
        let mut o3 = json!({ "frequency": "monthly", "status": "active", "direction": "gte", "startPeriod": "2026-04" });
        enrich(&mut o3, vec![], d(2026, 4, 20));
        assert_eq!(o3["missing"], json!(false));
        assert_eq!(o3["pending"], json!(false));
    }

    #[test]
    fn trend_respects_direction() {
        let r = |p: &str, a: f64| json!({ "period": p, "actual": a, "met": true, "carRef": "" });
        let mut lte = json!({ "frequency": "monthly", "status": "active", "direction": "lte", "startPeriod": "" });
        enrich(&mut lte, vec![r("2026-01", 5.0), r("2026-02", 3.0)], d(2026, 3, 1));
        assert_eq!(lte["trend"], json!("better"));
        let mut gte = json!({ "frequency": "monthly", "status": "active", "direction": "gte", "startPeriod": "" });
        enrich(&mut gte, vec![r("2026-01", 5.0), r("2026-02", 3.0)], d(2026, 3, 1));
        assert_eq!(gte["trend"], json!("worse"));
        assert!(met("lte", 2.0, 2.0) && !met("lte", 2.1, 2.0) && met("gte", 95.0, 95.0));
    }
}
