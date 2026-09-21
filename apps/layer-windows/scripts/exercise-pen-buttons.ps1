param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$run=Join-Path $repo ('artifacts/windows/pen-buttons/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$names=@('CAPY_SETTINGS_DIRECTORY','CAPY_TRACE_UI','CAPY_SMOKE_TEST','CAPY_TEST_DISPLAY','CAPY_TEST_PRIMARY','CAPY_PRESENT_PROBE')
$previous=@{};foreach($name in $names){$previous[$name]=[Environment]::GetEnvironmentVariable($name,'Process')}
function Model {
 try {
  $value=Get-Content -LiteralPath (Join-Path $run 'ui-state.json') -Raw|ConvertFrom-Json
  if($value.process_id -eq $review.Id -and $value.model.windows_isolated_settings){$value.model}
 }catch{}
}
function Point($Element) {
 $b=$Element.Current.BoundingRectangle
 if($Element.Current.IsOffscreen -or $b.IsEmpty -or $b.Width -le 0 -or $b.Height -le 0){throw 'Input target is not visible'}
 @{x=[int]($b.X+$b.Width*.5);y=[int]($b.Y+$b.Height*.5)}
}
function Transform-Enabled { @((Model).state.commands|Where-Object id -eq 'scale_rotate')[0].enabled }
try {
 foreach($name in $names){Remove-Item -LiteralPath ('Env:'+$name) -ErrorAction SilentlyContinue}
 $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1'
 $review=Start-Process -FilePath $Executable -WorkingDirectory $run -WindowStyle Hidden -PassThru -RedirectStandardError (Join-Path $run 'stderr.log')
 $null=$review.Handle
 Write-Output "Owned pen button review $($review.Id): $run"
 $watch=[Diagnostics.Stopwatch]::StartNew()
 do{$review.Refresh();if($review.HasExited){throw 'Review exited at startup'};Start-Sleep -Milliseconds 100}while($review.MainWindowHandle -eq [IntPtr]::Zero -and $watch.Elapsed.TotalSeconds -lt 45)
 . (Join-Path $repo 'tools/performance/windows-pen-ui.ps1') -ProcessId $review.Id
 Wait-Until {(Model).brush_ready -and (Model).windows_workspace.ready -and !(Model).windows_workspace.busy} 'Pen review did not start'
 [CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
 [CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)|Out-Null
 [CapyRowPointer]::Initialize([uint32]$review.Id)
 $canvas=(Find 'Drawing canvas' -Name).Current.BoundingRectangle
 $model=Model;$area=$model.layout.work_area;$density=$canvas.Width/$model.layout.viewport[0]
 $x=[int]($canvas.X+($area.x+$area.width*.5)*$density)
 $y=[int]($canvas.Y+($area.y+$area.height*.5)*$density)
 $clearTile=@($model.panels|Where-Object id -eq 'commands')[0].tiles|Where-Object {$_.control.command -eq 'clear_layer'}
 $clearId='tile-commands-'+$clearTile.id
 # Startup may still be validating the bundled filter library, which locks document edits.
 Wait-Until {(Find $clearId).Current.IsEnabled} 'Clear did not become available after startup'
 $records=@()
 foreach($movement in @(0,8,0,8,0,8)){
  if(Transform-Enabled){throw 'Fixture did not begin with an empty paint layer'}
  $revision=(Model).state.document_file.revision
  [CapyRowPointer]::PenHover($x,$y)
  [CapyRowPointer]::Down('pen',$x,$y)
  for($i=1;$i -le 8;$i++){[CapyRowPointer]::Move($x+3*$i,$y+$i);Start-Sleep -Milliseconds 10}
  [CapyRowPointer]::Up($true)
  Wait-Until {(Transform-Enabled) -and (Model).state.document_file.revision -gt $revision} 'Pen stroke did not create editable content'
  $drawn=(Model).state.document_file.revision
  $clear=Find $clearId
  if(!$clear.Current.IsEnabled){throw 'Clear remained disabled after the pen stroke'}
  $at=Point $clear
  [CapyRowPointer]::PenHover($at.x,$at.y)
  Start-Sleep -Milliseconds 35
  [CapyRowPointer]::Down('pen',$at.x,$at.y)
  Start-Sleep -Milliseconds 35
  [CapyRowPointer]::Move($at.x+$movement,$at.y)
  Start-Sleep -Milliseconds 35
  [CapyRowPointer]::Up($true)
  Wait-Until {!(Transform-Enabled) -and (Model).state.document_file.revision -gt $drawn} 'One pen tap did not clear the layer'
  $records+=@{movement_px=$movement;stroke_revision=$drawn;clear_revision=(Model).state.document_file.revision;single_tap=$true;pen_remained_in_range=$true}
  Write-Output "Draw then single Clear tap passed (drift $movement px)"
 }
 $records|ConvertTo-Json|Set-Content (Join-Path $run 'results.json')
 [CapyRowPointer]::Dispose()
 & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved -StateDirectory $run
 if((Get-Item (Join-Path $run 'stderr.log')).Length){throw 'Native stderr requires review'}
 [pscustomobject]@{draw_then_clear='6/6';scope='OS-injected pen with continuous hover; physical Wacom acceptance remains separate';evidence=$run}|ConvertTo-Json
} catch {
 $failure=$_
 if($review -and !$review.HasExited){
  try { & (Join-Path $PSScriptRoot 'inspect-window.ps1') -ProcessId $review.Id -Output (Join-Path $run 'failure.png') -ClientOnly *> (Join-Path $run 'failure-window.json') } catch { Write-Warning $_ }
 }
 throw $failure
} finally {
 [CapyRowPointer]::Dispose()
 foreach($name in $names){if($null -eq $previous[$name]){Remove-Item -LiteralPath ('Env:'+$name) -ErrorAction SilentlyContinue}else{[Environment]::SetEnvironmentVariable($name,$previous[$name],'Process')}}
}