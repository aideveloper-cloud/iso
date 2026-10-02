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

func renderFormHTML(in map[string]any) string {
	wm := asStr(in["watermark"])
	wmBlock := ""
	if wm != "" {
		wmBlock = `<div class="wm">` + esc(wm) + `</div>`
	}
	var fields strings.Builder
	for _, f := range asList(in["fields"]) {
		m, _ := f.(map[string]any)
		fields.WriteString(`<div class="fld"><div class="k">` + esc(m["label"]) + `</div><div class="v">` + esc(m["value"]) + `</div></div>`)
	}
	var head strings.Builder
	var cols []any
	if raw, ok := in["columns"].([]any); ok {
		cols = raw
		head.WriteString("<tr><th>ลำดับ</th>")
		for _, c := range cols {
			head.WriteString("<th>" + esc(c) + "</th>")
		}
		head.WriteString("</tr>")
	}
	var body strings.Builder
	for i, row := range asList(in["rows"]) {
		cells, _ := row.([]any)
		body.WriteString("<tr><td class='c'>" + itoa(i+1) + "</td>")
		for _, c := range cells {
			body.WriteString("<td>" + esc(c) + "</td>")
		}
		body.WriteString("</tr>")
	}
	var steps strings.Builder
	for _, s := range asList(in["approvals"]) {
		m, _ := s.(map[string]any)
		steps.WriteString(`<div class="sig"><div class="k">` + esc(m["label"]) + `</div><div class="n">` + esc(m["name"]) + `</div><div class="t">` + esc(m["at"]) + `</div><div class="cmt">` + esc(m["comment"]) + `</div></div>`)
	}
	qr := esc(in["qrText"])
	return `<!DOCTYPE html><html lang="th"><head><meta charset="utf-8"><title>` + esc(in["refNo"]) + `</title>
<style>
@page { size: A4; margin: 12mm; }
body { font-family: "Leelawadee UI", "Sarabun", "Tahoma", sans-serif; color: #1a1a1a; font-size: 13px; margin: 0; }
.sheet { position: relative; border: 1.5px solid #0B3679; padding: 16px 18px 20px; }
.wm { position: fixed; inset: 30%; display: flex; align-items: center; justify-content: center; font-size: 42px; font-weight: 700; color: rgba(192,80,77,.28); transform: rotate(-18deg); pointer-events: none; }
.head { display: flex; justify-content: space-between; gap: 16px; border-bottom: 2px solid #0B3679; padding-bottom: 8px; }
.org { font-size: 12px; color: #445; }
h1 { font-size: 18px; margin: 2px 0 0; color: #0B3679; }
.meta { text-align: right; font-size: 12px; line-height: 1.45; }
.meta b { color: #0B3679; }
.ref { margin: 10px 0; font-weight: 700; }
.grid { display: grid; grid-template-columns: 1fr 1fr; gap: 6px 18px; margin-bottom: 12px; }
.fld .k { color: #667; font-size: 11px; }
.fld .v { border-bottom: 1px solid #ccd; min-height: 18px; }
table { width: 100%; border-collapse: collapse; margin-top: 6px; }
th, td { border: 1px solid #9aa; padding: 4px 6px; vertical-align: top; font-size: 12px; }
th { background: #EAF1FB; color: #0B3679; }
td.c { text-align: center; width: 36px; }
.sigs { display: flex; gap: 12px; margin-top: 16px; }
.sig { flex: 1; border: 1px solid #ccd; padding: 8px; min-height: 72px; }
.sig .k { font-size: 11px; color: #667; }
.sig .n { font-weight: 700; margin-top: 8px; }
.foot { margin-top: 10px; font-size: 11px; color: #667; }
</style></head><body>
` + wmBlock + `
<div class="sheet">
<div class="head">
  <div><div class="org">บริษัท เค การ์เด้น แอนด์ เฟนซ์ จำกัด</div><h1>` + esc(in["title"]) + `</h1></div>
  <div class="meta"><div><b>` + esc(in["code"]) + `</b> ` + esc(in["rev"]) + `</div><div>วันที่บังคับใช้ ` + esc(in["effective"]) + `</div><div>สถานะ ` + esc(in["status"]) + `</div></div>
</div>
<div class="ref">เลขที่ ` + esc(in["refNo"]) + `</div>
<div class="grid">` + fields.String() + `</div>
<table><thead>` + head.String() + `</thead><tbody>` + body.String() + `</tbody></table>
<div class="sigs">` + steps.String() + `</div>
<div class="foot">เปิดดูบันทึกในระบบ: ` + qr + `</div>
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
