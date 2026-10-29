// หน้าพิมพ์แบบฟอร์ม (HTML ที่พิมพ์เป็น PDF ได้) — ย้ายมาจาก converter/formhtml.go เดิม
//   หัวฟอร์ม (โลโก้ ชื่อ รหัส/Rev.) · ช่องข้อมูล · ตาราง (เว้นแถวว่างแบบแบบฟอร์มกระดาษ) · ช่องลงนาม · ท้ายกระดาษ
use serde_json::Value;

/// จำนวนแถวขั้นต่ำของตารางในใบพิมพ์ (เว้นบรรทัดว่างแบบแบบฟอร์มกระดาษ)
const MIN_ROWS: usize = 12;

fn s(v: &Value) -> String {
    match v {
        Value::String(t) => t.clone(),
        Value::Null => String::new(),
        o => o.to_string().trim_matches('"').to_string(),
    }
}

fn esc(v: &str) -> String {
    v.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&#34;").replace('\'', "&#39;")
}

fn e(v: &Value) -> String { esc(&s(v)) }

fn list(v: &Value) -> &[Value] { v.as_array().map(|a| a.as_slice()).unwrap_or(&[]) }

/// ช่องติ๊กแบบแบบฟอร์มกระดาษ
fn bx(on: bool) -> &'static str { if on { r#"<span class="bx on"></span>"# } else { r#"<span class="bx"></span>"# } }

/// เซลล์ตาราง: รูปลายเซ็น (data:image/...) แสดงเป็นรูป, ผลการอบรม ☑/☒ แสดงเป็นช่องติ๊ก
fn cell(v: &Value) -> String {
    let t = s(v);
    if t.starts_with("data:image/png;base64,") || t.starts_with("data:image/jpeg;base64,") {
        format!(r#"<img class="sig" alt="" src="{}">"#, esc(&t))
    } else if let Some(r) = t.strip_prefix("☑ ") {
        format!("{} {}", bx(true), esc(r))
    } else if let Some(r) = t.strip_prefix("☒ ") {
        format!(r#"<span class="bx no"></span> {}"#, esc(r))
    } else {
        esc(&t)
    }
}

pub fn render(p: &Value) -> String {
    let wm = s(&p["watermark"]);
    let wm_block = if wm.is_empty() { String::new() } else { format!(r#"<div class="wm">{}</div>"#, esc(&wm)) };

    // ช่องหัวฟอร์ม: ข้อความเรียงต่อกันเป็นบรรทัด (ขีดเส้นใต้ค่า) · ช่องตัวเลือกขึ้นบรรทัดของตัวเองพร้อมช่องติ๊ก
    let mut fields = String::from(r#"<div class="line">"#);
    for f in list(&p["fields"]) {
        if let Some(opts) = f["options"].as_array() {
            fields += &format!(r#"</div><div class="line choice"><b>{}</b>"#, e(&f["label"]));
            for o in opts {
                fields += &format!(r#"<span class="opt">{}{}</span>"#, bx(o["on"].as_bool().unwrap_or(false)), e(&o["label"]));
            }
            let other = s(&f["other"]);
            if !other.is_empty() { fields += &format!(r#"<span class="v">{}</span>"#, esc(&other)); }
            fields += r#"</div><div class="line">"#;
            continue;
        }
        fields += &format!(r#"<span class="f"><b>{}</b> <span class="v">{}</span></span>"#, e(&f["label"]), e(&f["value"]));
    }
    fields += "</div>";

    let cols = list(&p["columns"]);
    let mut head = String::from(r#"<tr><th class="no">ลำดับ</th>"#);
    for c in cols { head += &format!("<th>{}</th>", e(c)); }
    head += "</tr>";
    let rows = list(&p["rows"]);
    let mut body = String::new();
    for i in 0..rows.len().max(MIN_ROWS) {
        body += &format!(r#"<tr><td class="c">{}</td>"#, i + 1);
        match rows.get(i) {
            Some(r) => for c in list(r) { body += &format!("<td>{}</td>", cell(c)); },
            None => for _ in cols { body += "<td></td>"; },
        }
        body += "</tr>";
    }

    let mut sigs = String::new();
    for a in list(&p["approvals"]) {
        let name = s(&a["name"]);
        let line = if name.is_empty() { r#"<div class="sl blank"></div>"#.to_string() } else { format!(r#"<div class="sl">{}</div>"#, esc(&name)) };
        let at = match s(&a["at"]) { t if t.is_empty() => "วันที่ ........./........./.........".to_string(), t => t };
        sigs += &format!(r#"<div class="sig-box"><div class="k">{}</div>{line}<div class="t">{}</div><div class="cmt">{}</div></div>"#, e(&a["label"]), esc(&at), e(&a["comment"]));
    }

    let (title, code, rev, eff) = (e(&p["title"]), e(&p["code"]), e(&p["rev"]), e(&p["effective"]));
    format!(r#"<!DOCTYPE html><html lang="th"><head><meta charset="utf-8"><title>{ref_no}</title>
<link rel="preconnect" href="https://fonts.googleapis.com"><link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link href="https://fonts.googleapis.com/css2?family=Sarabun:wght@400;600;700&display=swap" rel="stylesheet">
<style>{CSS}</style></head><body>
{wm_block}
<div class="page">
<table class="hd"><tr>
  <td class="logo"><img src="/kg-logo.jpg" alt=""><div class="co">K GARDEN &amp; FENCE</div></td>
  <td class="ttl">{title}</td>
  <td class="code"><b>{code}</b><br>({rev})<br>{eff}</td>
</tr></table>
<div class="ref"><span>เลขที่ <b>{ref_no}</b></span><span>สถานะ: {status}</span></div>
{fields}
<table class="tb"><thead>{head}</thead><tbody>{body}</tbody></table>
<div class="legend"><b>หมายเหตุ</b> ผลการอบรม <span>{on} = ผ่าน</span> <span><span class="bx no"></span> = ไม่ผ่าน</span></div>
<div class="sigs">{sigs}</div>
<div class="foot"><span>{code} {rev} วันที่มีผลบังคับใช้ {eff}</span><span class="q">{qr}</span></div>
</div></body></html>"#,
        ref_no = e(&p["refNo"]), status = e(&p["status"]), on = bx(true), qr = e(&p["qrText"]))
}

const CSS: &str = r#"
@page { size: A4 portrait; margin: 10mm 10mm 12mm; }
* { box-sizing: border-box; }
body { font-family: "Sarabun", "TH Sarabun New", "Leelawadee UI", Tahoma, sans-serif; color: #111; font-size: 13px; margin: 0; -webkit-print-color-adjust: exact; print-color-adjust: exact; }
.page { position: relative; max-width: 190mm; margin: 0 auto; padding: 6mm 0; }
.wm { position: fixed; top: 40%; left: 0; right: 0; text-align: center; font-size: 54px; font-weight: 700; color: rgba(192,57,43,.16); transform: rotate(-20deg); pointer-events: none; z-index: 10; }
table.hd { width: 100%; border-collapse: collapse; }
table.hd td { border: 1px solid #222; padding: 6px 8px; vertical-align: middle; }
.logo { width: 28mm; text-align: center; }
.logo img { width: 17mm; height: auto; }
.logo .co { font-size: 8.5px; line-height: 1.2; margin-top: 2px; }
.ttl { text-align: center; font-size: 19px; font-weight: 700; }
.code { width: 30mm; text-align: center; font-size: 12px; line-height: 1.45; }
.code b { font-size: 13.5px; }
.ref { display: flex; justify-content: space-between; margin: 6px 0 2px; font-size: 12px; }
.line { display: flex; flex-wrap: wrap; gap: 4px 18px; padding: 5px 0 2px; }
.line:empty { display: none; }
.f { display: inline-flex; align-items: baseline; gap: 6px; flex: 1 1 auto; }
.f b, .choice b { white-space: nowrap; font-weight: 700; }
.v { flex: 1 1 auto; min-width: 22mm; border-bottom: 1px dotted #444; padding: 0 4px; }
.choice { align-items: center; gap: 4px 16px; }
.opt { display: inline-flex; align-items: center; gap: 6px; }
.bx { display: inline-block; width: 12px; height: 12px; border: 1.3px solid #222; position: relative; vertical-align: -1px; }
.bx.on::after { content: ""; position: absolute; left: 3px; top: 0; width: 4px; height: 8px; border: solid #111; border-width: 0 2px 2px 0; transform: rotate(45deg); }
.bx.no::before, .bx.no::after { content: ""; position: absolute; left: 5px; top: -1px; width: 1.6px; height: 12px; background: #111; transform: rotate(45deg); }
.bx.no::after { transform: rotate(-45deg); }
table.tb { width: 100%; border-collapse: collapse; margin-top: 8px; }
table.tb th, table.tb td { border: 1px solid #222; padding: 3px 6px; font-size: 12.5px; height: 27px; vertical-align: middle; }
table.tb th { font-weight: 700; text-align: center; background: #f2f2f2; }
table.tb td.c, table.tb th.no { text-align: center; width: 12mm; }
img.sig { height: 22px; max-width: 32mm; object-fit: contain; display: block; margin: 0 auto; }
.legend { display: flex; gap: 14px; align-items: center; margin-top: 6px; font-size: 12px; }
.sigs { display: flex; gap: 10px; margin-top: 14px; page-break-inside: avoid; }
.sig-box { flex: 1; border: 1px solid #222; padding: 6px 8px 8px; text-align: center; min-height: 30mm; display: flex; flex-direction: column; }
.sig-box .k { font-weight: 700; }
.sig-box .sl { margin: auto 6mm 2px; border-bottom: 1px dotted #444; min-height: 20px; padding-top: 10mm; font-weight: 600; }
.sig-box .t { font-size: 11.5px; }
.sig-box .cmt { font-size: 10.5px; color: #444; }
.foot { display: flex; justify-content: space-between; margin-top: 10px; font-size: 10.5px; color: #333; }
.foot .q { max-width: 60%; text-align: right; word-break: break-all; color: #666; }
@media screen { body { background: #e9edf2; } .page { background: #fff; padding: 10mm 12mm; margin: 12px auto; box-shadow: 0 2px 12px rgba(0,0,0,.15); } }
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn renders_fields_rows_and_signatures() {
        let html = render(&json!({
            "title": "ใบบันทึก <อบรม>", "code": "FM-HR-007", "rev": "Rev.1", "refNo": "R-1", "status": "อนุมัติแล้ว",
            "fields": [{ "label": "หัวข้อ", "value": "ความปลอดภัย" }, { "label": "ประเภท", "options": [{ "label": "ภายใน", "on": true }] }],
            "columns": ["ชื่อ", "ผล"], "rows": [["สมชาย", "☑ ผ่าน"]],
            "approvals": [{ "label": "ผู้จัดทำ", "name": "ก", "at": "1/1/2569" }, { "label": "ผู้อนุมัติ", "name": "" }],
        }));
        assert!(html.contains("ใบบันทึก &lt;อบรม&gt;") && html.contains(r#"<span class="opt"><span class="bx on"></span>ภายใน</span>"#));
        assert!(html.contains(r#"<td><span class="bx on"></span> ผ่าน</td>"#));
        assert_eq!(html.matches(r#"<td class="c">"#).count(), MIN_ROWS);
        assert!(html.contains(r#"<div class="sl blank"></div>"#) && html.contains("วันที่ ........./"));
    }
}
