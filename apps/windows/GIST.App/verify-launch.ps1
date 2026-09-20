# Launch smoke test for the unpackaged shell: build output must start, show a titled window, stay
# responsive, and exit cleanly on close. Usage: pwsh -File verify-launch.ps1 (after `dotnet build`).
param([string]$Config = 'Debug', [int]$WaitSeconds = 6)
$exe = Join-Path $PSScriptRoot "bin/$Config/net10.0-windows10.0.19041.0/win-x64/GIST.exe"
if (-not (Test-Path $exe)) { Write-Error "Not built: $exe"; exit 2 }
$p = Start-Process $exe -PassThru
Start-Sleep -Seconds $WaitSeconds
$p.Refresh()
$ok = -not $p.HasExited -and $p.MainWindowTitle -eq 'GIST' -and $p.Responding
Write-Output "alive=$(-not $p.HasExited) title='$($p.MainWindowTitle)' responding=$($p.Responding)"
if ($p.HasExited) { exit 1 }
$null = $p.CloseMainWindow()
$exited = $p.WaitForExit(8000)
Write-Output "exited=$exited code=$(if ($exited) { $p.ExitCode })"
if (-not $exited) { $p.Kill(); exit 1 }
if ($ok -and $p.ExitCode -eq 0) { exit 0 } else { exit 1 }
