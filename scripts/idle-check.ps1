# Idle budget check. Launches the release build, lets it settle, then
# measures what neptune costs the machine while doing nothing: memory across
# neptune.exe and every WebView2 process it owns, and CPU over a quiet window.
# Exits non-zero if it is over budget. Run before every release.
#
#   powershell -ExecutionPolicy Bypass -File scripts\idle-check.ps1
#
# Budget: idle means under 60 MB and under 0.5% of one core.

param(
  [string]$Exe = "$PSScriptRoot\..\src-tauri\target\release\neptune.exe",
  [int]$SettleSecs = 20,
  [int]$WindowSecs = 30,
  [int]$BudgetMB = 60,
  [double]$BudgetCpuPct = 0.5
)

function Owned {
  $ids = @()
  $b = Get-Process neptune -ErrorAction SilentlyContinue
  if ($b) { $ids += $b.Id }
  $wv = Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'" | Where-Object { $_.CommandLine -like '*app.blurt*' }
  foreach ($p in $wv) { $ids += $p.ProcessId }
  $ids | ForEach-Object { Get-Process -Id $_ -ErrorAction SilentlyContinue }
}

Get-Process neptune -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 800
Start-Process (Resolve-Path $Exe)
Start-Sleep -Seconds $SettleSecs

$before = Owned
$cpu0 = ($before | Measure-Object CPU -Sum).Sum
Start-Sleep -Seconds $WindowSecs
$after = Owned
$cpu1 = ($after | Measure-Object CPU -Sum).Sum

$mb = [math]::Round((($after | Measure-Object WorkingSet64 -Sum).Sum) / 1MB)
$cpuPct = [math]::Round(100 * ($cpu1 - $cpu0) / $WindowSecs, 2)

foreach ($p in $after) {
  '  {0,-20} {1,6:N0} MB' -f $p.ProcessName, ($p.WorkingSet64 / 1MB)
}
'idle: {0} MB across {1} processes, {2}% of one core' -f $mb, @($after).Count, $cpuPct

$ok = ($mb -le $BudgetMB) -and ($cpuPct -le $BudgetCpuPct)
if ($ok) { 'PASS (budget {0} MB, {1}%)' -f $BudgetMB, $BudgetCpuPct; exit 0 }
'FAIL (budget {0} MB, {1}%)' -f $BudgetMB, $BudgetCpuPct; exit 1
