param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyWaitSeconds=45
Add-Type -AssemblyName System.Drawing
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
Add-Type -Name PenButtonsWindow -Namespace Capy -MemberDefinition '[DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr window,int[] point);'
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$run=Join-Path $repo ('artifacts/windows/pen-buttons/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$CapyTraceDirectory=$run
function Point($Element) {
 $b=$Element.Current.BoundingRectangle
 if($Element.Current.IsOffscreen -or $b.IsEmpty -or $b.Width -le 0 -or $b.Height -le 0){throw 'Input target is not visible'}
 @{x=[int]($b.X+$b.Width*.5);y=[int]($b.Y+$b.Height*.5)}
}
function Transform-Enabled { @((Model).state.commands|Where-Object id -eq 'scale_rotate')[0].enabled }
function Camera { $camera=(Model).state.camera;"$($camera.zoom) $($camera.translation -join ' ')" }
function Inked([string]$Name,[int]$Left,[int]$Right,[int]$Y) {
 & (Join-Path $PSScriptRoot 'inspect-window.ps1') -ProcessId $review.Id -Output (Join-Path $run ($Name+'.png')) -ClientOnly -Composed *> (Join-Path $run ($Name+'.json'))
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
 Enter-CapyEnvironment
 $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1'
 $review=Start-Process -FilePath $Executable -WorkingDirectory $run -WindowStyle Hidden -PassThru -RedirectStandardError (Join-Path $run 'stderr.log')
 $null=$review.Handle
 Write-Output "Owned pen button review $($review.Id): $run"
 $watch=[Diagnostics.Stopwatch]::StartNew()
 do{$review.Refresh();if($review.HasExited){throw 'Review exited at startup'};Start-Sleep -Milliseconds 100}while($review.MainWindowHandle -eq [IntPtr]::Zero -and $watch.Elapsed.TotalSeconds -lt 45)
 $root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
 Wait-Until {(Model).brush_ready -and (Model).windows_workspace.ready -and !(Model).windows_workspace.busy} 'Pen review did not start'
 [CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
 [CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)|Out-Null
 [CapyRowPointer]::Initialize([uint32]$review.Id)
 $canvas=(Find 'drawing-canvas').Current.BoundingRectangle
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
 (Find 'drawing-canvas').SetFocus()
 [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),[uint16]0xBC)
 Wait-Until {(Model).preferences} 'Ctrl+, did not open Preferences'
 Invoke 'Pen & Input' -Name
 Wait-Until {Find 'trigger-pen.button.primary'} 'Pen & Input did not list the lower pen button'
 if(Find 'trigger-pen.button.secondary'){throw 'Windows listed an upper pen button that Windows Ink cannot report'}
 Invoke-Id 'trigger-pen.button.primary'
 Wait-Until {(Model).preferences.pen_button_editor -and (Find 'pen-button-action-all')} 'The lower pen button page did not open'
 Invoke-Id 'pen-button-action-all'
 Wait-Until {(Model).preferences.shortcut_page.picker -and (Find 'action-picker-search')} 'The pen button picker did not open'
 (Find 'action-picker-search').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('Pan')
 Wait-Until {Find 'action-command.Hand'} 'The pen button picker did not offer Pan'
 Invoke-Id 'action-command.Hand'
 Wait-Until {!(Model).preferences.shortcut_page.picker -and "$((Model).state.settings.pen_buttons.'pen.button.primary')" -match 'command.Hand'} 'Pan was not bound to the lower pen button'
 Invoke-Id 'CloseButton'
 Wait-Until {!(Model).preferences -and !(Find 'CloseButton')} 'Preferences did not close';Start-Sleep -Milliseconds 300
 [CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)|Out-Null
 $view=Camera;$revision=(Model).state.document_file.revision;$row=$y+[int](80*$density)
 Barrel-Stroke $x $row -Held
 Wait-Until {(Camera) -ne $view} 'A barrel bound to Pan did not pan the canvas'
 Start-Sleep -Milliseconds 300
 if((Model).state.document_file.revision -ne $revision){throw 'A barrel bound to Pan painted'}
 Write-Output 'A barrel bound to Pan pans without painting'
 function Pen-Stroke([int]$X,[int]$Y){
  $revision=(Model).state.document_file.revision
  [CapyRowPointer]::PenHover($X,$Y);Start-Sleep -Milliseconds 35
  [CapyRowPointer]::Down('pen',$X,$Y)
  for($i=1;$i -le 18;$i++){[CapyRowPointer]::Move($X+4*$i,$Y);Start-Sleep -Milliseconds 10}
  [CapyRowPointer]::Up($true);[CapyRowPointer]::PenLeave()
  Wait-Until {(Model).state.document_file.revision -gt $revision} 'The pen stroke did not finish'
  Start-Sleep -Milliseconds 300
 }
 function Paper([double]$Across,[double]$Down){
  $m=Model;$c=$m.state.camera;$tab=@($m.state.tabs)[0];$b=(Find 'drawing-canvas').Current.BoundingRectangle
  @([int]($b.X+$c.translation[0]+$tab.width*$c.zoom*$Across),[int]($b.Y+$c.translation[1]+$tab.height*$c.zoom*$Down))
 }
 $at=Paper .3 .3;$x=$at[0];$row=$at[1]
 Pen-Stroke $x $row
 if((Inked 'eraser-before' ($x+8) ($x+64) $row) -lt .9){throw 'The eraser end check did not start from ink'}
 [CapyRowPointer]::EraserEnd($true);try{Pen-Stroke $x $row}finally{[CapyRowPointer]::EraserEnd($false)}
 $erased=Inked 'eraser-end' ($x+8) ($x+64) $row
 if($erased -gt .1){throw "The eraser end did not erase by default ($erased inked)"}
 (Find 'drawing-canvas').SetFocus()
 [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),[uint16]0xBC)
 Wait-Until {(Model).preferences} 'Ctrl+, did not open Preferences'
 Invoke 'Pen & Input' -Name
 (Control 'Paint with transparency' -Name -Type ([System.Windows.Automation.ControlType]::Button)).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
 Wait-Until {(Model).state.settings.eraser_end.erase -eq $false} 'Paint with transparency did not turn off'
 Invoke-Id 'CloseButton'
 Wait-Until {!(Model).preferences -and !(Find 'CloseButton')} 'Preferences did not close';Start-Sleep -Milliseconds 300
 [CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)|Out-Null
 $at=Paper .3 .7;$x=$at[0];$row=$at[1]
 [CapyRowPointer]::EraserEnd($true);try{Pen-Stroke $x $row}finally{[CapyRowPointer]::EraserEnd($false)}
 $painted=Inked 'eraser-end-paints' ($x+8) ($x+64) $row
 if($painted -lt .9){throw "The eraser end still erased with Paint with transparency off ($painted inked)"}
 Write-Output "Eraser end erases by default and paints ($painted) with transparency off"
 $records|ConvertTo-Json|Set-Content (Join-Path $run 'results.json')
 [CapyRowPointer]::Dispose()
 & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved -StateDirectory $run
 if((Get-Item (Join-Path $run 'stderr.log')).Length){throw 'Native stderr requires review'}
 [pscustomobject]@{draw_then_clear='6/6';barrel_mid_stroke='one stroke, one Undo';barrel_held='paints';middle_drag='pans';barrel_bound_to_pan='pans without painting';eraser_end='erases by default, paints with transparency off';scope='OS-injected pen with continuous hover; physical Wacom acceptance remains separate';evidence=$run}|ConvertTo-Json
} catch {
 $failure=$_
 if($review -and !$review.HasExited){
  try { & (Join-Path $PSScriptRoot 'inspect-window.ps1') -ProcessId $review.Id -Output (Join-Path $run 'failure.png') -ClientOnly -Composed *> (Join-Path $run 'failure-window.json') } catch { Write-Warning $_ }
 }
 throw $failure
} finally {
 [CapyRowPointer]::Dispose()
 Exit-CapyEnvironment
}