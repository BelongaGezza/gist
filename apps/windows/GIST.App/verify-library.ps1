# End-to-end check for the W1 Library page against a scratch store (never the real profile).
# Usage (after `dotnet build GIST.sln`, native DLL staged, bindings generated):
#   pwsh -File verify-library.ps1 -Scenario Library   # seeded item's title must appear; process stays Responding
#   pwsh -File verify-library.ps1 -Scenario Corrupt   # garbage content-key.dpapi -> blocking page; key bytes unchanged
# Uses GIST_DATA_ROOT (dev/test only) and UI Automation. Exit 0 = all checks passed.
param(
    [ValidateSet('Library', 'Corrupt')][string]$Scenario = 'Library',
    [string]$Config = 'Debug',
    [int]$TimeoutSeconds = 25
)
$ErrorActionPreference = 'Stop'
$repo = Resolve-Path (Join-Path $PSScriptRoot '../../..')
$exe = Join-Path $PSScriptRoot "bin/$Config/net10.0-windows10.0.19041.0/win-x64/GIST.exe"
$seed = Join-Path $PSScriptRoot "../spikes/seed/bin/$Config/net10.0-windows/win-x64/SeedTool.exe"
$fixture = Join-Path $repo 'fixtures/txt/basic_ascii.txt'
foreach ($f in @($exe, $seed, $fixture)) { if (-not (Test-Path $f)) { Write-Error "Missing: $f (build first)"; exit 2 } }

Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes

$root = Join-Path ([IO.Path]::GetTempPath()) ("gist-verify-" + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $root | Out-Null
$failed = $false
function Check($name, $ok, $detail = '') {
    Write-Output ("{0}: {1} {2}" -f ($(if ($ok) { 'PASS' } else { 'FAIL' }), $name, $detail))
    if (-not $ok) { $script:failed = $true }
}
function Get-Names($proc) {
    $win = [Windows.Automation.AutomationElement]::FromHandle($proc.MainWindowHandle)
    $all = $win.FindAll([Windows.Automation.TreeScope]::Descendants, [Windows.Automation.Condition]::TrueCondition)
    foreach ($e in $all) { [pscustomobject]@{ Name = $e.Current.Name; Type = $e.Current.ControlType.ProgrammaticName } }
}

$p = $null
try {
    $seedOut = & $seed $root $fixture
    if ($LASTEXITCODE -ne 0) { Write-Error "Seeding failed"; exit 2 }
    $title = (($seedOut | Where-Object { $_ -like 'TITLE=*' } | Select-Object -First 1) -replace '^TITLE=', '')
    Write-Output "seeded 1 item, title='$title'"

    $keyFile = Join-Path $root 'keys/content-key.dpapi'
    if ($Scenario -eq 'Corrupt') {
        [IO.File]::WriteAllBytes($keyFile, [byte[]](1..97 | ForEach-Object { ($_ * 37) % 251 }))
        $hashBefore = (Get-FileHash $keyFile -Algorithm SHA256).Hash
    }

    $env:GIST_DATA_ROOT = $root
    $p = Start-Process $exe -PassThru
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    $found = $false
    $names = @()
    do {
        Start-Sleep -Milliseconds 700
        $p.Refresh()
        if ($p.HasExited) { break }
        if ($p.MainWindowHandle -ne 0) {
            $names = @(Get-Names $p)
            if ($Scenario -eq 'Library') { $found = [bool]($names | Where-Object { $_.Name -eq $title }) }
            else { $found = [bool]($names | Where-Object { $_.Name -like "Encrypted items can*t be unlocked" }) }
        }
    } while (-not $found -and (Get-Date) -lt $deadline)

    Check 'process alive' (-not $p.HasExited)
    $p.Refresh()
    Check 'process Responding' $p.Responding
    $texts = ($names | ForEach-Object { $_.Name } | Where-Object { $_ }) -join ' | '
    Write-Output "UIA names: $texts"

    if ($Scenario -eq 'Library') {
        Check "title '$title' visible via UIA" $found
        Check 'empty state absent' (-not ($names | Where-Object { $_.Name -eq 'No books yet' }))
    } else {
        Check 'blocking "Encrypted items can''t be unlocked" page visible' $found
        $buttons = @($names | Where-Object { $_.Type -eq 'ControlType.Button' -and $_.Name -and $_.Name -notin @('Minimize', 'Maximize', 'Restore', 'Close', 'Back') })
        Write-Output ("buttons: " + (($buttons | ForEach-Object { $_.Name }) -join ', '))
        Check 'Retry offered' ([bool]($buttons | Where-Object { $_.Name -eq 'Retry' }))
        Check 'no Delete/Reset/Regenerate action (buttons)' (-not ($buttons | Where-Object { $_.Name -match 'delete|reset|regenerate|erase|clear' }))
        Check 'no raw exception/path text' (-not ($names | Where-Object { $_.Name -match 'Exception|panic|[A-Za-z]:\\' }))
    }

    $diag = Join-Path $root 'diag.log'
    if (Test-Path $diag) { Write-Output "diag.log: $(Get-Content $diag -Raw)" }
    if ($Scenario -eq 'Library') {
        Check 'diag.log reports state=Ready items=1' ((Test-Path $diag) -and ((Get-Content $diag -Raw) -match 'state=Ready items=1'))
    } else {
        Check 'diag.log reports state=KeyStoreCorrupt' ((Test-Path $diag) -and ((Get-Content $diag -Raw) -match 'state=KeyStoreCorrupt'))
    }

    $null = $p.CloseMainWindow()
    if (-not $p.WaitForExit(8000)) { $p.Kill(); Check 'clean exit' $false } else { Check 'clean exit' ($p.ExitCode -eq 0) "code=$($p.ExitCode)" }

    if ($Scenario -eq 'Corrupt') {
        $hashAfter = (Get-FileHash $keyFile -Algorithm SHA256).Hash
        Write-Output "key sha256 before=$hashBefore after=$hashAfter"
        Check 'key file bytes unchanged' ($hashBefore -eq $hashAfter)
        Check 'no other key files created' ((Get-ChildItem (Join-Path $root 'keys')).Count -eq 1)
    }
}
finally {
    Remove-Item Env:GIST_DATA_ROOT -ErrorAction SilentlyContinue
    if ($p -and -not $p.HasExited) { $p.Kill() }
    Remove-Item -Recurse -Force $root -ErrorAction SilentlyContinue
}
if ($failed) { exit 1 } else { exit 0 }
