# ISO 9001 Document Control — v2 (Svelte + Bun + Rust + Go)

Walking skeleton ของ stack ใหม่ — ทั้ง 4 เทคโนโลยีเชื่อมกันได้จริงแล้ว (รอบนี้ = โครง + 1 หน้า)

## สถาปัตยกรรม
```
Svelte + Bun  (frontend, :5173)  ──►  Rust API (Axum, :8080)  ──►  manifest.json / (ต่อไป: SQLite)
        │                                     │
        └───────── ผ่าน Vite proxy ───────────┴──►  Go converter (:8081)  (ต่อไป: แปลง docx/xlsx จริง)
```
- **Svelte + Bun** — หน้าเว็บ (Vite dev server รันด้วย Bun)
- **Rust (Axum)** — API หลัก: `/api/docs` (อ่าน manifest), `/api/health` (เช็คตัวเอง + ping Go)
- **Go** — บริการแปลงเอกสาร: `/health`, `/convert` (ตอนนี้เป็น stub)

## วิธีรัน
**ดับเบิลคลิก `start-all.bat`** — เปิด 3 บริการ (แต่ละอันหน้าต่างของตัวเอง) แล้วเปิดเบราว์เซอร์ที่ http://localhost:5173

หรือรันแยกเอง (3 หน้าต่าง):
```bash
# 1) Go converter
cd converter && go run .
# 2) Rust API
cd api && cargo run
# 3) Svelte + Bun
cd frontend && bun run dev
```

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
