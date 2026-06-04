@echo off
REM Runs the Seed Random Ratings Windows command workflow.
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

node --experimental-sqlite "%~dp0scripts\seed-random-ratings.mjs" "%DB_PATH%"
exit /b %ERRORLEVEL%
