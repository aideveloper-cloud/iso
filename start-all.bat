@echo off
chcp 65001 >nul
title ISO v2 (Svelte+Bun+Rust+Go) launcher
cd /d "%~dp0"

REM --- ที่อยู่ของ toolchain (จาก winget) ---
set "GOBIN=C:\Program Files\Go\bin"
set "CARGO=%USERPROFILE%\.cargo\bin"
set "MINGW=%LOCALAPPDATA%\Microsoft\WinGet\Packages\BrechtSanders.WinLibs.POSIX.MSVCRT_Microsoft.Winget.Source_8wekyb3d8bbwe\mingw64\bin"
set "BUN=%LOCALAPPDATA%\Microsoft\WinGet\Packages\Oven-sh.Bun_Microsoft.Winget.Source_8wekyb3d8bbwe\bun-windows-x64\bun.exe"

echo.
echo   เริ่ม 3 บริการ: Go converter (8081) · Rust API (8080) · Svelte/Bun (5173)
echo   (แต่ละบริการเปิดหน้าต่างของตัวเอง — ปิดหน้าต่างเพื่อหยุด)
echo.

start "1) Go converter :8081"  cmd /k "cd /d %~dp0converter && set PATH=%GOBIN%;%PATH% && set CONVERTER_PORT=8081 && go run ."
start "2) Rust API :8080"      cmd /k "cd /d %~dp0api && set PATH=%MINGW%;%CARGO%;%PATH% && set API_PORT=8080 && set CONVERTER_PORT=8081 && set DOCS_ROOT=../data && cargo run"
start "3) Svelte+Bun :5173"    cmd /k "cd /d %~dp0frontend && \"%BUN%\" run dev"

echo   รอสักครู่แล้วเปิดเบราว์เซอร์...
timeout /t 6 >nul
start "" http://localhost:5173
