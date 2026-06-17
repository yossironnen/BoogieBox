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
$torchHome = Join-Path $root "model-cache\torch"
$cudaIndexUrl = "https://download.pytorch.org/whl/cu128"
$cpuIndexUrl = "https://download.pytorch.org/whl/cpu"
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

$py = Join-Path $venvFull "Scripts\python.exe"
$pip = Join-Path $venvFull "Scripts\pip.exe"

Invoke-NativeChecked $py @("-m", "pip", "install", "--upgrade", "pip", "setuptools", "wheel")
Invoke-NativeChecked $pip @("install", "--upgrade", "Cython<3", "numpy>=1.24")

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
  # pip uninstall warns (to stderr) when packages aren't installed yet; that's fine — suppress.
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

Invoke-NativeChecked $pip @("install", "--no-build-isolation", "-r", (Join-Path $root "requirements.txt"))

# madmom: try pre-built bundled wheel first, then fall back to PyPI source build (requires MSVC).
Write-Host "Installing madmom (neural beat tracking)..."
$wheelsDir = Join-Path $root "wheels"
$madmomOk = $false

if (Test-Path $wheelsDir) {
  Write-Host "  Trying bundled wheel from $wheelsDir..."
  & $pip install --find-links $wheelsDir "madmom>=0.16.1" 2>&1 | Out-Null
  if ($LASTEXITCODE -eq 0) {
    $madmomOk = $true
    Write-Host "  madmom installed from bundled wheel."
  } else {
    Write-Warning "  No compatible bundled wheel found — trying PyPI source build..."
  }
}

if (-not $madmomOk) {
  & $pip install "madmom>=0.16.1" 2>&1 | Out-Null
  if ($LASTEXITCODE -eq 0) {
    $madmomOk = $true
    Write-Host "  madmom installed from source."
  } else {
    Write-Warning "madmom install failed — neural beat tracking will be unavailable."
    Write-Warning "To fix: run Services\boogiemix\python\build-wheels.bat on the build machine (requires MSVC Build Tools), then rebuild and redeploy."
  }
}

Invoke-NativeChecked $py @("-c", "import torch, demucs")

if ($PrimeDemucsModel) {
  Write-Host "Priming Demucs model cache for $defaultDemucsModel..."
  Invoke-NativeChecked $py @("-c", "from demucs.pretrained import get_model; get_model('$defaultDemucsModel'); print('Demucs model ready: $defaultDemucsModel')")
}

Write-Host "BoogieMix deep-analysis worker environment ready: $venvFull"
Write-Host "PyTorch mode: $torchMode"
Write-Host "Torch model cache: $torchHome"
exit 0
