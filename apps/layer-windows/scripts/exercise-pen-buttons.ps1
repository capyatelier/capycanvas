param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
Add-Type -AssemblyName System.Drawing
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
Add-Type -Name PenButtonsWindow -Namespace Capy -MemberDefinition '[DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr window,int[] point);'
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
function Camera { $camera=(Model).state.camera;"$($camera.zoom) $($camera.translation -join ' ')" }
function Inked([string]$Name,[int]$Left,[int]$Right,[int]$Y) {
 & (Join-Path $PSScriptRoot 'inspect-window.ps1') -ProcessId $review.Id -Output (Join-Path $run ($Name+'.png')) -ClientOnly *> (Join-Path $run ($Name+'.json'))
 $origin=[int[]]@(0,0);$null=[Capy.PenButtonsWindow]::ClientToScreen($review.MainWindowHandle,$origin)
 $image=[System.Drawing.Bitmap]::new((Join-Path $run ($Name+'.png')))
 try {
  $marked=0
  for($x=$Left;$x -le $Right;$x++){
   $ink=$false
   for($dy=-3;$dy -le 3;$dy++){if($image.GetPixel($x-$origin[0],$Y+$dy-$origin[1]).R -lt 128){$ink=$true}}
   if($ink -and $image.GetPixel($x-$origin[0],$Y-16-$origin[1]).R -gt 200){$marked++}
  }
  $marked/[Math]::Max(1,$Right-$Left+1)
 } finally { $image.Dispose() }
}
function Barrel-Stroke([int]$X,[int]$Y,[switch]$Held) {
 [CapyRowPointer]::PenHover($X,$Y)
 if($Held){[CapyRowPointer]::Barrel($true)}
 Start-Sleep -Milliseconds 35
 [CapyRowPointer]::Down('pen',$X,$Y)
 for($i=1;$i -le 18;$i++){
  if(!$Held -and $i -eq 7){[CapyRowPointer]::Barrel($true)}
  if(!$Held -and $i -eq 13){[CapyRowPointer]::Barrel($false)}
  [CapyRowPointer]::Move($X+4*$i,$Y);Start-Sleep -Milliseconds 10
 }
 [CapyRowPointer]::Up($true)
 [CapyRowPointer]::Barrel($false)
 [CapyRowPointer]::PenLeave()
}
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
 $size=Find 'tool-setting-size';$size.SetFocus();$size.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('18')
 (Find 'tool-setting-opacity').SetFocus()
 [CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)|Out-Null
 $undo=@((Model).panels|Where-Object id -eq 'commands')[0].tiles|Where-Object {$_.control.command -eq 'undo'}
 $view=Camera;$revision=(Model).state.document_file.revision
 Barrel-Stroke $x $y
 Wait-Until {(Transform-Enabled) -and (Model).state.document_file.revision -gt $revision} 'A mid-stroke barrel press left no stroke'
 if((Camera) -ne $view){throw 'A mid-stroke barrel press moved the view'}
 $midStroke=Inked 'barrel-mid-stroke' ($x+8) ($x+64) $y
 if($midStroke -lt .9){throw "A mid-stroke barrel press interrupted the stroke ($midStroke inked)"}
 Invoke-Id ('tile-commands-'+$undo.id)
 Wait-Until {!(Transform-Enabled)} 'One Undo did not remove the whole barrel stroke'
 $row=$y+[int](40*$density);$revision=(Model).state.document_file.revision
 Barrel-Stroke $x $row -Held
 Wait-Until {(Transform-Enabled) -and (Model).state.document_file.revision -gt $revision} 'A held barrel prevented the tip from painting'
 if((Camera) -ne $view){throw 'A held barrel moved the view'}
 $held=Inked 'barrel-held' ($x+8) ($x+64) $row
 if($held -lt .9){throw "A held barrel did not paint the whole stroke ($held inked)"}
 $revision=(Model).state.document_file.revision
 [CapyRowPointer]::MiddleDrag($x,($y-[int](60*$density)),($x+[int](80*$density)),($y-[int](30*$density)))
 Wait-Until {(Camera) -ne $view} 'Mouse middle drag did not pan the canvas'
 if((Model).state.document_file.revision -ne $revision){throw 'Mouse middle drag edited the drawing'}
 Write-Output "Barrel mid-stroke $midStroke, held $held, middle drag pans"
 $records|ConvertTo-Json|Set-Content (Join-Path $run 'results.json')
 [CapyRowPointer]::Dispose()
 & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved -StateDirectory $run
 if((Get-Item (Join-Path $run 'stderr.log')).Length){throw 'Native stderr requires review'}
 [pscustomobject]@{draw_then_clear='6/6';barrel_mid_stroke='one stroke, one Undo';barrel_held='paints';middle_drag='pans';scope='OS-injected pen with continuous hover; physical Wacom acceptance remains separate';evidence=$run}|ConvertTo-Json
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