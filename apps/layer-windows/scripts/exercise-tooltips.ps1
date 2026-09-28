param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyWaitSeconds=10
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
[CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path;$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/tooltips/'+[Guid]::NewGuid().ToString('N'));[IO.Directory]::CreateDirectory($run)|Out-Null
function Tooltip {
 $condition=[System.Windows.Automation.AndCondition]::new(
  [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::ToolTip),
  [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty,$review.Id))
 $found=[System.Windows.Automation.AutomationElement]::RootElement.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$condition)
 if(!$found){$found=$root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$condition)}
 $found
}
function Center([string]$Id,[switch]$Name){
 $property=if($Name){[System.Windows.Automation.AutomationElement]::NameProperty}else{[System.Windows.Automation.AutomationElement]::AutomationIdProperty}
 $element=$root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new($property,$Id))
 if(!$element){throw "Missing control: $Id"}
 $r=$element.Current.BoundingRectangle;@{x=[int]($r.X+$r.Width/2);y=[int]($r.Y+$r.Height/2);name=$element.Current.Name}
}
try{
 Enter-CapyEnvironment
 $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
 $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError (Join-Path $run 'stderr.log');$null=$review.Handle
 Write-Output "Owned tooltip review $($review.Id): $run"
 Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero -and (Model).brush_ready} 'Tooltip review did not start' 45
 $root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
 $root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Maximized)
 Start-Sleep -Milliseconds 800
 [CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)|Out-Null;[CapyRowPointer]::Initialize([uint32]$review.Id)
 $panel=@((Model).panels|Where-Object {$_.tiles.Count -gt 0})[0]
 $target=Center ("tile-"+$panel.id+"-"+$panel.tiles[0].id)
 $away=Center 'drawing-canvas'
 [CapyRowPointer]::Hover($target.x,$target.y)
 Wait-Until {Tooltip} "Mouse hover did not show the $($target.name) tooltip" 5
 [CapyRowPointer]::Hover($away.x,$away.y)
 Wait-Until {!(Tooltip)} 'Tooltip did not dismiss when the mouse left'
 (Find 'drawing-canvas').SetFocus()
 [CapyRowPointer]::Key([uint32]$review.Id,[ushort]0x4F)
 Wait-Until {((Model).state.commands|Where-Object id -eq 'move').selected} 'O did not choose Move'
 $hint=((Model).state.commands|Where-Object id -eq 'move').tooltip
 if($hint -notmatch '\(.+\)$'){throw "Move has no shortcut in its command tooltip: $hint"}
 $move=@{item=$null}
 Wait-Until {$move.item=@($root.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,'Move'))|Where-Object {$_.Current.AutomationId -match '^tool-(subtool|group)-' -and !$_.Current.IsOffscreen})[0];$move.item} 'Tool Set did not list the Move subtool'
 $r=$move.item.Current.BoundingRectangle
 [CapyRowPointer]::Hover($away.x,$away.y);Start-Sleep -Milliseconds 100
 for($step=0;$step -lt 4;$step++){[CapyRowPointer]::Hover([int]($r.X+$r.Width/2)+$step,[int]($r.Y+$r.Height/2));Start-Sleep -Milliseconds 80}
 $shown=@{text=''}
 $expected='Move'+$hint.Substring($hint.IndexOf(' ('))
 try{Wait-Until {$tip=Tooltip;$shown.text=if($tip){$tip.Current.Name}else{''};$shown.text -eq $expected} 'shortcut' 5}catch{throw "Hovering Move showed '$($shown.text)', expected '$expected'"}
 [CapyRowPointer]::Hover($away.x,$away.y)
 Wait-Until {!(Tooltip)} 'The shortcut tooltip did not dismiss'
 [CapyRowPointer]::Down('touch',$target.x,$target.y)
 try{
  $watch=[Diagnostics.Stopwatch]::StartNew()
  while($watch.ElapsedMilliseconds -lt 1800){if(Tooltip){throw 'A touch hold opened a tooltip'};Start-Sleep -Milliseconds 40}
 }finally{[CapyRowPointer]::Up()}
 Start-Sleep -Milliseconds 400
 if(Tooltip){throw 'A tooltip appeared after the touch hold'}
 [CapyRowPointer]::Key([uint32]$review.Id,[ushort]0x1B)
 @{mouse_hover='passed';touch_hold_without_tooltip='passed';action_shortcut_hint=$shown.text;target=$target.name}|ConvertTo-Json|Set-Content (Join-Path $run 'results.json')
 Get-Content (Join-Path $run 'results.json')
}catch{
 if($review -and !$review.HasExited){try{Capture 'failure' -WithModel}catch{}}
 [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace)
 throw
}finally{
 [CapyRowPointer]::Dispose()
 if($review -and !$review.HasExited){Stop-Process -Id $review.Id -Force}
 Exit-CapyEnvironment
}
