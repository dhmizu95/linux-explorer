param([string]$Out = "$PSScriptRoot\uia_tree.txt", [int]$MaxDepth = 14)
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
$target = "$env:TEMP\uia_probe"
New-Item -ItemType Directory -Force $target | Out-Null
"hello" | Set-Content "$target\sample.txt"
New-Item -ItemType Directory -Force "$target\Sub Folder" | Out-Null
Start-Process explorer.exe $target
Start-Sleep -Seconds 4
$root = [System.Windows.Automation.AutomationElement]::RootElement
$cond = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ClassNameProperty, "CabinetWClass")
$win = $root.FindFirst([System.Windows.Automation.TreeScope]::Children, $cond)
if (-not $win) { "No explorer window found"; exit 1 }
$walker = [System.Windows.Automation.TreeWalker]::ControlViewWalker
$sb = New-Object System.Text.StringBuilder
function Walk($el, $depth) {
  if ($depth -gt $MaxDepth) { return }
  try {
    $c = $el.Current
    $r = $c.BoundingRectangle
    [void]$sb.AppendLine(("{0}{1} [{2}] name='{3}' aid='{4}' cls='{5}' rect={6},{7},{8}x{9}" -f ('  ' * $depth), $c.ControlType.ProgrammaticName.Replace('ControlType.',''), $c.LocalizedControlType, $c.Name, $c.AutomationId, $c.ClassName, [int]$r.X, [int]$r.Y, [int]$r.Width, [int]$r.Height))
  } catch { return }
  $child = $walker.GetFirstChild($el)
  while ($child) { Walk $child ($depth + 1); $child = $walker.GetNextSibling($child) }
}
Walk $win 0
$sb.ToString() | Set-Content -Encoding utf8 $Out
"Wrote $Out ($($sb.Length) chars)"
