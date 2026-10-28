param([Parameter(Mandatory)][string]$Executable,[ValidateSet('dark','light')][string]$Theme='dark',[ValidateSet('full','preview-dark','preview-light','release-dark','release-light')][string]$Journey='full')
if($Journey -ne 'full'){$Theme=$Journey -replace '^(preview|release)-',''}
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyCacheModel=$true;$CapyEach={if($env:CAPY_PRIVATE_DESKTOP){[CapyRowPointer]::VerifyPrivateDesktop($env:CAPY_PRIVATE_DESKTOP)};[CapyCanvasTouch]::Verify()}
Add-Type -AssemblyName System.Drawing
Add-Type -Path (Join-Path $PSScriptRoot 'CanvasTouchDriver.cs')
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
if($env:CAPY_PRIVATE_DESKTOP){[CapyRowPointer]::VerifyPrivateDesktop($env:CAPY_PRIVATE_DESKTOP)}
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$run=Join-Path $repo ('artifacts/windows/tab-drag/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$directory=$run;$CapyTraceDirectory=$run
function Find-Preview {
 # Preview copies are excluded from the accessible control/content views.
 # Inspect only the workspace's direct raw children for this visual fixture.
 $workspace=Workspace-Root
 if(!$workspace){return}
 $walker=[System.Windows.Automation.TreeWalker]::RawViewWalker
 $child=$walker.GetFirstChild($workspace)
 while($child){
  if($child.Current.AutomationId -eq 'workspace-tab-preview'){return $child}
  $child=$walker.GetNextSibling($child)
 }
}
function Preview {
 $element=Find-Preview
 if($element){try{$element.Current.ItemStatus|ConvertFrom-Json}catch{}}
}
function Presentation {
 $workspace=Workspace-Root
 if($workspace){try{$workspace.Current.ItemStatus|ConvertFrom-Json}catch{}}
}
function Current-Group {
 $group=(Model).layout.groups|Where-Object {$_.panels -contains 'tool_settings'}
 if(!$group){return}
 # The full model deliberately retains the drag's initial placement.
 # Read the arranged native frame from the opt-in presentation diagnostics.
 $copy=$group|ConvertTo-Json -Depth 50 -Compress|ConvertFrom-Json
 $shown=(Presentation).groups|Where-Object id -eq $group.id
 if($shown){$copy.bounds=$shown.bounds}
 $copy
}
function Check-Motion($Before,$After,[uint32]$Group) {
 if($After.full_updates -ne $Before.full_updates -or $After.model_revision -ne $Before.model_revision){
  throw 'Steady drag refreshed retained UI models'
 }
 if($After.motion_updates -le $Before.motion_updates -or $After.revision -le $Before.revision){
  throw 'Steady drag did not apply incremental workspace presentation'
 }
 $position=$After.groups|Where-Object id -eq $Group
 $core=$After.workspace_update.drag.group
 if(!$position -or !$core -or $core.id -ne $Group){throw 'Missing shared/native floating placement'}
 foreach($field in @('x','y','width','height')){
  if([Math]::Abs($position.bounds.$field-$core.bounds.$field) -gt 1){throw "Native $field differs from the shared floating placement"}
 }
 $old=$Before.groups|Where-Object id -eq $Group
 $handles=@($Before.handles|Where-Object {$_.id -like "floating-$Group-*"})
 if(!$handles.Count){throw 'Fixture did not observe native floating resize grips'}
 foreach($handle in $handles){
  $moved=$After.handles|Where-Object id -eq $handle.id
  if(!$moved){throw 'Floating resize grip disappeared during motion'}
  foreach($axis in @('x','y')){
   if([Math]::Abs(($moved.bounds.$axis-$handle.bounds.$axis)-($position.bounds.$axis-$old.bounds.$axis)) -gt 1){
    throw 'Native resize grip did not move with the panel'
   }
  }
 }
}
function Check-OverviewOverlap([string]$Before,[string]$After) {
 $overview=(Control 'navigator-overview').Current.BoundingRectangle
 $origin=[CapyRowPointer+Point]::new()
 if(![CapyRowPointer]::ClientToScreen($drawingWindow,[ref]$origin)){throw 'Cannot locate client capture'}
 $model=Model;$document=$model.state.tabs[0]
 $fit=[Math]::Min(($overview.Width-8*$scale)/$document.width,($overview.Height-8*$scale)/$document.height)
 $width=$document.width*$fit;$height=$document.height*$fit
 $left=$overview.Left-$origin.x+($overview.Width-$width)/2
 $top=$overview.Top-$origin.y+($overview.Height-$height)/2
 $lower=($model.layout.groups|Where-Object active -eq 'brushes').bounds
 $hint=(Presentation).workspace_update.drag.drop_hint
 $accent=[Drawing.ColorTranslator]::FromHtml($model.state.palette.accent)
 $tinted=@($accent.R,$accent.G,$accent.B)|ForEach-Object {[int][Math]::Round(191+$_*64/255.)}
 $beforeImage=[Drawing.Bitmap]::new((Join-Path $run ($Before+'.png')))
 $afterImage=[Drawing.Bitmap]::new((Join-Path $run ($After+'.png')))
 try{
  $tested=0;$matching=0;$point=$null
  $referenceX=[int]($left+$width*.95)
  if($referenceX -le ($lower.x+$lower.width)*$scale+3){throw 'Navigator overview has no unobscured reference pixel'}
  # Sample an area: the legitimate camera outline can cross any one pixel.
  foreach($ix in 0..15){foreach($iy in 0..15){
   $fx=($ix+.5)/16;$fy=($iy+.5)/16
   $x=[int]($left+$width*$fx);$y=[int]($top+$height*$fy)
   if($x -le $lower.x*$scale -or $x -ge ($lower.x+$lower.width)*$scale -or
      $y -le ($lower.y+36)*$scale -or $y -ge ($lower.y+$lower.height)*$scale){continue}
   $old=$beforeImage.GetPixel($x,$y);$pixel=$afterImage.GetPixel($x,$y)
   if($old.R -gt 245 -and $old.G -gt 245 -and $old.B -gt 245){continue}
   $tested++
   $reference=$afterImage.GetPixel($referenceX,$y)
   $expected=@($reference.R,$reference.G,$reference.B);$h=$hint.bounds
   if($hint.target.kind -eq 'tab' -and $x -gt ($h.x+2)*$scale -and $x -lt ($h.x+$h.width-2)*$scale -and
      $y -gt ($h.y+2)*$scale -and $y -lt ($h.y+$h.height-2)*$scale){$expected=$tinted}
   if([Math]::Abs([int]$pixel.R-$expected[0]) -le 3 -and [Math]::Abs([int]$pixel.G-$expected[1]) -le 3 -and [Math]::Abs([int]$pixel.B-$expected[2]) -le 3){
    $matching++;if(!$point){$point=@{x=$x;y=$y;before=$old.ToArgb()}}
   }
  }}
  if($tested -lt 3){throw 'Navigator motion fixture did not cover enough opaque Tool Set pixels'}
  if($matching -lt [Math]::Ceiling($tested*.8)){throw "GPU Navigator did not move above the lower native panel ($matching of $tested samples)"}
  $point
 }finally{$beforeImage.Dispose();$afterImage.Dispose()}
}
function Walk([double]$FromX,[double]$FromY,[double]$ToX,[double]$ToY){
 for($i=1;$i -le 8;$i++){
  $x=[int]($FromX+($ToX-$FromX)*$i/8);$y=[int]($FromY+($ToY-$FromY)*$i/8)
  if($Journey -eq 'full'){[CapyCanvasTouch]::Move(1,$x,$y)}else{[CapyRowPointer]::Move($x,$y)}
  Start-Sleep -Milliseconds 18
 }
}
function Start-Slide([double]$Grab=8){
 (Control 'drawing-canvas').SetFocus()
 Invoke 'panel-tab-tool_settings'
 Wait-Until {@((Model).layout.groups|Where-Object {$_.panels -contains 'tool_settings'})[0].active -eq 'tool_settings'} 'Selected preview setup did not select Tool'
 if($Journey -notlike 'release-*' -and !$script:previewSelectionChecked){
  Wait-StablePixels {
   $box=(Control 'panel-tab-tool_settings' -Arranged).Current.BoundingRectangle
   "$box/$(Screen-Pixels ([int]($box.Left+$box.Width/2)) ([int]($box.Top+$box.Height/2)))"
  } -Composed|Set-Content (Join-Path $run 'selected-stable-pixels.txt')
  Capture 'selected-before-slide' -Composed
 }
 $selectedSource=(Control 'panel-tab-tool_settings' -Arranged).Current.BoundingRectangle
 $selectedNeighbor=(Control 'panel-tab-sizes' -Arranged).Current.BoundingRectangle
 Invoke 'panel-tab-sizes'
 Wait-Until {(Current-Group).active -eq 'sizes'} 'Inactive-source setup did not select Brush size'
 $source=(Control 'panel-tab-tool_settings').Current.BoundingRectangle
 $startX=$source.Left+$Grab*$scale;$startY=$source.Top+$source.Height/2
 if($Journey -eq 'full'){
  [CapyCanvasTouch]::Down(1,[int]$startX,[int]$startY)
  Start-Sleep -Milliseconds 20
 }else{
  [CapyRowPointer]::Down($(if($script:slideDevice){$script:slideDevice}else{'mouse'}),[int]$startX,[int]$startY)
  Wait-Until {@((Model).layout.groups|Where-Object {$_.panels -contains 'tool_settings'})[0].active -eq 'tool_settings'} 'Mouse down did not select Tool before dragging'
 }
 Wait-Until {
  $arranged=(Control 'panel-tab-tool_settings' -Arranged).Current.BoundingRectangle
  [Math]::Abs($arranged.Left-$selectedSource.Left) -le 1 -and [Math]::Abs($arranged.Width-$selectedSource.Width) -le 1
 } 'Selected tab did not return to its arranged geometry before pickup'
 $source=$selectedSource
 $finishX=$startX+$selectedNeighbor.Width/2+10*$scale
 Walk $startX $startY $finishX $startY
 Wait-Until {$null -ne (Preview) -and (Preview).insertion -eq 2} 'Attached native tab preview did not cross the shared insertion threshold'
 if((Presentation).workspace_update.drag.drop_hint){throw 'Attached tab preview showed an insertion target'}
 $actual=(Control 'panel-tab-tool_settings').Current.BoundingRectangle
 if([Math]::Abs($actual.Left-$source.Left) -gt 1 -or [Math]::Abs($actual.Width-$source.Width) -gt 1){throw 'Tab preview moved the original hit rectangle'}
 if(((Current-Group).panels -join ',') -ne 'tool_settings,sizes'){throw 'Preview prematurely committed tab order'}
 if($Journey -notlike 'release-*' -and !$script:previewSelectionChecked){
  $client=[CapyRowPointer+Point]::new();if(![CapyRowPointer]::ClientToScreen($drawingWindow,[ref]$client)){throw 'Cannot locate preview client'}
  Wait-StablePixels {
   $box=(Preview).bounds
   Screen-Pixels ([int]($client.x+($box.x+$box.width/2)*$scale)) ([int]($client.y+($box.y+$box.height/2)*$scale))
  } -Composed|Set-Content (Join-Path $run 'preview-stable-pixels.txt')
  Capture 'inactive-source-preview' -Composed
  $origin=[CapyRowPointer+Point]::new();if(![CapyRowPointer]::ClientToScreen($drawingWindow,[ref]$origin)){throw 'Cannot locate preview capture'}
  $before=[Drawing.Bitmap]::new((Join-Path $run 'selected-before-slide.png'))
  $after=[Drawing.Bitmap]::new((Join-Path $run 'inactive-source-preview.png'))
  try{
   $offset=(Preview).bounds.x*$scale+$origin.x-$source.Left
   foreach($dx in @(3,5)){
    $x=[int]($source.Left+$dx*$scale-$origin.x);$y=[int]($source.Bottom-2*$scale-$origin.y)
    if($before.GetPixel($x,$y).ToArgb() -ne $after.GetPixel([int]($x+$offset),$y).ToArgb()){throw 'Inactive-source preview lost the selected tab surface'}
   }
  }finally{$before.Dispose();$after.Dispose()}
  $script:previewSelectionChecked=$true
 }
 [pscustomobject]@{x=$finishX;y=$startY}
}
function Undo-Workspace {
 Wait-Until {$null -eq (Find-Preview)} 'Tab overlay survived release'
 Wait-Until {@((Model).state.commands|Where-Object {$_.id -eq 'undo_workspace' -and $_.enabled}).Count -eq 1} 'Released drag did not become undoable'
 Start-Sleep -Milliseconds 200
 & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'window';Invoke 'undo_workspace'
 Wait-Until {((Current-Group).panels -join ',') -eq 'tool_settings,sizes' -and !(Current-Group).floating} 'Workspace Undo did not restore the source group'
}
try{
 Enter-CapyEnvironment
 $env:CAPY_STORAGE_DIR=Join-Path $run 'profile'
 [IO.File]::WriteAllText((Settings-File),(@{language=@{Explicit='en'};theme=$Theme}|ConvertTo-Json -Compress))
 $env:CAPY_TRACE_UI='1';$env:CAPY_SMOKE_TEST='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
 $stderr=Join-Path $run 'stderr.log'
 $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
 $null=$review.Handle
 [IO.File]::WriteAllText((Join-Path $repo 'artifacts/windows/tab-drag-review.json'),(@{process_id=$review.Id;run=$run}|ConvertTo-Json))
 Write-Output "Owned tab review $($review.Id)"
 $native=@{window=$null}
 Wait-Until {$native.window=Owned-DrawingWindow $review;$native.window -and (Model).brush_ready -and (Model).windows_workspace.ready -and !(Model).windows_workspace.busy} 'Review did not start' 75
 $drawingWindow=$native.window.Handle;$root=$native.window.Root
 [CapyCanvasTouch]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
 [CapyWindowApi]::ShowWindow($drawingWindow,3)|Out-Null
 [CapyCanvasTouch]::SetForegroundWindow($drawingWindow)|Out-Null
 [CapyCanvasTouch]::Initialize([uint32]$review.Id)
 if($Journey -ne 'full'){[CapyRowPointer]::Initialize([uint32]$review.Id)}
 $scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
 if($scale -le 0 -or [double]::IsNaN($scale) -or [double]::IsInfinity($scale)){throw 'Invalid run: owned window DPI is unavailable'}
 $viewportProof=@{}
 try{Wait-Until {
  $nativeBounds=[CapyRowPointer+Rect]::new()
  if(![CapyRowPointer]::GetClientRect($drawingWindow,[ref]$nativeBounds)){return $false}
  $viewport=(Model).layout.viewport
  $width=($nativeBounds.right-$nativeBounds.left)/$scale;$height=($nativeBounds.bottom-$nativeBounds.top)/$scale
  $viewportProof.native=@($width,$height);$viewportProof.shared=$viewport;$viewportProof.dpi=96*$scale
  [Math]::Abs($width-$viewport[0]) -le 1 -and [Math]::Abs($height-$viewport[1]) -le 1
 } 'Maximized native workspace did not match its shared viewport'}
 finally{$viewportProof|ConvertTo-Json|Set-Content (Join-Path $run 'maximized-viewport.json')}
 Start-Sleep -Milliseconds 350
 $baseline=Model
 $normal=$baseline.state.workspace.layout|ConvertTo-Json -Depth 80 -Compress
 $baseline|ConvertTo-Json -Depth 90|Set-Content (Join-Path $run 'before-model.json')
 if($Journey -like 'release-*'){
  foreach($script:slideDevice in @('mouse','pen','touch')){
   $position=Start-Slide
   $retainedSource=(Control 'panel-tab-tool_settings').GetRuntimeId() -join ':'
   $retainedNeighbor=(Control 'panel-tab-sizes').GetRuntimeId() -join ':'
   $retainedField=(Control 'Brush size' -Name).GetRuntimeId() -join ':'
   Capture "before-release-$slideDevice" -Composed
   [CapyRowPointer]::Up()
   Wait-Until {$null -eq (Find-Preview)} 'Drop preview did not hand over to the committed layout'
   $shown=Presentation
   if($shown.workspace_update.drag){throw 'Drop exposed the original controls before the final workspace publication'}
   Wait-Until {((Current-Group).panels -join ',') -eq 'sizes,tool_settings'} 'Release did not commit the preview insertion'
   foreach($target in @(@('panel-tab-tool_settings',$retainedSource),@('panel-tab-sizes',$retainedNeighbor),@('Brush size',$retainedField))){
    $control=Control $target[0] -Name:($target[0] -eq 'Brush size')
    if(($control.GetRuntimeId() -join ':') -ne $target[1]){throw "Drop replaced retained control $($target[0])"}
   }
   Capture "after-release-$slideDevice" -Composed
   Undo-Workspace
  }
  $script:slideDevice='mouse'
  $position=Start-Slide
  $canvas=(Control 'drawing-canvas').Current.BoundingRectangle
  $x=$canvas.Left+$canvas.Width*.55;$y=$canvas.Top+$canvas.Height*.6
  Walk $position.x $position.y $x $y
  Wait-Until {(Current-Group).floating -and $null -eq (Find-Preview)} 'Detached mouse tab did not leave its attached preview'
  $target=(Control 'panel-tab-brushes' -Arranged).Current.BoundingRectangle
  $targetX=$target.Left+$target.Width/2;$targetY=$target.Top+$target.Height/2
  Walk $x $y $targetX $targetY
  Wait-Until {(Presentation).workspace_update.drag.drop_hint} 'Detached mouse tab did not show a target over Tool Set'
  Capture 'detached-drop-target-mouse' -Composed
  Walk $targetX $targetY $x $y
  Wait-Until {(Current-Group).floating -and !(Presentation).workspace_update.drag.drop_hint} 'Detached mouse tab did not leave the target'
  [CapyRowPointer]::Up()
  Undo-Workspace
  & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -StateDirectory $run -Action Close -DiscardUnsaved
  if(!$review.WaitForExit(15000) -or $review.ExitCode -ne 0){throw 'Release review did not close cleanly'}
  @{theme=$Theme;devices=@('mouse','pen','touch');retained_tabs='passed';retained_content='passed';committed_handoff='passed';attached_hint='hidden';detached_hint='shown'}|ConvertTo-Json
  return
 }
 $position=Start-Slide
 Capture 'attached' -Composed
 if($Journey -ne 'full'){
  [CapyRowPointer]::Key([uint32]$review.Id,0x1b);[CapyRowPointer]::Up()
  Wait-Until {$null -eq (Find-Preview)} 'Focused preview cancellation retained the overlay'
  & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -StateDirectory $run -Action Close -DiscardUnsaved
  if(!$review.WaitForExit(15000) -or $review.ExitCode -ne 0){throw 'Focused preview window did not close cleanly'}
  if((Get-Item -LiteralPath $stderr).Length){throw 'Focused preview wrote native stderr'}
  [pscustomobject]@{theme=$Theme;mouse_contact='passed';inactive_source_down='passed';selected_preview_pixels='passed';fixed_hit_rectangles='passed';cancel='passed';zero_exit='passed'}|ConvertTo-Json
  return
 }
 [CapyCanvasTouch]::Up(1)
 Wait-Until {((Current-Group).panels -join ',') -eq 'sizes,tool_settings'} 'Release did not commit the preview insertion'
 Undo-Workspace
 $position=Start-Slide 35
 Capture 'second-grab' -Composed
 [CapyCanvasTouch]::CancelAll()
 Wait-Until {$null -eq (Find-Preview)} 'Cancelled pointer retained the overlay'
 Wait-Until {((Model).state.workspace.layout|ConvertTo-Json -Depth 80 -Compress) -eq $normal} 'Cancellation changed the workspace'
 $position=Start-Slide
 $canvas=(Control 'drawing-canvas').Current.BoundingRectangle
 $x=$canvas.Left+$canvas.Width*.55;$y=$canvas.Top+$canvas.Height*.6
 Walk $position.x $position.y $x $y
 Wait-Until {(Current-Group).floating -and $null -eq (Find-Preview)} 'Tear-off did not release the attached preview'
 $target=(Control 'panel-tab-brushes' -Arranged).Current.BoundingRectangle
 $targetX=$target.Left+$target.Width/2;$targetY=$target.Top+$target.Height/2
 Walk $x $y $targetX $targetY
 Wait-Until {(Presentation).workspace_update.drag.drop_hint} 'Detached tab drag did not show a drop target over Tool Set'
 Capture 'detached-drop-target' -Composed
 Walk $targetX $targetY $x $y
 Wait-Until {(Current-Group).floating -and !(Presentation).workspace_update.drag.drop_hint} 'Detached tab did not leave the dock target'
 Capture 'detached' -Composed
 Start-Sleep -Milliseconds 800
 $floating=Current-Group;$floatingX=$floating.bounds.x
 $before=Presentation
 $retainedTab=(Control 'panel-tab-tool_settings').GetRuntimeId() -join ':'
 $retainedField=(Control 'Brush size' -Name).GetRuntimeId() -join ':'
 Walk $x $y ($x+45*$scale) $y
 Wait-Until {(Current-Group).floating -and (Current-Group).bounds.x -gt $floatingX+30} 'Floating panel stopped tracking the held contact after its source changed'
 $after=Presentation;Check-Motion $before $after $floating.id
 if(((Control 'panel-tab-tool_settings').GetRuntimeId() -join ':') -ne $retainedTab -or
    ((Control 'Brush size' -Name).GetRuntimeId() -join ':') -ne $retainedField){throw 'Steady drag replaced native controls'}
 [IO.File]::WriteAllText((Join-Path $run 'retained-motion.json'),(@{before=$before;after=$after}|ConvertTo-Json -Depth 60))
 [CapyCanvasTouch]::Up(1)
 Undo-Workspace
 $position=Start-Slide
 Walk $position.x $position.y $x $y
 Wait-Until {(Current-Group).floating -and $null -eq (Find-Preview)} 'Cancellation review did not tear off'
 Start-Sleep -Milliseconds 400
 [CapyCanvasTouch]::CancelAll()
 Wait-Until {((Model).state.workspace.layout|ConvertTo-Json -Depth 80 -Compress) -eq $normal} 'Detached cancellation did not restore the workspace'
 if((Model).state.document_file.modified){throw 'Workspace dragging modified the drawing'}

 (Control 'drawing-canvas').SetFocus()
 Capture 'navigator-before' -Composed
 $source=(Control 'panel-tab-navigator').Current.BoundingRectangle
 $startX=$source.Left+8*$scale;$startY=$source.Top+$source.Height/2
 $x=$canvas.Left+180*$scale;$y=$canvas.Top+90*$scale
 [CapyCanvasTouch]::Down(1,[int]$startX,[int]$startY)
 Walk $startX $startY $x $y
 Wait-Until {@((Model).layout.groups|Where-Object {$_.active -eq 'navigator' -and $_.floating}).Count -eq 1} 'Navigator did not tear off'
 Start-Sleep -Milliseconds 600
 $navigator=(Model).layout.groups|Where-Object {$_.active -eq 'navigator' -and $_.floating}
 $before=Presentation
 $retained=(Control 'navigator-zoom_in').GetRuntimeId() -join ':'
 Capture 'navigator-first' -Composed
 $firstPixel=Check-OverviewOverlap 'navigator-before' 'navigator-first'
 $oldCamera=(Control 'canvas-camera').Current.Name
 Invoke 'navigator-zoom_in'
 Wait-Until {(Control 'canvas-camera').Current.Name -ne $oldCamera -and (Presentation).model_revision -gt $before.model_revision} 'Mixed camera update did not publish its native controls and model'
 $cameraApplied=Presentation
 Walk $x $y ($x+45*$scale) ($y+12*$scale)
 Wait-Until {
  $p=Presentation;$g=$p.groups|Where-Object id -eq $navigator.id
  $old=$before.groups|Where-Object id -eq $navigator.id
  $g.bounds.x -gt $old.bounds.x+30
 } 'Native Navigator stopped moving'
 $after=Presentation
 [IO.File]::WriteAllText((Join-Path $run 'navigator-motion.json'),(@{before=$before;camera_applied=$cameraApplied;after=$after}|ConvertTo-Json -Depth 60))
 Check-Motion $cameraApplied $after $navigator.id
 if(((Control 'navigator-zoom_in').GetRuntimeId() -join ':') -ne $retained){throw 'Navigator motion replaced native controls'}
 if($before.overviews.Count -ne 1 -or $after.overviews.Count -ne 1){throw 'Navigator fixture did not retain exactly one GPU overview'}
 $old=$before.groups|Where-Object id -eq $navigator.id;$moved=$after.groups|Where-Object id -eq $navigator.id
 if([Math]::Abs(($after.overviews[0].bounds[0]-$before.overviews[0].bounds[0])-($moved.bounds.x-$old.bounds.x)) -gt 1 -or
    [Math]::Abs(($after.overviews[0].bounds[1]-$before.overviews[0].bounds[1])-($moved.bounds.y-$old.bounds.y)) -gt 1){
  throw 'GPU overview allocation did not follow native Navigator placement'
 }
 Capture 'navigator-moved' -Composed
 $secondPixel=Check-OverviewOverlap 'navigator-before' 'navigator-moved'
 [CapyCanvasTouch]::CancelAll()
 Wait-Until {((Model).state.workspace.layout|ConvertTo-Json -Depth 80 -Compress) -eq $normal} 'Navigator cancellation did not restore workspace layout'
 Wait-Until {
  Capture 'navigator-restored' -Composed
  $restored=[Drawing.Bitmap]::new((Join-Path $run 'navigator-restored.png'))
  try{foreach($point in @($firstPixel,$secondPixel)){
   $pixel=$restored.GetPixel($point.x,$point.y);$old=[Drawing.Color]::FromArgb($point.before)
   if([Math]::Abs([int]$pixel.R-$old.R) -gt 3 -or [Math]::Abs([int]$pixel.G-$old.G) -gt 3 -or [Math]::Abs([int]$pixel.B-$old.B) -gt 3){return $false}
  };$true}finally{$restored.Dispose()}
 } 'Navigator cancellation did not restore the lower native panel pixels'
 if((Model).state.document_file.modified){throw 'Navigator workspace/camera motion modified the drawing'}
 & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -StateDirectory $run -Action Close -DiscardUnsaved
 if(!$review.WaitForExit(15000)){throw 'Owned tab-drag window did not close'}
 if($review.ExitCode -ne 0){throw "Owned tab-drag process exited $($review.ExitCode)"}
 if((Get-Item -LiteralPath $stderr).Length){throw 'Native stderr requires inspection'}
 [pscustomobject]@{os_touch_routing='passed';attached_preview='passed';fixed_hit_rectangles='passed';release_insertion='passed';different_grab_position='passed';pointer_cancel='passed';tear_off='passed';held_floating_drag='passed';incremental_publication='passed';retained_native_controls='passed';resize_grip_placement='passed';navigator_gpu_motion='passed';fast_tearoff_capture='passed';mixed_camera='passed';overview_occlusion_and_restore='passed';detached_cancel='passed';workspace_undo='passed';zero_exit='passed';scope='OS-injected touch; physical input and latency acceptance remain separate'}|ConvertTo-Json
}catch{
 try{Capture 'failure' -WithModel -Composed}catch{}
 [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{
 try{[CapyRowPointer]::Dispose()}catch{}
 try{[CapyCanvasTouch]::Dispose()}catch{}
 if($review -and !$review.HasExited){if($drawingWindow){$null=[CapyWindowApi]::PostMessage($drawingWindow,0x10,[UIntPtr]::Zero,[IntPtr]::Zero)};if(!$review.WaitForExit(10000)){$review.Kill();$review.WaitForExit()}}
 Exit-CapyEnvironment
}
