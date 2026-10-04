# Regenerates the Windows app icon asset set from the real designed master.
#
# Master:  assets/a-windows-11-icon.png at the repo root (1024x1024, opaque RGB).
# Outputs (this directory):
#   GIST.ico              7 frames: 16/24/32/48/64/128/256, each a 32bpp RGBA PNG payload
#   StoreLogo.png         50x50     (Package.appxmanifest <Properties><Logo>)
#   Square44x44Logo.png   44x44     (uap:VisualElements Square44x44Logo)
#   Square150x150Logo.png 150x150   (uap:VisualElements Square150x150Logo)
#   Wide310x150Logo.png   310x150   (uap:DefaultTile Wide310x150Logo) - the 150x150 square art
#                                   centred on a #1C1C1E canvas, since the master is square and
#                                   Windows' wide tile is not.
#
# Design tokens come from docs/iconspecification.md (Windows 11 section: flat 2D, #1C1C1E
# background / #4361EE accent / #F4F4F0 surface, 256x256 .ico, embedded RGB).
#
# History: this script used to draw PLACEHOLDER artwork (an open book + magnifier) with
# System.Drawing primitives. On 2026-09-30 a macOS session replaced its output with assets
# derived from the real master via `sips` + a one-off packer, which left the script misleading
# (re-running it would have overwritten the real artwork with the placeholder). It now derives
# from the master itself, so the committed assets are reproducible rather than opaque binaries.
#
# That rewrite also fixed a real defect in the 2026-09-30 ICO: `sips` emitted PNG colour type 2
# (truecolour, NO alpha channel) because the master has no alpha, while each ICONDIRENTRY
# declared wBitCount=32. Windows' WIC ICO codec rejects a PNG-payload ICO frame that has no
# alpha channel - `BitmapDecoder.Create` / `IconBitmapDecoder` failed with "The image decoder
# cannot decode the image" on the whole file, even though the legacy GDI+ loader
# (System.Drawing.Icon, which is what <ApplicationIcon> and the shell use) accepted it. Frames
# are now rendered as Format32bppArgb and saved as colour-type-6 RGBA PNGs, which both decoders
# accept. Keep it that way: do not "optimise" the frames back to 24bpp RGB.
#
# Usage (from anywhere): pwsh -File generate-assets.ps1
#   -Master <path>  override the master image
#   -OutDir <path>  write elsewhere (e.g. to diff against the committed assets)

[CmdletBinding()]
param(
    [string]$Master,
    [string]$OutDir
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

if (-not $OutDir) { $OutDir = $PSScriptRoot }
if (-not $Master) {
    # this script lives at apps/windows/GIST.App/Assets -> four levels up is the repo root.
    $Master = Join-Path $PSScriptRoot '..' '..' '..' '..' | Join-Path -ChildPath 'assets/a-windows-11-icon.png'
}
if (-not (Test-Path -LiteralPath $Master)) {
    throw "Master icon not found: $Master (expected the 1024x1024 designed master at assets/a-windows-11-icon.png)"
}
$Master = (Resolve-Path -LiteralPath $Master).Path
$OutDir = (Resolve-Path -LiteralPath $OutDir).Path
Write-Verbose "master = $Master"
Write-Verbose "outdir = $OutDir"

$BackgroundToken = '#1C1C1E'   # docs/iconspecification.md, Primary Dark Background
$IcoSizes        = 16, 24, 32, 48, 64, 128, 256

# NB: not $master - PowerShell variable names are case-insensitive, so $master would alias the
# $Master parameter and a failed Bitmap construction would leave a String here, whose Dispose()
# in the finally below masks the real exception.
$masterBmp = New-Object System.Drawing.Bitmap -ArgumentList $Master
try {
    if ($masterBmp.Width -ne $masterBmp.Height) {
        throw "Master is $($masterBmp.Width)x$($masterBmp.Height); a square master is assumed by the square tile logos."
    }

    # Square downscale of the master. Format32bppArgb so PNG encodes as colour type 6 (RGBA) -
    # required for the ICO frames (see the header note) and harmless for the standalone tiles.
#
    # The ImageAttributes/TileFlipXY wrap mode is load-bearing, not decoration. A fresh
    # Format32bppArgb bitmap starts fully TRANSPARENT, and HighQualityBicubic's resampling kernel
    # reaches outside the source rectangle at the image border, so without it GDI+ blends the
    # transparent canvas into the outermost pixels: the first cut of this script produced edge
    # pixels at alpha 220-243 instead of 255, i.e. a faded ghost border around every asset, where
    # the master (and the assets this replaced) are fully opaque. TileFlipXY makes the kernel
    # mirror real source pixels instead. The explicit source/destination rectangle overload is
    # required for ImageAttributes to apply at all. Assert-Opaque below guards the regression.
    function New-Square([int]$Size) {
        $bmp = New-Object System.Drawing.Bitmap $Size, $Size, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
        $g = [System.Drawing.Graphics]::FromImage($bmp)
        $attr = New-Object System.Drawing.Imaging.ImageAttributes
        try {
            $g.InterpolationMode = 'HighQualityBicubic'
            $g.PixelOffsetMode   = 'HighQuality'
            $g.SmoothingMode     = 'HighQuality'
            $g.CompositingMode   = 'SourceCopy'
            $attr.SetWrapMode([System.Drawing.Drawing2D.WrapMode]::TileFlipXY)
            $g.DrawImage($masterBmp,
                (New-Object System.Drawing.Rectangle 0, 0, $Size, $Size),
                0, 0, $masterBmp.Width, $masterBmp.Height,
                [System.Drawing.GraphicsUnit]::Pixel,
                $attr)
        } finally { $g.Dispose(); $attr.Dispose() }
        return $bmp
    }

    # Every asset must be fully opaque: the master has no alpha channel, and a partially
    # transparent icon edge reads as a rendering bug on a taskbar or a Start tile.
    function Assert-Opaque([System.Drawing.Bitmap]$Bitmap, [string]$What) {
        for ($y = 0; $y -lt $Bitmap.Height; $y++) {
            for ($x = 0; $x -lt $Bitmap.Width; $x++) {
                $a = $Bitmap.GetPixel($x, $y).A
                if ($a -ne 255) { throw "$What is not fully opaque: pixel ($x,$y) has alpha $a (expected 255)." }
            }
        }
    }

    function Save-Png([System.Drawing.Bitmap]$Bitmap, [string]$Name) {
        $path = Join-Path $OutDir $Name
        $Bitmap.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
        Write-Output ("  {0,-22} {1}x{2}" -f $Name, $Bitmap.Width, $Bitmap.Height)
    }

    Write-Output "Writing tile logos:"
    foreach ($spec in @(@{ n = 'StoreLogo.png'; s = 50 }, @{ n = 'Square44x44Logo.png'; s = 44 })) {
        $b = New-Square $spec.s
        try { Save-Png $b $spec.n } finally { $b.Dispose() }
    }

    # One 150x150 render is reused for BOTH the square tile and the wide tile's centre band, so
    # Wide310x150Logo's centre is pixel-identical to Square150x150Logo by construction.
    $square150 = New-Square 150
    try {
        Save-Png $square150 'Square150x150Logo.png'

        $wide = New-Object System.Drawing.Bitmap 310, 150, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
        try {
            $g = [System.Drawing.Graphics]::FromImage($wide)
            try {
                $pad = [System.Drawing.ColorTranslator]::FromHtml($BackgroundToken)
                $g.Clear($pad)
                # A 1:1 pixel blit of the 150x150 render, centred => x = (310-150)/2 = 80.
                # Explicit source/destination rectangles in GraphicsUnit.Pixel, with SourceCopy:
                # DrawImageUnscaled() honours the source bitmap's DPI metadata rather than copying
                # pixel-for-pixel, which silently resamples the centre band (caught by this script's
                # own centre-band self-check below), and the default SourceOver compositing would
                # blend rather than replace.
                $g.CompositingMode = 'SourceCopy'
                $g.InterpolationMode = 'NearestNeighbor'
                $x0 = [int](($wide.Width - 150) / 2)
                $g.DrawImage($square150,
                    (New-Object System.Drawing.Rectangle $x0, 0, 150, 150),
                    (New-Object System.Drawing.Rectangle 0, 0, 150, 150),
                    [System.Drawing.GraphicsUnit]::Pixel)
            } finally { $g.Dispose() }
            Save-Png $wide 'Wide310x150Logo.png'
        } finally { $wide.Dispose() }
    } finally { $square150.Dispose() }

    # --- GIST.ico -----------------------------------------------------------------------------
    # PowerShell trap: `return $ms.ToArray()` from a function unrolls the byte[] into object[],
    # and BinaryWriter.Write(object[]) then binds the char[] overload and silently writes
    # garbage. Every payload stays strictly [byte[]] below, and `, [byte[]]` wraps the return so
    # the pipeline cannot unroll it.
    function Get-PngBytes([System.Drawing.Bitmap]$Bitmap) {
        $ms = New-Object System.IO.MemoryStream
        try {
            $Bitmap.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
            return , [byte[]]$ms.ToArray()
        } finally { $ms.Dispose() }
    }

    $payloads = New-Object System.Collections.ArrayList
    foreach ($s in $IcoSizes) {
        $b = New-Square $s
        try { [void]$payloads.Add((Get-PngBytes $b)) } finally { $b.Dispose() }
    }

    $icoPath = Join-Path $OutDir 'GIST.ico'
    $fs = [System.IO.File]::Create($icoPath)
    try {
        $bw = New-Object System.IO.BinaryWriter $fs
        # ICONDIR: idReserved=0, idType=1 (icon), idCount
        $bw.Write([uint16]0); $bw.Write([uint16]1); $bw.Write([uint16]$IcoSizes.Count)
        $offset = 6 + 16 * $IcoSizes.Count
        for ($i = 0; $i -lt $IcoSizes.Count; $i++) {
            $s = $IcoSizes[$i]
            $len = ([byte[]]$payloads[$i]).Length
            # ICONDIRENTRY. 256 is encoded as 0 in the single width/height bytes.
            $dim = if ($s -ge 256) { 0 } else { $s }
            $bw.Write([byte]$dim)      # bWidth
            $bw.Write([byte]$dim)      # bHeight
            $bw.Write([byte]0)         # bColorCount (0 = not palettised)
            $bw.Write([byte]0)         # bReserved
            $bw.Write([uint16]1)       # wPlanes
            $bw.Write([uint16]32)      # wBitCount - matches the RGBA payloads
            $bw.Write([uint32]$len)    # dwBytesInRes
            $bw.Write([uint32]$offset) # dwImageOffset
            $offset += $len
        }
        for ($i = 0; $i -lt $IcoSizes.Count; $i++) { $bw.Write([byte[]]$payloads[$i]) }
        $bw.Flush()
        $bw.Close()
    } finally { $fs.Dispose() }
    Write-Output ("  {0,-22} {1} frames: {2}" -f 'GIST.ico', $IcoSizes.Count, ($IcoSizes -join '/'))

    # --- self-check ---------------------------------------------------------------------------
    # Fail loudly rather than leaving a silently-malformed icon on disk, which is exactly how the
    # 2026-09-30 no-alpha ICO survived until a Windows session looked at it.
    Write-Output "Verifying:"
    $bytes = [System.IO.File]::ReadAllBytes($icoPath)
    $count = [BitConverter]::ToUInt16($bytes, 4)
    if ($count -ne $IcoSizes.Count) { throw "GIST.ico declares $count frames, expected $($IcoSizes.Count)." }
    for ($i = 0; $i -lt $count; $i++) {
        $o = 6 + 16 * $i
        $len = [BitConverter]::ToUInt32($bytes, $o + 8)
        $off = [BitConverter]::ToUInt32($bytes, $o + 12)
        if ($off + $len -gt $bytes.Length) { throw "GIST.ico frame $i runs past EOF." }
        if (-not ($bytes[$off] -eq 0x89 -and $bytes[$off + 1] -eq 0x50)) { throw "GIST.ico frame $i is not a PNG payload." }
        # IHDR colour type must be 6 (RGBA); 2 (RGB, no alpha) is what WIC rejects.
        $colourType = $bytes[$off + 8 + 17]
        if ($colourType -ne 6) { throw "GIST.ico frame $i has PNG colour type $colourType; WIC requires 6 (RGBA) for ICO frames." }
    }
    Write-Output "  ICO structure OK (7 PNG frames, all colour type 6/RGBA)"

    # Legacy GDI+ loader - the path <ApplicationIcon> and the shell use.
    $f = [System.IO.File]::OpenRead($icoPath)
    try {
        $icon = New-Object System.Drawing.Icon $f
        try { Write-Output "  System.Drawing.Icon loads OK (default $($icon.Size.Width)x$($icon.Size.Height))" }
        finally { $icon.Dispose() }
    } finally { $f.Dispose() }

    # WIC - the path WinUI/XAML and modern shell surfaces use. This is the check that the
    # 2026-09-30 asset failed.
    Add-Type -AssemblyName PresentationCore
    Add-Type -AssemblyName WindowsBase
    $f2 = [System.IO.File]::OpenRead($icoPath)
    try {
        $dec = [System.Windows.Media.Imaging.BitmapDecoder]::Create(
            $f2,
            [System.Windows.Media.Imaging.BitmapCreateOptions]::None,
            [System.Windows.Media.Imaging.BitmapCacheOption]::OnLoad)
        $got = @($dec.Frames | ForEach-Object { $_.PixelWidth }) | Sort-Object
        $want = $IcoSizes | Sort-Object
        if (($got -join ',') -ne ($want -join ',')) { throw "WIC sees frames $($got -join ','), expected $($want -join ',')." }
        Write-Output "  WIC decodes OK ($($dec.Frames.Count) frames: $($got -join ','))"
    } finally { $f2.Dispose() }

    foreach ($spec in @(@{ n = 'StoreLogo.png'; w = 50; h = 50 }, @{ n = 'Square44x44Logo.png'; w = 44; h = 44 },
                        @{ n = 'Square150x150Logo.png'; w = 150; h = 150 }, @{ n = 'Wide310x150Logo.png'; w = 310; h = 150 })) {
        $b = New-Object System.Drawing.Bitmap (Join-Path $OutDir $spec.n)
        try {
            if ($b.Width -ne $spec.w -or $b.Height -ne $spec.h) {
                throw "$($spec.n) is $($b.Width)x$($b.Height), expected $($spec.w)x$($spec.h)."
            }
            Assert-Opaque $b $spec.n
        } finally { $b.Dispose() }
    }
    Write-Output "  tile logo dimensions OK, all fully opaque"

    # Same opacity requirement for every ICO frame, read back from the written file.
    for ($i = 0; $i -lt $count; $i++) {
        $o = 6 + 16 * $i
        $len = [BitConverter]::ToUInt32($bytes, $o + 8)
        $off = [BitConverter]::ToUInt32($bytes, $o + 12)
        $ms = New-Object System.IO.MemoryStream (, [byte[]]$bytes[$off..($off + $len - 1)])
        try {
            $fr = New-Object System.Drawing.Bitmap $ms
            try { Assert-Opaque $fr "GIST.ico frame $($fr.Width)x$($fr.Height)" } finally { $fr.Dispose() }
        } finally { $ms.Dispose() }
    }
    Write-Output "  all ICO frames fully opaque"

    # The wide tile's padding must be exactly the background token, and its centre band must be
    # the square tile verbatim.
    $wideChk = New-Object System.Drawing.Bitmap (Join-Path $OutDir 'Wide310x150Logo.png')
    $sqChk   = New-Object System.Drawing.Bitmap (Join-Path $OutDir 'Square150x150Logo.png')
    try {
        $expected = [System.Drawing.ColorTranslator]::FromHtml($BackgroundToken)
        for ($x = 0; $x -lt $wideChk.Width; $x++) {
            if ($x -ge 80 -and $x -lt 230) { continue }
            for ($y = 0; $y -lt $wideChk.Height; $y++) {
                $c = $wideChk.GetPixel($x, $y)
                if ($c.R -ne $expected.R -or $c.G -ne $expected.G -or $c.B -ne $expected.B) {
                    throw "Wide310x150Logo padding at ($x,$y) is #$('{0:X2}{1:X2}{2:X2}' -f $c.R,$c.G,$c.B), expected $BackgroundToken."
                }
            }
        }
        for ($y = 0; $y -lt 150; $y++) {
            for ($x = 0; $x -lt 150; $x++) {
                $a = $sqChk.GetPixel($x, $y); $b2 = $wideChk.GetPixel(80 + $x, $y)
                if ($a.R -ne $b2.R -or $a.G -ne $b2.G -or $a.B -ne $b2.B) {
                    throw "Wide310x150Logo centre band differs from Square150x150Logo at ($x,$y)."
                }
            }
        }
        Write-Output "  wide tile: padding is $BackgroundToken, centre band (x=80..229) matches Square150x150Logo"
    } finally { $wideChk.Dispose(); $sqChk.Dispose() }
}
finally { $masterBmp.Dispose() }

Write-Output "assets generated"
