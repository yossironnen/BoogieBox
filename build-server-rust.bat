@echo off
REM Runs the Build Server Rust Windows command workflow.
setlocal EnableDelayedExpansion

set RUN_SMOKE=0
set SKIP_INSTALLER=0
set SKIP_TESTS=0

:ParseArgs
if "%~1"=="" goto ArgsDone
if /i "%~1"=="--smoke" set RUN_SMOKE=1
if /i "%~1"=="--no-installer" set SKIP_INSTALLER=1
if /i "%~1"=="--no-test" set SKIP_TESTS=1
shift
goto ParseArgs
:ArgsDone

echo.
echo  Building BoogieBox standalone server EXE (Rust)...
echo.

where cargo >nul 2>nul
IF ERRORLEVEL 1 (
  echo [ERROR] cargo ^(Rust toolchain^) is required on the build machine.
  echo         Install Rust from https://rustup.rs/ and ensure cargo is on PATH.
  exit /b 1
)

where node >nul 2>nul
IF ERRORLEVEL 1 (
  echo [ERROR] Node.js is required on the build machine to build the React client.
  echo         Install Node.js LTS from https://nodejs.org/
  exit /b 1
)

if "%SKIP_TESTS%"=="1" (
  echo  [1/8] Skipping quality checks ^(--no-test^).
) ELSE (
  echo  [1/8] Running quality checks...
  call npm run lint
  IF ERRORLEVEL 1 (echo [ERROR] Lint and typecheck failed & exit /b 1)
  call npm run security:semgrep
  IF ERRORLEVEL 1 (echo [ERROR] Semgrep security scan failed & exit /b 1)

  echo.
  echo  Running cargo audit...
  where cargo-audit >nul 2>nul
  IF ERRORLEVEL 1 (
    echo [INFO] cargo-audit not installed. Install with: cargo install cargo-audit
    echo        Continuing build without audit check...
  ) ELSE (
    pushd server-rs
    cargo audit
    IF ERRORLEVEL 1 (popd & echo [ERROR] Rust dependency advisory scan failed. Run 'npm run server-rs:audit' for details. & exit /b 1)
    popd
  )
)

echo.
echo  [2/8] Building React client...
call npm.cmd --prefix client run build
IF ERRORLEVEL 1 (echo [ERROR] Client build failed & exit /b 1)

echo.
echo  [3/8] Reading version...
for /f "tokens=2 delims='" %%V in ('findstr /r "APP_VERSION" "client\src\version.ts"') do set APP_VERSION=%%V
IF NOT DEFINED APP_VERSION (echo [ERROR] Could not read version from client\src\version.ts & exit /b 1)
echo  Version: %APP_VERSION%

echo.
echo  [4/8] Building Rust server release binary...
cargo build --release --manifest-path server-rs/Cargo.toml
IF ERRORLEVEL 1 (echo [ERROR] Rust server build failed & exit /b 1)

set RELEASE_NAME=boogiebox-%APP_VERSION%-win-rs
set DIST_DIR=Releases\%RELEASE_NAME%
set FFMPEG_CACHE_DIR=tools\ffmpeg
set FFMPEG_RELEASE_DIR=%DIST_DIR%\resources\ffmpeg
set WINSW_CACHE_EXE=tools\winsw\boogiebox-service.exe
set RUST_EXE=server-rs\target\release\boogiebox-server.exe

if not exist "%RUST_EXE%" (
  echo [ERROR] Rust build did not produce %RUST_EXE%
  exit /b 1
)

if not exist "%FFMPEG_CACHE_DIR%\ffmpeg.exe" (
  echo [ERROR] Missing %FFMPEG_CACHE_DIR%\ffmpeg.exe
  echo         Place ffmpeg.exe and ffprobe.exe in tools\ffmpeg\ before building.
  exit /b 1
)
if not exist "%FFMPEG_CACHE_DIR%\ffprobe.exe" (
  echo [ERROR] Missing %FFMPEG_CACHE_DIR%\ffprobe.exe
  exit /b 1
)

if not exist "%WINSW_CACHE_EXE%" (
  if "%SKIP_INSTALLER%"=="1" (
    echo [INFO] Missing %WINSW_CACHE_EXE% - WinSW wrapper will not be copied into this release-folder-only build.
  ) ELSE (
    echo [ERROR] Missing %WINSW_CACHE_EXE%
    echo         Download WinSW v2.12.0 x64, rename to boogiebox-service.exe, and place it in tools\winsw\.
    echo         Or run  build-server-rust.bat --no-installer  to skip the installer.
    exit /b 1
  )
)

echo.
echo  [5/8] Creating release folder: %DIST_DIR%\
if not exist "Releases" mkdir "Releases"
if exist "%DIST_DIR%" rmdir /s /q "%DIST_DIR%"
mkdir "%DIST_DIR%"
mkdir "%DIST_DIR%\client"
mkdir "%DIST_DIR%\resources"

echo.
echo  [6/8] Copying Rust server EXE...
copy /y "%RUST_EXE%" "%DIST_DIR%\boogiebox-server.exe" >nul
IF ERRORLEVEL 1 (echo [ERROR] Failed to copy Rust server EXE & exit /b 1)

echo  [7/8] Copying sidecars and assets...
xcopy /e /q "client\build" "%DIST_DIR%\client\build\" >nul
IF ERRORLEVEL 1 (echo [ERROR] Failed to copy client build & exit /b 1)

xcopy /e /q "%FFMPEG_CACHE_DIR%" "%FFMPEG_RELEASE_DIR%\" >nul
IF ERRORLEVEL 1 (echo [ERROR] Failed to copy FFmpeg binaries & exit /b 1)

if exist "%WINSW_CACHE_EXE%" (
  copy /y "%WINSW_CACHE_EXE%" "%DIST_DIR%\boogiebox-service.exe" >nul
  IF ERRORLEVEL 1 (echo [ERROR] Failed to copy WinSW service wrapper & exit /b 1)
  copy /y "installer\boogiebox-service.xml" "%DIST_DIR%\boogiebox-service.xml" >nul
  IF ERRORLEVEL 1 (echo [ERROR] Failed to copy WinSW service config & exit /b 1)
)

if exist "Services\boogiemix\python" (
  mkdir "%DIST_DIR%\resources\Services\boogiemix" >nul 2>nul
  xcopy /e /q "Services\boogiemix\python" "%DIST_DIR%\resources\Services\boogiemix\python\" >nul
  IF ERRORLEVEL 1 (echo [ERROR] Failed to copy BoogieMix Python assets & exit /b 1)
)
if exist "%DIST_DIR%\resources\Services\boogiemix\python\.venv" rmdir /s /q "%DIST_DIR%\resources\Services\boogiemix\python\.venv"

if exist "Services\boogiemix\ai" (
  mkdir "%DIST_DIR%\resources\Services\boogiemix" >nul 2>nul
  xcopy /e /q "Services\boogiemix\ai" "%DIST_DIR%\resources\Services\boogiemix\ai\" >nul
  IF ERRORLEVEL 1 (echo [ERROR] Failed to copy BoogieMix AI assets & exit /b 1)
)

echo.
echo  [8/8] Writing release metadata files...
(
  echo @echo off
  echo cd /d "%%~dp0"
  echo boogiebox-server.exe
) > "%DIST_DIR%\start.bat"

(
  echo # BoogieBox %APP_VERSION%
  echo.
  echo BoogieBox standalone server package for Windows.
  echo.
  echo ## Requirements
  echo.
  echo - Windows 10 or later
  echo - No Node.js or npm required
  echo - Bundled ffmpeg and ffprobe are included under resources\ffmpeg
  echo - Optional Windows service installation is available from the installer
  echo.
  echo ## Start
  echo.
  echo 1. Run **start.bat** or **boogiebox-server.exe**
  echo 2. Open **http://localhost:3001** in your browser
  echo 3. On first launch, follow the setup wizard to choose the database folder and libraries
  echo.
  echo ## Windows service
  echo.
  echo The installer defaults to a generated local .\BoogieBoxService account for local folders.
  echo For UNC library shares, choose the existing-account option and use a Windows account
  echo that exists on the BoogieBox machine and can authenticate to the share.
  echo Grant the service account access to UNC libraries and to any external database,
  echo cache, or mix-output folder selected during setup.
  echo.
  echo ## Notes
  echo.
  echo The React client is served from client\build beside the executable.
  echo FFmpeg is resolved from resources\ffmpeg before PATH.
  echo The first-run database locator is written to ProgramData for installed packages
  echo unless BOOGIEBOX_CONFIG_PATH or BOOGIEBOX_CONFIG_DIR is set.
  echo Experimental BoogieMix Python assets may be included under resources\Services,
  echo but Python runtime provisioning remains separate from the standard server package.
) > "%DIST_DIR%\README.md"

(
  echo # Third-Party Notices
  echo.
  echo ## Rust Runtime Crates
  echo.
  echo This BoogieBox release is built with Rust and links these third-party crates:
  echo   axum, tokio, rusqlite, serde, serde_json, reqwest, tower-http, tracing,
  echo   uuid, image, pbkdf2, sha2, hex, base64, rand, socket2, thiserror, time,
  echo   tokio-util, axum-extra, sha1, tower.
  echo.
  echo SQLite is bundled via rusqlite with the "bundled" feature. SQLite is public domain.
  echo See server-rs/Cargo.lock and each crate's upstream repository LICENSE file for terms.
  echo.
  echo ## FFmpeg
  echo.
  echo This BoogieBox release bundles FFmpeg and FFprobe under resources\ffmpeg.
  echo Keep the license and notice files from the selected FFmpeg distribution in
  echo tools\ffmpeg so they are copied into the release beside the binaries.
  echo Verify the selected FFmpeg build license terms before distributing this package.
  echo.
  echo ## WinSW
  echo.
  echo The optional service wrapper boogiebox-service.exe is WinSW v2.12.0 x64.
  echo WinSW is MIT licensed. See https://github.com/winsw/winsw for terms.
) > "%DIST_DIR%\THIRD_PARTY_NOTICES.md"

if "%SKIP_INSTALLER%"=="1" (
  echo.
  echo  Skipping Windows installer ^(--no-installer^).
) ELSE (
  echo.
  echo  Building Windows installer...
  call :ResolveIscc
  IF DEFINED ISCC_EXE (
    "!ISCC_EXE!" /DAppVersion=%APP_VERSION% "/DReleaseDir=%CD%\%DIST_DIR%" installer\boogiebox-server.iss
    IF ERRORLEVEL 1 (
      echo [WARN] Installer build failed - release folder %DIST_DIR%\ is still usable directly.
    ) ELSE (
      echo  Installer: Releases\boogiebox-%APP_VERSION%-win-setup.exe
    )
  ) ELSE (
    echo  [INFO] Inno Setup ^(iscc^) not found - skipping installer.
    echo         Install from https://jrsoftware.org/isinfo.php to enable .exe installer builds.
    echo         The release folder %DIST_DIR%\ is ready to use without an installer.
  )
)

echo.
echo  =================================================================
echo   Rust build complete: %DIST_DIR%\
echo   Run start.bat or boogiebox-server.exe
echo  =================================================================
echo.

if "%RUN_SMOKE%"=="1" call :SmokeTest
exit /b %ERRORLEVEL%

:ResolveIscc
set ISCC_EXE=
where iscc >nul 2>nul && set ISCC_EXE=iscc
IF NOT DEFINED ISCC_EXE (
  IF EXIST "!ProgramFiles(x86)!\Inno Setup 6\iscc.exe" set "ISCC_EXE=!ProgramFiles(x86)!\Inno Setup 6\iscc.exe"
)
IF NOT DEFINED ISCC_EXE (
  IF EXIST "!ProgramFiles!\Inno Setup 6\iscc.exe" set "ISCC_EXE=!ProgramFiles!\Inno Setup 6\iscc.exe"
)
exit /b 0

:SmokeTest
echo.
echo  Running Rust server smoke test...
set SMOKE_PORT=3199
set SMOKE_CONFIG_DIR=%DIST_DIR%\.smoke-config
if exist "%SMOKE_CONFIG_DIR%" rmdir /s /q "%SMOKE_CONFIG_DIR%"
mkdir "%SMOKE_CONFIG_DIR%"

powershell -NoProfile -ExecutionPolicy Bypass -Command "$ErrorActionPreference='Stop'; $env:PORT='%SMOKE_PORT%'; $env:BOOGIEBOX_CONFIG_DIR=(Resolve-Path '%SMOKE_CONFIG_DIR%').Path; $exe=(Resolve-Path '%DIST_DIR%\boogiebox-server.exe').Path; $work=(Resolve-Path '%DIST_DIR%').Path; $stdout=Join-Path $work 'smoke-stdout.log'; $stderr=Join-Path $work 'smoke-stderr.log'; Remove-Item $stdout,$stderr -Force -ErrorAction SilentlyContinue; $p=Start-Process -FilePath $exe -WorkingDirectory $work -WindowStyle Hidden -RedirectStandardOutput $stdout -RedirectStandardError $stderr -PassThru; try { $deadline=(Get-Date).AddSeconds(45); do { Start-Sleep -Milliseconds 500; if ($p.HasExited) { $out=(Get-Content $stdout,$stderr -Raw -ErrorAction SilentlyContinue) -join \"`n\"; throw \"boogiebox-server.exe exited before health check`n$out\" }; try { $status=Invoke-RestMethod -Uri 'http://127.0.0.1:%SMOKE_PORT%/api/system/status' -TimeoutSec 2; if ($status.server -eq 'boogiebox') { Write-Host 'Rust smoke test passed'; exit 0 } } catch { } } while ((Get-Date) -lt $deadline); $out=(Get-Content $stdout,$stderr -Raw -ErrorAction SilentlyContinue) -join \"`n\"; throw \"Timed out waiting for /api/system/status`n$out\" } finally { if ($p -and -not $p.HasExited) { Stop-Process -Id $p.Id -Force } }"
IF ERRORLEVEL 1 (
  echo [ERROR] Rust server smoke test failed.
  exit /b 1
)

echo  Rust server smoke test passed.
exit /b 0
