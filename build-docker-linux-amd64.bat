@echo off
setlocal EnableExtensions EnableDelayedExpansion

cd /d "%~dp0"

set "IMAGE_TAG=boogiebox:linux-amd64-boogiemix-cpu"
set "PRIME_MODEL=1"
set "NO_CACHE="
set "OUTPUT_FLAG=--load"
set "PROGRESS_FLAG="
set "RUN_AFTER_BUILD=0"
set "CONTAINER_NAME=boogiebox-local"
set "HOST_PORT=3001"
set "DATA_VOLUME=boogiebox-data"
set "MUSIC_MOUNT="
set "STARTUP_DIAGNOSTICS=0"

:parse
if "%~1"=="" goto :run
if /I "%~1"=="--help" goto :usage
if /I "%~1"=="-h" goto :usage
if /I "%~1"=="--no-prime-model" (
  set "PRIME_MODEL=0"
  shift
  goto :parse
)
if /I "%~1"=="--tag" (
  if "%~2"=="" (
    echo [ERROR] --tag requires a value.
    exit /b 1
  )
  set "IMAGE_TAG=%~2"
  shift
  shift
  goto :parse
)
if /I "%~1"=="--no-cache" (
  set "NO_CACHE=--no-cache"
  shift
  goto :parse
)
if /I "%~1"=="--push" (
  set "OUTPUT_FLAG=--push"
  shift
  goto :parse
)
if /I "%~1"=="--no-load" (
  set "OUTPUT_FLAG="
  shift
  goto :parse
)
if /I "%~1"=="--progress" (
  if "%~2"=="" (
    echo [ERROR] --progress requires a value such as plain, auto, or tty.
    exit /b 1
  )
  set "PROGRESS_FLAG=--progress=%~2"
  shift
  shift
  goto :parse
)
if /I "%~1"=="--run" (
  set "RUN_AFTER_BUILD=1"
  shift
  goto :parse
)
if /I "%~1"=="--name" (
  if "%~2"=="" (
    echo [ERROR] --name requires a value.
    exit /b 1
  )
  set "CONTAINER_NAME=%~2"
  shift
  shift
  goto :parse
)
if /I "%~1"=="--host-port" (
  if "%~2"=="" (
    echo [ERROR] --host-port requires a value.
    exit /b 1
  )
  set "HOST_PORT=%~2"
  shift
  shift
  goto :parse
)
if /I "%~1"=="--data-volume" (
  if "%~2"=="" (
    echo [ERROR] --data-volume requires a value.
    exit /b 1
  )
  set "DATA_VOLUME=%~2"
  shift
  shift
  goto :parse
)
if /I "%~1"=="--music" (
  if "%~2"=="" (
    echo [ERROR] --music requires a host path.
    exit /b 1
  )
  set "MUSIC_MOUNT=%~2"
  shift
  shift
  goto :parse
)
if /I "%~1"=="--diagnostics" (
  set "STARTUP_DIAGNOSTICS=1"
  shift
  goto :parse
)
echo [ERROR] Unknown argument: %~1
goto :usage_error

:run
where docker >nul 2>nul
if errorlevel 1 (
  echo [ERROR] Docker is required. Install Docker Desktop or Docker Engine first.
  exit /b 1
)
if "%RUN_AFTER_BUILD%"=="1" (
  if /I "%OUTPUT_FLAG%"=="--push" (
    echo [ERROR] --run cannot be used with --push because the image may not be loaded locally.
    exit /b 1
  )
  if "%OUTPUT_FLAG%"=="" (
    echo [ERROR] --run cannot be used with --no-load because the image may not be loaded locally.
    exit /b 1
  )
)

echo.
echo Building BoogieBox Docker image for linux/amd64...
echo   Tag:            %IMAGE_TAG%
echo   Prime model:    %PRIME_MODEL%
echo   Output:         %OUTPUT_FLAG%
if "%RUN_AFTER_BUILD%"=="1" (
  echo   Run after build: yes
  echo   Container:       %CONTAINER_NAME%
  echo   Port:            %HOST_PORT%:3001
  echo   Data volume:     %DATA_VOLUME%:/var/lib/boogiebox
  if not "%MUSIC_MOUNT%"=="" echo   Music mount:     %MUSIC_MOUNT%:/music:ro
)
echo.

docker buildx build ^
  --platform linux/amd64 ^
  -f Dockerfile.linux-amd64 ^
  -t "%IMAGE_TAG%" ^
  --build-arg BOOGIEMIX_PRIME_MODEL=%PRIME_MODEL% ^
  %NO_CACHE% ^
  %OUTPUT_FLAG% ^
  %PROGRESS_FLAG% ^
  .

if errorlevel 1 exit /b %ERRORLEVEL%

if "%RUN_AFTER_BUILD%"=="1" (
  echo.
  echo Starting container with host port mapping...
  docker rm -f "%CONTAINER_NAME%" >nul 2>nul
  if "%MUSIC_MOUNT%"=="" (
    docker run -d --name "%CONTAINER_NAME%" -p "%HOST_PORT%:3001" -v "%DATA_VOLUME%:/var/lib/boogiebox" -e BOOGIEBOX_STARTUP_DIAGNOSTICS=%STARTUP_DIAGNOSTICS% "%IMAGE_TAG%"
  ) else (
    docker run -d --name "%CONTAINER_NAME%" -p "%HOST_PORT%:3001" -v "%DATA_VOLUME%:/var/lib/boogiebox" -v "%MUSIC_MOUNT%:/music:ro" -e BOOGIEBOX_STARTUP_DIAGNOSTICS=%STARTUP_DIAGNOSTICS% "%IMAGE_TAG%"
  )
  if errorlevel 1 exit /b %ERRORLEVEL%
  echo.
  echo BoogieBox is starting at http://localhost:%HOST_PORT%
)

exit /b 0

:usage
echo Usage: build-docker-linux-amd64.bat [options]
echo.
echo Options:
echo   --tag ^<tag^>           Image tag. Default: boogiebox:linux-amd64-boogiemix-cpu
echo   --no-prime-model      Do not prime the htdemucs model during build
echo   --no-cache            Build without Docker layer cache
echo   --push                Push build result instead of loading it locally
echo   --no-load             Do not pass --load or --push
echo   --progress ^<mode^>     Docker progress mode: auto, plain, or tty
echo   --run                 Start/recreate a container after a successful build
echo   --name ^<name^>         Container name for --run. Default: boogiebox-local
echo   --host-port ^<port^>    Host port for --run. Default: 3001
echo   --data-volume ^<name^>  Docker volume for /var/lib/boogiebox. Default: boogiebox-data
echo   --music ^<path^>        Optional host music path mounted read-only at /music
echo   --diagnostics         Enable startup diagnostics when using --run
echo   --help                Show this help
exit /b 0

:usage_error
call :usage
exit /b 1
