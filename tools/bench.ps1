# Замер нагрузки процесса оверлея: среднее за N секунд (доля всего CPU и одного ядра) + память.
param([int]$Sec = 15)
$p = Get-Process Flurry -ErrorAction Stop | Where-Object { $_.MainWindowTitle -notlike '*Настройки*' } | Select-Object -First 1
$c0 = $p.TotalProcessorTime; [Threading.Thread]::Sleep($Sec * 1000); $p.Refresh()
$t = ($p.TotalProcessorTime - $c0).TotalSeconds / $Sec
"cpu: {0:0.00}% total, {1:0.0}% of one core, WS {2} MB, private {3} MB" -f ($t / [Environment]::ProcessorCount * 100), ($t * 100), [int]($p.WorkingSet64 / 1MB), [int]($p.PrivateMemorySize64 / 1MB)
