// ประทับตราควบคุมเอกสารลงทุกหน้าของ PDF (ISO 9001:2015 ข้อ 7.5.3.2 c–d — ควบคุมฉบับ และกันการนำฉบับล้าสมัยไปใช้)
//   ย่อเนื้อหาเดิมลงเล็กน้อยให้เหลือแถบว่างด้านล่าง แล้ววางข้อความ + QR ไว้ในแถบนั้น จึงไม่ทับหัว/ท้ายกระดาษของต้นฉบับ
//   ฉบับที่ไม่ใช่ฉบับใช้งาน (ถูกแทนที่/ยกเลิกใช้) ใส่ลายน้ำแนวทแยงเพิ่ม
//   ข้อความเป็น ASCII เท่านั้น — ฟอนต์มาตรฐานของ PDF (Helvetica) ไม่มีอักษรไทย; รายละเอียดภาษาไทยดูได้จากการสแกน QR
use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};

pub struct Stamp {
    /// บรรทัดหลัก (ตัวหนา) เช่น "CONTROLLED COPY No. CC-2569-0001"
    pub headline: String,
    /// บรรทัดรอง เช่น "FM-QC-003 Rev.01 | Issued 27/10/2569 | Scan QR to verify the current revision"
    pub detail: String,
    /// ลิงก์ใน QR (หน้าตรวจสอบ Rev. แบบไม่ต้อง login) — ว่าง = ไม่มี QR
    pub qr: String,
    /// ลายน้ำแนวทแยง เช่น "SUPERSEDED" / "OBSOLETE" — ว่าง = ไม่มี
    pub watermark: String,
    /// แถบสีแดง (ห้ามใช้งาน) แทนสีเทา
    pub alert: bool,
}

/// ความสูงแถบตราประทับด้านล่าง (pt) — QR ขนาดนี้สแกนด้วยมือถือได้เมื่อพิมพ์ A4
const BAND: f32 = 48.0;

type M = [f32; 6];

/// ผลคูณเมทริกซ์ affine แบบ PDF (เวกเตอร์แถว): ใช้ a ก่อนแล้วจึง b
fn mul(a: &M, b: &M) -> M {
    [
        a[0] * b[0] + a[1] * b[2],
        a[0] * b[1] + a[1] * b[3],
        a[2] * b[0] + a[3] * b[2],
        a[2] * b[1] + a[3] * b[3],
        a[4] * b[0] + a[5] * b[2] + b[4],
        a[4] * b[1] + a[5] * b[3] + b[5],
    ]
}
fn inv(m: &M) -> M {
    let det = m[0] * m[3] - m[1] * m[2];
    [m[3] / det, -m[1] / det, -m[2] / det, m[0] / det, (m[2] * m[5] - m[3] * m[4]) / det, (m[1] * m[4] - m[0] * m[5]) / det]
}
fn cm(m: &M) -> String { format!("{:.4} {:.4} {:.4} {:.4} {:.3} {:.3} cm\n", m[0], m[1], m[2], m[3], m[4], m[5]) }

/// พิกัดที่ผู้อ่านเห็น (มุมล่างซ้ายของหน้าที่หมุนแล้ว) → พิกัดของหน้า PDF ตาม /Rotate
fn vis_to_user(rot: i64, x0: f32, y0: f32, w: f32, h: f32) -> M {
    match rot {
        90 => [0.0, 1.0, -1.0, 0.0, x0 + w, y0],
        180 => [-1.0, 0.0, 0.0, -1.0, x0 + w, y0 + h],
        270 => [0.0, -1.0, 1.0, 0.0, x0, y0 + h],
        _ => [1.0, 0.0, 0.0, 1.0, x0, y0],
    }
}

fn num(o: &Object) -> Option<f32> {
    match o { Object::Integer(i) => Some(*i as f32), Object::Real(r) => Some(*r), _ => None }
}

/// ค่าที่สืบทอดจากโหนด Pages ได้ (MediaBox, CropBox, Rotate, Resources) — คืนค่าที่ dereference แล้ว
fn inherited(doc: &Document, page: ObjectId, key: &[u8]) -> Option<Object> {
    let mut id = page;
    for _ in 0..32 {
        let d = doc.get_dictionary(id).ok()?;
        if let Ok(o) = d.get(key) {
            return doc.dereference(o).ok().map(|(_, o)| o.clone());
        }
        id = d.get(b"Parent").and_then(Object::as_reference).ok()?;
    }
    None
}

/// กรอบที่มองเห็น (CropBox ถ้ามี ไม่งั้น MediaBox): x0, y0, กว้าง, สูง
fn page_box(doc: &Document, page: ObjectId) -> (f32, f32, f32, f32) {
    let b = inherited(doc, page, b"CropBox").or_else(|| inherited(doc, page, b"MediaBox"));
    let v: Vec<f32> = b.as_ref().and_then(|o| o.as_array().ok()).map(|a| a.iter().filter_map(num).collect()).unwrap_or_default();
    if v.len() != 4 { return (0.0, 0.0, 595.0, 842.0); }
    let (x0, x1) = (v[0].min(v[2]), v[0].max(v[2]));
    let (y0, y1) = (v[1].min(v[3]), v[1].max(v[3]));
    (x0, y0, (x1 - x0).max(1.0), (y1 - y0).max(1.0))
}

fn deref_dict(doc: &Document, o: Option<&Object>) -> Dictionary {
    o.and_then(|o| doc.dereference(o).ok()).and_then(|(_, o)| o.as_dict().ok().cloned()).unwrap_or_default()
}

fn matrix_of(o: Option<&Object>) -> M {
    let v: Vec<f32> = o.and_then(|o| o.as_array().ok()).map(|a| a.iter().filter_map(num).collect()).unwrap_or_default();
    if v.len() == 6 { [v[0], v[1], v[2], v[3], v[4], v[5]] } else { [1.0, 0.0, 0.0, 1.0, 0.0, 0.0] }
}

/// ลาย (Pattern) ของหน้าอ้างพิกัดของหน้าเสมอ ไม่ขยับตาม cm ที่ใช้ย่อเนื้อหา — ONLYOFFICE วาดรูปภาพ (เช่นโลโก้หัวกระดาษ) เป็นลาย
/// ถ้าไม่ย่อลายตาม รูปจะเลื่อนออกจากกรอบแล้วถูกตัด → คัดลอกลายเป็นของหน้านั้นแล้วคูณ /Matrix ด้วยเมทริกซ์ย่อ
fn shrink_patterns(doc: &mut Document, res: &mut Dictionary, m: &M) {
    let pats = deref_dict(doc, res.get(b"Pattern").ok());
    if pats.is_empty() { return; }
    let mut out = Dictionary::new();
    for (name, o) in pats.iter() {
        let Ok((_, obj)) = doc.dereference(o) else { out.set(name.clone(), o.clone()); continue };
        let mut obj = obj.clone();
        let dict = match &mut obj { Object::Stream(s) => Some(&mut s.dict), Object::Dictionary(d) => Some(d), _ => None };
        if let Some(d) = dict {
            let pm = mul(&matrix_of(d.get(b"Matrix").ok()), m);
            d.set("Matrix", Object::Array(pm.iter().map(|x| Object::Real(*x)).collect()));
        }
        out.set(name.clone(), Object::Reference(doc.add_object(obj)));
    }
    res.set("Pattern", Object::Dictionary(out));
}

/// ใส่ฟอนต์/ExtGState ของตราประทับลงใน Resources ของหน้า (คัดลอกเป็นของหน้านั้นเอง ไม่แตะหน้าอื่นที่ใช้ร่วมกัน)
fn add_resources(doc: &mut Document, page: ObjectId, fonts: &[(&str, ObjectId)], gs: ObjectId, content_m: &M) {
    let res_obj = inherited(doc, page, b"Resources");
    let mut res = res_obj.as_ref().and_then(|o| o.as_dict().ok().cloned()).unwrap_or_default();
    shrink_patterns(doc, &mut res, content_m);
    let mut font = deref_dict(doc, res.get(b"Font").ok());
    for (name, id) in fonts { font.set(name.as_bytes().to_vec(), Object::Reference(*id)); }
    res.set("Font", Object::Dictionary(font));
    let mut ext = deref_dict(doc, res.get(b"ExtGState").ok());
    ext.set("GSiso", Object::Reference(gs));
    res.set("ExtGState", Object::Dictionary(ext));
    if let Ok(d) = doc.get_dictionary_mut(page) { d.set("Resources", Object::Dictionary(res)); }
}

/// ข้อความใน PDF string: เฉพาะ ASCII ที่พิมพ์ได้ และ escape วงเล็บ/backslash
fn pdf_text(s: &str, max: usize) -> String {
    let mut out = String::new();
    for ch in s.chars().take(max) {
        match ch {
            '(' | ')' | '\\' => { out.push('\\'); out.push(ch); }
            c if (' '..='~').contains(&c) => out.push(c),
            _ => out.push('?'),
        }
    }
    out
}

/// โมดูลของ QR (EC ระดับ L เพื่อให้ช่องใหญ่ที่สุดในพื้นที่จำกัด)
fn qr_modules(text: &str) -> Option<(usize, Vec<bool>)> {
    if text.is_empty() { return None; }
    let code = qrcode::QrCode::with_error_correction_level(text.as_bytes(), qrcode::EcLevel::L).ok()?;
    let w = code.width();
    let mut v = Vec::with_capacity(w * w);
    for y in 0..w { for x in 0..w { v.push(code[(x, y)] == qrcode::Color::Dark); } }
    Some((w, v))
}

/// คำสั่งวาดตราประทับในพิกัดที่ผู้อ่านเห็น (กว้าง vw × สูง vh)
fn stamp_ops(s: &Stamp, vw: f32, vh: f32, qr: &Option<(usize, Vec<bool>)>) -> String {
    let mut o = String::new();
    if !s.watermark.is_empty() {
        let text = pdf_text(&s.watermark, 40);
        let diag = (vw * vw + vh * vh).sqrt();
        let (sin, cos) = (vh / diag, vw / diag);
        let size = (0.62 * diag / (text.len().max(1) as f32 * 0.68)).min(120.0);
        let tw = text.len() as f32 * size * 0.68;
        let x = vw / 2.0 - cos * tw / 2.0 + sin * size * 0.35;
        let y = vh / 2.0 - sin * tw / 2.0 - cos * size * 0.35;
        o.push_str(&format!("q /GSiso gs 0.85 0.1 0.1 rg BT /FisoB {size:.1} Tf {cos:.4} {sin:.4} {:.4} {cos:.4} {x:.2} {y:.2} Tm ({text}) Tj ET Q\n", -sin));
    }
    // แถบพื้นหลัง + เส้นคั่นด้านบน
    let (bg, line, ink) = if s.alert { ("1 0.92 0.92", "0.8 0.15 0.15", "0.7 0.05 0.05") } else { ("0.97 0.97 0.97", "0.6 0.6 0.6", "0.15 0.15 0.15") };
    o.push_str(&format!("q {bg} rg 0 0 {vw:.2} {BAND:.2} re f {line} RG 0.6 w 0 {BAND:.2} m {vw:.2} {BAND:.2} l S Q\n"));
    // QR ชิดขวา (มีขอบขาว 1 ช่อง)
    let mut text_right = vw - 8.0;
    if let Some((n, mods)) = qr {
        let side = BAND - 4.0;
        let cell = side / (*n as f32 + 2.0);
        let (qx, qy) = (vw - side - 6.0, 2.0);
        o.push_str(&format!("q 1 1 1 rg {qx:.2} {qy:.2} {side:.2} {side:.2} re f 0 0 0 rg\n"));
        for (i, dark) in mods.iter().enumerate() {
            if !*dark { continue; }
            let (cx, cy) = (i % n, i / n);
            let x = qx + (cx as f32 + 1.0) * cell;
            let y = qy + side - (cy as f32 + 2.0) * cell;
            o.push_str(&format!("{x:.3} {y:.3} {:.3} {:.3} re\n", cell + 0.02, cell + 0.02));
        }
        o.push_str("f Q\n");
        text_right = qx - 8.0;
    }
    // ข้อความ: ตัดตามความกว้างที่เหลือ (Helvetica เฉลี่ย ~0.52 em)
    let width = (text_right - 10.0).max(40.0);
    let head = pdf_text(&s.headline, (width / (9.0 * 0.6)) as usize);
    let detail = pdf_text(&s.detail, (width / (6.8 * 0.52)) as usize);
    o.push_str(&format!("q {ink} rg BT /FisoB 9 Tf 10 {:.2} Td ({head}) Tj ET BT /FisoR 6.8 Tf 10 {:.2} Td ({detail}) Tj ET Q\n", BAND - 17.0, BAND - 31.0));
    o
}

/// ประทับตราทุกหน้า แล้วคืนไฟล์ PDF ใหม่
pub fn apply(pdf: &[u8], s: &Stamp) -> Result<Vec<u8>, String> {
    let mut doc = Document::load_mem(pdf).map_err(|e| format!("อ่าน PDF ไม่ได้: {e}"))?;
    if doc.is_encrypted() { return Err("PDF นี้มีรหัสผ่าน — ประทับตราไม่ได้".into()); }
    let reg = doc.add_object(dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica", "Encoding" => "WinAnsiEncoding" });
    let bold = doc.add_object(dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica-Bold", "Encoding" => "WinAnsiEncoding" });
    let gs = doc.add_object(dictionary! { "Type" => "ExtGState", "ca" => Object::Real(0.16), "CA" => Object::Real(0.16) });
    let qr = qr_modules(&s.qr);

    let pages: Vec<ObjectId> = doc.get_pages().values().copied().collect();
    for page in pages {
        let (x0, y0, w, h) = page_box(&doc, page);
        let rot = inherited(&doc, page, b"Rotate").and_then(|o| o.as_i64().ok()).unwrap_or(0).rem_euclid(360);
        let (vw, vh) = if rot == 90 || rot == 270 { (h, w) } else { (w, h) };
        let v2u = vis_to_user(rot, x0, y0, w, h);
        // ย่อเนื้อหาเดิมเข้าพื้นที่เหนือแถบ (ยึดขอบบน กึ่งกลางแนวนอน) — คิดในพิกัดที่ผู้อ่านเห็นแล้วแปลงกลับเป็นพิกัดหน้า
        let k = (vh - BAND) / vh;
        let shrink: M = [k, 0.0, 0.0, k, vw * (1.0 - k) / 2.0, BAND];
        let content_m = mul(&mul(&inv(&v2u), &shrink), &v2u);

        add_resources(&mut doc, page, &[("FisoR", reg), ("FisoB", bold)], gs, &content_m);

        let pre = doc.add_object(Stream::new(dictionary! {}, format!("q\n{}", cm(&content_m)).into_bytes()));
        let mut post_stream = Stream::new(dictionary! {}, format!("\nQ\nq\n{}{}Q\n", cm(&v2u), stamp_ops(s, vw, vh, &qr)).into_bytes());
        let _ = post_stream.compress();
        let post = doc.add_object(post_stream);

        let old = doc.get_dictionary(page).ok().and_then(|d| d.get(b"Contents").ok().cloned());
        let mut parts = vec![Object::Reference(pre)];
        match old {
            Some(Object::Array(a)) => parts.extend(a),
            Some(Object::Reference(id)) => match doc.get_object(id) {
                Ok(Object::Array(a)) => parts.extend(a.clone()),
                _ => parts.push(Object::Reference(id)),
            },
            Some(other) => parts.push(other),
            None => {}
        }
        parts.push(Object::Reference(post));
        if let Ok(d) = doc.get_dictionary_mut(page) { d.set("Contents", Object::Array(parts)); }
    }
    let mut out = Vec::with_capacity(pdf.len() + 64 * 1024);
    doc.save_to(&mut out).map_err(|e| format!("บันทึก PDF ไม่ได้: {e}"))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_roundtrip() {
        let m = vis_to_user(90, 10.0, 20.0, 595.0, 842.0);
        let r = mul(&m, &inv(&m));
        for (a, b) in r.iter().zip([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]) { assert!((a - b).abs() < 1e-3); }
    }

    #[test]
    fn page_patterns_follow_shrink() {
        let mut doc = Document::new();
        let pat = doc.add_object(dictionary! { "PatternType" => 1, "Matrix" => vec![Object::Real(2.0), 0.into(), 0.into(), Object::Real(2.0), 10.into(), 20.into()] });
        let mut res = dictionary! { "Pattern" => dictionary! { "P1" => Object::Reference(pat) } };
        shrink_patterns(&mut doc, &mut res, &[0.5, 0.0, 0.0, 0.5, 5.0, 48.0]);
        let id = res.get(b"Pattern").unwrap().as_dict().unwrap().get(b"P1").unwrap().as_reference().unwrap();
        assert_ne!(id, pat); // คัดลอกใหม่ ไม่แก้ลายที่หน้าอื่นใช้ร่วม
        let m = matrix_of(doc.get_dictionary(id).unwrap().get(b"Matrix").ok());
        assert_eq!(m, [1.0, 0.0, 0.0, 1.0, 10.0, 58.0]);
    }

    #[test]
    fn escapes_and_strips_non_ascii() {
        assert_eq!(pdf_text("A(b)\\ ไทย", 50), "A\\(b\\)\\\\ ???");
    }
}
