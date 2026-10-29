// ผู้ช่วย AI (OpenAI) — เสนอแนะเท่านั้น ผู้ใช้เลือกเองว่าจะ "ทำตาม" หรือ "ไม่ทำตาม" ทีละข้อ
//   ตรวจเอกสาร: อ่านเนื้อหาไฟล์ + ข้อมูลทะเบียน (Rev./Rev. ถัดไป, ผลตรวจ Rev. ในไฟล์, เอกสารที่อ้างถึงและสถานะ, ข้อกำหนด ISO ที่เอกสารเป็นหลักฐาน)
//     → ข้อแนะนำ (ประเด็น + วิธีแก้ + ข้อความเดิม/ข้อความใหม่ ถ้าแก้ด้วยการแทนข้อความได้)
//     → ข้อที่ "ทำตาม" และมีข้อความเดิมตรงในไฟล์ นำไปแก้ไฟล์เป็นฉบับร่าง แล้วส่งเป็น DCR ตามปกติ (ลงนาม PIN → ตรวจสอบ → อนุมัติ)
//   ช่วยผู้ตรวจ DCR: เทียบฉบับปัจจุบันกับฉบับร่าง → สรุปสิ่งที่เปลี่ยน + ประเด็นที่ควรตรวจ (ข้อที่ทำตามใส่ในความเห็นผู้ตรวจได้)
//   ตั้งค่าด้วย env: OPENAI_API_KEY (ไม่ตั้ง = ปิดฟีเจอร์), OPENAI_MODEL, OPENAI_BASE_URL — คีย์อยู่ฝั่งเซิร์ฟเวอร์เท่านั้น ไม่ส่งถึงเบราว์เซอร์
//   ผลของ AI เป็นข้อมูลที่ไม่น่าเชื่อถือ: แสดงเป็นข้อความธรรมดา และแก้ไฟล์ได้เฉพาะเมื่อข้อความเดิมตรงทุกตัวอักษร
use crate::eform::{self, AuthUser};
use crate::{doccontrol, err, now_iso, revcontrol, revscan, sha256_hex, S};
use axum::{
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{collections::HashMap, io::{Cursor, Read, Write}, time::Duration};

pub const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS ai_reviews(
  id BIGSERIAL PRIMARY KEY,
  kind TEXT NOT NULL,
  subject TEXT NOT NULL,
  sha256 TEXT NOT NULL,
  model TEXT NOT NULL,
  summary TEXT NOT NULL DEFAULT '',
  tokens BIGINT NOT NULL DEFAULT 0,
  created_by TEXT NOT NULL,
  created_name TEXT NOT NULL,
  created_at TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS idx_air_subject ON ai_reviews(kind, subject);
CREATE TABLE IF NOT EXISTS ai_suggestions(
  id BIGSERIAL PRIMARY KEY,
  review_id BIGINT NOT NULL,
  seq INT NOT NULL,
  category TEXT NOT NULL,
  severity TEXT NOT NULL,
  location TEXT NOT NULL DEFAULT '',
  issue TEXT NOT NULL,
  suggestion TEXT NOT NULL,
  find TEXT NOT NULL DEFAULT '',
  replace TEXT NOT NULL DEFAULT '',
  status TEXT NOT NULL DEFAULT 'open',
  decided_by TEXT NOT NULL DEFAULT '',
  decided_name TEXT NOT NULL DEFAULT '',
  decided_at TEXT NOT NULL DEFAULT '');
CREATE INDEX IF NOT EXISTS idx_ais_review ON ai_suggestions(review_id);
";

/// เรียก AI ได้กี่ครั้งต่อคนต่อชั่วโมง (คุมค่าใช้จ่าย)
const PER_HOUR: i64 = 30;
const MAX_SUGGESTIONS: usize = 15;
const DEFAULT_MODEL: &str = "gpt-4.1-mini";

struct Cfg { key: String, model: String, base: String }

fn cfg() -> Option<Cfg> {
    let key = std::env::var("OPENAI_API_KEY").ok().map(|k| k.trim().to_string()).filter(|k| !k.is_empty())?;
    let model = std::env::var("OPENAI_MODEL").ok().map(|m| m.trim().to_string()).filter(|m| !m.is_empty()).unwrap_or_else(|| DEFAULT_MODEL.into());
    let base = std::env::var("OPENAI_BASE_URL").ok().map(|b| b.trim().trim_end_matches('/').to_string()).filter(|b| !b.is_empty())
        .unwrap_or_else(|| "https://api.openai.com/v1".into());
    Some(Cfg { key, model, base })
}

fn db_err() -> Response { err(StatusCode::INTERNAL_SERVER_ERROR, "เชื่อมต่อฐานข้อมูลไม่ได้") }
fn s(v: &Value, k: &str) -> String { v[k].as_str().unwrap_or("").to_string() }

/// โครงสร้างคำตอบที่บังคับให้ AI ตอบ (Structured Outputs)
fn response_schema() -> Value {
    json!({
        "type": "object", "additionalProperties": false, "required": ["summary", "suggestions"],
        "properties": {
            "summary": { "type": "string" },
            "suggestions": { "type": "array", "items": {
                "type": "object", "additionalProperties": false,
                "required": ["category", "severity", "location", "issue", "suggestion", "find", "replace"],
                "properties": {
                    "category": { "type": "string", "enum": ["rev", "reference", "iso", "consistency", "clarity", "missing", "format"] },
                    "severity": { "type": "string", "enum": ["high", "medium", "low"] },
                    "location": { "type": "string" },
                    "issue": { "type": "string" },
                    "suggestion": { "type": "string" },
                    "find": { "type": "string" },
                    "replace": { "type": "string" }
                }
            } }
        }
    })
}

/// เรียก Chat Completions พร้อม JSON schema — คืน (คำตอบ, จำนวน token)
async fn call_model(c: &Cfg, system: &str, user: &str) -> Result<(Value, i64), String> {
    let body = json!({
        "model": c.model,
        "messages": [{ "role": "system", "content": system }, { "role": "user", "content": user }],
        "response_format": { "type": "json_schema", "json_schema": { "name": "iso_assist", "strict": true, "schema": response_schema() } },
    });
    let client = reqwest::Client::builder().timeout(Duration::from_secs(150)).build().map_err(|e| e.to_string())?;
    let res = client.post(format!("{}/chat/completions", c.base))
        .header("Authorization", format!("Bearer {}", c.key))
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .send().await.map_err(|e| format!("เชื่อมต่อ OpenAI ไม่ได้: {e}"))?;
    let status = res.status();
    let text = res.text().await.map_err(|e| e.to_string())?;
    let v: Value = serde_json::from_str(&text).unwrap_or(json!({}));
    if !status.is_success() {
        let msg = v["error"]["message"].as_str().unwrap_or("").chars().take(200).collect::<String>();
        return Err(match status.as_u16() {
            401 => "OpenAI ปฏิเสธ API key — ตรวจค่า OPENAI_API_KEY".into(),
            429 => format!("OpenAI จำกัดการใช้งาน/เครดิตหมด — ลองใหม่ภายหลัง ({msg})"),
            404 => format!("ไม่พบโมเดล {} — ตั้ง OPENAI_MODEL ให้ถูกต้อง ({msg})", c.model),
            _ => format!("OpenAI ตอบกลับผิดพลาด ({status}): {msg}"),
        });
    }
    let msg = &v["choices"][0]["message"];
    if let Some(r) = msg["refusal"].as_str().filter(|r| !r.is_empty()) { return Err(format!("AI ปฏิเสธคำขอ: {r}")); }
    let content = msg["content"].as_str().ok_or("AI ไม่ได้ตอบกลับเนื้อหา")?;
    let out: Value = serde_json::from_str(content).map_err(|_| "AI ตอบกลับไม่ตรงรูปแบบ — ลองใหม่อีกครั้ง".to_string())?;
    Ok((out, v["usage"]["total_tokens"].as_i64().unwrap_or(0)))
}

const SYSTEM_DOC: &str = "คุณคือผู้ช่วยผู้ควบคุมเอกสาร (DCC) ของระบบบริหารคุณภาพ ISO 9001:2015 บริษัท เค การ์เด้น แอนด์ เฟนซ์ จำกัด (ผลิตรั้วตาข่าย แผงตะแกรงเหล็ก เสา)
หน้าที่: ตรวจเอกสารที่ให้มา แล้วเสนอแนะการแก้ไข — ผู้ใช้จะเลือกเองทีละข้อว่าจะทำตามหรือไม่
กติกา:
- เสนอเฉพาะประเด็นที่มีหลักฐานในเนื้อหาหรือข้อมูลทะเบียนที่ให้มา ห้ามแต่งข้อเท็จจริง ตัวเลข ชื่อคน หรือรหัสเอกสารขึ้นเอง
- เลขฉบับ: แบบฟอร์ม (FM-) ใช้ Rev. เช่น Rev.03 · เอกสารอื่น (QM/QP/WI) ใช้ v เช่น v4.0 (v1.0 = Rev.00 เดิม ลำดับเดียวกัน) — ไฟล์ QM/QP/WI ที่ยังเขียน Rev. ให้เสนอแก้เป็นเลขแบบ v
- ประเด็นที่ควรดู: เลขฉบับ/วันที่บังคับใช้ในไฟล์ไม่ตรงทะเบียน · อ้างถึงเอกสารที่ยกเลิกใช้หรือไม่มีในทะเบียน · ขาดสิ่งที่ข้อกำหนด ISO 9001 ที่เกี่ยวข้องต้องมี (ผู้รับผิดชอบ ความถี่ เกณฑ์ การเก็บบันทึก) · ข้อความขัดกันเอง · ภาษากำกวม เช่น \"ตามความเหมาะสม\" โดยไม่ระบุผู้รับผิดชอบ/เกณฑ์ · แบบฟอร์มที่ขาดช่องบันทึกหลักฐาน (ผู้บันทึก วันที่ ผลการตรวจ ผู้อนุมัติ)
- find/replace: ถ้าแก้ได้ด้วยการแทนข้อความ ให้ find คัดลอกข้อความเดิมจากส่วน [เนื้อหา] หรือ [หัว/ท้ายกระดาษ] ตรงทุกตัวอักษร (อยู่ในย่อหน้า/เซลล์เดียว สั้นที่สุดที่ยังชัดเจน) และ replace คือข้อความใหม่ทั้งช่วงนั้น · ถ้าต้องเพิ่มหัวข้อใหม่หรือแก้เองในโปรแกรม ให้ find และ replace เป็นสตริงว่าง
- ข้อแนะนำที่ผู้ใช้ทำตามจะถูกส่งเป็นคำขอเปลี่ยนแปลง (DCR) ออกฉบับถัดไปตามข้อมูลทะเบียน — ถ้าในไฟล์มีเลขฉบับของเอกสารนี้ ให้มีข้อเสนอแก้เป็นเลขฉบับถัดไปด้วย (category rev)
- location: บอกตำแหน่งให้หาเจอ เช่น \"ท้ายกระดาษ\", \"หัวข้อ 5.2\", \"ชีต FM-QC-003 แถวหัวตาราง\"
- เขียนภาษาไทย กระชับ ไม่เกิน 12 ข้อ เรียงจากสำคัญมากไปน้อย · summary สรุปภาพรวม 1–3 ประโยค · ถ้าเอกสารดีอยู่แล้วให้ suggestions น้อยหรือว่างได้";

const SYSTEM_DCR: &str = "คุณคือผู้ช่วยผู้อนุมัติคำขอเปลี่ยนแปลงเอกสาร (DCR) ระบบ ISO 9001:2015 บริษัท เค การ์เด้น แอนด์ เฟนซ์ จำกัด
หน้าที่: เทียบ [ฉบับใช้งานปัจจุบัน] กับ [ฉบับร่าง] แล้วช่วยผู้อนุมัติตัดสินใจ — ผู้อนุมัติจะเลือกเองว่าจะทำตามข้อแนะนำหรือไม่
กติกา:
- summary: สรุปสิ่งที่เปลี่ยนจริงในฉบับร่าง เป็นข้อสั้น ๆ คั่นด้วยขึ้นบรรทัด (ถ้าแทบไม่เปลี่ยนให้บอกตรง ๆ)
- suggestions: ประเด็นที่ผู้อนุมัติควรตรวจหรือขอให้ผู้จัดทำแก้ เช่น เลขฉบับ/วันที่ในไฟล์ยังไม่เป็นฉบับใหม่ (QM/QP/WI ใช้ v เช่น v5.0 · แบบฟอร์ม FM ใช้ Rev.) · สิ่งที่เปลี่ยนไม่ตรงกับเหตุผลที่ขอ หรือเปลี่ยนเนื้อหาสำคัญโดยไม่ระบุ · อ้างถึงเอกสารที่ยกเลิกใช้ · กระทบเอกสาร/แบบฟอร์มอื่นที่ต้องแก้ตาม · ภาษากำกวม
- find/replace อ้างอิงข้อความใน [ฉบับร่าง] ตรงทุกตัวอักษร หรือเป็นสตริงว่างถ้าไม่ใช่การแทนข้อความ
- ห้ามแต่งข้อเท็จจริง · ภาษาไทย กระชับ ไม่เกิน 10 ข้อ เรียงจากสำคัญมากไปน้อย";

// ---------- ข้อมูลประกอบ ----------

/// ข้อมูลทะเบียนของเอกสาร + เอกสารที่อ้างถึง + ข้อกำหนด ISO + ผลตรวจ Rev. ในไฟล์
async fn register_context(st: &S, code: &str, text: &str) -> String {
    let ctl = doccontrol::control_map(st).await;
    let info = |c: &str| -> Option<Value> { st.by_code.get(c).map(|d| { let mut o = d.clone(); ctl.apply(c, &mut o); o }) };
    let mut out = String::new();
    if let Some(d) = info(code) {
        let rev = s(&d, "rev");
        out.push_str(&format!("เอกสาร: {code} {}\nประเภท: {} · หน่วยงาน: {}\nเลขฉบับปัจจุบันในทะเบียน: {rev} · เลขฉบับถัดไปถ้าแก้ไข: {} · วันที่บังคับใช้ในทะเบียน: {}\nสถานะ: {}\n",
            s(&d, "name"), s(&d, "type"), s(&d, "deptName"), doccontrol::next_rev(code, &rev), s(&d, "eff"),
            if d["docStatus"] == json!("obsolete") { "ยกเลิกใช้" } else { "ใช้งาน" }));
    }
    let clauses: Vec<String> = st.clauses["clauses"].as_array().cloned().unwrap_or_default().iter()
        .filter(|c| c["docs"].as_array().map(|a| a.iter().any(|x| x.as_str() == Some(code))).unwrap_or(false))
        .map(|c| format!("{} {}", s(c, "clause"), s(c, "title"))).collect();
    if !clauses.is_empty() { out.push_str(&format!("เป็นหลักฐานของข้อกำหนด ISO 9001: {}\n", clauses.join(" · "))); }
    let refs: Vec<String> = revscan::find_codes(text).into_iter().filter(|r| r != code).take(40).map(|r| match info(&r) {
        Some(d) => format!("{r} {} ({}{})", s(&d, "name"), s(&d, "rev"), if d["docStatus"] == json!("obsolete") { ", ยกเลิกใช้แล้ว" } else { "" }),
        None => format!("{r} (ไม่มีในทะเบียน)"),
    }).collect();
    if !refs.is_empty() { out.push_str(&format!("เอกสารที่อ้างถึงในไฟล์: {}\n", refs.join(" · "))); }
    let rc = revcontrol::evaluate(st).await;
    let found: Vec<String> = rc["issues"].as_array().cloned().unwrap_or_default().iter()
        .filter(|i| i["docCode"].as_str() == Some(code)).map(|i| s(i, "detail")).collect();
    if !found.is_empty() { out.push_str(&format!("ผลตรวจอัตโนมัติของระบบ: {}\n", found.join(" · "))); }
    out
}

async fn file_text(path: std::path::PathBuf, ext: String, max: usize) -> Result<(String, String), String> {
    tokio::task::spawn_blocking(move || {
        let bytes = std::fs::read(&path).map_err(|e| format!("อ่านไฟล์ไม่ได้: {e}"))?;
        let text = revscan::doc_text(&bytes, &ext, max)?;
        Ok((text, sha256_hex(&bytes)))
    }).await.map_err(|_| "อ่านไฟล์ไม่สำเร็จ".to_string())?
}

// ---------- บันทึก/อ่านผลการตรวจ ----------

const SCOLS: &str = "id,seq,category,severity,location,issue,suggestion,find,replace,status,decided_by,decided_name,decided_at";

fn sugg_json(r: &tokio_postgres::Row) -> Value {
    let g = |i: usize| r.get::<_, String>(i);
    json!({
        "id": r.get::<_, i64>(0), "seq": r.get::<_, i32>(1), "category": g(2), "severity": g(3), "location": g(4),
        "issue": g(5), "suggestion": g(6), "find": g(7), "replace": g(8), "status": g(9),
        "decidedBy": g(10), "decidedName": g(11), "decidedAt": g(12),
    })
}

async fn review_json(c: &impl deadpool_postgres::GenericClient, id: i64, current_sha: &str) -> Option<Value> {
    let r = c.query_opt("SELECT id,kind,subject,sha256,model,summary,created_name,created_at FROM ai_reviews WHERE id=$1", &[&id]).await.ok()??;
    let sha: String = r.get(3);
    let suggestions: Vec<Value> = c.query(&format!("SELECT {SCOLS} FROM ai_suggestions WHERE review_id=$1 ORDER BY seq"), &[&id]).await
        .map(|rows| rows.iter().map(sugg_json).collect()).unwrap_or_default();
    Some(json!({
        "id": r.get::<_, i64>(0), "kind": r.get::<_, String>(1), "subject": r.get::<_, String>(2), "model": r.get::<_, String>(4),
        "summary": r.get::<_, String>(5), "createdName": r.get::<_, String>(6), "createdAt": r.get::<_, String>(7),
        "stale": !current_sha.is_empty() && sha != current_sha, "suggestions": suggestions,
    }))
}

async fn store_review(c: &impl deadpool_postgres::GenericClient, u: &AuthUser, kind: &str, subject: &str, sha: &str, model: &str, out: &Value, tokens: i64) -> Result<i64, String> {
    let summary = out["summary"].as_str().unwrap_or("").to_string();
    let id: i64 = c.query_one(
        "INSERT INTO ai_reviews(kind,subject,sha256,model,summary,tokens,created_by,created_name,created_at) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9) RETURNING id",
        &[&kind, &subject, &sha, &model, &summary, &tokens, &u.emp_code, &u.name, &now_iso()],
    ).await.map_err(|e| e.to_string())?.get(0);
    let pick = |v: &Value, k: &str, allowed: &[&str], dflt: &str| -> String {
        let x = v[k].as_str().unwrap_or("");
        if allowed.contains(&x) { x.to_string() } else { dflt.to_string() }
    };
    for (i, sg) in out["suggestions"].as_array().cloned().unwrap_or_default().iter().take(MAX_SUGGESTIONS).enumerate() {
        let cut = |k: &str, n: usize| sg[k].as_str().unwrap_or("").chars().take(n).collect::<String>();
        let category = pick(sg, "category", &["rev", "reference", "iso", "consistency", "clarity", "missing", "format"], "clarity");
        let severity = pick(sg, "severity", &["high", "medium", "low"], "medium");
        let _ = c.execute(
            "INSERT INTO ai_suggestions(review_id,seq,category,severity,location,issue,suggestion,find,replace) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)",
            &[&id, &(i as i32), &category, &severity, &cut("location", 200), &cut("issue", 1000), &cut("suggestion", 1500), &cut("find", 1000), &cut("replace", 1500)],
        ).await;
    }
    Ok(id)
}

/// ตรวจสิทธิ์ + โควตาก่อนเรียก AI
async fn precheck(c: &impl deadpool_postgres::GenericClient, u: &AuthUser) -> Result<Cfg, Response> {
    let Some(cfg) = cfg() else { return Err(err(StatusCode::SERVICE_UNAVAILABLE, "ยังไม่ได้ตั้งค่า OPENAI_API_KEY บนเซิร์ฟเวอร์")) };
    if u.role == "viewer" { return Err(err(StatusCode::FORBIDDEN, "บัญชีอ่านอย่างเดียวใช้ผู้ช่วย AI ไม่ได้")); }
    let since = (chrono::Utc::now() - chrono::Duration::hours(1)).to_rfc3339();
    let n: i64 = c.query_one("SELECT COUNT(*) FROM ai_reviews WHERE created_by=$1 AND created_at>$2", &[&u.emp_code, &since]).await.map(|r| r.get(0)).unwrap_or(0);
    if n >= PER_HOUR { return Err(err(StatusCode::TOO_MANY_REQUESTS, &format!("ใช้ผู้ช่วย AI ครบ {PER_HOUR} ครั้งในชั่วโมงนี้แล้ว — ลองใหม่ภายหลัง"))); }
    Ok(cfg)
}

/// SHA-256 ของไฟล์ฉบับใช้งานปัจจุบัน (ใช้บอกว่าผลตรวจเก่ากว่าเอกสารหรือไม่)
async fn current_doc_sha(st: &S, code: &str) -> String {
    let Ok((_, abs, _, _)) = crate::resolve(st, code, None).await else { return String::new() };
    tokio::task::spawn_blocking(move || std::fs::read(abs).map(|b| sha256_hex(&b)).unwrap_or_default()).await.unwrap_or_default()
}

// ---------- handlers ----------

/// GET /api/ai/status — เปิดใช้หรือไม่ (ไม่ส่งคีย์)
pub async fn status() -> Response {
    match cfg() {
        Some(c) => Json(json!({ "enabled": true, "model": c.model })).into_response(),
        None => Json(json!({ "enabled": false })).into_response(),
    }
}

/// GET /api/ai/reviews?kind=doc|dcr&subject= — ผลตรวจล่าสุดของเอกสาร/คำขอ
pub async fn latest(Query(q): Query<HashMap<String, String>>, State(st): State<S>) -> Response {
    let kind = q.get("kind").cloned().unwrap_or_default();
    let subject = q.get("subject").cloned().unwrap_or_default();
    let Ok(c) = st.db.get().await else { return db_err() };
    let Ok(Some(r)) = c.query_opt("SELECT id FROM ai_reviews WHERE kind=$1 AND subject=$2 ORDER BY id DESC LIMIT 1", &[&kind, &subject]).await else {
        return Json(Value::Null).into_response();
    };
    let sha = match kind.as_str() {
        "doc" => current_doc_sha(&st, &subject).await,
        _ => c.query_opt("SELECT sha256 FROM doc_changes WHERE id=$1", &[&subject.parse::<i64>().unwrap_or(0)]).await.ok().flatten().map(|r| r.get(0)).unwrap_or_default(),
    };
    Json(review_json(&c, r.get(0), &sha).await.unwrap_or(Value::Null)).into_response()
}

/// POST /api/ai/docs/:code/review — ให้ AI ตรวจเอกสารฉบับใช้งานปัจจุบัน
pub async fn review_doc(Path(code): Path<String>, State(st): State<S>, Extension(u): Extension<AuthUser>) -> Response {
    if !st.by_code.contains_key(&code) { return err(StatusCode::NOT_FOUND, "ไม่พบเอกสาร"); }
    let Ok(c) = st.db.get().await else { return db_err() };
    let cfg = match precheck(&c, &u).await { Ok(x) => x, Err(r) => return r };
    let (_, abs, ext, _) = match crate::resolve(&st, &code, None).await { Ok(x) => x, Err(r) => return r };
    let (text, sha) = match file_text(abs, ext, 60_000).await { Ok(x) => x, Err(e) => return err(StatusCode::BAD_REQUEST, &e) };
    let user = format!("ข้อมูลทะเบียน:\n{}\nเนื้อหาไฟล์:\n{text}", register_context(&st, &code, &text).await);
    let (out, tokens) = match call_model(&cfg, SYSTEM_DOC, &user).await { Ok(x) => x, Err(e) => return err(StatusCode::BAD_GATEWAY, &e) };
    let id = match store_review(&c, &u, "doc", &code, &sha, &cfg.model, &out, tokens).await { Ok(i) => i, Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, &e) };
    let _ = eform::audit(&c, &u, "ai_review", "document", &code, &format!("{} · {tokens} tokens", cfg.model)).await;
    Json(review_json(&c, id, &sha).await.unwrap_or(Value::Null)).into_response()
}

/// POST /api/ai/changes/:id/review — ช่วยผู้ตรวจ: เทียบฉบับปัจจุบันกับฉบับร่างของ DCR
pub async fn review_change(Path(id): Path<i64>, State(st): State<S>, Extension(u): Extension<AuthUser>) -> Response {
    let Ok(c) = st.db.get().await else { return db_err() };
    let cfg = match precheck(&c, &u).await { Ok(x) => x, Err(r) => return r };
    let Ok(Some(r)) = c.query_opt(
        "SELECT ref_no,doc_code,status,reason,note,from_rev,new_rev,ext,stored_name,base_version,prepared_name,sha256 FROM doc_changes WHERE id=$1", &[&id],
    ).await else { return err(StatusCode::NOT_FOUND, "ไม่พบคำขอเปลี่ยนแปลง") };
    let g = |i: usize| r.get::<_, String>(i);
    let (ref_no, code, ext, sha) = (g(0), g(1), g(7), g(11));
    let base_version: i64 = r.get(9);
    let draft = st.storage.join("changes").join(g(8));
    let (draft_text, _) = match file_text(draft, ext.clone(), 25_000).await { Ok(x) => x, Err(e) => return err(StatusCode::BAD_REQUEST, &e) };
    let base_text = match crate::resolve(&st, &code, Some(&base_version.to_string())).await {
        Ok((_, abs, bext, _)) => file_text(abs, bext, 25_000).await.map(|x| x.0).unwrap_or_else(|e| format!("(อ่านฉบับปัจจุบันไม่ได้: {e})")),
        Err(_) => "(ไม่พบฉบับปัจจุบัน)".into(),
    };
    let user = format!(
        "ข้อมูลทะเบียน:\n{}\nคำขอ {ref_no}: {} → {} · ผู้จัดทำ {} · สถานะ {}\nเหตุผลที่ขอ: {}\nบันทึกการแก้ไข: {}\n\n[ฉบับใช้งานปัจจุบัน]\n{base_text}\n\n[ฉบับร่าง]\n{draft_text}",
        register_context(&st, &code, &draft_text).await, g(5), g(6), g(10), g(2), g(3), g(4));
    let (out, tokens) = match call_model(&cfg, SYSTEM_DCR, &user).await { Ok(x) => x, Err(e) => return err(StatusCode::BAD_GATEWAY, &e) };
    let rid = match store_review(&c, &u, "dcr", &id.to_string(), &sha, &cfg.model, &out, tokens).await { Ok(i) => i, Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, &e) };
    let _ = eform::audit(&c, &u, "ai_review", "doc_change", &id.to_string(), &format!("{ref_no} · {} · {tokens} tokens", cfg.model)).await;
    Json(review_json(&c, rid, &sha).await.unwrap_or(Value::Null)).into_response()
}

#[derive(Deserialize)]
pub struct DecideReq { decision: String }

/// POST /api/ai/suggestions/:id/decide — ทำตาม / ไม่ทำตาม / ยังไม่ตัดสินใจ
pub async fn decide(Path(id): Path<i64>, State(st): State<S>, Extension(u): Extension<AuthUser>, Json(b): Json<DecideReq>) -> Response {
    if !matches!(b.decision.as_str(), "accepted" | "rejected" | "open") { return err(StatusCode::BAD_REQUEST, "การตัดสินใจไม่ถูกต้อง"); }
    if u.role == "viewer" { return err(StatusCode::FORBIDDEN, "บัญชีอ่านอย่างเดียวตัดสินใจไม่ได้"); }
    let Ok(c) = st.db.get().await else { return db_err() };
    // ข้อแนะนำของ DCR ตัดสินได้เฉพาะผู้ที่ลงนามคำขอนั้นได้ (ผู้อนุมัติขึ้นไป) หรือผู้จัดทำ — กันคนอื่นมาเปลี่ยนการตัดสินใจของผู้ตรวจ
    let Ok(Some(r)) = c.query_opt(
        "SELECT r.kind, COALESCE((SELECT d.prepared_by FROM doc_changes d WHERE r.kind='dcr' AND d.id::text=r.subject), '')
         FROM ai_suggestions s JOIN ai_reviews r ON r.id=s.review_id WHERE s.id=$1", &[&id],
    ).await else { return err(StatusCode::NOT_FOUND, "ไม่พบข้อแนะนำ") };
    let (kind, preparer): (String, String) = (r.get(0), r.get(1));
    if kind == "dcr" && !eform::can_approve(&u.role) && preparer != u.emp_code {
        return err(StatusCode::FORBIDDEN, "ตัดสินข้อแนะนำของคำขอนี้ได้เฉพาะผู้อนุมัติหรือผู้จัดทำคำขอ");
    }
    let (by, name, at) = if b.decision == "open" { (String::new(), String::new(), String::new()) } else { (u.emp_code.clone(), u.name.clone(), now_iso()) };
    match c.query_opt(
        &format!("UPDATE ai_suggestions SET status=$2,decided_by=$3,decided_name=$4,decided_at=$5 WHERE id=$1 RETURNING {SCOLS}"),
        &[&id, &b.decision, &by, &name, &at],
    ).await {
        Ok(Some(r)) => {
            let _ = eform::audit(&c, &u, "ai_decide", "ai_suggestion", &id.to_string(), &b.decision).await;
            Json(sugg_json(&r)).into_response()
        }
        _ => err(StatusCode::NOT_FOUND, "ไม่พบข้อแนะนำ"),
    }
}

#[derive(Deserialize)]
pub struct ApplyReq { ids: Vec<i64> }

/// POST /api/ai/reviews/:id/apply — นำข้อแนะนำที่ "ทำตาม" ไปแก้ไฟล์ฉบับใช้งาน → คืนไฟล์ฉบับร่างให้ส่งเป็น DCR (ยังไม่บันทึกอะไรในทะเบียน)
pub async fn apply(Path(rid): Path<i64>, State(st): State<S>, Extension(u): Extension<AuthUser>, Json(b): Json<ApplyReq>) -> Response {
    if u.role == "viewer" { return err(StatusCode::FORBIDDEN, "บัญชีอ่านอย่างเดียวแก้เอกสารไม่ได้"); }
    let Ok(c) = st.db.get().await else { return db_err() };
    let Ok(Some(r)) = c.query_opt("SELECT kind,subject,sha256 FROM ai_reviews WHERE id=$1", &[&rid]).await else { return err(StatusCode::NOT_FOUND, "ไม่พบผลตรวจ") };
    let (kind, code, sha): (String, String, String) = (r.get(0), r.get(1), r.get(2));
    if kind != "doc" { return err(StatusCode::BAD_REQUEST, "นำไปแก้ไฟล์ได้เฉพาะผลตรวจเอกสาร"); }
    if doccontrol::is_obsolete(&c, &code).await { return err(StatusCode::CONFLICT, "เอกสารนี้ยกเลิกใช้แล้ว"); }
    if let Ok(Some(o)) = c.query_opt("SELECT ref_no FROM doc_changes WHERE doc_code=$1 AND status IN ('review','approve','returned')", &[&code]).await {
        return err(StatusCode::CONFLICT, &format!("มีคำขอ {} ที่ยังไม่จบ — ดำเนินการให้เสร็จก่อน", o.get::<_, String>(0)));
    }
    let (_, abs, ext, filename) = match crate::resolve(&st, &code, None).await { Ok(x) => x, Err(r) => return r };
    if !matches!(ext.as_str(), "docx" | "xlsx") { return err(StatusCode::BAD_REQUEST, "แก้อัตโนมัติได้เฉพาะ .docx/.xlsx — ไฟล์ชนิดนี้ให้แก้ตามคำแนะนำในตัวแก้ไขเอกสาร"); }
    let Ok(bytes) = std::fs::read(&abs) else { return err(StatusCode::NOT_FOUND, "อ่านไฟล์ไม่ได้") };
    if sha256_hex(&bytes) != sha { return err(StatusCode::CONFLICT, "เอกสารมีเวอร์ชันใหม่หลังจาก AI ตรวจ — กด “ตรวจใหม่” ก่อน"); }
    let rows = c.query(&format!("SELECT {SCOLS} FROM ai_suggestions WHERE review_id=$1 AND id = ANY($2) AND status='accepted' ORDER BY seq"), &[&rid, &b.ids]).await.unwrap_or_default();
    let picked: Vec<Value> = rows.iter().map(sugg_json).filter(|s| !s["find"].as_str().unwrap_or("").is_empty()).collect();
    if picked.is_empty() { return err(StatusCode::BAD_REQUEST, "ยังไม่มีข้อแนะนำที่ทำตามและแก้ในไฟล์ได้"); }
    let pairs: Vec<(String, String)> = picked.iter().map(|s| (s["find"].as_str().unwrap_or("").to_string(), s["replace"].as_str().unwrap_or("").to_string())).collect();
    let ext2 = ext.clone();
    let (out, counts) = match tokio::task::spawn_blocking(move || apply_replacements(&bytes, &ext2, &pairs)).await {
        Ok(Ok(x)) => x,
        Ok(Err(e)) => return err(StatusCode::INTERNAL_SERVER_ERROR, &e),
        Err(_) => return err(StatusCode::INTERNAL_SERVER_ERROR, "แก้ไฟล์ไม่สำเร็จ"),
    };
    let results: Vec<Value> = picked.iter().zip(&counts).map(|(s, n)| json!({ "id": s["id"], "count": n, "issue": s["issue"] })).collect();
    let applied: Vec<&Value> = results.iter().filter(|r| r["count"].as_u64().unwrap_or(0) > 0).collect();
    if applied.is_empty() { return err(StatusCode::CONFLICT, "ไม่พบข้อความเดิมของข้อแนะนำที่เลือกในไฟล์ — แก้ตามคำแนะนำในตัวแก้ไขเอกสารแทน"); }
    let topics: Vec<String> = applied.iter().map(|r| s(r, "issue").chars().take(80).collect::<String>()).collect();
    let _ = eform::audit(&c, &u, "ai_apply", "document", &code, &format!("review {rid} · {} ข้อ", applied.len())).await;
    Json(json!({
        "dataBase64": B64.encode(&out), "ext": ext, "filename": filename, "results": results,
        "reason": format!("ปรับตามข้อแนะนำผู้ช่วย AI ที่เลือกทำตาม ({} ข้อ)", applied.len()),
        "note": topics.join(" · "),
    })).into_response()
}

// ---------- แก้ข้อความในไฟล์ Office (คงรูปแบบเดิม) ----------

/// ข้อความในแท็ก <w:t>/<t>: ช่วงไบต์ของแท็กเปิด/เนื้อหา และข้อความที่ถอด entity แล้ว
struct TNode { open: (usize, usize), content: (usize, usize), text: String }

/// แท็กข้อความ tname ทุกตัวตามลำดับ + ตำแหน่งจบย่อหน้า (para_close) ระหว่างนั้น — ใช้ต่อข้อความข้าม run ได้
fn text_nodes(xml: &str, tname: &str, para_close: &str) -> (Vec<TNode>, Vec<usize>) {
    let open_pat = format!("<{tname}");
    let close_pat = format!("</{tname}>");
    let (mut nodes, mut paras) = (vec![], vec![]);
    let mut pos = 0;
    while let Some(i) = xml[pos..].find(&open_pat).map(|x| pos + x) {
        let after = xml.as_bytes().get(i + open_pat.len()).copied().unwrap_or(b'>');
        let Some(gt) = xml[i..].find('>').map(|x| i + x) else { break };
        pos = gt + 1;
        if !matches!(after, b'>' | b' ' | b'/') || xml[i..gt].ends_with('/') { continue; }
        let Some(end) = xml[pos..].find(&close_pat).map(|x| pos + x) else { break };
        let mut text = String::new();
        revscan::push_unescaped(&mut text, &xml[pos..end]);
        nodes.push(TNode { open: (i, gt + 1), content: (pos, end), text });
        pos = end + close_pat.len();
    }
    let mut p = 0;
    while let Some(i) = xml[p..].find(para_close).map(|x| p + x) { paras.push(i); p = i + para_close.len(); }
    (nodes, paras)
}

fn esc(s: &str) -> String { s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;") }

/// แทนข้อความ find → rep ทุกตำแหน่ง (ข้าม run ในย่อหน้าเดียวกันได้ แต่ไม่ข้ามย่อหน้า) — คืน (xml ใหม่, จำนวนที่แทน)
/// ข้อความใหม่ใช้รูปแบบตัวอักษรของ run แรกในช่วงที่แทน
fn replace_in_xml(xml: &str, tname: &str, para_close: &str, find: &str, rep: &str) -> (String, usize) {
    if find.is_empty() || find.contains('\n') { return (xml.to_string(), 0); }
    let (nodes, paras) = text_nodes(xml, tname, para_close);
    // ข้อความต่อกันทั้งไฟล์ คั่นย่อหน้าด้วย \n
    let mut concat = String::new();
    let mut starts = Vec::with_capacity(nodes.len());
    let mut pi = 0;
    for n in &nodes {
        let mut sep = false;
        while pi < paras.len() && paras[pi] < n.open.0 { pi += 1; sep = true; }
        if sep && !concat.is_empty() { concat.push('\n'); }
        starts.push(concat.len());
        concat.push_str(&n.text);
    }
    let matches: Vec<usize> = concat.match_indices(find).map(|(i, _)| i).collect();
    if matches.is_empty() { return (xml.to_string(), 0); }
    let mut texts: Vec<String> = nodes.iter().map(|n| n.text.clone()).collect();
    for &m in matches.iter().rev() {
        let e = m + find.len();
        let covering: Vec<usize> = (0..nodes.len()).filter(|&k| starts[k] < e && starts[k] + nodes[k].text.len() > m).collect();
        let Some(&first) = covering.first() else { continue };
        let last = *covering.last().unwrap();
        let ls = m - starts[first];
        if first == last {
            let le = e - starts[first];
            texts[first] = format!("{}{}{}", &texts[first][..ls], rep, &texts[first][le..]);
        } else {
            let le = e - starts[last];
            texts[last] = texts[last][le..].to_string();
            for k in covering.iter().skip(1).take(covering.len().saturating_sub(2)) { texts[*k].clear(); }
            texts[first] = format!("{}{}", &texts[first][..ls], rep);
        }
    }
    let mut out = xml.to_string();
    for (k, n) in nodes.iter().enumerate().rev() {
        if texts[k] == n.text { continue; }
        out.replace_range(n.content.0..n.content.1, &esc(&texts[k]));
        let tag = &xml[n.open.0..n.open.1];
        if !tag.contains("xml:space") { out.replace_range(n.open.0..n.open.1, &format!("<{tname} xml:space=\"preserve\">")); }
    }
    (out, matches.len())
}

/// แก้ข้อความใน .docx (เนื้อหา หัว/ท้ายกระดาษ) หรือ .xlsx (sharedStrings + inline string) แล้วเขียน zip ใหม่ — ส่วนอื่นคัดลอกไบต์เดิม
fn apply_replacements(bytes: &[u8], ext: &str, pairs: &[(String, String)]) -> Result<(Vec<u8>, Vec<usize>), String> {
    let fixed = revscan::repair_eocd(bytes);
    let mut zin = zip::ZipArchive::new(Cursor::new(fixed.as_slice())).map_err(|e| format!("เปิดไฟล์ไม่ได้: {e}"))?;
    // ชีต Excel: inline string ในเซลล์ + หัว/ท้ายกระดาษ (แบบฟอร์มส่วนใหญ่ระบุ "FM-xx Rev.n วันที่มีผลบังคับใช้" ที่ท้ายกระดาษ)
    const SHEET_TAGS: [(&str, &str); 7] = [("t", "</c>"), ("oddFooter", "</headerFooter>"), ("evenFooter", "</headerFooter>"), ("firstFooter", "</headerFooter>"),
        ("oddHeader", "</headerFooter>"), ("evenHeader", "</headerFooter>"), ("firstHeader", "</headerFooter>")];
    let targets: Vec<(String, Vec<(&str, &str)>)> = zin.file_names().filter_map(|n| match ext {
        "docx" if n == "word/document.xml" || ((n.starts_with("word/header") || n.starts_with("word/footer")) && n.ends_with(".xml")) => Some((n.to_string(), vec![("w:t", "</w:p>")])),
        "xlsx" if n == "xl/sharedStrings.xml" => Some((n.to_string(), vec![("t", "</si>")])),
        "xlsx" if n.starts_with("xl/worksheets/sheet") && n.ends_with(".xml") => Some((n.to_string(), SHEET_TAGS.to_vec())),
        _ => None,
    }).collect();
    let mut counts = vec![0usize; pairs.len()];
    let mut changed: HashMap<String, String> = HashMap::new();
    for (name, tags) in &targets {
        let mut xml = String::new();
        zin.by_name(name).map_err(|e| e.to_string())?.read_to_string(&mut xml).map_err(|e| e.to_string())?;
        let mut cur = xml.clone();
        for (k, (find, rep)) in pairs.iter().enumerate() {
            for (tname, para) in tags {
                let (next, n) = replace_in_xml(&cur, tname, para, find, rep);
                counts[k] += n;
                cur = next;
            }
        }
        if cur != xml { changed.insert(name.clone(), cur); }
    }
    let mut zout = zip::ZipWriter::new(Cursor::new(Vec::with_capacity(bytes.len() + 4096)));
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for i in 0..zin.len() {
        let name = zin.by_index_raw(i).map_err(|e| e.to_string())?.name().to_string();
        if let Some(xml) = changed.get(&name) {
            zout.start_file(name.as_str(), opts).map_err(|e| e.to_string())?;
            zout.write_all(xml.as_bytes()).map_err(|e| e.to_string())?;
        } else {
            let f = zin.by_index_raw(i).map_err(|e| e.to_string())?;
            zout.raw_copy_file(f).map_err(|e| e.to_string())?;
        }
    }
    let out = zout.finish().map_err(|e| e.to_string())?.into_inner();
    Ok((out, counts))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_across_runs_keeping_other_text() {
        let xml = "<w:p><w:r><w:t>FM-QC-003 Rev.</w:t></w:r><w:r><w:t>0 วันที่</w:t></w:r></w:p><w:p><w:r><w:t>Rev.0</w:t></w:r></w:p>";
        let (out, n) = replace_in_xml(xml, "w:t", "</w:p>", "FM-QC-003 Rev.0", "FM-QC-003 Rev.1");
        assert_eq!(n, 1);
        assert!(out.contains(">FM-QC-003 Rev.1</w:t>") && out.contains("> วันที่</w:t>") && out.contains("<w:t>Rev.0</w:t>"));
    }

    #[test]
    fn does_not_match_across_paragraphs_or_tab_tags() {
        let xml = "<w:p><w:r><w:tab/><w:t>abc</w:t></w:r></w:p><w:p><w:r><w:t>def</w:t></w:r></w:p>";
        assert_eq!(replace_in_xml(xml, "w:t", "</w:p>", "abcdef", "x").1, 0);
        let (out, n) = replace_in_xml(xml, "w:t", "</w:p>", "c", "C & D");
        assert_eq!(n, 1);
        assert!(out.contains("abC &amp; D</w:t>") && out.contains("<w:tab/>"));
    }

    #[test]
    fn replaces_in_excel_footer_keeping_format_codes() {
        let xml = "<worksheet><headerFooter><oddFooter>&amp;L&amp;\"Angsana New,Regular\"&amp;14FM-MN-001 Rev.0 วันที่ผลบังคับใช้</oddFooter></headerFooter></worksheet>";
        let (out, n) = replace_in_xml(xml, "oddFooter", "</headerFooter>", "FM-MN-001 Rev.0", "FM-MN-001 Rev.1");
        assert_eq!(n, 1);
        assert!(out.contains("&amp;L&amp;\"Angsana New,Regular\"&amp;14FM-MN-001 Rev.1 วันที่"));
    }

    #[test]
    fn rewrites_docx_package() {
        let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let o = zip::write::SimpleFileOptions::default();
        z.start_file("word/document.xml", o).unwrap();
        z.write_all("<w:document><w:body><w:p><w:r><w:t>ทบทวนตามความเหมาะสม</w:t></w:r></w:p></w:body></w:document>".as_bytes()).unwrap();
        z.start_file("word/styles.xml", o).unwrap();
        z.write_all(b"<styles/>").unwrap();
        let src = z.finish().unwrap().into_inner();
        let (out, counts) = apply_replacements(&src, "docx", &[("ตามความเหมาะสม".into(), "ทุก 6 เดือน".into()), ("ไม่มี".into(), "x".into())]).unwrap();
        assert_eq!(counts, vec![1, 0]);
        let mut zr = zip::ZipArchive::new(Cursor::new(out)).unwrap();
        let mut s = String::new();
        zr.by_name("word/document.xml").unwrap().read_to_string(&mut s).unwrap();
        assert!(s.contains("ทบทวนทุก 6 เดือน"));
        let mut st = String::new();
        zr.by_name("word/styles.xml").unwrap().read_to_string(&mut st).unwrap();
        assert_eq!(st, "<styles/>");
    }
}
