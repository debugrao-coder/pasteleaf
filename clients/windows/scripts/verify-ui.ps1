# Runs only the isolated synthetic demo. No real clipboard capture, paste, iCloud, or startup changes.
# Runtime evidence is meaningful only when this script executes on an interactive Windows desktop.
param(
  [Parameter(Mandatory=$true)][string]$Exe,
  [string]$Output = "$PSScriptRoot\..\..\..\artifacts\windows-ui",
  [ValidateSet('light','dark')][string]$Theme='light'
)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes,System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class PasteLeafCapture {
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out Rect rect);
  [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left,Top,Right,Bottom; }
}
'@
$exePath=(Resolve-Path $Exe).Path
New-Item -ItemType Directory -Force -Path $Output | Out-Null
$process=Start-Process -FilePath $exePath -ArgumentList @('--demo','--theme',$Theme) -PassThru
$checks=[System.Collections.Generic.List[object]]::new()
try {
  $condition=[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty,$process.Id)
  $window=$null
  for($i=0;$i -lt 80 -and !$window;$i++){
    Start-Sleep -Milliseconds 150
    $window=[System.Windows.Automation.AutomationElement]::RootElement.FindFirst([System.Windows.Automation.TreeScope]::Children,$condition)
    if($process.HasExited){throw "Demo exited before a window became available."}
  }
  if(!$window){throw 'No native UI Automation window provider found.'}
  $hwnd=[IntPtr]$window.Current.NativeWindowHandle
  [PasteLeafCapture]::SetForegroundWindow($hwnd) | Out-Null
  Start-Sleep -Milliseconds 1500
  function Get-Controls {
    $window.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition)
  }
  function Find-Name([string]$name){
    $window.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,$name))
  }
  function Invoke-Name([string]$name){
    $control=Find-Name $name
    if(!$control){throw "Missing accessible control: $name"}
    $invoke=$control.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern)
    $invoke.Invoke()
    Start-Sleep -Milliseconds 350
  }
  function Snapshot([string]$name){
    $r=[PasteLeafCapture+Rect]::new()
    if(![PasteLeafCapture]::GetWindowRect($hwnd,[ref]$r)){throw 'Window bounds unavailable.'}
    $bitmap=[System.Drawing.Bitmap]::new($r.Right-$r.Left,$r.Bottom-$r.Top)
    $graphics=[System.Drawing.Graphics]::FromImage($bitmap)
    try{
      $graphics.CopyFromScreen($r.Left,$r.Top,0,0,$bitmap.Size)
      $bitmap.Save((Join-Path $Output "$Theme-$name.png"),[System.Drawing.Imaging.ImageFormat]::Png)
    }finally{$graphics.Dispose();$bitmap.Dispose()}
  }
  $controls=Get-Controls
  $buttons=@($controls | Where-Object {$_.Current.ControlType -eq [System.Windows.Automation.ControlType]::Button})
  $edits=@($controls | Where-Object {$_.Current.ControlType -eq [System.Windows.Automation.ControlType]::Edit})
  if($buttons.Count -lt 8 -or $edits.Count -lt 1){throw 'Missing standard native button/edit accessibility providers.'}
  $checks.Add(@{name='native-uia-controls';status='PASS';buttons=$buttons.Count;edits=$edits.Count})
  Snapshot 'history'
  Invoke-Name '设置'
  if(!(Find-Name '保存设置')){throw 'Settings screen did not open.'}
  $checks.Add(@{name='open-settings-via-uia-invoke';status='PASS'})
  Snapshot 'settings'
  Invoke-Name '← 返回'
  Invoke-Name '同步'
  $passwords=@((Get-Controls) | Where-Object {$_.Current.ControlType -eq [System.Windows.Automation.ControlType]::Edit -and $_.Current.IsPassword})
  if($passwords.Count -ne 1){throw 'Synchronization password must have native IsPassword semantics.'}
  $checks.Add(@{name='native-password-control';status='PASS'})
  Snapshot 'sync'
  Invoke-Name '← 返回'
  $search=@((Get-Controls) | Where-Object {$_.Current.ControlType -eq [System.Windows.Automation.ControlType]::Edit})[0]
  $value=$search.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern)
  $value.SetValue('NO-MATCH-SYNTHETIC-314159')
  Start-Sleep -Milliseconds 750
  Snapshot 'empty-search'
  $checks.Add(@{name='search-via-uia-value';status='PASS'})
  @{
    status='PASS_SCOPED';theme=$Theme;time=(Get-Date).ToUniversalTime().ToString('o');
    os=[Environment]::OSVersion.VersionString;exe=$exePath;
    checks=$checks;boundary='Synthetic demo only. Screenshots require human visual review. This does not verify clipboard injection, iCloud, all DPI modes, or accessibility with a screen reader.'
  } | ConvertTo-Json -Depth 8 | Set-Content -Encoding UTF8 (Join-Path $Output "$Theme-result.json")
} catch {
  @{status='HOLD';error=$_.Exception.Message;checks=$checks} | ConvertTo-Json -Depth 8 | Set-Content -Encoding UTF8 (Join-Path $Output "$Theme-result.json")
  throw
} finally {
  # Terminate only the demo process launched by this script, never an existing user app.
  if(!$process.HasExited){Stop-Process -Id $process.Id}
}
