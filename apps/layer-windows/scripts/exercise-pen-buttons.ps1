param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyWaitSeconds=45
Add-Type -AssemblyName System.Drawing
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
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
function Stroke-Pixels([int]$Row,[string]$Path='') { Screen-Pixels ($x+36) $Row $Path }
function Camera { $camera=(Model).state.camera;"$($camera.zoom) $($camera.translation -join ' ')" }
function Assert-PaperSegment([string]$Name,[int]$X1,[int]$Y1,[int]$X2,[int]$Y2){
 $model=Model;$camera=$model.state.camera;$document=@($model.state.tabs|Where-Object active)[0];$canvas=(Control 'drawing-canvas' -Arranged).Current.BoundingRectangle
 $paper=@{left=$canvas.X+$camera.translation[0];top=$canvas.Y+$camera.translation[1];width=$document.width*$camera.zoom;height=$document.height*$camera.zoom}
 $area=$camera.work_area;$points=@(@($X1,$Y1),@($X2,$Y2))
 @{name=$Name;paper=$paper;work_area=$area;points=$points}|ConvertTo-Json -Depth 6 -Compress|Add-Content (Join-Path $run 'paper-segments.jsonl')
 foreach($point in $points){
  if($point[0] -lt [Math]::Max($paper.left,$canvas.X+$area[0])+4 -or $point[0] -gt [Math]::Min($paper.left+$paper.width,$canvas.X+$area[0]+$area[2])-4 -or
     $point[1] -lt [Math]::Max($paper.top,$canvas.Y+$area[1])+4 -or $point[1] -gt [Math]::Min($paper.top+$paper.height,$canvas.Y+$area[1]+$area[3])-4){throw "$Name contact is outside visible paper"}
 }
}
function Inked([string]$Name,[string]$Baseline,[int]$Y) {
 $patch=Join-Path $run ($Name+'-patch.png');$null=Wait-StablePixels {Stroke-Pixels $Y $patch}
 & (Join-Path $PSScriptRoot 'inspect-window.ps1') -TraceDirectory $CapyTraceDirectory -ProcessId $review.Id -WindowHandle $ownedDrawing.Handle.ToInt64() -Output (Join-Path $run ($Name+'.png')) -ClientOnly -Composed *> (Join-Path $run ($Name+'.json'))
 $before=[System.Drawing.Bitmap]::new($Baseline);$image=[System.Drawing.Bitmap]::new($patch)
 try {
  $columns=foreach($column in 8..64){
   $change=0
   foreach($dy in -3..3){
    $a=$before.GetPixel($column+12,48+$dy);$b=$image.GetPixel($column+12,48+$dy)
    foreach($channel in @('R','G','B')){$change=[Math]::Max($change,[Math]::Abs([int]$a.$channel-[int]$b.$channel))}
   }
   $change
  }
  $coverage=@($columns|Where-Object {$_ -gt 6}).Count/$columns.Count
  @{stroke_start=@($x,$Y);patch=@(($x-12),($Y-48),96,96);columns=@(8,64);channel_threshold=6;changes=$columns;coverage=$coverage}|ConvertTo-Json -Depth 5|Set-Content (Join-Path $run ($Name+'-coverage.json'))
  $coverage
 } finally { $image.Dispose();$before.Dispose() }
}
function Barrel-Stroke([int]$X,[int]$Y,[switch]$Held) {
 Assert-PaperSegment 'barrel-stroke' $X $Y ($X+72) $Y
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
 $env:CAPY_STORAGE_DIR=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1'
 $review=Start-Process -FilePath $Executable -WorkingDirectory $run -WindowStyle Hidden -PassThru -RedirectStandardError (Join-Path $run 'stderr.log')
 $null=$review.Handle
 Write-Output "Owned pen button review $($review.Id): $run"
 Wait-Until {$review.Refresh();if($review.HasExited){throw 'Review exited at startup'};$script:ownedDrawing=Owned-DrawingWindow $review;$null -ne $ownedDrawing} 'Owned pen drawing window did not open'
 $root=$ownedDrawing.Root
 Wait-Until {(Model).brush_ready -and (Model).windows_workspace.ready -and !(Model).windows_workspace.busy} 'Pen review did not start'
 [CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
 [CapyRowPointer]::SetForegroundWindow($ownedDrawing.Handle)|Out-Null
 [CapyRowPointer]::Initialize([uint32]$review.Id)
 Use-PaintWorkspace
 $windowPattern=$root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern)
 $windowPattern.SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Maximized)
 Wait-Until {$windowPattern.Current.WindowVisualState -eq [System.Windows.Automation.WindowVisualState]::Maximized -and [Math]::Abs((Model).state.camera.viewport[0]-(Control 'drawing-canvas' -Arranged).Current.BoundingRectangle.Width) -lt 1} 'Owned pen drawing did not maximize'
 Fit-Canvas
 Wait-Until {(Model).canvas_ready -and (Model).brush_ready -and !(Model).state.document_file.busy} 'Blank drawing did not become ready'
 if((Model).state.document_file.modified){throw 'Fixture did not begin with a fresh drawing'}
 $canvas=(Find 'drawing-canvas').Current.BoundingRectangle
 $model=Model;$area=$model.layout.work_area;$density=$canvas.Width/$model.layout.viewport[0]
 $x=[int]($canvas.X+($area.x+$area.width*.5)*$density)
 $y=[int]($canvas.Y+($area.y+$area.height*.5)*$density)
 Assert-PaperSegment 'draw-and-clear' $x $y ($x+24) ($y+8)
 $clearTile=@($model.panels|Where-Object id -eq 'commands')[0].tiles|Where-Object {$_.control.command -eq 'clear_layer'}
 $clearId='tile-commands-'+$clearTile.id
 Wait-Until {(Find $clearId).Current.IsEnabled} 'Clear did not become available after startup'
 $park=Point (Find $clearId);[CapyRowPointer]::PenHover($park.x,$park.y)
 $empty=Wait-StablePixels {Stroke-Pixels $y}
 $records=@()
 foreach($movement in @(0,8,0,8,0,8)){
  if((Wait-StablePixels {Stroke-Pixels $y}) -ne $empty){throw 'Fixture did not restore the blank drawing pixels'}
  $revision=(Model).state.document_file.revision
  [CapyRowPointer]::PenHover($x,$y)
  [CapyRowPointer]::Down('pen',$x,$y)
  for($i=1;$i -le 8;$i++){[CapyRowPointer]::Move($x+3*$i,$y+$i);Start-Sleep -Milliseconds 10}
  [CapyRowPointer]::Up($true)
  [CapyRowPointer]::PenHover($park.x,$park.y)
  Wait-Until {(Model).state.document_file.revision -gt $revision -and (Stroke-Pixels $y) -ne $empty} 'Pen stroke did not paint actual pixels'
  if((Wait-StablePixels {Stroke-Pixels $y}) -eq $empty){throw 'Pen stroke did not retain painted pixels'}
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
  Wait-Until {(Model).state.document_file.revision -gt $drawn -and (Stroke-Pixels $y) -eq $empty} 'One pen tap did not restore exact blank drawing pixels'
  if((Wait-StablePixels {Stroke-Pixels $y}) -ne $empty){throw 'Cleared pixels did not remain blank'}
  $records+=@{movement_px=$movement;stroke_revision=$drawn;clear_revision=(Model).state.document_file.revision;single_tap=$true;pen_remained_in_range=$true}
  Write-Output "Draw then single Clear tap passed (drift $movement px)"
 }
 $size=Find 'tool-setting-size';$size.SetFocus();$size.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('18')
 (Find 'tool-setting-opacity').SetFocus()
 [CapyRowPointer]::SetForegroundWindow($ownedDrawing.Handle)|Out-Null
 $undo=@((Model).panels|Where-Object id -eq 'commands')[0].tiles|Where-Object {$_.control.command -eq 'undo'}
 $view=Camera;$revision=(Model).state.document_file.revision
 $midBlank=Join-Path $run 'barrel-mid-stroke-blank.png';$empty=Wait-StablePixels {Stroke-Pixels $y $midBlank}
 Barrel-Stroke $x $y
 [CapyRowPointer]::Hover($park.x,$park.y)
 Wait-Until {(Model).state.document_file.revision -gt $revision -and (Stroke-Pixels $y) -ne $empty} 'A mid-stroke barrel press left no painted pixels'
 if((Camera) -ne $view){throw 'A mid-stroke barrel press moved the view'}
 $midStroke=Inked 'barrel-mid-stroke' $midBlank $y
 if($midStroke -lt .9){throw "A mid-stroke barrel press interrupted the stroke ($midStroke inked)"}
 $drawn=(Model).state.document_file.revision;Invoke-Id ('tile-commands-'+$undo.id)
 Wait-Until {(Model).state.document_file.revision -gt $drawn -and (Stroke-Pixels $y) -eq $empty} 'One Undo did not restore exact pixels before the whole barrel stroke'
 if((Wait-StablePixels {Stroke-Pixels $y}) -ne $empty){throw 'Barrel stroke Undo did not retain exact baseline pixels'}
 $row=$y+[int](40*$density);$revision=(Model).state.document_file.revision
 $heldBlank=Join-Path $run 'barrel-held-blank.png';$heldEmpty=Wait-StablePixels {Stroke-Pixels $row $heldBlank}
 Barrel-Stroke $x $row -Held
 [CapyRowPointer]::Hover($park.x,$park.y)
 Wait-Until {(Model).state.document_file.revision -gt $revision -and (Stroke-Pixels $row) -ne $heldEmpty} 'A held barrel prevented the tip from painting actual pixels'
 if((Camera) -ne $view){throw 'A held barrel moved the view'}
 $held=Inked 'barrel-held' $heldBlank $row
 if($held -lt .9){throw "A held barrel did not paint the whole stroke ($held inked)"}
 $revision=(Model).state.document_file.revision
 Assert-PaperSegment 'middle-pan' $x ($y-[int](60*$density)) ($x+[int](80*$density)) ($y-[int](30*$density))
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
 [CapyRowPointer]::SetForegroundWindow($ownedDrawing.Handle)|Out-Null
 $view=Camera;$revision=(Model).state.document_file.revision;$row=$y+[int](80*$density)
 Barrel-Stroke $x $row -Held
 Wait-Until {(Camera) -ne $view} 'A barrel bound to Pan did not pan the canvas'
 Start-Sleep -Milliseconds 300
 if((Model).state.document_file.revision -ne $revision){throw 'A barrel bound to Pan painted'}
 Write-Output 'A barrel bound to Pan pans without painting'
 function Pen-Stroke([int]$X,[int]$Y){
  Assert-PaperSegment 'pen-stroke' $X $Y ($X+72) $Y
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
 $eraseBlank=Join-Path $run 'eraser-blank.png';$null=Wait-StablePixels {Stroke-Pixels $row $eraseBlank}
 Pen-Stroke $x $row
 if((Inked 'eraser-before' $eraseBlank $row) -lt .9){throw 'The eraser end check did not start from ink'}
 [CapyRowPointer]::EraserEnd($true);try{Pen-Stroke $x $row}finally{[CapyRowPointer]::EraserEnd($false)}
 $erased=Inked 'eraser-end' $eraseBlank $row
 if($erased -gt .1){throw "The eraser end did not erase by default ($erased inked)"}
 (Find 'drawing-canvas').SetFocus()
 [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),[uint16]0xBC)
 Wait-Until {(Model).preferences} 'Ctrl+, did not open Preferences'
 Invoke 'Pen & Input' -Name
 (Control 'Paint with transparency' -Name -Type ([System.Windows.Automation.ControlType]::Button)).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
 Wait-Until {(Model).state.settings.eraser_end.erase -eq $false} 'Paint with transparency did not turn off'
 Invoke-Id 'CloseButton'
 Wait-Until {!(Model).preferences -and !(Find 'CloseButton')} 'Preferences did not close';Start-Sleep -Milliseconds 300
 [CapyRowPointer]::SetForegroundWindow($ownedDrawing.Handle)|Out-Null
 $at=Paper .3 .7;$x=$at[0];$row=$at[1]
 $paintBlank=Join-Path $run 'eraser-end-paints-blank.png';$null=Wait-StablePixels {Stroke-Pixels $row $paintBlank}
 [CapyRowPointer]::EraserEnd($true);try{Pen-Stroke $x $row}finally{[CapyRowPointer]::EraserEnd($false)}
 $painted=Inked 'eraser-end-paints' $paintBlank $row
 if($painted -lt .9){throw "The eraser end still erased with Paint with transparency off ($painted inked)"}
 Write-Output "Eraser end erases by default and paints ($painted) with transparency off"
 $records|ConvertTo-Json|Set-Content (Join-Path $run 'results.json')
 [CapyRowPointer]::Dispose()
 & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $ownedDrawing.Handle.ToInt64() -Action Close -DiscardUnsaved -StateDirectory $run
 if((Get-Item (Join-Path $run 'stderr.log')).Length){throw 'Native stderr requires review'}
 [pscustomobject]@{draw_then_clear='6/6';barrel_mid_stroke='one stroke, one Undo';barrel_held='paints';middle_drag='pans';barrel_bound_to_pan='pans without painting';eraser_end='erases by default, paints with transparency off';scope='OS-injected pen with continuous hover; physical Wacom acceptance remains separate';evidence=$run}|ConvertTo-Json
} catch {
 $failure=$_
 if($review -and !$review.HasExited -and $ownedDrawing){
  try { & (Join-Path $PSScriptRoot 'inspect-window.ps1') -TraceDirectory $CapyTraceDirectory -ProcessId $review.Id -WindowHandle $ownedDrawing.Handle.ToInt64() -Output (Join-Path $run 'failure.png') -ClientOnly -Composed *> (Join-Path $run 'failure-window.json') } catch { Write-Warning $_ }
 }
 throw $failure
} finally {
 [CapyRowPointer]::Dispose()
 Exit-CapyEnvironment
}
