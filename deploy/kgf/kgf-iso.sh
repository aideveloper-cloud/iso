#!/usr/bin/env bash
# =============================================================
#  ISO (iso.kgarden.co.th) บน ServerKG — แนวเดียวกับ kgf-deploy.sh แต่หลาย container
#  รันจาก Git Bash บนเครื่อง Windows ที่ root ของ repo หรือที่ไหนก็ได้ (สคริปต์หา root เอง)
#
#  kgf-iso.sh setup      ครั้งแรก: สร้าง DB/user iso บน kgf-postgres + .env + ฟอนต์ + credentials tunnel
#  kgf-iso.sh deploy     build ARM64 (api/converter/web) → ส่งขึ้น server → รัน → health check
#  kgf-iso.sh data       คัดลอกฐานข้อมูล + โฟลเดอร์ data/ จาก Windows ทับของ server (หยุด api ระหว่างทำ)
#  kgf-iso.sh cutover    ปิดเว็บบน Windows → data ล่าสุด → เปิด tunnel บน Mac (iso.kgarden.co.th ย้ายมา)
#  kgf-iso.sh back       ย้อนกลับ: ปิด tunnel บน Mac → เปิดบริการบน Windows ตามเดิม
#  kgf-iso.sh status | logs [service] | rollback | restart
# =============================================================
set -euo pipefail
SERVER=${KGF_SERVER:-server@192.168.1.15}
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
HERE="$ROOT/deploy/kgf"
DIR='~/server/apps/iso'
SERVICES=(api converter web)
FONTS_DIR=/c/Windows/Fonts
CF_DIR=~/.cloudflared
CF_CONF="$CF_DIR/iso-config.yml"

# บังคับ docker context ของ Colima — บน Mac มี Docker Desktop ด้วย ถ้า context ปัจจุบันถูกสลับไป จะ deploy ผิดที่ (ไม่มี network kgf, พอร์ตชน)
DCTX=${KGF_DOCKER_CONTEXT:-colima}
R(){ ssh -o ConnectTimeout=5 "$SERVER" "export PATH=/opt/homebrew/bin:/usr/local/bin:\$PATH DOCKER_CONTEXT=$DCTX; $1"; }
RI(){ ssh -o ConnectTimeout=5 "$SERVER" "export PATH=/opt/homebrew/bin:/usr/local/bin:\$PATH DOCKER_CONTEXT=$DCTX; $1"; }   # รับ stdin
COMPOSE="cd $DIR && docker compose -p iso"
# เขียนข้อมูลจาก stdin (tar.gz) ลง named volume โดยล้างของเดิมก่อน
to_volume(){ RI "docker run --rm -i -v iso_$1:/v alpine sh -c 'find /v -mindepth 1 -delete && tar xzf - -C /v'"; }
health(){ R "for i in \$(seq 1 30); do c=\$(curl -s http://127.0.0.1:8001/api/health); case \"\$c\" in *'\"db\":\"ok\"'*) echo \"\$c\"; exit 0;; esac; sleep 2; done; echo \"\${c:-ไม่ตอบ}\"; exit 1"; }
win_stop(){
  # ปิด watchdog/startup ก่อน ไม่งั้นมันเปิด cloudflared กลับมาภายใน 5 นาที แล้ว traffic จะแบ่งสองเครื่อง
  powershell.exe -NoProfile -Command "Disable-ScheduledTask -TaskName 'ISO-Kgarden-Watchdog' | Out-Null; Disable-ScheduledTask -TaskName 'ISO-Kgarden-Deploy' | Out-Null;
    Get-CimInstance Win32_Process -Filter \"Name='cloudflared.exe'\" | ? { \$_.CommandLine -match 'iso-config\.yml' } | % { Stop-Process -Id \$_.ProcessId -Force };
    Get-Process iso-api,iso-converter -ErrorAction SilentlyContinue | Stop-Process -Force;
    Get-CimInstance Win32_Process -Filter \"Name='bun.exe'\" | ? { \$_.CommandLine -match 'serve\.js' } | % { Stop-Process -Id \$_.ProcessId -Force }"
}

CMD=${1:-}; shift || true
case "$CMD" in
# -------------------------------------------------------------
setup)
  echo "== ตรวจ server =="
  R "docker network inspect kgf >/dev/null && docker exec kgf-postgres pg_isready -U kgf" || { echo "✖ ไม่พบ network kgf หรือ kgf-postgres"; exit 1; }
  R "mkdir -p $DIR && touch $DIR/.env && chmod 600 $DIR/.env"
  R "for v in data fonts cloudflared; do docker volume create iso_iso-\$v >/dev/null; done"

  echo "== DB/user iso บน kgf-postgres =="
  if R "grep -q '^DB_PASSWORD=' $DIR/.env"; then
    echo "  มี DB_PASSWORD อยู่แล้ว — ข้าม"
  else
    PW=$(head -c 32 /dev/urandom | base64 | tr -dc 'A-Za-z0-9' | head -c 32)
    R "docker exec kgf-postgres psql -U kgf -d postgres -v ON_ERROR_STOP=1 -c \"CREATE USER iso WITH PASSWORD '$PW';\" -c 'CREATE DATABASE iso OWNER iso;'"
    R "echo DB_PASSWORD=$PW >> $DIR/.env"
    unset PW
    echo "  ✔ สร้างแล้ว (รหัสเก็บใน $DIR/.env บน server เท่านั้น)"
  fi
  # ONLYOFFICE ลงนาม/ตรวจ JWT ร่วมกับ iso-api
  R "grep -q '^ONLYOFFICE_JWT_SECRET=' $DIR/.env || echo ONLYOFFICE_JWT_SECRET=\$(head -c 32 /dev/urandom | base64 | tr -dc A-Za-z0-9) >> $DIR/.env"

  echo "== ฟอนต์เอกสาร (ไทย + Office) → volume iso-fonts =="
  ( cd "$FONTS_DIR" && ls | grep -iE '^(angsana|browalia|cordia|upc[a-z]+|tahoma|leela|arial|times|calibri|cambria|cour[a-z]*\.ttf|micross|segoeui|symbol|wingding)' \
      | tar czf - -T - ) | to_volume iso-fonts
  echo "  ✔ ส่งฟอนต์แล้ว"

  echo "== credentials ของ tunnel iso-kgarden → volume iso-cloudflared =="
  TID=$(sed -n 's/^tunnel:[[:space:]]*//p' "$CF_CONF" | tr -d '\r')
  [ -f "$CF_DIR/$TID.json" ] || { echo "✖ ไม่พบ $CF_DIR/$TID.json"; exit 1; }
  TMP=$(mktemp -d); trap 'rm -rf "$TMP"' EXIT
  cp "$CF_DIR/$TID.json" "$TMP/"
  printf 'tunnel: %s\ncredentials-file: /etc/cloudflared/%s.json\ningress:\n  - hostname: iso.kgarden.co.th\n    service: http://iso-web:8090\n  - hostname: iso-office.kgarden.co.th\n    service: http://iso-office:80\n  - service: http_status:404\n' "$TID" "$TID" > "$TMP/config.yml"
  tar czf - -C "$TMP" . | to_volume iso-cloudflared
  echo "  ✔ เสร็จ — ต่อไป: kgf-iso.sh deploy แล้ว kgf-iso.sh data" ;;
# -------------------------------------------------------------
deploy)
  VER=$(date +%Y%m%d-%H%M%S)
  PG_MAJOR=$(R "docker exec kgf-postgres sh -c 'echo \$PG_MAJOR'" | tr -d '\r')
  echo "== 1/5 Build ARM64 ($VER, pg_dump $PG_MAJOR) =="
  for s in "${SERVICES[@]}"; do
    docker buildx build --platform linux/arm64 --load --build-arg PG_MAJOR="$PG_MAJOR" \
      -f "$HERE/Dockerfile.$s" -t kgf/iso-$s:$VER -t kgf/iso-$s:latest "$ROOT"
  done

  echo "== 2/5 ส่ง image ขึ้น server =="
  IMGS=(); for s in "${SERVICES[@]}"; do IMGS+=(kgf/iso-$s:$VER kgf/iso-$s:latest); done
  docker save "${IMGS[@]}" | gzip | RI "gunzip | docker load" | tail -3

  echo "== 3/5 ส่ง compose =="
  R "mkdir -p $DIR"
  RI "cat > $DIR/docker-compose.yml" < "$HERE/compose.yml"

  echo "== 4/5 รันบน server =="
  R "$COMPOSE up -d"
  for s in "${SERVICES[@]}"; do
    R "docker images kgf/iso-$s --format '{{.Tag}}' | grep -v latest | sort -r | tail -n +4 | xargs -I{} docker rmi kgf/iso-$s:{} >/dev/null 2>&1; true"
  done
  R "docker image prune -f >/dev/null"
  docker image prune -f >/dev/null

  echo "== 5/5 Health check =="
  if health; then echo "  ✔ http://${SERVER#*@}:8001  (เวอร์ชัน $VER)"; else echo "  ⚠ ดู log: kgf-iso.sh logs api"; exit 1; fi ;;
# -------------------------------------------------------------
data)
  docker exec iso-postgres pg_isready -U iso -d iso >/dev/null || { echo "✖ Postgres บน Windows ไม่ทำงาน"; exit 1; }
  echo "== หยุด api/converter บน server =="
  R "$COMPOSE stop api converter"

  echo "== ฐานข้อมูล: Windows iso-postgres → kgf-postgres/iso =="
  R "docker exec kgf-postgres psql -U kgf -d postgres -v ON_ERROR_STOP=1 -c 'DROP DATABASE IF EXISTS iso WITH (FORCE);' -c 'CREATE DATABASE iso OWNER iso;'"
  # SET ROLE iso -> ทุกตารางเป็นของ user iso (ไม่ใช่ kgf)
  { echo 'SET ROLE iso;'; docker exec iso-postgres pg_dump -U iso --no-owner --no-acl iso; } | gzip \
    | RI "gunzip | docker exec -i kgf-postgres psql -q -U kgf -d iso -v ON_ERROR_STOP=1 >/dev/null"
  R "docker exec kgf-postgres psql -U kgf -d iso -tAc \"select 'ตาราง ' || count(*) from pg_tables where schemaname='public'\""

  echo "== โฟลเดอร์ data/ → volume iso-data (ไม่รวม backups/pdfcache) =="
  tar czf - -C "$ROOT/data" --exclude=./backups --exclude=./pdfcache . | to_volume iso-data

  echo "== เปิด api/converter =="
  R "$COMPOSE up -d api converter"
  health ;;
# -------------------------------------------------------------
cutover)
  R "docker inspect iso-web >/dev/null 2>&1" || { echo "✖ ยังไม่ได้ deploy บน server"; exit 1; }
  read -r -p "ย้าย iso.kgarden.co.th ไป Mac — เว็บจะหยุด ~2-3 นาที พิมพ์ yes เพื่อยืนยัน: " ok
  [ "$ok" = yes ] || { echo "ยกเลิก"; exit 1; }
  echo "== ปิดบริการบน Windows (tunnel/api/converter/web + Task Scheduler) — Postgres บน Windows ยังเปิดไว้ให้ dump =="
  win_stop
  "$0" data
  echo "== เปิด tunnel บน Mac =="
  R "grep -q '^COMPOSE_PROFILES=' $DIR/.env || echo COMPOSE_PROFILES=tunnel >> $DIR/.env"   # deploy ครั้งต่อไปคง tunnel ไว้
  R "$COMPOSE up -d tunnel"
  for i in $(seq 1 20); do
    c=$(curl -s -m 5 https://iso.kgarden.co.th/api/health || true)
    case "$c" in *'"db":"ok"'*) echo "  ✔ $c"; echo "ย้ายเสร็จ — iso.kgarden.co.th รันบน Mac แล้ว"; exit 0;; esac; sleep 3
  done
  echo "  ⚠ โดเมนยังไม่ตอบ — ดู: kgf-iso.sh logs tunnel  (ย้อนกลับ: kgf-iso.sh back)"; exit 1 ;;
# -------------------------------------------------------------
back)
  read -r -p "ย้อน iso.kgarden.co.th กลับ Windows? ข้อมูลที่แก้บน Mac หลัง cutover จะไม่ตามมา พิมพ์ yes: " ok
  [ "$ok" = yes ] || { echo "ยกเลิก"; exit 1; }
  R "$COMPOSE stop tunnel; sed -i '' '/^COMPOSE_PROFILES=/d' $DIR/.env"
  powershell.exe -NoProfile -Command "Enable-ScheduledTask -TaskName 'ISO-Kgarden-Watchdog' | Out-Null; Enable-ScheduledTask -TaskName 'ISO-Kgarden-Deploy' | Out-Null; Start-ScheduledTask -TaskName ISO-Kgarden-Watchdog"
  echo "✔ เปิด watchdog แล้ว — บริการบน Windows จะกลับมาภายในไม่กี่วินาที" ;;
# -------------------------------------------------------------
status)
  R "docker ps -a --filter name=iso- --format 'table {{.Names}}\t{{.Status}}\t{{.Ports}}'"
  echo; echo "เวอร์ชันบน server:"; R "docker images kgf/iso-api --format '  {{.Tag}}  {{.CreatedSince}}' | grep -v latest"
  echo; R "curl -s http://127.0.0.1:8001/api/health"; echo ;;
logs)
  ssh -t "$SERVER" "export PATH=/opt/homebrew/bin:\$PATH; docker logs -f --tail 100 iso-${1:-api}" ;;
rollback)
  R "TAGS=\$(docker images kgf/iso-api --format '{{.Tag}}' | grep -v latest | sort -r)
     NEW=\$(echo \"\$TAGS\" | sed -n 1p); PREV=\$(echo \"\$TAGS\" | sed -n 2p)
     [ -z \"\$PREV\" ] && { echo '✖ ไม่มีเวอร์ชันก่อนหน้า'; exit 1; }
     for s in ${SERVICES[*]}; do docker tag kgf/iso-\$s:\$PREV kgf/iso-\$s:latest && docker rmi kgf/iso-\$s:\$NEW >/dev/null; done
     $COMPOSE up -d && echo \"✔ rollback: \$NEW → \$PREV\"" ;;
restart)
  R "$COMPOSE restart" ;;
*)
  sed -n '6,12p' "$0" | sed 's/^#  //' ;;
esac
