param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
[CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path;$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/tooltips/'+[Guid]::NewGuid().ToString('N'));[IO.Directory]::CreateDirectory($run)|Out-Null
$names=@('CAPY_SETTINGS_DIRECTORY','CAPY_TRACE_UI','CAPY_SMOKE_TEST','CAPY_TEST_DISPLAY','CAPY_TEST_PRIMARY')
$previous=@{};foreach($name in $names){$previous[$name]=[Environment]::GetEnvironmentVariable($name,'Process')}
function Model {try{$s=Get-Content -LiteralPath (Join-Path $directory 'ui-state.json') -Raw|ConvertFrom-Json;if($s.process_id -eq $app.Id -and $s.model.windows_isolated_settings){$s.model}}catch{}}
function Wait-Until([scriptblock]$Condition,[string]$Message,[int]$Seconds=10){
 $watch=[Diagnostics.Stopwatch]::StartNew()
 do{if(& $Condition){return};$app.Refresh();if($app.HasExited){throw 'Owned tooltip review exited'};Start-Sleep -Milliseconds 60}while($watch.Elapsed.TotalSeconds -lt $Seconds)
 throw $Message
}
function Tooltip {
 $condition=[System.Windows.Automation.AndCondition]::new(
  [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::ToolTip),
  [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty,$app.Id))
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
 foreach($name in $names){Remove-Item -LiteralPath ('Env:'+$name) -ErrorAction SilentlyContinue}
 $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
 $app=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError (Join-Path $run 'stderr.log');$null=$app.Handle
 Write-Output "Owned tooltip review $($app.Id): $run"
 Wait-Until {$app.Refresh();$app.MainWindowHandle -ne [IntPtr]::Zero -and (Model).brush_ready} 'Tooltip review did not start' 45
 $root=[System.Windows.Automation.AutomationElement]::FromHandle($app.MainWindowHandle)
 $root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Maximized)
 Start-Sleep -Milliseconds 800
 [CapyRowPointer]::SetForegroundWindow($app.MainWindowHandle)|Out-Null;[CapyRowPointer]::Initialize([uint32]$app.Id)
 $panel=@((Model).panels|Where-Object {$_.tiles.Count -gt 0})[0]
 $target=Center ("tile-"+$panel.id+"-"+$panel.tiles[0].id)
 $away=Center 'Drawing canvas' -Name
 [CapyRowPointer]::Hover($target.x,$target.y)
 Wait-Until {Tooltip} "Mouse hover did not show the $($target.name) tooltip" 5
 [CapyRowPointer]::Hover($away.x,$away.y)
 Wait-Until {!(Tooltip)} 'Tooltip did not dismiss when the mouse left'
 [CapyRowPointer]::Down('touch',$target.x,$target.y)
 try{
  $watch=[Diagnostics.Stopwatch]::StartNew()
  while($watch.ElapsedMilliseconds -lt 1800){if(Tooltip){throw 'A touch hold opened a tooltip'};Start-Sleep -Milliseconds 40}
 }finally{[CapyRowPointer]::Up()}
 Start-Sleep -Milliseconds 400
 if(Tooltip){throw 'A tooltip appeared after the touch hold'}
 [CapyRowPointer]::Key([uint32]$app.Id,[ushort]0x1B)
 @{mouse_hover='passed';touch_hold_without_tooltip='passed';target=$target.name}|ConvertTo-Json|Set-Content (Join-Path $run 'results.json')
 Get-Content (Join-Path $run 'results.json')
}catch{
 [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace)
 throw
}finally{
 [CapyRowPointer]::Dispose()
 if($app -and !$app.HasExited){Stop-Process -Id $app.Id -Force}
 foreach($name in $names){if($null -eq $previous[$name]){Remove-Item -LiteralPath ('Env:'+$name) -ErrorAction SilentlyContinue}else{[Environment]::SetEnvironmentVariable($name,$previous[$name],'Process')}}
}
