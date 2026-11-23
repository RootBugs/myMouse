$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root

# Cargo is installed per-user and is not on npm's inherited PATH.
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"

Write-Host '==> Building release backend'
& cargo build --release --manifest-path (Join-Path $root 'backend\Cargo.toml')
if ($LASTEXITCODE -ne 0) { throw 'release backend build failed' }

Write-Host '==> Building React and Electron sources'
& npm run build
if ($LASTEXITCODE -ne 0) { throw 'desktop source build failed' }

Write-Host '==> Packaging unsigned Windows app (NSIS + win-unpacked)'
& npx electron-builder --win nsis
if ($LASTEXITCODE -ne 0) { throw 'electron-builder packaging failed' }

Write-Host 'Windows build complete. Artifacts: dist\win-unpacked and dist\*.exe'
