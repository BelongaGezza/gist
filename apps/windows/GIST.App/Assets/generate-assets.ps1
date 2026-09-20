# Regenerates the app icon asset set (docs/iconspecification.md, Windows 11 section: flat 2D, tokens
# #1C1C1E / #4361EE / #F4F4F0). PLACEHOLDER ARTWORK: a simplified open book + magnifier drawn
# programmatically; replace with the designed vector master when it exists, then re-run.
# Usage (from anywhere): pwsh -File generate-assets.ps1
Add-Type -AssemblyName System.Drawing
$out = $PSScriptRoot

function New-Icon([int]$w, [int]$h) {
    $bmp = New-Object System.Drawing.Bitmap $w, $h, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = 'AntiAlias'
    $g.Clear([System.Drawing.Color]::Transparent)
    $s = [Math]::Min($w, $h)
    $ox = ($w - $s) / 2.0; $oy = ($h - $s) / 2.0
    $u = $s / 256.0
    $dark = [System.Drawing.ColorTranslator]::FromHtml('#1C1C1E')
    $indigo = [System.Drawing.ColorTranslator]::FromHtml('#4361EE')
    $cream = [System.Drawing.ColorTranslator]::FromHtml('#F4F4F0')
    # tile
    $r = 12 * $u
    $p = New-Object System.Drawing.Drawing2D.GraphicsPath
    $x0 = $ox; $y0 = $oy; $d = 2 * $r
    $p.AddArc($x0, $y0, $d, $d, 180, 90); $p.AddArc($x0 + $s - $d, $y0, $d, $d, 270, 90)
    $p.AddArc($x0 + $s - $d, $y0 + $s - $d, $d, $d, 0, 90); $p.AddArc($x0, $y0 + $s - $d, $d, $d, 90, 90)
    $p.CloseFigure()
    $g.FillPath((New-Object System.Drawing.SolidBrush $dark), $p)
    # open book: two cream pages
    $cb = New-Object System.Drawing.SolidBrush $cream
    $L = @(@(40,80),@(124,92),@(124,196),@(40,184))
    $R = @(@(132,92),@(216,80),@(216,184),@(132,196))
    foreach ($poly in @($L, $R)) {
        $pts = $poly | ForEach-Object { New-Object System.Drawing.PointF ($ox + $_[0] * $u), ($oy + $_[1] * $u) }
        $g.FillPolygon($cb, [System.Drawing.PointF[]]$pts)
    }
    # text lines on pages
    $pen = New-Object System.Drawing.Pen $dark, (6 * $u)
    foreach ($y in 112, 130, 148) {
        $g.DrawLine($pen, $ox + 54 * $u, $oy + ($y + 2) * $u, $ox + 110 * $u, $oy + ($y + 8) * $u)
    }
    # magnifier (indigo lens ring + handle) over lower right
    $ip = New-Object System.Drawing.Pen $indigo, (16 * $u)
    $ip.StartCap = 'Round'; $ip.EndCap = 'Round'
    $g.FillEllipse((New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(230, $dark))), $ox + 138 * $u, $oy + 118 * $u, 70 * $u, 70 * $u)
    $g.DrawEllipse($ip, $ox + 138 * $u, $oy + 118 * $u, 70 * $u, 70 * $u)
    $g.DrawLine($ip, $ox + 200 * $u, $oy + 180 * $u, $ox + 228 * $u, $oy + 212 * $u)
    $g.Dispose()
    return $bmp
}

function Save-Png([string]$name, [int]$w, [int]$h) {
    $b = New-Icon $w $h
    $b.Save((Join-Path $out $name), [System.Drawing.Imaging.ImageFormat]::Png)
    $b.Dispose()
}

Save-Png 'StoreLogo.png' 50 50
Save-Png 'Square44x44Logo.png' 44 44
Save-Png 'Square150x150Logo.png' 150 150
Save-Png 'Wide310x150Logo.png' 310 150

# .ico with PNG-compressed frames (16..256), written by hand.
$sizes = 16, 24, 32, 48, 64, 128, 256
$frames = foreach ($sz in $sizes) {
    $b = New-Icon $sz $sz
    $ms = New-Object System.IO.MemoryStream
    $b.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
    $b.Dispose()
    , $ms.ToArray()
}
$fs = [System.IO.File]::Create((Join-Path $out 'GIST.ico'))
$bw = New-Object System.IO.BinaryWriter $fs
$bw.Write([uint16]0); $bw.Write([uint16]1); $bw.Write([uint16]$sizes.Count)
$offset = 6 + 16 * $sizes.Count
for ($i = 0; $i -lt $sizes.Count; $i++) {
    $sz = $sizes[$i]; $wb = if ($sz -ge 256) { 0 } else { $sz }
    $bw.Write([byte]$wb); $bw.Write([byte]$wb); $bw.Write([byte]0); $bw.Write([byte]0)
    $bw.Write([uint16]1); $bw.Write([uint16]32)
    $bw.Write([uint32]$frames[$i].Length); $bw.Write([uint32]$offset)
    $offset += $frames[$i].Length
}
foreach ($f in $frames) { $bw.Write($f) }
$bw.Close(); $fs.Close()
Write-Output 'assets generated'
