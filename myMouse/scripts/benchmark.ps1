$ErrorActionPreference = 'Stop'

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$projectDir = Split-Path -Parent $scriptDir
$exe = Join-Path $projectDir 'backend\target\debug\mymouse-backend.exe'
$reportPath = Join-Path $projectDir 'docs\benchmarks.md'

$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
& cargo build --manifest-path (Join-Path $projectDir 'backend\Cargo.toml')
if ($LASTEXITCODE -ne 0) { throw 'backend build failed' }
if (-not (Test-Path $exe)) { throw "Backend binary not found at '$exe'." }

# Each run gets its own directory. Redirected stdout handles stay alive briefly
# after a child exits, so fixed paths race on back-to-back runs.
$logDir = Join-Path $projectDir ('backend\target\benchmark-logs\' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $logDir | Out-Null
$workDir = Join-Path $logDir 'work'
New-Item -ItemType Directory -Force -Path $workDir | Out-Null

function Get-FreePort {
  $listener = New-Object System.Net.Sockets.TcpListener ([System.Net.IPAddress]::Loopback, 0)
  $listener.Start()
  $port = $listener.LocalEndpoint.Port
  $listener.Stop()
  return $port
}

function Read-LogText {
  param([string]$Path)
  if (-not (Test-Path $Path)) { return '' }
  for ($attempt = 0; $attempt -lt 5; $attempt++) {
    try {
      $stream = [System.IO.File]::Open(
        $Path,
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
    catch { Start-Sleep -Milliseconds 100 }
  }
  return ''
}

function Wait-Line {
  param(
    [string]$LogPath,
    [string]$Pattern,
    [int]$From,
    [int]$TimeoutMs,
    [string]$What
  )

  $deadline = [datetime]::UtcNow.AddMilliseconds($TimeoutMs)
  $rx = [regex]('(?m)^.*' + $Pattern + '.*$')
  while ([datetime]::UtcNow -lt $deadline) {
    $text = Read-LogText $LogPath
    $segment = if ($text.Length -gt $From) { $text.Substring($From) } else { '' }
    $match = $rx.Match($segment)
    if ($match.Success) {
      return [pscustomobject]@{
        Time = [datetime]::UtcNow
        Line = $match.Value.Trim()
        Offset = $From + $match.Index + $match.Length
      }
    }
    Start-Sleep -Milliseconds 10
  }
  $log = Read-LogText $LogPath
  if ($log.Length -gt 4000) { $log = '...<truncated>...' + $log.Substring($log.Length - 4000) }
  throw "Timed out waiting for $What. Log: $LogPath`n$log"
}

function Log-Offset {
  param([string]$LogPath)
  return (Read-LogText $LogPath).Length
}

function Get-BackendPid {
  param([int]$ShellPid)
  $deadline = [datetime]::UtcNow.AddSeconds(10)
  while ([datetime]::UtcNow -lt $deadline) {
    $child = Get-CimInstance Win32_Process -Filter "ParentProcessId = $ShellPid" |
      Where-Object { $_.Name -eq 'mymouse-backend.exe' } |
      Select-Object -First 1
    if ($child) { return [int]$child.ProcessId }
    Start-Sleep -Milliseconds 50
  }
  throw "Could not find mymouse-backend.exe child of shell PID $ShellPid"
}

function Start-Backend {
  param(
    [string]$Name,
    [string]$DeviceId,
    [int]$Port,
    [string]$PeerAddress
  )

  $outPath = Join-Path $logDir "$Name.out"
  $errPath = Join-Path $logDir "$Name.err"
  [System.IO.File]::WriteAllText($outPath, '')
  [System.IO.File]::WriteAllText($errPath, '')

  # A real stdin pipe is required: test_mode.rs reads stdin once to EOF, so
  # appending RPCs to a redirected stdin file after process start is invisible.
  # cmd.exe owns only stdout/stderr file redirection; the backend inherits our
  # stdin pipe through cmd.
  $command = '"' + $exe + '" --test-mode --device-id "' + $DeviceId + '" --port "' + $Port + '"'
  if ($PeerAddress) {
    $command += ' --peer-id "pc-b" --peer "' + $PeerAddress + '"'
  }
  $command += ' > "' + $outPath + '" 2> "' + $errPath + '"'

  $psi = New-Object System.Diagnostics.ProcessStartInfo
  $psi.FileName = 'cmd.exe'
  # cmd's /s /c form preserves the quoted executable path while still giving
  # the backend our redirected stdin pipe.
  $psi.Arguments = '/s /c "' + $command + '"'
  $psi.UseShellExecute = $false
  $psi.CreateNoWindow = $true
  $psi.RedirectStandardInput = $true
  $psi.RedirectStandardOutput = $false
  $psi.RedirectStandardError = $false

  $process = New-Object System.Diagnostics.Process
  $process.StartInfo = $psi
  $null = $process.Start()
  $backendPid = Get-BackendPid -ShellPid $process.Id

  return [pscustomobject]@{
    Name = $Name
    Process = $process
    BackendPid = $backendPid
    OutPath = $outPath
    ErrPath = $errPath
  }
}

function Send-Rpc {
  param($Target, [int]$Id, [string]$Method, [hashtable]$Params)

  $request = @{ id = $Id; method = $Method; params = $Params } | ConvertTo-Json -Depth 20 -Compress
  $Target.Process.StandardInput.WriteLine($request)
  $Target.Process.StandardInput.Flush()
  return [datetime]::UtcNow
}

function Stop-Backend {
  param($Target)
  if (-not $Target) { return }
  try {
    if (-not $Target.Process.HasExited) {
      # taskkill /T reaches the backend child; Stop-Process would only kill cmd.
      & taskkill.exe /PID $Target.Process.Id /T /F 2>$null | Out-Null
    }
    $Target.Process.WaitForExit(5000) | Out-Null
  }
  finally { $Target.Process.Dispose() }
}

function Get-ProcSample {
  param($Target)
  $process = Get-Process -Id $Target.BackendPid -ErrorAction Stop
  return [pscustomobject]@{
    Cpu = $process.TotalProcessorTime.TotalMilliseconds
    Ram = $process.WorkingSet64 / 1MB
    At = [datetime]::UtcNow
  }
}

$portB = Get-FreePort
do { $portA = Get-FreePort } while ($portA -eq $portB)

$a = $null
$b = $null
try {
  $b = Start-Backend -Name 'pc-b' -DeviceId 'pc-b' -Port $portB
  $null = Wait-Line -LogPath $b.OutPath -Pattern '"notification"\s*:\s*"ready"' -From 0 -TimeoutMs 30000 -What 'pc-b ready'
  $null = Send-Rpc $b 1 'setFileTransferSettings' @{ autoAccept = $true }
  $null = Wait-Line -LogPath $b.OutPath -Pattern '"id"\s*:\s*1' -From 0 -TimeoutMs 10000 -What 'pc-b autoAccept response'

  $connectStarted = [datetime]::UtcNow
  $a = Start-Backend -Name 'pc-a' -DeviceId 'pc-a' -Port $portA -PeerAddress "127.0.0.1:$portB"
  $connected = Wait-Line -LogPath $a.OutPath -Pattern '"notification"\s*:\s*"connected"' -From 0 -TimeoutMs 30000 -What 'pc-a pinned TLS connect'

  $inputOffset = Log-Offset $b.OutPath
  $inputStarted = Send-Rpc $a 1 'crossTo' @{ target = 'pc-b'; x = 100; y = 200 }
  $inputDelivered = Wait-Line -LogPath $b.OutPath -Pattern '"notification"\s*:\s*"input"' -From $inputOffset -TimeoutMs 10000 -What 'pc-b InputEvent delivery'

  $clipboardBytes = 262144
  $clipboardText = 'a' * $clipboardBytes
  $clipboardOffset = Log-Offset $b.OutPath
  $clipboardStarted = Send-Rpc $a 2 'sendClipboard' @{ kind = 'text'; text = $clipboardText; mime = 'text/plain' }
  $clipboardDelivered = Wait-Line -LogPath $b.OutPath -Pattern '"notification"\s*:\s*"clipboardUpdate"' -From $clipboardOffset -TimeoutMs 20000 -What 'pc-b clipboardUpdate'

  $fileBytes = 1048576
  $filePath = Join-Path $workDir 'benchmark.bin'
  $buffer = New-Object byte[] $fileBytes
  for ($i = 0; $i -lt $fileBytes; $i++) { $buffer[$i] = [byte]($i % 251) }
  [System.IO.File]::WriteAllBytes($filePath, $buffer)

  $fileOffset = Log-Offset $b.OutPath
  $offerSent = Send-Rpc $a 3 'offerFile' @{ path = $filePath; peerDeviceId = 'pc-b' }
  $offerLine = Wait-Line -LogPath $b.OutPath -Pattern '"notification"\s*:\s*"transferUpdate"' -From $fileOffset -TimeoutMs 20000 -What 'pc-b file offer'
  $offer = $offerLine.Line | ConvertFrom-Json
  $transferId = [string]$offer.transferId
  if (-not $transferId) { throw "File offer did not contain a transferId: $($offerLine.Line)" }
  $completed = Wait-Line -LogPath $b.OutPath -Pattern ('"transferId"\s*:\s*"' + [regex]::Escape($transferId) + '"[\s\S]*"state"\s*:\s*"completed"') -From $offerLine.Offset -TimeoutMs 30000 -What 'pc-b file completion'

  $idleSamples = @()
  for ($i = 0; $i -lt 10; $i++) {
    $idleSamples += [pscustomobject]@{
      A = Get-ProcSample $a
      B = Get-ProcSample $b
    }
    Start-Sleep -Milliseconds 200
  }

  $connectMs = [math]::Round(($connected.Time - $connectStarted).TotalMilliseconds, 2)
  $inputMs = [math]::Round(($inputDelivered.Time - $inputStarted).TotalMilliseconds, 2)
  $clipboardSeconds = ($clipboardDelivered.Time - $clipboardStarted).TotalSeconds
  $fileSeconds = ($completed.Time - $offerSent).TotalSeconds
  $clipboardMbps = [math]::Round(($clipboardBytes * 8 / $clipboardSeconds / 1000000), 3)
  $fileMbps = [math]::Round(($fileBytes * 8 / $fileSeconds / 1000000), 3)

  $sampleSeconds = ($idleSamples[-1].A.At - $idleSamples[0].A.At).TotalSeconds
  $cpuA = [math]::Round((($idleSamples[-1].A.Cpu - $idleSamples[0].A.Cpu) / 1000 / $sampleSeconds / [Environment]::ProcessorCount) * 100, 3)
  $cpuB = [math]::Round((($idleSamples[-1].B.Cpu - $idleSamples[0].B.Cpu) / 1000 / $sampleSeconds / [Environment]::ProcessorCount) * 100, 3)
  $ramA = [math]::Round((($idleSamples | ForEach-Object { $_.A.Ram } | Measure-Object -Average).Average), 2)
  $ramB = [math]::Round((($idleSamples | ForEach-Object { $_.B.Ram } | Measure-Object -Average).Average), 2)

  if ($connectMs -le 0 -or $inputMs -le 0 -or $clipboardMbps -le 0 -or $fileMbps -le 0 -or $cpuA -lt 0 -or $cpuB -lt 0) {
    throw "Benchmark produced an invalid metric: connect=$connectMs input=$inputMs clipboard=$clipboardMbps file=$fileMbps cpu=$cpuA/$cpuB"
  }

  $raw = [ordered]@{
    input_delivery_ms = $inputMs
    clipboard_mbps = $clipboardMbps
    file_mbps = $fileMbps
    connect_ms = $connectMs
    reconnect_ms = $null
    idle_cpu_percent_pc_a = $cpuA
    idle_cpu_percent_pc_b = $cpuB
    idle_ram_mb_pc_a = $ramA
    idle_ram_mb_pc_b = $ramB
    clipboard_bytes = $clipboardBytes
    file_bytes = $fileBytes
    logs = $logDir
  }
  $json = $raw | ConvertTo-Json -Depth 10

  $report = @"
# myMouse two-process benchmark

Measured: $([datetime]::UtcNow.ToString('o'))

All values below come from two real ``mymouse-backend.exe`` processes on this
Windows host, connected over the pinned-TLS v2 test transport. This is a
**two-process loopback harness**, not field latency, not a physical two-PC
session, and **not a comparison with any upstream product**.

| Metric | Value | Unit | Evidence |
| --- | ---: | --- | --- |
| Input delivery | $inputMs | ms | RPC ``crossTo`` written to pc-a stdin -> ``"notification":"input"`` observed in pc-b log; 10 ms polling included |
| Clipboard throughput | $clipboardMbps | Mbit/s | 256 KiB text through ``sendClipboard`` -> ``clipboardUpdate`` observed on pc-b |
| File throughput | $fileMbps | Mbit/s | 1 MiB auto-accepted offer -> ``"state":"completed"`` observed on pc-b |
| Cold pinned-TLS connect | $connectMs | ms | pc-a process start -> ``"notification":"connected"`` |
| Reconnect after peer loss | not measured | ms | Test mode has no auto-reconnect loop; production reconnect remains unmeasured |
| Idle CPU, pc-a | $cpuA | % | average across 10 samples over ~2 s, divided by logical processors |
| Idle CPU, pc-b | $cpuB | % | average across 10 samples over ~2 s, divided by logical processors |
| Idle RAM, pc-a | $ramA | MiB | average ``WorkingSet64`` across 10 samples |
| Idle RAM, pc-b | $ramB | MiB | average ``WorkingSet64`` across 10 samples |

Raw values:

````json
$json
````

Logs: ``$logDir``

Reconnect time is intentionally reported as unmeasured. Test mode starts one
outbound connection and does not retry after a peer disappears; claiming a
reconnect number from this harness would be dishonest. See
[product-roadmap.md](product-roadmap.md) rank 7.
"@
  [System.IO.File]::WriteAllText($reportPath, $report)
  Write-Host "OK: benchmark wrote $reportPath"
  Write-Host "input=$inputMs ms clipboard=$clipboardMbps Mbit/s file=$fileMbps Mbit/s connect=$connectMs ms idle_cpu=$cpuA/$cpuB% idle_ram=$ramA/$ramB MiB"
}
finally {
  foreach ($target in @($a, $b)) { Stop-Backend $target }
}
