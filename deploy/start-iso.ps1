# start-iso.ps1 — เริ่มทุกบริการของ iso.kgarden.co.th (เรียกโดย Task Scheduler ตอน logon)
# บริการ: PostgreSQL (Docker) -> Go converter -> Rust API -> Bun static/proxy -> Cloudflare Tunnel
$ErrorActionPreference = 'SilentlyContinue'
$root = 'C:\Users\aidev\OneDrive\Documents\MasterData\iso'
$bun  = 'C:\Users\aidev\.bun\bin\bun.exe'
$cfd  = 'C:\Program Files (x86)\cloudflared\cloudflared.exe'
$log  = Join-Path $root 'deploy\logs'
New-Item -ItemType Directory -Force -Path $log | Out-Null

function Log($m) { "$([DateTime]::Now.ToString('s'))  $m" | Out-File -Append -Encoding utf8 (Join-Path $log 'startup.log') }
Log '=== start-iso begin ==='

# 1) รอ Docker Desktop พร้อม (สูงสุด ~6 นาที) แล้วเปิด Postgres
$dd = "$env:LOCALAPPDATA\Programs\DockerDesktop\Docker Desktop.exe"
if (-not (Test-Path $dd)) { $dd = 'C:\Program Files\Docker\Docker\Docker Desktop.exe' }
if (Test-Path $dd) { Start-Process $dd }
$deadline = (Get-Date).AddMinutes(6)
do { docker info *> $null; $ok = $?; if (-not $ok) { Start-Sleep 6 } } until ($ok -or (Get-Date) -gt $deadline)
Log "docker ready=$ok"
Push-Location $root
docker compose up -d *> $null
# รอ Postgres รับการเชื่อมต่อ
$deadline = (Get-Date).AddMinutes(2)
do { $r = (docker exec iso-postgres pg_isready -U iso -d iso 2>$null); Start-Sleep 2 } until (($r -match 'accepting') -or (Get-Date) -gt $deadline)
Log "postgres ready=$($r -match 'accepting')"
Pop-Location

# 2) Go converter (:8081)
$env:CONVERTER_PORT = '8081'
Start-Process -WindowStyle Hidden -WorkingDirectory (Join-Path $root 'converter') `
  -FilePath (Join-Path $root 'converter\iso-converter.exe') `
  -RedirectStandardOutput (Join-Path $log 'converter.log') -RedirectStandardError (Join-Path $log 'converter.err.log')

# 3) Rust API (:8080) — release build
$env:API_PORT = '8080'
$env:CONVERTER_PORT = '8081'
$env:DOCS_ROOT = '../data'
$env:DATABASE_URL = 'host=localhost port=5432 user=iso password=iso dbname=iso'
Start-Process -WindowStyle Hidden -WorkingDirectory (Join-Path $root 'api') `
  -FilePath (Join-Path $root 'api\target\release\iso-api.exe') `
  -RedirectStandardOutput (Join-Path $log 'api.log') -RedirectStandardError (Join-Path $log 'api.err.log')

# 4) Bun static + proxy (:8090)
$env:PORT = '8090'
Start-Process -WindowStyle Hidden -WorkingDirectory $root `
  -FilePath $bun -ArgumentList 'serve.js' `
  -RedirectStandardOutput (Join-Path $log 'serve.log') -RedirectStandardError (Join-Path $log 'serve.err.log')

# 5) Cloudflare Tunnel (iso-kgarden -> iso.kgarden.co.th)
Start-Process -WindowStyle Hidden `
  -FilePath $cfd -ArgumentList 'tunnel','--config','C:\Users\aidev\.cloudflared\iso-config.yml','run','iso-kgarden' `
  -RedirectStandardOutput (Join-Path $log 'tunnel.log') -RedirectStandardError (Join-Path $log 'tunnel.err.log')

Log '=== start-iso launched all services ==='
