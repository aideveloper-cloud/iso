// บันทึกแบบฟอร์มออนไลน์: login พนักงาน, schema, เลขที่, อนุมัติ, เช็คอิน
use crate::{call_converter, err, now_iso, S};
use axum::{
    body::Body,
    extract::{Extension, Path, Query, State},
    http::{header, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::{HashMap, HashSet}, path::PathBuf};

pub const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS employees(
  emp_code TEXT PRIMARY KEY,
  name_th TEXT NOT NULL,
  name_en TEXT NOT NULL DEFAULT '',
  position TEXT NOT NULL DEFAULT '',
  dept TEXT NOT NULL DEFAULT '',
  manager_code TEXT NOT NULL DEFAULT '',
  pin_hash TEXT NOT NULL,
  role TEXT NOT NULL DEFAULT 'preparer',
  status TEXT NOT NULL DEFAULT 'active',
  signature TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL);
ALTER TABLE IF EXISTS employees ADD COLUMN IF NOT EXISTS must_change_pin BOOLEAN NOT NULL DEFAULT false;
CREATE TABLE IF NOT EXISTS sessions(
  token TEXT PRIMARY KEY,
  emp_code TEXT NOT NULL,
  expires_at TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS form_schemas(
  form_code TEXT NOT NULL,
  form_rev TEXT NOT NULL,
  title TEXT NOT NULL,
  body TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  PRIMARY KEY(form_code, form_rev));
CREATE TABLE IF NOT EXISTS form_records(
  id BIGSERIAL PRIMARY KEY,
  form_code TEXT NOT NULL,
  form_rev TEXT NOT NULL,
  ref_no TEXT NOT NULL,
  status TEXT NOT NULL,
  title TEXT NOT NULL DEFAULT '',
  dept TEXT NOT NULL DEFAULT '',
  data TEXT NOT NULL,
  prepared_by TEXT NOT NULL,
  prepared_name TEXT NOT NULL,
  prepared_position TEXT NOT NULL DEFAULT '',
  rev_no BIGINT NOT NULL DEFAULT 1,
  family_id BIGINT NOT NULL DEFAULT 0,
  checkin_token TEXT NOT NULL DEFAULT '',
  cancel_reason TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  submitted_at TEXT NOT NULL DEFAULT '');
CREATE TABLE IF NOT EXISTS form_steps(
  id BIGSERIAL PRIMARY KEY,
  record_id BIGINT NOT NULL,
  step TEXT NOT NULL,
  action TEXT NOT NULL,
  actor_code TEXT NOT NULL,
  actor_name TEXT NOT NULL,
  actor_position TEXT NOT NULL DEFAULT '',
  comment TEXT NOT NULL DEFAULT '',
  at TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS form_files(
  id BIGSERIAL PRIMARY KEY,
  record_id BIGINT NOT NULL,
  filename TEXT NOT NULL,
  stored_name TEXT NOT NULL,
  created_at TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS training_log(
  id BIGSERIAL PRIMARY KEY,
  record_id BIGINT NOT NULL,
  emp_code TEXT NOT NULL DEFAULT '',
  name TEXT NOT NULL,
  course TEXT NOT NULL,
  train_date TEXT NOT NULL DEFAULT '',
  result TEXT NOT NULL DEFAULT '',
  ref_no TEXT NOT NULL,
  dept TEXT NOT NULL DEFAULT '');
CREATE TABLE IF NOT EXISTS audit_log(
  id BIGSERIAL PRIMARY KEY,
  actor_code TEXT NOT NULL,
  actor_name TEXT NOT NULL,
  action TEXT NOT NULL,
  entity TEXT NOT NULL,
  entity_id TEXT NOT NULL DEFAULT '',
  detail TEXT NOT NULL DEFAULT '',
  at TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS form_seq(
  form_code TEXT NOT NULL,
  year_be BIGINT NOT NULL,
  last_seq BIGINT NOT NULL,
  PRIMARY KEY(form_code, year_be));
CREATE INDEX IF NOT EXISTS idx_fr_code ON form_records(form_code);
CREATE INDEX IF NOT EXISTS idx_fr_status ON form_records(status);
CREATE INDEX IF NOT EXISTS idx_steps_rec ON form_steps(record_id);
";

const RCOLS: &str = "id,form_code,form_rev,ref_no,status,title,dept,data,prepared_by,prepared_name,prepared_position,rev_no,family_id,checkin_token,cancel_reason,created_at,updated_at,submitted_at";

#[derive(Clone, Debug)]
pub struct AuthUser {
    pub emp_code: String,
    pub name: String,
    pub position: String,
    pub dept: String,
    pub role: String,
    /// PIN ที่ผู้ดูแลตั้งให้/PIN เริ่มต้น — ต้องเปลี่ยนก่อนใช้งานส่วนอื่น
    pub must_change_pin: bool,
}

pub(crate) fn sha_hex(s: &str) -> String {
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}
/// แฮช PIN ด้วย Argon2id + salt สุ่มต่อคน (รูปแบบ PHC `$argon2id$...`)
pub(crate) fn pin_hash(_code: &str, pin: &str) -> String {
    use argon2::password_hash::{rand_core::OsRng, PasswordHasher, SaltString};
    let salt = SaltString::generate(&mut OsRng);
    argon2::Argon2::default().hash_password(pin.as_bytes(), &salt).map(|h| h.to_string()).unwrap_or_default()
}
/// แฮชรุ่นเก่า (SHA-256 ไม่มี salt) — ใช้ตรวจ PIN เดิมแล้วอัปเกรดเป็น Argon2 ตอนใช้งานครั้งถัดไปเท่านั้น
fn legacy_pin_hash(code: &str, pin: &str) -> String { sha_hex(&format!("iso-pin-v1:{code}:{pin}")) }

/// ตรวจ PIN กับค่าที่เก็บ; ถ้ายังเป็นแฮชรุ่นเก่าและถูกต้อง จะเขียนทับเป็น Argon2 ทันที
pub(crate) async fn check_pin(c: &impl deadpool_postgres::GenericClient, code: &str, pin: &str) -> bool {
    use argon2::password_hash::{PasswordHash, PasswordVerifier};
    let Some(row) = c.query_opt("SELECT pin_hash FROM employees WHERE emp_code=$1", &[&code]).await.ok().flatten() else { return false };
    let stored: String = row.get(0);
    if stored.starts_with("$argon2") {
        return PasswordHash::new(&stored).map(|h| argon2::Argon2::default().verify_password(pin.as_bytes(), &h).is_ok()).unwrap_or(false);
    }
    let legacy = legacy_pin_hash(code, pin);
    if stored.is_empty() || legacy.len() != stored.len() || !legacy.bytes().zip(stored.bytes()).fold(true, |ok, (a, b)| ok & (a == b)) { return false; }
    let upgraded = pin_hash(code, pin);
    if !upgraded.is_empty() {
        let _ = c.execute("UPDATE employees SET pin_hash=$2 WHERE emp_code=$1 AND pin_hash=$3", &[&code, &upgraded, &stored]).await;
    }
    true
}

/// token สุ่มจาก CSPRNG ของระบบปฏิบัติการ 256 บิต (session, check-in, ชื่อไฟล์)
pub(crate) fn new_token(_purpose: &str) -> String {
    use argon2::password_hash::rand_core::{OsRng, RngCore};
    let mut b = [0u8; 32];
    OsRng.fill_bytes(&mut b);
    b.iter().map(|x| format!("{x:02x}")).collect()
}
pub(crate) fn be_year_now() -> i64 {
    let tz = chrono::FixedOffset::east_opt(7 * 3600).unwrap();
    chrono::Utc::now().with_timezone(&tz).format("%Y").to_string().parse::<i64>().unwrap_or(2026) + 543
}
fn th_date(iso: &str) -> String {
    let p: Vec<&str> = iso.split('-').collect();
    if p.len() != 3 { return iso.to_string(); }
    let y: i64 = p[0].parse().unwrap_or(0);
    let m: usize = p[1].parse().unwrap_or(0);
    let months = ["", "ม.ค.", "ก.พ.", "มี.ค.", "เม.ย.", "พ.ค.", "มิ.ย.", "ก.ค.", "ส.ค.", "ก.ย.", "ต.ค.", "พ.ย.", "ธ.ค."];
    if !(1..=12).contains(&m) { return iso.to_string(); }
    let d = p[2].trim_start_matches('0');
    format!("{} {} {}", if d.is_empty() { "0" } else { d }, months[m], y + 543)
}
fn th_when(rfc: &str) -> String {
    if rfc.is_empty() { return String::new(); }
    let Ok(dt) = chrono::DateTime::parse_from_rfc3339(rfc) else { return rfc.to_string(); };
    let tz = chrono::FixedOffset::east_opt(7 * 3600).unwrap();
    let local = dt.with_timezone(&tz);
    let y: i64 = local.format("%Y").to_string().parse().unwrap_or(0) + 543;
    let mo: usize = local.format("%m").to_string().parse().unwrap_or(0);
    format!("{} {} {y} {} น.", local.format("%d"), th_month(mo), local.format("%H:%M"))
}
fn th_month(m: usize) -> &'static str {
    ["", "ม.ค.", "ก.พ.", "มี.ค.", "เม.ย.", "พ.ค.", "มิ.ย.", "ก.ค.", "ส.ค.", "ก.ย.", "ต.ค.", "พ.ย.", "ธ.ค."].get(m).copied().unwrap_or("")
}

pub(crate) fn can_review(role: &str) -> bool { matches!(role, "reviewer" | "approver" | "admin") }
pub(crate) fn can_approve(role: &str) -> bool { matches!(role, "approver" | "admin") }
pub(crate) fn is_admin(role: &str) -> bool { role == "admin" }
fn editable_status(s: &str) -> bool { matches!(s, "draft" | "returned") }

pub(crate) fn limited(st: &S, key: &str) -> bool {
    let now = chrono::Utc::now().timestamp();
    let g = st.login_hits.lock().unwrap_or_else(|e| e.into_inner());
    g.get(key).map(|v| v.iter().filter(|t| now - *t < 600).count() >= 8).unwrap_or(false)
}
pub(crate) fn mark_fail(st: &S, key: &str) {
    let now = chrono::Utc::now().timestamp();
    let mut g = st.login_hits.lock().unwrap_or_else(|e| e.into_inner());
    let v = g.entry(key.to_string()).or_default();
    v.retain(|t| now - *t < 600);
    v.push(now);
}
pub(crate) fn clear_fails(st: &S, key: &str) {
    let mut g = st.login_hits.lock().unwrap_or_else(|e| e.into_inner());
    g.remove(key);
}

pub async fn user_from_token(st: &S, token: &str) -> Option<AuthUser> {
    if token.is_empty() { return None; }
    let client = st.db.get().await.ok()?;
    let now = now_iso();
    let row = client.query_opt(
        "SELECT e.emp_code,e.name_th,e.position,e.dept,e.role,e.must_change_pin FROM sessions s JOIN employees e ON e.emp_code=s.emp_code WHERE s.token=$1 AND s.expires_at>$2 AND e.status='active'",
        &[&token, &now],
    ).await.ok()??;
    Some(AuthUser {
        emp_code: row.get(0), name: row.get(1), position: row.get(2), dept: row.get(3), role: row.get(4), must_change_pin: row.get(5),
    })
}

pub async fn require_auth(State(st): State<S>, mut req: Request<Body>, next: Next) -> Response {
    let path = req.uri().path().to_string();
    if path == "/api/health" || path == "/api/auth/login" || path.starts_with("/api/public/") {
        return next.run(req).await;
    }
    let header_token = req.headers().get("authorization").and_then(|h| h.to_str().ok()).unwrap_or("");
    let header_token = header_token.strip_prefix("Bearer ").unwrap_or("").trim().to_string();
    // คุกกี้ HttpOnly ใช้ได้เฉพาะคำขออ่าน (GET/HEAD: ดาวน์โหลด/พรีวิวไฟล์) — การแก้ไขข้อมูลต้องมี Authorization header เสมอ (กัน CSRF)
    let readonly = matches!(*req.method(), axum::http::Method::GET | axum::http::Method::HEAD);
    let token = if !header_token.is_empty() { header_token } else if readonly { cookie_token(req.headers()) } else { String::new() };
    match user_from_token(&st, &token).await {
        Some(u) => {
            if u.must_change_pin && !matches!(path.as_str(), "/api/auth/me" | "/api/auth/pin" | "/api/auth/logout") {
                return (StatusCode::FORBIDDEN, Json(json!({ "error": "กรุณาเปลี่ยน PIN ก่อนใช้งาน", "mustChangePin": true }))).into_response();
            }
            req.extensions_mut().insert(u);
            next.run(req).await
        }
        None => err(StatusCode::UNAUTHORIZED, "กรุณาเข้าสู่ระบบ"),
    }
}

const COOKIE: &str = "iso_sid";

fn cookie_token(h: &axum::http::HeaderMap) -> String {
    h.get_all(header::COOKIE).iter().filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .find_map(|p| p.trim().strip_prefix("iso_sid=").map(|t| t.to_string()))
        .unwrap_or_default()
}

/// ผ่าน HTTPS (Cloudflare) หรือไม่ — ใส่ Secure ให้คุกกี้เฉพาะตอนเป็น HTTPS เพื่อให้ใช้ใน LAN แบบ http ได้
fn is_https(h: &axum::http::HeaderMap) -> bool {
    h.get("x-forwarded-proto").and_then(|v| v.to_str().ok()).map(|v| v.eq_ignore_ascii_case("https")).unwrap_or(false)
        || h.get("cf-visitor").and_then(|v| v.to_str().ok()).map(|v| v.contains("https")).unwrap_or(false)
}

/// คุกกี้ session สำหรับดาวน์โหลด/พรีวิวไฟล์ (HttpOnly · SameSite=Strict · Path=/api · อายุเท่าเซสชัน 8 ชม.)
fn session_cookie(token: &str, secure: bool, max_age: i64) -> String {
    format!("{COOKIE}={token}; HttpOnly; SameSite=Strict; Path=/api; Max-Age={max_age}{}", if secure { "; Secure" } else { "" })
}

fn user_json(u: &AuthUser) -> Value {
    json!({ "empCode": u.emp_code, "name": u.name, "position": u.position, "dept": u.dept, "role": u.role, "mustChangePin": u.must_change_pin })
}

async fn pending_count(st: &S, u: &AuthUser) -> i64 {
    let Ok(client) = st.db.get().await else { return 0 };
    let Ok(rows) = client.query("SELECT status, prepared_by FROM form_records WHERE status IN ('review','approve')", &[]).await else { return 0 };
    rows.iter().filter(|r| {
        let status: String = r.get(0);
        let by: String = r.get(1);
        if !is_admin(&u.role) && by == u.emp_code { return false; }
        (status == "review" && can_review(&u.role)) || (status == "approve" && can_approve(&u.role))
    }).count() as i64
}

#[derive(Deserialize)]
#[allow(non_snake_case)]
pub struct LoginReq { empCode: String, pin: String }

pub async fn login(State(st): State<S>, headers: axum::http::HeaderMap, Json(b): Json<LoginReq>) -> Response {
    let code = b.empCode.trim().to_string();
    let pin = b.pin.trim().to_string();
    if limited(&st, &code) { return err(StatusCode::TOO_MANY_REQUESTS, "ลองเข้าสู่ระบบบ่อยเกินไป รอสักครู่"); }
    if !pin.chars().all(|c| c.is_ascii_digit()) || pin.len() != 6 {
        mark_fail(&st, &code);
        return err(StatusCode::BAD_REQUEST, "PIN ต้องเป็นตัวเลข 6 หลัก");
    }
    let client = match st.db.get().await { Ok(c) => c, Err(_) => return err(StatusCode::INTERNAL_SERVER_ERROR, "เชื่อมต่อฐานข้อมูลไม่ได้") };
    let row = if check_pin(&client, &code, &pin).await {
        client.query_opt("SELECT emp_code,name_th,position,dept,role,status,must_change_pin FROM employees WHERE emp_code=$1", &[&code]).await.ok().flatten()
    } else { None };
    let Some(row) = row else {
        mark_fail(&st, &code);
        return err(StatusCode::UNAUTHORIZED, "รหัสพนักงานหรือ PIN ไม่ถูกต้อง");
    };
    let status: String = row.get(5);
    if status != "active" { return err(StatusCode::FORBIDDEN, "บัญชีนี้ไม่ได้ใช้งานแล้ว"); }
    clear_fails(&st, &code);
    let user = AuthUser { emp_code: row.get(0), name: row.get(1), position: row.get(2), dept: row.get(3), role: row.get(4), must_change_pin: row.get(6) };
    let token = new_token(&user.emp_code);
    let exp = (chrono::Utc::now() + chrono::Duration::hours(8)).to_rfc3339();
    if client.execute("INSERT INTO sessions(token,emp_code,expires_at) VALUES($1,$2,$3)", &[&token, &user.emp_code, &exp]).await.is_err() {
        return err(StatusCode::INTERNAL_SERVER_ERROR, "สร้างเซสชันไม่ได้");
    }
    let _ = audit(&client, &user, "login", "session", &user.emp_code, "").await;
    let cookie = session_cookie(&token, is_https(&headers), 8 * 3600);
    ([(header::SET_COOKIE, cookie)], Json(json!({ "ok": true, "token": token, "user": user_json(&user) }))).into_response()
}

pub async fn logout(State(st): State<S>, req: Request<Body>) -> Response {
    let token = req.headers().get("authorization").and_then(|h| h.to_str().ok()).unwrap_or("");
    let token = token.strip_prefix("Bearer ").unwrap_or("").to_string();
    if let Ok(client) = st.db.get().await {
        let _ = client.execute("DELETE FROM sessions WHERE token=$1", &[&token]).await;
    }
    ([(header::SET_COOKIE, session_cookie("", is_https(req.headers()), 0))], Json(json!({ "ok": true }))).into_response()
}

pub async fn me(State(st): State<S>, Extension(u): Extension<AuthUser>, headers: axum::http::HeaderMap) -> Response {
    let pending = pending_count(&st, &u).await;
    let pending_docs = crate::doccontrol::pending_count(&st, &u).await;
    let pending_acks = crate::acks::pending_count(&st, &u).await;
    let body = Json(json!({ "user": user_json(&u), "pending": pending, "pendingDocs": pending_docs, "pendingAcks": pending_acks }));
    // เซสชันที่ login ก่อนเปลี่ยนมาใช้คุกกี้: ออกคุกกี้ให้จาก Bearer token เดิม (อายุไม่เกินเวลาที่เหลือของเซสชัน)
    let bearer = headers.get("authorization").and_then(|h| h.to_str().ok()).and_then(|h| h.strip_prefix("Bearer ")).unwrap_or("").trim().to_string();
    if !bearer.is_empty() && cookie_token(&headers) != bearer {
        if let Ok(c) = st.db.get().await {
            if let Ok(Some(r)) = c.query_opt("SELECT expires_at FROM sessions WHERE token=$1", &[&bearer]).await {
                let left = chrono::DateTime::parse_from_rfc3339(&r.get::<_, String>(0)).map(|e| (e.with_timezone(&chrono::Utc) - chrono::Utc::now()).num_seconds()).unwrap_or(0);
                if left > 0 { return ([(header::SET_COOKIE, session_cookie(&bearer, is_https(&headers), left))], body).into_response(); }
            }
        }
    }
    body.into_response()
}

/// ยืนยันตัวตนซ้ำก่อนลงนามอิเล็กทรอนิกส์ (กรอก PIN อีกครั้ง) — นับครั้งที่ผิดร่วมกับการกันเดา PIN
/// ตอบ 400 ไม่ใช่ 401 เพื่อไม่ให้หน้าเว็บตีความว่าเซสชันหมดอายุ
pub(crate) async fn verify_pin(st: &S, c: &impl deadpool_postgres::GenericClient, u: &AuthUser, pin: &str) -> Result<(), Response> {
    let key = format!("pin:{}", u.emp_code);
    if limited(st, &key) { return Err(err(StatusCode::TOO_MANY_REQUESTS, "กรอก PIN ผิดบ่อยเกินไป รอสักครู่")); }
    let pin = pin.trim();
    if pin.is_empty() { return Err(err(StatusCode::BAD_REQUEST, "กรุณากรอก PIN เพื่อยืนยันการลงนาม")); }
    let active = c.query_opt("SELECT 1 FROM employees WHERE emp_code=$1 AND status='active'", &[&u.emp_code]).await.ok().flatten().is_some();
    if !active || !check_pin(c, &u.emp_code, pin).await {
        mark_fail(st, &key);
        return Err(err(StatusCode::BAD_REQUEST, "PIN ไม่ถูกต้อง — ลงนามไม่สำเร็จ"));
    }
    clear_fails(st, &key);
    Ok(())
}

#[derive(Deserialize)]
pub struct PinReq { old: String, new: String }

/// PIN ที่เดาง่าย: เลขซ้ำทั้งหมด, เรียงขึ้น/ลง, ตรงกับท้ายรหัสพนักงาน
pub(crate) fn weak_pin(code: &str, pin: &str) -> bool {
    let d: Vec<i32> = pin.bytes().map(|b| (b - b'0') as i32).collect();
    let same = d.windows(2).all(|w| w[0] == w[1]);
    let up = d.windows(2).all(|w| (w[1] - w[0]).rem_euclid(10) == 1);
    let down = d.windows(2).all(|w| (w[0] - w[1]).rem_euclid(10) == 1);
    same || up || down || (code.len() >= 4 && pin.ends_with(&code[code.len() - 4..]) && code.ends_with(&pin[2..]))
}

pub async fn change_pin(State(st): State<S>, Extension(u): Extension<AuthUser>, Json(b): Json<PinReq>) -> Response {
    if b.new.len() != 6 || !b.new.chars().all(|c| c.is_ascii_digit()) {
        return err(StatusCode::BAD_REQUEST, "PIN ใหม่ต้องเป็นตัวเลข 6 หลัก");
    }
    if weak_pin(&u.emp_code, &b.new) { return err(StatusCode::BAD_REQUEST, "PIN นี้เดาง่ายเกินไป (เลขซ้ำ/เรียงกัน/ตรงกับรหัสพนักงาน) — เลือกใหม่"); }
    if b.new == b.old.trim() { return err(StatusCode::BAD_REQUEST, "PIN ใหม่ต้องไม่ซ้ำกับ PIN เดิม"); }
    let client = match st.db.get().await { Ok(c) => c, Err(_) => return err(StatusCode::INTERNAL_SERVER_ERROR, "เชื่อมต่อฐานข้อมูลไม่ได้") };
    let key = format!("pin:{}", u.emp_code);
    if limited(&st, &key) { return err(StatusCode::TOO_MANY_REQUESTS, "กรอก PIN ผิดบ่อยเกินไป รอสักครู่"); }
    if !check_pin(&client, &u.emp_code, b.old.trim()).await { mark_fail(&st, &key); return err(StatusCode::BAD_REQUEST, "PIN เดิมไม่ถูกต้อง"); }
    clear_fails(&st, &key);
    let new_h = pin_hash(&u.emp_code, &b.new);
    let _ = client.execute("UPDATE employees SET pin_hash=$2, must_change_pin=false WHERE emp_code=$1", &[&u.emp_code, &new_h]).await;
    let _ = audit(&client, &u, "pin_change", "employee", &u.emp_code, if u.must_change_pin { "เปลี่ยน PIN ที่ได้รับ (บังคับ)" } else { "เปลี่ยน PIN" }).await;
    let _ = client.execute("DELETE FROM sessions WHERE emp_code=$1", &[&u.emp_code]).await;
    Json(json!({ "ok": true })).into_response()
}

fn emp_json(r: &tokio_postgres::Row) -> Value {
    json!({
        "empCode": r.get::<_, String>(0), "nameTh": r.get::<_, String>(1), "nameEn": r.get::<_, String>(2),
        "position": r.get::<_, String>(3), "dept": r.get::<_, String>(4), "managerCode": r.get::<_, String>(5),
        "role": r.get::<_, String>(6), "status": r.get::<_, String>(7), "internalAuditor": r.get::<_, bool>(8),
        "mustChangePin": r.get::<_, bool>(9),
    })
}

pub async fn employees_list(State(st): State<S>, Extension(u): Extension<AuthUser>, Query(q): Query<HashMap<String, String>>) -> Response {
    let client = match st.db.get().await { Ok(c) => c, Err(_) => return err(StatusCode::INTERNAL_SERVER_ERROR, "เชื่อมต่อฐานข้อมูลไม่ได้") };
    let needle = format!("%{}%", q.get("q").map(|s| s.trim()).unwrap_or(""));
    let dept = q.get("dept").cloned().unwrap_or_default();
    // ผู้ดูแลระบบดูบัญชีที่ปิดใช้งานแล้วได้ด้วย (หน้า ผู้ใช้และสิทธิ์)
    let all = is_admin(&u.role) && q.get("all").map(|v| v == "1").unwrap_or(false);
    let sql = "SELECT emp_code,name_th,name_en,position,dept,manager_code,role,status,internal_auditor,must_change_pin FROM employees WHERE ($3 OR status='active') AND ($2='' OR dept=$2) AND (emp_code ILIKE $1 OR name_th ILIKE $1 OR name_en ILIKE $1) ORDER BY emp_code LIMIT 1000";
    match client.query(sql, &[&needle, &dept, &all]).await {
        Ok(rows) => Json(Value::Array(rows.iter().map(emp_json).collect())).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, &format!("อ่านพนักงานไม่ได้: {e}")),
    }
}

#[derive(Deserialize)]
#[allow(non_snake_case)]
pub struct EmpItem {
    empCode: Option<String>, nameTh: Option<String>, nameEn: Option<String>, position: Option<String>,
    dept: Option<String>, role: Option<String>, status: Option<String>, pin: Option<String>, managerCode: Option<String>,
}

#[derive(Deserialize)]
#[allow(non_snake_case)]
pub struct EmpBody {
    empCode: Option<String>, nameTh: Option<String>, nameEn: Option<String>, position: Option<String>,
    dept: Option<String>, role: Option<String>, status: Option<String>, pin: Option<String>, managerCode: Option<String>,
    employees: Option<Vec<EmpItem>>, csv: Option<String>,
}

fn valid_role(r: &str) -> bool { matches!(r, "preparer" | "reviewer" | "approver" | "admin" | "viewer") }

async fn upsert_emp(client: &tokio_postgres::Client, code: &str, name: &str, name_en: &str, position: &str, dept: &str, role: &str, status: &str, manager: &str, pin: Option<&str>) -> Result<(), String> {
    if code.trim().is_empty() || name.trim().is_empty() { return Err("ต้องมีรหัสและชื่อ".into()); }
    let role = if valid_role(role) { role } else { "preparer" };
    let status = if status == "inactive" { "inactive" } else { "active" };
    let now = now_iso();
    let pin_v = match pin {
        Some(p) if p.len() == 6 && p.chars().all(|c| c.is_ascii_digit()) => pin_hash(code.trim(), p),
        Some(_) => return Err(format!("PIN ของ {code} ต้องเป็นตัวเลข 6 หลัก")),
        None => String::new(),
    };
    if pin_v.is_empty() {
        let n = client.execute(
            "UPDATE employees SET name_th=$2,name_en=$3,position=$4,dept=$5,role=$6,status=$7,manager_code=$8 WHERE emp_code=$1",
            &[&code.trim().to_string(), &name.trim().to_string(), &name_en.to_string(), &position.to_string(), &dept.to_string(), &role.to_string(), &status.to_string(), &manager.to_string()],
        ).await.map_err(|e| e.to_string())?;
        if n == 0 { return Err(format!("ไม่พบพนักงาน {code}")); }
    } else {
        client.execute(
            "INSERT INTO employees(emp_code,name_th,name_en,position,dept,manager_code,pin_hash,role,status,signature,created_at,must_change_pin) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,'',$10,true)
             ON CONFLICT(emp_code) DO UPDATE SET name_th=$2,name_en=$3,position=$4,dept=$5,manager_code=$6,pin_hash=$7,role=$8,status=$9,must_change_pin=true",
            &[&code.trim().to_string(), &name.trim().to_string(), &name_en.to_string(), &position.to_string(), &dept.to_string(), &manager.to_string(), &pin_v, &role.to_string(), &status.to_string(), &now],
        ).await.map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub async fn employees_save(State(st): State<S>, Extension(u): Extension<AuthUser>, Json(b): Json<EmpBody>) -> Response {
    if !is_admin(&u.role) { return err(StatusCode::FORBIDDEN, "เฉพาะผู้ดูแลระบบ"); }
    let client = match st.db.get().await { Ok(c) => c, Err(_) => return err(StatusCode::INTERNAL_SERVER_ERROR, "เชื่อมต่อฐานข้อมูลไม่ได้") };
    let mut n = 0;
    if let Some(csv) = b.csv {
        for line in csv.lines() {
            let line = line.trim();
            if line.is_empty() || line.to_lowercase().starts_with("emp") { continue; }
            let c: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
            if c.len() < 2 { continue; }
            let pin = c.get(6).copied().filter(|s| !s.is_empty());
            if upsert_emp(&client, c[0], c[1], c.get(2).copied().unwrap_or(""), c.get(3).copied().unwrap_or(""), c.get(4).copied().unwrap_or(""), c.get(5).copied().unwrap_or("preparer"), "active", "", pin).await.is_err() {
                return err(StatusCode::BAD_REQUEST, &format!("นำเข้าไม่ได้ที่บรรทัด {}", c[0]));
            }
            n += 1;
        }
    } else if let Some(list) = b.employees {
        for e in list {
            if upsert_emp(&client, e.empCode.as_deref().unwrap_or(""), e.nameTh.as_deref().unwrap_or(""), e.nameEn.as_deref().unwrap_or(""), e.position.as_deref().unwrap_or(""), e.dept.as_deref().unwrap_or(""), e.role.as_deref().unwrap_or("preparer"), e.status.as_deref().unwrap_or("active"), e.managerCode.as_deref().unwrap_or(""), e.pin.as_deref()).await.is_err() {
                return err(StatusCode::BAD_REQUEST, "นำเข้ารายชื่อไม่ได้");
            }
            n += 1;
        }
    } else if let Some(code) = b.empCode.clone() {
        // กันระบบไม่มีผู้ดูแล: ห้ามลดสิทธิ์/ปิดบัญชี admin คนสุดท้ายที่ยังใช้งาน
        let demote = b.role.as_deref().map(|r| r != "admin").unwrap_or(false) || b.status.as_deref() == Some("inactive");
        if demote {
            let is_adm = client.query_opt("SELECT 1 FROM employees WHERE emp_code=$1 AND role='admin' AND status='active'", &[&code]).await.ok().flatten().is_some();
            let admins: i64 = client.query_one("SELECT COUNT(*) FROM employees WHERE role='admin' AND status='active'", &[]).await.map(|r| r.get(0)).unwrap_or(0);
            if is_adm && admins <= 1 { return err(StatusCode::BAD_REQUEST, "ต้องมีผู้ดูแลระบบที่ใช้งานอยู่อย่างน้อย 1 คน"); }
        }
        if upsert_emp(&client, &code, b.nameTh.as_deref().unwrap_or(""), b.nameEn.as_deref().unwrap_or(""), b.position.as_deref().unwrap_or(""), b.dept.as_deref().unwrap_or(""), b.role.as_deref().unwrap_or("preparer"), b.status.as_deref().unwrap_or("active"), b.managerCode.as_deref().unwrap_or(""), b.pin.as_deref()).await.is_err() {
            return err(StatusCode::BAD_REQUEST, "บันทึกพนักงานไม่ได้");
        }
        n = 1;
    }
    let detail = match &b.empCode {
        Some(code) => format!("{code} role={} status={}{}", b.role.as_deref().unwrap_or("-"), b.status.as_deref().unwrap_or("-"), if b.pin.is_some() { " ตั้ง PIN ใหม่" } else { "" }),
        None => format!("{n} ราย"),
    };
    let _ = audit(&client, &u, "employee_save", "employee", b.empCode.as_deref().unwrap_or(""), &detail).await;
    Json(json!({ "ok": true, "count": n })).into_response()
}

pub(crate) async fn audit(c: &impl deadpool_postgres::GenericClient, u: &AuthUser, action: &str, entity: &str, id: &str, detail: &str) -> Result<(), String> {
    c.execute(
        "INSERT INTO audit_log(actor_code,actor_name,action,entity,entity_id,detail,at) VALUES($1,$2,$3,$4,$5,$6,$7)",
        &[&u.emp_code, &u.name, &action.to_string(), &entity.to_string(), &id.to_string(), &detail.to_string(), &now_iso()],
    ).await.map(|_| ()).map_err(|e| e.to_string())
}

async fn load_schema(c: &impl deadpool_postgres::GenericClient, code: &str) -> Result<Value, String> {
    let row = c.query_opt("SELECT body FROM form_schemas WHERE form_code=$1 ORDER BY form_rev DESC LIMIT 1", &[&code])
        .await.map_err(|e| e.to_string())?;
    let Some(row) = row else { return Err(format!("ยังไม่มี schema ของ {code}")); };
    let body: String = row.get(0);
    serde_json::from_str(&body).map_err(|_| "schema เสียหาย".into())
}

fn schema_summary(body: &str) -> Value {
    let v: Value = serde_json::from_str(body).unwrap_or(json!({}));
    json!({
        "formCode": v["form_code"], "formRev": v["form_rev"], "title": v["title"], "effective": v["effective"],
    })
}

pub async fn schemas_list(State(st): State<S>) -> Response {
    let client = match st.db.get().await { Ok(c) => c, Err(_) => return err(StatusCode::INTERNAL_SERVER_ERROR, "เชื่อมต่อฐานข้อมูลไม่ได้") };
    match client.query("SELECT body FROM form_schemas ORDER BY form_code", &[]).await {
        Ok(rows) => Json(Value::Array(rows.iter().map(|r| schema_summary(&r.get::<_, String>(0))).collect())).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

pub async fn schema_get(State(st): State<S>, Path(code): Path<String>) -> Response {
    let client = match st.db.get().await { Ok(c) => c, Err(_) => return err(StatusCode::INTERNAL_SERVER_ERROR, "เชื่อมต่อฐานข้อมูลไม่ได้") };
    match load_schema(&client, &code).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => err(StatusCode::NOT_FOUND, &e),
    }
}

pub async fn schema_save(State(st): State<S>, Extension(u): Extension<AuthUser>, Json(b): Json<Value>) -> Response {
    if !is_admin(&u.role) { return err(StatusCode::FORBIDDEN, "เฉพาะผู้ดูแลระบบ"); }
    let code = b["form_code"].as_str().unwrap_or("").trim().to_string();
    let rev = b["form_rev"].as_str().unwrap_or("").trim().to_string();
    let title = b["title"].as_str().unwrap_or("").trim().to_string();
    if code.is_empty() || rev.is_empty() || title.is_empty() { return err(StatusCode::BAD_REQUEST, "schema ต้องมี form_code, form_rev, title"); }
    let client = match st.db.get().await { Ok(c) => c, Err(_) => return err(StatusCode::INTERNAL_SERVER_ERROR, "เชื่อมต่อฐานข้อมูลไม่ได้") };
    let body = b.to_string();
    let now = now_iso();
    if client.execute(
        "INSERT INTO form_schemas(form_code,form_rev,title,body,updated_at) VALUES($1,$2,$3,$4,$5) ON CONFLICT(form_code,form_rev) DO UPDATE SET title=$3,body=$4,updated_at=$5",
        &[&code, &rev, &title, &body, &now],
    ).await.is_err() { return err(StatusCode::INTERNAL_SERVER_ERROR, "บันทึก schema ไม่ได้"); }
    let _ = audit(&client, &u, "schema_save", "schema", &code, &rev).await;
    Json(json!({ "ok": true })).into_response()
}

fn parse_data(s: &str) -> Value { serde_json::from_str(s).unwrap_or(json!({})) }

fn record_summary(r: &tokio_postgres::Row) -> Value {
    let data = parse_data(&r.get::<_, String>(7));
    json!({
        "id": r.get::<_, i64>(0), "formCode": r.get::<_, String>(1), "formRev": r.get::<_, String>(2),
        "refNo": r.get::<_, String>(3), "status": r.get::<_, String>(4), "title": r.get::<_, String>(5),
        "dept": r.get::<_, String>(6), "preparedBy": r.get::<_, String>(8), "preparedName": r.get::<_, String>(9),
        "revNo": r.get::<_, i64>(11), "createdAt": r.get::<_, String>(15), "updatedAt": r.get::<_, String>(16),
        "submittedAt": r.get::<_, String>(17), "trainDate": data["trainDate"], "place": data["place"],
    })
}

pub async fn records_list(State(st): State<S>, Query(q): Query<HashMap<String, String>>) -> Response {
    let client = match st.db.get().await { Ok(c) => c, Err(_) => return err(StatusCode::INTERNAL_SERVER_ERROR, "เชื่อมต่อฐานข้อมูลไม่ได้") };
    let sql = format!("SELECT {RCOLS} FROM form_records ORDER BY updated_at DESC LIMIT 500");
    let rows = match client.query(&sql, &[]).await {
        Ok(r) => r, Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };
    let form = q.get("form").map(|s| s.as_str()).unwrap_or("");
    let status = q.get("status").map(|s| s.as_str()).unwrap_or("");
    let dept = q.get("dept").map(|s| s.as_str()).unwrap_or("");
    let mine = q.get("mine").map(|s| s.as_str()).unwrap_or("");
    let needle = q.get("q").map(|s| s.to_lowercase()).unwrap_or_default();
    let mut out = Vec::new();
    for r in &rows {
        let item = record_summary(r);
        if !form.is_empty() && item["formCode"].as_str() != Some(form) { continue; }
        if !status.is_empty() && item["status"].as_str() != Some(status) { continue; }
        if !dept.is_empty() && item["dept"].as_str() != Some(dept) { continue; }
        if !mine.is_empty() && item["preparedBy"].as_str() != Some(mine) { continue; }
        if !needle.is_empty() {
            let blob = format!("{} {} {} {}", item["refNo"], item["title"], item["preparedName"], item["place"]).to_lowercase();
            if !blob.contains(&needle) { continue; }
        }
        out.push(item);
    }
    Json(Value::Array(out)).into_response()
}

#[derive(Deserialize)]
#[allow(non_snake_case)]
pub struct CreateReq { formCode: String }

pub async fn record_create(State(st): State<S>, Extension(u): Extension<AuthUser>, Json(b): Json<CreateReq>) -> Response {
    if u.role == "viewer" { return err(StatusCode::FORBIDDEN, "สิทธิ์นี้ดูได้อย่างเดียว"); }
    let client = match st.db.get().await { Ok(c) => c, Err(_) => return err(StatusCode::INTERNAL_SERVER_ERROR, "เชื่อมต่อฐานข้อมูลไม่ได้") };
    let schema = match load_schema(&client, b.formCode.trim()).await { Ok(s) => s, Err(e) => return err(StatusCode::BAD_REQUEST, &e) };
    let code = schema["form_code"].as_str().unwrap_or("").to_string();
    let rev = schema["form_rev"].as_str().unwrap_or("").to_string();
    let now = now_iso();
    let token = new_token(&format!("checkin-{code}"));
    let empty = json!({ "participants": [] }).to_string();
    let sql = format!(
        "INSERT INTO form_records(form_code,form_rev,ref_no,status,title,dept,data,prepared_by,prepared_name,prepared_position,rev_no,family_id,checkin_token,cancel_reason,created_at,updated_at,submitted_at)
         VALUES($1,$2,'', 'draft','',$3,$4,$5,$6,$7,1,0,$8,'',$9,$9,'') RETURNING id");
    let id: i64 = match client.query_one(&sql, &[&code, &rev, &u.dept, &empty, &u.emp_code, &u.name, &u.position, &token, &now]).await {
        Ok(r) => r.get(0), Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };
    let draft = format!("DRAFT-{id:04}");
    let _ = client.execute("UPDATE form_records SET ref_no=$2, family_id=$3 WHERE id=$1", &[&id, &draft, &id]).await;
    let _ = audit(&client, &u, "create", "form_record", &id.to_string(), &code).await;
    record_detail_resp(&st, id, &u, "").await
}

pub async fn record_get(State(st): State<S>, Extension(u): Extension<AuthUser>, Path(id): Path<i64>, Query(q): Query<HashMap<String, String>>) -> Response {
    let origin = q.get("origin").cloned().unwrap_or_else(|| "http://localhost:5173".into());
    record_detail_resp(&st, id, &u, &origin).await
}

async fn record_detail_resp(st: &S, id: i64, _u: &AuthUser, origin: &str) -> Response {
    let client = match st.db.get().await { Ok(c) => c, Err(_) => return err(StatusCode::INTERNAL_SERVER_ERROR, "เชื่อมต่อฐานข้อมูลไม่ได้") };
    let sql = format!("SELECT {RCOLS} FROM form_records WHERE id=$1");
    let row = match client.query_opt(&sql, &[&id]).await { Ok(Some(r)) => r, _ => return err(StatusCode::NOT_FOUND, "ไม่พบบันทึก") };
    let mut item = record_summary(&row);
    let data = parse_data(&row.get::<_, String>(7));
    let family: i64 = row.get(12);
    let token: String = row.get(13);
    item["data"] = data;
    item["preparedPosition"] = json!(row.get::<_, String>(10));
    item["cancelReason"] = json!(row.get::<_, String>(14));
    item["familyId"] = json!(family);
    let checkin = format!("{}/#checkin={token}", origin.trim_end_matches('/'));
    item["checkinUrl"] = json!(checkin);
    item["checkinQr"] = json!(qr_svg(&checkin));
    let steps = client.query("SELECT step,action,actor_code,actor_name,actor_position,comment,at FROM form_steps WHERE record_id=$1 ORDER BY id", &[&id]).await.unwrap_or_default();
    item["steps"] = Value::Array(steps.iter().map(|s| json!({
        "step": s.get::<_, String>(0), "action": s.get::<_, String>(1), "actorCode": s.get::<_, String>(2),
        "actorName": s.get::<_, String>(3), "actorPosition": s.get::<_, String>(4), "comment": s.get::<_, String>(5), "at": s.get::<_, String>(6),
    })).collect());
    let files = client.query("SELECT id,filename,created_at FROM form_files WHERE record_id=$1 ORDER BY id", &[&id]).await.unwrap_or_default();
    item["files"] = Value::Array(files.iter().map(|f| json!({ "id": f.get::<_, i64>(0), "filename": f.get::<_, String>(1), "createdAt": f.get::<_, String>(2) })).collect());
    let hist = client.query(&format!("SELECT id,ref_no,rev_no,status,updated_at FROM form_records WHERE family_id=$1 ORDER BY rev_no"), &[&family]).await.unwrap_or_default();
    item["history"] = Value::Array(hist.iter().map(|h| json!({
        "id": h.get::<_, i64>(0), "refNo": h.get::<_, String>(1), "revNo": h.get::<_, i64>(2), "status": h.get::<_, String>(3), "updatedAt": h.get::<_, String>(4),
    })).collect());
    if let Ok(schema) = load_schema(&client, item["formCode"].as_str().unwrap_or("")).await { item["schema"] = schema; }
    Json(item).into_response()
}

#[derive(Deserialize)]
pub struct DataReq { data: Value }

pub async fn record_save(State(st): State<S>, Extension(u): Extension<AuthUser>, Path(id): Path<i64>, Json(b): Json<DataReq>) -> Response {
    let client = match st.db.get().await { Ok(c) => c, Err(_) => return err(StatusCode::INTERNAL_SERVER_ERROR, "เชื่อมต่อฐานข้อมูลไม่ได้") };
    let cur = match load_row(&client, id).await { Some(v) => v, None => return err(StatusCode::NOT_FOUND, "ไม่พบบันทึก") };
    if !can_edit(&u, &cur) { return err(StatusCode::FORBIDDEN, "บันทึกนี้แก้ไม่ได้"); }
    let title = b.data["topic"].as_str().unwrap_or("").to_string();
    let now = now_iso();
    let body = b.data.to_string();
    if client.execute("UPDATE form_records SET data=$2,title=$3,updated_at=$4 WHERE id=$1", &[&id, &body, &title, &now]).await.is_err() {
        return err(StatusCode::INTERNAL_SERVER_ERROR, "บันทึกไม่สำเร็จ");
    }
    Json(json!({ "ok": true, "updatedAt": now })).into_response()
}

async fn load_row(c: &impl deadpool_postgres::GenericClient, id: i64) -> Option<Value> {
    let sql = format!("SELECT {RCOLS} FROM form_records WHERE id=$1");
    let r = c.query_opt(&sql, &[&id]).await.ok()??;
    Some(record_full(&r))
}
fn record_full(r: &tokio_postgres::Row) -> Value {
    let mut v = record_summary(r);
    v["data"] = parse_data(&r.get::<_, String>(7));
    v["preparedPosition"] = json!(r.get::<_, String>(10));
    v["familyId"] = json!(r.get::<_, i64>(12));
    v["checkinToken"] = json!(r.get::<_, String>(13));
    v["cancelReason"] = json!(r.get::<_, String>(14));
    v
}
fn can_edit(u: &AuthUser, cur: &Value) -> bool {
    if !editable_status(cur["status"].as_str().unwrap_or("")) { return false; }
    is_admin(&u.role) || cur["preparedBy"].as_str() == Some(u.emp_code.as_str())
}

fn validate_data(schema: &Value, data: &Value) -> Result<(), String> {
    if let Some(fields) = schema["header_fields"].as_array() {
        for f in fields {
            if f["required"].as_bool() != Some(true) { continue; }
            let key = f["key"].as_str().unwrap_or("");
            let label = f["label"].as_str().unwrap_or(key);
            let typ = f["type"].as_str().unwrap_or("");
            let v = &data[key];
            let empty = if typ == "checkbox" {
                v.as_array().map(|a| a.is_empty()).unwrap_or(true)
            } else {
                v.as_str().map(|s| s.trim().is_empty()).unwrap_or(true)
            };
            if empty { return Err(format!("กรุณากรอก{label}")); }
            if typ == "radio" && v.as_str() == Some("other") {
                let ok = f["otherKey"].as_str().unwrap_or("");
                if data[ok].as_str().unwrap_or("").trim().is_empty() { return Err(format!("กรุณาระบุ{label}")); }
            }
            if typ == "checkbox" {
                let wants_other = v.as_array().map(|a| a.iter().any(|x| x.as_str() == Some("other"))).unwrap_or(false);
                if wants_other {
                    let ok = f["otherKey"].as_str().unwrap_or("");
                    if data[ok].as_str().unwrap_or("").trim().is_empty() { return Err(format!("กรุณาระบุ{label}")); }
                }
            }
        }
    }
    if let Some(tables) = schema["table_sections"].as_array() {
        for t in tables {
            if t["required"].as_bool() != Some(true) { continue; }
            let key = t["key"].as_str().unwrap_or("participants");
            let rows = data[key].as_array().cloned().unwrap_or_default();
            let meaningful: Vec<&Value> = rows.iter().filter(|r| {
                !r["name"].as_str().unwrap_or("").trim().is_empty() || !r["empCode"].as_str().unwrap_or("").trim().is_empty()
            }).collect();
            if meaningful.is_empty() { return Err("ต้องมีผู้เข้าร่วมอย่างน้อย 1 คน".into()); }
            let mut seen = HashSet::new();
            for r in meaningful {
                let c = r["empCode"].as_str().unwrap_or("").trim().to_string();
                if c.is_empty() || c == "-" { continue; }
                if !seen.insert(c.clone()) { return Err(format!("รหัสพนักงาน {c} ซ้ำในใบเดียวกัน")); }
            }
        }
    }
    Ok(())
}

async fn add_step(c: &impl deadpool_postgres::GenericClient, id: i64, step: &str, action: &str, u: &AuthUser, comment: &str) -> Result<(), String> {
    c.execute(
        "INSERT INTO form_steps(record_id,step,action,actor_code,actor_name,actor_position,comment,at) VALUES($1,$2,$3,$4,$5,$6,$7,$8)",
        &[&id, &step.to_string(), &action.to_string(), &u.emp_code, &u.name, &u.position, &comment.to_string(), &now_iso()],
    ).await.map(|_| ()).map_err(|e| e.to_string())
}

async fn write_training(c: &impl deadpool_postgres::GenericClient, cur: &Value) -> Result<(), String> {
    let family = cur["familyId"].as_i64().unwrap_or(0);
    let id = cur["id"].as_i64().unwrap_or(0);
    c.execute("DELETE FROM training_log WHERE record_id IN (SELECT id FROM form_records WHERE family_id=$1)", &[&family]).await.map_err(|e| e.to_string())?;
    let data = &cur["data"];
    let course = data["topic"].as_str().unwrap_or("").to_string();
    let date = data["trainDate"].as_str().unwrap_or("").to_string();
    let ref_no = cur["refNo"].as_str().unwrap_or("").to_string();
    if let Some(rows) = data["participants"].as_array() {
        for r in rows {
            let name = r["name"].as_str().unwrap_or("").trim().to_string();
            if name.is_empty() { continue; }
            let emp = r["empCode"].as_str().unwrap_or("").to_string();
            let result = r["result"].as_str().unwrap_or("").to_string();
            let dept = r["dept"].as_str().unwrap_or("").to_string();
            c.execute(
                "INSERT INTO training_log(record_id,emp_code,name,course,train_date,result,ref_no,dept) VALUES($1,$2,$3,$4,$5,$6,$7,$8)",
                &[&id, &emp, &name, &course, &date, &result, &ref_no, &dept],
            ).await.map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

pub async fn record_submit(State(st): State<S>, Extension(u): Extension<AuthUser>, Path(id): Path<i64>) -> Response {
    let mut client = match st.db.get().await { Ok(c) => c, Err(_) => return err(StatusCode::INTERNAL_SERVER_ERROR, "เชื่อมต่อฐานข้อมูลไม่ได้") };
    let cur = match load_row(&client, id).await { Some(v) => v, None => return err(StatusCode::NOT_FOUND, "ไม่พบบันทึก") };
    if !can_edit(&u, &cur) { return err(StatusCode::FORBIDDEN, "ส่งได้เฉพาะร่างของตนเอง"); }
    let schema = match load_schema(&client, cur["formCode"].as_str().unwrap_or("")).await { Ok(s) => s, Err(e) => return err(StatusCode::BAD_REQUEST, &e) };
    if let Err(e) = validate_data(&schema, &cur["data"]) { return err(StatusCode::BAD_REQUEST, &e); }
    let flow = schema["approval_flow"].as_array().cloned().unwrap_or_default();
    let next_status = flow.first().and_then(|s| s["step"].as_str()).unwrap_or("approved");
    let next_status = if matches!(next_status, "review" | "approve") { next_status } else { "approved" };
    let tx = match client.transaction().await { Ok(t) => t, Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()) };
    let mut ref_no = cur["refNo"].as_str().unwrap_or("").to_string();
    if ref_no.starts_with("DRAFT-") || ref_no.is_empty() {
        let year = be_year_now();
        let code = cur["formCode"].as_str().unwrap_or("").to_string();
        let seq: i64 = match tx.query_one(
            "INSERT INTO form_seq(form_code,year_be,last_seq) VALUES($1,$2,1) ON CONFLICT(form_code,year_be) DO UPDATE SET last_seq=form_seq.last_seq+1 RETURNING last_seq",
            &[&code, &year],
        ).await { Ok(r) => r.get(0), Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()) };
        ref_no = format!("{code}-{year}-{seq:04}");
    }
    let now = now_iso();
    let submitted = if cur["submittedAt"].as_str().unwrap_or("").is_empty() { now.clone() } else { cur["submittedAt"].as_str().unwrap_or("").to_string() };
    if tx.execute("UPDATE form_records SET ref_no=$2,status=$3,updated_at=$4,submitted_at=$5 WHERE id=$1", &[&id, &ref_no, &next_status.to_string(), &now, &submitted]).await.is_err() {
        return err(StatusCode::INTERNAL_SERVER_ERROR, "ส่งอนุมัติไม่สำเร็จ");
    }
    let step_name = if next_status == "approved" { "approve" } else { "submit" };
    if add_step(&tx, id, step_name, "submit", &u, "").await.is_err() { return err(StatusCode::INTERNAL_SERVER_ERROR, "บันทึกขั้นตอนไม่ได้"); }
    if next_status == "approved" {
        let mut approved = cur.clone();
        approved["refNo"] = json!(ref_no);
        if write_training(&tx, &approved).await.is_err() { return err(StatusCode::INTERNAL_SERVER_ERROR, "ลงทะเบียนอบรมไม่ได้"); }
    }
    if tx.commit().await.is_err() { return err(StatusCode::INTERNAL_SERVER_ERROR, "ยืนยันรายการไม่ได้"); }
    let _ = audit_user(&st, &u, "submit", &id.to_string(), &ref_no).await;
    record_detail_resp(&st, id, &u, "").await
}

async fn audit_user(st: &S, u: &AuthUser, action: &str, id: &str, detail: &str) {
    if let Ok(c) = st.db.get().await { let _ = audit(&c, u, action, "form_record", id, detail).await; }
}

#[derive(Deserialize)]
pub struct ActReq { action: String, comment: Option<String> }

pub async fn record_act(State(st): State<S>, Extension(u): Extension<AuthUser>, Path(id): Path<i64>, Json(b): Json<ActReq>) -> Response {
    let client = match st.db.get().await { Ok(c) => c, Err(_) => return err(StatusCode::INTERNAL_SERVER_ERROR, "เชื่อมต่อฐานข้อมูลไม่ได้") };
    let cur = match load_row(&client, id).await { Some(v) => v, None => return err(StatusCode::NOT_FOUND, "ไม่พบบันทึก") };
    let status = cur["status"].as_str().unwrap_or("").to_string();
    let owner = cur["preparedBy"].as_str().unwrap_or("");
    if !is_admin(&u.role) && owner == u.emp_code { return err(StatusCode::FORBIDDEN, "ผู้จัดทำตรวจใบของตนเองไม่ได้"); }
    let pass = b.action == "pass";
    let send_back = b.action == "return";
    if !pass && !send_back { return err(StatusCode::BAD_REQUEST, "การดำเนินการไม่ถูกต้อง"); }
    let comment = b.comment.unwrap_or_default();
    if send_back && comment.trim().is_empty() { return err(StatusCode::BAD_REQUEST, "กรุณาระบุเหตุผลที่ตีกลับ"); }
    let (ok_role, step) = if status == "review" { (can_review(&u.role), "review") } else if status == "approve" { (can_approve(&u.role), "approve") } else { (false, "") };
    if !ok_role { return err(StatusCode::FORBIDDEN, "ไม่มีสิทธิ์ในขั้นนี้"); }
    let schema = load_schema(&client, cur["formCode"].as_str().unwrap_or("")).await.unwrap_or(json!({}));
    let flow = schema["approval_flow"].as_array().cloned().unwrap_or_default();
    let mut next = "approved".to_string();
    if pass {
        if let Some(i) = flow.iter().position(|s| s["step"].as_str() == Some(step)) {
            if let Some(n) = flow.get(i + 1).and_then(|s| s["step"].as_str()) {
                if matches!(n, "review" | "approve") { next = n.to_string(); }
            }
        }
    } else {
        next = "returned".into();
    }
    let now = now_iso();
    if client.execute("UPDATE form_records SET status=$2,updated_at=$3 WHERE id=$1", &[&id, &next, &now]).await.is_err() {
        return err(StatusCode::INTERNAL_SERVER_ERROR, "อัปเดตสถานะไม่ได้");
    }
    let action = if pass { "pass" } else { "return" };
    let _ = add_step(&client, id, step, action, &u, &comment).await;
    if next == "approved" { let _ = write_training(&client, &cur).await; }
    let _ = audit(&client, &u, action, "form_record", &id.to_string(), &comment).await;
    record_detail_resp(&st, id, &u, "").await
}

#[derive(Deserialize)]
pub struct ReasonReq { reason: String }

pub async fn record_cancel(State(st): State<S>, Extension(u): Extension<AuthUser>, Path(id): Path<i64>, Json(b): Json<ReasonReq>) -> Response {
    if b.reason.trim().is_empty() { return err(StatusCode::BAD_REQUEST, "กรุณาระบุเหตุผลการยกเลิก"); }
    let client = match st.db.get().await { Ok(c) => c, Err(_) => return err(StatusCode::INTERNAL_SERVER_ERROR, "เชื่อมต่อฐานข้อมูลไม่ได้") };
    let cur = match load_row(&client, id).await { Some(v) => v, None => return err(StatusCode::NOT_FOUND, "ไม่พบบันทึก") };
    let status = cur["status"].as_str().unwrap_or("");
    if matches!(status, "cancelled" | "imported" | "superseded") { return err(StatusCode::BAD_REQUEST, "สถานะนี้ยกเลิกไม่ได้"); }
    let owner = cur["preparedBy"].as_str() == Some(u.emp_code.as_str());
    if !is_admin(&u.role) && !(owner && status != "approved") { return err(StatusCode::FORBIDDEN, "ไม่มีสิทธิ์ยกเลิก"); }
    if status == "approved" && !is_admin(&u.role) { return err(StatusCode::FORBIDDEN, "ใบที่อนุมัติแล้วให้ขอแก้ไข"); }
    let now = now_iso();
    let reason = b.reason.trim().to_string();
    let _ = client.execute("UPDATE form_records SET status='cancelled',cancel_reason=$2,updated_at=$3 WHERE id=$1", &[&id, &reason, &now]).await;
    let _ = add_step(&client, id, "cancel", "cancel", &u, &reason).await;
    let _ = audit(&client, &u, "cancel", "form_record", &id.to_string(), &reason).await;
    record_detail_resp(&st, id, &u, "").await
}

pub async fn record_amend(State(st): State<S>, Extension(u): Extension<AuthUser>, Path(id): Path<i64>, Json(b): Json<ReasonReq>) -> Response {
    if b.reason.trim().is_empty() { return err(StatusCode::BAD_REQUEST, "กรุณาระบุเหตุผลที่ขอแก้ไข"); }
    let client = match st.db.get().await { Ok(c) => c, Err(_) => return err(StatusCode::INTERNAL_SERVER_ERROR, "เชื่อมต่อฐานข้อมูลไม่ได้") };
    let cur = match load_row(&client, id).await { Some(v) => v, None => return err(StatusCode::NOT_FOUND, "ไม่พบบันทึก") };
    if cur["status"].as_str() != Some("approved") { return err(StatusCode::BAD_REQUEST, "ขอแก้ไขได้เฉพาะใบที่อนุมัติแล้ว"); }
    let owner = cur["preparedBy"].as_str() == Some(u.emp_code.as_str());
    if !is_admin(&u.role) && !owner { return err(StatusCode::FORBIDDEN, "เฉพาะผู้จัดทำหรือผู้ดูแล"); }
    let open = client.query_opt("SELECT id FROM form_records WHERE family_id=$1 AND status IN ('draft','returned','review','approve')", &[&cur["familyId"].as_i64().unwrap_or(0)]).await.ok().flatten();
    if open.is_some() { return err(StatusCode::BAD_REQUEST, "มีฉบับแก้ไขที่ยังไม่จบอยู่แล้ว"); }
    let now = now_iso();
    let rev_no = cur["revNo"].as_i64().unwrap_or(1) + 1;
    let data = cur["data"].to_string();
    let token = new_token("amend");
    let title = cur["title"].as_str().unwrap_or("").to_string();
    let dept = cur["dept"].as_str().unwrap_or("").to_string();
    let form_code = cur["formCode"].as_str().unwrap_or("").to_string();
    let form_rev = cur["formRev"].as_str().unwrap_or("").to_string();
    let ref_no = cur["refNo"].as_str().unwrap_or("").to_string();
    let family = cur["familyId"].as_i64().unwrap_or(0);
    let sql = format!(
        "INSERT INTO form_records(form_code,form_rev,ref_no,status,title,dept,data,prepared_by,prepared_name,prepared_position,rev_no,family_id,checkin_token,cancel_reason,created_at,updated_at,submitted_at)
         VALUES($1,$2,$3,'draft',$4,$5,$6,$7,$8,$9,$10,$11,$12,'',$13,$13,'') RETURNING id");
    let new_id: i64 = match client.query_one(&sql, &[&form_code, &form_rev, &ref_no, &title, &dept, &data, &u.emp_code, &u.name, &u.position, &rev_no, &family, &token, &now]).await {
        Ok(r) => r.get(0), Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };
    let _ = client.execute("UPDATE form_records SET status='superseded',updated_at=$2 WHERE id=$1", &[&id, &now]).await;
    let reason = b.reason.trim().to_string();
    let _ = add_step(&client, id, "amend", "amend", &u, &reason).await;
    let _ = add_step(&client, new_id, "draft", "amend", &u, &reason).await;
    let _ = audit(&client, &u, "amend", "form_record", &new_id.to_string(), &reason).await;
    record_detail_resp(&st, new_id, &u, "").await
}

pub async fn pending(State(st): State<S>, Extension(u): Extension<AuthUser>) -> Response {
    let client = match st.db.get().await { Ok(c) => c, Err(_) => return err(StatusCode::INTERNAL_SERVER_ERROR, "เชื่อมต่อฐานข้อมูลไม่ได้") };
    match pending_items(&client, &u).await {
        Ok(out) => Json(json!({ "count": out.len(), "items": out })).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }
}

/// แบบฟอร์มที่รอผู้ใช้นี้ตรวจสอบ/อนุมัติ (ไม่รวมใบที่ตนจัดทำ ยกเว้น admin)
pub(crate) async fn pending_items(client: &impl deadpool_postgres::GenericClient, u: &AuthUser) -> Result<Vec<Value>, String> {
    let sql = format!("SELECT {RCOLS} FROM form_records WHERE status IN ('review','approve') ORDER BY updated_at");
    let rows = client.query(&sql, &[]).await.map_err(|e| e.to_string())?;
    Ok(rows.iter().filter_map(|r| {
        let item = record_summary(r);
        let status = item["status"].as_str().unwrap_or("");
        let by = item["preparedBy"].as_str().unwrap_or("");
        if !is_admin(&u.role) && by == u.emp_code { return None; }
        let ok = (status == "review" && can_review(&u.role)) || (status == "approve" && can_approve(&u.role));
        ok.then_some(item)
    }).collect())
}

pub async fn training_list(State(st): State<S>, Query(q): Query<HashMap<String, String>>) -> Response {
    let client = match st.db.get().await { Ok(c) => c, Err(_) => return err(StatusCode::INTERNAL_SERVER_ERROR, "เชื่อมต่อฐานข้อมูลไม่ได้") };
    let needle = format!("%{}%", q.get("q").map(|s| s.trim()).unwrap_or(""));
    let rows = match client.query(
        "SELECT emp_code,name,course,train_date,result,ref_no,dept,record_id FROM training_log WHERE emp_code ILIKE $1 OR name ILIKE $1 OR course ILIKE $1 OR ref_no ILIKE $1 ORDER BY train_date DESC, name",
        &[&needle],
    ).await { Ok(r) => r, Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()) };
    Json(Value::Array(rows.iter().map(|r| json!({
        "empCode": r.get::<_, String>(0), "name": r.get::<_, String>(1), "course": r.get::<_, String>(2),
        "trainDate": r.get::<_, String>(3), "result": r.get::<_, String>(4), "refNo": r.get::<_, String>(5),
        "dept": r.get::<_, String>(6), "recordId": r.get::<_, i64>(7),
    })).collect())).into_response()
}

pub async fn form_stats(State(st): State<S>) -> Response {
    let client = match st.db.get().await { Ok(c) => c, Err(_) => return err(StatusCode::INTERNAL_SERVER_ERROR, "เชื่อมต่อฐานข้อมูลไม่ได้") };
    let rows = client.query("SELECT status, created_at, submitted_at FROM form_records", &[]).await.unwrap_or_default();
    let mut pending = 0i64;
    let mut month = 0i64;
    let mut overdue = 0i64;
    let now = chrono::Utc::now();
    let ym = {
        let tz = chrono::FixedOffset::east_opt(7 * 3600).unwrap();
        now.with_timezone(&tz).format("%Y-%m").to_string()
    };
    for r in &rows {
        let status: String = r.get(0);
        let created: String = r.get(1);
        let submitted: String = r.get(2);
        if created.starts_with(&ym) || submitted.starts_with(&ym) { month += 1; }
        if matches!(status.as_str(), "review" | "approve") {
            pending += 1;
            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(if submitted.is_empty() { &created } else { &submitted }) {
                if now.signed_duration_since(dt.with_timezone(&chrono::Utc)) > chrono::Duration::days(7) { overdue += 1; }
            }
        }
    }
    Json(json!({ "thisMonth": month, "pending": pending, "overdue": overdue })).into_response()
}

#[derive(Deserialize)]
pub struct FileReq { filename: String, data: String }

pub async fn file_add(State(st): State<S>, Extension(u): Extension<AuthUser>, Path(id): Path<i64>, Json(b): Json<FileReq>) -> Response {
    let client = match st.db.get().await { Ok(c) => c, Err(_) => return err(StatusCode::INTERNAL_SERVER_ERROR, "เชื่อมต่อฐานข้อมูลไม่ได้") };
    let cur = match load_row(&client, id).await { Some(v) => v, None => return err(StatusCode::NOT_FOUND, "ไม่พบบันทึก") };
    if !can_edit(&u, &cur) { return err(StatusCode::FORBIDDEN, "แนบไฟล์ได้ตอนเป็นร่างเท่านั้น"); }
    let bytes = match B64.decode(b.data.trim()) { Ok(v) => v, Err(_) => return err(StatusCode::BAD_REQUEST, "ไฟล์ไม่ถูกต้อง") };
    if bytes.len() > 8 * 1024 * 1024 { return err(StatusCode::BAD_REQUEST, "ไฟล์ใหญ่เกิน 8MB"); }
    let ext = b.filename.rsplit('.').next().unwrap_or("bin").chars().filter(|c| c.is_ascii_alphanumeric()).take(5).collect::<String>();
    let stored = format!("{id}-{}-{}.{}", now_iso().replace([':', '.'], ""), new_token("f")[..8].to_string(), ext);
    let dir = st.docs_root.join("storage").join("eform");
    let _ = std::fs::create_dir_all(&dir);
    if std::fs::write(dir.join(&stored), &bytes).is_err() { return err(StatusCode::INTERNAL_SERVER_ERROR, "เก็บไฟล์ไม่ได้"); }
    let name = b.filename.chars().filter(|c| *c != '/' && *c != '\\' && !c.is_control()).take(120).collect::<String>();
    let now = now_iso();
    let fid: i64 = match client.query_one("INSERT INTO form_files(record_id,filename,stored_name,created_at) VALUES($1,$2,$3,$4) RETURNING id", &[&id, &name, &stored, &now]).await {
        Ok(r) => r.get(0), Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };
    Json(json!({ "ok": true, "id": fid, "filename": name })).into_response()
}

pub async fn file_get(State(st): State<S>, Path(fid): Path<i64>) -> Response {
    let client = match st.db.get().await { Ok(c) => c, Err(_) => return err(StatusCode::INTERNAL_SERVER_ERROR, "เชื่อมต่อฐานข้อมูลไม่ได้") };
    let row = match client.query_opt("SELECT filename,stored_name FROM form_files WHERE id=$1", &[&fid]).await { Ok(Some(r)) => r, _ => return err(StatusCode::NOT_FOUND, "ไม่พบไฟล์") };
    let filename: String = row.get(0);
    let stored: String = row.get(1);
    let bytes = match std::fs::read(st.docs_root.join("storage").join("eform").join(&stored)) { Ok(b) => b, Err(_) => return err(StatusCode::NOT_FOUND, "อ่านไฟล์ไม่ได้") };
    Response::builder()
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::CONTENT_DISPOSITION, format!("attachment; filename*=UTF-8''{}", crate::pct(&filename)))
        .body(Body::from(bytes))
        .unwrap()
}

pub async fn record_print(State(st): State<S>, Path(id): Path<i64>, Query(q): Query<HashMap<String, String>>) -> Response {
    let origin = q.get("origin").cloned().unwrap_or_else(|| "http://localhost:5173".into());
    match print_with_steps(&st, id, &origin).await {
        Ok(html) if !html.is_empty() => Response::builder().header(header::CONTENT_TYPE, "text/html; charset=utf-8").body(Body::from(html)).unwrap(),
        Ok(_) => err(StatusCode::INTERNAL_SERVER_ERROR, "สร้างหน้าพิมพ์ไม่ได้"),
        Err(e) => err(StatusCode::NOT_FOUND, &e),
    }
}

fn print_payload(cur: &Value, schema: &Value, origin: &str, token: &str) -> Value {
    let data = &cur["data"];
    let mut fields = Vec::new();
    if let Some(list) = schema["header_fields"].as_array() {
        for f in list {
            let key = f["key"].as_str().unwrap_or("");
            let label = f["label"].as_str().unwrap_or(key).to_string();
            let typ = f["type"].as_str().unwrap_or("");
            let value = match typ {
                "date" => th_date(data[key].as_str().unwrap_or("")),
                "checkbox" => {
                    let picked = data[key].as_array().cloned().unwrap_or_default();
                    let mut labels = Vec::new();
                    for o in f["options"].as_array().cloned().unwrap_or_default() {
                        let v = o["value"].as_str().unwrap_or("");
                        if picked.iter().any(|p| p.as_str() == Some(v)) { labels.push(o["label"].as_str().unwrap_or(v).to_string()); }
                    }
                    if let Some(ok) = f["otherKey"].as_str() {
                        let extra = data[ok].as_str().unwrap_or("").trim().to_string();
                        if !extra.is_empty() { labels.push(extra); }
                    }
                    labels.join(", ")
                }
                "radio" => {
                    let v = data[key].as_str().unwrap_or("");
                    let mut label_v = v.to_string();
                    for o in f["options"].as_array().cloned().unwrap_or_default() {
                        if o["value"].as_str() == Some(v) { label_v = o["label"].as_str().unwrap_or(v).to_string(); }
                    }
                    if v == "other" {
                        let extra = data[f["otherKey"].as_str().unwrap_or("")].as_str().unwrap_or("");
                        if !extra.is_empty() { label_v = format!("{label_v}: {extra}"); }
                    }
                    label_v
                }
                _ => data[key].as_str().unwrap_or("").to_string(),
            };
            if key == "timeStart" { continue; }
            if key == "timeEnd" {
                fields.push(json!({ "label": "เวลา", "value": format!("{} – {} น.", data["timeStart"].as_str().unwrap_or(""), data["timeEnd"].as_str().unwrap_or("")) }));
                continue;
            }
            if typ == "radio" || typ == "checkbox" {
                let picked: Vec<String> = if typ == "radio" { vec![data[key].as_str().unwrap_or("").to_string()] }
                    else { data[key].as_array().cloned().unwrap_or_default().iter().filter_map(|v| v.as_str().map(String::from)).collect() };
                let opts: Vec<Value> = f["options"].as_array().cloned().unwrap_or_default().iter().map(|o| {
                    let v = o["value"].as_str().unwrap_or("");
                    json!({ "label": o["label"].as_str().unwrap_or(v), "on": picked.iter().any(|p| p == v) })
                }).collect();
                let other = f["otherKey"].as_str().map(|k| data[k].as_str().unwrap_or("").trim().to_string()).unwrap_or_default();
                fields.push(json!({ "label": label, "value": value, "options": opts, "other": other }));
                continue;
            }
            fields.push(json!({ "label": label, "value": value }));
        }
    }
    let mut columns = Vec::new();
    let mut rows = Vec::new();
    if let Some(sec) = schema["table_sections"].as_array().and_then(|a| a.first()) {
        let cols = sec["columns"].as_array().cloned().unwrap_or_default();
        for c in &cols {
            if c["type"].as_str() == Some("signature") { columns.push(json!("ลายมือชื่อ")); }
            else { columns.push(json!(c["label"])); }
        }
        for (i, p) in data["participants"].as_array().cloned().unwrap_or_default().iter().enumerate() {
            let _ = i;
            let mut cells = Vec::new();
            for c in &cols {
                let key = c["key"].as_str().unwrap_or("");
                let typ = c["type"].as_str().unwrap_or("");
                let val = if typ == "signature" {
                    let sig = p["signature"].as_str().unwrap_or("");
                    let at = p["attendedAt"].as_str().unwrap_or("");
                    if sig.starts_with("data:image/") { sig.to_string() }
                    else if !at.is_empty() { format!("เช็คอิน {}", th_when(at)) }
                    else { String::new() }
                } else if key == "result" {
                    match p[key].as_str().unwrap_or("") { "pass" => "☑ ผ่าน".into(), "fail" => "☒ ไม่ผ่าน".into(), v => v.into() }
                } else { p[key].as_str().unwrap_or("").to_string() };
                cells.push(Value::String(val));
            }
            rows.push(Value::Array(cells));
        }
    }
    let status_code = cur["status"].as_str().unwrap_or("");
    let status_th = match status_code {
        "draft" => "ร่าง", "review" => "รอตรวจสอบ", "approve" => "รออนุมัติ", "approved" => "อนุมัติแล้ว",
        "returned" => "ตีกลับ", "cancelled" => "ยกเลิก", "imported" => "นำเข้าจากเอกสารเดิม", "superseded" => "ถูกแทนที่", _ => status_code,
    };
    let watermark = if status_code == "approved" || status_code == "imported" { "" } else { "ร่าง – ยังไม่อนุมัติ" };
    let mut approvals = vec![json!({
        "label": "ผู้จัดทำ", "name": cur["preparedName"], "at": th_when(cur["submittedAt"].as_str().unwrap_or("")), "comment": cur["preparedPosition"],
    })];
    // ขั้นตอนที่ผ่านแล้วถูกใส่ตอนพิมพ์จากอีก endpoint; ที่นี่ใส่จาก field ว่างถ้ายังไม่มีขั้น
    let _ = approvals.len();
    json!({
        "code": cur["formCode"], "rev": cur["formRev"], "title": schema["title"], "effective": schema["effective"],
        "refNo": cur["refNo"], "status": status_th, "watermark": watermark,
        "fields": fields, "columns": columns, "rows": rows, "approvals": approvals,
        "qrText": format!("{}/#checkin={token}", origin.trim_end_matches('/')),
    })
}

fn fallback_html(p: &Value) -> String {
    format!("<!DOCTYPE html><html lang=\"th\"><meta charset=\"utf-8\"><body style=\"font-family:Leelawadee UI,Tahoma,sans-serif\"><h1>{}</h1><p>{} {} · {}</p><pre>{}</pre></body></html>",
        p["title"].as_str().unwrap_or(""), p["code"].as_str().unwrap_or(""), p["refNo"].as_str().unwrap_or(""), p["status"].as_str().unwrap_or(""),
        serde_json::to_string_pretty(p).unwrap_or_default())
}

fn qr_svg(text: &str) -> String {
    let Ok(code) = qrcode::QrCode::new(text.as_bytes()) else { return String::new() };
    let w = code.width();
    let mut s = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {w} {w}\" shape-rendering=\"crispEdges\"><rect width=\"100%\" height=\"100%\" fill=\"#fff\"/>");
    for y in 0..w {
        for x in 0..w {
            if code[(x, y)] == qrcode::Color::Dark {
                s.push_str(&format!("<rect x=\"{x}\" y=\"{y}\" width=\"1\" height=\"1\" fill=\"#111\"/>"));
            }
        }
    }
    s.push_str("</svg>");
    s
}

pub async fn checkin_info(State(st): State<S>, Path(token): Path<String>) -> Response {
    let client = match st.db.get().await { Ok(c) => c, Err(_) => return err(StatusCode::INTERNAL_SERVER_ERROR, "เชื่อมต่อฐานข้อมูลไม่ได้") };
    let row = match client.query_opt("SELECT title,status,data FROM form_records WHERE checkin_token=$1", &[&token]).await { Ok(Some(r)) => r, _ => return err(StatusCode::NOT_FOUND, "ไม่พบรอบอบรม") };
    let data = parse_data(&row.get::<_, String>(2));
    let expired = checkin_expired(&data);
    Json(json!({
        "title": row.get::<_, String>(0), "status": row.get::<_, String>(1),
        "trainDate": th_date(data["trainDate"].as_str().unwrap_or("")),
        "time": format!("{}–{}", data["timeStart"].as_str().unwrap_or(""), data["timeEnd"].as_str().unwrap_or("")),
        "place": data["place"], "expired": expired.is_err(),
    })).into_response()
}

#[derive(Deserialize)]
#[allow(non_snake_case)]
pub struct CheckReq { token: String, empCode: String, pin: String }

pub async fn checkin(State(st): State<S>, Json(b): Json<CheckReq>) -> Response {
    let code = b.empCode.trim().to_string();
    if limited(&st, &format!("in:{code}")) { return err(StatusCode::TOO_MANY_REQUESTS, "ลองบ่อยเกินไป"); }
    if b.pin.len() != 6 { mark_fail(&st, &format!("in:{code}")); return err(StatusCode::BAD_REQUEST, "PIN ไม่ถูกต้อง"); }
    let client = match st.db.get().await { Ok(c) => c, Err(_) => return err(StatusCode::INTERNAL_SERVER_ERROR, "เชื่อมต่อฐานข้อมูลไม่ได้") };
    let found = if check_pin(&client, &code, b.pin.trim()).await {
        client.query_opt("SELECT name_th,position,dept,status FROM employees WHERE emp_code=$1", &[&code]).await.ok().flatten()
    } else { None };
    let emp = match found {
        Some(r) => r, None => { mark_fail(&st, &format!("in:{code}")); return err(StatusCode::UNAUTHORIZED, "รหัสพนักงานหรือ PIN ไม่ถูกต้อง"); }
    };
    if emp.get::<_, String>(3) != "active" { return err(StatusCode::FORBIDDEN, "บัญชีนี้ไม่ได้ใช้งาน"); }
    clear_fails(&st, &format!("in:{code}"));
    let rec = match client.query_opt("SELECT id,status,data FROM form_records WHERE checkin_token=$1", &[&b.token]).await { Ok(Some(r)) => r, _ => return err(StatusCode::NOT_FOUND, "ไม่พบรอบอบรม") };
    let status: String = rec.get(1);
    if matches!(status.as_str(), "cancelled" | "superseded" | "imported") { return err(StatusCode::BAD_REQUEST, "รอบนี้ปิดการเช็คอินแล้ว"); }
    let mut data = parse_data(&rec.get::<_, String>(2));
    if let Err(e) = checkin_expired(&data) { return err(StatusCode::BAD_REQUEST, &e); }
    let id: i64 = rec.get(0);
    let name: String = emp.get(0);
    let position: String = emp.get(1);
    let dept: String = emp.get(2);
    let now = now_iso();
    let rows = data["participants"].as_array_mut();
    let Some(rows) = rows else { return err(StatusCode::BAD_REQUEST, "ไม่มีรายชื่อ"); };
    let mut found = false;
    for r in rows.iter_mut() {
        if r["empCode"].as_str().unwrap_or("").trim() == code {
            r["attendedAt"] = json!(now);
            r["name"] = json!(name);
            found = true;
            break;
        }
    }
    if !found {
        rows.push(json!({ "empCode": code, "name": name, "position": position, "dept": dept, "attendedAt": now, "result": "", "note": "", "signature": "" }));
    }
    let body = data.to_string();
    let _ = client.execute("UPDATE form_records SET data=$2,updated_at=$3 WHERE id=$1", &[&id, &body, &now]).await;
    Json(json!({ "ok": true, "name": name, "at": th_when(&now) })).into_response()
}

fn checkin_expired(data: &Value) -> Result<(), String> {
    let date = data["trainDate"].as_str().unwrap_or("");
    if date.is_empty() { return Err("ยังไม่ระบุวันอบรม".into()); }
    let end = data["timeEnd"].as_str().filter(|s| !s.is_empty()).unwrap_or("23:59");
    let raw = format!("{date}T{end}:00+07:00");
    let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&raw) else { return Err("เวลาสิ้นสุดไม่ถูกต้อง".into()) };
    if chrono::Utc::now() > dt { return Err("เลยเวลาอบรมแล้ว เช็คอินไม่ได้".into()); }
    Ok(())
}

pub async fn places(State(st): State<S>, Query(q): Query<HashMap<String, String>>) -> Response {
    let client = match st.db.get().await { Ok(c) => c, Err(_) => return err(StatusCode::INTERNAL_SERVER_ERROR, "เชื่อมต่อฐานข้อมูลไม่ได้") };
    let code = q.get("form").cloned().unwrap_or_else(|| "FM-HR-008".into());
    let rows = client.query("SELECT data FROM form_records WHERE form_code=$1 ORDER BY id DESC LIMIT 80", &[&code]).await.unwrap_or_default();
    let mut seen = Vec::new();
    for r in &rows {
        let d = parse_data(&r.get::<_, String>(0));
        let p = d["place"].as_str().unwrap_or("").trim().to_string();
        if !p.is_empty() && !seen.contains(&p) { seen.push(p); }
    }
    Json(json!(seen)).into_response()
}

pub async fn copy_previous(State(st): State<S>, Extension(u): Extension<AuthUser>, Query(q): Query<HashMap<String, String>>) -> Response {
    let client = match st.db.get().await { Ok(c) => c, Err(_) => return err(StatusCode::INTERNAL_SERVER_ERROR, "เชื่อมต่อฐานข้อมูลไม่ได้") };
    let code = q.get("form").cloned().unwrap_or_else(|| "FM-HR-008".into());
    let row = client.query_opt(
        "SELECT data FROM form_records WHERE form_code=$1 AND prepared_by=$2 AND status<>'draft' ORDER BY id DESC LIMIT 1",
        &[&code, &u.emp_code],
    ).await.ok().flatten().or_else(|| {
        // ไม่มีของตัวเอง ใช้ใบล่าสุดของฟอร์มนี้
        None
    });
    let row = if row.is_some() { row } else {
        client.query_opt("SELECT data FROM form_records WHERE form_code=$1 ORDER BY id DESC LIMIT 1", &[&code]).await.ok().flatten()
    };
    let Some(row) = row else { return Json(json!({ "participants": [] })).into_response() };
    let data = parse_data(&row.get::<_, String>(0));
    let rows = data["participants"].as_array().cloned().unwrap_or_default();
    let clean: Vec<Value> = rows.into_iter().map(|mut r| {
        if let Some(o) = r.as_object_mut() {
            o.insert("signature".into(), json!(""));
            o.insert("result".into(), json!(""));
            o.insert("attendedAt".into(), json!(""));
            o.insert("note".into(), json!(""));
        }
        r
    }).collect();
    Json(json!({ "participants": clean })).into_response()
}

pub async fn seed(pool: &deadpool_postgres::Pool, docs_root: &PathBuf) {
    let Ok(client) = pool.get().await else { return };
    let now = now_iso();
    let exists = |code: String| { let c = &client; async move { c.query_opt("SELECT 1 FROM employees WHERE emp_code=$1", &[&code]).await.ok().flatten().is_some() } };
    // แฮช Argon2 เฉพาะตอนต้องสร้างบัญชีใหม่ (ไม่คำนวณซ้ำทุกครั้งที่เริ่มระบบ)
    // ผู้ดูแลระบบคนแรก (ฐานข้อมูลใหม่เท่านั้น): PIN จาก env ADMIN_PIN หรือสุ่มแล้วแสดงในคอนโซลครั้งเดียว — ต้องเปลี่ยนเมื่อ login ครั้งแรก
    let has_admin = client.query_opt("SELECT 1 FROM employees WHERE role='admin'", &[]).await.ok().flatten().is_some();
    if !has_admin && !exists("900001".into()).await {
        let pin = std::env::var("ADMIN_PIN").ok().filter(|p| p.len() == 6 && p.chars().all(|c| c.is_ascii_digit())).unwrap_or_else(|| {
            use argon2::password_hash::rand_core::{OsRng, RngCore};
            let p = format!("{:06}", OsRng.next_u32() % 1_000_000);
            println!("  🔑 สร้างผู้ดูแลระบบ 900001 — PIN เริ่มต้น {p} (ต้องเปลี่ยนเมื่อเข้าสู่ระบบครั้งแรก)");
            p
        });
        let _ = client.execute(
            "INSERT INTO employees(emp_code,name_th,name_en,position,dept,manager_code,pin_hash,role,status,signature,created_at,must_change_pin) VALUES('900001','ผู้ดูแลระบบ','','DCC','DCC','',$1,'admin','active','',$2,true) ON CONFLICT(emp_code) DO NOTHING",
            &[&pin_hash("900001", &pin), &now],
        ).await;
    }
    let import_path = docs_root.join("eform").join("fmhr008-import.json");
    if let Ok(text) = std::fs::read_to_string(&import_path) {
        if let Ok(v) = serde_json::from_str::<Value>(&text) {
            if let Some(list) = v["employees"].as_array() {
                for e in list {
                    let code = e["empCode"].as_str().unwrap_or("");
                    let name = e["nameTh"].as_str().unwrap_or("");
                    if code.is_empty() || name.is_empty() { continue; }
                    // นำเข้าเฉพาะพนักงานใหม่ — บทบาท/สถานะ/ข้อมูลที่ผู้ดูแลแก้ในหน้า "ผู้ใช้และสิทธิ์" ต้องไม่ถูกเขียนทับตอนรีสตาร์ต
                    if exists(code.to_string()).await { continue; }
                    // ไม่มี PIN เริ่มต้น — ผู้ดูแลตั้ง PIN ให้ในหน้า "ผู้ใช้และสิทธิ์" แล้วพนักงานต้องเปลี่ยนเองเมื่อ login ครั้งแรก
                    let hash = String::new();
                    let role = e["role"].as_str().unwrap_or("preparer");
                    let name_en = e["nameEn"].as_str().unwrap_or("");
                    let pos = e["position"].as_str().unwrap_or("");
                    let dept = e["dept"].as_str().unwrap_or("");
                    let _ = client.execute(
                        "INSERT INTO employees(emp_code,name_th,name_en,position,dept,manager_code,pin_hash,role,status,signature,created_at) VALUES($1,$2,$3,$4,$5,'',$6,$7,'active','',$8) ON CONFLICT(emp_code) DO NOTHING",
                        &[&code.to_string(), &name.to_string(), &name_en.to_string(), &pos.to_string(), &dept.to_string(), &hash, &role.to_string(), &now],
                    ).await;
                }
            }
        }
    }
    // schema จากไฟล์ ถ้ายังไม่มี
    let schema_path = docs_root.join("eform").join("FM-HR-008.json");
    if let Ok(text) = std::fs::read_to_string(&schema_path) {
        if let Ok(v) = serde_json::from_str::<Value>(&text) {
            let code = v["form_code"].as_str().unwrap_or("").to_string();
            let rev = v["form_rev"].as_str().unwrap_or("").to_string();
            let title = v["title"].as_str().unwrap_or("").to_string();
            if !code.is_empty() {
                let _ = client.execute(
                    "INSERT INTO form_schemas(form_code,form_rev,title,body,updated_at) VALUES($1,$2,$3,$4,$5) ON CONFLICT DO NOTHING",
                    &[&code, &rev, &title, &text, &now],
                ).await;
            }
        }
    }
    seed_imported(&client, &import_path, &now).await;
}

async fn seed_imported(client: &tokio_postgres::Client, path: &PathBuf, now: &str) {
    let existing: i64 = client.query_one("SELECT COUNT(*) FROM form_records WHERE status='imported'", &[]).await.map(|r| r.get(0)).unwrap_or(1);
    if existing > 0 { return; }
    let Ok(text) = std::fs::read_to_string(path) else { return };
    let Ok(v) = serde_json::from_str::<Value>(&text) else { return };
    let Some(recs) = v["records"].as_array() else { return };
    for rec in recs {
        let people = rec["participants"].as_array().cloned().unwrap_or_default();
        let rows: Vec<Value> = people.into_iter().map(|p| json!({
            "empCode": p["empCode"], "name": p["name"], "position": p["position"], "dept": p["dept"],
            "signature": "", "result": "", "note": "", "attendedAt": "",
        })).collect();
        let data = json!({
            "trainDate": rec["trainDate"], "timeStart": rec["timeStart"], "timeEnd": rec["timeEnd"],
            "place": rec["place"], "topic": rec["topic"], "trainer": "", "courseType": "",
            "evalMethods": [], "participants": rows,
        });
        let ref_no = rec["refNo"].as_str().unwrap_or("").to_string();
        let title = rec["topic"].as_str().unwrap_or("").to_string();
        let body = data.to_string();
        let id = client.query_one(
            "INSERT INTO form_records(form_code,form_rev,ref_no,status,title,dept,data,prepared_by,prepared_name,prepared_position,rev_no,family_id,checkin_token,cancel_reason,created_at,updated_at,submitted_at)
             VALUES('FM-HR-008','Rev.0',$1,'imported',$2,'HR',$3,'900001','ระบบนำเข้า','',1,0,'','นำเข้าจากเอกสารเดิม',$4,$4,$4) RETURNING id",
            &[&ref_no, &title, &body, &now],
        ).await.ok().map(|r| r.get::<_, i64>(0));
        if let Some(id) = id {
            let _ = client.execute("UPDATE form_records SET family_id=$1 WHERE id=$1", &[&id]).await;
            let _ = client.execute(
                "INSERT INTO form_steps(record_id,step,action,actor_code,actor_name,actor_position,comment,at) VALUES($1,'import','import','900001','ระบบนำเข้า','','นำเข้าจากเอกสารเดิม',$2)",
                &[&id, &now],
            ).await;
        }
    }
}

pub async fn backup_once(pool: &deadpool_postgres::Pool, dir: &PathBuf) {
    let Ok(client) = pool.get().await else { return };
    let rows = client.query("SELECT id,form_code,ref_no,status,data,updated_at FROM form_records", &[]).await.unwrap_or_default();
    let items: Vec<Value> = rows.iter().map(|r| json!({
        "id": r.get::<_, i64>(0), "formCode": r.get::<_, String>(1), "refNo": r.get::<_, String>(2),
        "status": r.get::<_, String>(3), "data": parse_data(&r.get::<_, String>(4)), "updatedAt": r.get::<_, String>(5),
    })).collect();
    let _ = std::fs::create_dir_all(dir);
    let day = chrono::Utc::now().format("%Y%m%d").to_string();
    let _ = std::fs::write(dir.join(format!("forms-{day}.json")), serde_json::to_string_pretty(&items).unwrap_or_else(|_| "[]".into()));
}

/// เติมชื่อผู้ตรวจ/ผู้อนุมัติในหน้าพิมพ์
pub async fn print_with_steps(st: &S, id: i64, origin: &str) -> Result<String, String> {
    let client = st.db.get().await.map_err(|e| e.to_string())?;
    let cur = load_row(&client, id).await.ok_or("ไม่พบ")?;
    let schema = load_schema(&client, cur["formCode"].as_str().unwrap_or("")).await.unwrap_or(json!({}));
    let token = cur["checkinToken"].as_str().unwrap_or("").to_string();
    let mut payload = print_payload(&cur, &schema, origin, &token);
    let steps = client.query("SELECT step,action,actor_name,actor_position,comment,at FROM form_steps WHERE record_id=$1 ORDER BY id", &[&id]).await.unwrap_or_default();
    let mut approvals = vec![json!({
        "label": "ผู้จัดทำ",
        "name": cur["preparedName"],
        "at": th_when(cur["submittedAt"].as_str().unwrap_or(cur["createdAt"].as_str().unwrap_or(""))),
        "comment": cur["preparedPosition"],
    })];
    for s in &steps {
        let action: String = s.get(1);
        if action != "pass" { continue; }
        let step: String = s.get(0);
        let label = if step == "approve" { "ผู้อนุมัติ" } else { "ผู้ตรวจสอบ" };
        approvals.push(json!({
            "label": label, "name": s.get::<_, String>(2), "at": th_when(&s.get::<_, String>(5)),
            "comment": format!("{} {}", s.get::<_, String>(3), s.get::<_, String>(4)).trim().to_string(),
        }));
    }
    for f in schema["approval_flow"].as_array().cloned().unwrap_or_default() {
        let label = if f["step"].as_str() == Some("approve") { "ผู้อนุมัติ" } else { "ผู้ตรวจสอบ" };
        if !approvals.iter().any(|a| a["label"].as_str() == Some(label)) {
            approvals.push(json!({ "label": label, "name": "", "at": "", "comment": "" }));
        }
    }
    payload["approvals"] = Value::Array(approvals);
    let body = payload.to_string();
    match call_converter(&st.conv_port, "/form-html", &body).await {
        Ok(v) => Ok(v["html"].as_str().unwrap_or("").to_string()),
        Err(_) => Ok(fallback_html(&payload)),
    }
}
