// converter — บริการแปลงเอกสาร (Go)
//
//	POST /convert {path, ext}  ->  แปลง .docx เป็น HTML (ใช้ stdlib zip+xml ไม่ต้องพึ่ง lib ภายนอก)
//	GET  /health
package main

import (
	"encoding/json"
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

func main() {
	if len(os.Args) > 2 && os.Args[1] == "dump" {
		h, err := docxToHTML(os.Args[2])
		if err != nil {
			log.Fatal(err)
		}
		os.Stdout.WriteString(h)
		return
	}
	port := os.Getenv("CONVERTER_PORT")
	if port == "" {
		port = "8081"
	}
	mux := http.NewServeMux()
	mux.HandleFunc("/health", health)
	mux.HandleFunc("/convert", convert)
	mux.HandleFunc("/form-html", formHTML)
	log.Printf("  ✅ %s พร้อมที่ http://localhost:%s", service, port)
	if err := http.ListenAndServe(":"+port, mux); err != nil {
		log.Fatal(err)
	}
}
