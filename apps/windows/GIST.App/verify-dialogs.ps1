# UIA check of the Library dialogs via the dev-only GIST_DIALOG_HARNESS hook (Dialogs/DialogHarness.cs).
# Usage (after build + `dotnet build spikes/seed`): pwsh -File verify-dialogs.ps1 [-Config Debug]
# Scratch store only (GIST_DATA_ROOT). Exit 0 = all checks passed.
param([string]$Config = 'Debug', [int]$TimeoutSeconds = 25)
$ErrorActionPreference = 'Stop'
$repo = Resolve-Path (Join-Path $PSScriptRoot '../../..')
$exe = Join-Path $PSScriptRoot "bin/$Config/net10.0-windows10.0.19041.0/win-x64/GIST.exe"
$seed = Join-Path $PSScriptRoot "../spikes/seed/bin/$Config/net10.0-windows/win-x64/SeedTool.exe"
$fixture = Join-Path $repo 'fixtures/txt/basic_ascii.txt'
foreach ($f in @($exe, $seed, $fixture)) { if (-not (Test-Path $f)) { Write-Error "Missing: $f"; exit 2 } }
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
$failed = $false
function Check($name, $ok) { Write-Output ("{0}: {1}" -f ($(if ($ok) { 'PASS' } else { 'FAIL' }), $name)); if (-not $ok) { $script:failed = $true } }
function Get-Els($proc) {
    $win = [Windows.Automation.AutomationElement]::FromHandle($proc.MainWindowHandle)
    foreach ($e in $win.FindAll([Windows.Automation.TreeScope]::Descendants, [Windows.Automation.Condition]::TrueCondition)) {
        [pscustomobject]@{ Name = $e.Current.Name; Type = $e.Current.ControlType.ProgrammaticName; Enabled = $e.Current.IsEnabled }
    }
}
# scenario -> (title regex, body regex, buttons that must exist)
$scenarios = [ordered]@{
  ImportUrl      = @('^Import URL$', '^GIST fetches the page, extracts', @('Import', 'Cancel'))
  NewCollection  = @('^New Collection$', '^Collection name$', @('Create', 'Cancel'))
  RemoveConfirm  = @('^Remove 1 item\?$', 'can''t be undone', @('Remove', 'Cancel'))
  EncryptConfirm = @('^Encrypt 1 item\?$', 'no way to recover encrypted items', @('Encrypt', 'Cancel'))
  EncryptResult  = @('^Encryption finished$', 'No items were selected|encrypted', @('OK'))
  DrmProtected   = @('^Can''t import this book$', '^This book is DRM-protected and can''t be imported\.$', @('OK'))
  Error          = @('^Something went wrong$', 'couldn''t be completed', @('OK'))
  TagEditor      = @('.+', '^No tags yet\.$', @('Done', 'Add'))
}
foreach ($s in $scenarios.Keys) {
    $root = Join-Path ([IO.Path]::GetTempPath()) ("gist-dlg-" + [Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $root | Out-Null
    $p = $null
    try {
        & $seed $root $fixture | Out-Null
        $env:GIST_DATA_ROOT = $root
        $env:GIST_DIALOG_HARNESS = $s
        $p = Start-Process $exe -PassThru
        $exp = $scenarios[$s]; $names = @(); $ok = $false
        $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
        do {
            Start-Sleep -Milliseconds 700; $p.Refresh()
            if ($p.HasExited) { break }
            if ($p.MainWindowHandle -ne 0) {
                $names = @(Get-Els $p)
                $btn = @($names | Where-Object { $_.Type -eq 'ControlType.Button' } | ForEach-Object { $_.Name })
                $missing = @($exp[2] | Where-Object { $_ -notin $btn })
                $ok = [bool]($names | Where-Object { $_.Name -match $exp[1] }) -and ($missing.Count -eq 0)
            }
        } while (-not $ok -and (Get-Date) -lt $deadline)
        Write-Output "--- $s"
        Check "$s title" ([bool]($names | Where-Object { $_.Name -match $exp[0] }))
        Check "$s body" ([bool]($names | Where-Object { $_.Name -match $exp[1] }))
        foreach ($b in $exp[2]) { Check "$s button '$b'" ([bool]($names | Where-Object { $_.Type -eq 'ControlType.Button' -and $_.Name -eq $b })) }
        Check "$s no raw exception/path text" (-not ($names | Where-Object { $_.Name -match 'Exception|panic|[A-Za-z]:\\' }))
        $p.Refresh(); Check "$s process Responding" $p.Responding
        if ($s -eq 'RemoveConfirm') { Check 'no "Original File" wording' (-not ($names | Where-Object { $_.Name -match 'Delete Original' })) }
        if ($s -eq 'ImportUrl') {
            $imp = $names | Where-Object { $_.Type -eq 'ControlType.Button' -and $_.Name -eq 'Import' } | Select-Object -First 1
            Check 'ImportUrl: Import disabled while blank' (($null -ne $imp) -and -not $imp.Enabled)
        }
        $null = $p.CloseMainWindow(); if (-not $p.WaitForExit(8000)) { $p.Kill() }
    }
    finally {
        Remove-Item Env:GIST_DATA_ROOT, Env:GIST_DIALOG_HARNESS -ErrorAction SilentlyContinue
        if ($p -and -not $p.HasExited) { $p.Kill() }
        Start-Sleep -Milliseconds 300
        Remove-Item -Recurse -Force $root -ErrorAction SilentlyContinue
    }
}
if ($failed) { exit 1 } else { exit 0 }
