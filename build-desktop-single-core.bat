@echo off
setlocal
REM Work around compiler crashes by running the desktop build on logical CPU 16.
pushd "%~dp0"
if errorlevel 1 exit /b 1
REM One core can't feed the audio engine's real-time tests in parallel: run them one at a time.
set "RUST_TEST_THREADS=1"
start "" /b /wait /affinity 10000 /d "%~dp0" "%ComSpec%" /d /c call .\build-desktop.bat %*
set "BUILD_EXIT=%ERRORLEVEL%"
popd
exit /b %BUILD_EXIT%
