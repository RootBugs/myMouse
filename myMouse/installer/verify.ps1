$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)

$desktop = Join-Path $root 'dist\win-unpacked\myMouse.exe'
$backend = Join-Path $root 'backend\target\release\mymouse-backend.exe'
$resource = Join-Path $root 'dist\win-unpacked\resources\mymouse-backend.exe'

if (-not (Test-Path $desktop)) { throw 'desktop build missing' }
if (-not (Test-Path $backend)) { throw 'backend build missing' }
if (-not (Test-Path $resource)) { throw 'packaged backend resource missing' }

Write-Host 'OK: unsigned Windows desktop, release backend, and packaged backend resource are present.'
