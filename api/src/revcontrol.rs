// ควบคุม Rev. และสำเนาเอกสาร (ISO 9001:2015 ข้อ 7.5.3.2 c–d — ควบคุมการเปลี่ยนแปลง/ฉบับ และกันการนำเอกสารล้าสมัยไปใช้โดยไม่ตั้งใจ)
//   สำเนาควบคุม (พิมพ์): DCC ออกเลขสำเนา CC-<พ.ศ.>-<ลำดับ> ต่อผู้ถือ/จุดใช้งาน → PDF ประทับเลขสำเนา + QR ทุกหน้า
//   ไฟล์ต้นฉบับที่ดาวน์โหลด: บันทึกเป็นสำเนา DL-<พ.ศ.>-<ลำดับ> ต่อคน/ฉบับ (ไฟล์ Excel/Word ที่เก็บไว้ในเครื่องคือที่มาของ "ใช้ Rev. เก่า")
//   ทุกหน้าที่เปิดดู (PDF) ประทับ "สำเนาไม่ควบคุมเมื่อพิมพ์" + QR — สแกนแล้วรู้ทันทีว่ากระดาษในมือยังเป็นฉบับปัจจุบันหรือไม่
//   DCR มีผล (Rev. ใหม่) / ยกเลิกใช้ → สำเนาทุกใบของฉบับเก่าเปลี่ยนเป็น "ต้องเรียกคืน" + งานถึงผู้ถือ จนกว่าจะยืนยันคืน/ทำลาย
//   ฉบับเก่า/ยกเลิกใช้: ดาวน์โหลดไฟล์ต้นฉบับได้เฉพาะผู้ควบคุมเอกสาร (ผู้ตรวจสอบขึ้นไป) และ PDF ติดลายน้ำ SUPERSEDED/OBSOLETE
//   รายงาน DCC: สำเนาฉบับเก่าที่ยังไม่คืน · เลข Rev. ในไฟล์ไม่ตรงทะเบียน · อ้างถึงเอกสารที่ยกเลิกใช้ · แบบฟอร์มออนไลน์ไม่ตรง Rev.
use crate::eform::{self, AuthUser};
use crate::{doccontrol, err, now_iso, pdfview, revscan, sha256_hex, stamp, S};
use axum::{
    extract::{Extension, Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

pub const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS doc_copies(
  id BIGSERIAL PRIMARY KEY,
  copy_no TEXT NOT NULL UNIQUE,
  token TEXT NOT NULL UNIQUE,
  doc_code TEXT NOT NULL,
  version BIGINT NOT NULL,
  rev TEXT NOT NULL DEFAULT '',
  kind TEXT NOT NULL,
  holder_code TEXT NOT NULL DEFAULT '',
  holder_name TEXT NOT NULL DEFAULT '',
  location TEXT NOT NULL DEFAULT '',
  note TEXT NOT NULL DEFAULT '',
  status TEXT NOT NULL DEFAULT 'active',
  issued_by TEXT NOT NULL,
  issued_name TEXT NOT NULL,
  issued_at TEXT NOT NULL,
  prints BIGINT NOT NULL DEFAULT 0,
  recall_reason TEXT NOT NULL DEFAULT '',
  recall_at TEXT NOT NULL DEFAULT '',
  closed_by TEXT NOT NULL DEFAULT '',
  closed_name TEXT NOT NULL DEFAULT '',
  closed_at TEXT NOT NULL DEFAULT '',
  closed_note TEXT NOT NULL DEFAULT '');
CREATE INDEX IF NOT EXISTS idx_copies_doc ON doc_copies(doc_code, status);
CREATE INDEX IF NOT EXISTS idx_copies_holder ON doc_copies(holder_code, status);
CREATE UNIQUE INDEX IF NOT EXISTS uq_copies_file ON doc_copies(doc_code, version, holder_code) WHERE kind='file' AND status IN ('active','recall');
CREATE TABLE IF NOT EXISTS doc_verify(
  token TEXT PRIMARY KEY,
  doc_code TEXT NOT NULL,
  version BIGINT NOT NULL,
  UNIQUE(doc_code, version));
CREATE TABLE IF NOT EXISTS doc_rev_scan(
  key TEXT PRIMARY KEY,
  body TEXT NOT NULL,
  scanned_at TEXT NOT NULL);
";

/// ต้องเรียกคืนสำเนาฉบับเก่าภายในกี่วันหลัง Rev. ใหม่มีผล
pub const RECALL_DAYS: i64 = 14;
const COLS: &str = "id,copy_no,token,doc_code,version,rev,kind,holder_code,holder_name,location,note,status,issued_by,issued_name,issued_at,prints,recall_reason,recall_at,closed_by,closed_name,closed_at,closed_note";

fn db_err() -> Response { err(StatusCode::INTERNAL_SERVER_ERROR, "เชื่อมต่อฐานข้อมูลไม่ได้") }

fn days_since(rfc: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(rfc).map(|d| (chrono::Utc::now() - d.with_timezone(&chrono::Utc)).num_days()).unwrap_or(0)
}

/// วันที่ วว/ดด/พ.ศ. (เวลาไทย) จาก RFC 3339
fn th_day(rfc: &str) -> String {
    let tz = chrono::FixedOffset::east_opt(7 * 3600).unwrap();
    chrono::DateTime::parse_from_rfc3339(rfc).map(|d| {
        let d = d.with_timezone(&tz);
        let y: i64 = d.format("%Y").to_string().parse().unwrap_or(2026) + 543;
        format!("{}/{y}", d.format("%d/%m"))
    }).unwrap_or_default()
}

fn copy_json(st: &S, r: &tokio_postgres::Row) -> Value {
    let g = |i: usize| r.get::<_, String>(i);
    let code = g(3);
    let status = g(11);
    let recall_at = g(17);
    json!({
        "id": r.get::<_, i64>(0), "copyNo": g(1), "token": g(2), "docCode": code,
        "docName": st.by_code.get(&code).and_then(|d| d["name"].as_str()).unwrap_or(""),
        "version": r.get::<_, i64>(4), "rev": g(5), "kind": g(6),
        "holderCode": g(7), "holderName": g(8), "location": g(9), "note": g(10), "status": status,
        "issuedBy": g(12), "issuedName": g(13), "issuedAt": g(14), "prints": r.get::<_, i64>(15),
        "recallReason": g(16), "recallAt": recall_at,
        "closedBy": g(18), "closedName": g(19), "closedAt": g(20), "closedNote": g(21),
        "overdue": status == "recall" && days_since(&recall_at) > RECALL_DAYS,
    })
}

async fn load_copy(c: &impl deadpool_postgres::GenericClient, id: i64) -> Option<tokio_postgres::Row> {
    c.query_opt(&format!("SELECT {COLS} FROM doc_copies WHERE id=$1"), &[&id]).await.ok().flatten()
}

/// รหัสตรวจสอบสั้น (80 บิต) — QR เล็กพอจะพิมพ์ที่ขอบกระดาษแล้วสแกนได้
fn short_token() -> String { eform::new_token("verify")[..20].to_string() }

/// ที่อยู่เว็บสำหรับลิงก์ใน QR: APP_URL (production) ไม่งั้นใช้ host ที่เบราว์เซอร์เรียกมา
pub(crate) fn public_base(h: &HeaderMap) -> String {
    if let Ok(u) = std::env::var("APP_URL") {
        if !u.trim().is_empty() { return u.trim().trim_end_matches('/').to_string(); }
    }
    let host = h.get("x-forwarded-host").or_else(|| h.get(header::HOST)).and_then(|v| v.to_str().ok()).unwrap_or("localhost:5173");
    let https = h.get("x-forwarded-proto").and_then(|v| v.to_str().ok()).map(|v| v.eq_ignore_ascii_case("https")).unwrap_or(false);
    format!("{}://{host}", if https { "https" } else { "http" })
}
fn verify_url(base: &str, token: &str) -> String { format!("{base}/#v={token}") }

/// เวอร์ชันล่าสุด + Rev./วันที่บังคับใช้ของฉบับใช้งาน
pub(crate) async fn current(st: &S, c: &impl deadpool_postgres::GenericClient, code: &str) -> (i64, String, String) {
    let latest: i64 = c.query_one("SELECT COALESCE(MAX(version),0) FROM versions WHERE doc_code=$1", &[&code]).await.map(|r| r.get(0)).unwrap_or(0);
    let d = st.by_code.get(code);
    let (mut rev, mut eff) = (
        d.and_then(|d| d["rev"].as_str()).unwrap_or("").to_string(),
        d.and_then(|d| d["eff"].as_str()).unwrap_or("").to_string(),
    );
    if let Ok(Some(r)) = c.query_opt("SELECT rev,effective_date FROM versions WHERE doc_code=$1 AND rev<>'' ORDER BY version DESC LIMIT 1", &[&code]).await {
        rev = r.get(0);
        let e: String = r.get(1);
        if !e.is_empty() { eff = e; }
    }
    (latest, rev, eff)
}

/// Rev. ของเวอร์ชัน v (เวอร์ชันที่แก้ก่อนมี DCR ไม่มี Rev. ของตัวเอง → ใช้ Rev. ล่าสุดก่อนหน้า หรือ Rev. ในทะเบียน)
async fn rev_at(st: &S, c: &impl deadpool_postgres::GenericClient, code: &str, v: i64) -> String {
    match c.query_opt("SELECT rev FROM versions WHERE doc_code=$1 AND version<=$2 AND rev<>'' ORDER BY version DESC LIMIT 1", &[&code, &v]).await {
        Ok(Some(r)) => r.get(0),
        _ => st.by_code.get(code).and_then(|d| d["rev"].as_str()).unwrap_or("").to_string(),
    }
}

/// รหัสตรวจสอบของเวอร์ชันหนึ่ง (ใช้กับตราประทับ "สำเนาไม่ควบคุม" ในหน้าที่เปิดดู) — สร้างครั้งเดียวต่อเวอร์ชัน
async fn doc_token(c: &impl deadpool_postgres::GenericClient, code: &str, v: i64) -> String {
    let get = || async { c.query_opt("SELECT token FROM doc_verify WHERE doc_code=$1 AND version=$2", &[&code, &v]).await.ok().flatten().map(|r| r.get::<_, String>(0)) };
    if let Some(t) = get().await { return t; }
    let _ = c.execute("INSERT INTO doc_verify(token,doc_code,version) VALUES($1,$2,$3) ON CONFLICT DO NOTHING", &[&short_token(), &code, &v]).await;
    get().await.unwrap_or_default()
}

// ---------- ตราประทับหน้าที่เปิดดู / ล็อกการดาวน์โหลด ----------

/// ตราประทับของ PDF ที่เปิดดูในระบบ: ฉบับปัจจุบัน = "สำเนาไม่ควบคุมเมื่อพิมพ์" · ฉบับเก่า/ยกเลิกใช้ = แถบแดง + ลายน้ำ
pub async fn view_stamp(st: &S, u: &AuthUser, code: &str, v: i64, h: &HeaderMap) -> Option<stamp::Stamp> {
    let c = st.db.get().await.ok()?;
    let (latest, cur_rev, eff) = current(st, &c, code).await;
    let rev = rev_at(st, &c, code, v).await;
    let qr = verify_url(&public_base(h), &doc_token(&c, code, v).await);
    let today = doccontrol::today_th();
    let who = &u.emp_code;
    Some(if doccontrol::is_obsolete(&c, code).await {
        stamp::Stamp {
            headline: "OBSOLETE DOCUMENT - DO NOT USE".into(),
            detail: format!("{code} {rev} | Withdrawn from use, for reference only | Viewed {today} by {who} | Scan QR for status"),
            qr, watermark: "OBSOLETE".into(), alert: true,
        }
    } else if v < latest {
        stamp::Stamp {
            headline: "SUPERSEDED VERSION - DO NOT USE".into(),
            detail: format!("{code} {rev} (version {v}) | Current revision: {cur_rev} | Viewed {today} by {who}"),
            qr, watermark: "SUPERSEDED".into(), alert: true,
        }
    } else {
        let eff = if eff.is_empty() { String::new() } else { format!(" | Eff. {eff}") };
        stamp::Stamp {
            headline: "UNCONTROLLED COPY WHEN PRINTED".into(),
            detail: format!("{code} {cur_rev}{eff} | Printed/viewed {today} by {who} | Scan QR to check this is still the current revision"),
            qr, watermark: String::new(), alert: false,
        }
    })
}

/// ไฟล์ต้นฉบับ (ไม่ว่าจะดาวน์โหลดหรือเปิด inline) ของฉบับเก่า/ยกเลิกใช้ ได้เฉพาะผู้ควบคุมเอกสาร
/// คนอื่นดูฉบับเก่าได้ทาง /pdf ซึ่งประทับ SUPERSEDED/OBSOLETE ทุกหน้า
pub async fn authorize_original(st: &S, u: &AuthUser, code: &str, v: i64) -> Result<(), Response> {
    if eform::can_review(&u.role) { return Ok(()); }
    let Ok(c) = st.db.get().await else { return Err(db_err()) };
    let (latest, cur_rev, _) = current(st, &c, code).await;
    if doccontrol::is_obsolete(&c, code).await {
        return Err(err(StatusCode::FORBIDDEN, "เอกสารนี้ยกเลิกใช้แล้ว — เปิด/ดาวน์โหลดไฟล์ต้นฉบับไม่ได้ (ดูฉบับ PDF ในระบบได้เพื่ออ้างอิง)"));
    }
    if v < latest {
        return Err(err(StatusCode::FORBIDDEN, &format!("เวอร์ชันนี้ไม่ใช่ฉบับใช้งาน — ไฟล์ต้นฉบับได้เฉพาะฉบับปัจจุบัน {cur_rev} (ฉบับเก่าดูเป็น PDF ในระบบได้เพื่ออ้างอิง)")));
    }
    Ok(())
}

/// ดาวน์โหลดไฟล์ต้นฉบับฉบับปัจจุบัน (dl=1) → บันทึกเป็นสำเนาของผู้ดาวน์โหลด (ต้องเรียกคืนเมื่อออก Rev. ใหม่)
/// เรียกหลัง authorize_original เท่านั้น · ผู้ควบคุมเอกสารที่ดาวน์โหลดฉบับเก่า/ยกเลิกใช้ไม่ถูกบันทึก (ไม่ใช่ฉบับที่แจกจ่ายใช้งาน)
pub async fn record_download(st: &S, u: &AuthUser, code: &str, v: i64) {
    let Ok(c) = st.db.get().await else { return };
    let (latest, _, _) = current(st, &c, code).await;
    if v < latest || doccontrol::is_obsolete(&c, code).await { return; }
    let rev = rev_at(st, &c, code, v).await;
    let n = c.execute(
        "UPDATE doc_copies SET prints=prints+1 WHERE kind='file' AND doc_code=$1 AND version=$2 AND holder_code=$3 AND status IN ('active','recall')",
        &[&code, &v, &u.emp_code],
    ).await.unwrap_or(0);
    if n == 0 {
        let year = eform::be_year_now();
        if let Ok(r) = c.query_one(
            "INSERT INTO form_seq(form_code,year_be,last_seq) VALUES('DL',$1,1) ON CONFLICT(form_code,year_be) DO UPDATE SET last_seq=form_seq.last_seq+1 RETURNING last_seq",
            &[&year],
        ).await {
            let seq: i64 = r.get(0);
            let _ = c.execute(
                "INSERT INTO doc_copies(copy_no,token,doc_code,version,rev,kind,holder_code,holder_name,location,note,status,issued_by,issued_name,issued_at,prints)
                 VALUES($1,$2,$3,$4,$5,'file',$6,$7,'',$8,'active',$6,$7,$9,1) ON CONFLICT DO NOTHING",
                &[&format!("DL-{year}-{seq:04}"), &short_token(), &code, &v, &rev, &u.emp_code, &u.name, &"ดาวน์โหลดไฟล์ต้นฉบับ".to_string(), &now_iso()],
            ).await;
        }
    }
}

// ---------- จุดเชื่อมจาก DCR / ยกเลิกใช้ ----------

/// Rev. ใหม่มีผล → สำเนาทุกใบของเวอร์ชันก่อนหน้าต้องเรียกคืน
pub async fn on_effective(c: &impl deadpool_postgres::GenericClient, code: &str, version: i64, new_rev: &str) {
    let n = c.execute(
        "UPDATE doc_copies SET status='recall',recall_reason=$3,recall_at=$4 WHERE doc_code=$1 AND version<$2 AND status='active'",
        &[&code, &version, &format!("ออก {new_rev} แทน"), &now_iso()],
    ).await.unwrap_or(0);
    if n > 0 { println!("  📄 {code}: สำเนาฉบับเก่า {n} รายการต้องเรียกคืน"); }
}

/// ยกเลิกใช้เอกสาร → สำเนาทุกใบต้องเรียกคืน
pub async fn on_obsolete(c: &impl deadpool_postgres::GenericClient, code: &str, reason: &str) {
    let _ = c.execute(
        "UPDATE doc_copies SET status='recall',recall_reason=$2,recall_at=$3 WHERE doc_code=$1 AND status='active'",
        &[&code, &format!("ยกเลิกใช้เอกสาร: {reason}"), &now_iso()],
    ).await;
}

/// นำเอกสารกลับมาใช้ → สำเนาของฉบับปัจจุบันที่ถูกเรียกคืนเพราะยกเลิกใช้ (และยังไม่ได้คืน) กลับมาใช้ได้
pub async fn on_reinstate(c: &impl deadpool_postgres::GenericClient, code: &str) {
    let _ = c.execute(
        "UPDATE doc_copies SET status='active',recall_reason='',recall_at='' WHERE doc_code=$1 AND status='recall' AND recall_reason LIKE 'ยกเลิกใช้เอกสาร%'
           AND version=(SELECT COALESCE(MAX(version),0) FROM versions WHERE doc_code=$1)",
        &[&code],
    ).await;
}

// ---------- handlers: สำเนาควบคุม ----------

#[derive(Deserialize)]
#[allow(non_snake_case)]
pub struct IssueReq { holderCode: Option<String>, location: Option<String>, note: Option<String> }

/// ออกสำเนาควบคุม (ผู้ควบคุมเอกสาร = ผู้ตรวจสอบขึ้นไป) — ฉบับปัจจุบันเท่านั้น
pub async fn issue(Path(code): Path<String>, State(st): State<S>, Extension(u): Extension<AuthUser>, Json(b): Json<IssueReq>) -> Response {
    if !st.by_code.contains_key(&code) { return err(StatusCode::NOT_FOUND, "ไม่พบเอกสาร"); }
    if !eform::can_review(&u.role) { return err(StatusCode::FORBIDDEN, "ออกสำเนาควบคุมได้เฉพาะผู้ควบคุมเอกสาร (ผู้ตรวจสอบขึ้นไป)"); }
    let location = b.location.unwrap_or_default().trim().to_string();
    if location.is_empty() { return err(StatusCode::BAD_REQUEST, "กรุณาระบุจุดใช้งาน/ที่เก็บสำเนา เช่น โรงงาน เครื่องตัดลวด"); }
    let holder_code = b.holderCode.unwrap_or_default().trim().to_string();
    let note = b.note.unwrap_or_default().trim().to_string();
    let (v, _, ext, _) = match crate::resolve(&st, &code, None).await { Ok(x) => x, Err(r) => return r };
    if !pdfview::convertible(&ext) && ext != "pdf" { return err(StatusCode::BAD_REQUEST, "ไฟล์ชนิดนี้ออกเป็นสำเนาควบคุม (PDF) ไม่ได้"); }
    let Ok(c) = st.db.get().await else { return db_err() };
    if doccontrol::is_obsolete(&c, &code).await { return err(StatusCode::CONFLICT, "เอกสารนี้ยกเลิกใช้แล้ว — ออกสำเนาไม่ได้"); }
    let holder_name = if holder_code.is_empty() { String::new() } else {
        match c.query_opt("SELECT name_th FROM employees WHERE emp_code=$1 AND status='active'", &[&holder_code]).await {
            Ok(Some(r)) => r.get(0),
            _ => return err(StatusCode::BAD_REQUEST, "ไม่พบพนักงานผู้ถือสำเนา"),
        }
    };
    let (_, rev, _) = current(&st, &c, &code).await;
    let year = eform::be_year_now();
    let seq: i64 = match c.query_one(
        "INSERT INTO form_seq(form_code,year_be,last_seq) VALUES('CC',$1,1) ON CONFLICT(form_code,year_be) DO UPDATE SET last_seq=form_seq.last_seq+1 RETURNING last_seq",
        &[&year],
    ).await { Ok(r) => r.get(0), Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()) };
    let copy_no = format!("CC-{year}-{seq:04}");
    let row = c.query_one(
        &format!("INSERT INTO doc_copies(copy_no,token,doc_code,version,rev,kind,holder_code,holder_name,location,note,status,issued_by,issued_name,issued_at)
                  VALUES($1,$2,$3,$4,$5,'print',$6,$7,$8,$9,'active',$10,$11,$12) RETURNING {COLS}"),
        &[&copy_no, &short_token(), &code, &v, &rev, &holder_code, &holder_name, &location, &note, &u.emp_code, &u.name, &now_iso()],
    ).await;
    match row {
        Ok(r) => {
            let _ = eform::audit(&c, &u, "copy_issue", "doc_copy", &copy_no, &format!("{code} {rev} → {location} {holder_name}")).await;
            Json(copy_json(&st, &r)).into_response()
        }
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, &format!("ออกสำเนาไม่สำเร็จ: {e}")),
    }
}

/// PDF ของสำเนาควบคุม (ประทับเลขสำเนา + QR ทุกหน้า) — พิมพ์ได้เฉพาะสำเนาที่ยังใช้งาน · นับจำนวนครั้งที่พิมพ์
pub async fn copy_pdf(Path(id): Path<i64>, State(st): State<S>, Extension(u): Extension<AuthUser>, headers: HeaderMap) -> Response {
    let Ok(c) = st.db.get().await else { return db_err() };
    let Some(r) = load_copy(&c, id).await else { return err(StatusCode::NOT_FOUND, "ไม่พบสำเนา") };
    let cp = copy_json(&st, &r);
    if !(eform::can_review(&u.role) || cp["holderCode"].as_str() == Some(u.emp_code.as_str())) {
        return err(StatusCode::FORBIDDEN, "พิมพ์ได้เฉพาะผู้ควบคุมเอกสารหรือผู้ถือสำเนา");
    }
    if cp["kind"] != json!("print") { return err(StatusCode::BAD_REQUEST, "รายการนี้ไม่ใช่สำเนาพิมพ์"); }
    if cp["status"] != json!("active") { return err(StatusCode::CONFLICT, "สำเนานี้ถูกเรียกคืน/ปิดแล้ว — พิมพ์ไม่ได้ (ออกสำเนาใหม่ของฉบับปัจจุบันแทน)"); }
    let code = cp["docCode"].as_str().unwrap_or("").to_string();
    let v = cp["version"].as_i64().unwrap_or(0);
    let (v, abs, ext, filename) = match crate::resolve(&st, &code, Some(&v.to_string())).await { Ok(x) => x, Err(r) => return r };
    let title = st.by_code.get(&code).map(|d| format!("{code} {}", d["name"].as_str().unwrap_or(""))).unwrap_or_else(|| code.clone());
    let pdf = if ext == "pdf" { std::fs::read(&abs).map_err(|e| e.to_string()) } else { pdfview::pdf_bytes(&st, &abs, &ext, title.trim()).await };
    let pdf = match pdf { Ok(p) => p, Err(e) => return err(StatusCode::BAD_GATEWAY, &e) };
    let (_, _, eff) = current(&st, &c, &code).await;
    let s = stamp::Stamp {
        headline: format!("CONTROLLED COPY No. {}", cp["copyNo"].as_str().unwrap_or("")),
        detail: format!("{code} {}{} | Holder: {} | Issued {} | Scan QR to verify the current revision",
            cp["rev"].as_str().unwrap_or(""), if eff.is_empty() { String::new() } else { format!(" | Eff. {eff}") },
            if cp["holderCode"].as_str().unwrap_or("").is_empty() { "-" } else { cp["holderCode"].as_str().unwrap_or("") },
            th_day(cp["issuedAt"].as_str().unwrap_or(""))),
        qr: verify_url(&public_base(&headers), cp["token"].as_str().unwrap_or("")),
        watermark: String::new(),
        alert: false,
    };
    // สำเนาควบคุมที่ไม่มีตราประทับใช้ไม่ได้ — ประทับไม่สำเร็จ = ไม่ส่งไฟล์
    let out = match tokio::task::spawn_blocking(move || stamp::apply(&pdf, &s)).await {
        Ok(Ok(b)) => b,
        Ok(Err(e)) => return err(StatusCode::INTERNAL_SERVER_ERROR, &format!("ประทับตราสำเนาไม่สำเร็จ: {e}")),
        Err(_) => return err(StatusCode::INTERNAL_SERVER_ERROR, "ประทับตราสำเนาไม่สำเร็จ"),
    };
    let prints: i64 = c.query_one("UPDATE doc_copies SET prints=prints+1 WHERE id=$1 RETURNING prints", &[&id]).await.map(|r| r.get(0)).unwrap_or(0);
    let copy_no = cp["copyNo"].as_str().unwrap_or("");
    let _ = eform::audit(&c, &u, "copy_print", "doc_copy", copy_no, &format!("{code} v{v} ครั้งที่ {prints}")).await;
    pdfview::pdf_response(out, &format!("{copy_no} {filename}"))
}

/// ทะเบียนสำเนา — ผู้ตรวจสอบขึ้นไปเห็นทั้งหมด · คนอื่นเห็นเฉพาะที่ตนถือ
pub async fn list(Query(q): Query<HashMap<String, String>>, State(st): State<S>, Extension(u): Extension<AuthUser>) -> Response {
    let Ok(c) = st.db.get().await else { return db_err() };
    let doc = q.get("doc").cloned().unwrap_or_default();
    let mine = q.get("mine").map(|s| s == "1").unwrap_or(false) || !eform::can_review(&u.role);
    let holder = if mine { u.emp_code.clone() } else { String::new() };
    match c.query(
        &format!("SELECT {COLS} FROM doc_copies WHERE ($1='' OR doc_code=$1) AND ($2='' OR holder_code=$2) ORDER BY id DESC LIMIT 3000"),
        &[&doc, &holder],
    ).await {
        Ok(rows) => Json(Value::Array(rows.iter().map(|r| copy_json(&st, r)).collect())).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

#[derive(Deserialize)]
pub struct CloseReq { result: String, note: Option<String> }

/// ปิดสำเนา: คืนแล้ว / ทำลาย(ลบไฟล์)แล้ว / สูญหาย — ผู้ถือหรือผู้ควบคุมเอกสาร
pub async fn close(Path(id): Path<i64>, State(st): State<S>, Extension(u): Extension<AuthUser>, Json(b): Json<CloseReq>) -> Response {
    if !matches!(b.result.as_str(), "returned" | "destroyed" | "lost") { return err(StatusCode::BAD_REQUEST, "ผลการเรียกคืนไม่ถูกต้อง"); }
    let note = b.note.unwrap_or_default().trim().to_string();
    if b.result == "lost" && note.is_empty() { return err(StatusCode::BAD_REQUEST, "กรุณาระบุรายละเอียดกรณีสูญหาย"); }
    let Ok(c) = st.db.get().await else { return db_err() };
    let Some(r) = load_copy(&c, id).await else { return err(StatusCode::NOT_FOUND, "ไม่พบสำเนา") };
    let cp = copy_json(&st, &r);
    if !(eform::can_review(&u.role) || cp["holderCode"].as_str() == Some(u.emp_code.as_str())) {
        return err(StatusCode::FORBIDDEN, "ปิดสำเนาได้เฉพาะผู้ถือหรือผู้ควบคุมเอกสาร");
    }
    let n = c.execute(
        "UPDATE doc_copies SET status=$2,closed_by=$3,closed_name=$4,closed_at=$5,closed_note=$6 WHERE id=$1 AND status IN ('active','recall')",
        &[&id, &b.result, &u.emp_code, &u.name, &now_iso(), &note],
    ).await.unwrap_or(0);
    if n == 0 { return err(StatusCode::CONFLICT, "สำเนานี้ปิดไปแล้ว — โหลดหน้าใหม่"); }
    let copy_no = cp["copyNo"].as_str().unwrap_or("");
    let _ = eform::audit(&c, &u, "copy_close", "doc_copy", copy_no, &format!("{} {} {note}", cp["docCode"].as_str().unwrap_or(""), b.result)).await;
    match load_copy(&c, id).await {
        Some(r) => Json(copy_json(&st, &r)).into_response(),
        None => err(StatusCode::NOT_FOUND, "ไม่พบสำเนา"),
    }
}

// ---------- ตรวจสอบสถานะจาก QR (ไม่ต้อง login) ----------

/// GET /api/public/verify/:token — กระดาษในมือยังเป็นฉบับปัจจุบันหรือไม่ (ไม่เปิดเผยเนื้อหาเอกสาร)
pub async fn verify(Path(token): Path<String>, State(st): State<S>) -> Response {
    if token.is_empty() || token.len() > 40 || !token.chars().all(|c| c.is_ascii_alphanumeric()) { return err(StatusCode::NOT_FOUND, "รหัสตรวจสอบไม่ถูกต้อง"); }
    let Ok(c) = st.db.get().await else { return db_err() };
    let (code, version, copy) = match c.query_opt(&format!("SELECT {COLS} FROM doc_copies WHERE token=$1"), &[&token]).await {
        Ok(Some(r)) => {
            let cp = copy_json(&st, &r);
            let copy = json!({
                "copyNo": cp["copyNo"], "kind": cp["kind"], "rev": cp["rev"], "holderName": cp["holderName"], "location": cp["location"],
                "status": cp["status"], "issuedAt": cp["issuedAt"], "recallReason": cp["recallReason"], "closedAt": cp["closedAt"],
            });
            (cp["docCode"].as_str().unwrap_or("").to_string(), cp["version"].as_i64().unwrap_or(0), Some(copy))
        }
        _ => match c.query_opt("SELECT doc_code,version FROM doc_verify WHERE token=$1", &[&token]).await {
            Ok(Some(r)) => (r.get(0), r.get(1), None),
            _ => return err(StatusCode::NOT_FOUND, "ไม่พบรหัสตรวจสอบนี้ — เอกสารนี้อาจไม่ได้พิมพ์จากระบบควบคุมเอกสาร"),
        },
    };
    let Some(doc) = st.by_code.get(&code) else { return err(StatusCode::NOT_FOUND, "ไม่พบเอกสารในทะเบียน") };
    let (latest, cur_rev, cur_eff) = current(&st, &c, &code).await;
    let rev = match &copy { Some(cp) => cp["rev"].as_str().unwrap_or("").to_string(), None => rev_at(&st, &c, &code, version).await };
    let state = if doccontrol::is_obsolete(&c, &code).await { "obsolete" } else if version < latest { "superseded" } else { "current" };
    Json(json!({
        "docCode": code, "docName": doc["name"], "type": doc["type"], "deptName": doc["deptName"],
        "rev": rev, "version": version, "currentRev": cur_rev, "currentEff": cur_eff, "state": state,
        "copy": copy, "checkedAt": now_iso(),
    })).into_response()
}

// ---------- รายงานสำหรับผู้ควบคุมเอกสาร (DCC) ----------

fn issue_item(st: &S, code: &str, kind: &str, level: &str, detail: String) -> Value {
    json!({ "docCode": code, "docName": st.by_code.get(code).and_then(|d| d["name"].as_str()).unwrap_or(""), "kind": kind, "level": level, "detail": detail })
}
fn revs_text(v: &[u32]) -> String { v.iter().map(|n| format!("Rev.{n}")).collect::<Vec<_>>().join(", ") }

/// ผลการอ่านไฟล์ (แคชตาม SHA-256 ของไฟล์ + รุ่นของกติกา)
async fn scan_cached(c: &impl deadpool_postgres::GenericClient, bytes: Vec<u8>, ext: &str, code: &str) -> Value {
    let key = format!("{}:{code}:{}", revscan::SCAN_VERSION, sha256_hex(&bytes));
    if let Ok(Some(r)) = c.query_opt("SELECT body FROM doc_rev_scan WHERE key=$1", &[&key]).await {
        if let Ok(v) = serde_json::from_str::<Value>(&r.get::<_, String>(0)) { return v; }
    }
    let (ext, owner) = (ext.to_string(), code.to_string());
    let v = match tokio::task::spawn_blocking(move || revscan::scan(&bytes, &ext, &owner)).await {
        Ok(Ok(f)) => json!({ "self": f.self_revs, "head": f.head_revs, "body": f.body_revs, "names": f.name_revs, "selfNames": f.self_name_revs, "codes": f.codes, "error": "" }),
        Ok(Err(e)) => json!({ "self": [], "head": [], "body": [], "names": [], "codes": [], "error": e }),
        Err(_) => json!({ "self": [], "head": [], "body": [], "names": [], "codes": [], "error": "อ่านไฟล์ไม่สำเร็จ" }),
    };
    let _ = c.execute("INSERT INTO doc_rev_scan(key,body,scanned_at) VALUES($1,$2,$3) ON CONFLICT(key) DO UPDATE SET body=$2,scanned_at=$3",
        &[&key, &v.to_string(), &now_iso()]).await;
    v
}

fn nums(v: &Value) -> Vec<u32> { v.as_array().map(|a| a.iter().filter_map(|x| x.as_u64().map(|n| n as u32)).collect()).unwrap_or_default() }

/// คำนวณรายงานทั้งหมดจากข้อมูลจริง (ใช้ทั้งหน้า DCC และหน้าเตรียม Audit)
pub async fn evaluate(st: &S) -> Value {
    let Ok(c) = st.db.get().await else { return json!({ "error": "เชื่อมต่อฐานข้อมูลไม่ได้" }) };
    let ctl = doccontrol::control_map(st).await;
    let mut reg: HashMap<String, (String, bool)> = HashMap::new(); // code → (Rev. ทะเบียน, ยกเลิกใช้)
    for d in &st.manifest {
        let code = d["code"].as_str().unwrap_or("").to_string();
        let mut o = d.clone();
        ctl.apply(&code, &mut o);
        reg.insert(code, (o["rev"].as_str().unwrap_or("").to_string(), o["docStatus"] == json!("obsolete")));
    }

    // สำเนาที่ต้องเรียกคืน + สรุปสำเนาที่ใช้งาน
    let recall: Vec<Value> = c.query(&format!("SELECT {COLS} FROM doc_copies WHERE status='recall' ORDER BY recall_at"), &[]).await
        .map(|rows| rows.iter().map(|r| {
            let mut cp = copy_json(st, r);
            let code = cp["docCode"].as_str().unwrap_or("").to_string();
            cp["currentRev"] = json!(reg.get(&code).map(|x| x.0.clone()).unwrap_or_default());
            cp["days"] = json!(days_since(cp["recallAt"].as_str().unwrap_or("")));
            cp
        }).collect()).unwrap_or_default();
    let active_print: i64 = c.query_one("SELECT COUNT(*) FROM doc_copies WHERE status='active' AND kind='print'", &[]).await.map(|r| r.get(0)).unwrap_or(0);
    let active_file: i64 = c.query_one("SELECT COUNT(*) FROM doc_copies WHERE status='active' AND kind='file'", &[]).await.map(|r| r.get(0)).unwrap_or(0);

    // เลข Rev. ในไฟล์ / ชื่อไฟล์ / การอ้างถึงเอกสารอื่น
    let mut issues: Vec<Value> = vec![];
    let (mut scanned, mut unscannable) = (0, 0);
    for d in &st.manifest {
        let code = d["code"].as_str().unwrap_or("").to_string();
        let Some((reg_rev, obsolete)) = reg.get(&code).cloned() else { continue };
        if obsolete { continue; }
        let Ok((_, abs, ext, filename)) = crate::resolve(st, &code, None).await else { continue };
        let reg_n = revscan::rev_num(&reg_rev);
        if let (Some(f), Some(r)) = (revscan::find_revs(&filename).first(), reg_n) {
            if *f != r { issues.push(issue_item(st, &code, "filename", "warn", format!("ชื่อไฟล์ “{filename}” ระบุ Rev.{f} แต่ทะเบียนเป็น {reg_rev}"))); }
        }
        if !revscan::scannable(&ext) { unscannable += 1; continue; }
        let Ok(bytes) = std::fs::read(&abs) else { continue };
        let f = scan_cached(&c, bytes, &ext, &code).await;
        scanned += 1;
        let (own, head, body, names, own_names) = (nums(&f["self"]), nums(&f["head"]), nums(&f["body"]), nums(&f["names"]), nums(&f["selfNames"]));
        let error = f["error"].as_str().unwrap_or("");
        if !error.is_empty() {
            issues.push(issue_item(st, &code, "unreadable", "info", format!("อ่านเนื้อหาไฟล์ไม่ได้: {error}")));
        } else if let Some(r) = reg_n {
            // ลำดับความน่าเชื่อถือ: Rev. ที่เขียนติดรหัสเอกสารเอง → หัว/ท้ายกระดาษ → ชื่อชีตที่มีรหัสเอกสารเอง → เนื้อหา/เซลล์
            // (เนื้อหาอาจมีตัวอย่าง ตารางประวัติ Rev. หรือ Rev. ของเอกสารอื่น เช่น Master List)
            // Rev. ที่สูงกว่าทะเบียน = แก้ไฟล์เป็นฉบับใหม่โดยไม่ผ่าน DCR หรือทะเบียนยังไม่ปรับ
            let (main, where_) = if !own.is_empty() { (&own, format!("ในไฟล์ระบุ {code}")) }
                else if !head.is_empty() { (&head, "หัว/ท้ายกระดาษในไฟล์ระบุ".to_string()) }
                else if !own_names.is_empty() { (&own_names, format!("ชื่อชีต {code} ระบุ")) }
                else { (&body, "ในไฟล์ระบุ".to_string()) };
            let higher: Vec<u32> = main.iter().copied().filter(|n| *n > r).collect();
            if !higher.is_empty() {
                issues.push(issue_item(st, &code, "rev", "bad", format!("{where_} {} สูงกว่าทะเบียน ({reg_rev}) — แก้ไฟล์โดยไม่ผ่าน DCR หรือทะเบียนยังไม่ปรับ", revs_text(&higher))));
            } else if !main.is_empty() && !main.contains(&r) {
                issues.push(issue_item(st, &code, "rev", "bad", format!("{where_} {} แต่ทะเบียนเป็น {reg_rev}", revs_text(main))));
            } else if main.is_empty() && names.is_empty() {
                issues.push(issue_item(st, &code, "norev", "info", format!("ไม่พบเลข Rev. ในไฟล์ — ควรระบุ {reg_rev} ที่หัวหรือท้ายกระดาษ")));
            } else {
                // ชื่อชีตระบุ Rev. อื่น (เช่น คัดลอกชีตเตรียมฉบับใหม่ไว้) — ไม่ใช่ตัวตัดสิน แต่ควรเก็บให้ตรง
                let odd: Vec<u32> = names.iter().copied().filter(|n| *n != r).collect();
                if !odd.is_empty() {
                    issues.push(issue_item(st, &code, "sheetname", "warn", format!("ชื่อชีตระบุ {} ขณะที่ทะเบียนเป็น {reg_rev} — ตั้งชื่อชีตให้ตรงฉบับปัจจุบัน", revs_text(&odd))));
                } else if main.is_empty() && !names.contains(&r) {
                    issues.push(issue_item(st, &code, "norev", "info", format!("ไม่พบเลข Rev. ในไฟล์ — ควรระบุ {reg_rev} ที่หัวหรือท้ายกระดาษ")));
                }
            }
        }
        for other in f["codes"].as_array().cloned().unwrap_or_default().iter().filter_map(|x| x.as_str().map(String::from)) {
            if other == code || code.starts_with(&format!("{other}-")) || other.starts_with(&format!("{code}-")) { continue; }
            match reg.get(&other) {
                Some((_, true)) => issues.push(issue_item(st, &code, "ref", "warn", format!("อ้างถึง {other} ซึ่งยกเลิกใช้แล้ว — ปรับเนื้อหาผ่าน DCR"))),
                None => issues.push(issue_item(st, &code, "ref", "info", format!("อ้างถึง {other} ซึ่งไม่มีในทะเบียนเอกสาร"))),
                _ => {}
            }
        }
    }

    // แบบฟอร์มออนไลน์ (e-form) เทียบกับ Rev. ในทะเบียน
    let latest_schema = eform::latest_schema_revs(&c).await;
    for (fcode, frev) in &latest_schema {
        let Some((reg_rev, obsolete)) = reg.get(fcode) else { continue };
        if *obsolete {
            issues.push(issue_item(st, fcode, "eform", "warn", "แบบฟอร์มนี้ยกเลิกใช้แล้ว — ระบบไม่ให้สร้างบันทึกออนไลน์ใหม่".into()));
        } else if revscan::rev_num(frev) != revscan::rev_num(reg_rev) {
            issues.push(issue_item(st, fcode, "eform", "bad", format!("แบบฟอร์มออนไลน์ใช้ {frev} แต่ทะเบียนเป็น {reg_rev} — ปรับ schema หรือทะเบียนให้ตรงกัน")));
        }
    }
    if let Ok(rows) = c.query("SELECT form_code,form_rev,COUNT(*) FROM form_records WHERE status IN ('draft','returned') GROUP BY 1,2", &[]).await {
        for r in rows {
            let (fcode, frev, n): (String, String, i64) = (r.get(0), r.get(1), r.get(2));
            if let Some(cur) = latest_schema.get(&fcode) {
                if *cur != frev {
                    issues.push(issue_item(st, &fcode, "eform-draft", "info", format!("ร่าง/ใบตีกลับ {n} ใบยังใช้แบบฟอร์ม {frev} (ปัจจุบัน {cur}) — ต้องย้ายไปฉบับปัจจุบันก่อนส่ง")));
                }
            }
        }
    }

    let count = |lvl: &str| issues.iter().filter(|i| i["level"] == json!(lvl)).count();
    let overdue = recall.iter().filter(|r| r["overdue"] == json!(true)).count();
    let summary = json!({
        "recall": recall.len(), "recallOverdue": overdue, "activePrint": active_print, "activeFile": active_file,
        "bad": count("bad"), "warn": count("warn"), "info": count("info"),
        "revMismatch": issues.iter().filter(|i| i["kind"] == json!("rev")).count(),
        "scanned": scanned, "unscannable": unscannable, "recallDays": RECALL_DAYS,
    });
    json!({ "summary": summary, "recall": recall, "issues": issues })
}

/// GET /api/revcontrol (ผู้ตรวจสอบขึ้นไป)
pub async fn report(State(st): State<S>, Extension(u): Extension<AuthUser>) -> Response {
    if !eform::can_review(&u.role) { return err(StatusCode::FORBIDDEN, "เฉพาะผู้ควบคุมเอกสาร (ผู้ตรวจสอบขึ้นไป)"); }
    Json(evaluate(&st).await).into_response()
}
