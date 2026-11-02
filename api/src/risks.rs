// ทะเบียนความเสี่ยงและโอกาส (ISO 9001:2015 ข้อ 6.1 — อ้างอิงบริบท 4.1 และผู้มีส่วนได้ส่วนเสีย 4.2)
//   ประเมิน โอกาสเกิด × ผลกระทบ (1–5) → ระดับ สูง ≥ 15 · กลาง 8–14 · ต่ำ ≤ 7
//   แผนจัดการ (6.1.2 a) + ผู้รับผิดชอบ + กำหนดทบทวน → ทบทวนประเมินซ้ำและประสิทธิผลของการจัดการ (6.1.2 b, 9.3.2 e)
//   ความเสี่ยงที่ต้องมีมาตรการเชิงระบบเปิด PAR ได้จากหน้าเดียวกัน (ผูกเลขที่ใบกลับไปที่ความเสี่ยง)
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
CREATE TABLE IF NOT EXISTS risks(
  id BIGSERIAL PRIMARY KEY,
  ref_no TEXT NOT NULL UNIQUE,
  version BIGINT NOT NULL DEFAULT 1,
  kind TEXT NOT NULL DEFAULT 'risk',
  title TEXT NOT NULL,
  detail TEXT NOT NULL DEFAULT '',
  context TEXT NOT NULL DEFAULT '',
  party TEXT NOT NULL DEFAULT '',
  process TEXT NOT NULL DEFAULT '',
  dept TEXT NOT NULL DEFAULT '',
  owner_code TEXT NOT NULL DEFAULT '',
  owner_name TEXT NOT NULL DEFAULT '',
  likelihood BIGINT NOT NULL DEFAULT 1,
  impact BIGINT NOT NULL DEFAULT 1,
  existing_controls TEXT NOT NULL DEFAULT '',
  treatment TEXT NOT NULL DEFAULT '',
  plan TEXT NOT NULL DEFAULT '',
  residual_likelihood BIGINT NOT NULL DEFAULT 0,
  residual_impact BIGINT NOT NULL DEFAULT 0,
  action_ref TEXT NOT NULL DEFAULT '',
  review_due TEXT NOT NULL DEFAULT '',
  last_review_at TEXT NOT NULL DEFAULT '',
  last_review_by TEXT NOT NULL DEFAULT '',
  effectiveness TEXT NOT NULL DEFAULT 'pending',
  status TEXT NOT NULL DEFAULT 'open',
  close_reason TEXT NOT NULL DEFAULT '',
  created_by TEXT NOT NULL,
  created_name TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS risk_reviews(
  id BIGSERIAL PRIMARY KEY,
  risk_id BIGINT NOT NULL,
  event TEXT NOT NULL,
  actor_code TEXT NOT NULL,
  actor_name TEXT NOT NULL,
  likelihood BIGINT NOT NULL DEFAULT 0,
  impact BIGINT NOT NULL DEFAULT 0,
  effectiveness TEXT NOT NULL DEFAULT '',
  note TEXT NOT NULL DEFAULT '',
  at TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS idx_rr_risk ON risk_reviews(risk_id);
";

const COLS: &str = "id,ref_no,version,kind,title,detail,context,party,process,dept,owner_code,owner_name,likelihood,impact,existing_controls,treatment,plan,
  residual_likelihood,residual_impact,action_ref,review_due,last_review_at,last_review_by,effectiveness,status,close_reason,created_by,created_name,created_at,updated_at";
const CONTEXTS: [&str; 5] = ["internal", "external", "party", "process", "other"];
const TREATMENTS_RISK: [&str; 4] = ["avoid", "reduce", "transfer", "accept"];
const TREATMENTS_OPP: [&str; 3] = ["exploit", "enhance", "accept"];
/// ทบทวนความเสี่ยงระดับสูงอย่างน้อยทุก 3 เดือน, ระดับอื่นปีละครั้ง (ค่าเริ่มต้นของกำหนดทบทวน)
const REVIEW_DAYS_HIGH: i64 = 90;
const REVIEW_DAYS_OTHER: i64 = 365;

fn db_err() -> Response { err(StatusCode::INTERNAL_SERVER_ERROR, "เชื่อมต่อฐานข้อมูลไม่ได้") }
fn bad(m: &str) -> Response { err(StatusCode::BAD_REQUEST, m) }
fn forbid(m: &str) -> Response { err(StatusCode::FORBIDDEN, m) }
fn s(v: &Value, k: &str) -> String { v[k].as_str().unwrap_or("").to_string() }
fn i(v: &Value, k: &str) -> i64 { v[k].as_i64().unwrap_or(0) }
fn clip(t: &str, n: usize) -> String { t.trim().chars().take(n).collect() }
fn valid_date(d: &str) -> bool { d.is_empty() || chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").is_ok() }

/// ระดับจากคะแนน (โอกาสเกิด × ผลกระทบ)
pub fn level(score: i64) -> &'static str {
    if score >= 15 { "high" } else if score >= 8 { "medium" } else if score > 0 { "low" } else { "" }
}
/// ชื่อระดับภาษาไทย
pub fn level_th(l: &str) -> &'static str {
    match l { "high" => "สูง", "medium" => "กลาง", "low" => "ต่ำ", _ => "-" }
}
fn add_days(days: i64) -> String {
    let d = chrono::NaiveDate::parse_from_str(&capa::today(), "%Y-%m-%d").unwrap() + chrono::Duration::days(days);
    d.format("%Y-%m-%d").to_string()
}

fn row_json(r: &tokio_postgres::Row) -> Value {
    let g = |k: usize| r.get::<_, String>(k);
    let n = |k: usize| r.get::<_, i64>(k);
    let (l, im, rl, ri) = (n(12), n(13), n(17), n(18));
    let review_due = g(20);
    let status = g(24);
    let today = capa::today();
    json!({
        "id": n(0), "refNo": g(1), "version": n(2), "kind": g(3), "title": g(4), "detail": g(5), "context": g(6), "party": g(7),
        "process": g(8), "dept": g(9), "ownerCode": g(10), "ownerName": g(11), "likelihood": l, "impact": im,
        "existingControls": g(14), "treatment": g(15), "plan": g(16), "residualLikelihood": rl, "residualImpact": ri,
        "actionRef": g(19), "reviewDue": review_due, "lastReviewAt": g(21), "lastReviewBy": g(22), "effectiveness": g(23),
        "status": status, "closeReason": g(25), "createdBy": g(26), "createdName": g(27), "createdAt": g(28), "updatedAt": g(29),
        "score": l * im, "level": level(l * im), "residualScore": rl * ri, "residualLevel": level(rl * ri),
        "reviewOverdue": status != "closed" && !review_due.is_empty() && review_due < today,
    })
}

fn can_flags(u: &AuthUser, r: &Value) -> Value {
    let approver = eform::can_approve(&u.role);
    let owner = s(r, "ownerCode") == u.emp_code;
    let open = s(r, "status") != "closed";
    let write = u.role != "viewer";
    json!({
        "edit": open && write && (owner || approver || s(r, "createdBy") == u.emp_code),
        "review": open && write && (owner || approver),
        "par": open && write && (owner || approver) && s(r, "actionRef").is_empty(),
        "close": open && approver,
        "reopen": !open && approver,
    })
}

/// เงื่อนไขขั้นต่ำของแถวในทะเบียน — ระดับกลางขึ้นไปต้องมีแผนจัดการ
fn gaps(r: &Value) -> Vec<&'static str> {
    let mut g = vec![];
    if s(r, "ownerCode").is_empty() { g.push("ผู้รับผิดชอบ"); }
    if s(r, "treatment").is_empty() { g.push("แนวทางจัดการ"); }
    if matches!(s(r, "level").as_str(), "high" | "medium") && s(r, "plan").trim().is_empty() && s(r, "treatment") != "accept" { g.push("แผนจัดการ/มาตรการ"); }
    if s(r, "reviewDue").is_empty() { g.push("กำหนดทบทวน"); }
    g
}

async fn load(c: &impl deadpool_postgres::GenericClient, id: i64) -> Option<Value> {
    let r = c.query_opt(&format!("SELECT {COLS} FROM risks WHERE id=$1"), &[&id]).await.ok()??;
    Some(row_json(&r))
}
async fn event(c: &impl deadpool_postgres::GenericClient, id: i64, ev: &str, u: &AuthUser, l: i64, im: i64, eff: &str, note: &str) {
    let _ = c.execute("INSERT INTO risk_reviews(risk_id,event,actor_code,actor_name,likelihood,impact,effectiveness,note,at) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)",
        &[&id, &ev.to_string(), &u.emp_code, &u.name, &l, &im, &eff.to_string(), &note.to_string(), &now_iso()]).await;
}

async fn detail_resp(st: &S, u: &AuthUser, id: i64) -> Response {
    let Ok(c) = st.db.get().await else { return db_err() };
    let Some(mut r) = load(&c, id).await else { return err(StatusCode::NOT_FOUND, "ไม่พบรายการนี้") };
    r["can"] = can_flags(u, &r);
    r["gaps"] = json!(gaps(&r));
    r["history"] = json!(c.query("SELECT event,actor_name,likelihood,impact,effectiveness,note,at FROM risk_reviews WHERE risk_id=$1 ORDER BY id", &[&id]).await
        .map(|rows| rows.iter().map(|x| json!({ "event": x.get::<_, String>(0), "actorName": x.get::<_, String>(1), "likelihood": x.get::<_, i64>(2),
            "impact": x.get::<_, i64>(3), "effectiveness": x.get::<_, String>(4), "note": x.get::<_, String>(5), "at": x.get::<_, String>(6) })).collect::<Vec<_>>())
        .unwrap_or_default());
    Json(json!({ "ok": true, "risk": r })).into_response()
}

// ---------- handlers ----------

pub async fn list(State(st): State<S>, Extension(u): Extension<AuthUser>) -> Response {
    let Ok(c) = st.db.get().await else { return db_err() };
    match c.query(&format!("SELECT {COLS} FROM risks ORDER BY (likelihood*impact) DESC, id"), &[]).await {
        Ok(rows) => Json(Value::Array(rows.iter().map(|r| { let mut x = row_json(r); x["can"] = can_flags(&u, &x); x["gaps"] = json!(gaps(&x)); x }).collect())).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, &format!("อ่านทะเบียนไม่ได้: {e}")),
    }
}

pub async fn get(Path(id): Path<i64>, State(st): State<S>, Extension(u): Extension<AuthUser>) -> Response { detail_resp(&st, &u, id).await }

#[derive(Deserialize, Default)]
#[allow(non_snake_case)]
pub struct Fields {
    kind: Option<String>, title: Option<String>, detail: Option<String>, context: Option<String>, party: Option<String>,
    process: Option<String>, dept: Option<String>, ownerCode: Option<String>, likelihood: Option<i64>, impact: Option<i64>,
    existingControls: Option<String>, treatment: Option<String>, plan: Option<String>, residualLikelihood: Option<i64>,
    residualImpact: Option<i64>, reviewDue: Option<String>,
}

async fn apply(c: &impl deadpool_postgres::GenericClient, r: &mut Value, f: Fields) -> Result<(), Response> {
    if let Some(k) = f.kind { r["kind"] = json!(if k == "opportunity" { "opportunity" } else { "risk" }); }
    if let Some(t) = f.title { let t = clip(&t, 300); if t.is_empty() { return Err(bad("ต้องระบุความเสี่ยง/โอกาส")); } r["title"] = json!(t); }
    if let Some(x) = f.context { if !x.is_empty() && !CONTEXTS.contains(&x.as_str()) { return Err(bad("ประเด็นบริบทไม่ถูกต้อง")); } r["context"] = json!(x); }
    for (k, v, n) in [("detail", f.detail, 5000), ("party", f.party, 200), ("process", f.process, 200), ("dept", f.dept, 20),
                      ("existingControls", f.existingControls, 3000), ("plan", f.plan, 5000)] {
        if let Some(x) = v { r[k] = json!(clip(&x, n)); }
    }
    if let Some(code) = f.ownerCode {
        let code = clip(&code, 20);
        let name: String = if code.is_empty() { String::new() } else {
            c.query_opt("SELECT name_th FROM employees WHERE emp_code=$1 AND status='active'", &[&code]).await.ok().flatten()
                .map(|x| x.get(0)).ok_or_else(|| bad("ไม่พบผู้รับผิดชอบในรายชื่อพนักงาน"))?
        };
        r["ownerCode"] = json!(code); r["ownerName"] = json!(name);
    }
    for (k, v, min) in [("likelihood", f.likelihood, 1), ("impact", f.impact, 1), ("residualLikelihood", f.residualLikelihood, 0), ("residualImpact", f.residualImpact, 0)] {
        if let Some(n) = v { if !(min..=5).contains(&n) { return Err(bad("คะแนนต้องอยู่ระหว่าง 1–5")); } r[k] = json!(n); }
    }
    if let Some(t) = f.treatment {
        let ok = t.is_empty() || if s(r, "kind") == "opportunity" { TREATMENTS_OPP.contains(&t.as_str()) } else { TREATMENTS_RISK.contains(&t.as_str()) };
        if !ok { return Err(bad("แนวทางจัดการไม่ถูกต้องสำหรับประเภทนี้")); }
        r["treatment"] = json!(t);
    }
    if let Some(d) = f.reviewDue { let d = clip(&d, 10); if !valid_date(&d) { return Err(bad("รูปแบบวันที่ไม่ถูกต้อง (YYYY-MM-DD)")); } r["reviewDue"] = json!(d); }
    let (l, im) = (i(r, "likelihood"), i(r, "impact"));
    r["score"] = json!(l * im);
    r["level"] = json!(level(l * im));
    Ok(())
}

/// บันทึกความเสี่ยง/โอกาสใหม่ (ผู้จัดทำขึ้นไป) — ไม่ระบุกำหนดทบทวน = ตามระดับ (สูง 3 เดือน, อื่น ๆ 12 เดือน)
pub async fn create(State(st): State<S>, Extension(u): Extension<AuthUser>, Json(f): Json<Fields>) -> Response {
    if u.role == "viewer" { return forbid("บัญชีผู้อ่านอย่างเดียวบันทึกไม่ได้"); }
    let Ok(c) = st.db.get().await else { return db_err() };
    if f.title.as_deref().map(|t| t.trim().is_empty()).unwrap_or(true) { return bad("ต้องระบุความเสี่ยง/โอกาส"); }
    let mut r = json!({ "kind": "risk", "likelihood": 1, "impact": 1, "residualLikelihood": 0, "residualImpact": 0, "status": "open" });
    if let Some(k) = &f.kind { r["kind"] = json!(if k == "opportunity" { "opportunity" } else { "risk" }); }
    if let Err(x) = apply(&c, &mut r, f).await { return x; }
    if s(&r, "reviewDue").is_empty() { r["reviewDue"] = json!(add_days(if s(&r, "level") == "high" { REVIEW_DAYS_HIGH } else { REVIEW_DAYS_OTHER })); }
    let g = gaps(&r);
    if !g.is_empty() { return bad(&format!("ยังขาด: {}", g.join(", "))); }
    let year = eform::be_year_now();
    let prefix = if s(&r, "kind") == "opportunity" { "OPP" } else { "RSK" };
    let seq: i64 = match c.query_one(
        "INSERT INTO form_seq(form_code,year_be,last_seq) VALUES($1,$2,1) ON CONFLICT(form_code,year_be) DO UPDATE SET last_seq=form_seq.last_seq+1 RETURNING last_seq",
        &[&prefix, &year],
    ).await { Ok(x) => x.get(0), Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()) };
    let ref_no = format!("{prefix}-{year}-{seq:03}");
    let now = now_iso();
    let res = c.query_one(
        "INSERT INTO risks(ref_no,kind,title,detail,context,party,process,dept,owner_code,owner_name,likelihood,impact,existing_controls,treatment,plan,
           residual_likelihood,residual_impact,review_due,created_by,created_name,created_at,updated_at)
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$21) RETURNING id",
        &[&ref_no, &s(&r, "kind"), &s(&r, "title"), &s(&r, "detail"), &s(&r, "context"), &s(&r, "party"), &s(&r, "process"), &s(&r, "dept"),
          &s(&r, "ownerCode"), &s(&r, "ownerName"), &i(&r, "likelihood"), &i(&r, "impact"), &s(&r, "existingControls"), &s(&r, "treatment"),
          &s(&r, "plan"), &i(&r, "residualLikelihood"), &i(&r, "residualImpact"), &s(&r, "reviewDue"), &u.emp_code, &u.name, &now],
    ).await;
    let id: i64 = match res { Ok(x) => x.get(0), Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, &format!("บันทึกไม่สำเร็จ: {e}")) };
    event(&c, id, "create", &u, i(&r, "likelihood"), i(&r, "impact"), "", &format!("ระดับ{}", level_th(&s(&r, "level")))).await;
    let _ = eform::audit(&c, &u, "risk_create", "risk", &ref_no, &s(&r, "title")).await;
    detail_resp(&st, &u, id).await
}

#[derive(Deserialize)]
pub struct UpdateReq { version: i64, fields: Fields }

pub async fn update(Path(id): Path<i64>, State(st): State<S>, Extension(u): Extension<AuthUser>, Json(b): Json<UpdateReq>) -> Response {
    let Ok(c) = st.db.get().await else { return db_err() };
    let Some(mut r) = load(&c, id).await else { return err(StatusCode::NOT_FOUND, "ไม่พบรายการนี้") };
    if can_flags(&u, &r)["edit"] != json!(true) { return forbid("แก้ไขได้เฉพาะผู้บันทึก ผู้รับผิดชอบ หรือผู้อนุมัติขึ้นไป ขณะยังไม่ปิด"); }
    if b.fields.kind.is_some() { return bad("เปลี่ยนประเภทไม่ได้ — บันทึกรายการใหม่แทน"); }
    let before = r.clone();
    if let Err(x) = apply(&c, &mut r, b.fields).await { return x; }
    let g = gaps(&r);
    if !g.is_empty() { return bad(&format!("ยังขาด: {}", g.join(", "))); }
    let n = c.execute(
        "UPDATE risks SET version=version+1,title=$3,detail=$4,context=$5,party=$6,process=$7,dept=$8,owner_code=$9,owner_name=$10,likelihood=$11,impact=$12,
           existing_controls=$13,treatment=$14,plan=$15,residual_likelihood=$16,residual_impact=$17,review_due=$18,updated_at=$19 WHERE id=$1 AND version=$2",
        &[&id, &b.version, &s(&r, "title"), &s(&r, "detail"), &s(&r, "context"), &s(&r, "party"), &s(&r, "process"), &s(&r, "dept"), &s(&r, "ownerCode"),
          &s(&r, "ownerName"), &i(&r, "likelihood"), &i(&r, "impact"), &s(&r, "existingControls"), &s(&r, "treatment"), &s(&r, "plan"),
          &i(&r, "residualLikelihood"), &i(&r, "residualImpact"), &s(&r, "reviewDue"), &now_iso()],
    ).await.unwrap_or(0);
    if n == 0 { return err(StatusCode::CONFLICT, "ข้อมูลถูกแก้ไขโดยผู้อื่นแล้ว — โหลดใหม่ก่อนบันทึก"); }
    let changed: Vec<&str> = ["title", "detail", "context", "party", "process", "dept", "ownerCode", "likelihood", "impact", "existingControls", "treatment", "plan", "residualLikelihood", "residualImpact", "reviewDue"]
        .into_iter().filter(|k| before[*k] != r[*k]).collect();
    if !changed.is_empty() {
        event(&c, id, "edit", &u, i(&r, "likelihood"), i(&r, "impact"), "", &changed.join(", ")).await;
        let _ = eform::audit(&c, &u, "risk_edit", "risk", &s(&r, "refNo"), &changed.join(", ")).await;
    }
    detail_resp(&st, &u, id).await
}

#[derive(Deserialize)]
#[allow(non_snake_case)]
pub struct ReviewReq { version: i64, likelihood: i64, impact: i64, effectiveness: String, note: String, nextReview: String }

/// ทบทวน: ประเมินซ้ำ + ประสิทธิผลของการจัดการ + กำหนดทบทวนครั้งถัดไป (ผู้รับผิดชอบหรือผู้อนุมัติขึ้นไป)
pub async fn review(Path(id): Path<i64>, State(st): State<S>, Extension(u): Extension<AuthUser>, Json(b): Json<ReviewReq>) -> Response {
    let Ok(c) = st.db.get().await else { return db_err() };
    let Some(r) = load(&c, id).await else { return err(StatusCode::NOT_FOUND, "ไม่พบรายการนี้") };
    if can_flags(&u, &r)["review"] != json!(true) { return forbid("ทบทวนได้เฉพาะผู้รับผิดชอบ หรือผู้อนุมัติขึ้นไป"); }
    if !(1..=5).contains(&b.likelihood) || !(1..=5).contains(&b.impact) { return bad("คะแนนต้องอยู่ระหว่าง 1–5"); }
    if !matches!(b.effectiveness.as_str(), "effective" | "not_effective" | "pending") { return bad("ผลประสิทธิผลไม่ถูกต้อง"); }
    let note = clip(&b.note, 3000);
    if note.is_empty() { return bad("กรุณาบันทึกผลการทบทวน/หลักฐาน"); }
    let next = clip(&b.nextReview, 10);
    if next.is_empty() || !valid_date(&next) || next <= capa::today() { return bad("กรุณากำหนดวันทบทวนครั้งถัดไป (หลังวันนี้)"); }
    let now = now_iso();
    let n = c.execute(
        "UPDATE risks SET version=version+1,likelihood=$3,impact=$4,effectiveness=$5,review_due=$6,last_review_at=$7,last_review_by=$8,
           status=CASE WHEN status='open' THEN 'monitoring' ELSE status END,updated_at=$7 WHERE id=$1 AND version=$2",
        &[&id, &b.version, &b.likelihood, &b.impact, &b.effectiveness, &next, &now, &u.name],
    ).await.unwrap_or(0);
    if n == 0 { return err(StatusCode::CONFLICT, "ข้อมูลถูกแก้ไขโดยผู้อื่นแล้ว — โหลดใหม่"); }
    let summary = format!("{}×{} ({}→{}) · {}", b.likelihood, b.impact, i(&r, "score"), b.likelihood * b.impact, note);
    event(&c, id, "review", &u, b.likelihood, b.impact, &b.effectiveness, &note).await;
    let _ = eform::audit(&c, &u, "risk_review", "risk", &s(&r, "refNo"), &summary).await;
    detail_resp(&st, &u, id).await
}

#[derive(Deserialize)]
pub struct ActReq { version: i64, action: String, reason: Option<String> }

/// ปิด (ความเสี่ยงหมดไป/ไม่เกี่ยวข้องแล้ว) หรือเปิดใหม่ — ผู้อนุมัติขึ้นไป พร้อมเหตุผล
pub async fn act(Path(id): Path<i64>, State(st): State<S>, Extension(u): Extension<AuthUser>, Json(b): Json<ActReq>) -> Response {
    let Ok(c) = st.db.get().await else { return db_err() };
    let Some(r) = load(&c, id).await else { return err(StatusCode::NOT_FOUND, "ไม่พบรายการนี้") };
    let can = can_flags(&u, &r);
    let reason = clip(&b.reason.unwrap_or_default(), 2000);
    if reason.is_empty() { return bad("กรุณาระบุเหตุผล"); }
    let (status, ev) = match b.action.as_str() {
        "close" if can["close"] == json!(true) => ("closed", "close"),
        "reopen" if can["reopen"] == json!(true) => ("open", "reopen"),
        "close" | "reopen" => return forbid("เฉพาะผู้อนุมัติขึ้นไป"),
        _ => return bad("การดำเนินการไม่ถูกต้อง"),
    };
    let n = c.execute("UPDATE risks SET version=version+1,status=$3,close_reason=$4,updated_at=$5 WHERE id=$1 AND version=$2",
        &[&id, &b.version, &status.to_string(), &if status == "closed" { reason.clone() } else { String::new() }, &now_iso()]).await.unwrap_or(0);
    if n == 0 { return err(StatusCode::CONFLICT, "ข้อมูลถูกแก้ไขโดยผู้อื่นแล้ว — โหลดใหม่"); }
    event(&c, id, ev, &u, i(&r, "likelihood"), i(&r, "impact"), "", &reason).await;
    let _ = eform::audit(&c, &u, &format!("risk_{ev}"), "risk", &s(&r, "refNo"), &reason).await;
    detail_resp(&st, &u, id).await
}

/// เปิด PAR สำหรับมาตรการจัดการความเสี่ยง (10.2 / 6.1.2) — ผูกเลขที่ใบกลับไปที่รายการ
pub async fn raise_par(Path(id): Path<i64>, State(st): State<S>, Extension(u): Extension<AuthUser>) -> Response {
    let Ok(c) = st.db.get().await else { return db_err() };
    let Some(r) = load(&c, id).await else { return err(StatusCode::NOT_FOUND, "ไม่พบรายการนี้") };
    if can_flags(&u, &r)["par"] != json!(true) { return forbid("เปิด PAR ได้เฉพาะผู้รับผิดชอบหรือผู้อนุมัติขึ้นไป และรายการที่ยังไม่มีใบผูกไว้"); }
    let f = capa::Fields {
        kind: Some("PAR".into()),
        title: Some(format!("มาตรการจัดการ{}: {}", if s(&r, "kind") == "opportunity" { "โอกาส" } else { "ความเสี่ยง" }, s(&r, "title"))),
        detail: Some(format!("{} · ระดับ{} ({}×{}){}", s(&r, "refNo"), level_th(&s(&r, "level")), i(&r, "likelihood"), i(&r, "impact"),
            if s(&r, "detail").is_empty() { String::new() } else { format!(" · {}", s(&r, "detail")) })),
        dept: Some(s(&r, "dept")),
        source: Some("process".into()),
        assigneeCode: Some(s(&r, "ownerCode")),
        rootCause: Some(s(&r, "detail")),
        actionTaken: Some(s(&r, "plan")),
        priority: Some(if s(&r, "level") == "high" { "high".into() } else { "medium".into() }),
        linkedRef: Some(s(&r, "refNo")),
        ..Default::default()
    };
    let (_, ref_no) = match capa::open_action(&st, &c, &u, f).await { Ok(x) => x, Err(x) => return x };
    let _ = c.execute("UPDATE risks SET version=version+1,action_ref=$2,updated_at=$3 WHERE id=$1", &[&id, &ref_no, &now_iso()]).await;
    event(&c, id, "par", &u, i(&r, "likelihood"), i(&r, "impact"), "", &ref_no).await;
    let _ = eform::audit(&c, &u, "risk_par", "risk", &s(&r, "refNo"), &ref_no).await;
    detail_resp(&st, &u, id).await
}

// ---------- ใช้ร่วมกับโมดูลอื่น ----------

/// รายการที่ยังไม่ปิดทั้งหมด (หน้าเตรียม Audit · ทบทวนฝ่ายบริหาร · แจ้งเตือน)
pub async fn open_items(c: &impl deadpool_postgres::GenericClient) -> Vec<Value> {
    c.query(&format!("SELECT {COLS} FROM risks WHERE status<>'closed' ORDER BY (likelihood*impact) DESC, id"), &[]).await
        .map(|rows| rows.iter().map(|r| { let mut x = row_json(r); x["gaps"] = json!(gaps(&x)); x }).collect()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn levels() {
        assert_eq!(level(25), "high");
        assert_eq!(level(15), "high");
        assert_eq!(level(14), "medium");
        assert_eq!(level(8), "medium");
        assert_eq!(level(7), "low");
        assert_eq!(level(0), "");
    }
    #[test]
    fn gaps_need_plan_for_medium() {
        let r = json!({ "ownerCode": "1", "treatment": "reduce", "level": "medium", "plan": "", "reviewDue": "2027-01-01" });
        assert_eq!(gaps(&r), vec!["แผนจัดการ/มาตรการ"]);
        let accepted = json!({ "ownerCode": "1", "treatment": "accept", "level": "high", "plan": "", "reviewDue": "2027-01-01" });
        assert!(gaps(&accepted).is_empty());
        let low = json!({ "ownerCode": "1", "treatment": "reduce", "level": "low", "plan": "", "reviewDue": "2027-01-01" });
        assert!(gaps(&low).is_empty());
    }
}
