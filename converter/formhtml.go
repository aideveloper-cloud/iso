package main

import (
	"encoding/json"
	"html"
	"net/http"
	"strings"
)

// จัดหน้าพิมพ์แบบฟอร์ม (HTML ที่พิมพ์เป็น PDF ได้ ฟอนต์ไทยของ Windows)
func formHTML(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		writeJSON(w, 405, map[string]any{"ok": false, "error": "ต้องเป็น POST"})
		return
	}
	var in map[string]any
	if err := json.NewDecoder(r.Body).Decode(&in); err != nil {
		writeJSON(w, 400, map[string]any{"ok": false, "error": "อ่านข้อมูลไม่ได้"})
		return
	}
	writeJSON(w, 200, map[string]any{"ok": true, "html": renderFormHTML(in)})
}

func esc(v any) string {
	return html.EscapeString(asStr(v))
}

func asStr(v any) string {
	switch t := v.(type) {
	case string:
		return t
	case nil:
		return ""
	default:
		b, _ := json.Marshal(t)
		s := string(b)
		return strings.Trim(s, `"`)
	}
}

func asList(v any) []any {
	if a, ok := v.([]any); ok {
		return a
	}
	return nil
}

// minRows — จำนวนแถวขั้นต่ำของตารางในใบพิมพ์ (เว้นบรรทัดว่างแบบแบบฟอร์มกระดาษ)
const minRows = 12

// ช่องติ๊กแบบแบบฟอร์มกระดาษ
func box(on bool) string {
	if on {
		return `<span class="bx on"></span>`
	}
	return `<span class="bx"></span>`
}

// เซลล์ตาราง: รูปลายเซ็น (data:image/...) แสดงเป็นรูป, ผลการอบรม ☑/☒ แสดงเป็นช่องติ๊ก
func cellHTML(v any) string {
	s := asStr(v)
	switch {
	case strings.HasPrefix(s, "data:image/png;base64,") || strings.HasPrefix(s, "data:image/jpeg;base64,"):
		return `<img class="sig" alt="" src="` + html.EscapeString(s) + `">`
	case strings.HasPrefix(s, "☑ "):
		return box(true) + " " + html.EscapeString(strings.TrimPrefix(s, "☑ "))
	case strings.HasPrefix(s, "☒ "):
		return `<span class="bx no"></span> ` + html.EscapeString(strings.TrimPrefix(s, "☒ "))
	}
	return html.EscapeString(s)
}

func renderFormHTML(in map[string]any) string {
	wm := asStr(in["watermark"])
	wmBlock := ""
	if wm != "" {
		wmBlock = `<div class="wm">` + esc(wm) + `</div>`
	}

	// ช่องหัวฟอร์ม: ข้อความเรียงต่อกันเป็นบรรทัด (ขีดเส้นใต้ค่า) · ช่องตัวเลือกขึ้นบรรทัดของตัวเองพร้อมช่องติ๊ก
	var fields strings.Builder
	fields.WriteString(`<div class="line">`)
	for _, f := range asList(in["fields"]) {
		m, _ := f.(map[string]any)
		if opts := asList(m["options"]); opts != nil {
			fields.WriteString(`</div><div class="line choice"><b>` + esc(m["label"]) + `</b>`)
			for _, o := range opts {
				om, _ := o.(map[string]any)
				on, _ := om["on"].(bool)
				fields.WriteString(`<span class="opt">` + box(on) + esc(om["label"]) + `</span>`)
			}
			if other := asStr(m["other"]); other != "" {
				fields.WriteString(`<span class="v">` + esc(other) + `</span>`)
			}
			fields.WriteString(`</div><div class="line">`)
			continue
		}
		fields.WriteString(`<span class="f"><b>` + esc(m["label"]) + `</b> <span class="v">` + esc(m["value"]) + `</span></span>`)
	}
	fields.WriteString(`</div>`)

	var head strings.Builder
	cols := asList(in["columns"])
	head.WriteString(`<tr><th class="no">ลำดับ</th>`)
	for _, c := range cols {
		head.WriteString("<th>" + esc(c) + "</th>")
	}
	head.WriteString("</tr>")
	var body strings.Builder
	rows := asList(in["rows"])
	n := len(rows)
	if n < minRows {
		n = minRows
	}
	for i := 0; i < n; i++ {
		body.WriteString(`<tr><td class="c">` + itoa(i+1) + `</td>`)
		if i < len(rows) {
			cells, _ := rows[i].([]any)
			for _, c := range cells {
				body.WriteString(`<td>` + cellHTML(c) + `</td>`)
			}
		} else {
			for range cols {
				body.WriteString(`<td></td>`)
			}
		}
		body.WriteString("</tr>")
	}

	var sigs strings.Builder
	for _, s := range asList(in["approvals"]) {
		m, _ := s.(map[string]any)
		name := asStr(m["name"])
		line := `<div class="sl">` + esc(name) + `</div>`
		if name == "" {
			line = `<div class="sl blank"></div>`
		}
		at := asStr(m["at"])
		if at == "" {
			at = "วันที่ ........./........./........."
		}
		sigs.WriteString(`<div class="sig-box"><div class="k">` + esc(m["label"]) + `</div>` + line + `<div class="t">` + esc(at) + `</div><div class="cmt">` + esc(m["comment"]) + `</div></div>`)
	}

	return `<!DOCTYPE html><html lang="th"><head><meta charset="utf-8"><title>` + esc(in["refNo"]) + `</title>
<link rel="preconnect" href="https://fonts.googleapis.com"><link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link href="https://fonts.googleapis.com/css2?family=Sarabun:wght@400;600;700&display=swap" rel="stylesheet">
<style>
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
</style></head><body>
` + wmBlock + `
<div class="page">
<table class="hd"><tr>
  <td class="logo"><img src="/kg-logo.jpg" alt=""><div class="co">K GARDEN &amp; FENCE</div></td>
  <td class="ttl">` + esc(in["title"]) + `</td>
  <td class="code"><b>` + esc(in["code"]) + `</b><br>(` + esc(in["rev"]) + `)<br>` + esc(in["effective"]) + `</td>
</tr></table>
<div class="ref"><span>เลขที่ <b>` + esc(in["refNo"]) + `</b></span><span>สถานะ: ` + esc(in["status"]) + `</span></div>
` + fields.String() + `
<table class="tb"><thead>` + head.String() + `</thead><tbody>` + body.String() + `</tbody></table>
<div class="legend"><b>หมายเหตุ</b> ผลการอบรม <span>` + box(true) + ` = ผ่าน</span> <span><span class="bx no"></span> = ไม่ผ่าน</span></div>
<div class="sigs">` + sigs.String() + `</div>
<div class="foot"><span>` + esc(in["code"]) + ` ` + esc(in["rev"]) + ` วันที่มีผลบังคับใช้ ` + esc(in["effective"]) + `</span><span class="q">` + esc(in["qrText"]) + `</span></div>
</div></body></html>`
}

func itoa(n int) string {
	if n == 0 {
		return "0"
	}
	var b [12]byte
	i := len(b)
	for n > 0 {
		i--
		b[i] = byte('0' + n%10)
		n /= 10
	}
	return string(b[i:])
}
