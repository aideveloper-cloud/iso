// งานของฉัน / การแจ้งเตือน — รวมทุกงานที่รอผู้ใช้คนนี้จากทุกโมดูล (คำนวณสดจากข้อมูลจริง ไม่มีคิวแจ้งเตือนแยก)
//   DCR รออนุมัติ · แบบฟอร์มรออนุมัติ · เอกสารที่ต้องรับทราบ · CAR/DAR/PAR ที่รับผิดชอบ/รอทวนสอบ
//   · งานจากการทบทวนฝ่ายบริหาร · การตรวจติดตามที่เป็นหัวหน้าผู้ตรวจ · ผล KPI ที่ต้องบันทึก · ความเสี่ยงที่ถึงกำหนดทบทวน
//   · เครื่องมือวัดที่ถึงกำหนดสอบเทียบ/ค้างประเมินผลกระทบ · ผู้ขายที่ถึงกำหนดประเมินซ้ำ/ผลประเมินต่ำที่ยังไม่มี CAR
//   · สำเนาเอกสารฉบับเก่าที่ต้องคืน/ทำลาย (ผู้ถือ) และที่เลยกำหนดเรียกคืน (ผู้ควบคุมเอกสาร)
//   อีเมลสรุปงานรายวัน (เลือกรับเอง) ส่งเมื่อตั้งค่า SMTP_HOST — ไม่ตั้ง = ใช้เฉพาะในระบบ
use crate::eform::{self, AuthUser};
use crate::{calibration, capa, err, kpi, revcontrol, risks, suppliers, S};
use axum::{
    extract::{Extension, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

pub const SCHEMA: &str = "
ALTER TABLE IF EXISTS employees ADD COLUMN IF NOT EXISTS email TEXT NOT NULL DEFAULT '';
ALTER TABLE IF EXISTS employees ADD COLUMN IF NOT EXISTS notify_email BOOLEAN NOT NULL DEFAULT false;
ALTER TABLE IF EXISTS employees ADD COLUMN IF NOT EXISTS notify_sent TEXT NOT NULL DEFAULT '';
";

/// งานที่ครบกำหนดภายในกี่วันนับเป็น "ใกล้ครบกำหนด"
const SOON_DAYS: i64 = 7;

fn s(v: &Value, k: &str) -> String { v[k].as_str().unwrap_or("").to_string() }
fn plus_days(d: &str, n: i64) -> String {
    chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").map(|x| (x + chrono::Duration::days(n)).format("%Y-%m-%d").to_string()).unwrap_or_default()
}

/// ระดับความเร่งด่วนจากกำหนดเสร็จ: overdue · soon · todo
fn urgency(due: &str, today: &str) -> &'static str {
    if due.is_empty() { "todo" } else if due < today { "overdue" } else if due <= plus_days(today, SOON_DAYS).as_str() { "soon" } else { "todo" }
}

fn item(kind: &str, tab: &str, refno: &str, title: &str, detail: String, due: &str, level: &str) -> Value {
    json!({ "kind": kind, "tab": tab, "ref": refno, "title": title, "detail": detail, "due": due, "level": level })
}

/// งานทั้งหมดที่รอผู้ใช้คนนี้ — เรียงจากเลยกำหนด → ใกล้ครบกำหนด → อื่น ๆ
pub async fn items(st: &S, u: &AuthUser) -> Vec<Value> {
    let today = capa::today();
    let mut out = vec![];
    let Ok(c) = st.db.get().await else { return out };
    let me = u.emp_code.as_str();

    // คำขอเปลี่ยนแปลงเอกสาร (DCR) ที่รออนุมัติ
    for ch in crate::doccontrol::pending_items(st, u).await {
        let code = s(&ch, "docCode");
        let name = st.by_code.get(&code).and_then(|d| d["name"].as_str()).unwrap_or("").to_string();
        out.push(item("dcr", "dcr", &s(&ch, "refNo"), &format!("รออนุมัติ: {code} {name}"), format!("ผู้ขอ {} · {}", s(&ch, "preparedName"), s(&ch, "reason")), "", "todo"));
    }
    // แบบฟอร์มที่รออนุมัติ
    for f in eform::pending_items(&c, u).await.unwrap_or_default() {
        out.push(item("form", "forms", &s(&f, "refNo"), &format!("รออนุมัติ: {} {}", s(&f, "formCode"), s(&f, "title")), format!("ผู้จัดทำ {}", s(&f, "preparedName")), "", "todo"));
    }
    // คำขอเปลี่ยนแปลงเอกสาร/แบบฟอร์มของตนที่ถูกตีกลับ — ต้องแก้แล้วส่งใหม่ (หรือยกเลิก)
    if let Ok(rows) = c.query("SELECT ref_no,doc_code FROM doc_changes WHERE prepared_by=$1 AND status='returned' ORDER BY id", &[&me]).await {
        for r in rows {
            let (refno, code): (String, String) = (r.get(0), r.get(1));
            let name = st.by_code.get(&code).and_then(|d| d["name"].as_str()).unwrap_or("").to_string();
            out.push(item("dcr", "dcr", &refno, &format!("ถูกตีกลับ แก้แล้วส่งใหม่: {code} {name}"), String::new(), "", "soon"));
        }
    }
    if let Ok(rows) = c.query("SELECT ref_no,form_code,title FROM form_records WHERE prepared_by=$1 AND status='returned' ORDER BY id", &[&me]).await {
        for r in rows {
            let (refno, code, title): (String, String, String) = (r.get(0), r.get(1), r.get(2));
            out.push(item("form", "forms", &refno, &format!("แบบฟอร์มถูกตีกลับ: {code} {title}"), String::new(), "", "soon"));
        }
    }
    // หลักสูตรอบรมที่ตนยังขาด/หมดอายุ (Training matrix ข้อ 7.2)
    for (title, detail) in crate::training::my_gaps(st, &c, me).await {
        out.push(item("train", "tm", "", &format!("อบรมตามข้อกำหนด: {title}"), detail, "", "todo"));
    }
    // เอกสารที่ต้องรับทราบ
    if let Ok(rows) = c.query("SELECT doc_code,rev,due_date FROM doc_acks WHERE emp_code=$1 AND status='pending' ORDER BY due_date", &[&me]).await {
        for r in rows {
            let (code, rev, due): (String, String, String) = (r.get(0), r.get(1), r.get(2));
            let name = st.by_code.get(&code).and_then(|d| d["name"].as_str()).unwrap_or("").to_string();
            out.push(item("ack", "ack", &code, &format!("รับทราบเอกสาร {code} {name}"), if rev.is_empty() { String::new() } else { format!("ฉบับ {rev}") }, &due, urgency(&due, &today)));
        }
    }
    // สำเนาฉบับเก่าที่ตนถือ (กระดาษ/ไฟล์ที่ดาวน์โหลดไว้) — ต้องคืน/ทำลายหลังออก Rev. ใหม่หรือยกเลิกใช้
    if let Ok(rows) = c.query("SELECT copy_no,doc_code,rev,kind,location,recall_at,recall_reason FROM doc_copies WHERE holder_code=$1 AND status='recall' ORDER BY recall_at", &[&me]).await {
        for r in rows {
            let (copy_no, code, rev, kind, location, at, reason): (String, String, String, String, String, String, String) =
                (r.get(0), r.get(1), r.get(2), r.get(3), r.get(4), r.get(5), r.get(6));
            let due = plus_days(at.get(..10).unwrap_or(""), revcontrol::RECALL_DAYS);
            let title = if kind == "file" { format!("ลบไฟล์ {code} {rev} ที่ดาวน์โหลดไว้ แล้วใช้ฉบับปัจจุบัน") } else { format!("คืน/ทำลายสำเนา {code} {rev} ({copy_no})") };
            let detail = if location.is_empty() { reason } else { format!("{reason} · {location}") };
            out.push(item("copy", "rev", &copy_no, &title, detail, &due, urgency(&due, &today)));
        }
    }
    // ผู้ควบคุมเอกสาร: สำเนาฉบับเก่าของผู้อื่นที่เลยกำหนดเรียกคืน
    if eform::can_approve(&u.role) {
        let cutoff = (chrono::Utc::now() - chrono::Duration::days(revcontrol::RECALL_DAYS)).to_rfc3339();
        let n: i64 = c.query_one("SELECT COUNT(*) FROM doc_copies WHERE status='recall' AND holder_code<>$1 AND recall_at<$2", &[&me, &cutoff])
            .await.map(|r| r.get(0)).unwrap_or(0);
        if n > 0 {
            out.push(item("copy", "rev", "", &format!("ติดตามเรียกคืนสำเนาเอกสารฉบับเก่า {n} รายการ"), format!("เลยกำหนด {} วันหลังออก Rev. ใหม่", revcontrol::RECALL_DAYS), "", "overdue"));
        }
    }
    // CAR/DAR/PAR — ที่ตนรับผิดชอบ และที่รอทวนสอบประสิทธิผล (ผู้อนุมัติที่ไม่ใช่ผู้รับผิดชอบ)
    if let Ok(rows) = c.query("SELECT ref_no,kind,title,status,due_date,assignee_code,verify_due FROM actions WHERE status IN ('open','in_progress','verify')", &[]).await {
        for r in rows {
            let (refno, kind, title, status, due, assignee, vdue): (String, String, String, String, String, String, String) =
                (r.get(0), r.get(1), r.get(2), r.get(3), r.get(4), r.get(5), r.get(6));
            if status == "verify" {
                if eform::can_approve(&u.role) && (assignee != me || eform::is_admin(&u.role)) {
                    out.push(item("capa", "act", &refno, &format!("ทวนสอบประสิทธิผล {kind}: {title}"), String::new(), &vdue, urgency(&vdue, &today)));
                }
            } else if assignee == me {
                let step = if status == "open" { "เริ่มดำเนินการ" } else { "ดำเนินการแก้ไข" };
                out.push(item("capa", "act", &refno, &format!("{step} {kind}: {title}"), String::new(), &due, urgency(&due, &today)));
            }
        }
    }
    // งานจากการทบทวนฝ่ายบริหาร
    if let Ok(rows) = c.query(
        "SELECT a.ref_no,a.title,a.due_date,m.ref_no FROM mr_actions a JOIN mr_meetings m ON m.id=a.meeting_id
         WHERE a.owner_code=$1 AND a.status IN ('not_started','in_progress') AND m.status='monitoring' ORDER BY a.due_date", &[&me],
    ).await {
        for r in rows {
            let (refno, title, due, mref): (String, String, String, String) = (r.get(0), r.get(1), r.get(2), r.get(3));
            out.push(item("mr", "mr", &refno, &format!("งานจากการทบทวนฝ่ายบริหาร: {title}"), format!("การประชุม {mref}"), &due, urgency(&due, &today)));
        }
    }
    if eform::can_approve(&u.role) {
        // รายงานการประชุมทบทวนที่รออนุมัติ (ผู้จัดทำอนุมัติของตนเองไม่ได้ — ระบบตรวจซ้ำตอนลงนาม)
        let admin = eform::is_admin(&u.role);
        if let Ok(rows) = c.query(
            "SELECT m.ref_no,m.period FROM mr_meetings m WHERE m.status='approve' AND ($2 OR COALESCE((SELECT s.emp_code FROM mr_signatures s
               WHERE s.meeting_id=m.id AND s.stage='prepared' ORDER BY s.id DESC LIMIT 1),'')<>$1) ORDER BY m.id", &[&me, &admin],
        ).await {
            for r in rows {
                let (refno, period): (String, String) = (r.get(0), r.get(1));
                out.push(item("mr", "mr", &refno, &format!("อนุมัติรายงานการทบทวนฝ่ายบริหาร {period}"), String::new(), "", "todo"));
            }
        }
        // งานจากการทบทวนที่เสร็จแล้ว รอทวนสอบประสิทธิผล (ไม่ใช่งานของตน)
        if let Ok(rows) = c.query(
            "SELECT a.ref_no,a.title FROM mr_actions a JOIN mr_meetings m ON m.id=a.meeting_id
             WHERE a.status='completed' AND a.effectiveness='pending' AND a.owner_code<>$1 AND m.status='monitoring' ORDER BY a.id", &[&me],
        ).await {
            for r in rows {
                let (refno, title): (String, String) = (r.get(0), r.get(1));
                out.push(item("mr", "mr", &refno, &format!("ทวนสอบประสิทธิผลงานจากการทบทวน: {title}"), String::new(), "", "todo"));
            }
        }
        // การตรวจติดตามที่รายงานแล้ว และ CAR ปิดครบ — รอผู้อนุมัติปิดการตรวจ
        if let Ok(rows) = c.query(
            "SELECT a.ref_no,a.title FROM ia_audits a WHERE a.status='reported' AND ($1 OR (a.lead_code<>$2 AND a.team NOT LIKE '%\"' || $2 || '\"%')) AND NOT EXISTS (
               SELECT 1 FROM ia_items i JOIN actions x ON x.id=i.car_id WHERE i.audit_id=a.id AND x.status NOT IN ('closed','cancelled'))
             ORDER BY a.id", &[&admin, &me],
        ).await {
            for r in rows {
                let (refno, title): (String, String) = (r.get(0), r.get(1));
                out.push(item("ia", "ia", &refno, &format!("ปิดการตรวจติดตาม (CAR ปิดครบแล้ว): {title}"), String::new(), "", "todo"));
            }
        }
    }
    // การตรวจติดตามภายในที่ตนเป็นหัวหน้าผู้ตรวจ และยังไม่ลงนามรายงาน
    if let Ok(rows) = c.query("SELECT ref_no,title,planned_date,status FROM ia_audits WHERE lead_code=$1 AND status IN ('planned','in_progress') ORDER BY planned_date", &[&me]).await {
        for r in rows {
            let (refno, title, planned, status): (String, String, String, String) = (r.get(0), r.get(1), r.get(2), r.get(3));
            let step = if status == "planned" { "ตรวจติดตามตามแผน" } else { "สรุปและลงนามรายงานตรวจติดตาม" };
            // ระหว่างตรวจไม่มีกำหนดเสร็จ — แสดงเป็นงานค้าง
            let due = if status == "planned" { planned.as_str() } else { "" };
            out.push(item("ia", "ia", &refno, &format!("{step}: {title}"), String::new(), due, urgency(due, &today)));
        }
    }
    // ผล KPI ที่ต้องบันทึก (ผู้รับผิดชอบตัวชี้วัด)
    for o in kpi::active(&c).await {
        if s(&o, "ownerCode") != me { continue; }
        if o["missing"] == json!(true) || o["pending"] == json!(true) {
            let due = s(&o, "dueDate");
            out.push(item("kpi", "kpi", &s(&o, "code"), &format!("บันทึกผล KPI งวด {}: {}", s(&o, "expectedPeriod"), s(&o, "title")), String::new(), &due, urgency(&due, &today)));
        }
        let off = o["offTargetNoCar"].as_array().cloned().unwrap_or_default();
        if !off.is_empty() {
            let periods: Vec<String> = off.iter().filter_map(|p| p.as_str().map(String::from)).collect();
            out.push(item("kpi", "kpi", &s(&o, "code"), &format!("KPI ไม่ถึงเป้า ยังไม่มีการแก้ไข: {}", s(&o, "title")), format!("งวด {}", periods.join(", ")), "", "todo"));
        }
    }
    // ความเสี่ยง/โอกาสที่ถึงกำหนดทบทวน (ผู้รับผิดชอบ)
    for r in risks::open_items(&c).await {
        if s(&r, "ownerCode") != me { continue; }
        let due = s(&r, "reviewDue");
        let lvl = urgency(&due, &today);
        if lvl != "todo" {
            out.push(item("risk", "risk", &s(&r, "refNo"), &format!("ทบทวน{}: {}", if r["kind"] == json!("opportunity") { "โอกาส" } else { "ความเสี่ยง" }, s(&r, "title")), format!("ระดับ{}", risks::level_th(&s(&r, "level"))), &due, lvl));
        }
    }
    // เครื่องมือวัด (ผู้รับผิดชอบ): ถึงกำหนดสอบเทียบภายใน 7 วัน/เลยกำหนด (เฉพาะที่ใช้งาน) · ยังไม่เคยสอบเทียบ · ผลไม่ผ่านที่ค้างประเมินผลกระทบ
    for e in calibration::active(&c).await {
        if s(&e, "ownerCode") != me { continue; }
        let (code, name, due) = (s(&e, "code"), s(&e, "name"), s(&e, "nextDue"));
        if e["pendingImpact"].as_i64().unwrap_or(0) > 0 {
            out.push(item("cal", "cal", &code, &format!("ประเมินผลกระทบผลสอบเทียบไม่ผ่าน: {code} {name}"), "ทบทวนผลการวัดที่ผ่านมาตั้งแต่สอบเทียบผ่านครั้งก่อน".into(), "", "overdue"));
        } else if e["neverCalibrated"] == json!(true) {
            out.push(item("cal", "cal", &code, &format!("สอบเทียบก่อนใช้งาน: {code} {name}"), s(&e, "location"), "", "todo"));
        } else if e["status"] == json!("in_use") {
            let lvl = urgency(&due, &today);
            if lvl != "todo" { out.push(item("cal", "cal", &code, &format!("สอบเทียบ: {code} {name}"), s(&e, "location"), &due, lvl)); }
        }
        if e["affectedNoCar"].as_i64().unwrap_or(0) > 0 {
            out.push(item("cal", "cal", &code, &format!("ผลสอบเทียบไม่ผ่านกระทบผลการวัด ยังไม่เปิด CAR: {code} {name}"), String::new(), "", "todo"));
        }
    }
    // ผู้ขาย (ผู้รับผิดชอบประเมิน): ถึงกำหนดประเมินซ้ำภายใน 7 วัน/เลยกำหนด · ผลประเมินต่ำที่ยังไม่เปิด CAR
    for p in suppliers::all(&c).await {
        if s(&p, "ownerCode") != me || p["status"] == json!("blocked") { continue; }
        let (code, name, due) = (s(&p, "code"), s(&p, "name"), s(&p, "nextEval"));
        let lvl = urgency(&due, &today);
        if lvl != "todo" {
            let what = if p["neverEvaluated"] == json!(true) { "ประเมินผู้ขายครั้งแรก" } else { "ประเมินผู้ขายซ้ำ" };
            out.push(item("sup", "sup", &code, &format!("{what}: {code} {name}"), format!("สถานะ{}", suppliers::status_th(&s(&p, "status"))), &due, lvl));
        }
        if p["poorNoCar"].as_i64().unwrap_or(0) > 0 {
            out.push(item("sup", "sup", &code, &format!("ผลประเมินผู้ขายต่ำ ยังไม่เปิด CAR: {code} {name}"), format!("เกรดล่าสุด {}", s(&p, "lastGrade")), "", "todo"));
        }
    }

    let rank = |l: &str| match l { "overdue" => 0, "soon" => 1, _ => 2 };
    out.sort_by(|a, b| rank(a["level"].as_str().unwrap_or("")).cmp(&rank(b["level"].as_str().unwrap_or("")))
        .then_with(|| { let (x, y) = (s(a, "due"), s(b, "due")); (x.is_empty(), x).cmp(&(y.is_empty(), y)) }));
    out
}

// ---------- อีเมล (SMTP) ----------

struct Smtp { host: String, port: u16, user: String, pass: String, from: String, tls: String }

fn smtp_config() -> Option<Smtp> {
    let host = std::env::var("SMTP_HOST").ok().filter(|h| !h.trim().is_empty())?;
    let port: u16 = std::env::var("SMTP_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(587);
    let user = std::env::var("SMTP_USER").unwrap_or_default();
    let from = std::env::var("SMTP_FROM").ok().filter(|f| !f.trim().is_empty()).unwrap_or_else(|| user.clone());
    // tls: starttls (587) · tls (465) · none (เซิร์ฟเวอร์ภายใน)
    let tls = std::env::var("SMTP_TLS").ok().filter(|t| !t.is_empty()).unwrap_or_else(|| if port == 465 { "tls".into() } else { "starttls".into() });
    Some(Smtp { host, port, user, pass: std::env::var("SMTP_PASS").unwrap_or_default(), from, tls })
}

fn app_url() -> String {
    std::env::var("APP_URL").ok().filter(|u| !u.trim().is_empty()).unwrap_or_else(|| "https://iso.kgarden.co.th".into()).trim_end_matches('/').to_string()
}

async fn send_mail(to: &str, subject: &str, html: String) -> Result<(), String> {
    use lettre::message::{header::ContentType, Mailbox};
    use lettre::transport::smtp::authentication::Credentials;
    use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
    let cfg = smtp_config().ok_or("ยังไม่ได้ตั้งค่า SMTP บนเซิร์ฟเวอร์")?;
    let from: Mailbox = format!("ระบบเอกสาร ISO <{}>", cfg.from).parse().map_err(|e| format!("SMTP_FROM ไม่ถูกต้อง: {e}"))?;
    let to: Mailbox = to.parse().map_err(|e| format!("อีเมลผู้รับไม่ถูกต้อง: {e}"))?;
    let msg = Message::builder().from(from).to(to).subject(subject).header(ContentType::TEXT_HTML).body(html).map_err(|e| e.to_string())?;
    let builder = match cfg.tls.as_str() {
        "tls" => AsyncSmtpTransport::<Tokio1Executor>::relay(&cfg.host).map_err(|e| e.to_string())?,
        "none" => AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&cfg.host),
        _ => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&cfg.host).map_err(|e| e.to_string())?,
    }.port(cfg.port).timeout(Some(std::time::Duration::from_secs(20)));
    let builder = if cfg.user.is_empty() { builder } else { builder.credentials(Credentials::new(cfg.user, cfg.pass)) };
    builder.build().send(msg).await.map(|_| ()).map_err(|e| format!("ส่งอีเมลไม่สำเร็จ: {e}"))
}

fn esc(t: &str) -> String { t.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;") }

fn digest_html(name: &str, list: &[Value]) -> String {
    let label = |l: &str| match l { "overdue" => ("เลยกำหนด", "#b42318"), "soon" => ("ใกล้ครบกำหนด", "#b54708"), _ => ("รอดำเนินการ", "#475467") };
    let rows: String = list.iter().map(|it| {
        let (txt, color) = label(it["level"].as_str().unwrap_or(""));
        format!("<tr><td style=\"padding:6px 8px;border-bottom:1px solid #eaecf0;color:{color};white-space:nowrap\">{txt}</td>\
                 <td style=\"padding:6px 8px;border-bottom:1px solid #eaecf0\"><b>{}</b> {}<div style=\"color:#667085;font-size:12px\">{}</div></td>\
                 <td style=\"padding:6px 8px;border-bottom:1px solid #eaecf0;white-space:nowrap\">{}</td></tr>",
            esc(&s(it, "ref")), esc(&s(it, "title")), esc(&s(it, "detail")), esc(&s(it, "due")))
    }).collect();
    format!("<div style=\"font-family:Tahoma,sans-serif;font-size:14px;color:#101828\">\
        <p>เรียน คุณ{}</p><p>งานในระบบควบคุมเอกสาร ISO 9001 ที่รอคุณดำเนินการ {} รายการ:</p>\
        <table style=\"border-collapse:collapse;width:100%;max-width:760px\"><thead><tr style=\"background:#f2f4f7\">\
        <th style=\"text-align:left;padding:6px 8px\">สถานะ</th><th style=\"text-align:left;padding:6px 8px\">งาน</th><th style=\"text-align:left;padding:6px 8px\">กำหนด</th></tr></thead>\
        <tbody>{rows}</tbody></table>\
        <p><a href=\"{url}\" style=\"color:#1570ef\">เปิดระบบ ISO</a></p>\
        <p style=\"color:#667085;font-size:12px\">อีเมลอัตโนมัติ — ปิดการรับได้ที่ไอคอนกระดิ่ง → ตั้งค่าอีเมล</p></div>",
        esc(name), list.len(), url = app_url())
}

/// ส่งสรุปงานรายวันให้ทุกคนที่เปิดรับ (วันละครั้ง — กันส่งซ้ำด้วย notify_sent)
pub async fn send_digests(st: &S) {
    if smtp_config().is_none() { return; }
    let today = capa::today();
    let Ok(c) = st.db.get().await else { return };
    let Ok(rows) = c.query(
        "SELECT emp_code,name_th,position,dept,role,email FROM employees WHERE status='active' AND notify_email AND email<>'' AND notify_sent<>$1", &[&today],
    ).await else { return };
    let (mut sent, mut failed) = (0, 0);
    for r in rows {
        let u = AuthUser { emp_code: r.get(0), name: r.get(1), position: r.get(2), dept: r.get(3), role: r.get(4), must_change_pin: false };
        let email: String = r.get(5);
        let list = items(st, &u).await;
        if !list.is_empty() {
            let overdue = list.iter().filter(|x| x["level"] == json!("overdue")).count();
            let subject = if overdue > 0 { format!("[ISO] งานรอดำเนินการ {} รายการ (เลยกำหนด {overdue})", list.len()) } else { format!("[ISO] งานรอดำเนินการ {} รายการ", list.len()) };
            match send_mail(&email, &subject, digest_html(&u.name, &list)).await {
                Ok(()) => sent += 1,
                Err(e) => { failed += 1; eprintln!("  ⚠ อีเมลสรุปงานถึง {} ไม่สำเร็จ: {e}", u.emp_code); continue; }
            }
        }
        let _ = c.execute("UPDATE employees SET notify_sent=$2 WHERE emp_code=$1", &[&u.emp_code, &today]).await;
    }
    if sent + failed > 0 { println!("  ✉ ส่งสรุปงานรายวัน {sent} ฉบับ · ไม่สำเร็จ {failed}"); }
}

/// วนส่งสรุปงานทุกวันเวลา DIGEST_TIME (ค่าเริ่มต้น 07:30 เวลาไทย)
pub async fn digest_loop(st: S) {
    if smtp_config().is_none() { return; }
    let (h, m) = std::env::var("DIGEST_TIME").ok().and_then(|t| { let (h, m) = t.split_once(':')?; Some((h.parse::<u32>().ok()?, m.parse::<u32>().ok()?)) })
        .filter(|(h, m)| *h < 24 && *m < 60).unwrap_or((7, 30));
    let tz = chrono::FixedOffset::east_opt(7 * 3600).unwrap();
    loop {
        let now = chrono::Utc::now().with_timezone(&tz);
        let mut next = now.date_naive().and_hms_opt(h, m, 0).unwrap();
        if now.naive_local() >= next { next += chrono::Duration::days(1); }
        let wait = (next - now.naive_local()).to_std().unwrap_or(std::time::Duration::from_secs(60));
        tokio::time::sleep(wait).await;
        send_digests(&st).await;
    }
}

// ---------- handlers ----------

pub async fn list(State(st): State<S>, Extension(u): Extension<AuthUser>) -> Response {
    let list = items(&st, &u).await;
    let (email, on) = match st.db.get().await {
        Ok(c) => c.query_opt("SELECT email,notify_email FROM employees WHERE emp_code=$1", &[&u.emp_code]).await.ok().flatten()
            .map(|r| (r.get::<_, String>(0), r.get::<_, bool>(1))).unwrap_or_default(),
        Err(_) => (String::new(), false),
    };
    let count = |l: &str| list.iter().filter(|x| x["level"] == json!(l)).count();
    Json(json!({
        "items": list, "overdue": count("overdue"), "soon": count("soon"),
        "email": email, "emailOn": on, "smtp": smtp_config().is_some(),
    })).into_response()
}

#[derive(Deserialize)]
#[allow(non_snake_case)]
pub struct SettingsReq { email: String, emailOn: bool }

fn valid_email(e: &str) -> bool {
    let Some((local, domain)) = e.split_once('@') else { return false };
    !local.is_empty() && domain.contains('.') && !domain.starts_with('.') && !domain.ends_with('.') && e.len() <= 200 && !e.chars().any(|c| c.is_whitespace() || c == ',' || c == '<' || c == '>')
}

/// ผู้ใช้ตั้งอีเมลของตนเองและเลือกรับ/ไม่รับสรุปงานรายวัน
pub async fn settings(State(st): State<S>, Extension(u): Extension<AuthUser>, Json(b): Json<SettingsReq>) -> Response {
    let email = b.email.trim().to_lowercase();
    if !email.is_empty() && !valid_email(&email) { return err(StatusCode::BAD_REQUEST, "รูปแบบอีเมลไม่ถูกต้อง"); }
    if b.emailOn && email.is_empty() { return err(StatusCode::BAD_REQUEST, "กรุณาระบุอีเมลก่อนเปิดรับสรุปงาน"); }
    let Ok(c) = st.db.get().await else { return err(StatusCode::INTERNAL_SERVER_ERROR, "เชื่อมต่อฐานข้อมูลไม่ได้") };
    if let Err(e) = c.execute("UPDATE employees SET email=$2,notify_email=$3 WHERE emp_code=$1", &[&u.emp_code, &email, &b.emailOn]).await {
        return err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string());
    }
    let _ = eform::audit(&c, &u, "notify_settings", "employee", &u.emp_code, &format!("{} {}", email, if b.emailOn { "รับสรุปรายวัน" } else { "ไม่รับ" })).await;
    Json(json!({ "ok": true, "email": email, "emailOn": b.emailOn })).into_response()
}

/// ส่งอีเมลทดสอบ (สรุปงานปัจจุบัน) ไปที่อีเมลของผู้ใช้เอง
pub async fn test(State(st): State<S>, Extension(u): Extension<AuthUser>) -> Response {
    let Ok(c) = st.db.get().await else { return err(StatusCode::INTERNAL_SERVER_ERROR, "เชื่อมต่อฐานข้อมูลไม่ได้") };
    let email: String = c.query_opt("SELECT email FROM employees WHERE emp_code=$1", &[&u.emp_code]).await.ok().flatten().map(|r| r.get(0)).unwrap_or_default();
    if email.is_empty() { return err(StatusCode::BAD_REQUEST, "ยังไม่ได้บันทึกอีเมล"); }
    let list = items(&st, &u).await;
    match send_mail(&email, "[ISO] ทดสอบการแจ้งเตือนทางอีเมล", digest_html(&u.name, &list)).await {
        Ok(()) => Json(json!({ "ok": true, "to": email })).into_response(),
        Err(e) => err(StatusCode::BAD_GATEWAY, &e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn urgency_levels() {
        assert_eq!(urgency("", "2026-10-16"), "todo");
        assert_eq!(urgency("2026-10-15", "2026-10-16"), "overdue");
        assert_eq!(urgency("2026-10-16", "2026-10-16"), "soon");
        assert_eq!(urgency("2026-10-23", "2026-10-16"), "soon");
        assert_eq!(urgency("2026-10-24", "2026-10-16"), "todo");
    }
    #[test]
    fn emails() {
        assert!(valid_email("qmr@kgarden.co.th"));
        assert!(!valid_email("qmr@kgarden"));
        assert!(!valid_email("a b@x.co"));
        assert!(!valid_email("@x.co"));
        assert!(!valid_email("a@x.co,b@y.co"));
    }
    #[test]
    fn digest_escapes() {
        let h = digest_html("<x>", &[item("dcr", "dcr", "DCR-1", "<script>", String::new(), "", "todo")]);
        assert!(!h.contains("<script>") && h.contains("&lt;script&gt;"));
    }
}
