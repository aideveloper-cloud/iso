// อ่านเลข Rev. และรหัสเอกสารที่เขียนอยู่ "ในไฟล์" (หัว/ท้ายกระดาษ, เนื้อหา, เซลล์ Excel) เพื่อเทียบกับทะเบียน
//   .docx: word/document.xml + header/footer
//   .xlsx: ค่าในเซลล์ + ชื่อชีต (อ่าน XML ตรง — ไฟล์ที่บันทึกจากตัวแก้ไขบนเว็บบางไฟล์ calamine เปิดไม่ได้) + หัว/ท้ายกระดาษ
//   .xls: ค่าในเซลล์ + ชื่อชีต (calamine) + หัว/ท้ายกระดาษจากเรคคอร์ด HEADER/FOOTER ของ BIFF8 (แบบฟอร์มส่วนใหญ่ระบุ Rev. ที่ท้ายกระดาษ)
//   .doc / .pdf / รูป: ตรวจไม่ได้ (ไม่มีข้อความให้อ่านแบบเชื่อถือได้)
use std::io::{Cursor, Read};

/// เปลี่ยนเมื่อแก้กติกาการอ่าน — ผลที่แคชไว้ของรุ่นเก่าจะถูกอ่านใหม่
pub const SCAN_VERSION: &str = "rs5";

#[derive(Default)]
pub struct Found {
    /// เลข Rev. ที่เขียนติดกับรหัสของเอกสารเอง เช่น "FM-QC-003 Rev.0", "QP-DCC-001 Page 1 of 9 Rev. : 00" (น่าเชื่อถือที่สุด —
    /// ทะเบียน/Master List มี Rev. ของเอกสารอื่นปนอยู่ได้)
    pub self_revs: Vec<u32>,
    /// เลข Rev. ในหัว/ท้ายกระดาษ
    pub head_revs: Vec<u32>,
    /// เลข Rev. ในเนื้อหา/เซลล์
    pub body_revs: Vec<u32>,
    /// เลข Rev. ในชื่อชีต Excel (มักค้างจากการคัดลอกชีต — น่าเชื่อถือน้อยกว่าท้ายกระดาษ)
    pub name_revs: Vec<u32>,
    /// เลข Rev. ในชื่อชีตที่ติดกับรหัสเอกสารเอง เช่น ชีต "FM-DCC-001 Rev.0-FM" ของ Master List ที่ไม่มีท้ายกระดาษ
    pub self_name_revs: Vec<u32>,
    /// รหัสเอกสารที่ถูกอ้างถึง (QP/FM/WI/…)
    pub codes: Vec<String>,
}

pub fn scannable(ext: &str) -> bool { matches!(ext, "docx" | "xlsx" | "xls") }

/// อ่านไฟล์แล้วหาเลข Rev./รหัสเอกสาร (งาน CPU — เรียกใน spawn_blocking)
pub fn scan(bytes: &[u8], ext: &str, code: &str) -> Result<Found, String> {
    let (head, body, names) = match ext {
        "docx" => { let (h, b) = docx_text(bytes)?; (h, b, String::new()) }
        "xlsx" => xlsx_text(bytes)?,
        "xls" => { let (b, n) = xls_text(bytes)?; (clean_header_footer(&xls_header_footer(bytes)), b, n) }
        _ => return Err("ไฟล์ชนิดนี้ตรวจเลข Rev. ในไฟล์ไม่ได้".into()),
    };
    let mut self_revs = revs_near_code(&head, code);
    self_revs.extend(revs_near_code(&body, code));
    self_revs.sort_unstable();
    self_revs.dedup();
    let mut f = Found {
        self_revs, head_revs: find_revs(&head), body_revs: find_revs(&body),
        name_revs: find_revs(&names), self_name_revs: revs_near_code(&names, code), codes: find_codes(&head),
    };
    for c in find_codes(&body) { if !f.codes.contains(&c) { f.codes.push(c); } }
    Ok(f)
}

/// ข้อความจาก XML ของ Office: ตัดแท็กทิ้ง ขึ้นบรรทัดใหม่ที่จบย่อหน้า/แถว/เซลล์ และแปลง entity
fn xml_text(xml: &str) -> String {
    let mut out = String::with_capacity(xml.len() / 4);
    let mut rest = xml;
    while let Some(lt) = rest.find('<') {
        push_unescaped(&mut out, &rest[..lt]);
        let Some(gt) = rest[lt..].find('>') else { break };
        let tag = &rest[lt + 1..lt + gt];
        if tag.starts_with("/w:p") || tag.starts_with("/w:tc") || tag.starts_with("w:br") || tag.starts_with("w:tab") || tag.starts_with("/si")
            || tag.starts_with("/oddHeader") || tag.starts_with("/oddFooter") || tag.starts_with("/evenHeader") || tag.starts_with("/evenFooter")
            || tag.starts_with("/firstHeader") || tag.starts_with("/firstFooter") {
            out.push('\n');
        }
        rest = &rest[lt + gt + 1..];
    }
    push_unescaped(&mut out, rest);
    out
}

pub(crate) fn push_unescaped(out: &mut String, s: &str) {
    if !s.contains('&') { out.push_str(s); return; }
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let Some(semi) = tail.find(';').filter(|n| *n <= 10) else { out.push('&'); rest = &tail[1..]; continue };
        let ent = &tail[1..semi];
        let ch = match ent {
            "amp" => Some('&'), "lt" => Some('<'), "gt" => Some('>'), "quot" => Some('"'), "apos" => Some('\''),
            _ if ent.starts_with("#x") => u32::from_str_radix(&ent[2..], 16).ok().and_then(char::from_u32),
            _ if ent.starts_with('#') => ent[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match ch { Some(c) => out.push(c), None => out.push_str(&tail[..=semi]) }
        rest = &tail[semi + 1..];
    }
    out.push_str(rest);
}

/// ข้อความทั้งเอกสารสำหรับผู้ช่วย AI: หัว/ท้ายกระดาษ + ชื่อชีต + เนื้อหา (ตัดที่ max ตัวอักษร)
pub fn doc_text(bytes: &[u8], ext: &str, max: usize) -> Result<String, String> {
    let (head, body, names) = match ext {
        "docx" => { let (h, b) = docx_text(bytes)?; (h, b, String::new()) }
        "xlsx" => xlsx_text(bytes)?,
        "xls" => { let (b, n) = xls_text(bytes)?; (clean_header_footer(&xls_header_footer(bytes)), b, n) }
        _ => return Err("ไฟล์ชนิดนี้อ่านเนื้อหาไม่ได้ (รองรับ .docx .xlsx .xls)".into()),
    };
    let squeeze = |t: &str| t.lines().map(str::trim_end).filter(|l| !l.trim().is_empty()).collect::<Vec<_>>().join("
");
    let mut out = String::new();
    if !head.trim().is_empty() { out.push_str("[หัว/ท้ายกระดาษ]
"); out.push_str(&squeeze(&head)); out.push_str("

"); }
    if !names.trim().is_empty() { out.push_str("[ชื่อชีต]
"); out.push_str(&squeeze(&names)); out.push_str("

"); }
    out.push_str("[เนื้อหา]
");
    out.push_str(&squeeze(&body));
    if out.chars().count() > max { out = out.chars().take(max).collect::<String>() + "
…(ตัดเนื้อหาส่วนที่เหลือ)"; }
    Ok(out)
}

/// แก้ความยาว comment ใน End of Central Directory ให้ตรงกับไฟล์ — บางไฟล์ระบุยาวเกินจริง (Excel/Python เปิดได้ แต่ zip crate ปฏิเสธ)
pub(crate) fn repair_eocd(bytes: &[u8]) -> Vec<u8> {
    let mut b = bytes.to_vec();
    let start = b.len().saturating_sub(22 + 65_535);
    if let Some(i) = (start..b.len().saturating_sub(21)).rev().find(|&i| b[i..i + 4] == [0x50, 0x4b, 0x05, 0x06]) {
        let actual = (b.len() - i - 22) as u16;
        b[i + 20..i + 22].copy_from_slice(&actual.to_le_bytes());
    }
    b
}

fn zip_entries(bytes: &[u8], want: impl Fn(&str) -> bool) -> Result<Vec<(String, String)>, String> {
    let fixed = repair_eocd(bytes);
    let mut z = zip::ZipArchive::new(Cursor::new(fixed.as_slice())).map_err(|e| format!("เปิดไฟล์ไม่ได้: {e}"))?;
    let names: Vec<String> = z.file_names().filter(|n| want(n)).map(String::from).collect();
    let mut out = vec![];
    for n in names {
        let mut s = String::new();
        if let Ok(mut f) = z.by_name(&n) { let _ = f.read_to_string(&mut s); }
        out.push((n, s));
    }
    Ok(out)
}

/// (หัว/ท้ายกระดาษ, เนื้อหา) ของ .docx
fn docx_text(bytes: &[u8]) -> Result<(String, String), String> {
    let parts = zip_entries(bytes, |n| n == "word/document.xml" || (n.starts_with("word/header") || n.starts_with("word/footer")) && n.ends_with(".xml"))?;
    let (mut head, mut body) = (String::new(), String::new());
    for (n, xml) in parts {
        let t = xml_text(&xml);
        if n == "word/document.xml" { body = t } else { head.push_str(&t); head.push('\n'); }
    }
    if body.is_empty() { return Err("ไม่พบเนื้อหาเอกสาร (word/document.xml)".into()); }
    Ok((head, body))
}

/// ตัดรหัสจัดรูปแบบของหัว/ท้ายกระดาษ Excel (&L &C &R &"Angsana New,Bold" &14 &P …) ออก — เหลือแต่ข้อความ
fn clean_header_footer(s: &str) -> String {
    let cs: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < cs.len() {
        if cs[i] == '&' && i + 1 < cs.len() {
            let c = cs[i + 1];
            if c == '"' {
                i += 2;
                while i < cs.len() && cs[i] != '"' { i += 1; }
                i += 1;
            } else if c.is_ascii_digit() {
                i += 1;
                while i < cs.len() && cs[i].is_ascii_digit() { i += 1; }
            } else if c == '&' {
                out.push('&');
                i += 2;
            } else {
                i += 2;
            }
            out.push(' ');
            continue;
        }
        out.push(cs[i]);
        i += 1;
    }
    out
}

fn attr<'a>(tag: &'a str, name: &str) -> &'a str {
    let key = format!(" {name}=\"");
    tag.find(&key).map(|i| &tag[i + key.len()..]).and_then(|r| r.find('"').map(|e| &r[..e])).unwrap_or("")
}

/// (หัว/ท้ายกระดาษ, ค่าทุกเซลล์, ชื่อชีต) ของ .xlsx — อ่านตามลำดับใน XML: คั่นเซลล์ด้วยช่องว่าง ขึ้นบรรทัดใหม่ทุกแถว
/// "Rev." กับ "0" ที่อยู่คนละเซลล์ติดกันจึงอ่านเป็น "Rev. 0" ได้
fn xlsx_text(bytes: &[u8]) -> Result<(String, String, String), String> {
    let mut parts = zip_entries(bytes, |n| n == "xl/sharedStrings.xml" || n == "xl/workbook.xml" || (n.starts_with("xl/worksheets/sheet") && n.ends_with(".xml")))?;
    parts.sort_by(|a, b| a.0.cmp(&b.0));
    let mut sst: Vec<String> = vec![];
    let (mut head, mut body, mut names) = (String::new(), String::new(), String::new());
    for (n, xml) in &parts {
        if n == "xl/sharedStrings.xml" {
            for si in xml.split("<si>").skip(1) { sst.push(xml_text(&si[..si.find("</si>").unwrap_or(si.len())])); }
        } else if n == "xl/workbook.xml" {
            for t in xml.split("<sheet ").skip(1) { push_unescaped(&mut names, attr(&format!(" {t}"), "name")); names.push('\n'); }
        }
    }
    if parts.iter().all(|(n, _)| !n.starts_with("xl/worksheets/")) { return Err("ไม่พบชีตในไฟล์ Excel".into()); }
    for (n, xml) in &parts {
        if !n.starts_with("xl/worksheets/") { continue; }
        if let Some(a) = xml.find("<headerFooter") {
            if let Some(b) = xml[a..].find("</headerFooter>") { head.push_str(&clean_header_footer(&xml_text(&xml[a..a + b]))); head.push('\n'); }
        }
        let mut pos = 0;
        loop {
            let (nc, nr) = (xml[pos..].find("<c ").map(|x| pos + x), xml[pos..].find("</row>").map(|x| pos + x));
            match (nc, nr) {
                (Some(c), r) if r.map_or(true, |r| c < r) => {
                    let Some(te) = xml[c..].find('>').map(|e| c + e) else { break };
                    let tag = &xml[c..=te];
                    if tag.ends_with("/>") { pos = te + 1; continue; }
                    let close = xml[te..].find("</c>").map(|e| te + e).unwrap_or(xml.len());
                    let inner = &xml[te + 1..close];
                    let t = attr(tag, "t");
                    let raw = inner.find("<v>").and_then(|a| inner[a + 3..].find("</v>").map(|b| &inner[a + 3..a + 3 + b])).unwrap_or("");
                    match t {
                        "s" => if let Some(s) = raw.trim().parse::<usize>().ok().and_then(|i| sst.get(i)) { body.push_str(s); },
                        "inlineStr" => body.push_str(&xml_text(inner)),
                        _ => push_unescaped(&mut body, raw),
                    }
                    body.push(' ');
                    pos = close;
                }
                (_, Some(r)) => { body.push('\n'); pos = r + 6; }
                _ => break,
            }
            if body.len() > 400_000 { break; }
        }
    }
    Ok((head, body, names))
}

/// หัว/ท้ายกระดาษของ .xls: เรคคอร์ด BIFF8 HEADER (0x14) / FOOTER (0x15) = ความยาว + XLUnicodeString
/// ไม่แกะโครงสร้าง CFB ทั้งไฟล์ — ค้นเรคคอร์ดที่ความยาวสอดคล้องกับสตริงพอดีและเป็นข้อความที่พิมพ์ได้เท่านั้น
fn xls_header_footer(b: &[u8]) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i + 8 < b.len() {
        if (b[i] == 0x14 || b[i] == 0x15) && b[i + 1] == 0 {
            let len = u16::from_le_bytes([b[i + 2], b[i + 3]]) as usize;
            let cch = u16::from_le_bytes([b[i + 4], b[i + 5]]) as usize;
            let flags = b[i + 6];
            let high = flags & 1 == 1;
            let width = if high { 2 } else { 1 };
            if cch > 0 && (flags & 0xFE) == 0 && len == 3 + cch * width && i + 4 + len <= b.len() {
                let s = &b[i + 7..i + 4 + len];
                let text: String = if high {
                    char::decode_utf16(s.chunks_exact(2).map(|p| u16::from_le_bytes([p[0], p[1]]))).map(|r| r.unwrap_or('\u{fffd}')).collect()
                } else {
                    s.iter().map(|&c| c as char).collect()
                };
                if !text.contains('\u{fffd}') && text.chars().all(|c| !c.is_control() || c == '\n' || c == '\r') {
                    out.push_str(&text);
                    out.push('\n');
                    i += 4 + len;
                    continue;
                }
            }
        }
        i += 1;
    }
    out
}

/// (ค่าทุกเซลล์เรียงทีละแถว, ชื่อชีต) ของ .xls
fn xls_text(bytes: &[u8]) -> Result<(String, String), String> {
    use calamine::Reader;
    let mut wb = calamine::open_workbook_auto_from_rs(Cursor::new(bytes.to_vec())).map_err(|e| format!("เปิดไฟล์ Excel ไม่ได้: {e}"))?;
    let names = wb.sheet_names().join("\n");
    let mut out = String::new();
    for name in wb.sheet_names() {
        let Ok(range) = wb.worksheet_range(&name) else { continue };
        for row in range.rows().take(400) {
            for c in row {
                let s = c.to_string();
                if !s.is_empty() { out.push_str(&s); out.push(' '); }
            }
            out.push('\n');
            if out.len() > 400_000 { return Ok((out, names)); }
        }
    }
    Ok((out, names))
}

fn thai_digit(c: char) -> Option<u32> {
    let n = c as u32;
    if (0x0E50..=0x0E59).contains(&n) { Some(n - 0x0E50) } else { c.to_digit(10) }
}

/// ตัวเลข 1–3 หลักที่ตามหลังตำแหน่ง i (ข้ามช่องว่าง/ขึ้นบรรทัด . : # - และคำว่า no) — เลขยาวกว่านั้นไม่ใช่ Rev. (เช่น ปี 2565)
/// ขึ้นบรรทัดข้ามได้ เพราะตาราง Word มักแยก "Rev." กับเลขไว้คนละเซลล์
fn digits_after(cs: &[char], mut i: usize) -> Option<(u32, usize)> {
    let mut skipped = 0;
    while i < cs.len() && skipped < 12 {
        let c = cs[i];
        if matches!(c, ' ' | '.' | ':' | '#' | '-' | '\u{a0}' | '\n' | '\t') { i += 1; skipped += 1; continue; }
        if (c == 'n' || c == 'N') && i + 1 < cs.len() && (cs[i + 1] == 'o' || cs[i + 1] == 'O') { i += 2; skipped += 2; continue; }
        break;
    }
    let start = i;
    let mut n = 0u32;
    while i < cs.len() {
        match thai_digit(cs[i]) { Some(d) => { n = n * 10 + d; i += 1; } None => break }
        if i - start > 3 { return None; }
    }
    if i == start { None } else { Some((n, i)) }
}

/// ตำแหน่งของเลข Rev. ทั้งหมด: (ตัวเลข, ตำแหน่งเริ่มคำ, ตำแหน่งจบตัวเลข) — นับเป็นตัวอักษร (char)
fn rev_spans(text: &str) -> Vec<(u32, usize, usize)> {
    let cs: Vec<char> = text.chars().collect();
    let low: Vec<char> = cs.iter().map(|c| c.to_ascii_lowercase()).collect();
    let thai: Vec<Vec<char>> = ["แก้ไขครั้งที่", "ครั้งที่แก้ไข"].iter().map(|s| s.chars().collect()).collect();
    let mut out = vec![];
    let mut i = 0;
    while i < cs.len() {
        // rev / revision — ต้องไม่ใช่ส่วนหนึ่งของคำอื่น (prev, review, revised)
        if low[i..].starts_with(&['r', 'e', 'v']) && (i == 0 || !low[i - 1].is_ascii_alphabetic()) {
            let mut j = i + 3;
            if low[j..].starts_with(&['i', 's', 'i', 'o', 'n']) { j += 5; }
            if j >= cs.len() || !low[j].is_ascii_alphabetic() {
                if let Some((n, end)) = digits_after(&cs, j) { out.push((n, i, end)); i = end; continue; }
            }
        }
        for t in &thai {
            if cs[i..].starts_with(t) {
                if let Some((n, end)) = digits_after(&cs, i + t.len()) { out.push((n, i, end)); i = end - 1; break; }
            }
        }
        i += 1;
    }
    out
}

/// เลข Rev. ที่อยู่ถัดจากรหัสเอกสาร code ไม่เกิน 40 ตัวอักษร
fn revs_near_code(text: &str, code: &str) -> Vec<u32> {
    if code.is_empty() { return vec![]; }
    let cs: Vec<char> = text.chars().collect();
    let pat: Vec<char> = code.chars().collect();
    let ends: Vec<usize> = (0..cs.len().saturating_sub(pat.len() - 1))
        // รหัสอื่นที่ขึ้นต้นเหมือนกัน (FM-QC-0031, FM-QC-003-1_2) ไม่นับ — ตัวอักษรตามติดได้ เพราะเซลล์ตารางถูกต่อกันโดยไม่มีตัวคั่น
        .filter(|&i| cs[i..].starts_with(&pat) && {
            let next = cs.get(i + pat.len());
            !next.is_some_and(|c| c.is_ascii_digit()) && !(next == Some(&'-') && cs.get(i + pat.len() + 1).is_some_and(|c| c.is_ascii_digit()))
        })
        .map(|i| i + pat.len()).collect();
    if ends.is_empty() { return vec![]; }
    let mut v: Vec<u32> = rev_spans(text).into_iter()
        .filter(|(_, s, _)| ends.iter().any(|&e| e <= *s && *s - e <= 40))
        .map(|(n, _, _)| n).collect();
    v.sort_unstable();
    v.dedup();
    v
}

pub fn find_revs(text: &str) -> Vec<u32> {
    let mut v: Vec<u32> = rev_spans(text).into_iter().map(|(n, _, _)| n).collect();
    v.sort_unstable();
    v.dedup();
    v
}

/// เลข Rev. ของข้อความ เช่น "Rev.01" → 1, "Rev.0" → 0
pub fn rev_num(rev: &str) -> Option<u32> {
    rev_spans(rev).first().map(|x| x.0).or_else(|| rev.trim().parse().ok())
}

/// เปลี่ยนเลข Rev. ตัวแรกในชื่อไฟล์เป็น rev ใหม่ เช่น ("FM-QC-003 Rev.0 ใบตรวจ.xlsx", "Rev.01") → "FM-QC-003 Rev.01 ใบตรวจ.xlsx"
/// ชื่อที่ไม่มีเลข Rev. คงเดิม
pub fn rename_rev(filename: &str, rev: &str) -> String {
    let rev = rev.trim();
    let Some((_, s, e)) = rev_spans(filename).into_iter().next() else { return filename.to_string() };
    if rev.is_empty() { return filename.to_string(); }
    let cs: Vec<char> = filename.chars().collect();
    format!("{}{}{}", cs[..s].iter().collect::<String>(), rev, cs[e..].iter().collect::<String>())
}

/// รหัสเอกสารรูปแบบ XX-YYY-NNN (และส่วนต่อท้าย -1_2) ที่อ้างถึงในข้อความ
pub fn find_codes(text: &str) -> Vec<String> {
    const PRE: [&str; 6] = ["QM", "QP", "WI", "FM", "SD", "JD"];
    let cs: Vec<char> = text.chars().collect();
    let mut out: Vec<String> = vec![];
    let mut i = 0;
    while i + 8 <= cs.len() {
        let pre: String = cs[i..i + 2].iter().collect();
        if PRE.contains(&pre.as_str()) && cs[i + 2] == '-' && (i == 0 || !cs[i - 1].is_ascii_alphanumeric()) {
            let mut j = i + 3;
            while j < cs.len() && cs[j].is_ascii_uppercase() && j - i - 3 < 4 { j += 1; }
            let dept = j - i - 3;
            if (2..=4).contains(&dept) && j < cs.len() && cs[j] == '-' {
                let mut k = j + 1;
                while k < cs.len() && cs[k].is_ascii_digit() { k += 1; }
                let nd = k - j - 1;
                if (2..=3).contains(&nd) && (k >= cs.len() || !cs[k].is_ascii_alphanumeric() || cs[k] == '-') {
                    // ส่วนต่อท้ายแบบ FM-QC-003-1_2
                    if k + 4 <= cs.len() && cs[k] == '-' && cs[k + 1].is_ascii_digit() && cs[k + 2] == '_' && cs[k + 3].is_ascii_digit() { k += 4; }
                    let code: String = cs[i..k].iter().collect();
                    if !out.contains(&code) { out.push(code); }
                    i = k;
                    continue;
                }
            }
        }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_revision_forms() {
        assert_eq!(find_revs("FM-QC-003 Rev.0 ใบตรวจ"), vec![0]);
        assert_eq!(find_revs("REV : 02 / Revision 3 / Rev.No. 4 / rev no:5"), vec![2, 3, 4, 5]);
        assert_eq!(find_revs("แก้ไขครั้งที่ : ๒"), vec![2]);
        assert_eq!(find_revs("Rev.\n0 "), vec![0]);
    }

    #[test]
    fn ignores_lookalikes() {
        assert!(find_revs("review 2, revised 3, prev 4, Rev 2565").is_empty());
    }

    #[test]
    fn renames_rev_in_filename() {
        assert_eq!(rename_rev("FM-QC-003 Rev.0 ใบตรวจ.xlsx", "Rev.01"), "FM-QC-003 Rev.01 ใบตรวจ.xlsx");
        assert_eq!(rename_rev("QM คู่มือคุณภาพ.docx", "Rev.01"), "QM คู่มือคุณภาพ.docx");
        assert_eq!(rev_num("Rev.01"), Some(1));
        assert_eq!(rev_num("Rev.00"), Some(0));
    }

    #[test]
    fn finds_document_codes() {
        assert_eq!(find_codes("ใช้แบบฟอร์ม FM-DCC-002 และ WI-PD-01, FM-QC-003-1_2."), vec!["FM-DCC-002", "WI-PD-01", "FM-QC-003-1_2"]);
        assert!(find_codes("XFM-DCC-002 FM-DCC-0021").is_empty());
    }

    #[test]
    fn prefers_rev_next_to_own_code() {
        let master = "FM-DCC-002 ใบขออนุมัติ Rev.1
FM-QC-003 ใบตรวจ Rev.2
FM-DCC-001 Rev.0 วันที่มีผลบังคับใช้";
        assert_eq!(revs_near_code(master, "FM-DCC-001"), vec![0]);
        assert_eq!(revs_near_code("หมายเลข : QP-DCC-001Page 1 of 9Rev. : 00", "QP-DCC-001"), vec![0]);
        assert!(revs_near_code("FM-QC-003-1_2 Rev.4", "FM-QC-003").is_empty());
    }

    #[test]
    fn cleans_excel_header_codes() {
        let t = clean_header_footer("&L&\"Angsana New,Regular\"&14FM-MN-001 Rev.0 วันที่&R&P");
        assert_eq!(find_codes(&t), vec!["FM-MN-001"]);
        assert_eq!(find_revs(&t), vec![0]);
    }

    #[test]
    fn reads_biff_footer_record() {
        // FOOTER (0x15): len, cch, flags=1 (UTF-16LE), "FM-QC-001 Rev.2"
        let text: Vec<u16> = "FM-QC-001 Rev.2".encode_utf16().collect();
        let mut rec = vec![0x15, 0x00];
        rec.extend(((3 + text.len() * 2) as u16).to_le_bytes());
        rec.extend((text.len() as u16).to_le_bytes());
        rec.push(1);
        for u in &text { rec.extend(u.to_le_bytes()); }
        let mut file = vec![0xAAu8; 37];
        file.extend(rec);
        file.extend([0u8; 16]);
        assert_eq!(find_revs(&xls_header_footer(&file)), vec![2]);
    }

    #[test]
    fn xml_to_text() {
        let t = xml_text("<w:p><w:r><w:t>Rev</w:t></w:r><w:r><w:t>.0&amp;1</w:t></w:r></w:p><w:p><w:t>x</w:t></w:p>");
        assert_eq!(t, "Rev.0&1\nx\n");
    }
}

