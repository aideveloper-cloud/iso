// converter — บริการแปลงเอกสาร (Go)
//   POST /convert {path, ext}  ->  แปลง .docx เป็น HTML (ใช้ stdlib zip+xml ไม่ต้องพึ่ง lib ภายนอก)
//   GET  /health
package main

import (
	"archive/zip"
	"bytes"
	"encoding/json"
	"encoding/xml"
	"fmt"
	"io"
	"log"
	"net/http"
	"os"
	"strconv"
	"strings"
	"time"
)

const service = "iso-converter (Go)"

func writeJSON(w http.ResponseWriter, code int, v any) {
	b, _ := json.Marshal(v)
	w.Header().Set("Content-Type", "application/json; charset=utf-8")
	w.Header().Set("Content-Length", strconv.Itoa(len(b)))
	w.WriteHeader(code)
	_, _ = w.Write(b)
}

func health(w http.ResponseWriter, r *http.Request) {
	writeJSON(w, 200, map[string]any{"service": service, "ok": true, "time": time.Now().Format(time.RFC3339)})
}

type convReq struct {
	Path string `json:"path"`
	Ext  string `json:"ext"`
}

func convert(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		writeJSON(w, 405, map[string]any{"ok": false, "error": "ต้องเป็น POST"})
		return
	}
	var req convReq
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		writeJSON(w, 400, map[string]any{"ok": false, "error": "อ่าน body ไม่ได้"})
		return
	}
	if strings.ToLower(req.Ext) != "docx" {
		writeJSON(w, 400, map[string]any{"ok": false, "error": "converter รองรับเฉพาะ .docx"})
		return
	}
	html, err := docxToHTML(req.Path)
	if err != nil {
		writeJSON(w, 500, map[string]any{"ok": false, "error": err.Error()})
		return
	}
	writeJSON(w, 200, map[string]any{"ok": true, "kind": "html", "html": html})
}

// ---------- docx -> HTML ----------
func docxToHTML(path string) (string, error) {
	zr, err := zip.OpenReader(path)
	if err != nil {
		return "", fmt.Errorf("เปิดไฟล์ไม่ได้: %w", err)
	}
	defer zr.Close()
	var data []byte
	for _, f := range zr.File {
		if f.Name == "word/document.xml" {
			rc, e := f.Open()
			if e != nil {
				return "", e
			}
			data, _ = io.ReadAll(rc)
			rc.Close()
			break
		}
	}
	if data == nil {
		return "", fmt.Errorf("ไม่พบ word/document.xml")
	}
	return renderDocx(data), nil
}

var htmlEsc = strings.NewReplacer("&", "&amp;", "<", "&lt;", ">", "&gt;")

func renderDocx(data []byte) string {
	dec := xml.NewDecoder(bytes.NewReader(data))
	var out strings.Builder
	var para strings.Builder
	inPara, inText, paraHasText := false, false, false
	bold, italic := false, false
	style := ""

	flush := func() {
		if !inPara {
			return
		}
		tag := "p"
		switch {
		case style == "Title" || strings.HasPrefix(style, "Heading1"):
			tag = "h2"
		case strings.HasPrefix(style, "Heading2"):
			tag = "h3"
		case strings.HasPrefix(style, "Heading"):
			tag = "h4"
		}
		if !paraHasText && tag == "p" {
			out.WriteString("<p></p>")
		} else {
			out.WriteString("<" + tag + ">" + para.String() + "</" + tag + ">")
		}
		para.Reset()
		inPara, paraHasText, style = false, false, ""
	}

	for {
		tok, err := dec.Token()
		if err != nil {
			break
		}
		switch t := tok.(type) {
		case xml.StartElement:
			switch t.Name.Local {
			case "p":
				flush()
				inPara = true
			case "pStyle":
				for _, a := range t.Attr {
					if a.Name.Local == "val" {
						style = a.Value
					}
				}
			case "b":
				bold = attrTrue(t)
			case "i":
				italic = attrTrue(t)
			case "t":
				inText = true
			case "tab":
				if inPara {
					para.WriteString("&nbsp;&nbsp;&nbsp;")
				}
			case "br":
				if inPara {
					para.WriteString("<br>")
				}
			case "tbl":
				flush()
				out.WriteString("<table class=\"docx-tbl\"><tbody>")
			case "tr":
				out.WriteString("<tr>")
			case "tc":
				out.WriteString("<td>")
			}
		case xml.EndElement:
			switch t.Name.Local {
			case "p":
				flush()
			case "t":
				inText = false
			case "r":
				bold, italic = false, false
			case "tbl":
				out.WriteString("</tbody></table>")
			case "tr":
				out.WriteString("</tr>")
			case "tc":
				out.WriteString("</td>")
			}
		case xml.CharData:
			if inText && inPara {
				s := htmlEsc.Replace(string(t))
				if s != "" {
					if italic {
						s = "<em>" + s + "</em>"
					}
					if bold {
						s = "<strong>" + s + "</strong>"
					}
					para.WriteString(s)
					paraHasText = true
				}
			}
		}
	}
	flush()
	return out.String()
}

func attrTrue(e xml.StartElement) bool {
	for _, a := range e.Attr {
		if a.Name.Local == "val" {
			v := strings.ToLower(a.Value)
			return v != "false" && v != "0" && v != "none"
		}
	}
	return true
}

func main() {
	port := os.Getenv("CONVERTER_PORT")
	if port == "" {
		port = "8081"
	}
	mux := http.NewServeMux()
	mux.HandleFunc("/health", health)
	mux.HandleFunc("/convert", convert)
	log.Printf("  ✅ %s พร้อมที่ http://localhost:%s", service, port)
	if err := http.ListenAndServe(":"+port, mux); err != nil {
		log.Fatal(err)
	}
}
