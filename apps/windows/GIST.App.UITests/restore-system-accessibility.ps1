# Restores the Windows text size and high-contrast switch that GIST.App.UITests' SystemAccessibility changed,
# if a test run was killed before it could. Reads the originals from the marker file the helper wrote first.
# Safe to run when nothing needs restoring (no marker = no-op).
$marker = Join-Path ([IO.Path]::GetTempPath()) 'gist-a11y-restore.txt'
if (-not (Test-Path $marker)) { Write-Host 'Nothing to restore.'; exit 0 }

$orig = @{}
Get-Content $marker | ForEach-Object { $k, $v = $_ -split '=', 2; $orig[$k] = $v }

Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class GistHc {
  [StructLayout(LayoutKind.Sequential)] public struct HIGHCONTRAST { public int cbSize; public int dwFlags; public IntPtr lpszDefaultScheme; }
  [DllImport("user32.dll", SetLastError=true)] static extern bool SystemParametersInfo(int a, int b, ref HIGHCONTRAST c, int d);
  public static bool Set(int flags) {
    var h = new HIGHCONTRAST(); h.cbSize = Marshal.SizeOf(h); h.dwFlags = flags; h.lpszDefaultScheme = Marshal.StringToHGlobalUni("High Contrast Black");
    try { return SystemParametersInfo(0x43, h.cbSize, ref h, 3); } finally { Marshal.FreeHGlobal(h.lpszDefaultScheme); }
  }
}
'@

[void][GistHc]::Set([int]$orig['hcFlags'])
$key = 'HKCU:\Software\Microsoft\Accessibility'
if ($orig['textScale'] -eq 'absent') { Remove-ItemProperty -Path $key -Name TextScaleFactor -ErrorAction SilentlyContinue }
else { Set-ItemProperty -Path $key -Name TextScaleFactor -Value ([int]$orig['textScale']) -Type DWord }
Remove-Item $marker
Write-Host 'Restored text size and high-contrast switch.'
