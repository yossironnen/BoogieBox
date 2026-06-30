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
REM If no supported Python is installed, add one:
REM   winget install Python.Python.3.11

setlocal

set "SCRIPT_DIR=%~dp0"
set "WHEELS_DIR=%SCRIPT_DIR%wheels"
set "BUILD_VENV=%TEMP%\boogiebox-wheel-build"
set HAS_WHEEL=0

if not exist "%WHEELS_DIR%" mkdir "%WHEELS_DIR%"

for %%V in (3.12 3.11 3.10) do (
    py -%%V --version >nul 2>nul
    if not errorlevel 1 (
        echo [py %%V] Setting up build environment...
        if exist "%BUILD_VENV%" rmdir /s /q "%BUILD_VENV%"
        py -%%V -m venv "%BUILD_VENV%" >nul 2>&1
        if not errorlevel 1 (
            "%BUILD_VENV%\Scripts\pip.exe" install -q "Cython>=3" "numpy>=1.24" "setuptools" "wheel" "torch"
            if not errorlevel 1 (
                echo [py %%V] Building madmom wheel...
                "%BUILD_VENV%\Scripts\pip.exe" wheel "madmom>=0.16.1" --no-deps --no-build-isolation -w "%WHEELS_DIR%"
                if not errorlevel 1 (
                    echo [py %%V] madmom OK
                    set HAS_WHEEL=1
                ) else (
                    echo [py %%V] WARN: madmom wheel build failed
                )
                echo [py %%V] Building diffq wheel...
                "%BUILD_VENV%\Scripts\pip.exe" wheel "diffq>=0.2.4" --no-deps --no-build-isolation -w "%WHEELS_DIR%"
                if not errorlevel 1 (
                    echo [py %%V] diffq OK
                ) else (
                    echo [py %%V] WARN: diffq wheel build failed
                )
            ) else (
                echo [py %%V] WARN: failed to install build dependencies
            )
            rmdir /s /q "%BUILD_VENV%" >nul 2>&1
        ) else (
            echo [py %%V] WARN: could not create temp venv
        )
    ) else (
        echo [py %%V] Not installed, skipping.
    )
)

if "%HAS_WHEEL%"=="1" goto :success

echo.
echo [ERROR] No madmom wheels built.
echo   A Python 3.10, 3.11, or 3.12 installation is required (3.13+ is incompatible).
echo   Install Python 3.11:   winget install Python.Python.3.11
exit /b 1

:success
echo.
echo Wheels written to: %WHEELS_DIR%
exit /b 0
