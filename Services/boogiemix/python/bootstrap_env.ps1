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
  $pyLauncher = Get-Command "py"     -ErrorAction SilentlyContinue
  $pythonCmd  = Get-Command "python" -ErrorAction SilentlyContinue
  $resolvedExe = $null
  $resolvedArgs = @()

  # Determine which Python version(s) the *bundled* madmom wheel(s) actually
  # target (parsed from the wheel filename's cpXY ABI tag). A build machine
  # only produces a wheel for whatever Python version(s) it happens to have
  # installed (e.g. only 3.11), so this is NOT just a preference — a target
  # machine that ends up with a *different*, merely "generally compatible"
  # version (e.g. 3.12, because that's what happens to already be installed)
  # still gets an ABI mismatch: pip silently skips the incompatible wheel and
  # falls back to a PyPI source build that needs MSVC Build Tools — not
  # present on a typical end-user machine — so madmom silently fails to
  # install. Verified on two real clean-machine installs: the first attempt
  # only *reordered* preference and still settled for an already-installed
  # but mismatched 3.12 when 3.11 wasn't present at all. When a wheel is
  # bundled, only that exact version is acceptable — if it isn't already
  # installed, download and install it (side by side with whatever else is
  # already there) rather than settling for a mismatched "compatible" one.
  $wheelVersions = @()
  $wheelsDir = Join-Path $root "wheels"
  if (Test-Path $wheelsDir) {
    Get-ChildItem -Path $wheelsDir -Filter "madmom-*.whl" -ErrorAction SilentlyContinue | ForEach-Object {
      if ($_.Name -match 'cp(\d)(\d+)-') {
        $wheelVersions += "$($Matches[1]).$($Matches[2])"
      }
    }
  }
  # @(...) forces array semantics even when exactly one wheel matches —
  # otherwise PowerShell unwraps a single-item pipeline result to a bare
  # string, and $wheelVersions[0] would then index a *character* of that
  # string ("3.10"[0] -> "3") instead of the version itself, silently
  # breaking the download-fallback's version lookup below.
  $wheelVersions = @($wheelVersions | Select-Object -Unique)
  $requireExactMatch = $wheelVersions.Count -gt 0
  # Versions this run will accept: only the wheel's own version(s) if a wheel
  # is bundled (no substitutes), otherwise the general compatible range.
  $probeVersions = if ($requireExactMatch) { $wheelVersions } else { @("3.12", "3.11", "3.10") }

  if ($pyLauncher) {
    foreach ($v in $probeVersions) {
      & $pyLauncher.Source "-$v" "--version" *> $null
      if ($LASTEXITCODE -eq 0) {
        $resolvedExe = $pyLauncher.Source
        $resolvedArgs = @("-$v")
        break
      }
    }
  }

  if (-not $resolvedExe) {
    $candidateList = [System.Collections.Generic.List[string]]::new()
    foreach ($v in $probeVersions) {
      $candidateList.Add("$env:LOCALAPPDATA\Programs\Python\Python$($v.Replace('.',''))\python.exe")
      $candidateList.Add("${env:ProgramFiles}\Python$($v.Replace('.',''))\python.exe")
    }
    if (-not $requireExactMatch) {
      foreach ($p in @(
        "$env:USERPROFILE\miniconda3\python.exe",
        "$env:USERPROFILE\miniforge3\python.exe",
        "$env:USERPROFILE\anaconda3\python.exe"
      )) { $candidateList.Add($p) }
    }
    $resolvedExe = $candidateList | Where-Object {
      $_ -and ($_ -notlike "*\WindowsApps\*") -and (Test-Path $_)
    } | Select-Object -First 1
  }

  # Last resort: whatever py/python resolves to by default. Only acceptable
  # when no specific wheel version is required — a bare `py`/`python` command
  # can resolve to whatever is registered as the system default (possibly an
  # incompatible interpreter like 3.13+, or a compatible-but-mismatched one),
  # which is exactly what this fix avoids when a wheel is bundled.
  if ((-not $resolvedExe) -and (-not $requireExactMatch)) {
    $fallbackList = [System.Collections.Generic.List[string]]::new()
    if ($pyLauncher) { $fallbackList.Add($pyLauncher.Source) }
    if ($pythonCmd)  { $fallbackList.Add($pythonCmd.Source) }
    $resolvedExe = $fallbackList | Where-Object {
      $_ -and ($_ -notlike "*\WindowsApps\*") -and (Test-Path $_)
    } | Select-Object -First 1
    if ($resolvedExe -like "*\py.exe") {
      $resolvedArgs = @("-3")
    }
  }

  if (-not $resolvedExe) {
    # Download whichever version the bundled wheel targets (falls back to
    # 3.12 if no wheel is bundled) — installing a mismatched version here
    # would silently reproduce the exact ABI mismatch this script exists to
    # avoid, just via the "nothing found" path instead of the probing path.
    $downloadMinor = if ($wheelVersions.Count -gt 0) { $wheelVersions[0] } else { "3.12" }
    $patchVersions = @{ "3.12" = "3.12.10"; "3.11" = "3.11.9"; "3.10" = "3.10.11" }
    $pyVersion = $patchVersions[$downloadMinor]
    if (-not $pyVersion) { $pyVersion = "3.12.10"; $downloadMinor = "3.12" }
    Write-Host "No matching Python found - downloading and installing Python $pyVersion..."
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
    if ($pyCmd2) {
      & $pyCmd2.Source "-$downloadMinor" "--version" *> $null
      if ($LASTEXITCODE -eq 0) {
        $resolvedExe = $pyCmd2.Source
        $resolvedArgs = @("-$downloadMinor")
      }
    }
    if (-not $resolvedExe) {
      $cl2 = [System.Collections.Generic.List[string]]::new()
      $cl2.Add("$env:LOCALAPPDATA\Programs\Python\Python$($downloadMinor.Replace('.',''))\python.exe")
      if ((-not $requireExactMatch) -and $pythonCmd2) { $cl2.Add($pythonCmd2.Source) }
      $resolvedExe = $cl2 | Where-Object {
        $_ -and ($_ -notlike "*\WindowsApps\*") -and (Test-Path $_)
      } | Select-Object -First 1
    }
    if (-not $resolvedExe) {
      throw "Python was installed but still cannot be found. Please restart the terminal and re-run this script."
    }
  }

  $PythonExe = $resolvedExe
  $PythonExeArgs = $resolvedArgs + $PythonExeArgs
  Write-Host "Using Python: $PythonExe $($resolvedArgs -join ' ')"
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
