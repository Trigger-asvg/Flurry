# Тёмное окно-подложка (без активации, не крадёт фокус) — чтобы на скриншотах были видны светлые частицы.
param([int]$X = 1960, [int]$Y = 60, [int]$W = 700, [int]$H = 460, [int]$Seconds = 60)
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type -ReferencedAssemblies System.Windows.Forms, System.Drawing @"
using System; using System.Windows.Forms; using System.Drawing;
public class Backdrop : Form {
  protected override bool ShowWithoutActivation { get { return true; } }
  protected override CreateParams CreateParams { get { var cp = base.CreateParams; cp.ExStyle |= 0x08000000 | 0x80; return cp; } }
  public Backdrop(int x, int y, int w, int h) {
    FormBorderStyle = FormBorderStyle.None; StartPosition = FormStartPosition.Manual;
    Bounds = new Rectangle(x, y, w, h); BackColor = Color.FromArgb(18, 20, 28); ShowInTaskbar = false;
  }
}
"@
$f = New-Object Backdrop $X, $Y, $W, $H
$t = New-Object System.Windows.Forms.Timer; $t.Interval = $Seconds * 1000; $t.add_Tick({ $f.Close() }); $t.Start()
[System.Windows.Forms.Application]::Run($f)
