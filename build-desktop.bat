@echo off
REM Runs the Build Desktop Windows command workflow.
setlocal enabledelayedexpansion

set SKIP_INSTALLER=0
set "ROOT_DIR=%~dp0"
set "PROGRAM_FILES_X86=%ProgramFiles(x86)%"

:ParseArgs
if "%~1"=="" goto ArgsDone
if /i "%~1"=="--no-installer" set SKIP_INSTALLER=1
shift
goto ParseArgs
:ArgsDone

echo ============================================================
echo  BoogieBox Desktop Client Build
echo ============================================================
echo.

:: -- Prerequisite: Node.js ------------------------------------
where node >nul 2>&1
if errorlevel 1 (
    echo [ERROR] Node.js not found. Install Node.js 22+ from https://nodejs.org
    exit /b 1
)
for /f "tokens=1 delims=v" %%v in ('node -v 2^>nul') do set NODE_VER_RAW=%%v
for /f "tokens=1 delims=." %%M in ('node -v 2^>nul') do (
    set NODE_MAJOR=%%M
    set NODE_MAJOR=!NODE_MAJOR:v=!
)
if !NODE_MAJOR! LSS 22 (
    echo [ERROR] Node.js 22+ required. Installed version:
    node -v
    exit /b 1
)
echo [OK] Node.js
node -v

echo.
echo Running local quality checks...
cd /d "%~dp0"
call npm run lint
if errorlevel 1 ( echo [ERROR] Lint and typecheck failed & exit /b 1 )
call npm run security:semgrep
if errorlevel 1 ( echo [ERROR] Semgrep security scan failed & exit /b 1 )

:: -- Prerequisite: Rust / cargo -------------------------------
where cargo >nul 2>&1
if errorlevel 1 (
    echo [ERROR] Rust/cargo not found. Install from https://rustup.rs
    exit /b 1
)
echo [OK] Rust
cargo --version

:: -- Prerequisite: WebView2 Runtime ---------------------------
reg query "HKLM\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}" >nul 2>&1
if errorlevel 1 (
    reg query "HKCU\Software\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}" ^>nul 2^>^&1
    if errorlevel 1 (
        echo [WARN] WebView2 Runtime not detected in registry.
        echo        If the desktop app fails to launch, install WebView2 from:
        echo        https://developer.microsoft.com/en-us/microsoft-edge/webview2/
    ) else (
        echo [OK] WebView2 Runtime ^(user-installed^)
    )
) else (
    echo [OK] WebView2 Runtime ^(machine-wide^)
)

:: -- Prerequisite: MSVC / link.exe ----------------------------
where link.exe >nul 2>&1
if errorlevel 1 (
    set "MSVC_LINK="
    set "VSWHERE=!PROGRAM_FILES_X86!\Microsoft Visual Studio\Installer\vswhere.exe"
    if exist "!VSWHERE!" (
        for /f "usebackq delims=" %%i in (`"!VSWHERE!" -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -find VC\Tools\MSVC\**\bin\Hostx64\x64\link.exe 2^>nul`) do (
            if not defined MSVC_LINK set "MSVC_LINK=%%i"
        )
    )
    if not defined MSVC_LINK (
        for /d %%v in ("%ProgramFiles%\Microsoft Visual Studio\2022\*\VC\Tools\MSVC\*") do (
            if exist "%%~fv\bin\Hostx64\x64\link.exe" if not defined MSVC_LINK set "MSVC_LINK=%%~fv\bin\Hostx64\x64\link.exe"
        )
    )
    if defined MSVC_LINK (
        echo [OK] MSVC linker detected
    ) else (
        echo [WARN] MSVC linker not detected. Make sure Microsoft C++ Build Tools are installed.
        echo        The Rust build may fail without it.
    )
) else (
    echo [OK] MSVC linker on PATH
)

echo.
echo -- Step 1: Install desktop npm dependencies -----------------
cd /d "%~dp0desktop"
call npm install --prefer-offline
if errorlevel 1 ( echo [ERROR] npm install failed & exit /b 1 )

echo.
echo -- Step 2: Generate icons (if not already present) ----------
if not exist "src-tauri\icons\icon.ico" (
    echo Generating placeholder icons...
    powershell -ExecutionPolicy Bypass -File src-tauri\generate-icons.ps1
    if errorlevel 1 ( echo [WARN] Icon generation failed - build may still succeed with existing icons )
) else (
    echo [OK] Icons already present
)

echo.
echo -- Step 3: Rust check ---------------------------------------
call npm run rust:check
if errorlevel 1 ( echo [ERROR] Rust check failed & exit /b 1 )

echo.
echo -- Step 4: Build Tauri desktop app --------------------------
if "%SKIP_INSTALLER%"=="1" (
    call npm run build -- --no-bundle
) else (
    call npm run build
)
if errorlevel 1 ( echo [ERROR] Tauri build failed & exit /b 1 )

echo.
echo -- Step 5: Copy artifacts -----------------------------------

for /f "usebackq delims=" %%v in (`node -p "require('./package.json').version"`) do set APP_VERSION=%%v
if not defined APP_VERSION (
    echo [ERROR] Could not read desktop package version
    exit /b 1
)
echo Current desktop version: %APP_VERSION%

set BUNDLE_DIR=%ROOT_DIR%desktop\src-tauri\target\release\bundle
if "%SKIP_INSTALLER%"=="1" (
    set RELEASE_NAME=boogiebox-desktop-%APP_VERSION%-win
    set RELEASE_DIR=%ROOT_DIR%Releases\!RELEASE_NAME!
    set DESKTOP_EXE=%ROOT_DIR%desktop\src-tauri\target\release\boogiebox-desktop.exe
    if not exist "!DESKTOP_EXE!" (
        echo [ERROR] Tauri build did not produce !DESKTOP_EXE!
        exit /b 1
    )
    if not exist "%ROOT_DIR%Releases" mkdir "%ROOT_DIR%Releases"
    if exist "!RELEASE_DIR!" rmdir /s /q "!RELEASE_DIR!"
    mkdir "!RELEASE_DIR!"
    copy /y "!DESKTOP_EXE!" "!RELEASE_DIR!\boogiebox-desktop.exe" >nul
    if errorlevel 1 ( echo [ERROR] Failed to copy desktop executable & exit /b 1 )
    if exist "%ROOT_DIR%desktop\src-tauri\target\release\resources" (
        xcopy /e /q /y "%ROOT_DIR%desktop\src-tauri\target\release\resources" "!RELEASE_DIR!\resources\" >nul
        if errorlevel 1 ( echo [ERROR] Failed to copy desktop resources & exit /b 1 )
    )
    (
        echo @echo off
        echo cd /d "%%~dp0"
        echo boogiebox-desktop.exe
    ) > "!RELEASE_DIR!\start.bat"
    (
        echo # BoogieBox Desktop %APP_VERSION%
        echo.
        echo BoogieBox desktop client package for Windows.
        echo.
        echo Run start.bat or boogiebox-desktop.exe.
    ) > "!RELEASE_DIR!\README.md"
    echo [OK] Desktop release folder copied to Releases\!RELEASE_NAME!\
) else (
    set RELEASE_DIR=%ROOT_DIR%Releases\desktop
    if not exist "!RELEASE_DIR!" mkdir "!RELEASE_DIR!"
    del /q "!RELEASE_DIR!\*.exe" >nul 2>&1
    del /q "!RELEASE_DIR!\*.msi" >nul 2>&1

    :: Copy installer/bundle output to Releases/desktop/
    if exist "%BUNDLE_DIR%\nsis" (
        xcopy /y /q "%BUNDLE_DIR%\nsis\*%APP_VERSION%*.exe" "!RELEASE_DIR!\" >nul 2>&1
        if errorlevel 1 ( echo [ERROR] Current-version NSIS installer not found & exit /b 1 )
        echo [OK] NSIS installer copied to Releases\desktop\
    )
    if exist "%BUNDLE_DIR%\msi" (
        xcopy /y /q "%BUNDLE_DIR%\msi\*%APP_VERSION%*.msi" "!RELEASE_DIR!\" >nul 2>&1
        if errorlevel 1 ( echo [ERROR] Current-version MSI installer not found & exit /b 1 )
        echo [OK] MSI installer copied to Releases\desktop\
    )
)

echo.
echo ============================================================
echo  BoogieBox Desktop build complete!
if "%SKIP_INSTALLER%"=="1" (
    echo  Artifacts: Releases\!RELEASE_NAME!\
) else (
    echo  Artifacts: Releases\desktop\
)
echo ============================================================
cd /d "%~dp0"
