// เครื่องมือวัดและการสอบเทียบ (ISO 9001:2015 ข้อ 7.1.5)
//   ทะเบียนเครื่องมือ EQ-xxx: ช่วงวัด/ความแม่นยำ/เกณฑ์ยอมรับ · ผู้รับผิดชอบ · รอบสอบเทียบ (เดือน) → กำหนดครั้งถัดไป
//   บันทึกผลสอบเทียบ (วันที่ · ผ่าน/ไม่ผ่าน · เลขที่ใบรับรอง · ผู้สอบเทียบ) — ผ่าน = ใช้งานได้ถึงกำหนดถัดไป
//   ไม่ผ่าน = หยุดใช้งานทันที + ต้องประเมินผลกระทบต่อผลการวัดที่ผ่านมา (7.1.5.2) และเปิด CAR ได้ (ผูกเลขที่ใบไว้ที่ผลสอบเทียบ)
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
CREATE TABLE IF NOT EXISTS equipment(
  id BIGSERIAL PRIMARY KEY,
  code TEXT NOT NULL UNIQUE,
  version BIGINT NOT NULL DEFAULT 1,
  name TEXT NOT NULL,
  asset_no TEXT NOT NULL DEFAULT '',
  serial_no TEXT NOT NULL DEFAULT '',
  maker TEXT NOT NULL DEFAULT '',
  location TEXT NOT NULL DEFAULT '',
  dept TEXT NOT NULL DEFAULT '',
  owner_code TEXT NOT NULL DEFAULT '',
  owner_name TEXT NOT NULL DEFAULT '',
  range_text TEXT NOT NULL DEFAULT '',
  accuracy TEXT NOT NULL DEFAULT '',
  acceptance TEXT NOT NULL DEFAULT '',
  cal_method TEXT NOT NULL DEFAULT 'external',
  interval_months BIGINT NOT NULL DEFAULT 12,
  last_cal TEXT NOT NULL DEFAULT '',
  next_due TEXT NOT NULL DEFAULT '',
  last_result TEXT NOT NULL DEFAULT '',
  status TEXT NOT NULL DEFAULT 'in_use',
  status_reason TEXT NOT NULL DEFAULT '',
  created_by TEXT NOT NULL,
  created_name TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS calibrations(
  id BIGSERIAL PRIMARY KEY,
  equipment_id BIGINT NOT NULL,
  cal_date TEXT NOT NULL,
  method TEXT NOT NULL DEFAULT 'external',
  result TEXT NOT NULL,
  cert_ref TEXT NOT NULL DEFAULT '',
  provider TEXT NOT NULL DEFAULT '',
  finding TEXT NOT NULL DEFAULT '',
  next_due TEXT NOT NULL DEFAULT '',
  impact_result TEXT NOT NULL DEFAULT '',
  impact TEXT NOT NULL DEFAULT '',
  impact_action TEXT NOT NULL DEFAULT '',
  impact_by TEXT NOT NULL DEFAULT '',
  impact_at TEXT NOT NULL DEFAULT '',
  car_ref TEXT NOT NULL DEFAULT '',
  recorded_code TEXT NOT NULL,
  recorded_name TEXT NOT NULL,
  recorded_at TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS idx_cal_eq ON calibrations(equipment_id);
CREATE TABLE IF NOT EXISTS equipment_events(
  id BIGSERIAL PRIMARY KEY,
  equipment_id BIGINT NOT NULL,
  event TEXT NOT NULL,
  actor_code TEXT NOT NULL,
  actor_name TEXT NOT NULL,
  note TEXT NOT NULL DEFAULT '',
  at TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS idx_eqe_eq ON equipment_events(equipment_id);
";

const COLS: &str = "e.id,e.code,e.version,e.name,e.asset_no,e.serial_no,e.maker,e.location,e.dept,e.owner_code,e.owner_name,e.range_text,e.accuracy,e.acceptance,
  e.cal_method,e.interval_months,e.last_cal,e.next_due,e.last_result,e.status,e.status_reason,e.created_by,e.created_name,e.created_at,e.updated_at,
  (SELECT COUNT(*) FROM calibrations k WHERE k.equipment_id=e.id AND k.result='fail' AND k.impact_result=''),
  (SELECT COUNT(*) FROM calibrations k WHERE k.equipment_id=e.id AND k.result='fail' AND k.impact_result='affected' AND k.car_ref='')";
const KCOLS: &str = "id,equipment_id,cal_date,method,result,cert_ref,provider,finding,next_due,impact_result,impact,impact_action,impact_by,impact_at,car_ref,recorded_code,recorded_name,recorded_at";
const METHODS: [&str; 2] = ["external", "internal"];
/// แจ้งเตือนเมื่อเหลือไม่เกินกี่วันก่อนครบกำหนดสอบเทียบ
pub const SOON_DAYS: i64 = 7;

fn db_err() -> Response { err(StatusCode::INTERNAL_SERVER_ERROR, "เชื่อมต่อฐานข้อมูลไม่ได้") }
fn bad(m: &str) -> Response { err(StatusCode::BAD_REQUEST, m) }
fn forbid(m: &str) -> Response { err(StatusCode::FORBIDDEN, m) }
fn stale() -> Response { err(StatusCode::CONFLICT, "ข้อมูลถูกแก้ไขโดยผู้อื่นแล้ว — โหลดใหม่ก่อนบันทึก") }
fn s(v: &Value, k: &str) -> String { v[k].as_str().unwrap_or("").to_string() }
fn i(v: &Value, k: &str) -> i64 { v[k].as_i64().unwrap_or(0) }
fn clip(t: &str, n: usize) -> String { t.trim().chars().take(n).collect() }
fn parse(d: &str) -> Option<chrono::NaiveDate> { chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok() }
fn valid_date(d: &str) -> bool { d.is_empty() || parse(d).is_some() }

/// วันที่ + n เดือน (31 ม.ค. + 1 เดือน = สิ้นเดือน ก.พ.)
pub fn add_months(d: &str, months: i64) -> String {
    parse(d).and_then(|x| x.checked_add_months(chrono::Months::new(months.max(0) as u32))).map(|x| x.format("%Y-%m-%d").to_string()).unwrap_or_default()
}
/// จำนวนวันจากวันนี้ถึงกำหนด (ติดลบ = เลยกำหนด)
pub fn days_left(due: &str, today: &str) -> Option<i64> { Some((parse(due)? - parse(today)?).num_days()) }
/// สถานะกำหนดสอบเทียบ: overdue · soon (≤ 7 วัน) · ok · none (ยังไม่มีกำหนด)
pub fn due_state(due: &str, today: &str) -> &'static str {
    match days_left(due, today) { None => "none", Some(n) if n < 0 => "overdue", Some(n) if n <= SOON_DAYS => "soon", _ => "ok" }
}

fn row_json(r: &tokio_postgres::Row) -> Value {
    let g = |k: usize| r.get::<_, String>(k);
    let n = |k: usize| r.get::<_, i64>(k);
    let (next_due, status) = (g(17), g(19));
    let today = capa::today();
    let state = if status == "retired" { "none" } else { due_state(&next_due, &today) };
    json!({
        "id": n(0), "code": g(1), "version": n(2), "name": g(3), "assetNo": g(4), "serialNo": g(5), "maker": g(6), "location": g(7), "dept": g(8),
        "ownerCode": g(9), "ownerName": g(10), "rangeText": g(11), "accuracy": g(12), "acceptance": g(13), "calMethod": g(14), "intervalMonths": n(15),
        "lastCal": g(16), "nextDue": next_due, "lastResult": g(18), "status": status, "statusReason": g(20), "createdBy": g(21), "createdName": g(22),
        "createdAt": g(23), "updatedAt": g(24), "pendingImpact": n(25), "affectedNoCar": n(26),
        "dueState": state, "daysLeft": days_left(&next_due, &today),
        "overdue": status == "in_use" && state == "overdue",
        "neverCalibrated": status != "retired" && g(18).is_empty(),
    })
}
fn cal_json(r: &tokio_postgres::Row) -> Value {
    let g = |k: usize| r.get::<_, String>(k);
    json!({
        "id": r.get::<_, i64>(0), "equipmentId": r.get::<_, i64>(1), "calDate": g(2), "method": g(3), "result": g(4), "certRef": g(5), "provider": g(6),
        "finding": g(7), "nextDue": g(8), "impactResult": g(9), "impact": g(10), "impactAction": g(11), "impactBy": g(12), "impactAt": g(13),
        "carRef": g(14), "recordedCode": g(15), "recordedName": g(16), "recordedAt": g(17),
    })
}

fn can_flags(u: &AuthUser, e: &Value) -> Value {
    let reviewer = eform::can_review(&u.role);
    let owner = s(e, "ownerCode") == u.emp_code;
    let write = u.role != "viewer";
    let active = s(e, "status") != "retired";
    let work = write && active && (owner || reviewer);
    json!({
        "edit": write && active && (owner || reviewer || s(e, "createdBy") == u.emp_code),
        "calibrate": work,
        "impact": work,
        "car": work,
        "suspend": work && s(e, "status") == "in_use",
        // กลับมาใช้งานได้เมื่อผลสอบเทียบล่าสุดผ่านและยังไม่เลยกำหนด
        "resume": work && s(e, "status") == "out_of_service" && s(e, "lastResult") == "pass" && s(e, "dueState") != "overdue" && i(e, "pendingImpact") == 0,
        "retire": write && active && reviewer,
        "reinstate": write && !active && reviewer,
    })
}

/// ข้อมูลขั้นต่ำของทะเบียน
fn gaps(e: &Value) -> Vec<&'static str> {
    let mut g = vec![];
    if s(e, "name").trim().is_empty() { g.push("ชื่อเครื่องมือ"); }
    if s(e, "ownerCode").is_empty() { g.push("ผู้รับผิดชอบ"); }
    if s(e, "location").trim().is_empty() && s(e, "dept").is_empty() { g.push("สถานที่/แผนก"); }
    if s(e, "rangeText").trim().is_empty() && s(e, "accuracy").trim().is_empty() { g.push("ช่วงวัด/ความแม่นยำ"); }
    if !(1..=60).contains(&i(e, "intervalMonths")) { g.push("รอบสอบเทียบ"); }
    g
}

/// ผลสอบเทียบไม่ผ่านต้องประเมินผลกระทบ — ถ้ากระทบต้องระบุการดำเนินการกับผลการวัด/ผลิตภัณฑ์
fn impact_gaps(result: &str, impact: &str, action: &str, text: &str) -> Option<&'static str> {
    if result != "fail" { return None; }
    if !matches!(impact, "none" | "affected") { return Some("กรุณาเลือกผลการประเมินผลกระทบ"); }
    if text.trim().is_empty() { return Some("กรุณาบันทึกการประเมินผลกระทบต่อผลการวัดที่ผ่านมา"); }
    if impact == "affected" && action.trim().is_empty() { return Some("กรุณาระบุการดำเนินการกับผลการวัด/ผลิตภัณฑ์ที่ได้รับผลกระทบ"); }
    None
}

async fn load(c: &impl deadpool_postgres::GenericClient, id: i64) -> Option<Value> {
    let r = c.query_opt(&format!("SELECT {COLS} FROM equipment e WHERE e.id=$1"), &[&id]).await.ok()??;
    Some(row_json(&r))
}
async fn event(c: &impl deadpool_postgres::GenericClient, id: i64, ev: &str, u: &AuthUser, note: &str) {
    let _ = c.execute("INSERT INTO equipment_events(equipment_id,event,actor_code,actor_name,note,at) VALUES($1,$2,$3,$4,$5,$6)",
        &[&id, &ev.to_string(), &u.emp_code, &u.name, &note.to_string(), &now_iso()]).await;
}
/// ผลสอบเทียบผ่านครั้งก่อนหน้า (จุดเริ่มของช่วงผลการวัดที่อาจได้รับผลกระทบ)
async fn previous_pass(c: &impl deadpool_postgres::GenericClient, eq: i64, k: &Value) -> String {
    c.query_opt("SELECT cal_date FROM calibrations WHERE equipment_id=$1 AND result='pass' AND (cal_date<$2 OR (cal_date=$2 AND id<$3)) ORDER BY cal_date DESC, id DESC LIMIT 1",
        &[&eq, &s(k, "calDate"), &i(k, "id")])
        .await.ok().flatten().map(|r| r.get(0)).unwrap_or_default()
}

async fn detail_resp(st: &S, u: &AuthUser, id: i64) -> Response {
    let Ok(c) = st.db.get().await else { return db_err() };
    let Some(mut e) = load(&c, id).await else { return err(StatusCode::NOT_FOUND, "ไม่พบเครื่องมือนี้") };
    let can = can_flags(u, &e);
    let mut cals: Vec<Value> = c.query(&format!("SELECT {KCOLS} FROM calibrations WHERE equipment_id=$1 ORDER BY cal_date DESC, id DESC"), &[&id]).await
        .map(|rows| rows.iter().map(cal_json).collect()).unwrap_or_default();
    // ช่วงผลการวัดที่อาจได้รับผลกระทบ = ตั้งแต่ผลผ่านครั้งก่อนหน้าถึงวันที่ไม่ผ่าน
    for k in 0..cals.len() {
        if cals[k]["result"] != json!("fail") { continue; }
        // รายการเรียงใหม่ → เก่า: ผลผ่านรายการแรกที่อยู่ถัดไปคือครั้งก่อนหน้า (วันเดียวกันได้)
        let since = cals[k + 1..].iter().find(|x| x["result"] == json!("pass")).map(|x| s(x, "calDate")).unwrap_or_default();
        cals[k]["affectedSince"] = json!(since);
        cals[k]["can"] = json!({
            "impact": can["impact"] == json!(true) && s(&cals[k], "impactResult").is_empty(),
            "car": can["car"] == json!(true) && s(&cals[k], "carRef").is_empty(),
        });
    }
    e["can"] = can;
    e["gaps"] = json!(gaps(&e));
    e["calibrations"] = json!(cals);
    e["history"] = json!(c.query("SELECT event,actor_name,note,at FROM equipment_events WHERE equipment_id=$1 ORDER BY id", &[&id]).await
        .map(|rows| rows.iter().map(|x| json!({ "event": x.get::<_, String>(0), "actorName": x.get::<_, String>(1), "note": x.get::<_, String>(2), "at": x.get::<_, String>(3) })).collect::<Vec<_>>())
        .unwrap_or_default());
    Json(json!({ "ok": true, "equipment": e })).into_response()
}

// ---------- handlers ----------

pub async fn list(State(st): State<S>, Extension(u): Extension<AuthUser>) -> Response {
    let Ok(c) = st.db.get().await else { return db_err() };
    match c.query(&format!("SELECT {COLS} FROM equipment e ORDER BY CASE e.status WHEN 'retired' THEN 1 ELSE 0 END, NULLIF(e.next_due,'') NULLS FIRST, e.code"), &[]).await {
        Ok(rows) => Json(Value::Array(rows.iter().map(|r| { let mut x = row_json(r); x["can"] = can_flags(&u, &x); x["gaps"] = json!(gaps(&x)); x }).collect())).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, &format!("อ่านทะเบียนไม่ได้: {e}")),
    }
}

pub async fn get(Path(id): Path<i64>, State(st): State<S>, Extension(u): Extension<AuthUser>) -> Response { detail_resp(&st, &u, id).await }

#[derive(Deserialize, Default)]
#[allow(non_snake_case)]
pub struct Fields {
    name: Option<String>, assetNo: Option<String>, serialNo: Option<String>, maker: Option<String>, location: Option<String>, dept: Option<String>,
    ownerCode: Option<String>, rangeText: Option<String>, accuracy: Option<String>, acceptance: Option<String>, calMethod: Option<String>,
    intervalMonths: Option<i64>, nextDue: Option<String>,
}

async fn apply(c: &impl deadpool_postgres::GenericClient, e: &mut Value, f: Fields) -> Result<(), Response> {
    if let Some(t) = f.name { let t = clip(&t, 200); if t.is_empty() { return Err(bad("ต้องระบุชื่อเครื่องมือ")); } e["name"] = json!(t); }
    for (k, v, n) in [("assetNo", f.assetNo, 60), ("serialNo", f.serialNo, 100), ("maker", f.maker, 200), ("location", f.location, 200), ("dept", f.dept, 20),
                      ("rangeText", f.rangeText, 300), ("accuracy", f.accuracy, 300), ("acceptance", f.acceptance, 1000)] {
        if let Some(x) = v { e[k] = json!(clip(&x, n)); }
    }
    if let Some(m) = f.calMethod { if !METHODS.contains(&m.as_str()) { return Err(bad("วิธีสอบเทียบไม่ถูกต้อง")); } e["calMethod"] = json!(m); }
    if let Some(n) = f.intervalMonths { if !(1..=60).contains(&n) { return Err(bad("รอบสอบเทียบต้องอยู่ระหว่าง 1–60 เดือน")); } e["intervalMonths"] = json!(n); }
    if let Some(d) = f.nextDue { let d = clip(&d, 10); if !valid_date(&d) { return Err(bad("รูปแบบวันที่ไม่ถูกต้อง (YYYY-MM-DD)")); } e["nextDue"] = json!(d); }
    if let Some(code) = f.ownerCode {
        let code = clip(&code, 20);
        let name: String = if code.is_empty() { String::new() } else {
            c.query_opt("SELECT name_th FROM employees WHERE emp_code=$1 AND status='active'", &[&code]).await.ok().flatten()
                .map(|x| x.get(0)).ok_or_else(|| bad("ไม่พบผู้รับผิดชอบในรายชื่อพนักงาน"))?
        };
        e["ownerCode"] = json!(code); e["ownerName"] = json!(name);
    }
    Ok(())
}

#[derive(Deserialize)]
#[allow(non_snake_case)]
pub struct CreateReq {
    #[serde(flatten)]
    fields: Fields,
    /// ผลสอบเทียบล่าสุดที่มีอยู่แล้ว (ไม่บังคับ) — ไม่ระบุ = ยังไม่สอบเทียบ (หยุดใช้งานจนกว่าจะสอบเทียบ)
    initialCalDate: Option<String>, initialCertRef: Option<String>, initialProvider: Option<String>,
}

/// ขึ้นทะเบียนเครื่องมือ (ผู้จัดทำขึ้นไป) — รหัส EQ-xxx ต่อเนื่อง
pub async fn create(State(st): State<S>, Extension(u): Extension<AuthUser>, Json(b): Json<CreateReq>) -> Response {
    if u.role == "viewer" { return forbid("บัญชีผู้อ่านอย่างเดียวบันทึกไม่ได้"); }
    let Ok(c) = st.db.get().await else { return db_err() };
    if b.fields.name.as_deref().map(|t| t.trim().is_empty()).unwrap_or(true) { return bad("ต้องระบุชื่อเครื่องมือ"); }
    let mut e = json!({ "calMethod": "external", "intervalMonths": 12, "nextDue": "" });
    if let Err(x) = apply(&c, &mut e, b.fields).await { return x; }
    let g = gaps(&e);
    if !g.is_empty() { return bad(&format!("ยังขาด: {}", g.join(", "))); }
    let today = capa::today();
    let init_date = clip(&b.initialCalDate.unwrap_or_default(), 10);
    if !valid_date(&init_date) || init_date.as_str() > today.as_str() { return bad("วันที่สอบเทียบล่าสุดไม่ถูกต้อง (ต้องไม่เกินวันนี้)"); }
    let (last_result, status, reason, next_due) = if init_date.is_empty() {
        (String::new(), "out_of_service", "รอสอบเทียบก่อนใช้งาน".to_string(), String::new())
    } else {
        let nd = if s(&e, "nextDue").is_empty() { add_months(&init_date, i(&e, "intervalMonths")) } else { s(&e, "nextDue") };
        if nd <= init_date { return bad("กำหนดสอบเทียบครั้งถัดไปต้องหลังวันที่สอบเทียบ"); }
        ("pass".to_string(), if nd < today { "out_of_service" } else { "in_use" }, if nd < today { "เลยกำหนดสอบเทียบ".to_string() } else { String::new() }, nd)
    };
    let n: i64 = match c.query_one(
        "INSERT INTO form_seq(form_code,year_be,last_seq) VALUES('EQ',0,1) ON CONFLICT(form_code,year_be) DO UPDATE SET last_seq=form_seq.last_seq+1 RETURNING last_seq", &[],
    ).await { Ok(x) => x.get(0), Err(x) => return err(StatusCode::INTERNAL_SERVER_ERROR, &x.to_string()) };
    let code = format!("EQ-{n:03}");
    let now = now_iso();
    let res = c.query_one(
        "INSERT INTO equipment(code,name,asset_no,serial_no,maker,location,dept,owner_code,owner_name,range_text,accuracy,acceptance,cal_method,interval_months,
           last_cal,next_due,last_result,status,status_reason,created_by,created_name,created_at,updated_at)
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$22) RETURNING id",
        &[&code, &s(&e, "name"), &s(&e, "assetNo"), &s(&e, "serialNo"), &s(&e, "maker"), &s(&e, "location"), &s(&e, "dept"), &s(&e, "ownerCode"),
          &s(&e, "ownerName"), &s(&e, "rangeText"), &s(&e, "accuracy"), &s(&e, "acceptance"), &s(&e, "calMethod"), &i(&e, "intervalMonths"),
          &init_date, &next_due, &last_result, &status.to_string(), &reason, &u.emp_code, &u.name, &now],
    ).await;
    let id: i64 = match res { Ok(x) => x.get(0), Err(x) => return err(StatusCode::INTERNAL_SERVER_ERROR, &format!("บันทึกไม่สำเร็จ: {x}")) };
    if !init_date.is_empty() {
        let _ = c.execute(
            "INSERT INTO calibrations(equipment_id,cal_date,method,result,cert_ref,provider,finding,next_due,recorded_code,recorded_name,recorded_at)
             VALUES($1,$2,$3,'pass',$4,$5,'ผลสอบเทียบล่าสุดก่อนขึ้นทะเบียนในระบบ',$6,$7,$8,$9)",
            &[&id, &init_date, &s(&e, "calMethod"), &clip(&b.initialCertRef.unwrap_or_default(), 100), &clip(&b.initialProvider.unwrap_or_default(), 200),
              &next_due, &u.emp_code, &u.name, &now],
        ).await;
    }
    event(&c, id, "create", &u, &s(&e, "name")).await;
    let _ = eform::audit(&c, &u, "equipment_create", "equipment", &code, &s(&e, "name")).await;
    detail_resp(&st, &u, id).await
}

#[derive(Deserialize)]
pub struct UpdateReq { version: i64, fields: Fields }

/// แก้ไขทะเบียน — ผู้บันทึก ผู้รับผิดชอบ หรือผู้ตรวจสอบขึ้นไป (กำหนดถัดไปปรับได้ เช่น ขยายรอบตามผลย้อนหลัง)
pub async fn update(Path(id): Path<i64>, State(st): State<S>, Extension(u): Extension<AuthUser>, Json(b): Json<UpdateReq>) -> Response {
    let Ok(c) = st.db.get().await else { return db_err() };
    let Some(mut e) = load(&c, id).await else { return err(StatusCode::NOT_FOUND, "ไม่พบเครื่องมือนี้") };
    if can_flags(&u, &e)["edit"] != json!(true) { return forbid("แก้ไขได้เฉพาะผู้บันทึก ผู้รับผิดชอบ หรือผู้ตรวจสอบขึ้นไป"); }
    let before = e.clone();
    if let Err(x) = apply(&c, &mut e, b.fields).await { return x; }
    let g = gaps(&e);
    if !g.is_empty() { return bad(&format!("ยังขาด: {}", g.join(", "))); }
    if !s(&e, "lastCal").is_empty() && !s(&e, "nextDue").is_empty() && s(&e, "nextDue") <= s(&e, "lastCal") { return bad("กำหนดสอบเทียบครั้งถัดไปต้องหลังวันที่สอบเทียบล่าสุด"); }
    let n = c.execute(
        "UPDATE equipment SET version=version+1,name=$3,asset_no=$4,serial_no=$5,maker=$6,location=$7,dept=$8,owner_code=$9,owner_name=$10,range_text=$11,
           accuracy=$12,acceptance=$13,cal_method=$14,interval_months=$15,next_due=$16,updated_at=$17 WHERE id=$1 AND version=$2",
        &[&id, &b.version, &s(&e, "name"), &s(&e, "assetNo"), &s(&e, "serialNo"), &s(&e, "maker"), &s(&e, "location"), &s(&e, "dept"), &s(&e, "ownerCode"),
          &s(&e, "ownerName"), &s(&e, "rangeText"), &s(&e, "accuracy"), &s(&e, "acceptance"), &s(&e, "calMethod"), &i(&e, "intervalMonths"), &s(&e, "nextDue"), &now_iso()],
    ).await.unwrap_or(0);
    if n == 0 { return stale(); }
    let changed: Vec<String> = ["name", "assetNo", "serialNo", "maker", "location", "dept", "ownerCode", "rangeText", "accuracy", "acceptance", "calMethod", "intervalMonths", "nextDue"]
        .into_iter().filter(|k| before[*k] != e[*k])
        .map(|k| if k == "nextDue" { format!("nextDue {}→{}", s(&before, k), s(&e, k)) } else { k.to_string() }).collect();
    if !changed.is_empty() {
        event(&c, id, "edit", &u, &changed.join(", ")).await;
        let _ = eform::audit(&c, &u, "equipment_edit", "equipment", &s(&e, "code"), &changed.join(", ")).await;
    }
    detail_resp(&st, &u, id).await
}

#[derive(Deserialize)]
#[allow(non_snake_case)]
pub struct CalReq {
    version: i64, calDate: String, method: Option<String>, result: String, certRef: String, provider: String, finding: Option<String>,
    nextDue: Option<String>, impactResult: Option<String>, impact: Option<String>, impactAction: Option<String>,
}

/// บันทึกผลสอบเทียบ — ผ่าน: ใช้งานได้ + กำหนดครั้งถัดไป · ไม่ผ่าน: หยุดใช้งาน (ประเมินผลกระทบพร้อมกันหรือภายหลังได้)
pub async fn calibrate(Path(id): Path<i64>, State(st): State<S>, Extension(u): Extension<AuthUser>, Json(b): Json<CalReq>) -> Response {
    let Ok(c) = st.db.get().await else { return db_err() };
    let Some(e) = load(&c, id).await else { return err(StatusCode::NOT_FOUND, "ไม่พบเครื่องมือนี้") };
    if can_flags(&u, &e)["calibrate"] != json!(true) { return forbid("บันทึกผลได้เฉพาะผู้รับผิดชอบเครื่องมือ หรือผู้ตรวจสอบขึ้นไป"); }
    let today = capa::today();
    let date = clip(&b.calDate, 10);
    if parse(&date).is_none() || date > today { return bad("วันที่สอบเทียบไม่ถูกต้อง (ต้องไม่เกินวันนี้)"); }
    if !s(&e, "lastCal").is_empty() && date < s(&e, "lastCal") { return bad(&format!("วันที่สอบเทียบต้องไม่ก่อนครั้งล่าสุด ({})", s(&e, "lastCal"))); }
    if !matches!(b.result.as_str(), "pass" | "fail") { return bad("ผลต้องเป็น ผ่าน หรือ ไม่ผ่าน"); }
    let method = b.method.unwrap_or_else(|| s(&e, "calMethod"));
    if !METHODS.contains(&method.as_str()) { return bad("วิธีสอบเทียบไม่ถูกต้อง"); }
    let cert = clip(&b.certRef, 100);
    let provider = clip(&b.provider, 200);
    if cert.is_empty() { return bad("กรุณาระบุเลขที่ใบรับรอง/รายงานผลสอบเทียบ"); }
    if provider.is_empty() { return bad("กรุณาระบุผู้สอบเทียบ (ห้องปฏิบัติการ/ผู้ทวนสอบ)"); }
    let finding = clip(&b.finding.unwrap_or_default(), 3000);
    let pass = b.result == "pass";
    let next = if pass {
        let d = clip(&b.nextDue.unwrap_or_default(), 10);
        let d = if d.is_empty() { add_months(&date, i(&e, "intervalMonths")) } else { d };
        if parse(&d).is_none() || d <= date { return bad("กำหนดสอบเทียบครั้งถัดไปต้องหลังวันที่สอบเทียบ"); }
        d
    } else { String::new() };
    if !pass && finding.is_empty() { return bad("กรุณาบันทึกผลที่พบ (ค่าที่วัดได้/ความคลาดเคลื่อนเทียบเกณฑ์)"); }
    // ประเมินผลกระทบพร้อมกันได้ (ไม่บังคับ — ทำภายหลังที่ผลสอบเทียบนั้น)
    let (ir, it, ia) = (b.impactResult.unwrap_or_default(), clip(&b.impact.unwrap_or_default(), 3000), clip(&b.impactAction.unwrap_or_default(), 3000));
    let with_impact = !pass && !ir.is_empty();
    if with_impact { if let Some(m) = impact_gaps("fail", &ir, &ia, &it) { return bad(m); } }
    let now = now_iso();
    // ผ่าน = ใช้งานได้ (ยกเว้นยังมีผลไม่ผ่านที่ค้างประเมินผลกระทบ) · ไม่ผ่าน = หยุดใช้งาน · กำหนดเดิมคงไว้เมื่อไม่ผ่าน
    let pending = i(&e, "pendingImpact") > 0;
    let (status, reason, nd) = if pass {
        if pending { ("out_of_service", "รอประเมินผลกระทบของผลสอบเทียบที่ไม่ผ่าน".to_string(), next.clone()) } else { ("in_use", String::new(), next.clone()) }
    } else { ("out_of_service", format!("ผลสอบเทียบไม่ผ่าน {date}"), s(&e, "nextDue")) };
    let n = c.execute(
        "UPDATE equipment SET version=version+1,last_cal=$3,last_result=$4,next_due=$5,status=$6,status_reason=$7,updated_at=$8 WHERE id=$1 AND version=$2",
        &[&id, &b.version, &date, &b.result, &nd, &status.to_string(), &reason, &now],
    ).await.unwrap_or(0);
    if n == 0 { return stale(); }
    let _ = c.execute(
        "INSERT INTO calibrations(equipment_id,cal_date,method,result,cert_ref,provider,finding,next_due,impact_result,impact,impact_action,impact_by,impact_at,
           recorded_code,recorded_name,recorded_at) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)",
        &[&id, &date, &method, &b.result, &cert, &provider, &finding, &next, &if with_impact { ir.clone() } else { String::new() },
          &if with_impact { it.clone() } else { String::new() }, &if with_impact { ia.clone() } else { String::new() },
          &if with_impact { u.name.clone() } else { String::new() }, &if with_impact { now.clone() } else { String::new() }, &u.emp_code, &u.name, &now],
    ).await;
    let summary = format!("{date} {} · {cert} · {provider}{}", if pass { "ผ่าน" } else { "ไม่ผ่าน" }, if pass { format!(" · ครั้งถัดไป {next}") } else { String::new() });
    event(&c, id, if pass { "cal_pass" } else { "cal_fail" }, &u, &summary).await;
    let _ = eform::audit(&c, &u, if pass { "calibration_pass" } else { "calibration_fail" }, "equipment", &s(&e, "code"), &summary).await;
    if with_impact {
        let _ = eform::audit(&c, &u, "calibration_impact", "equipment", &s(&e, "code"), &format!("{ir}: {it}")).await;
    }
    detail_resp(&st, &u, id).await
}

#[derive(Deserialize)]
#[allow(non_snake_case)]
pub struct ImpactReq { impactResult: String, impact: String, impactAction: Option<String> }

/// ประเมินผลกระทบต่อผลการวัดที่ผ่านมาของผลสอบเทียบที่ไม่ผ่าน (7.1.5.2) — บันทึกครั้งเดียว
pub async fn impact(Path((id, cid)): Path<(i64, i64)>, State(st): State<S>, Extension(u): Extension<AuthUser>, Json(b): Json<ImpactReq>) -> Response {
    let Ok(c) = st.db.get().await else { return db_err() };
    let Some(e) = load(&c, id).await else { return err(StatusCode::NOT_FOUND, "ไม่พบเครื่องมือนี้") };
    if can_flags(&u, &e)["impact"] != json!(true) { return forbid("ประเมินผลกระทบได้เฉพาะผู้รับผิดชอบเครื่องมือ หรือผู้ตรวจสอบขึ้นไป"); }
    let Some(k) = c.query_opt(&format!("SELECT {KCOLS} FROM calibrations WHERE id=$1 AND equipment_id=$2"), &[&cid, &id]).await.ok().flatten().map(|r| cal_json(&r))
        else { return err(StatusCode::NOT_FOUND, "ไม่พบผลสอบเทียบนี้") };
    if k["result"] != json!("fail") { return bad("ประเมินผลกระทบเฉพาะผลสอบเทียบที่ไม่ผ่าน"); }
    if !s(&k, "impactResult").is_empty() { return bad("ผลสอบเทียบนี้ประเมินผลกระทบแล้ว"); }
    let (text, action) = (clip(&b.impact, 3000), clip(&b.impactAction.unwrap_or_default(), 3000));
    if let Some(m) = impact_gaps("fail", &b.impactResult, &action, &text) { return bad(m); }
    let now = now_iso();
    let n = c.execute("UPDATE calibrations SET impact_result=$2,impact=$3,impact_action=$4,impact_by=$5,impact_at=$6 WHERE id=$1 AND impact_result=''",
        &[&cid, &b.impactResult, &text, &action, &u.name, &now]).await.unwrap_or(0);
    if n == 0 { return stale(); }
    // ผลล่าสุดผ่านแล้วและไม่มีรายการค้างประเมิน → กลับมาใช้งานได้
    if s(&e, "lastResult") == "pass" && i(&e, "pendingImpact") <= 1 && s(&e, "status") == "out_of_service" && s(&e, "dueState") != "overdue" {
        let _ = c.execute("UPDATE equipment SET version=version+1,status='in_use',status_reason='',updated_at=$2 WHERE id=$1", &[&id, &now]).await;
    } else {
        let _ = c.execute("UPDATE equipment SET version=version+1,updated_at=$2 WHERE id=$1", &[&id, &now]).await;
    }
    let since = previous_pass(&c, id, &k).await;
    let note = format!("ผลสอบเทียบ {} · {} (ช่วง {}–{}) · {text}", s(&k, "calDate"), if b.impactResult == "affected" { "กระทบผลการวัด" } else { "ไม่กระทบ" },
        if since.is_empty() { "เริ่มใช้งาน".into() } else { since }, s(&k, "calDate"));
    event(&c, id, "impact", &u, &note).await;
    let _ = eform::audit(&c, &u, "calibration_impact", "equipment", &s(&e, "code"), &note).await;
    detail_resp(&st, &u, id).await
}

/// เปิด CAR จากผลสอบเทียบที่ไม่ผ่าน (ผู้รับผิดชอบเครื่องมือเป็นผู้รับผิดชอบใบ) — ผูกเลขที่ใบกลับไปที่ผลสอบเทียบ
pub async fn raise_car(Path((id, cid)): Path<(i64, i64)>, State(st): State<S>, Extension(u): Extension<AuthUser>) -> Response {
    let Ok(c) = st.db.get().await else { return db_err() };
    let Some(e) = load(&c, id).await else { return err(StatusCode::NOT_FOUND, "ไม่พบเครื่องมือนี้") };
    if can_flags(&u, &e)["car"] != json!(true) { return forbid("เปิด CAR ได้เฉพาะผู้รับผิดชอบเครื่องมือ หรือผู้ตรวจสอบขึ้นไป"); }
    let Some(k) = c.query_opt(&format!("SELECT {KCOLS} FROM calibrations WHERE id=$1 AND equipment_id=$2"), &[&cid, &id]).await.ok().flatten().map(|r| cal_json(&r))
        else { return err(StatusCode::NOT_FOUND, "ไม่พบผลสอบเทียบนี้") };
    if k["result"] != json!("fail") { return bad("เปิด CAR ได้เฉพาะผลสอบเทียบที่ไม่ผ่าน"); }
    if !s(&k, "carRef").is_empty() { return bad(&format!("ผลสอบเทียบนี้เปิด {} ไว้แล้ว", s(&k, "carRef"))); }
    let since = previous_pass(&c, id, &k).await;
    let impact = match s(&k, "impactResult").as_str() {
        "affected" => format!(" · ผลกระทบ: กระทบผลการวัดตั้งแต่ {} — {} · การดำเนินการ: {}", if since.is_empty() { "เริ่มใช้งาน".into() } else { since }, s(&k, "impact"), s(&k, "impactAction")),
        "none" => format!(" · ผลกระทบ: ไม่กระทบ — {}", s(&k, "impact")),
        _ => " · ยังไม่ได้ประเมินผลกระทบต่อผลการวัดที่ผ่านมา".into(),
    };
    let f = capa::Fields {
        kind: Some("CAR".into()),
        title: Some(format!("ผลสอบเทียบไม่ผ่าน: {} {}", s(&e, "code"), s(&e, "name"))),
        detail: Some(format!("สอบเทียบ {} ใบรับรอง {} โดย {} · ผลที่พบ: {} · เกณฑ์ยอมรับ: {}{impact}", s(&k, "calDate"), s(&k, "certRef"), s(&k, "provider"),
            s(&k, "finding"), if s(&e, "acceptance").is_empty() { s(&e, "accuracy") } else { s(&e, "acceptance") })),
        dept: Some(s(&e, "dept")),
        source: Some("process".into()),
        assigneeCode: Some(s(&e, "ownerCode")),
        containment: Some(format!("หยุดใช้งาน {} และติดป้ายห้ามใช้{}", s(&e, "code"), if s(&k, "impactAction").is_empty() { String::new() } else { format!(" · {}", s(&k, "impactAction")) })),
        priority: Some(if k["impactResult"] == json!("affected") { "high".into() } else { "medium".into() }),
        linkedRef: Some(format!("{} สอบเทียบ {}", s(&e, "code"), s(&k, "calDate"))),
        ..Default::default()
    };
    let (_, ref_no) = match capa::open_action(&st, &c, &u, f).await { Ok(x) => x, Err(x) => return x };
    let _ = c.execute("UPDATE calibrations SET car_ref=$2 WHERE id=$1", &[&cid, &ref_no]).await;
    let _ = c.execute("UPDATE equipment SET version=version+1,updated_at=$2 WHERE id=$1", &[&id, &now_iso()]).await;
    event(&c, id, "car", &u, &format!("{ref_no} จากผลสอบเทียบ {}", s(&k, "calDate"))).await;
    let _ = eform::audit(&c, &u, "calibration_car", "equipment", &s(&e, "code"), &ref_no).await;
    detail_resp(&st, &u, id).await
}

#[derive(Deserialize)]
pub struct ActReq { version: i64, action: String, reason: Option<String> }

/// หยุดใช้งาน/กลับมาใช้งาน (ผู้รับผิดชอบหรือผู้ตรวจสอบ) · ยกเลิกใช้งาน/นำกลับเข้าทะเบียน (ผู้ตรวจสอบขึ้นไป) — พร้อมเหตุผล
pub async fn act(Path(id): Path<i64>, State(st): State<S>, Extension(u): Extension<AuthUser>, Json(b): Json<ActReq>) -> Response {
    let Ok(c) = st.db.get().await else { return db_err() };
    let Some(e) = load(&c, id).await else { return err(StatusCode::NOT_FOUND, "ไม่พบเครื่องมือนี้") };
    let can = can_flags(&u, &e);
    let reason = clip(&b.reason.unwrap_or_default(), 2000);
    if reason.is_empty() { return bad("กรุณาระบุเหตุผล"); }
    let allowed = can.get(b.action.as_str()) == Some(&json!(true));
    let status = match b.action.as_str() {
        "suspend" => "out_of_service",
        "resume" => "in_use",
        "retire" => "retired",
        // นำกลับเข้าทะเบียน = หยุดใช้งานไว้ก่อนจนกว่าจะสอบเทียบ/ตรวจสภาพ
        "reinstate" => "out_of_service",
        _ => return bad("การดำเนินการไม่ถูกต้อง"),
    };
    if !allowed {
        return forbid(match b.action.as_str() {
            "resume" => "กลับมาใช้งานได้เมื่อผลสอบเทียบล่าสุดผ่าน ยังไม่เลยกำหนด และประเมินผลกระทบครบแล้ว",
            "retire" | "reinstate" => "เฉพาะผู้ตรวจสอบขึ้นไป",
            _ => "เฉพาะผู้รับผิดชอบเครื่องมือ หรือผู้ตรวจสอบขึ้นไป",
        });
    }
    let n = c.execute("UPDATE equipment SET version=version+1,status=$3,status_reason=$4,updated_at=$5 WHERE id=$1 AND version=$2",
        &[&id, &b.version, &status.to_string(), &if status == "in_use" { String::new() } else { reason.clone() }, &now_iso()]).await.unwrap_or(0);
    if n == 0 { return stale(); }
    event(&c, id, &b.action, &u, &reason).await;
    let _ = eform::audit(&c, &u, &format!("equipment_{}", b.action), "equipment", &s(&e, "code"), &reason).await;
    detail_resp(&st, &u, id).await
}

// ---------- ใช้ร่วมกับโมดูลอื่น ----------

/// เครื่องมือที่ยังอยู่ในทะเบียน (ไม่รวมที่ยกเลิกใช้งาน) — หน้าเตรียม Audit · ทบทวนฝ่ายบริหาร · แจ้งเตือน
pub async fn active(c: &impl deadpool_postgres::GenericClient) -> Vec<Value> {
    c.query(&format!("SELECT {COLS} FROM equipment e WHERE e.status<>'retired' ORDER BY NULLIF(e.next_due,'') NULLS FIRST, e.code"), &[]).await
        .map(|rows| rows.iter().map(|r| { let mut x = row_json(r); x["gaps"] = json!(gaps(&x)); x }).collect()).unwrap_or_default()
}

/// ผลสอบเทียบที่ไม่ผ่านในรอบ 12 เดือน พร้อมรหัสเครื่องมือ (ทบทวนฝ่ายบริหาร)
pub async fn recent_fails(c: &impl deadpool_postgres::GenericClient, since: &str) -> Vec<Value> {
    c.query("SELECT e.code,e.name,k.cal_date,k.impact_result,k.car_ref,e.owner_name FROM calibrations k JOIN equipment e ON e.id=k.equipment_id
             WHERE k.result='fail' AND k.cal_date>=$1 ORDER BY k.cal_date DESC, k.id DESC", &[&since.to_string()]).await
        .map(|rows| rows.iter().map(|r| json!({ "code": r.get::<_, String>(0), "name": r.get::<_, String>(1), "calDate": r.get::<_, String>(2),
            "impactResult": r.get::<_, String>(3), "carRef": r.get::<_, String>(4), "ownerName": r.get::<_, String>(5) })).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn months() {
        assert_eq!(add_months("2026-01-31", 1), "2026-02-28");
        assert_eq!(add_months("2026-10-16", 12), "2027-10-16");
        assert_eq!(add_months("2026-10-16", 6), "2027-04-16");
        assert_eq!(add_months("bad", 6), "");
    }
    #[test]
    fn due_states() {
        assert_eq!(due_state("", "2026-10-16"), "none");
        assert_eq!(due_state("2026-10-15", "2026-10-16"), "overdue");
        assert_eq!(due_state("2026-10-16", "2026-10-16"), "soon");
        assert_eq!(due_state("2026-10-23", "2026-10-16"), "soon");
        assert_eq!(due_state("2026-10-24", "2026-10-16"), "ok");
        assert_eq!(days_left("2026-10-20", "2026-10-16"), Some(4));
    }
    #[test]
    fn impact_rules() {
        assert_eq!(impact_gaps("pass", "", "", ""), None);
        assert!(impact_gaps("fail", "", "", "").is_some());
        assert!(impact_gaps("fail", "none", "", " ").is_some());
        assert_eq!(impact_gaps("fail", "none", "", "ตรวจซ้ำงานล็อตล่าสุดด้วยเครื่องสำรอง ผลอยู่ในเกณฑ์"), None);
        assert!(impact_gaps("fail", "affected", "", "ค่าคลาดเคลื่อน +0.3 มม.").is_some());
        assert_eq!(impact_gaps("fail", "affected", "เรียกคืนล็อต 2610-01 ตรวจซ้ำ", "ค่าคลาดเคลื่อน +0.3 มม."), None);
    }
    #[test]
    fn register_gaps() {
        let ok = json!({ "name": "เวอร์เนียร์", "ownerCode": "1", "location": "QC", "dept": "", "rangeText": "0–150 มม.", "accuracy": "", "intervalMonths": 12 });
        assert!(gaps(&ok).is_empty());
        let bad = json!({ "name": "เวอร์เนียร์", "ownerCode": "", "location": "", "dept": "", "rangeText": "", "accuracy": "", "intervalMonths": 0 });
        assert_eq!(gaps(&bad), vec!["ผู้รับผิดชอบ", "สถานที่/แผนก", "ช่วงวัด/ความแม่นยำ", "รอบสอบเทียบ"]);
    }
}
