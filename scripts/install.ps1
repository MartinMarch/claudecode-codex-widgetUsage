# Builds Agent Gauge from source and runs the Windows installer it produces.
# Usage (PowerShell, from the repo folder):  ./scripts/install.ps1
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")

# Rust installs into ~/.cargo/bin; a terminal opened before installing it
# does not have that folder in PATH yet.
$cargoBin = Join-Path $env:USERPROFILE ".cargo\bin"
if ((Test-Path $cargoBin) -and -not ($env:Path -split ";" -contains $cargoBin)) {
  $env:Path = "$cargoBin;$env:Path"
}

$missing = @()
if (-not (Get-Command node -ErrorAction SilentlyContinue)) {
  $missing += "Node.js 20 o superior:  winget install OpenJS.NodeJS.LTS"
}
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
  $missing += "Rust:  winget install Rustlang.Rustup"
}
# Rust on Windows links with the Visual Studio C++ build tools.
$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$hasMsvc = (Test-Path $vswhere) -and (& $vswhere -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath)
if (-not $hasMsvc) {
  $missing += 'Herramientas de C++ de Visual Studio:  winget install Microsoft.VisualStudio.2022.BuildTools --override "--passive --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"'
}
if ($missing.Count -gt 0) {
  Write-Host "Antes de compilar, instala lo que falta y abre una terminal nueva:" -ForegroundColor Yellow
  $missing | ForEach-Object { Write-Host "  - $_" }
  exit 1
}

# An open copy of the widget would lock the files the installer replaces.
Get-Process agent-gauge -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false

Write-Host "Instalando dependencias..."
npm ci --no-audit --no-fund
Write-Host "Compilando (la primera vez tarda unos minutos)..."
npx tauri build --bundles nsis

$installer = Get-ChildItem "src-tauri/target/release/bundle/nsis/*-setup.exe" | Select-Object -First 1
Write-Host "Abriendo el instalador: $($installer.Name)"
Start-Process $installer.FullName -Wait
Write-Host "Listo. Agent Gauge está en el menú Inicio y en el escritorio."
Write-Host "Para que se abra al encender el equipo: clic derecho en su icono de la bandeja > 'Abrir al iniciar sesión'."
