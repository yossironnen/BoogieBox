@echo off
REM Runs the Setup Dev Windows command workflow.
setlocal EnableExtensions EnableDelayedExpansion

pushd "%~dp0" >nul

echo BoogieBox development environment setup
echo.
echo Press Enter to accept the default answer for each prompt.
echo.

call :require_windows

call :ensure_git
if errorlevel 1 exit /b 1

call :ensure_vscode
if errorlevel 1 exit /b 1

call :ask "Install or update Node.js 22+ with winget" Y
if errorlevel 2 (
    call :install_winget "OpenJS.NodeJS.LTS" "Node.js LTS"
    if errorlevel 1 exit /b 1
)

call :ask "Install or update Rust via rustup with winget" Y
if errorlevel 2 (
    call :install_winget "Rustlang.Rustup" "Rustup"
    if errorlevel 1 exit /b 1
)

call :ask "Install Microsoft C++ Build Tools with winget" Y
if errorlevel 2 (
    call :install_build_tools
    if errorlevel 1 exit /b 1
)

call :ask "Install WebView2 Runtime with winget" Y
if errorlevel 2 (
    call :install_winget "Microsoft.EdgeWebView2Runtime" "Microsoft Edge WebView2 Runtime"
    if errorlevel 1 exit /b 1
)

call :ask "Install Python 3.12 with winget" Y
if errorlevel 2 (
    call :install_winget "Python.Python.3.12" "Python 3.12"
    if errorlevel 1 exit /b 1
)

call :ask "Install Inno Setup 6 with winget" Y
if errorlevel 2 (
    call :install_winget "JRSoftware.InnoSetup" "Inno Setup 6"
    if errorlevel 1 exit /b 1
)

call :ask "Install Semgrep with Python pip" Y
if errorlevel 2 (
    call :install_semgrep
    if errorlevel 1 exit /b 1
)

call :ask "Configure Rust stable MSVC toolchain" Y
if errorlevel 2 (
    call :configure_rust
    if errorlevel 1 exit /b 1
)

call :ask "Install root, client, and desktop npm dependencies" Y
if errorlevel 2 (
    call :install_npm
    if errorlevel 1 exit /b 1
)

call :ask "Download pinned FFmpeg and FFprobe into tools\ffmpeg" Y
if errorlevel 2 (
    call :download_ffmpeg
    if errorlevel 1 exit /b 1
)

call :ask "Download WinSW service wrapper into tools\winsw" Y
if errorlevel 2 (
    call :download_winsw
    if errorlevel 1 exit /b 1
)

call :ask "Create dev.config template if missing" Y
if errorlevel 2 (
    call :create_dev_config
    if errorlevel 1 exit /b 1
)

call :ask "Set BOOGIEBOX_FFMPEG_DIR for this user to tools\ffmpeg" Y
if errorlevel 2 (
    call :set_ffmpeg_env
    if errorlevel 1 exit /b 1
)

call :ask "Verify installed development tools" Y
if errorlevel 2 (
    call :verify_tools
    if errorlevel 1 exit /b 1
)

echo.
echo Setup complete.
popd >nul
exit /b 0

:require_windows
if /i "%OS%"=="Windows_NT" exit /b 0
echo [ERROR] This setup script is Windows-only.
exit /b 1

:ask
set "QUESTION=%~1"
set "DEFAULT=%~2"
set "ANSWER="
set /p "ANSWER=%QUESTION%? [Y/n] "
if "%ANSWER%"=="" set "ANSWER=%DEFAULT%"
if /i "%ANSWER%"=="Y" exit /b 2
if /i "%ANSWER%"=="YES" exit /b 2
echo Skipped.
exit /b 0

:need_winget
where winget >nul 2>&1
if errorlevel 1 (
    echo [ERROR] winget is required for this step. Install App Installer from Microsoft Store, then rerun.
    exit /b 1
)
exit /b 0

:ensure_git
where git >nul 2>&1
if not errorlevel 1 (
    git --version 2>nul
    exit /b 0
)
call :ask "Install Git with winget" Y
if errorlevel 2 (
    call :install_winget "Git.Git" "Git"
    if errorlevel 1 exit /b 1
)
exit /b 0

:ensure_vscode
where code >nul 2>&1
if not errorlevel 1 (
    echo VS Code already installed:
    where code
    exit /b 0
)
call :ask "Install Visual Studio Code with winget" Y
if errorlevel 2 (
    call :install_winget "Microsoft.VisualStudioCode" "Visual Studio Code"
    if errorlevel 1 exit /b 1
)
exit /b 0

:install_winget
call :need_winget
if errorlevel 1 exit /b 1
echo Installing %~2...
winget install --id "%~1" --exact --source winget --accept-package-agreements --accept-source-agreements
if errorlevel 1 (
    echo [ERROR] Failed to install %~2.
    exit /b 1
)
exit /b 0

:install_build_tools
call :need_winget
if errorlevel 1 exit /b 1
echo Installing Microsoft C++ Build Tools...
winget install --id Microsoft.VisualStudio.2022.BuildTools --exact --source winget --accept-package-agreements --accept-source-agreements --override "--quiet --wait --norestart --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
if errorlevel 1 (
    echo [ERROR] Failed to install Microsoft C++ Build Tools.
    exit /b 1
)
exit /b 0

:install_semgrep
where python >nul 2>&1
if errorlevel 1 (
    echo [ERROR] Python is not available. Install Python first, then rerun this step.
    exit /b 1
)
python -m pip install --user --upgrade pip semgrep
if errorlevel 1 (
    echo [ERROR] Failed to install Semgrep.
    exit /b 1
)
exit /b 0

:configure_rust
where rustup >nul 2>&1
if errorlevel 1 (
    echo [ERROR] rustup is not available. Install Rust first, then rerun this step.
    exit /b 1
)
rustup default stable-msvc
if errorlevel 1 exit /b 1
rustup component add rustfmt clippy
if errorlevel 1 exit /b 1
exit /b 0

:install_npm
where npm >nul 2>&1
if errorlevel 1 (
    echo [ERROR] npm is not available. Install Node.js first, then open a new terminal and rerun.
    exit /b 1
)
call :ensure_node_version
if errorlevel 1 exit /b 1
call npm install
if errorlevel 1 exit /b 1
call npm --prefix client install
if errorlevel 1 exit /b 1
if exist desktop\package.json (
    call npm --prefix desktop install
    if errorlevel 1 exit /b 1
)
exit /b 0

:ensure_node_version
where node >nul 2>&1
if errorlevel 1 (
    echo [ERROR] Node.js is not available. Install Node.js 22+ first, then open a new terminal and rerun.
    exit /b 1
)
powershell -NoProfile -ExecutionPolicy Bypass -Command ^
  "$v=(node --version).TrimStart('v');" ^
  "$p=$v.Split('.');" ^
  "$major=[int]$p[0]; $minor=[int]$p[1];" ^
  "if (($major -eq 20 -and $minor -ge 19) -or ($major -eq 22 -and $minor -ge 12) -or ($major -ge 23)) { exit 0 }" ^
  "Write-Host ('[ERROR] Node.js ' + $v + ' is too old. BoogieBox dev dependencies now require Node 20.19+, 22.12+, or newer.'); exit 1"
if errorlevel 1 exit /b 1
exit /b 0

:download_ffmpeg
powershell -NoProfile -ExecutionPolicy Bypass -Command ^
  "$ErrorActionPreference='Stop';" ^
  "$dir=Join-Path $PWD 'tools\ffmpeg'; New-Item -ItemType Directory -Force $dir | Out-Null;" ^
  "$zip=Join-Path $env:TEMP 'boogiebox-ffmpeg-release-essentials.zip';" ^
  "$url='https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip';" ^
  "Invoke-WebRequest -Uri $url -OutFile $zip;" ^
  "$extract=Join-Path $env:TEMP ('boogiebox-ffmpeg-' + [guid]::NewGuid()); New-Item -ItemType Directory -Force $extract | Out-Null;" ^
  "Expand-Archive -LiteralPath $zip -DestinationPath $extract -Force;" ^
  "$root=Get-ChildItem -LiteralPath $extract -Directory | Select-Object -First 1;" ^
  "Copy-Item -LiteralPath (Join-Path $root.FullName 'bin\ffmpeg.exe') -Destination (Join-Path $dir 'ffmpeg.exe') -Force;" ^
  "Copy-Item -LiteralPath (Join-Path $root.FullName 'bin\ffprobe.exe') -Destination (Join-Path $dir 'ffprobe.exe') -Force;" ^
  "$license=Get-ChildItem -LiteralPath $root.FullName -File | Where-Object { $_.Name -match 'license|copying|notice' };" ^
  "$license | ForEach-Object { Copy-Item -LiteralPath $_.FullName -Destination (Join-Path $dir $_.Name) -Force };" ^
  "Remove-Item -LiteralPath $extract -Recurse -Force; Remove-Item -LiteralPath $zip -Force"
if errorlevel 1 (
    echo [ERROR] Failed to download FFmpeg.
    exit /b 1
)
exit /b 0

:download_winsw
powershell -NoProfile -ExecutionPolicy Bypass -Command ^
  "$ErrorActionPreference='Stop';" ^
  "$dir=Join-Path $PWD 'tools\winsw'; New-Item -ItemType Directory -Force $dir | Out-Null;" ^
  "$url='https://github.com/winsw/winsw/releases/download/v2.12.0/WinSW-x64.exe';" ^
  "Invoke-WebRequest -Uri $url -OutFile (Join-Path $dir 'boogiebox-service.exe')"
if errorlevel 1 (
    echo [ERROR] Failed to download WinSW.
    exit /b 1
)
exit /b 0

:create_dev_config
if exist dev.config (
    echo dev.config already exists.
    exit /b 0
)
powershell -NoProfile -ExecutionPolicy Bypass -Command ^
  "$json=[ordered]@{integrations=[ordered]@{lastfmKey='';discogsToken='';spotifyClientId='';spotifyClientSecret='';geniusClientId='';geniusClientSecret=''}} | ConvertTo-Json -Depth 4;" ^
  "Set-Content -LiteralPath 'dev.config' -Value $json -Encoding UTF8"
if errorlevel 1 (
    echo [ERROR] Failed to create dev.config.
    exit /b 1
)
exit /b 0

:set_ffmpeg_env
set "FFMPEG_DIR=%CD%\tools\ffmpeg"
setx BOOGIEBOX_FFMPEG_DIR "%FFMPEG_DIR%"
if errorlevel 1 exit /b 1
set "BOOGIEBOX_FFMPEG_DIR=%FFMPEG_DIR%"
exit /b 0

:verify_tools
echo.
echo Versions:
where node >nul 2>&1 && node --version
where npm >nul 2>&1 && npm --version
where git >nul 2>&1 && git --version
where code >nul 2>&1 && where code
where cargo >nul 2>&1 && cargo --version
where rustup >nul 2>&1 && rustup --version
where python >nul 2>&1 && python --version
where semgrep >nul 2>&1 && semgrep --version
if exist tools\ffmpeg\ffmpeg.exe tools\ffmpeg\ffmpeg.exe -version | findstr /b "ffmpeg version"
if exist tools\ffmpeg\ffprobe.exe tools\ffmpeg\ffprobe.exe -version | findstr /b "ffprobe version"
if exist tools\winsw\boogiebox-service.exe echo WinSW: tools\winsw\boogiebox-service.exe
exit /b 0
