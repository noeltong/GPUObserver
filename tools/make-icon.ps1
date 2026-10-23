param(
    [string]$Output = (Join-Path $PSScriptRoot "..\GPUObserver\Assets\app.ico"),
    [string]$Preview = (Join-Path $env:TEMP "gpuobserver-icon-preview.png")
)

Add-Type -AssemblyName System.Drawing

function New-RoundedPath([System.Drawing.RectangleF]$Rect, [float]$Radius) {
    $path = New-Object System.Drawing.Drawing2D.GraphicsPath
    $d = $Radius * 2
    if ($d -le 0) {
        $path.AddRectangle($Rect)
        return $path
    }
    $path.AddArc($Rect.X, $Rect.Y, $d, $d, 180, 90)
    $path.AddArc($Rect.Right - $d, $Rect.Y, $d, $d, 270, 90)
    $path.AddArc($Rect.Right - $d, $Rect.Bottom - $d, $d, $d, 0, 90)
    $path.AddArc($Rect.X, $Rect.Bottom - $d, $d, $d, 90, 90)
    $path.CloseFigure()
    return $path
}

function New-AppBitmap([int]$Size) {
    $bmp = New-Object System.Drawing.Bitmap($Size, $Size, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    $g.Clear([System.Drawing.Color]::Transparent)

    $pad = [Math]::Max(1.0, $Size * 0.035)
    $bgRect = New-Object System.Drawing.RectangleF($pad, $pad, ($Size - 2 * $pad), ($Size - 2 * $pad))
    $bgPath = New-RoundedPath $bgRect ([float]($Size * 0.22))
    $bgBrush = New-Object System.Drawing.Drawing2D.LinearGradientBrush(
        $bgRect,
        [System.Drawing.Color]::FromArgb(255, 35, 99, 235),
        [System.Drawing.Color]::FromArgb(255, 11, 36, 92),
        [System.Drawing.Drawing2D.LinearGradientMode]::ForwardDiagonal)
    $g.FillPath($bgBrush, $bgPath)

    $chipW = $Size * 0.50
    $chipH = $Size * 0.40
    $chipRect = New-Object System.Drawing.RectangleF((($Size - $chipW) / 2.0), (($Size - $chipH) / 2.0), $chipW, $chipH)
    $chipPath = New-RoundedPath $chipRect ([float]($Size * 0.075))
    $chipBrush = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(255, 246, 250, 255))
    $g.FillPath($chipBrush, $chipPath)

    $pinBrush = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(255, 246, 250, 255))
    $pinLen = $Size * 0.085
    $pinW = [Math]::Max(1.0, $Size * 0.030)
    $pinYs = @(0.33, 0.50, 0.67)
    foreach ($t in $pinYs) {
        $y = [float]($Size * $t - $pinW / 2.0)
        $g.FillRectangle($pinBrush, [float]($chipRect.X - $pinLen), $y, [float]$pinLen, [float]$pinW)
        $g.FillRectangle($pinBrush, [float]$chipRect.Right, $y, [float]$pinLen, [float]$pinW)
    }

    $linePen = New-Object System.Drawing.Pen([System.Drawing.Color]::FromArgb(255, 16, 185, 129), [Math]::Max(1.2, $Size * 0.055))
    $linePen.StartCap = [System.Drawing.Drawing2D.LineCap]::Round
    $linePen.EndCap = [System.Drawing.Drawing2D.LineCap]::Round
    $linePen.LineJoin = [System.Drawing.Drawing2D.LineJoin]::Round
    $points = @(
        (New-Object System.Drawing.PointF([float]($Size * 0.32), [float]($Size * 0.55))),
        (New-Object System.Drawing.PointF([float]($Size * 0.43), [float]($Size * 0.55))),
        (New-Object System.Drawing.PointF([float]($Size * 0.49), [float]($Size * 0.42))),
        (New-Object System.Drawing.PointF([float]($Size * 0.58), [float]($Size * 0.66))),
        (New-Object System.Drawing.PointF([float]($Size * 0.64), [float]($Size * 0.50))),
        (New-Object System.Drawing.PointF([float]($Size * 0.69), [float]($Size * 0.50)))
    )
    $g.DrawLines($linePen, $points)

    $shine = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(38, 255, 255, 255))
    $g.FillEllipse($shine, [float](-$Size * 0.30), [float](-$Size * 0.55), [float]($Size * 1.25), [float]($Size * 0.95))

    $g.Dispose()
    return $bmp
}

$sizes = @(16, 20, 24, 32, 40, 48, 64, 128, 256)
$frames = @()
foreach ($size in $sizes) {
    $bmp = New-AppBitmap $size
    $stream = New-Object System.IO.MemoryStream
    $bmp.Save($stream, [System.Drawing.Imaging.ImageFormat]::Png)
    $frames += ,@($size, $stream.ToArray())
    if ($size -eq 256) {
        $bmp.Save($Preview, [System.Drawing.Imaging.ImageFormat]::Png)
    }
    $stream.Dispose()
    $bmp.Dispose()
}

$dir = Split-Path -Parent $Output
if (-not (Test-Path -LiteralPath $dir)) {
    New-Item -ItemType Directory -Path $dir | Out-Null
}

$file = [System.IO.File]::Open($Output, [System.IO.FileMode]::Create)
$writer = New-Object System.IO.BinaryWriter($file)
$writer.Write([UInt16]0)
$writer.Write([UInt16]1)
$writer.Write([UInt16]$frames.Count)

$offset = 6 + 16 * $frames.Count
foreach ($frame in $frames) {
    $size = $frame[0]
    $data = $frame[1]
    $dim = if ($size -ge 256) { 0 } else { $size }
    $writer.Write([byte]$dim)
    $writer.Write([byte]$dim)
    $writer.Write([byte]0)
    $writer.Write([byte]0)
    $writer.Write([UInt16]1)
    $writer.Write([UInt16]32)
    $writer.Write([UInt32]$data.Length)
    $writer.Write([UInt32]$offset)
    $offset += $data.Length
}
foreach ($frame in $frames) {
    $writer.Write([byte[]]$frame[1])
}
$writer.Flush()
$writer.Close()
$file.Close()

Write-Output "Icon: $Output"
Write-Output "Preview: $Preview"
