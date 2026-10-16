# ย้าย iso.kgarden.co.th ขึ้น DigitalOcean

```
Cloudflare (iso.kgarden.co.th) → tunnel → web (Bun :8090) → api (Rust :8085) ⇄ converter (Go :8081, network เดียวกับ api)
                                                             └→ db (Postgres 16, volume iso-pgdata)
ข้อมูลเอกสาร/เวอร์ชัน/backup: /opt/iso/data บน Droplet
```

ไม่เปิดพอร์ตเว็บหรือฐานข้อมูลออกอินเทอร์เน็ต — เข้าทาง Cloudflare Tunnel อย่างเดียว เปิดแค่ SSH

## 1. สร้าง Droplet (เจ้าของบัญชีทำเอง)
- Ubuntu 24.04 LTS · Basic 2 GB RAM / 1 vCPU / 50 GB (build Rust ต้องการ RAM ≥ 2 GB) · Region **Singapore (SGP1)**
- Authentication: **SSH key** (ไม่ใช้รหัสผ่าน) · เปิด **Backups** รายสัปดาห์
- Networking → Firewall: อนุญาตขาเข้าเฉพาะ SSH (22) จาก IP ที่ใช้ดูแล

## 2. แพ็กข้อมูลบนเครื่อง Windows
```bash
bash deploy/digitalocean/package.sh
```
ได้ `code.tar.gz` `db.sql` `data.tar.gz` — **มีข้อมูลพนักงานและเอกสารจริง** ส่งผ่าน `scp` เท่านั้น

## 3. อัปโหลดและติดตั้ง
```bash
scp -r ../iso-migrate-<เวลา> root@<IP>:/root/iso-migrate
ssh root@<IP> "bash -s /root/iso-migrate" < deploy/digitalocean/server-setup.sh
```
สคริปต์จะติดตั้ง Docker, สุ่มรหัสผ่านฐานข้อมูลใหม่ลง `.env`, กู้ฐานข้อมูล แล้วเปิด db/api/converter/web
ตรวจ: บรรทัดสุดท้ายต้องเป็น `{"api":"ok","converter":"ok","db":"ok",...}`

## 4. สลับ tunnel (ช่วงนี้เว็บหยุด ~1–2 นาที)
ใช้ tunnel เดิม `iso-kgarden` — DNS ไม่ต้องแก้
1. บน Windows: ปิด `cloudflared.exe` (และปิดงาน start-iso ใน Task Scheduler ไม่ให้เปิดซ้ำ) — **ห้ามรันสองที่พร้อมกัน** Cloudflare จะแบ่ง traffic ไปทั้งสองเครื่องที่ฐานข้อมูลคนละชุด
2. ทำซ้ำข้อ 2–3 อีกรอบเพื่อเอาข้อมูลล่าสุด (หรือหยุดใช้งานเว็บก่อนแพ็กครั้งแรก)
3. คัดลอก `C:\Users\aidev\.cloudflared\<tunnel-id>.json` ไป `/opt/iso/deploy/digitalocean/cloudflared/` และสร้าง `config.yml` จาก `config.example.yml`
4. `cd /opt/iso/deploy/digitalocean && docker compose up -d tunnel`
5. เปิด https://iso.kgarden.co.th/api/health

## ดูแลระบบ
| งาน | คำสั่ง (ใน /opt/iso/deploy/digitalocean) |
|---|---|
| สถานะ / log | `docker compose ps` · `docker compose logs -f api` |
| อัปเดตเวอร์ชันใหม่ | แตก code.tar.gz ใหม่ทับ /opt/iso แล้ว `docker compose up -d --build` |
| backup ฐานข้อมูล | อัตโนมัติวันละครั้งที่ /opt/iso/data/backups (เก็บ 30 วัน) — ควรคัดลอกออกนอกเครื่องด้วย (DO Spaces / ดาวน์โหลดมาเก็บ) |
| ย้อนกลับไป Windows | หยุด tunnel บน Droplet แล้วเปิด cloudflared บน Windows ตามเดิม |
