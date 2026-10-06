#!/usr/bin/env bash
# รันบนเครื่อง Windows (Git Bash) ที่ root ของ repo — แพ็กทุกอย่างที่ต้องย้ายไป Droplet
#   1) โค้ด (git archive ของ HEAD — commit ก่อน)  2) dump ฐานข้อมูลจริง  3) โฟลเดอร์ data/ (เอกสาร เวอร์ชัน ข้อมูลพนักงาน)
# ผลลัพธ์อยู่ที่ OUT (ค่าเริ่มต้น ../iso-migrate-<เวลา>) — มีข้อมูลส่วนบุคคล ห้ามอัปโหลดที่สาธารณะ
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
STAMP=$(date +%Y%m%d-%H%M)
OUT=${OUT:-../iso-migrate-$STAMP}
mkdir -p "$OUT"

if [ -n "$(git status --porcelain)" ]; then echo "มีไฟล์ที่ยังไม่ commit — commit ก่อนแพ็ก" >&2; exit 1; fi
git archive --format=tar.gz -o "$OUT/code.tar.gz" HEAD
docker exec iso-postgres pg_dump -U iso --no-owner iso > "$OUT/db.sql"
tar --exclude='data/backups' -czf "$OUT/data.tar.gz" data
( cd "$OUT" && sha256sum code.tar.gz db.sql data.tar.gz > SHA256SUMS )
ls -lh "$OUT"
echo "แพ็กเสร็จ: $OUT"
