@echo off
REM Runs the Seed Random Playback History Windows command workflow.
setlocal

if "%~1"=="" (
  echo Usage: %~nx0 ^<path-to-boogiebox.db^>
  exit /b 1
)

set "DB_PATH=%~1"

if not exist "%DB_PATH%" (
  echo Database not found: %DB_PATH%
  exit /b 1
)

node --experimental-sqlite "%~dp0scripts\seed-random-playback-history.mjs" "%DB_PATH%"
exit /b %ERRORLEVEL%
