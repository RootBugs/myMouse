$ErrorActionPreference = 'Stop'

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$projectDir = Split-Path -Parent $scriptDir
$exe = Join-Path $projectDir 'backend\target\debug\mymouse-backend.exe'

$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
& cargo build --manifest-path (Join-Path $projectDir 'backend\Cargo.toml')
if ($LASTEXITCODE -ne 0) { throw 'backend build failed' }

if (-not (Test-Path $exe)) {
  throw "Backend binary not found at '$exe'."
}

$logDir = Join-Path $projectDir 'backend\target\smoke-logs'
New-Item -ItemType Directory -Force -Path $logDir | Out-Null
$outA = Join-Path $logDir 'pc-a.out'
$errA = Join-Path $logDir 'pc-a.err'
$outB = Join-Path $logDir 'pc-b.out'
$errB = Join-Path $logDir 'pc-b.err'
foreach ($file in @($outA, $errA, $outB, $errB)) { '' | Set-Content -Path $file }

function Get-FreePort {
  $listener = New-Object System.Net.Sockets.TcpListener ([System.Net.IPAddress]::Loopback, 0)
  $listener.Start()
  $port = $listener.LocalEndpoint.Port
  $listener.Stop()
  return $port
}

function Read-Log($path) {
  if (-not (Test-Path $path)) { return '' }
  for ($attempt = 0; $attempt -lt 5; $attempt++) {
    try {
      $stream = [System.IO.File]::Open(
        $path,
        [System.IO.FileMode]::Open,
        [System.IO.FileAccess]::Read,
        [System.IO.FileShare]::ReadWrite
      )
      try {
        $reader = New-Object System.IO.StreamReader($stream)
        return $reader.ReadToEnd()
      }
      finally { $stream.Dispose() }
    }
    catch {
      Start-Sleep -Milliseconds 100
    }
  }
  return ''
}

$portB = Get-FreePort
do { $portA = Get-FreePort } while ($portA -eq $portB)

$pB = $null
$pA = $null
try {
  # PC B listens; PC A connects with the pinned test fingerprint and injects a
  # synthetic right-edge crossing at startup. No stdin control is needed.
  $pB = Start-Process -FilePath $exe -ArgumentList @(
    '--test-mode', '--device-id', 'pc-b', '--port', "$portB"
  ) -RedirectStandardOutput $outB -RedirectStandardError $errB -PassThru -WindowStyle Hidden

  $pA = Start-Process -FilePath $exe -ArgumentList @(
    '--test-mode', '--device-id', 'pc-a', '--port', "$portA",
    '--peer-id', 'pc-b', '--peer', "127.0.0.1:$portB",
    '--cross-to', 'pc-b', '--x', '100', '--y', '200'
  ) -RedirectStandardOutput $outA -RedirectStandardError $errA -PassThru -WindowStyle Hidden

  $deadline = (Get-Date).AddSeconds(30)
  $aReady = $false; $aConnected = $false; $aForwarded = $false; $bInput = $false
  while ((Get-Date) -lt $deadline) {
    Start-Sleep -Milliseconds 200
    $aLog = Read-Log $outA
    $bLog = Read-Log $outB
    $aReady = $aLog -match '"notification"\s*:\s*"ready"'
    $aConnected = $aLog -match '"notification"\s*:\s*"connected"'
    $aForwarded = $aLog -match '"notification"\s*:\s*"forwarded"'
    $bInput = $bLog -match '"notification"\s*:\s*"input"'
    if ($aConnected -and $aForwarded -and $bInput) { break }
    if ($pA.HasExited -or $pB.HasExited) { break }
  }

  if (-not $aReady) { throw "pc-a did not become ready. Log: $(Read-Log $outA) Errors: $(Read-Log $errA)" }
  if (-not $aConnected) { throw "pc-a did not connect to pc-b. Log: $(Read-Log $outA) Errors: $(Read-Log $errA)" }
  if (-not $aForwarded) { throw "pc-a did not forward the crossing. Log: $(Read-Log $outA)" }
  if (-not $bInput) { throw "pc-b did not receive an InputEvent. Log: $(Read-Log $outB) Errors: $(Read-Log $errB)" }

  Write-Host 'OK: pc-a forwarded one crossing to pc-b over pinned TLS; pc-b received one InputEvent.'
  Write-Host "Logs: $outA and $outB"
}
finally {
  foreach ($process in @($pA, $pB)) {
    if ($process -and -not $process.HasExited) {
      Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
    }
  }
}
