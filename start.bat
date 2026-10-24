@echo off
rem Start ResearchTrail. Usage: start.bat [project folder]
cd /d "%~dp0"
where cargo >nul 2>nul
if errorlevel 1 (
  echo Rust is not installed. Install it from https://rustup.rs, then run this again.
  pause
  exit /b 1
)
if not exist target\release\researchtrail.exe (
  echo Building ResearchTrail. The first time takes a few minutes...
)
cargo build --release
if errorlevel 1 (
  pause
  exit /b 1
)
target\release\researchtrail.exe %*
pause
