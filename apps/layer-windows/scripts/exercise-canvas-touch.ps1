param([Parameter(Mandatory)][string]$Executable,[ValidateSet('dark','light')][string]$Theme='dark',[switch]$ScrubOnly)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyEach={[CapyCanvasTouch]::Verify()}
Add-Type -Path (Join-Path $PSScriptRoot 'CanvasTouchDriver.cs')
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
Add-Type -AssemblyName System.Drawing
[CapyCanvasTouch]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path;$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/canvas-touch/'+[Guid]::NewGuid().ToString('N'));[IO.Directory]::CreateDirectory($run)|Out-Null
function Select-Tool([string]$Id){
 if(((Model).state.commands|Where-Object id -eq $Id).selected){return}
 $target=@{id=$null};Wait-Until {foreach($panel in (Model).panels){foreach($tile in $panel.tiles){if($tile.control.kind -eq 'command' -and $tile.control.command -eq $Id){$target.id="tile-$($panel.id)-$($tile.id)";return $true}}};$false} "No native $Id tile"
 Invoke $target.id;Wait-Until {((Model).state.commands|Where-Object id -eq $Id).selected} "Tool did not select: $Id"
}
function Camera {
 if(!$script:cameraFile){
  $path=State-File;$owned=Read-Snapshot $path
  if($owned.process_id -ne $review.Id -or !$owned.model.windows_isolated_settings){throw 'Camera snapshot is not from the owned isolated drawing'}
  $script:cameraWindow=$owned.window_id
  $script:cameraFile=Join-Path (Split-Path -Parent $path) ((Split-Path -Leaf $path) -replace '^ui-state-','camera-state-')
 }
 $snapshot=Read-Snapshot $script:cameraFile
 if($snapshot.process_id -ne $review.Id -or $snapshot.window_id -ne $script:cameraWindow){throw 'Camera snapshot is not from the owned drawing window'}
 $snapshot.camera
}
function Same-Camera($a,$b,[double]$Pixels=.1){
 $a -and $b -and [Math]::Abs($a.zoom-$b.zoom) -lt .0001 -and [Math]::Abs($a.rotation-$b.rotation) -lt .0001 -and
 [Math]::Abs($a.translation[0]-$b.translation[0]) -lt $Pixels -and [Math]::Abs($a.translation[1]-$b.translation[1]) -lt $Pixels
}
# Compare the resulting view over the whole document, allowing one physical
# pixel for OS contact coordinates and accumulated float camera arithmetic.
function Matches-Gesture($actual,$expected){
 if(!$actual -or !$expected){return $false}
 $tab=@((Model).state.tabs|Where-Object active)[0]
 if(!$tab){return $false}
 $extent=@($tab.width,$tab.height)
 foreach($x in @(0,$extent[0])){foreach($y in @(0,$extent[1])){
  $ax=$actual.translation[0]+$actual.zoom*([Math]::Cos($actual.rotation)*$x-[Math]::Sin($actual.rotation)*$y)
  $ay=$actual.translation[1]+$actual.zoom*([Math]::Sin($actual.rotation)*$x+[Math]::Cos($actual.rotation)*$y)
  $ex=$expected.translation[0]+$expected.zoom*([Math]::Cos($expected.rotation)*$x-[Math]::Sin($expected.rotation)*$y)
  $ey=$expected.translation[1]+$expected.zoom*([Math]::Sin($expected.rotation)*$x+[Math]::Cos($expected.rotation)*$y)
  if([Math]::Sqrt(($ax-$ex)*($ax-$ex)+($ay-$ey)*($ay-$ey)) -ge 1){return $false}
 }}
 $true
}
function Signature {$m=$null;for($i=0;$i -lt 40 -and !$m;$i++){$m=Model;if(!$m){Start-Sleep -Milliseconds 25}};@($m.state.document_file,@($m.state.layers|Select-Object id,paint_revision,mask_revision))|ConvertTo-Json -Depth 30 -Compress}
$checks=[Collections.Generic.List[object]]::new()
function Check([string]$Name,[scriptblock]$Condition){Wait-Until $Condition $Name;if((Signature) -ne $drawing){throw "Touch changed the drawing: $Name"};$checks.Add(@{name=$Name;camera=Camera});Write-Output "$Name passed"}
function Stable([string]$Name,$Expected){Start-Sleep -Milliseconds 160;Check $Name {(Same-Camera (Camera) $Expected)}}
function Camera-Pose{$c=Camera;@($c.viewport,$c.translation,$c.zoom,$c.rotation,$c.flipped)|ConvertTo-Json -Depth 6 -Compress}
function Paint-Checkpoint{@((Model).state.layers|Select-Object id,paint_revision,mask_revision,locked,alpha_locked,visible)|ConvertTo-Json -Depth 6 -Compress}
function Scrub-Park{
 $box=(Control 'column-icon-layers' -Arranged).Current.BoundingRectangle
 [CapyRowPointer]::Hover([int]($box.X+$box.Width/2),[int]($box.Y+$box.Height/2))
}
function Scrub-Pixels{
 if((Camera-Pose) -ne $scrubCamera -or (Control 'drawing-canvas').Current.BoundingRectangle -ne $scrubBounds){throw 'The scrub moved the camera or canvas during exact pixel comparisons'}
 @($scrubSamples|ForEach-Object {Screen-Pixels $_[0] $_[1]}) -join ':'
}
function Scrub-Key([uint16]$Key){
 (Control 'drawing-canvas').SetFocus()
 [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),$Key)
 Scrub-Park
}
function Scrub-Touch([double[]]$Direction,[switch]$Cancel){
 $scale=[CapyRowPointer]::GetDpiForWindow($handle)/96.
 $origins=@(@([int]($cx-48*$scale),$cy),@($cx,$cy),@([int]($cx+48*$scale),$cy))
 foreach($id in 1..3){[CapyCanvasTouch]::Down($id,$origins[$id-1][0],$origins[$id-1][1])}
 $clock=[Diagnostics.Stopwatch]::StartNew();$from=0.
 try{
  $legs=if($Cancel){@(64.,0.)}else{@(64.,0.,64.,0.,64.)}
  foreach($to in $legs){
   for($step=1;$step -le 8;$step++){
    $distance=($from+($to-$from)*$step/8)*$scale
    foreach($id in 1..3){[CapyCanvasTouch]::Move($id,[int]($origins[$id-1][0]+$Direction[0]*$distance),[int]($origins[$id-1][1]+$Direction[1]*$distance))}
    Start-Sleep -Milliseconds 12
   }
   $from=$to
  }
  if(!$Cancel){
   [CapyCanvasTouch]::Up(1)
   [CapyCanvasTouch]::Move(2,[int]($origins[1][0]+$Direction[0]*64*$scale+20*$scale),[int]($origins[1][1]+$Direction[1]*64*$scale+20*$scale))
   [CapyCanvasTouch]::Up(2);[CapyCanvasTouch]::Up(3)
  }
 }finally{[CapyCanvasTouch]::CancelAll()}
 $checks.Add(@{name='native scrub contacts';direction=$Direction;cancelled=[bool]$Cancel;elapsed_ms=$clock.Elapsed.TotalMilliseconds})
 Scrub-Park
}
function Scrub-Lock{
 Invoke 'column-icon-layers'
 Wait-Until {@((Model).layout.groups|Where-Object active -eq 'layers').Count -eq 1} 'Layers did not open for its native lock menu'
 Invoke 'layer-actions'
 (Control 'menu-layer-settings').GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
 Invoke 'layer-menu-lock'
 Wait-Until {(Model).state.layer_tools.editing_layer.locked -and !(Find 'layer-menu-lock' -Visible)} 'The native Layer Settings menu did not lock editing'
 Invoke 'column-icon-layers'
 Wait-Until {@((Model).layout.groups|Where-Object active -eq 'layers').Count -eq 0} 'Layers did not close after locking'
 Scrub-Park
}
function Check-Scrubs{
 $painted=Wait-StablePixels {Scrub-Pixels};$paint=Paint-Checkpoint
 if($painted -eq $scrubEmpty){throw 'The real mouse seed did not change its composed pixel samples'}
 Capture 'scrub-seed' -WithModel -Composed
 [CapyCanvasTouch]::Initialize([uint32]$review.Id)
 $before=Signature
 Scrub-Touch @(1.,0.) -Cancel
 if((Signature) -ne $before -or (Wait-StablePixels {Scrub-Pixels}) -ne $painted){throw 'A cancelled three-finger scrub changed artwork or history'}
 $checks.Add(@{name='scrub cancellation before admission';pixels='exact';history='unchanged'})
 foreach($degrees in 0,45,90){
  $angle=$degrees*[Math]::PI/180
  Scrub-Touch @([Math]::Cos($angle),[Math]::Sin($angle))
  Wait-Until {(Paint-Checkpoint) -ne $paint -and (Scrub-Pixels) -eq $scrubEmpty} "The $degrees-degree scrub did not clear the active paint layer"
  $cleared=Paint-Checkpoint
  if((Wait-StablePixels {Scrub-Pixels}) -ne $scrubEmpty){throw "The $degrees-degree clear did not retain exact empty-layer pixels"}
  Capture ("scrub-clear-"+$degrees) -WithModel -Composed
  Scrub-Key 0x5A
  Wait-Until {(Paint-Checkpoint) -eq $paint -and (Scrub-Pixels) -eq $painted} "One Undo did not restore the exact $degrees-degree scrub artwork"
  Scrub-Key 0x59
  Wait-Until {(Paint-Checkpoint) -eq $cleared -and (Scrub-Pixels) -eq $scrubEmpty} "One Redo did not restore the exact $degrees-degree scrub clear"
  Scrub-Key 0x5A
  Wait-Until {(Paint-Checkpoint) -eq $paint -and (Scrub-Pixels) -eq $painted} "The $degrees-degree scrub did not restore the seed for the next contact group"
  $checks.Add(@{name="scrub $degrees degrees";camera='exact';pixels='exact';undo_redo='one step';painted=$painted;cleared=$scrubEmpty})
 }
 Scrub-Lock
 $locked=Signature;$lockedPaint=Paint-Checkpoint
 Scrub-Touch @(1.,0.)
 if((Signature) -ne $locked -or (Paint-Checkpoint) -ne $lockedPaint -or (Wait-StablePixels {Scrub-Pixels}) -ne $painted){throw 'A scrub changed the locked layer, history or exact pixels'}
 Scrub-Key 0x5A
 Wait-Until {!(Model).state.layer_tools.editing_layer.locked -and (Paint-Checkpoint) -eq $paint -and (Scrub-Pixels) -eq $painted} 'A refused scrub added history after Lock editing'
 Scrub-Key 0x59
 Wait-Until {(Model).state.layer_tools.editing_layer.locked -and (Paint-Checkpoint) -eq $lockedPaint -and (Scrub-Pixels) -eq $painted} 'One Redo did not restore the exact locked layer'
 Scrub-Key 0x5A
 Wait-Until {!(Model).state.layer_tools.editing_layer.locked -and (Paint-Checkpoint) -eq $paint} 'The layer did not unlock for continued native contacts'
 $checks.Add(@{name='locked-layer scrub refusal';camera='exact';pixels='exact';history='one Undo unlocks'})
 $before=Camera
 [CapyCanvasTouch]::Down(1,($cx-80),$cy);[CapyCanvasTouch]::Down(2,($cx+80),$cy)
 [CapyCanvasTouch]::Pair(($cx-40),($cy+20),($cx+120),($cy+20))
 $expected=$before|ConvertTo-Json -Depth 8|ConvertFrom-Json;$expected.translation[0]+=40;$expected.translation[1]+=20
 Wait-Until {Matches-Gesture (Camera) $expected} 'Fresh native contacts did not pan after the scrub releases'
 [CapyCanvasTouch]::Up(2);[CapyCanvasTouch]::Up(1)
 Fit-Canvas;Scrub-Park
 Wait-Until {(Camera-Pose) -eq $scrubCamera -and (Paint-Checkpoint) -eq $paint -and (Scrub-Pixels) -eq $painted} 'Navigation after scrubbing did not preserve the exact artwork'
 [CapyCanvasTouch]::Dispose()
 [CapyRowPointer]::Down('pen',($cx-60),($cy-25))
 try{for($i=1;$i -le 24;$i++){[CapyRowPointer]::Move(($cx-60+$i*5),($cy-25));Start-Sleep -Milliseconds 8}}finally{[CapyRowPointer]::Up()}
 Scrub-Park
 Wait-Until {(Paint-Checkpoint) -ne $paint -and (Scrub-Pixels) -ne $painted} 'A pen did not paint after the scrub releases'
 Scrub-Key 0x5A
 Wait-Until {(Paint-Checkpoint) -eq $paint -and (Scrub-Pixels) -eq $painted} 'One Undo did not restore exact artwork after the continued pen contact'
 Capture 'scrub-continued-input' -WithModel -Composed
 [CapyRowPointer]::Verify();[CapyRowPointer]::Dispose()
 & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $handle.ToInt64() -Action Close -DiscardUnsaved
 if((Get-Item -LiteralPath (Join-Path $run 'stderr.log')).Length){throw 'Touch runtime stderr requires inspection'}
 @{theme=$Theme;scrub_only=$true;checks=$checks;continued_input='two-finger navigation and pen';scope='OS-injected native contacts; physical digitizers and presentation timing remain separate'}|ConvertTo-Json -Depth 12|Set-Content -LiteralPath (Join-Path $run 'result.json')
 Write-Output "Canvas scrub acceptance passed: $run"
}
$fixtureFailed=$false
try{
 Enter-CapyEnvironment
 $env:CAPY_STORAGE_DIR=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1';$env:CAPY_SMOKE_TEST='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
 [IO.File]::WriteAllText((Settings-File),(@{language=@{Explicit='en'};theme=$Theme}|ConvertTo-Json -Depth 4))
 if($ScrubOnly){$CapyFind='prefer-visible';$CapyPopups=$true}
 $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError (Join-Path $run 'stderr.log') -RedirectStandardOutput (Join-Path $run 'stdout.log');$null=$review.Handle
 @{process_id=$review.Id;executable=$Executable;sha256=(Get-FileHash -LiteralPath $Executable).Hash}|ConvertTo-Json|Set-Content -LiteralPath (Join-Path $run 'owner.json')
 Write-Output "Owned touch review $($review.Id): $run"
 $native=@{window=$null}
 Wait-Until {$native.window=Owned-DrawingWindow $review;$native.window -and (Model).brush_ready -and (Model).windows_workspace.ready} 'Isolated canvas did not start' 45
 $handle=$native.window.Handle;$root=$native.window.Root
 $root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Maximized)
 Wait-Until {$c=Camera;$b=(Control 'drawing-canvas').Current.BoundingRectangle;[Math]::Abs($c.viewport[0]-$b.Width) -lt .1 -and $b.Width -gt 1600} 'Maximized canvas did not settle'
 Use-PaintWorkspace
 if(@((Model).layout.groups|Where-Object active -eq 'layers').Count){Invoke 'column-icon-layers';Wait-Until {@((Model).layout.groups|Where-Object active -eq 'layers').Count -eq 0} 'Column did not close for gesture space'}
 Fit-Canvas;Select-Tool 'pen'
 Wait-Until {(Model).canvas_ready -and (Model).brush_ready -and !(Model).state.document_file.busy} 'Canvas and selected pen did not settle before input'
 $area=(Camera).work_area
 $bounds=(Control 'drawing-canvas').Current.BoundingRectangle
 $cx=[int]($bounds.X+$area[0]+$area[2]/2);$cy=[int]($bounds.Y+$area[1]+$area[3]/2)
 if($area[2] -lt 600 -or $area[3] -lt 500){throw 'Available canvas is too small for multi-touch acceptance'}
 [CapyCanvasTouch]::SetForegroundWindow($handle)|Out-Null
 [CapyRowPointer]::Initialize([uint32]$review.Id)
 if($ScrubOnly){
  $scrubBounds=$bounds;$scrubCamera=Camera-Pose;$scrubSamples=@(@(($cx-48),($cy-50)),@($cx,($cy-50)),@(($cx+48),($cy-50)))
  Scrub-Park;$scrubEmpty=Wait-StablePixels {Scrub-Pixels}
 }
 [CapyRowPointer]::Down('mouse',($cx-60),($cy-50))
 try{for($i=1;$i -le 24;$i++){[CapyRowPointer]::Move(($cx-60+$i*5),($cy-50+[int](10*[Math]::Sin($i/4))));Start-Sleep -Milliseconds 8}}finally{[CapyRowPointer]::Up()}
 Wait-Until {(Model).state.document_file.modified} 'Visible mouse seed stroke did not finish'
 $settled=@{value=''};Wait-Until {$now=Signature;$same=$now -eq $settled.value;$settled.value=$now;Start-Sleep -Milliseconds 150;$same} 'Seed stroke did not settle'
 if($ScrubOnly){Scrub-Park;Check-Scrubs;return}
 $seeded=$settled.value;$unpanned=Camera
 [CapyRowPointer]::RightDrag(($cx+40),($cy+40),($cx+100),($cy+70))
 Wait-Until {[Math]::Abs((Camera).translation[0]-$unpanned.translation[0]) -gt 20} 'Mouse right-drag did not pan'
 if((Signature) -ne $seeded){throw 'Mouse right-drag painted'}
 [CapyRowPointer]::Verify();[CapyRowPointer]::Dispose()
 $drawing=Signature;Capture 'before-touch'
 [CapyCanvasTouch]::Initialize([uint32]$review.Id)
 $before=Camera;[CapyCanvasTouch]::Down(1,($cx-100),$cy)
 for($i=1;$i -le 10;$i++){[CapyCanvasTouch]::Move(1,($cx-100+$i*4),($cy+$i*3));Start-Sleep -Milliseconds 12}
 Stable 'single finger neither paints nor navigates with Pen' $before
 [CapyCanvasTouch]::Up(1);Stable 'single-finger release does not jump' $before
 $panClock=[Diagnostics.Stopwatch]::StartNew()
 [CapyCanvasTouch]::Down(1,($cx-100),$cy);[CapyCanvasTouch]::Down(2,($cx+100),$cy)
 Stable 'adding second finger does not jump' $before
 $checks.Add(@{name='pan input timing';before_first_move_ms=$panClock.Elapsed.TotalMilliseconds})
 for($i=1;$i -le 10;$i++){[CapyCanvasTouch]::Pair(($cx-100+$i*6),($cy+$i*3),($cx+100+$i*6),($cy+$i*3));Start-Sleep -Milliseconds 12}
 $expected=$before|ConvertTo-Json -Depth 8|ConvertFrom-Json;$expected.translation[0]+=60;$expected.translation[1]+=30
 Check 'two-finger pan preserves scale and rotation' {(Matches-Gesture (Camera) $expected)}
 $pan=Camera;$ax=$cx+60;$ay=$cy+30
 for($i=1;$i -le 10;$i++){[CapyCanvasTouch]::Pair(($ax-100-$i*5),$ay,($ax+100+$i*5),$ay);Start-Sleep -Milliseconds 12}
 $anchor=@(($ax-$bounds.X),($ay-$bounds.Y))
 $expected=$pan|ConvertTo-Json -Depth 8|ConvertFrom-Json;$expected.zoom*=1.5
 for($axis=0;$axis -lt 2;$axis++){$expected.translation[$axis]=$anchor[$axis]+1.5*($pan.translation[$axis]-$anchor[$axis])}
 Check 'pinch scales by 1.5 around its midpoint' {(Matches-Gesture (Camera) $expected)}
 $zoomed=Camera
 for($i=1;$i -le 20;$i++){$angle=$i*[Math]::PI/40;$dx=[int](150*[Math]::Cos($angle));$dy=[int](150*[Math]::Sin($angle));[CapyCanvasTouch]::Pair(($ax-$dx),($ay-$dy),($ax+$dx),($ay+$dy));Start-Sleep -Milliseconds 12}
 $expected=$zoomed|ConvertTo-Json -Depth 8|ConvertFrom-Json;$expected.rotation+=[Math]::PI/2
 $expected.translation[0]=$anchor[0]-($zoomed.translation[1]-$anchor[1]);$expected.translation[1]=$anchor[1]+($zoomed.translation[0]-$anchor[0])
 Check 'two-finger quarter-turn preserves midpoint and scale' {(Matches-Gesture (Camera) $expected)}
 Capture 'rotated-touch';$turned=Camera
 [CapyCanvasTouch]::Down(3,($ax+90),$ay);Stable 'third finger rebases without a jump' $turned
 [CapyCanvasTouch]::Move(3,($ax+110),($ay+20));Stable 'three fingers suspend two-finger navigation' $turned
 [CapyCanvasTouch]::Up(2);Stable 'removing an original finger rebases the replacement pair' $turned
 [CapyCanvasTouch]::Move(3,($ax+135),($ay+35));Check 'remaining two fingers resume navigation' {!(Same-Camera (Camera) $turned)}
 [CapyCanvasTouch]::Up(3);$one=Camera;[CapyCanvasTouch]::Move(1,($ax+20),($ay-140));Stable 'remaining single finger does not pan with Pen' $one
 [CapyCanvasTouch]::Up(1);Stable 'all contacts released without a jump' $one
 foreach($attempt in 1..3){
  [CapyCanvasTouch]::Down(1,($cx-100),$cy);[CapyCanvasTouch]::Down(2,($cx+100),$cy);$cancelled=Camera
  [CapyCanvasTouch]::CancelAll();Stable "cancelled touch pair $attempt preserves the drawing and camera" $cancelled
 }
 [CapyCanvasTouch]::Down(1,($cx-100),$cy);[CapyCanvasTouch]::Down(2,($cx+100),$cy)
 Stable 'fresh contact pair after cancellation does not jump' $cancelled
 [CapyCanvasTouch]::Pair(($cx-70),($cy+20),($cx+130),($cy+20))
 $expected=$cancelled|ConvertTo-Json -Depth 8|ConvertFrom-Json;$expected.translation[0]+=30;$expected.translation[1]+=20
 Check 'new contacts pan normally after cancellation' {(Matches-Gesture (Camera) $expected)}
 [CapyCanvasTouch]::Up(2);[CapyCanvasTouch]::Up(1)
 Select-Tool 'hand';$hand=Camera;[CapyCanvasTouch]::Down(1,$cx,$cy);[CapyCanvasTouch]::Move(1,($cx+40),($cy+25))
 $expected=$hand|ConvertTo-Json -Depth 8|ConvertFrom-Json;$expected.translation[0]+=40;$expected.translation[1]+=25
 Check 'Hand permits one-finger pan' {(Matches-Gesture (Camera) $expected)}
 [CapyCanvasTouch]::Up(1);[CapyCanvasTouch]::Dispose()
 Stable 'Hand release preserves the drawing and camera' (Camera)
 Select-Tool 'pen';Fit-Canvas;Start-Sleep -Milliseconds 200
 Check 'returning to Pen and Fit preserves the drawing' {$true}
 [CapyCanvasTouch]::Initialize([uint32]$review.Id)
 $resting=Camera;[CapyCanvasTouch]::Down(1,($cx-100),$cy);Start-Sleep -Milliseconds 120
 (Control 'drawing-canvas').SetFocus()
 [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),[uint16]0xBC)
 Wait-Until {$null -ne (Find 'Preferences' -Name)} 'Preferences did not open over a resting finger'
 [CapyCanvasTouch]::Up(1);Start-Sleep -Milliseconds 150
 Invoke 'CloseButton'
 Wait-Until {$null -eq (Find 'Preferences' -Name)} 'Preferences did not close'
 [CapyCanvasTouch]::SetForegroundWindow($handle)|Out-Null;Start-Sleep -Milliseconds 150
 [CapyCanvasTouch]::Down(1,($cx+40),$cy)
 for($i=1;$i -le 10;$i++){[CapyCanvasTouch]::Move(1,($cx+40+$i*6),($cy+$i*4));Start-Sleep -Milliseconds 12}
 Stable 'a finger lifted under a modal dialog does not pair with the next finger' $resting
 [CapyCanvasTouch]::Up(1);[CapyCanvasTouch]::Dispose()
 [CapyRowPointer]::Initialize([uint32]$review.Id)
 $revision=(Model).state.document_file.revision
 [CapyRowPointer]::Down('pen',($cx-70),($cy+60))
 try{for($i=1;$i -le 28;$i++){[CapyRowPointer]::Move(($cx-70+$i*5),($cy+60+[int](12*[Math]::Sin($i/4))));Start-Sleep -Milliseconds 8}}finally{[CapyRowPointer]::Up()}
 Start-Sleep -Milliseconds 300
 Wait-Until {(Model).state.document_file.revision -gt $revision} 'Pen stroke after touch did not finish'
 @{before_pen_revision=$revision;after_pen=(Model).state.document_file}|ConvertTo-Json -Compress|Write-Output
 Capture 'pen-after-touch';[CapyRowPointer]::Verify();[CapyRowPointer]::Dispose()
 function Redo {((Model).state.commands|Where-Object id -eq 'redo').enabled}
 [CapyCanvasTouch]::Initialize([uint32]$review.Id)
 if(Redo){throw 'Redo was available before the finger taps'}
 $tapped=Camera
 [CapyCanvasTouch]::Down(1,($cx-80),$cy);[CapyCanvasTouch]::Down(2,($cx+80),$cy);Start-Sleep -Milliseconds 60;[CapyCanvasTouch]::Up(2);[CapyCanvasTouch]::Up(1)
 Wait-Until {Redo} 'A two-finger tap did not undo'
 if(!(Same-Camera (Camera) $tapped)){throw 'A two-finger tap moved the view'}
 [CapyCanvasTouch]::Dispose()
 Invoke 'Redo' -Name
 Wait-Until {!(Redo)} 'Redo did not restore the pen stroke after the tap'
 Write-Output 'two-finger tap undoes without moving the view passed'
 $revision=(Model).state.document_file.revision;Invoke 'Undo' -Name
 Wait-Until {(Model).state.document_file.revision -gt $revision -and (Model).state.document_file.modified} 'One Undo did not leave the seed drawing'
 $revision=(Model).state.document_file.revision;Invoke 'Undo' -Name
 Wait-Until {(Model).state.document_file.revision -gt $revision -and !(Model).state.document_file.modified} 'Two independent Undo steps did not remove pen and seed strokes'
 & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $handle.ToInt64() -Action Close
 if((Get-Item -LiteralPath (Join-Path $run 'stderr.log')).Length){throw 'Touch runtime stderr requires inspection'}
 @{checks=$checks;drawing='preserved throughout navigation';pen_after_touch='new stroke and independent Undo';close='zero exit within five seconds';scope='OS-injected multi-touch and pen; physical digitizers, pressure/tilt and 120 Hz acceptance remain separate'}|ConvertTo-Json -Depth 12|Set-Content -LiteralPath (Join-Path $run 'result.json')
 Write-Output "Canvas touch acceptance passed: $run"
}catch{$fixtureFailed=$true;($_|Out-String)+$_.ScriptStackTrace|Set-Content -LiteralPath (Join-Path $run 'failure.txt');@{camera=Camera;last_expected=$expected;checks=$checks;drawing=$drawing;signature=(Signature)}|ConvertTo-Json -Depth 12|Set-Content -LiteralPath (Join-Path $run 'failure-state.json');if($review -and !$review.HasExited -and $root){try{Capture 'failure'}catch{}};throw}
finally{
 $cleanup=@()
 foreach($finish in @({[CapyCanvasTouch]::Dispose()},{[CapyRowPointer]::Dispose()},{Exit-CapyEnvironment})){try{& $finish}catch{$cleanup+=($_|Out-String)}}
 if($cleanup.Count){$cleanup|Set-Content -LiteralPath (Join-Path $run 'cleanup-failure.txt');if(!$fixtureFailed){throw ($cleanup -join "`n")}}
}
