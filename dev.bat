@echo off
REM Runs the Dev Windows command workflow.
setlocal

set "ROOT_DIR=%~dp0"
set "SERVER_PORT=3001"

set /p APP_VERSION=<"%ROOT_DIR%VERSION"
set "APP_VERSION=%APP_VERSION: =%"

if not defined APP_VERSION (
  echo [ERROR] Could not read version from VERSION file.
  exit /b 1
)

set "SERVER_RELEASE_DIR=%ROOT_DIR%Releases\boogiebox-%APP_VERSION%-win-rs"
set "SERVER_EXE=%SERVER_RELEASE_DIR%\boogiebox-server.exe"

if not exist "%SERVER_EXE%" (
  for /f "delims=" %%D in ('dir /b /ad /o-n "%ROOT_DIR%Releases\boogiebox-*-win-rs" 2^>nul') do (
    if not defined FALLBACK_SERVER_EXE (
      if exist "%ROOT_DIR%Releases\%%D\boogiebox-server.exe" (
        set "SERVER_RELEASE_DIR=%ROOT_DIR%Releases\%%D"
        set "SERVER_EXE=%ROOT_DIR%Releases\%%D\boogiebox-server.exe"
        set "FALLBACK_SERVER_EXE=1"
      )
    )
  )
)

if not exist "%SERVER_EXE%" (
  echo.
  echo [ERROR] Missing Rust standalone server EXE. Expected:
  echo         %ROOT_DIR%Releases\boogiebox-%APP_VERSION%-win-rs\boogiebox-server.exe
  echo.
  echo         Run build-server-rust.bat --no-installer, then rerun dev.bat.
  echo.
  exit /b 1
)

if defined FALLBACK_SERVER_EXE (
  echo.
  echo [WARN] Matching %APP_VERSION% server EXE was not found.
  echo        Using latest available release instead:
  echo        %SERVER_EXE%
)

echo.
echo  Starting BoogieBox in development mode...
echo  Server EXE: %SERVER_EXE%
echo  Server: http://localhost:%SERVER_PORT%
echo  Packaged client: http://localhost:%SERVER_PORT%
echo.
echo  Starting server in a separate window.
echo.

:: Start standalone server EXE in a new window.
:: dev.config values are loaded into DB settings only when this flag is set.
set "PORT=%SERVER_PORT%"
set "NODE_ENV=development"
set "BOOGIEBOX_LOAD_DEV_CONFIG=1"
set "BOOGIEBOX_DEV_CONFIG_PATH=%ROOT_DIR%dev.config"
set "BOOGIEBOX_DEBUG_LOG_PATH=%ROOT_DIR%logs\debug.log"
start "BoogieBox Server EXE Debug" /D "%SERVER_RELEASE_DIR%" cmd /k ""%SERVER_EXE%""

:: Wait a moment for server to initialize
timeout /t 3 /nobreak >nul

where node >nul 2>nul
if errorlevel 1 (
  echo  Node.js was not found, so Vite cannot be started.
  echo  Open the packaged development server at:
  echo  http://localhost:%SERVER_PORT%
  echo.
  echo  To run the live Vite client, install Node.js LTS and rerun dev.bat.
  echo.
  pause
  exit /b 0
)

where npx >nul 2>nul
if errorlevel 1 (
  echo  npx was not found, so Vite cannot be started.
  echo  Open the packaged development server at:
  echo  http://localhost:%SERVER_PORT%
  echo.
  echo  To run the live Vite client, reinstall Node.js with npm and rerun dev.bat.
  echo.
  pause
  exit /b 0
)

echo  Client: http://localhost:3000
echo.
echo  Starting Vite client in a separate window.
echo  Close both windows to stop.
echo.

start "BoogieBox Client" /D "%ROOT_DIR%client" cmd /k "npx vite --port 3000"

echo  Both windows opened.
echo  Server logs: BoogieBox Server window
echo  Client logs: BoogieBox Client window
echo.
pause
