<#
.SYNOPSIS
Defines BoogieMix Python deep-analysis setup or worker behavior.
#>

param(
  [string]$PythonExe = "python",
  [string[]]$PythonExeArgs = @(),
  [string]$VenvPath = ".venv",
  [switch]$Auto,
  [switch]$Cuda,
  [switch]$CpuOnly,
  [switch]$PrimeDemucsModel,
  [switch]$Force
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $MyInvocation.MyCommand.Path
$venvFull = Join-Path $root $VenvPath

# If no explicit -PythonExe was given, resolve Python from known locations before
# falling back to PATH. Exit code 9009 means "not found" on Windows.
if ($PythonExe -eq "python") {
  $pyCmd     = Get-Command "py"     -ErrorAction SilentlyContinue
  $pythonCmd = Get-Command "python" -ErrorAction SilentlyContinue
  $candidateList = [System.Collections.Generic.List[string]]::new()
  if ($pyCmd)     { $candidateList.Add($pyCmd.Source) }
  foreach ($p in @(
    "$env:LOCALAPPDATA\Programs\Python\Python312\python.exe",
    "$env:LOCALAPPDATA\Programs\Python\Python311\python.exe",
    "$env:LOCALAPPDATA\Programs\Python\Python310\python.exe",
    "${env:ProgramFiles}\Python312\python.exe",
    "${env:ProgramFiles}\Python311\python.exe",
    "${env:ProgramFiles}\Python310\python.exe",
    "$env:USERPROFILE\miniconda3\python.exe",
    "$env:USERPROFILE\miniforge3\python.exe",
    "$env:USERPROFILE\anaconda3\python.exe"
  )) { $candidateList.Add($p) }
  if ($pythonCmd) { $candidateList.Add($pythonCmd.Source) }
  $candidates = $candidateList | Where-Object {
    $_ -and ($_ -notlike "*\WindowsApps\*") -and (Test-Path $_)
  } | Select-Object -First 1

  if (-not $candidates) {
    Write-Host "Python not found - downloading and installing Python 3.12..."
    $pyVersion   = "3.12.10"
    $pyInstaller = Join-Path $env:TEMP "python-$pyVersion-amd64.exe"
    $pyUrl       = "https://www.python.org/ftp/python/$pyVersion/python-$pyVersion-amd64.exe"
    Write-Host "  Downloading $pyUrl ..."
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    Invoke-WebRequest -Uri $pyUrl -OutFile $pyInstaller -UseBasicParsing
    Write-Host "  Installing Python $pyVersion (user install, no elevation required)..."
    $installArgs = @("/quiet", "InstallAllUsers=0", "PrependPath=1", "Include_launcher=1")
    $proc = Start-Process -FilePath $pyInstaller -ArgumentList $installArgs -Wait -PassThru
    Remove-Item $pyInstaller -ErrorAction SilentlyContinue
    if ($proc.ExitCode -ne 0) {
      throw "Python installer failed with exit code $($proc.ExitCode)."
    }
    # Reload PATH so the new install is visible in this session
    $userPath    = [System.Environment]::GetEnvironmentVariable("PATH", "User")
    $machinePath = [System.Environment]::GetEnvironmentVariable("PATH", "Machine")
    $env:PATH    = $userPath + ";" + $machinePath
    $pyCmd2     = Get-Command "py"     -ErrorAction SilentlyContinue
    $pythonCmd2 = Get-Command "python" -ErrorAction SilentlyContinue
    $cl2 = [System.Collections.Generic.List[string]]::new()
    if ($pyCmd2)     { $cl2.Add($pyCmd2.Source) }
    $cl2.Add("$env:LOCALAPPDATA\Programs\Python\Python312\python.exe")
    if ($pythonCmd2) { $cl2.Add($pythonCmd2.Source) }
    $candidates = $cl2 | Where-Object {
      $_ -and ($_ -notlike "*\WindowsApps\*") -and (Test-Path $_)
    } | Select-Object -First 1
    if (-not $candidates) {
      throw "Python was installed but still cannot be found. Please restart the terminal and re-run this script."
    }
  }

  # If we found py.exe, use it with -3 to invoke Python 3
  if ($candidates -like "*\py.exe") {
    $PythonExeArgs = @("-3") + $PythonExeArgs
  }
  $PythonExe = $candidates
  Write-Host "Using Python: $PythonExe"
}

$torchHome       = Join-Path $root "model-cache\torch"
$constraintsPath = Join-Path $root "constraints.txt"
$cudaIndexUrl    = "https://download.pytorch.org/whl/cu128"
$cpuIndexUrl     = "https://download.pytorch.org/whl/cpu"
$defaultDemucsModel = "htdemucs"

if ($CpuOnly -and $Cuda) {
  throw "Use only one of -CpuOnly or -Cuda."
}

if (-not $CpuOnly -and -not $Cuda -and -not $Auto) {
  $Auto = $true
}

function Invoke-NativeChecked {
  param(
    [Parameter(Mandatory = $true)]
    [string]$Command,
    [string[]]$Arguments = @()
  )

  & $Command @Arguments
  if ($LASTEXITCODE -ne 0) {
    throw "$Command failed with exit code $LASTEXITCODE"
  }
}

if ((Test-Path $venvFull) -and $Force) {
  Remove-Item -Recurse -Force $venvFull
}

if (-not (Test-Path $venvFull)) {
  Invoke-NativeChecked $PythonExe ($PythonExeArgs + @("-m", "venv", $venvFull))
}

$py  = Join-Path $venvFull "Scripts\python.exe"
$pip = Join-Path $venvFull "Scripts\pip.exe"

Invoke-NativeChecked $py @("-m", "pip", "install", "--upgrade", "pip", "setuptools", "wheel")
Invoke-NativeChecked $pip @("install", "--upgrade", "-c", $constraintsPath, "Cython>=3,<4", "numpy>=1.26.4,<3")

New-Item -ItemType Directory -Force -Path $torchHome | Out-Null
$env:TORCH_HOME = $torchHome

function Test-NvidiaGpu {
  try {
    & nvidia-smi *> $null
    return $LASTEXITCODE -eq 0
  } catch {
    return $false
  }
}

function Test-TorchCuda {
  & $py -c "import torch; raise SystemExit(0 if torch.cuda.is_available() else 1)"
  return $LASTEXITCODE -eq 0
}

function Reset-TorchPackages {
  # pip uninstall warns (to stderr) when packages aren't installed yet; that's fine - suppress.
  try { & $pip uninstall -y torch torchvision torchaudio 2>&1 | Out-Null } catch { }
}

function Install-TorchCpu {
  Write-Host "Installing CPU PyTorch..."
  Reset-TorchPackages
  Invoke-NativeChecked $pip @("install", "--upgrade", "--force-reinstall", "torch", "torchvision", "torchaudio", "--index-url", $cpuIndexUrl)
}

function Install-TorchCuda {
  Write-Host "Installing CUDA PyTorch from $cudaIndexUrl..."
  Reset-TorchPackages
  Invoke-NativeChecked $pip @("install", "--upgrade", "--force-reinstall", "torch", "torchvision", "torchaudio", "--index-url", $cudaIndexUrl)
  if (-not (Test-TorchCuda)) {
    throw "CUDA PyTorch installed, but torch.cuda.is_available() is false."
  }
}

$torchMode = $null
if ($CpuOnly) {
  Install-TorchCpu
  $torchMode = "cpu"
} elseif ($Cuda) {
  Install-TorchCuda
  $torchMode = "cuda"
} elseif ($Auto -and (Test-NvidiaGpu)) {
  try {
    Install-TorchCuda
    $torchMode = "cuda"
  } catch {
    Write-Warning "CUDA PyTorch setup failed: $($_.Exception.Message)"
    Write-Warning "Falling back to CPU PyTorch."
    Install-TorchCpu
    $torchMode = "cpu"
  }
} else {
  Install-TorchCpu
  $torchMode = "cpu"
}

Invoke-NativeChecked $pip @("install", "--no-build-isolation", "-c", $constraintsPath, "-r", (Join-Path $root "requirements.txt"))

# madmom: try pre-built bundled wheel first, then fall back to PyPI source build (requires MSVC).
Write-Host "Installing madmom (neural beat tracking)..."
$wheelsDir = Join-Path $root "wheels"
$madmomOk  = $false

if (Test-Path $wheelsDir) {
  Write-Host "  Trying bundled wheel from $wheelsDir..."
  & $pip install --find-links $wheelsDir "madmom>=0.16.1" 2>&1 | Out-Null
  if ($LASTEXITCODE -eq 0) {
    $madmomOk = $true
    Write-Host "  madmom installed from bundled wheel."
  } else {
    Write-Warning "  No compatible bundled wheel found - trying PyPI source build..."
  }
}

if (-not $madmomOk) {
  & $pip install "madmom>=0.16.1" 2>&1 | Out-Null
  if ($LASTEXITCODE -eq 0) {
    $madmomOk = $true
    Write-Host "  madmom installed from source."
  } else {
    Write-Warning "madmom install failed - neural beat tracking will be unavailable."
    Write-Warning "To fix: run Services\boogiemix\python\build-wheels.bat on the build machine (requires MSVC Build Tools), then rebuild and redeploy."
  }
}

# diffq: required by mdx_extra_q Demucs model. Has a C extension - try bundled wheel first.
Write-Host "Installing diffq (required by mdx_extra_q Demucs model)..."
$diffqOk = $false

if (Test-Path $wheelsDir) {
  & $pip install --find-links $wheelsDir "diffq>=0.2.4" 2>&1 | Out-Null
  if ($LASTEXITCODE -eq 0) {
    $diffqOk = $true
    Write-Host "  diffq installed from bundled wheel."
  } else {
    Write-Warning "  No compatible bundled diffq wheel found - trying PyPI source build..."
  }
}

if (-not $diffqOk) {
  & $pip install "diffq>=0.2.4" 2>&1 | Out-Null
  if ($LASTEXITCODE -eq 0) {
    Write-Host "  diffq installed from source."
  } else {
    Write-Warning "diffq install failed - mdx_extra_q Demucs model will not work."
    Write-Warning "To fix: run Services\boogiemix\python\build-wheels.bat on the build machine (requires MSVC Build Tools), then rebuild and redeploy."
  }
}

Invoke-NativeChecked $py @("-c", "import torch, demucs")

if ($PrimeDemucsModel) {
  Write-Host "Priming Demucs model cache for $defaultDemucsModel..."
  try {
    Invoke-NativeChecked $py @("-c", "from demucs.pretrained import get_model; get_model('$defaultDemucsModel'); print('Demucs model ready: $defaultDemucsModel')")
  } catch {
    Write-Warning "Model priming failed (model will be downloaded on first analysis run): $($_.Exception.Message)"
  }
}

Write-Host "BoogieMix deep-analysis worker environment ready: $venvFull"
Write-Host "PyTorch mode: $torchMode"
Write-Host "Torch model cache: $torchHome"
exit 0
