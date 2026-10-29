@echo off
REM Builds pre-compiled wheels for the BoogieMix bootstrapper.
REM Run this on a machine with Microsoft C++ Build Tools installed (same machine that runs build-server-rust.bat).
REM Output .whl files land in wheels\ and are bundled with the installer via xcopy in build-server-rust.bat.
REM End-user machines then install from the bundled wheel without needing MSVC.
REM
REM Packages with C extensions that require MSVC on Windows:
REM   - madmom 0.16.1: compatible with Python 3.10-3.12 only
REM       Python 3.13+: _PyLong_AsByteArray signature changed, breaks compiled C
REM       Python 3.11 with Cython<3: generates longintrepr.h references, breaks on some installs
REM       Fix: use Cython>=3 which generates clean C code without internal CPython headers.
REM   - diffq: required by mdx_extra_q Demucs model, has a C bitpack extension
REM
REM Only ONE Python version is built: the newest of 3.12/3.11/3.10 that succeeds.
REM bootstrap_env.ps1 installs exactly the version the bundled madmom wheel targets,
REM so shipping wheels for several versions would let an older one win.
REM If no supported Python is installed, add one:
REM   winget install Python.Python.3.12

setlocal

set "SCRIPT_DIR=%~dp0"
set "WHEELS_DIR=%SCRIPT_DIR%wheels"
set "BUILD_VENV=%TEMP%\boogiebox-wheel-build"

if not exist "%WHEELS_DIR%" mkdir "%WHEELS_DIR%"
del /q "%WHEELS_DIR%\madmom-*.whl" "%WHEELS_DIR%\diffq-*.whl" >nul 2>&1

for %%V in (3.12 3.11 3.10) do (
    call :build %%V
    if not errorlevel 1 goto :success
)

echo.
echo [ERROR] No madmom wheels built.
echo   A Python 3.10, 3.11, or 3.12 installation is required (3.13+ is incompatible).
echo   Install Python 3.12:   winget install Python.Python.3.12
exit /b 1

:build
py -%1 --version >nul 2>nul
if errorlevel 1 (
    echo [py %1] Not installed, skipping.
    exit /b 1
)
echo [py %1] Setting up build environment...
if exist "%BUILD_VENV%" rmdir /s /q "%BUILD_VENV%"
py -%1 -m venv "%BUILD_VENV%" >nul 2>&1
if errorlevel 1 (
    echo [py %1] WARN: could not create temp venv
    exit /b 1
)
"%BUILD_VENV%\Scripts\pip.exe" install -q "Cython>=3" "numpy>=1.24" "setuptools" "wheel" "torch"
if errorlevel 1 (
    echo [py %1] WARN: failed to install build dependencies
    rmdir /s /q "%BUILD_VENV%" >nul 2>&1
    exit /b 1
)
echo [py %1] Building madmom wheel...
"%BUILD_VENV%\Scripts\pip.exe" wheel "madmom>=0.16.1" --no-deps --no-build-isolation -w "%WHEELS_DIR%"
if errorlevel 1 (
    echo [py %1] WARN: madmom wheel build failed
    rmdir /s /q "%BUILD_VENV%" >nul 2>&1
    exit /b 1
)
echo [py %1] madmom OK
echo [py %1] Building diffq wheel...
"%BUILD_VENV%\Scripts\pip.exe" wheel "diffq>=0.2.4" --no-deps --no-build-isolation -w "%WHEELS_DIR%"
if errorlevel 1 (
    echo [py %1] WARN: diffq wheel build failed
) else (
    echo [py %1] diffq OK
)
rmdir /s /q "%BUILD_VENV%" >nul 2>&1
exit /b 0

:success
echo.
echo Wheels written to: %WHEELS_DIR%
exit /b 0
