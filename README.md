# ISO 9001 Document Control — v2 (Svelte + Tailwind + Bun + Rust + Go + PostgreSQL)

ระบบควบคุมเอกสาร ISO 9001:2015 ของบริษัท เค การ์เด้น แอนด์ เฟนซ์ จำกัด

## สถาปัตยกรรม
```
Svelte + Tailwind + Bun  (frontend, :5173)  ──►  Rust API (Axum, :8080)  ──►  PostgreSQL (:5432, Docker)
        │                                              │
        └───────────── ผ่าน Vite proxy ───────────────┴──►  Go converter (:8081)  (docx → HTML)
```
- **Svelte + Tailwind CSS + Bun** — หน้าเว็บ (Vite dev server รันด้วย Bun); สไตล์ทั้งหมดใช้ Tailwind (ธีม/สีอยู่ใน `frontend/tailwind.config.js`)
- **Rust (Axum)** — API หลัก: `/api/docs`, `/api/stats`, `/api/overview`, `/api/actions` (CAR/DAR/PAR), เวอร์ชัน+ลายเซ็นเก็บใน **PostgreSQL** (`/api/health` เช็ค DB + Go)
- **PostgreSQL** — ตาราง `versions` เก็บประวัติเวอร์ชัน + ลายเซ็นผู้แก้ไข (รันผ่าน Docker Compose)
- **Go** — บริการแปลงเอกสาร: `/health`, `/convert` (docx → HTML)

## วิธีรัน
**ดับเบิลคลิก `start-all.bat`** — เริ่ม PostgreSQL (Docker) + 3 บริการ แล้วเปิดเบราว์เซอร์ที่ http://localhost:5173

### ใช้ในวง LAN
Frontend เปิดรับจากทุก interface (`--host`) — เครื่องอื่นใน Wi‑Fi/LAN เดียวกันเข้าได้ที่:
`http://<IP-เครื่องเซิร์ฟเวอร์>:5173`  
(สคริปต์ `start-all.bat` จะพิมพ์ IP ให้ และพยายามเปิด Firewall พอร์ต 5173)  
คำขอ `/api` ถูก proxy ผ่าน Vite ไปยัง API บนเครื่องเซิร์ฟเวอร์ ไม่ต้องเปิดพอร์ต 8080 ให้ LAN

หรือรันแยกเอง:
```bash
# 0) PostgreSQL (Docker Desktop ต้องเปิดอยู่)
docker compose up -d
# 1) Go converter
cd converter && go run .
# 2) Rust API  (DATABASE_URL ชี้ไป Postgres ของ docker-compose)
cd api && DATABASE_URL="host=localhost port=5432 user=iso password=iso dbname=iso" cargo run
# 3) Svelte + Bun
cd frontend && bun install && bun run dev
```

### PostgreSQL / การย้ายข้อมูล
- ตั้งค่า connection ผ่าน env `DATABASE_URL` (ค่าเริ่มต้นตรงกับ `docker-compose.yml`: user/password/db = `iso`)
- ครั้งแรกที่รัน ถ้าตาราง `versions` ใน Postgres ยังว่าง **และ** มีไฟล์ SQLite เดิม (`data/versions.db`) API จะ **migrate ข้อมูลเวอร์ชันเก่าให้อัตโนมัติ** ครั้งเดียว
- หยุด/ลบ DB: `docker compose down` (คง data), `docker compose down -v` (ลบ data ทั้งหมด)

## โปรเจกต์ open source ที่ใกล้เคียง (อ้างอิงแนวทาง UI/workflow)
เนื่องจากไม่มี QMS open source ที่ใช้ stack เดียวกัน (Rust/Svelte) โดยตรง ใช้เป็นแรงบันดาลใจด้าน UX/เวิร์กโฟลว์:
- **Docuseal** (github.com/docusealco/docuseal, Rails+Vue, AGPL) — ตัวอย่าง e-signature/approval flow ที่ดีที่สุด: ลำดับผู้เซ็น, สถานะการอนุมัติ, หน้า audit trail — ตรงกับฟีเจอร์ "ลงชื่อก่อนบันทึกเวอร์ชันใหม่"
- **Mayan EDMS** (github.com/mayan-edms/Mayan-EDMS, Django, Apache-2.0) — DMS ที่ใกล้ ISO ที่สุด: version list + revert, workflow state machine, cabinet/metadata คล้ายการจัดตามแผนก
- **Paperless-ngx** (github.com/paperless-ngx/paperless-ngx, Django+Angular, GPLv3) — UI ทะเบียนเอกสาร/แดชบอร์ด + ตัวอ่าน PDF ที่ทันสมัยและดูแลต่อเนื่อง
- อื่น ๆ: OpenQMS.net (โดเมน ISO 9001 ตรงสุด), LogicalDOC / OpenKM (workflow engine + version history UI)

## โครงสร้าง
```
iso-webapp-v2/
├─ frontend/   Svelte + Vite (รันด้วย Bun)   → src/App.svelte
├─ api/        Rust + Axum                   → src/main.rs
├─ converter/  Go (net/http)                 → main.go
├─ data/       manifest.json (แชร์)
└─ start-all.bat
```

## Toolchain ที่ติดตั้งแล้วในเครื่อง
- **Bun** 1.4 · **Go** 1.27 · **Rust** (GNU toolchain) + **MinGW-w64** (WinLibs) เป็น C linker

## รอบถัดไป (ยังไม่ทำ)
1. Rust: ต่อ **SQLite** (rusqlite) — เก็บเวอร์ชัน + ลายเซ็นผู้แก้ไข
2. Go: แปลง **docx → HTML** และอ่าน/เขียน **xlsx** จริง (Rust เรียกผ่าน HTTP)
3. Svelte: หน้าเปิด/แก้ไขเอกสาร + version tree + 2 หมวด (เหมือน v1)

> v1 (Node + vanilla JS) เดิมยังอยู่ที่ `C:\iso-webapp` และใช้งานได้ปกติ
