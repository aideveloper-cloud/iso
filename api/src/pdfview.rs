// แสดงเอกสาร Word/Excel ให้เหมือนต้นฉบับ: แปลงเป็น PDF ด้วย LibreOffice (Gotenberg) แล้วเก็บไว้ใช้ซ้ำ
// แคชตาม SHA-256 ของไฟล์ (docs_root/pdfcache/<sha>.pdf) — แปลงครั้งเดียวต่อเนื้อหาไฟล์ ไม่ผูกกับชื่อ/เวอร์ชัน
// ตั้งที่อยู่ Gotenberg ด้วย env GOTENBERG_URL (ค่าเริ่มต้น http://127.0.0.1:3030)
// เอกสารในทะเบียนประทับตราสถานะ Rev. + QR ทุกหน้าตอนส่งออก (revcontrol::view_stamp) — แคชเก็บเฉพาะ PDF ที่ยังไม่ประทับ
use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::Response,
    Extension,
};
use std::{collections::HashMap, fs, path::PathBuf, time::Duration};

use sha2::{Digest, Sha256};

use crate::{acks, doccontrol, eform, err, pct, revcontrol, stamp, S};

fn sha_hex(b: &[u8]) -> String { Sha256::digest(b).iter().map(|x| format!("{x:02x}")).collect() }

/// ชนิดไฟล์ที่แปลงเป็น PDF ได้
pub fn convertible(ext: &str) -> bool { matches!(ext, "docx" | "doc" | "xlsx" | "xls" | "pptx" | "ppt") }

fn gotenberg_url() -> String {
    std::env::var("GOTENBERG_URL").unwrap_or_else(|_| "http://127.0.0.1:3030".into())
}

/// คืนไฟล์ PDF ของไฟล์ต้นฉบับ (แปลงใหม่ถ้ายังไม่มีในแคช)
pub(crate) async fn pdf_bytes(st: &S, abs: &PathBuf, ext: &str, title: &str) -> Result<Vec<u8>, String> {
    let src = fs::read(abs).map_err(|e| format!("อ่านไฟล์ไม่ได้: {e}"))?;
    let dir = st.docs_root.join("pdfcache");
    // Excel: หนึ่งชีตต่อหนึ่งหน้า (เห็นทั้งตารางเหมือนเปิดใน Excel) — ตามค่าหน้าพิมพ์เดิม ตารางที่กว้างกว่า A4 จะถูกตัดคอลัมน์ข้ามหน้า
    let sheet = matches!(ext, "xls" | "xlsx");
    let key = sha_hex(&[src.as_slice(), title.as_bytes()].concat());
    let cached = dir.join(format!("{key}{}.pdf", if sheet { "-sheet" } else { "" }));
    if let Ok(b) = fs::read(&cached) { return Ok(b); }

    let part = reqwest::multipart::Part::bytes(src).file_name(format!("document.{ext}"));
    let mut form = reqwest::multipart::Form::new().part("files", part);
    if sheet { form = form.text("singlePageSheets", "true"); }
    if !title.is_empty() { form = form.text("metadata", serde_json::json!({ "Title": title }).to_string()); }
    let client = reqwest::Client::builder().timeout(Duration::from_secs(120)).build().map_err(|e| e.to_string())?;
    let res = client
        .post(format!("{}/forms/libreoffice/convert", gotenberg_url()))
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("ตัวแปลง PDF ไม่ตอบสนอง: {e}"))?;
    if !res.status().is_success() {
        let code = res.status();
        let msg = res.text().await.unwrap_or_default();
        return Err(format!("แปลงเป็น PDF ไม่สำเร็จ ({code}): {}", msg.chars().take(200).collect::<String>()));
    }
    let pdf = res.bytes().await.map_err(|e| e.to_string())?.to_vec();
    let _ = fs::create_dir_all(&dir);
    let tmp = cached.with_extension("pdf.part");
    if fs::write(&tmp, &pdf).is_ok() { let _ = fs::rename(&tmp, &cached); }
    Ok(pdf)
}

pub(crate) fn pdf_response(pdf: Vec<u8>, name: &str) -> Response {
    let stem = name.rsplit_once('.').map(|(s, _)| s).unwrap_or(name);
    Response::builder()
        .header(header::CONTENT_TYPE, "application/pdf")
        .header(header::CONTENT_DISPOSITION, format!("inline; filename*=UTF-8''{}", pct(&format!("{stem}.pdf"))))
        .header(header::CACHE_CONTROL, "private, max-age=300")
        .body(Body::from(pdf))
        .unwrap()
}

/// GET /api/docs/:code/pdf?v= — เอกสารในทะเบียน (Word/Excel แปลงเป็น PDF · PDF เดิมส่งตามไฟล์) พร้อมตราประทับสถานะ Rev.
pub async fn doc_pdf(Path(code): Path<String>, Query(q): Query<HashMap<String, String>>, State(st): State<S>, Extension(u): Extension<eform::AuthUser>, headers: HeaderMap) -> Response {
    let (v, abs, ext, filename) = match crate::resolve(&st, &code, q.get("v")).await { Ok(x) => x, Err(r) => return r };
    if !convertible(&ext) && ext != "pdf" { return err(StatusCode::BAD_REQUEST, "ไฟล์ชนิดนี้ไม่ต้องแปลงเป็น PDF"); }
    acks::mark_opened(&st, &u, &code, v).await;
    let name = doccontrol::download_name(&st, &code, v, &filename).await;
    let title = st.by_code.get(&code).map(|d| format!("{code} {}", d["name"].as_str().unwrap_or(""))).unwrap_or_else(|| code.clone());
    let pdf = if ext == "pdf" { fs::read(&abs).map_err(|e| format!("อ่านไฟล์ไม่ได้: {e}")) } else { pdf_bytes(&st, &abs, &ext, title.trim()).await };
    let pdf = match pdf { Ok(p) => p, Err(e) => return err(StatusCode::BAD_GATEWAY, &e) };
    let Some(s) = revcontrol::view_stamp(&st, &u, &code, v, &headers).await else { return pdf_response(pdf, &name) };
    // ประทับไม่ได้ (PDF เข้ารหัส/เสียหาย) → ยังเปิดอ่านได้ แต่ไม่มีตรา
    let original = pdf.clone();
    match tokio::task::spawn_blocking(move || stamp::apply(&pdf, &s)).await {
        Ok(Ok(b)) => pdf_response(b, &name),
        Ok(Err(e)) => { eprintln!("  ⚠ ประทับตรา {code} #{v} ไม่ได้: {e}"); pdf_response(original, &name) }
        Err(_) => pdf_response(original, &name),
    }
}

/// GET /api/overview/pdf?path=&v= — ไฟล์ในหน้าภาพรวม
pub async fn overview_pdf(Query(q): Query<HashMap<String, String>>, State(st): State<S>) -> Response {
    let rel = q.get("path").cloned().unwrap_or_default();
    let (abs, ext, name, _, _) = match crate::overview_live_file(&st, &rel, q.get("v").and_then(|v| v.parse().ok())).await { Ok(x) => x, Err(r) => return r };
    if !convertible(&ext) { return err(StatusCode::BAD_REQUEST, "ไฟล์ชนิดนี้ไม่ต้องแปลงเป็น PDF"); }
    let title = name.rsplit_once('.').map(|(s, _)| s).unwrap_or(&name).to_string();
    match pdf_bytes(&st, &abs, &ext, &title).await {
        Ok(pdf) => pdf_response(pdf, &name),
        Err(e) => err(StatusCode::BAD_GATEWAY, &e),
    }
}

/// ตัวแปลง PDF พร้อมใช้งานไหม (สำหรับ /api/health)
pub async fn ready() -> bool {
    let Ok(c) = reqwest::Client::builder().timeout(Duration::from_millis(800)).build() else { return false };
    c.get(format!("{}/health", gotenberg_url())).send().await.map(|r| r.status().is_success()).unwrap_or(false)
}
