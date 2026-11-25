# Builds Agent Gauge from source and runs the Windows installer it produces.
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")

foreach ($tool in @("node", "cargo")) {
  if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) {
    Write-Host "Falta $tool. Instala Node 20+ (https://nodejs.org) y Rust (https://rustup.rs)."
    exit 1
  }
}

Write-Host "Instalando dependencias..."
npm ci --no-audit --no-fund
Write-Host "Compilando (la primera vez tarda unos minutos)..."
npx tauri build --bundles nsis

$installer = Get-ChildItem "src-tauri/target/release/bundle/nsis/*-setup.exe" | Select-Object -First 1
Start-Process $installer.FullName -Wait
Write-Host "Listo. Busca 'Agent Gauge' en el menú Inicio."
