// ความพร้อมรับการตรวจประเมิน (Audit readiness) — คำนวณจากข้อมูลจริงในระบบ
//   แนวทางคะแนนเดียวกับ AI & Audit Dashboard: ผ่าน = 1, ต้องติดตาม = 0.5, วิกฤต = 0 หารด้วยจำนวนรายการตรวจ
//   แผนผังข้อกำหนด → เอกสารหลักฐานอยู่ใน data/clauses.json (QMR แก้ไขได้)
//   ผลนี้เป็นการคัดกรองว่ามีหลักฐานหรือไม่ ไม่ใช่การตัดสินความสอดคล้องหรือการรับรอง
use crate::eform::{self, AuthUser};
use crate::{err, now_iso, S};
use axum::{
    extract::{Extension, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

pub const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS readiness_runs(
  id BIGSERIAL PRIMARY KEY,
  at TEXT NOT NULL,
  actor_code TEXT NOT NULL,
  actor_name TEXT NOT NULL,
  score BIGINT NOT NULL,
  passed BIGINT NOT NULL,
  warnings BIGINT NOT NULL,
  critical BIGINT NOT NULL,
  total BIGINT NOT NULL,
  body TEXT NOT NULL);
";

const METHODOLOGY: &str = "คะแนนคัดกรอง: ผ่าน = 1, ต้องติดตาม = 0.5, วิกฤต = 0 หารด้วยจำนวนรายการตรวจทั้งหมด · ทะเบียนเอกสารว่าง = 0 · \
การมีเอกสาร/บันทึกในระบบไม่ใช่การตัดสินว่าสอดคล้องกับ ISO 9001 — Auditor ต้องสุ่มตรวจการปฏิบัติจริง";

/// วันที่วันนี้ตามเวลาไทย (YYYY-MM-DD)
fn today() -> chrono::NaiveDate {
    let tz = chrono::FixedOffset::east_opt(7 * 3600).unwrap();
    chrono::Utc::now().with_timezone(&tz).date_naive()
}
fn days_until(date: &str, now: chrono::NaiveDate) -> Option<i64> {
    chrono::NaiveDate::parse_from_str(date.trim(), "%Y-%m-%d").ok().map(|d| (d - now).num_days())
}
fn days_since(rfc: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(rfc)
        .map(|d| (chrono::Utc::now() - d.with_timezone(&chrono::Utc)).num_days())
        .unwrap_or(0)
}

struct Controls(Vec<Value>);
impl Controls {
    /// pass → ผ่าน; ไม่ผ่าน → วิกฤต (critical=true) หรือ ต้องติดตาม
    fn check(&mut self, id: &str, title: &str, category: &str, clause: &str, pass: bool, critical: bool, detail: String, evidence: Vec<String>) {
        let status = if pass { "pass" } else if critical { "critical" } else { "warning" };
        self.0.push(json!({ "id": id, "title": title, "category": category, "clause": clause, "status": status, "detail": detail, "evidence": evidence }));
    }
    fn push_status(&mut self, id: &str, title: &str, category: &str, clause: &str, status: &str, detail: String, evidence: Vec<String>) {
        self.0.push(json!({ "id": id, "title": title, "category": category, "clause": clause, "status": status, "detail": detail, "evidence": evidence }));
    }
}

/// สถานะเอกสารแต่ละฉบับที่ใช้ในการคัดกรอง
struct DocInfo { name: String, active: bool, problem: Option<&'static str>, rev: String, eff: String }

pub async fn evaluate(st: &S) -> Value {
    let now = today();
    let client = st.db.get().await.ok();
    let ctl = crate::doccontrol::control_map(st).await;

    // ---------- สถานะเอกสาร ----------
    let mut docs: HashMap<String, DocInfo> = HashMap::new();
    let (mut missing, mut unreg, mut dup, mut obsolete) = (vec![], vec![], vec![], vec![]);
    for d in &st.manifest {
        let code = d["code"].as_str().unwrap_or("").to_string();
        let rel = d["path"].as_str().unwrap_or("");
        let file_exists = !rel.is_empty() && st.docs_root.join(rel).exists();
        let mut o = d.clone();
        ctl.apply(&code, &mut o);
        let is_missing = d["missing"].as_bool().unwrap_or(false) || !file_exists;
        let is_obsolete = o["docStatus"] == json!("obsolete");
        let is_unreg = d["unreg"].as_bool().unwrap_or(false);
        if is_missing { missing.push(code.clone()); }
        if is_unreg { unreg.push(code.clone()); }
        if d["dup"].as_i64().unwrap_or(0) > 1 { dup.push(code.clone()); }
        if is_obsolete { obsolete.push(code.clone()); }
        let problem = if is_obsolete { Some("ยกเลิกใช้") } else if is_missing { Some("ไฟล์หาย") } else if is_unreg { Some("นอกทะเบียน") } else { None };
        docs.insert(code, DocInfo {
            name: d["name"].as_str().unwrap_or("").to_string(),
            active: !is_obsolete && !is_missing,
            problem,
            rev: o["rev"].as_str().unwrap_or("").to_string(),
            eff: o["eff"].as_str().unwrap_or("").to_string(),
        });
    }

    // เวอร์ชันใช้งานล่าสุดที่แก้ไขโดยไม่ผ่าน DCR (บันทึกก่อนเปิดระบบอนุมัติ)
    let mut unapproved = vec![];
    let mut stale_dcr = vec![];
    let (mut car_rows, mut form_rows, mut training_n) = (vec![], vec![], 0i64);
    if let Some(c) = &client {
        if let Ok(rows) = c.query(
            "SELECT DISTINCT ON (doc_code) doc_code,source,change_id FROM versions ORDER BY doc_code, version DESC", &[],
        ).await {
            for r in rows {
                let code: String = r.get(0);
                let source: String = r.get(1);
                let change_id: i64 = r.get(2);
                if st.by_code.contains_key(&code) && source != "original" && change_id == 0 { unapproved.push(code); }
            }
        }
        if let Ok(rows) = c.query("SELECT doc_code,ref_no,updated_at FROM doc_changes WHERE status IN ('review','approve','returned')", &[]).await {
            for r in rows {
                if days_since(&r.get::<_, String>(2)) > 14 { stale_dcr.push(format!("{} ({})", r.get::<_, String>(1), r.get::<_, String>(0))); }
            }
        }
        car_rows = c.query("SELECT ref_no,status,due_date,assignee_code,action_taken,effectiveness,root_cause,kind,verified_code FROM actions WHERE status<>'cancelled'", &[]).await.unwrap_or_default();
        form_rows = c.query("SELECT ref_no,status,created_at,submitted_at FROM form_records WHERE status IN ('review','approve')", &[]).await.unwrap_or_default();
        training_n = c.query_one("SELECT COUNT(*) FROM training_log", &[]).await.map(|r| r.get(0)).unwrap_or(0);
    }

    let mut cs = Controls(vec![]);
    let total_docs = st.manifest.len();
    cs.check("doc-register", "มีทะเบียนเอกสารควบคุม", "เอกสาร", "7.5", total_docs > 0, true,
        format!("เอกสารในทะเบียน {total_docs} ฉบับ"), vec![]);
    cs.check("doc-missing", "ไฟล์เอกสารควบคุมครบถ้วน", "เอกสาร", "7.5", missing.is_empty(), true,
        if missing.is_empty() { "ทุกฉบับมีไฟล์ในระบบ".into() } else { format!("ไฟล์หาย {} ฉบับ — อัปโหลดผ่านคำขอเปลี่ยนแปลง (DCR)", missing.len()) }, missing.clone());
    cs.check("doc-approval", "ฉบับที่ใช้งานผ่านการตรวจสอบ/อนุมัติ", "เอกสาร", "7.5", unapproved.is_empty(), false,
        if unapproved.is_empty() { "ทุกฉบับที่แก้ไขผ่าน DCR หรือเป็นต้นฉบับ".into() } else { format!("{} ฉบับใช้เวอร์ชันที่แก้ไขก่อนมีขั้นตอนอนุมัติ — ควรส่ง DCR ทบทวนและอนุมัติ", unapproved.len()) }, unapproved);
    cs.check("doc-unreg", "เอกสารขึ้นทะเบียนครบ", "เอกสาร", "7.5", unreg.is_empty(), false,
        format!("นอกทะเบียน {} ฉบับ", unreg.len()), unreg);
    cs.check("doc-dup", "ไม่มีไฟล์ซ้ำซ้อนในแฟ้ม", "เอกสาร", "7.5", dup.is_empty(), false,
        format!("{} ฉบับมีไฟล์ซ้ำหลายชุดในแฟ้ม — เสี่ยงหยิบผิดฉบับ", dup.len()), dup);
    cs.check("dcr-stale", "คำขอเปลี่ยนแปลงเอกสารไม่ค้างนาน", "เอกสาร", "7.5", stale_dcr.is_empty(), false,
        if stale_dcr.is_empty() { "ไม่มี DCR ค้างเกิน 14 วัน".into() } else { format!("DCR ค้างเกิน 14 วัน: {}", stale_dcr.join(", ")) }, vec![]);

    // ---------- CAR / DAR / PAR ----------
    let (mut overdue, mut due_soon, mut no_owner, mut no_record) = (vec![], vec![], vec![], vec![]);
    for r in &car_rows {
        let ref_no: String = r.get(0);
        let status: String = r.get(1);
        let due: String = r.get(2);
        let assignee: String = r.get(3);
        let taken: String = r.get(4);
        let effect: String = r.get(5);
        let cause: String = r.get(6);
        let kind: String = r.get(7);
        let verifier: String = r.get(8);
        if status == "closed" {
            // ปิดที่ถูกต้อง: มีการดำเนินการ, มีสาเหตุ (ยกเว้น DAR), ทวนสอบ "มีประสิทธิผล" โดยผู้อื่น (10.2.1 b, d)
            if taken.trim().is_empty() || effect != "effective" || verifier.is_empty() || verifier == assignee || (kind != "DAR" && cause.trim().is_empty()) { no_record.push(ref_no); }
            continue;
        }
        match days_until(&due, now) {
            Some(d) if d < 0 => overdue.push(ref_no.clone()),
            Some(d) if d <= 14 => due_soon.push(ref_no.clone()),
            None => no_owner.push(ref_no.clone()),
            _ => {}
        }
        if assignee.trim().is_empty() && !no_owner.contains(&ref_no) { no_owner.push(ref_no); }
    }
    let open_n = car_rows.iter().filter(|r| r.get::<_, String>(1) != "closed").count();
    let awaiting: Vec<String> = car_rows.iter().filter(|r| r.get::<_, String>(1) == "verify").map(|r| r.get::<_, String>(0)).collect();
    cs.check("car-records", "มีการบันทึก CAR/DAR/PAR ในระบบ", "การแก้ไข", "10.2", !car_rows.is_empty(), false,
        if car_rows.is_empty() { "ยังไม่มีใบ CAR/DAR/PAR ในระบบเลย — Auditor จะขอดูบันทึกการแก้ไขจริง ถ้าบันทึกอยู่นอกระบบให้นำเข้าก่อนตรวจ".into() }
        else { format!("มีบันทึก {} ใบในระบบ", car_rows.len()) }, vec![]);
    cs.check("car-overdue", "CAR/DAR/PAR ไม่เกินกำหนด", "การแก้ไข", "10.2", overdue.is_empty(), true,
        format!("เกินกำหนด {} ใบ จากที่ยังเปิดอยู่ {open_n} ใบ", overdue.len()), overdue);
    cs.check("car-due", "CAR/DAR/PAR ใกล้ครบกำหนด", "การแก้ไข", "10.2", due_soon.is_empty(), false,
        format!("ครบกำหนดภายใน 14 วัน {} ใบ", due_soon.len()), due_soon);
    cs.check("car-owner", "CAR/DAR/PAR มีผู้รับผิดชอบและกำหนดเสร็จ", "การแก้ไข", "10.2", no_owner.is_empty(), false,
        format!("{} ใบยังไม่ระบุผู้รับผิดชอบหรือวันแล้วเสร็จ", no_owner.len()), no_owner);
    cs.check("car-closure", "ปิด CAR/DAR/PAR ครบ: สาเหตุ การแก้ไข และทวนสอบประสิทธิผล", "การแก้ไข", "10.2", no_record.is_empty(), false,
        format!("{} ใบปิดแล้วแต่ขาดสาเหตุ/การดำเนินการ หรือไม่ได้ทวนสอบประสิทธิผลโดยผู้อื่น", no_record.len()), no_record);
    cs.check("car-verify", "CAR/DAR/PAR ที่รอทวนสอบประสิทธิผล", "การแก้ไข", "10.2", awaiting.is_empty(), false,
        format!("รอผู้ตรวจสอบทวนสอบ {} ใบ", awaiting.len()), awaiting);

    // ---------- บันทึก / การอบรม ----------
    let late_forms: Vec<String> = form_rows.iter().filter(|r| {
        let created: String = r.get(2);
        let submitted: String = r.get(3);
        days_since(if submitted.is_empty() { &created } else { &submitted }) > 7
    }).map(|r| r.get::<_, String>(0)).collect();
    cs.check("form-pending", "บันทึกแบบฟอร์มได้รับการอนุมัติตามเวลา", "บันทึก", "7.5", late_forms.is_empty(), false,
        format!("รอตรวจสอบ/อนุมัติเกิน 7 วัน {} ใบ", late_forms.len()), late_forms);
    cs.check("training-records", "มีบันทึกการอบรมในระบบ", "การอบรม", "7.2", training_n > 0, false,
        if training_n > 0 { format!("บันทึกผู้เข้าอบรม {training_n} รายการ (จาก FM-HR-008 ที่อนุมัติแล้ว)") }
        else { "ยังไม่มีบันทึกการอบรมที่อนุมัติในระบบ — หลักฐานยังอยู่ในแฟ้มกระดาษ/Excel".into() }, vec![]);

    // ---------- ความสามารถของบุคลากร: training matrix (7.2) ----------
    if let Some(c) = &client {
        let tm = crate::training::summary(st, c).await;
        let (reqs, gaps) = (tm["requirements"].as_i64().unwrap_or(0), tm["peopleWithGaps"].as_i64().unwrap_or(0));
        cs.check("training-matrix", "พนักงานผ่านการอบรม/ความสามารถตามที่กำหนด (training matrix)", "การอบรม", "7.2", reqs > 0 && gaps == 0, false,
            if reqs == 0 { "ยังไม่กำหนดข้อกำหนดการอบรมต่อตำแหน่ง/แผนก (เมนู Training matrix)".into() }
            else { format!("ผ่าน {}% ({}/{}) · หมดอายุ {} · พนักงานที่ยังขาด {gaps} คน", tm["rate"], tm["met"], tm["cells"], tm["expired"]) }, vec![]);
    }

    // ---------- การแจกจ่ายและรับทราบเอกสาร (7.5.3, 7.3) ----------
    if let Some(c) = &client {
        let ak = crate::acks::facts(st, c).await;
        let (total, overdue) = (ak["total"].as_i64().unwrap_or(0), ak["overdue"].as_i64().unwrap_or(0));
        cs.check("doc-ack", "ผู้เกี่ยวข้องรับทราบเอกสารฉบับปัจจุบันตามกำหนด", "เอกสาร", "7.5", total > 0 && overdue == 0, false,
            if total == 0 { "ยังไม่มีหลักฐานการแจกจ่าย/รับทราบเอกสารในระบบ — ส่งคำขอรับทราบจากเมนู การรับทราบเอกสาร".into() }
            else { format!("รับทราบแล้ว {}% ({}/{}) · เลยกำหนด {overdue} คำขอ", ak["rate"], ak["acknowledged"], total) },
            ak["overdueDocs"].as_array().cloned().unwrap_or_default().iter().filter_map(|x| x.as_str().map(String::from)).collect());
    }

    // ---------- ควบคุม Rev. / สำเนาเอกสาร (7.5.3.2 c–d) ----------
    let rc = crate::revcontrol::evaluate(st).await;
    if let Some(sm) = rc.get("summary") {
        let n = |k: &str| sm[k].as_i64().unwrap_or(0);
        let mism: Vec<String> = rc["issues"].as_array().cloned().unwrap_or_default().iter()
            .filter(|i| i["level"] == json!("bad")).filter_map(|i| i["docCode"].as_str().map(String::from)).collect();
        cs.check("doc-rev-file", "เลข Rev. ในไฟล์และแบบฟอร์มออนไลน์ตรงกับทะเบียน", "เอกสาร", "7.5", mism.is_empty(), false,
            if mism.is_empty() { format!("ตรวจเนื้อหาไฟล์แล้ว {} ฉบับ — ไม่พบ Rev. ที่ขัดกับทะเบียน", n("scanned")) }
            else { format!("{} ฉบับระบุ Rev. ไม่ตรงทะเบียน — ตรวจที่เมนู ควบคุม Rev. / สำเนา", mism.len()) }, mism);
        let overdue: Vec<String> = rc["recall"].as_array().cloned().unwrap_or_default().iter()
            .filter(|c| c["overdue"] == json!(true)).filter_map(|c| c["copyNo"].as_str().map(String::from)).collect();
        cs.check("doc-copy-recall", "เรียกคืนสำเนาเอกสารฉบับเก่าครบตามกำหนด", "เอกสาร", "7.5", overdue.is_empty(), false,
            if n("recall") == 0 { format!("ไม่มีสำเนาฉบับเก่าค้างเรียกคืน · สำเนาควบคุมที่ใช้งาน {} ชุด", n("activePrint")) }
            else { format!("ค้างเรียกคืน {} รายการ · เลยกำหนด {} วัน {} รายการ", n("recall"), n("recallDays"), overdue.len()) }, overdue);
    }

    // ---------- การตรวจติดตามภายใน (9.2) ----------
    if let Some(c) = &client {
        let ia = crate::internalaudit::facts(c).await;
        let planned: Vec<String> = ia["planned"].as_array().cloned().unwrap_or_default().iter()
            .flat_map(|a| a["clauses"].as_array().cloned().unwrap_or_default()).filter_map(|x| x.as_str().map(String::from)).collect();
        let catalog: Vec<String> = st.clauses["clauses"].as_array().cloned().unwrap_or_default().iter()
            .filter(|c| !c["excludable"].as_bool().unwrap_or(false)).filter_map(|c| c["clause"].as_str().map(String::from)).collect();
        let missing: Vec<String> = catalog.iter().filter(|cl| !planned.iter().any(|p| *cl == p || cl.starts_with(&format!("{p}.")))).cloned().collect();
        let n_plan = ia["planned"].as_array().map(|a| a.len()).unwrap_or(0);
        cs.check("ia-plan", &format!("แผนตรวจติดตามภายในปี {} ครอบคลุมทุกข้อกำหนด", ia["year"]), "ตรวจติดตาม", "9.2", n_plan > 0 && missing.is_empty(), n_plan == 0,
            if n_plan == 0 { "ยังไม่มีแผนตรวจติดตามภายในของปีนี้ในระบบ (เมนู ตรวจติดตามภายใน)".into() }
            else if missing.is_empty() { format!("{n_plan} การตรวจ ครอบคลุมครบทุกข้อ") }
            else { format!("{n_plan} การตรวจ · ยังไม่ครอบคลุม {} ข้อ: {}", missing.len(), missing.join(", ")) }, vec![]);
        let done = ia["done"].as_array().cloned().unwrap_or_default();
        cs.check("ia-done", "ตรวจติดตามภายในและลงนามรายงานภายใน 12 เดือน", "ตรวจติดตาม", "9.2", !done.is_empty(), true,
            if done.is_empty() { "ยังไม่มีรายงานการตรวจติดตามภายในที่ลงนามในรอบ 12 เดือน".into() } else { format!("รายงานที่ลงนามแล้ว {} ฉบับ", done.len()) },
            done.iter().filter_map(|d| d["refNo"].as_str().map(String::from)).collect());
        let overdue: Vec<String> = ia["overdue"].as_array().cloned().unwrap_or_default().iter().filter_map(|x| x.as_str().map(String::from)).collect();
        cs.check("ia-overdue", "การตรวจติดตามเป็นไปตามกำหนดแผน", "ตรวจติดตาม", "9.2", overdue.is_empty(), false,
            format!("เลยวันที่วางแผนแต่ยังไม่เริ่มตรวจ {} การตรวจ", overdue.len()), overdue);
    }

    // ---------- การทบทวนของฝ่ายบริหาร (9.3) ----------
    if let Some(c) = &client {
        let (last, late) = crate::mreview::readiness_facts(c).await;
        let recent = last.as_ref().and_then(|(_, d)| days_until(d, now)).map(|d| d >= -365).unwrap_or(false);
        cs.check("mr-done", "ทบทวนฝ่ายบริหารที่อนุมัติแล้วภายใน 12 เดือน", "ฝ่ายบริหาร", "9.3", recent, true,
            match &last {
                Some((r, d)) if recent => format!("ครั้งล่าสุด {r} วันที่ {d}"),
                Some((r, d)) => format!("ครั้งล่าสุด {r} วันที่ {d} — เกิน 12 เดือน"),
                None => "ยังไม่มีการทบทวนฝ่ายบริหารที่อนุมัติในระบบ (เมนู ทบทวนฝ่ายบริหาร)".into(),
            }, last.map(|(r, _)| vec![r]).unwrap_or_default());
        cs.check("mr-actions", "งานจากการทบทวนฝ่ายบริหารไม่เกินกำหนด", "ฝ่ายบริหาร", "9.3", late.is_empty(), false,
            format!("งานเลยกำหนด {} งาน", late.len()), late);
    }

    // ---------- วัตถุประสงค์คุณภาพและการวัด (6.2, 9.1) ----------
    if let Some(c) = &client {
        let objs = crate::kpi::active(c).await;
        let no_owner: Vec<String> = objs.iter().filter(|o| o["ownerCode"].as_str().unwrap_or("").is_empty()).filter_map(|o| o["code"].as_str().map(String::from)).collect();
        cs.check("kpi-defined", "มีวัตถุประสงค์คุณภาพที่วัดได้ พร้อมผู้รับผิดชอบ", "วัตถุประสงค์", "6.2", !objs.is_empty() && no_owner.is_empty(), objs.is_empty(),
            if objs.is_empty() { "ยังไม่มีวัตถุประสงค์คุณภาพ/KPI ในระบบ (เมนู วัตถุประสงค์คุณภาพ) — ISO 9001 ข้อ 6.2 กำหนดให้เก็บเป็นเอกสาร".into() }
            else if no_owner.is_empty() { format!("{} ตัวชี้วัดที่ใช้งาน", objs.len()) }
            else { format!("{} ตัวชี้วัดยังไม่มีผู้รับผิดชอบ", no_owner.len()) }, no_owner);
        let missing: Vec<String> = objs.iter().filter(|o| o["missing"] == json!(true))
            .map(|o| format!("{} ({})", o["code"].as_str().unwrap_or(""), o["expectedPeriod"].as_str().unwrap_or(""))).collect();
        cs.check("kpi-results", "บันทึกผลตัวชี้วัดครบตามรอบ", "วัตถุประสงค์", "9.1", !objs.is_empty() && missing.is_empty(), false,
            if objs.is_empty() { "ยังไม่มีตัวชี้วัด".into() }
            else if missing.is_empty() { "ทุกตัวชี้วัดบันทึกผลงวดล่าสุดแล้ว".into() }
            else { format!("ค้างบันทึกผลเกิน {} วันหลังสิ้นงวด {} ตัวชี้วัด", crate::kpi::GRACE_DAYS, missing.len()) }, missing);
        let off: Vec<String> = objs.iter().filter(|o| o["offTargetNoCar"].as_array().map(|a| !a.is_empty()).unwrap_or(false))
            .filter_map(|o| o["code"].as_str().map(String::from)).collect();
        cs.check("kpi-offtarget", "ผลที่ไม่ถึงเป้ามีการแก้ไข (CAR)", "วัตถุประสงค์", "10.2", off.is_empty(), false,
            if off.is_empty() { "ไม่มีผลที่ไม่ถึงเป้าค้างโดยไม่มีการแก้ไข".into() } else { format!("{} ตัวชี้วัดไม่ถึงเป้าแต่ยังไม่เปิด CAR", off.len()) }, off);
    }

    // ---------- ความเสี่ยงและโอกาส (6.1) ----------
    if let Some(c) = &client {
        let items = crate::risks::open_items(c).await;
        let risks_n = items.iter().filter(|r| r["kind"] == json!("risk")).count();
        let opps_n = items.len() - risks_n;
        let refs = |f: &dyn Fn(&Value) -> bool| -> Vec<String> { items.iter().filter(|r| f(r)).filter_map(|r| r["refNo"].as_str().map(String::from)).collect() };
        cs.check("risk-register", "มีทะเบียนความเสี่ยงและโอกาส", "ความเสี่ยง", "6.1", risks_n > 0 && opps_n > 0, false,
            if items.is_empty() { "ยังไม่มีทะเบียนความเสี่ยงและโอกาสในระบบ (เมนู ความเสี่ยงและโอกาส)".into() }
            else { format!("ความเสี่ยง {risks_n} · โอกาส {opps_n} รายการที่ยังติดตาม{}", if opps_n == 0 { " — ข้อ 6.1 ต้องพิจารณาโอกาสด้วย" } else { "" }) }, vec![]);
        let weak = refs(&|r| r["gaps"].as_array().map(|g| !g.is_empty()).unwrap_or(false));
        cs.check("risk-plan", "ความเสี่ยงระดับกลาง–สูงมีแผนจัดการและผู้รับผิดชอบ", "ความเสี่ยง", "6.1", weak.is_empty(), false,
            format!("{} รายการยังขาดผู้รับผิดชอบ/แนวทาง/แผนจัดการ", weak.len()), weak);
        let overdue = refs(&|r| r["reviewOverdue"] == json!(true));
        let high_overdue = items.iter().any(|r| r["reviewOverdue"] == json!(true) && r["level"] == json!("high"));
        cs.check("risk-review", "ทบทวนความเสี่ยงและประสิทธิผลของการจัดการตามกำหนด", "ความเสี่ยง", "6.1", overdue.is_empty(), high_overdue,
            if overdue.is_empty() { "ไม่มีรายการเลยกำหนดทบทวน".into() } else { format!("เลยกำหนดทบทวน {} รายการ{}", overdue.len(), if high_overdue { " (มีระดับสูง)" } else { "" }) }, overdue);
    }

    // ---------- เครื่องมือวัดและการสอบเทียบ (7.1.5) ----------
    if let Some(c) = &client {
        let eq = crate::calibration::active(c).await;
        let codes = |f: &dyn Fn(&Value) -> bool| -> Vec<String> { eq.iter().filter(|e| f(e)).filter_map(|e| e["code"].as_str().map(String::from)).collect() };
        let in_use = eq.iter().filter(|e| e["status"] == json!("in_use")).count();
        let weak = codes(&|e| e["gaps"].as_array().map(|g| !g.is_empty()).unwrap_or(false));
        cs.check("cal-register", "มีทะเบียนเครื่องมือวัดพร้อมช่วงวัด/ความแม่นยำและผู้รับผิดชอบ", "เครื่องมือวัด", "7.1.5", !eq.is_empty() && weak.is_empty(), false,
            if eq.is_empty() { "ยังไม่มีทะเบียนเครื่องมือวัดในระบบ (เมนู เครื่องมือวัดและสอบเทียบ) — ข้อ 7.1.5 ต้องเก็บหลักฐานการสอบเทียบ".into() }
            else if weak.is_empty() { format!("{} เครื่อง (ใช้งาน {in_use})", eq.len()) }
            else { format!("{} เครื่อง · ข้อมูลไม่ครบ {} เครื่อง", eq.len(), weak.len()) }, weak);
        let overdue = codes(&|e| e["overdue"] == json!(true));
        cs.check("cal-due", "เครื่องมือที่ใช้งานอยู่สอบเทียบไม่เลยกำหนด", "เครื่องมือวัด", "7.1.5.2", overdue.is_empty(), !overdue.is_empty(),
            if overdue.is_empty() { format!("ไม่มีเครื่องมือที่ใช้งานเลยกำหนดสอบเทียบ · ไม่เคยสอบเทียบ {} เครื่อง (หยุดใช้งานไว้)", codes(&|e| e["neverCalibrated"] == json!(true)).len()) }
            else { format!("เลยกำหนดสอบเทียบแต่ยังอยู่ในสถานะใช้งาน {} เครื่อง — ต้องหยุดใช้หรือสอบเทียบทันที", overdue.len()) }, overdue);
        let pending = codes(&|e| e["pendingImpact"].as_i64().unwrap_or(0) > 0);
        let no_car = codes(&|e| e["affectedNoCar"].as_i64().unwrap_or(0) > 0);
        let mut ev = pending.clone();
        ev.extend(no_car.iter().filter(|x| !pending.contains(x)).cloned());
        cs.check("cal-impact", "ผลสอบเทียบไม่ผ่านมีการประเมินผลกระทบต่อผลการวัดที่ผ่านมา", "เครื่องมือวัด", "7.1.5.2", ev.is_empty(), false,
            if ev.is_empty() { "ไม่มีผลสอบเทียบไม่ผ่านที่ค้างประเมินผลกระทบ".into() }
            else { format!("ค้างประเมินผลกระทบ {} เครื่อง · กระทบผลการวัดแต่ยังไม่เปิด CAR {} เครื่อง", pending.len(), no_car.len()) }, ev);
    }

    // ---------- ผู้ให้บริการภายนอก (8.4) ----------
    if let Some(c) = &client {
        let sups = crate::suppliers::all(c).await;
        let codes = |f: &dyn Fn(&Value) -> bool| -> Vec<String> { sups.iter().filter(|p| f(p)).filter_map(|p| p["code"].as_str().map(String::from)).collect() };
        let approved = sups.iter().filter(|p| p["status"] == json!("approved")).count();
        let cond = sups.iter().filter(|p| p["status"] == json!("conditional")).count();
        cs.check("sup-list", "มีทะเบียนผู้ขายที่อนุมัติ (Approved supplier list)", "ผู้ขาย", "8.4.1", approved > 0, false,
            if sups.is_empty() { "ยังไม่มีทะเบียนผู้ขายในระบบ (เมนู ผู้ขายและการประเมิน)".into() }
            else { format!("อนุมัติ {approved} · มีเงื่อนไข {cond} · ระงับ {}", sups.len() - approved - cond) }, vec![]);
        let late = codes(&|p| p["evalOverdue"] == json!(true));
        let never = codes(&|p| p["status"] == json!("approved") && p["neverEvaluated"] == json!(true));
        let mut ev = late.clone();
        ev.extend(never.iter().filter(|x| !late.contains(x)).cloned());
        cs.check("sup-eval", "ประเมินผู้ขายตามกำหนด และมีผลประเมินเป็นหลักฐาน", "ผู้ขาย", "8.4.1", ev.is_empty(), false,
            if ev.is_empty() { "ไม่มีผู้ขายที่เลยกำหนดประเมิน".into() }
            else { format!("เลยกำหนดประเมิน {} ราย · อนุมัติโดยยังไม่มีผลประเมินในระบบ {} ราย", late.len(), never.len()) }, ev);
        let poor = codes(&|p| p["poorNoCar"].as_i64().unwrap_or(0) > 0);
        cs.check("sup-car", "ผลประเมินผู้ขายต่ำมีการแก้ไข (CAR)", "ผู้ขาย", "8.4.2", poor.is_empty(), false,
            if poor.is_empty() { "ไม่มีผลประเมินต่ำที่ค้างโดยไม่มีการแก้ไข".into() } else { format!("{} รายมีผลประเมินต่ำแต่ยังไม่เปิด CAR", poor.len()) }, poor);
    }

    // ---------- หลักฐานตามข้อกำหนด ----------
    let catalog: Vec<Value> = st.clauses["clauses"].as_array().cloned().unwrap_or_default();
    let mut clause_out = vec![];
    for c in &catalog {
        let clause = c["clause"].as_str().unwrap_or("").to_string();
        let title = c["title"].as_str().unwrap_or("").to_string();
        let required = c["required"].as_bool().unwrap_or(false);
        let excludable = c["excludable"].as_bool().unwrap_or(false);
        let codes: Vec<String> = c["docs"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
        let mut items = vec![];
        let (mut active, mut issues) = (0, vec![]);
        for code in &codes {
            match docs.get(code) {
                Some(d) => {
                    if d.active { active += 1; }
                    if let Some(p) = d.problem { issues.push(format!("{code} {p}")); }
                    items.push(json!({ "code": code, "name": d.name, "rev": d.rev, "eff": d.eff, "status": d.problem.unwrap_or("ใช้งาน"), "active": d.active }));
                }
                None => {
                    issues.push(format!("{code} ไม่มีในทะเบียน"));
                    items.push(json!({ "code": code, "name": "", "status": "ไม่มีในทะเบียน", "active": false }));
                }
            }
        }
        let (status, detail) = if codes.is_empty() && excludable {
            ("warning", "ไม่มีเอกสาร — ยืนยันว่ายกเว้นข้อนี้ไว้ในขอบเขตระบบ (ข้อ 4.3) พร้อมเหตุผล".to_string())
        } else if active == 0 {
            (if required { "critical" } else { "warning" }, "ไม่พบเอกสารที่ใช้งานได้สำหรับข้อนี้".to_string())
        } else if !issues.is_empty() {
            ("warning", format!("มีหลักฐาน {active} ฉบับ · ต้องแก้: {}", issues.join(", ")))
        } else {
            ("pass", format!("มีหลักฐานที่ใช้งาน {active} ฉบับ"))
        };
        cs.push_status(&format!("clause-{clause}"), &format!("ข้อ {clause} {title}"), "ข้อกำหนด", &clause, status, detail.clone(), codes.clone());
        clause_out.push(json!({
            "clause": clause, "title": title, "required": required, "excludable": excludable,
            "status": status, "detail": detail, "docs": items,
        }));
    }

    // เอกสารในทะเบียนที่ยังไม่ผูกกับข้อกำหนดใด — ช่วย QMR เติมแผนผัง
    let mapped: HashSet<String> = catalog.iter().flat_map(|c| c["docs"].as_array().cloned().unwrap_or_default())
        .filter_map(|x| x.as_str().map(String::from)).collect();
    let mut unmapped: Vec<String> = docs.keys().filter(|k| !mapped.contains(*k)).cloned().collect();
    unmapped.sort();

    let controls = cs.0;
    let n = |s: &str| controls.iter().filter(|c| c["status"] == json!(s)).count();
    let (passed, warnings, critical) = (n("pass"), n("warning"), n("critical"));
    let total = controls.len();
    let score = if total_docs == 0 || total == 0 { 0 } else { (((passed as f64) + (warnings as f64) * 0.5) / (total as f64) * 100.0).round() as i64 };
    let doc_names: HashMap<&String, &String> = docs.iter().map(|(k, v)| (k, &v.name)).collect();
    json!({
        "asOf": now_iso(), "score": score, "passed": passed, "warnings": warnings, "critical": critical, "total": total,
        "controls": controls, "clauses": clause_out, "unmapped": unmapped, "obsolete": obsolete,
        "docNames": doc_names, "methodology": METHODOLOGY,
    })
}

pub async fn get(State(st): State<S>) -> Response {
    Json(evaluate(&st).await).into_response()
}

/// บันทึกผลการคัดกรองครั้งนี้ไว้เป็นหลักฐาน/ดูแนวโน้ม (ผู้ตรวจสอบขึ้นไป)
pub async fn save_run(State(st): State<S>, Extension(u): Extension<AuthUser>) -> Response {
    if !eform::can_review(&u.role) { return err(StatusCode::FORBIDDEN, "เฉพาะผู้ตรวจสอบ ผู้อนุมัติ หรือผู้ดูแลระบบ"); }
    let r = evaluate(&st).await;
    let Ok(c) = st.db.get().await else { return err(StatusCode::INTERNAL_SERVER_ERROR, "เชื่อมต่อฐานข้อมูลไม่ได้") };
    let g = |k: &str| r[k].as_i64().unwrap_or(0);
    let row = c.query_one(
        "INSERT INTO readiness_runs(at,actor_code,actor_name,score,passed,warnings,critical,total,body) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9) RETURNING id",
        &[&now_iso(), &u.emp_code, &u.name, &g("score"), &g("passed"), &g("warnings"), &g("critical"), &g("total"), &r.to_string()],
    ).await;
    match row {
        Ok(row) => {
            let id: i64 = row.get(0);
            let _ = eform::audit(&c, &u, "readiness_run", "readiness", &id.to_string(), &format!("{}%", g("score"))).await;
            Json(json!({ "ok": true, "id": id, "result": r })).into_response()
        }
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, &format!("บันทึกผลไม่สำเร็จ: {e}")),
    }
}

pub async fn runs(State(st): State<S>) -> Response {
    let Ok(c) = st.db.get().await else { return err(StatusCode::INTERNAL_SERVER_ERROR, "เชื่อมต่อฐานข้อมูลไม่ได้") };
    let rows = c.query("SELECT id,at,actor_name,score,passed,warnings,critical,total FROM readiness_runs ORDER BY id DESC LIMIT 24", &[]).await.unwrap_or_default();
    Json(Value::Array(rows.iter().map(|r| json!({
        "id": r.get::<_, i64>(0), "at": r.get::<_, String>(1), "actorName": r.get::<_, String>(2),
        "score": r.get::<_, i64>(3), "passed": r.get::<_, i64>(4), "warnings": r.get::<_, i64>(5),
        "critical": r.get::<_, i64>(6), "total": r.get::<_, i64>(7),
    })).collect())).into_response()
}
