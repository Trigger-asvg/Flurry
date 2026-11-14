# Скриншот виртуального экрана вместе с layered-окнами (BitBlt + CAPTUREBLT) -> PNG.
param([string]$Out, [double]$Scale = 0.5, [int]$X = 0, [int]$Y = 0, [int]$W = 0, [int]$H = 0)
Add-Type -AssemblyName System.Drawing
Add-Type -ReferencedAssemblies System.Drawing @"
using System; using System.Drawing; using System.Runtime.InteropServices;
public static class Shot {
  [DllImport("user32.dll")] static extern bool SetProcessDpiAwarenessContext(IntPtr v);
  [DllImport("user32.dll")] static extern IntPtr GetDC(IntPtr h);
  [DllImport("user32.dll")] static extern int ReleaseDC(IntPtr h, IntPtr dc);
  [DllImport("user32.dll")] static extern int GetSystemMetrics(int i);
  [DllImport("gdi32.dll")] static extern bool BitBlt(IntPtr d, int x, int y, int w, int h, IntPtr s, int sx, int sy, int op);
  public static Bitmap Grab(int x, int y, int w, int h) {
    SetProcessDpiAwarenessContext(new IntPtr(-4));
    if (w == 0) { x = GetSystemMetrics(76); y = GetSystemMetrics(77); w = GetSystemMetrics(78); h = GetSystemMetrics(79); }
    var bmp = new Bitmap(w, h);
    using (var g = Graphics.FromImage(bmp)) {
      IntPtr dst = g.GetHdc(); IntPtr src = GetDC(IntPtr.Zero);
      BitBlt(dst, 0, 0, w, h, src, x, y, 0x00CC0020 | 0x40000000);
      ReleaseDC(IntPtr.Zero, src); g.ReleaseHdc(dst);
    }
    return bmp;
  }
}
"@
$bmp = [Shot]::Grab($X, $Y, $W, $H)
if ($Scale -ne 1) {
    $sw = [int]($bmp.Width * $Scale); $sh = [int]($bmp.Height * $Scale)
    $small = New-Object System.Drawing.Bitmap $sw, $sh
    $g2 = [System.Drawing.Graphics]::FromImage($small); $g2.InterpolationMode = 'HighQualityBicubic'
    $g2.DrawImage($bmp, 0, 0, $sw, $sh); $g2.Dispose(); $bmp.Dispose(); $bmp = $small
}
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
Write-Host "saved $Out"
