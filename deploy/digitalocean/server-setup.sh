#!/usr/bin/env bash
# รันบน Droplet (Ubuntu 24.04) ด้วย root: bash server-setup.sh /root/iso-migrate
# ติดตั้ง Docker → แตกโค้ด/ข้อมูลไป /opt/iso → กู้ฐานข้อมูล → เปิดทุกบริการ (ยกเว้น tunnel ถ้ายังไม่มี config)
set -euo pipefail
SRC=${1:?ระบุโฟลเดอร์ที่อัปโหลดจาก package.sh}
cd "$SRC" && sha256sum -c SHA256SUMS

if ! command -v docker >/dev/null; then curl -fsSL https://get.docker.com | sh; fi
timedatectl set-timezone Asia/Bangkok || true

mkdir -p /opt/iso && tar -xzf "$SRC/code.tar.gz" -C /opt/iso && tar -xzf "$SRC/data.tar.gz" -C /opt/iso
mkdir -p /opt/iso/data/backups
cd /opt/iso/deploy/digitalocean
if [ ! -f .env ]; then
  printf 'DB_PASSWORD=%s\nDATA_DIR=/opt/iso/data\n' "$(openssl rand -hex 24)" > .env && chmod 600 .env
fi

# กู้ฐานข้อมูลก่อนเปิด API (API สร้างตาราง/คอลัมน์ที่ขาดเองตอนเริ่ม)
docker compose up -d db
until docker exec iso-postgres pg_isready -U iso -d iso >/dev/null 2>&1; do sleep 2; done
if [ "$(docker exec iso-postgres psql -U iso -d iso -tAc "SELECT count(*) FROM information_schema.tables WHERE table_name='employees'")" = "0" ]; then
  docker exec -i iso-postgres psql -U iso -d iso -v ON_ERROR_STOP=1 -q < "$SRC/db.sql"
  echo "กู้ฐานข้อมูลแล้ว"
else
  echo "ฐานข้อมูลมีข้อมูลอยู่แล้ว — ข้ามการกู้"
fi

docker compose up -d --build db api converter web
sleep 5
docker exec iso-web bun -e "fetch('http://api:8085/api/health').then(r=>r.text()).then(console.log)"

if [ -f cloudflared/config.yml ]; then docker compose up -d tunnel; echo "เปิด tunnel แล้ว"
else echo "ยังไม่เปิด tunnel — วาง cloudflared/config.yml + <tunnel-id>.json แล้วสั่ง: docker compose up -d tunnel"; fi
