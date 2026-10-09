param([Parameter(Mandatory)][string]$Executable,[ValidateSet('dark','light')][string]$Theme='dark',[switch]$Navigation,[ValidateSet('mouse','pen','touch')][string]$Device='mouse')
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
. (Join-Path $PSScriptRoot 'LocalizationProfiles.ps1')
$CapyWaitSeconds=20
$CapyEach={[CapyRowPointer]::Verify()}
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
if($Navigation){[CapyRowPointer]::VerifyPrivateDesktop($env:CAPY_PRIVATE_DESKTOP)}
Add-Type -AssemblyName System.Drawing
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$run=Join-Path $repo ('artifacts/windows/zoom-readout/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$CapyTraceDirectory=$run
$checks=[ordered]@{}
function Zoom{(Model).state.camera.zoom}
function Near([double]$Value,[double]$Expected){[Math]::Abs($Value-$Expected) -lt .001}
function Center($Element){$r=$Element.Current.BoundingRectangle;@([int]($r.X+$r.Width/2),[int]($r.Y+$r.Height/2))}
function Tap([string]$Device,$Element){$at=Center $Element;[CapyRowPointer]::Down($Device,$at[0],$at[1]);Start-Sleep -Milliseconds 40;[CapyRowPointer]::Up();Start-Sleep -Milliseconds 150}
function Focused{(Find 'drawing-canvas').Current.HasKeyboardFocus}
function Readout-State([bool]$Open){
 $control=Find 'canvas-view-info';$state=if($Open){'open'}else{'closed'}
 if(!$control -or $control.Current.ItemStatus -ne (Catalog-Text (Model).windows_active_tag ('native-header-'+$state) 'common')){return $false}
 $item=Zoom-Item 'zoom-fit_canvas'
 if(!$Open){return !$item}
 $item -and !$item.Current.IsOffscreen -and $item.Current.BoundingRectangle.Width -gt 0 -and $item.Current.BoundingRectangle.Height -gt 0
}
function Caption-Bounds($Bounds){[pscustomobject]@{x=$Bounds.X;y=$Bounds.Y;width=$Bounds.Width;height=$Bounds.Height;right=$Bounds.Right;bottom=$Bounds.Bottom}}
function Native-Caption([string]$Id,[string]$Label){
 $hit=@{row=$null;caption=$null;texts=@()}
 Wait-Until {
  $hit.row=Zoom-Item $Id
  if(!$hit.row -or $hit.row.Current.IsOffscreen -or $hit.row.Current.ProcessId -ne $review.Id){return $false}
  $hit.texts=@($hit.row.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition)|Where-Object {!$_.Current.IsOffscreen -and ($_.Current.ControlType -eq [System.Windows.Automation.ControlType]::Text -or $_.Current.ClassName -eq 'TextBlock')})
  $matches=@($hit.texts|Where-Object {$_.Current.Name -eq $Label -and $_.Current.BoundingRectangle.Width -gt 0 -and $_.Current.BoundingRectangle.Height -gt 0})
  if($matches.Count -ne 1){return $false}
  $hit.caption=$matches[0];$bounds=$hit.row.Current.BoundingRectangle;$bounds.Inflate(1,1)
  $hit.caption.Current.ProcessId -eq $review.Id -and $bounds.Contains($hit.caption.Current.BoundingRectangle)
 } "Missing arranged native caption for $Id"
 $row=$hit.row.Current.BoundingRectangle;$caption=$hit.caption.Current.BoundingRectangle;$rowLimit=[Windows.Rect]::new($row.Location,$row.Size);$rowLimit.Inflate(1,1);$captionLimit=[Windows.Rect]::new($caption.Location,$caption.Size);$captionLimit.Inflate(1,1)
 $others=@($hit.texts|Where-Object {$_ -ne $hit.caption -and $_.Current.Name});$characters=@();$issues=@();$pattern=$null;$fullText=$null;$fullBounds=@()
 try{
  if(!$hit.caption.TryGetCurrentPattern([System.Windows.Automation.TextPattern]::Pattern,[ref]$pattern)){throw 'Caption does not expose TextPattern'}
  $full=$pattern.DocumentRange;$fullText=$full.GetText(-1);$fullBounds=@($full.GetBoundingRectangles()|ForEach-Object {Caption-Bounds $_})
  if($fullText -cne $Label){$issues+='TextPattern full text does not equal the shared caption'}
  $start=[System.Windows.Automation.Text.TextPatternRangeEndpoint]::Start;$end=[System.Windows.Automation.Text.TextPatternRangeEndpoint]::End
  $range=$full.Clone();$range.MoveEndpointByRange($end,$full,$start)
  for($i=0;$i -lt $Label.Length;$i++){
   $moved=$range.MoveEndpointByUnit($end,[System.Windows.Automation.Text.TextUnit]::Character,1);$text=$range.GetText(-1);$rectangles=@($range.GetBoundingRectangles());$expected=$Label.Substring($i,1)
   if($moved -ne 1 -or $text -cne $expected){$issues+="Character $i was not exposed as the expected single character"}
   if(![string]::IsNullOrWhiteSpace($expected)){
    if(!$rectangles.Count){$issues+="Character $i has no visible rectangle"}
    foreach($rectangle in $rectangles){
     if($rectangle.IsEmpty -or $rectangle.Width -le 0 -or $rectangle.Height -le 0 -or ![double]::IsFinite($rectangle.X+$rectangle.Y+$rectangle.Width+$rectangle.Height)){$issues+="Character $i has no positive finite rectangle";continue}
     if(!$captionLimit.Contains($rectangle) -or !$rowLimit.Contains($rectangle)){$issues+="Character $i extends outside its caption or row"}
     foreach($other in $others){$hint=$other.Current.BoundingRectangle;if($hint.Width -gt 0 -and $hint.Height -gt 0 -and $rectangle.Right -gt $hint.X+1 -and $rectangle.X -lt $hint.Right-1 -and $rectangle.Bottom -gt $hint.Y+1 -and $rectangle.Y -lt $hint.Bottom-1){$issues+="Character $i overlaps its native shortcut"}}
    }
   }
   $characters+=[pscustomobject]@{index=$i;expected=$expected;text=$text;moved=$moved;rectangles=@($rectangles|ForEach-Object {Caption-Bounds $_})}
   $range.MoveEndpointByRange($start,$range,$end)
  }
 }catch{$issues+='TextPattern observation failed: '+$_.Exception.Message}
 [pscustomobject]@{id=$Id;name=$hit.caption.Current.Name;row_name=$hit.row.Current.Name;process_id=$hit.row.Current.ProcessId;row_id=($hit.row.GetRuntimeId() -join ':');caption_id=($hit.caption.GetRuntimeId() -join ':');row=(Caption-Bounds $row);caption=(Caption-Bounds $caption);full_text=$fullText;full_rectangles=$fullBounds;characters=$characters;other_text=@($others|ForEach-Object {[pscustomobject]@{name=$_.Current.Name;bounds=(Caption-Bounds $_.Current.BoundingRectangle)}});issues=$issues}
}
function Check-ZoomCaptions{
 $before=Model;$captions=@();$issues=@()
 foreach($id in @('zoom_in','zoom_out','fit_canvas','actual_pixels')){
  $command=@($before.state.commands|Where-Object id -eq $id)
  if($command.Count -ne 1){throw "Shared command $id is missing"}
  $caption=Native-Caption ('zoom-'+$id) $command[0].label;$captions+=$caption;$issues+=@($caption.issues|ForEach-Object {"${id}: $_"})
 }
 Capture "zoom-caption-characters-$Theme" -Composed -WithModel
 [pscustomobject]@{theme=$Theme;captions=$captions;issues=$issues;rounding_allowance_physical_px=1;visual_review='required';limitation='Positive TextPattern rectangles can represent partial glyphs; inspect raw pixels for complete glyph shapes and occlusion.'}|ConvertTo-Json -Depth 16|Set-Content (Join-Path $run 'zoom-caption-characters.json')
 $after=Model;Assert-Camera $before.state.camera $after.state.camera
 if(($before.state.document_file|ConvertTo-Json -Depth 12 -Compress) -ne ($after.state.document_file|ConvertTo-Json -Depth 12 -Compress)){throw 'Inspecting zoom captions changed the document'}
 if($issues.Count){throw ($issues -join '; ')}
 $checks.caption_character_visibility='passed';$checks.caption_visual_review='required'
}
function Open-Readout([string]$Device){
 Tap $Device (Control 'canvas-view-info')
 Wait-Until {Readout-State $true} "$Device did not open the zoom menu"
 if(!(Focused)){throw "Opening the zoom menu with $Device took focus from the canvas"}
}
function Choose([string]$Device,[string]$Id){
 Tap $Device (Item $Id)
 Wait-Until {Readout-State $false} "Choosing $Id did not close the zoom menu"
}
function Item([string]$Id){$item=@{value=$null};Wait-Until {$item.value=Zoom-Item $Id;$item.value -and !$item.value.Current.IsOffscreen} "Missing zoom item $Id";$item.value}
function Camera{(Model).state.camera}
function Close-Menu{[CapyRowPointer]::Key([uint32]$review.Id,0x1B);Wait-Until {Readout-State $false} 'Escape did not close the zoom menu'}
function Wheel-At([int[]]$At,[switch]$Horizontal,[switch]$Control,[switch]$Shift,[scriptblock]$Acknowledge){
 try{
  if($Control){[CapyRowPointer]::Hold(0x11,$true)}
  if($Shift){[CapyRowPointer]::Hold(0x10,$true)}
  [CapyRowPointer]::Wheel($At[0],$At[1],120,[bool]$Horizontal)
  if($Acknowledge){& $Acknowledge}
 }finally{
  if($Shift){[CapyRowPointer]::Hold(0x10,$false)}
  if($Control){[CapyRowPointer]::Hold(0x11,$false)}
 }
}
function Control-Wheel([scriptblock]$Acknowledge){
 $at=Center (Control 'drawing-canvas' -Arranged);[CapyRowPointer]::Hover($at[0],$at[1]);Wheel-At $at -Control -Acknowledge $Acknowledge
}
function Changed-Camera($Before,[string]$Operation,[scriptblock]$Expected={param($Camera) $true}){
 $next=@{camera=$null}
 Wait-Until {$next.camera=Camera;$next.camera -and $next.camera.revision -gt $Before.revision -and (& $Expected $next.camera)} "$Operation did not advance the camera"
 $next.camera
}
function Assert-Camera($Before,$After,[double]$Dx=0,[double]$Dy=0){
 if(!$Before -or !$After -or !(Near $After.zoom $Before.zoom) -or !(Near $After.rotation $Before.rotation) -or
  [Math]::Abs($After.translation[0]-$Before.translation[0]-$Dx) -gt 1 -or
  [Math]::Abs($After.translation[1]-$Before.translation[1]-$Dy) -gt 1){
  throw ('Unexpected camera change: '+(@{before=$Before;after=$After;expected_pan=@($Dx,$Dy)}|ConvertTo-Json -Depth 8 -Compress))
 }
}
function Exact-Camera($Value){
 ConvertTo-Json -InputObject @($Value.zoom,$Value.rotation,$Value.translation,$Value.flipped,$Value.zoom_locked,$Value.rotation_locked) -Depth 8 -Compress
}
function Readout-SearchState{
 $path=State-File;$base=Read-Snapshot $path
 $searchPath=Join-Path (Split-Path -Parent $path) ((Split-Path -Leaf $path) -replace '^ui-state-','search-state-')
 $search=Read-Snapshot $searchPath
 if($base.process_id -ne $review.Id -or $search.process_id -ne $review.Id -or $search.window_id -ne $base.window_id -or 'search' -notin $search.PSObject.Properties.Name){throw 'The readout search acknowledgement is not from the owned drawing'}
 $search.search
}
function Check-ReadoutCamera{
 Navigation-Command 'actual_pixels'
 Wait-Until {(Camera).zoom -eq 1 -and (Control 'canvas-camera').Current.Name -eq '100% · 0°'} 'Actual Pixels did not settle before the readout camera check'
 $before=Camera;Navigation-Command 'fit_canvas'
 $fit=Changed-Camera $before 'Direct Fit before opening the readout' {param($Camera)$Camera.zoom -ne 1}
 Wait-Until {(Control 'canvas-camera').Current.Name -eq ([string][int][Math]::Round($fit.zoom*100)+'% · 0°') -and (Readout-State $false)} 'Direct Fit did not reach the native closed readout'
 $expected=Exact-Camera $fit;$artwork=Navigation-PaintState
 Capture "zoom-camera-fit-$Theme" -Composed -WithModel
 Open-Readout 'mouse'
 Capture "zoom-camera-open-$Theme" -Composed -WithModel
 if((Exact-Camera (Camera)) -cne $expected -or (Navigation-PaintState) -cne $artwork){throw 'Opening the zoom readout changed the exact camera, artwork or history'}
 Close-Menu
 Navigation-Focus
 [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),0x4b)
 Wait-Until {(Find 'command-search' -Visible) -and (Control 'command-search').Current.HasKeyboardFocus -and $null -ne (Readout-SearchState)} 'The readout camera check did not acknowledge its shared command-search roundtrip'
 [CapyRowPointer]::Key([uint32]$review.Id,0x1b)
 Wait-Until {!(Find 'command-search' -Visible) -and $null -eq (Readout-SearchState) -and (Focused)} 'The readout camera check did not settle after Escape'
 Capture "zoom-camera-closed-$Theme" -Composed -WithModel
 if((Exact-Camera (Camera)) -cne $expected -or (Navigation-PaintState) -cne $artwork){throw 'Closing the zoom readout changed the exact camera, artwork or history'}
 $checks.readout_preserves_exact_camera=@{camera=$fit;artwork_history='unchanged';acknowledgement='native readout open/closed and shared command-search open/closed';raw_visual_review='required'}
}
function Held-Wheel([string]$Button){
 $at=Center (Control 'drawing-canvas' -Arranged);$before=Camera;$drawing=(Model).state.document_file.revision
 [CapyRowPointer]::Down('mouse',$at[0],$at[1],$Button)
 try{
  Wheel-At $at;$vertical=Changed-Camera $before "$Button held vertical wheel" {param($Camera) $Camera.translation[1] -gt $before.translation[1]}
  $dy=$vertical.translation[1]-$before.translation[1]
  if($dy -le 0){throw "$Button held vertical wheel did not pan upward"}
  Assert-Camera $before $vertical 0 $dy
  Wheel-At $at -Horizontal;$horizontalCamera=Changed-Camera $vertical "$Button held horizontal wheel" {param($Camera) $Camera.translation[0] -lt $vertical.translation[0]}
  $dx=$horizontalCamera.translation[0]-$vertical.translation[0]
  if($dx -ge 0){throw "$Button held horizontal wheel did not pan rightward"}
  Assert-Camera $vertical $horizontalCamera $dx 0
  $shifted=Wheel-At $at -Shift -Acknowledge {Changed-Camera $horizontalCamera "$Button held Shift+wheel" {param($Camera) $Camera.translation[0] -gt $horizontalCamera.translation[0]}}
  Assert-Camera $horizontalCamera $shifted $dy 0
  $zoomed=Wheel-At $at -Control -Acknowledge {Changed-Camera $shifted "$Button held Ctrl+wheel" {param($Camera) $Camera.zoom -gt $shifted.zoom}}
  if($zoomed.zoom -le $shifted.zoom -or !(Near $zoomed.rotation $shifted.rotation)){throw "$Button held Ctrl+wheel did not zoom without rotation"}
  $canvas=(Control 'drawing-canvas' -Arranged).Current.BoundingRectangle
  for($axis=0;$axis -lt 2;$axis++){
   $anchor=$at[$axis]-@($canvas.X,$canvas.Y)[$axis]
   $expected=$anchor-($anchor-$shifted.translation[$axis])*$zoomed.zoom/$shifted.zoom
   if([Math]::Abs($zoomed.translation[$axis]-$expected) -gt 1){throw "$Button held zoom moved its anchor on axis $axis"}
  }
  [CapyRowPointer]::Move(($at[0]+24),($at[1]+16));$moved=Changed-Camera $zoomed "$Button motion after wheel" {param($Camera) !(Near $Camera.translation[0] $zoomed.translation[0])}
  Assert-Camera $zoomed $moved 24 16
  if($Button -eq 'right'){[CapyRowPointer]::Cancel()}else{[CapyRowPointer]::Up()}
  if([CapyRowPointer]::Active){throw "$Button cleanup retained the driver contact"}
  $hover=@(($at[0]+39),($at[1]+27));[CapyRowPointer]::Hover($hover[0],$hover[1]);Wheel-At $hover
  $released=Changed-Camera $moved "$Button cleanup followed by idle wheel" {param($Camera) $Camera.translation[1] -gt $moved.translation[1]}
  Assert-Camera $moved $released 0 $dy
  if((Model).state.document_file.revision -ne $drawing){throw "$Button wheel navigation changed the drawing"}
  $checks["held_${Button}_wheel"]=@{vertical=$dy;horizontal=$dx;shift='horizontal pan';zoom_anchor='preserved';motion='24,16';cleanup=$(if($Button -eq 'right'){'driver Cancel releases mouse'}else{'Up'});camera=$released}
 }finally{[CapyRowPointer]::Cancel()}
}
function Locked-Held-Wheel([string]$Button){
 $at=Center (Control 'drawing-canvas' -Arranged);$before=Camera
 if(!$before.zoom_locked){throw 'Held locked-wheel check needs zoom lock'}
 [CapyRowPointer]::Down('mouse',$at[0],$at[1],$Button)
 try{
  $after=Wheel-At $at -Control -Acknowledge {
   [CapyRowPointer]::Move(($at[0]+18),($at[1]+12));Changed-Camera $before "$Button pan after locked wheel" {param($Camera) !(Near $Camera.translation[0] $before.translation[0])}
  }
  Assert-Camera $before $after 18 12
  [CapyRowPointer]::Up()
  $checks["held_${Button}_zoom_lock"]='wheel refused; later same-contact pan acknowledged'
 }finally{[CapyRowPointer]::Cancel()}
}
function Paint-Ink([int[]]$Point,[string]$Stage){
 $sample=Join-Path $run ('paint-ink-'+$Stage+'.png');$null=Screen-Pixels $Point[0] $Point[1] $sample
 $bitmap=[Drawing.Bitmap]::FromFile($sample)
 try{
  for($y=44;$y -le 51;$y++){for($x=44;$x -le 51;$x++){
   $ink=0
   foreach($dy in 0..1){foreach($dx in 0..1){$color=$bitmap.GetPixel($x+$dx,$y+$dy);if($color.G -gt $color.R+10 -and $color.G -gt $color.B+10){$ink++}}}
   if($ink -eq 4){return $true}
  }}
  return $false
 }finally{$bitmap.Dispose()}
}
function Move-Paint($Stroke){
 $midpoint=@(($Stroke.point[0]+12),($Stroke.point[1]+6))
 $Stroke.point=@(($Stroke.point[0]+24),($Stroke.point[1]+12))
 [CapyRowPointer]::Move($Stroke.point[0],$Stroke.point[1])
 Wait-Until {Paint-Ink $midpoint $Stroke.stage} ($Stroke.stage+' same-contact Move did not present live ink')
}
function Paint-Wheel{
 Invoke-Id (Tool-Tile 'pen')
 Wait-Until {$view=Model;$view.brush_ready -and ($view.state.commands|Where-Object id -eq 'pen').selected} 'Pen did not become ready before the paint-contact wheel check'
 $fit=Camera;Fit-Canvas
 Wait-Until {Readout-State $false} 'Fit did not acknowledge closing the zoom menu'
 $null=Changed-Camera $fit 'Fit before the paint-contact wheel check'
 (Control 'drawing-canvas').SetFocus()
 Wait-Until {Focused} 'Canvas did not regain focus after Fit'
 if((Model).state.customization.drawer){
  [CapyRowPointer]::Key([uint32]$review.Id,0x1b)
  Wait-Until {$view=Model;$view -and $null -eq $view.state.customization.drawer} 'The tool drawer did not close'
 }
 $view=Model;$paintCamera=$view.state.camera;$revision=$view.state.document_file.revision;$modified=$view.state.document_file.modified
 $canvas=(Control 'drawing-canvas' -Arranged).Current.BoundingRectangle;$area=$paintCamera.work_area
 $stroke=@{point=@([int]($canvas.X+$area[0]+$area[2]/2),[int]($canvas.Y+$area[1]+$area[3]/2));stage='initial'}
 [CapyRowPointer]::Down('mouse',$stroke.point[0],$stroke.point[1])
 try{
  Move-Paint $stroke;Assert-Camera $paintCamera (Camera)
  foreach($wheelStage in @('vertical','horizontal','shift','control')){
   $stroke.stage=$wheelStage
   $ack={Move-Paint $stroke;Assert-Camera $paintCamera (Camera)}
   if($wheelStage -eq 'horizontal'){Wheel-At $stroke.point -Horizontal -Acknowledge $ack}
   elseif($wheelStage -eq 'shift'){Wheel-At $stroke.point -Shift -Acknowledge $ack}
   elseif($wheelStage -eq 'control'){Wheel-At $stroke.point -Control -Acknowledge $ack}
   else{Wheel-At $stroke.point -Acknowledge $ack}
  }
  [CapyRowPointer]::Up()
 }finally{[CapyRowPointer]::Cancel()}
 $committed=@{view=$null}
 Wait-Until {$committed.view=Model;$committed.view.state.document_file.revision -gt $revision} 'The mouse stroke after refused wheels did not commit'
 Assert-Camera $paintCamera $committed.view.state.camera
 Capture "paint-wheel-$Theme" -WithModel -Composed
 $revision=$committed.view.state.document_file.revision;Invoke 'Undo' -Name
 Wait-Until {$view=Model;$view.state.document_file.revision -gt $revision -and $view.state.document_file.modified -eq $modified} 'One Undo did not restore the drawing after the wheel-protected stroke'
 $checks.paint_contact_wheel='live ink before and after vertical, horizontal, Shift and Ctrl wheel; camera fixed; stroke commits and undoes once'
}
function Toggle-Lock([string]$Id,[string]$Field,[bool]$Locked){
 Open-Readout 'mouse';Choose 'mouse' $Id
 Wait-Until {(Camera).$Field -eq $Locked} "$Id did not set $Field to $Locked"
 Open-Readout 'mouse'
 Wait-Until {((Item $Id).Current.ItemStatus -ne '') -eq $Locked} "$Id check mark did not follow the camera"
 Close-Menu
}
function Navigation-Point {
 $view=Camera;$bounds=(Control 'drawing-canvas' -Arranged).Current.BoundingRectangle;$ratio=$bounds.Width/$view.viewport[0]
 @([int]($bounds.X+($view.work_area[0]+$view.work_area[2]/2)*$ratio),[int]($bounds.Y+($view.work_area[1]+$view.work_area[3]/2)*$ratio))
}
function Navigation-Focus {
 if((Model).state.customization.drawer){[CapyRowPointer]::Key([uint32]$review.Id,0x1b);Wait-Until {$null -eq (Model).state.customization.drawer} 'Navigation tool drawer did not close'}
 (Control 'drawing-canvas' -Arranged).SetFocus()
 Wait-Until {(Focused) -and [CapyRowPointer]::GetForegroundWindow() -eq $drawingWindow} 'Navigation canvas did not own keyboard focus'
 $point=Navigation-Point;[CapyRowPointer]::Hover($point[0],$point[1])
}
function Navigation-Command([string]$Id) {
 $command=@((Model).state.commands|Where-Object id -eq $Id)
 if($command.Count -ne 1 -or !$command[0].enabled){throw "Shared navigation command is unavailable: $Id"}
 Navigation-Focus
 [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),0x4b)
 Wait-Until {(Find 'command-search' -Visible) -and (Control 'command-search').Current.HasKeyboardFocus} 'Navigation command search did not open'
 (Control 'command-search').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($command[0].label)
 Wait-Until {$row=Find 'command-result-0' -Visible;$row -and $row.Current.Name.StartsWith($command[0].label)} "Command search did not resolve $Id"
 [CapyRowPointer]::Key([uint32]$review.Id,0x0d)
 Wait-Until {!(Find 'command-search' -Visible)} "Command search did not invoke $Id"
}
function Navigation-PaintState {
 $view=Model
 [ordered]@{file=$view.state.document_file|Select-Object revision,modified,location;layers=@($view.state.layers|Select-Object id,paint_revision,mask_revision,object);history=@($view.state.commands|Where-Object id -in @('undo','redo')|Select-Object id,enabled)}|ConvertTo-Json -Depth 12 -Compress
}
function Navigation-Selected([string]$Command){@((Model).state.commands|Where-Object {$_.id -eq $Command -and $_.selected}).Count -eq 1}
function Navigation-Changed($Before,[string]$Mode,[string]$Label) {
 Changed-Camera $Before $Label {
  param($Camera)
  if($Mode -in @('zoom','zoom_out')){return !(Near $Camera.zoom $Before.zoom)}
  if($Mode -eq 'rotate'){return !(Near $Camera.rotation $Before.rotation)}
  !(Near $Camera.translation[0] $Before.translation[0]) -or !(Near $Camera.translation[1] $Before.translation[1])
 }
}
function Navigation-Cursor([string]$Label,$Mode,[switch]$Stationary) {
 if($Device -ne 'mouse'){return}
 $start=[CapyRowPointer]::Cursor();$visible=$null -ne $Mode
 $point=if($Stationary){@($start.position.x,$start.position.y)}else{Navigation-Point}
 if(!$Stationary){[CapyRowPointer]::Hover($point[0],$point[1])}
 $expected=[IntPtr]::Zero
 if($Mode -eq 'pan'){$expected=[CapyRowPointer]::StockCursor(32646)}elseif($Mode -eq 'rotate'){$expected=[CapyRowPointer]::StockCursor(32515)}
 $observation=@{last=$null}
 try{
  Wait-Until {
   $nativeCursor=[CapyRowPointer]::Cursor();$published=(Model).navigation_cursor
   $observation.last=[ordered]@{utc=[DateTime]::UtcNow.ToString('o');qpc=[Diagnostics.Stopwatch]::GetTimestamp();mode=$published;expected_mode=$Mode;flags=$nativeCursor.flags;handle=$nativeCursor.cursor.ToInt64();point=@($nativeCursor.position.x,$nativeCursor.position.y);expected_stock_handle=$expected.ToInt64();drawing_hwnd=$drawingWindow.ToInt64();foreground_hwnd=$null}
   try{
    $observation.last.foreground_hwnd=[CapyRowPointer]::GetForegroundWindow().ToInt64()
   }catch{$observation.last.diagnostic_error=$_.Exception.Message}
   if($Stationary -and ($nativeCursor.position.x -ne $point[0] -or $nativeCursor.position.y -ne $point[1])){throw 'Stationary cursor check moved the pointer'}
   $published -eq $Mode -and (($nativeCursor.flags -band 1) -ne 0) -eq $visible -and ($expected -eq [IntPtr]::Zero -or $nativeCursor.cursor -eq $expected)
  } "$Label cursor did not publish"
 }catch{
  try{
   $evidence=[ordered]@{failure=$_.Exception.Message;last_observation=$observation.last;captured_at_failure_provider=$null;provider_error=$null;cursor_capture=$null;capture_error=$null;scope='Last predicate observation is from polling; provider and cursor image captured after timeout before held-key release. No timing claim.'}
   $path=Join-Path $run ('navigation-cursor-'+$Label+'-timeout.json')
   $evidence|ConvertTo-Json -Depth 8|Set-Content -LiteralPath $path
   try{
    $focused=[System.Windows.Automation.AutomationElement]::FocusedElement
    if($focused){$fields=$focused.Current;$evidence.captured_at_failure_provider=@{utc=[DateTime]::UtcNow.ToString('o');id=$fields.AutomationId;name=$fields.Name;class=$fields.ClassName;type=$fields.ControlType.ProgrammaticName;pid=$fields.ProcessId;hwnd=$fields.NativeWindowHandle;focused=$fields.HasKeyboardFocus;offscreen=$fields.IsOffscreen;bounds=$fields.BoundingRectangle.ToString();runtime_id=@($focused.GetRuntimeId())}}
   }catch{$evidence.provider_error=$_.Exception.Message}
   try{
    $cursor=[CapyRowPointer]::Cursor();$foreground=[CapyRowPointer]::GetForegroundWindow();$bounds=(Control 'drawing-canvas' -Arranged).Current.BoundingRectangle
    $evidence.cursor_capture=@{utc=[DateTime]::UtcNow.ToString('o');flags=$cursor.flags;handle=$cursor.cursor.ToInt64();point=@($cursor.position.x,$cursor.position.y);foreground_hwnd=$foreground.ToInt64();image=$null}
    if(($cursor.flags -band 1) -ne 0 -and $foreground -eq $drawingWindow -and $bounds.Contains([double]$cursor.position.x,[double]$cursor.position.y)){
     $image=Join-Path $run ('navigation-cursor-'+$Label+'-timeout.png')
     $null=Screen-Pixels $cursor.position.x $cursor.position.y $image -Cursor
     $evidence.cursor_capture.image=$image
    }
   }catch{$evidence.capture_error=$_.Exception.Message}
   $evidence|ConvertTo-Json -Depth 8|Set-Content -LiteralPath $path
  }catch{}
  throw
 }
 $raw=Join-Path $run ('navigation-cursor-'+$Label+'.png')
 $hash=Wait-StablePixels {Screen-Pixels $point[0] $point[1] -Cursor:$visible}
 if((Screen-Pixels $point[0] $point[1] $raw -Cursor:$visible) -ne $hash){throw 'Native navigation cursor changed during capture'}
 $cursor=[CapyRowPointer]::Cursor()
 if((($cursor.flags -band 1) -ne 0) -ne $visible -or ($expected -ne [IntPtr]::Zero -and $cursor.cursor -ne $expected)){throw "Native $Mode cursor changed before capture acknowledgment"}
 if($Stationary -and ($cursor.position.x -ne $point[0] -or $cursor.position.y -ne $point[1])){throw 'Stationary cursor capture moved the pointer'}
 @{mode=$Mode;hash=$hash;position=$cursor.position;handle=$cursor.cursor.ToInt64();expected_stock_handle=$expected.ToInt64();flags=$cursor.flags;composed_with=$(if($visible){'Actual GetCursorInfo handle, DrawIconEx and native hotspot over unmodified screen pixels'}else{'Unmodified screen pixels with hidden native cursor confirmed by GetCursorInfo'});raw_visual_review='required'}|ConvertTo-Json -Depth 5|Set-Content (Join-Path $run ('navigation-cursor-'+$Label+'.json'))
 $hash
}
function Navigation-Stationary {
 if($Device -ne 'mouse'){return}
 $before=Navigation-PaintState;$point=[CapyRowPointer]::Cursor().position
 if(!(Navigation-Selected 'pen')){throw 'Stationary cursor check requires Pen'}
 $null=Navigation-Cursor 'stationary-pen' $null -Stationary
 [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),0x4b)
 Wait-Until {(Find 'command-search' -Visible) -and (Control 'command-search').Current.HasKeyboardFocus} 'Stationary command search did not open'
 if($null -ne (Model).navigation_cursor){throw 'Command search changed the brush navigation mode'}
 [CapyRowPointer]::Key([uint32]$review.Id,0x1b)
 Wait-Until {!(Find 'command-search' -Visible) -and (Focused) -and [CapyRowPointer]::GetForegroundWindow() -eq $drawingWindow} 'Stationary command search did not restore canvas focus'
 $null=Navigation-Cursor 'stationary-popup' $null -Stationary
 $after=[CapyRowPointer]::Cursor()
 if($after.position.x -ne $point.x -or $after.position.y -ne $point.y){throw 'Stationary command search moved the pointer'}
 if(!(Navigation-Selected 'pen') -or (Navigation-PaintState) -ne $before){throw 'Stationary command search changed Pen, artwork or history'}
 $checks.navigation_stationary_cursor=@{point=@($point.x,$point.y);flags=$after.flags;handle=$after.cursor.ToInt64();mode=(Model).navigation_cursor;result='Native brush cursor stays hidden after held navigation and command-search dismissal without pointer movement'}
}
function Navigation-Held([uint16[]]$Keys,[string]$Mode,[string]$Label,[switch]$KeepTool) {
 if(!$KeepTool){
  $selected=if($Device -eq 'touch' -and $Mode -eq 'pan'){'hand'}else{'pen'}
  Navigation-Command $selected;Wait-Until {Navigation-Selected $selected} "$selected did not select before held navigation"
 }
 Navigation-Focus;$kept=Navigation-PaintState;$tool=(Model).state.layer_tools.tool|ConvertTo-Json -Compress
 $point=Navigation-Point;$start=@($point[0],$point[1]);if($Mode -eq 'rotate'){$start[0]+=80}
 try{
  foreach($key in $Keys){[CapyRowPointer]::Hold($key,$true)}
  Wait-Until {(Model).navigation_cursor -eq $Mode} "$Label held mode did not publish"
  $null=Navigation-Cursor $Label $Mode
  $before=Camera
  [CapyRowPointer]::Down($Device,$start[0],$start[1])
  [CapyRowPointer]::Move(($start[0]+48),($start[1]+48))
  $first=Navigation-Changed $before $Mode "$Label first contact motion"
  foreach($key in @($Keys)[($Keys.Count-1)..0]){[CapyRowPointer]::Hold($key,$false)}
  [CapyRowPointer]::Move(($start[0]+80),($start[1]+80))
  $last=Navigation-Changed $first $Mode "$Label motion after key release"
  [CapyRowPointer]::Up()
  Wait-Until {((Model).state.layer_tools.tool|ConvertTo-Json -Compress) -eq $tool} "$Label changed the selected tool"
  if((Navigation-PaintState) -ne $kept){throw "$Label changed document or artwork history"}
  $checks["navigation_$Label"]=@{device=$Device;camera_before=$before;camera_held=$first;camera_after_key_release=$last;selected_tool=$tool;selected_tool_preserved=$true;history='preserved'}
 }finally{foreach($key in $Keys){[CapyRowPointer]::Hold($key,$false)};[CapyRowPointer]::Cancel()}
}
function Navigation-Header {
 & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'Window'
 Invoke-Id 'customize_workspace_ui'
 Wait-Until {(Model).header.editing} 'Navigation header customization did not open'
 $before=(Model).header.model|ConvertTo-Json -Depth 30 -Compress;$firstId=(Model).header.model.next_id
 $from=Center (Control 'header-component-tools' -Arranged)
 $geometry=(Control 'title-bar').Current.ItemStatus|ConvertFrom-Json;$zone=$geometry.geometry.zones[1]
 $origin=[CapyRowPointer+Point]::new()
 if(![CapyRowPointer]::ClientToScreen($drawingWindow,[ref]$origin)){throw 'Owned header origin unavailable'}
 $scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
 try{
  [CapyRowPointer]::Down($Device,$from[0],$from[1])
  [CapyRowPointer]::Move([int]($origin.x+($zone.x+$zone.width/2)*$scale),[int]($origin.y+($zone.y+$zone.height/2)*$scale))
  Wait-Until {$gesture=(Control 'title-bar').Current.HelpText|ConvertFrom-Json;$gesture.phase -eq 'dragging' -and $null -ne $gesture.preview.target} 'Navigation header tool drop has no target'
  if(((Model).header.model|ConvertTo-Json -Depth 30 -Compress) -ne $before){throw 'Header preview changed the workspace'}
  [CapyRowPointer]::Up()
 }finally{[CapyRowPointer]::Cancel()}
 Wait-Until {(Model).picker -and (Find 'tool-picker-search')} 'Navigation tool picker did not open'
 (Control 'tool-picker-search').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('Hand')
 Wait-Until {(Model).picker.query -eq 'Hand'} 'Navigation tool search did not publish'
 (Control 'picker-choice-command-hand').GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
 Wait-Until {(Model).picker.can_confirm} 'Navigation tool was not selected'
 (Control 'Add Tools' -Name -Within (Control 'tool-picker') -Type ([System.Windows.Automation.ControlType]::Button)).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
 Wait-Until {!(Model).picker -and (Control 'drawing-canvas').Current.IsEnabled} 'Navigation header tool insertion did not complete'
 Invoke-Id 'header-edit-done';Wait-Until {!(Model).header.editing} 'Navigation header edit did not finish'
 $view=Model;$items=@($view.header.items|Where-Object id -eq $firstId)
 $entries=@(foreach($zone in $view.header.model.zones){$zone|Where-Object id -eq $firstId})
 if($items.Count -ne 1 -or $entries.Count -ne 1 -or $entries[0].item.kind -ne 'tool' -or $entries[0].item.control.command -ne 'hand' -or !$items[0].has_variants){throw 'Expected one added Hand navigation-group header tool'}
 $checks.navigation_header=@{id=$firstId;stored_control=$entries[0].item.control;has_variants=$items[0].has_variants}
 $items[0].id
}
function Navigation-Double([string]$Command,[long]$HeaderId=0) {
 Navigation-Command $Command;Wait-Until {Navigation-Selected $Command} "$Command did not select"
 $fit=$null
 if($Command -eq 'hand'){Navigation-Command 'fit_canvas';$fit=Camera}
 if($Command -eq 'rotate_view'){
  Navigation-Command 'rotate_right';Wait-Until {!(Near (Camera).rotation 0)} 'Rotate preparation did not change view'
 }else{
  $zoomBefore=Camera;Navigation-Command 'zoom_in';$null=Changed-Camera $zoomBefore 'Double-press zoom preparation' {param($Camera)$Camera.zoom -gt $zoomBefore.zoom};$before=Camera
  if($Command -eq 'zoom' -and (Near $before.zoom 1)){Navigation-Command 'zoom_in';Wait-Until {!(Near (Camera).zoom 1)} 'Zoom preparation did not leave Actual Pixels'}
 }
 Navigation-Focus;$before=Camera
 if($HeaderId){
  $spec=@((Model).header.items|Where-Object id -eq $HeaderId)[0]
  $element=Control ('header-item-'+$HeaderId) -Arranged
 }else{
  $id=Tool-Tile 'hand';$element=Control $id -Arranged
  foreach($panel in (Model).panels){foreach($tile in $panel.tiles){if("tile-$($panel.id)-$($tile.id)" -eq $id){$spec=$tile}}}
 }
 if($spec.resolved_control.command -ne $Command -or !$spec.double_click -or ($HeaderId -and !$spec.has_variants)){throw "Navigation button did not resolve its live $Command double action"}
 $identity=$element.GetRuntimeId() -join ':';$point=Center $element
 $elapsed=[CapyRowPointer]::DoubleClick($Device,$point[0],$point[1])
 Wait-Until {
  $camera=Camera
  $expected=if($Command -eq 'zoom'){Near $camera.zoom 1}elseif($Command -eq 'rotate_view'){Near $camera.rotation 0}else{Near $camera.zoom $fit.zoom}
  $expected -and (Navigation-Selected $Command) -and !(Model).state.customization.drawer
 } "$Device double press did not apply live $Command action"
 if($fit){Assert-Camera $fit (Camera)}
 if(($element.GetRuntimeId() -join ':') -ne $identity){throw 'Navigation double press replaced its native button'}
 $kind=if($HeaderId){'header'}else{'panel'}
 $checks[('navigation_double_'+$kind+'_'+$Command)]=@{device=$Device;elapsed_ms=$elapsed;resolved_control=$spec.resolved_control;camera=Camera;runtime_id=$identity}
 Capture "navigation-double-$kind-$Command-$Device-$Theme" -Composed -WithModel
}
function Navigation-Shortcut([string]$Id,[string]$Label,[uint16]$Key,[switch]$Replace,[switch]$Clear) {
 & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'Edit';Invoke-Id 'settings'
 Wait-Until {(Model).preferences} 'Shortcut Preferences did not open'
 Invoke-Id 'preference-page-shortcuts';Wait-Until {(Find 'shortcuts-search')} 'Shortcut search did not open'
 (Control 'shortcuts-search').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($Label)
 Invoke-Id ('shortcut-command.'+$Id);Wait-Until {(Model).preferences.shortcut_editor.id -eq ('command.'+$Id)} "$Label shortcut editor did not open"
 if($Replace -or $Clear){
  for($remaining=@((Model).preferences.shortcut_editor.bindings).Count;$remaining -gt 0;$remaining--){
   Invoke-Id 'remove-shortcut-0'
   Wait-Until {@((Model).preferences.shortcut_editor.bindings).Count -eq $remaining-1} "$Label shortcut removal did not commit"
  }
 }
 if(!$Clear){
  Invoke-Id 'add-shortcut';Wait-Until {(Model).preferences.capture -and (Find 'shortcut-recording')} 'Drawing shortcut capture did not open'
  [CapyRowPointer]::Key([uint32]$review.Id,$Key)
  Wait-Until {(Model).preferences.capture.chord -and (Control 'confirm-shortcut').Current.IsEnabled} "$Label shortcut was not captured"
  Invoke-Id 'confirm-shortcut';Wait-Until {!(Model).preferences.capture} "$Label shortcut did not commit"
 }
 Invoke-Id 'shortcut-editor-close';Invoke-Id 'CloseButton';Wait-Until {!(Model).preferences} 'Shortcut Preferences did not close'
}
function Navigation-DrawingsPopup([string]$Name,[uint16[]]$Modifiers,[uint16]$Key) {
 Navigation-Focus;$before=Navigation-PaintState;$tabs=(Model).windows_tabs|Select-Object selected,@{n='ids';e={@($_.tabs.id)}}|ConvertTo-Json -Compress
 [CapyRowPointer]::Chord([uint32]$review.Id,$Modifiers,$Key)
 Wait-Until {(Find 'drawing-list' -Visible) -and (Find ('drawing-row-'+(Model).windows_tabs.selected) -Visible)} "$Name did not open the native drawing list"
 Capture "navigation-drawings-$Name-$Device-$Theme" -Composed -WithModel
 [CapyRowPointer]::Key([uint32]$review.Id,0x1b);Wait-Until {!(Find 'drawing-list' -Visible)} 'Drawing selector did not close'
 Navigation-Focus
 if((Navigation-PaintState) -ne $before -or ((Model).windows_tabs|Select-Object selected,@{n='ids';e={@($_.tabs.id)}}|ConvertTo-Json -Compress) -ne $tabs){throw "$Name changed the drawing, history or tab identities"}
}
function Navigation-DrawingsClosed([string]$Name) {
 try{
  [CapyRowPointer]::Hold(0x20,$true)
  Wait-Until {(Model).navigation_cursor -eq 'pan' -and (Focused) -and !(Find 'drawing-list' -Visible)} "$Name blocked native held Pan or opened Drawings"
 }finally{[CapyRowPointer]::Hold(0x20,$false)}
 Wait-Until {!(Model).navigation_cursor -and (Focused) -and !(Find 'drawing-list' -Visible)} "$Name did not release held Pan with Drawings closed"
}
function Navigation-Drawings {
 Navigation-Command 'pen';Wait-Until {Navigation-Selected 'pen'} 'Pen did not select before drawing cycle'
 $source=(Model).windows_tabs.selected;$sourceDrawing=Navigation-PaintState;$count=@((Model).windows_tabs.tabs).Count
 & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'File'
 Invoke-Id 'new_document';Invoke 'Create' -Name -Within (Control 'document-dialog')
 Wait-Until {@((Model).windows_tabs.tabs).Count -eq $count+1 -and (Model).windows_tabs.selected -ne $source -and (Model).brush_ready -and !(Find 'canvas-status' -Visible)} 'Navigation second drawing did not become ready' 90
 $peer=(Model).windows_tabs.selected;$peerDrawing=Navigation-PaintState;$tabIds=@((Model).windows_tabs.tabs.id)|ConvertTo-Json -Compress
 foreach($pair in @(@{mods=@(0x11,0x10);key=0x09;target=$source},@{mods=@(0x11);key=0x22;target=$peer},@{mods=@(0x11);key=0x21;target=$source},@{mods=@(0x11);key=0x09;target=$peer})){
  Navigation-Focus;[CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]$pair.mods,[uint16]$pair.key)
  Wait-Until {(Model).windows_tabs.selected -eq $pair.target -and (Model).brush_ready -and !(Find 'canvas-status' -Visible)} 'Native drawing cycle shortcut did not activate the adjacent drawing'
  $expectedDrawing=if($pair.target -eq $source){$sourceDrawing}else{$peerDrawing}
  if((Navigation-PaintState) -ne $expectedDrawing){throw 'Drawing activation changed its retained document/history'}
 }
 foreach($pair in @(@{command='previous_drawing';target=$source},@{command='next_drawing';target=$peer})){
  & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'Window'
  $item=Control $pair.command -Type ([System.Windows.Automation.ControlType]::MenuItem) -Arranged
  $command=@((Model).state.commands|Where-Object id -eq $pair.command)[0]
  if(!$item.Current.IsEnabled -or !$command.enabled -or $item.Current.Name -ne $command.label){throw 'Window drawing command did not match its enabled shared caption'}
  if($pair.command -eq 'previous_drawing'){Capture "navigation-window-menu-$Device-$Theme" -Composed -WithModel}
  $item.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
  Wait-Until {
   $closed=try{$item.Current.IsOffscreen}catch [System.Windows.Automation.ElementNotAvailableException]{$true}
   $closed -and (Model).windows_tabs.selected -eq $pair.target -and (Model).brush_ready -and !(Find 'canvas-status' -Visible)
  } 'Window-menu drawing command did not close and activate the adjacent drawing'
  Navigation-Focus
  $expectedDrawing=if($pair.target -eq $source){$sourceDrawing}else{$peerDrawing}
  if((Navigation-PaintState) -ne $expectedDrawing -or (@((Model).windows_tabs.tabs.id)|ConvertTo-Json -Compress) -ne $tabIds){throw 'Window-menu drawing cycle changed the retained drawing, history or tab identities'}
 }
 Navigation-Focus
 try{
  [CapyRowPointer]::Hold(0x5a,$true);Wait-Until {(Model).navigation_cursor -eq 'zoom'} 'Parked Zoom key was not acknowledged'
  [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),0x09)
  Wait-Until {(Model).windows_tabs.selected -eq $source -and (Model).brush_ready -and !(Find 'canvas-status' -Visible)} 'Held-key drawing cycle did not activate its target'
  [CapyRowPointer]::Hold(0x5a,$false)
  Navigation-Command 'save_view'
  if(!(Navigation-Selected 'pen') -or (Model).navigation_cursor){throw 'Releasing a parked navigation key changed the newly active drawing'}
 }finally{[CapyRowPointer]::Hold(0x5a,$false)}
 Wait-Until {((Model).state.commands|Where-Object id -eq 'drawings').shortcut -eq 'Ctrl+Shift+A'} 'Shared Drawings default hint is missing'
 Navigation-DrawingsPopup 'default' ([uint16[]]@(0x11,0x10)) 0x41
 Navigation-Focus;[CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),0x4b)
 Wait-Until {(Find 'command-search' -Visible) -and (Control 'command-search').Current.HasKeyboardFocus} 'Drawings text ownership field did not focus'
 [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11,0x10),0x41);[CapyRowPointer]::Key([uint32]$review.Id,0x44)
 Wait-Until {(Control 'command-search').Current.HasKeyboardFocus -and (Control 'command-search').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value -eq 'd'} 'Default Drawings shortcut escaped focused native text'
 if((Find 'drawing-list' -Visible) -or (Navigation-PaintState) -ne $sourceDrawing -or (Model).windows_tabs.selected -ne $source){throw 'Default Drawings shortcut changed the focused drawing'}
 Capture "navigation-drawings-text-default-$Device-$Theme" -Composed -WithModel
 [CapyRowPointer]::Key([uint32]$review.Id,0x1b);Wait-Until {!(Find 'command-search' -Visible)} 'Drawings text ownership search did not close'
 Navigation-Shortcut 'NextDrawing' 'Next drawing' 0x75
 Navigation-Shortcut 'Drawings' 'Drawings' 0x76 -Replace
 Wait-Until {$command=(Model).state.commands|Where-Object id -eq 'drawings';$command.shortcut -eq 'F7' -and @($command.bindings).Count -eq 1 -and $command.bindings[0].key -eq 'f7'} 'Drawings did not adopt only the captured F7 binding'
 Navigation-Focus;[CapyRowPointer]::Key([uint32]$review.Id,0x75)
 Wait-Until {(Model).windows_tabs.selected -eq $peer -and (Model).brush_ready -and !(Find 'canvas-status' -Visible)} 'Custom F6 did not invoke shared Next Drawing'
 Navigation-Focus;[CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11,0x10),0x41)
 Navigation-DrawingsClosed 'Replaced Drawings chord'
 [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),0x4b)
 Wait-Until {(Find 'command-search' -Visible) -and (Control 'command-search').Current.HasKeyboardFocus} 'Replaced Drawings chord blocked native command search'
 foreach($key in @(0x48,0x5a,0x52,0x75,0x76)){[CapyRowPointer]::Key([uint32]$review.Id,[uint16]$key)}
 [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),0x22)
 Wait-Until {(Control 'command-search').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value -match 'hzr'} 'Focused native text field did not receive navigation letters'
 if((Find 'drawing-list' -Visible) -or !(Control 'command-search').Current.HasKeyboardFocus){throw 'Custom Drawings shortcut escaped native text ownership'}
 Capture "navigation-drawings-text-custom-$Device-$Theme" -Composed -WithModel
 [CapyRowPointer]::Key([uint32]$review.Id,0x1b);Wait-Until {!(Find 'command-search' -Visible)} 'Text ownership command search did not close'
 if((Model).windows_tabs.selected -ne $peer -or !(Navigation-Selected 'pen')){throw 'Focused native text input leaked navigation or drawing commands'}
 Navigation-DrawingsPopup 'custom-f7' ([uint16[]]@()) 0x76
 Navigation-Shortcut 'Drawings' 'Drawings' 0 -Clear
 Wait-Until {$command=(Model).state.commands|Where-Object id -eq 'drawings';!$command.shortcut -and @($command.bindings).Count -eq 0} 'Clearing Drawings retained a shortcut or hint'
 Navigation-Focus;[CapyRowPointer]::Key([uint32]$review.Id,0x76)
 [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11,0x10),0x41)
 Navigation-DrawingsClosed 'Cleared Drawings chords'
 [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),0x4b)
 Wait-Until {(Find 'command-search' -Visible) -and (Control 'command-search').Current.HasKeyboardFocus} 'Cleared Drawings shortcuts blocked native command search'
 [CapyRowPointer]::Key([uint32]$review.Id,0x44)
 Wait-Until {(Control 'command-search').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value -eq 'd'} 'Native text did not acknowledge the cleared shortcut sequence'
 if(Find 'drawing-list' -Visible){throw 'Cleared Drawings shortcut opened the drawing list'}
 Capture "navigation-drawings-cleared-$Device-$Theme" -Composed -WithModel
 [CapyRowPointer]::Key([uint32]$review.Id,0x1b);Wait-Until {!(Find 'command-search' -Visible)} 'Cleared shortcut search did not close'
 if((Navigation-PaintState) -ne $peerDrawing -or (Model).windows_tabs.selected -ne $peer -or (@((Model).windows_tabs.tabs.id)|ConvertTo-Json -Compress) -ne $tabIds){throw 'Drawings shortcut edits changed the document, history or tab identities'}
 $checks.navigation_drawings_shortcut='Default Ctrl+Shift+A, focused native text ownership, captured replacement F7 with old chord stopped, cleared binding, unchanged drawing/history/tab identities; Krita preset covered by shared tests only'
 $checks.navigation_drawing_cycle='Window-menu Next/Previous Drawing, Ctrl+Tab/Ctrl+Shift+Tab/Ctrl+PageUp/PageDown, captured F6 binding, parked key release and native text ownership'
}
function Navigation-Polygon {
 Navigation-Command 'reset_view';Navigation-Command 'polygon_select'
 Wait-Until {Navigation-Selected 'polygon_select'} 'Polygon selection did not select'
 Navigation-Focus;$point=Navigation-Point;$vertexDevice=if($Device -eq 'touch'){'mouse'}else{$Device}
 $paint=@((Model).state.layers|Select-Object id,paint_revision,mask_revision)|ConvertTo-Json -Compress
 foreach($vertex in @(@(($point[0]-70),($point[1]-50)),@(($point[0]+70),($point[1]-50)))){
  [CapyRowPointer]::Down($vertexDevice,$vertex[0],$vertex[1]);[CapyRowPointer]::Up()
 }
 Wait-Until {(Model).state.canvas_bar.context.kind -eq 'polygon'} 'Two polygon vertices did not expose the native completion controls'
 if($Device -eq 'touch'){Navigation-Held ([uint16[]]@(0x5a)) 'zoom' 'polygon-zoom' -KeepTool}
 else{Navigation-Held ([uint16[]]@(0x20)) 'pan' 'polygon-pan' -KeepTool}
 $beforeFit=Camera;[CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),0x30)
 $polygonFit=Changed-Camera $beforeFit 'Fit with unfinished polygon'
 Wait-Until {(Model).state.canvas_bar.context.kind -eq 'polygon' -and (Navigation-Selected 'polygon_select')} 'Navigation lost the unfinished polygon'
 $point=Navigation-Point;[CapyRowPointer]::Down($vertexDevice,($point[0]+70),($point[1]+50));[CapyRowPointer]::Up()
 Wait-Until {((Model).state.commands|Where-Object id -eq 'complete_selection').enabled -and (Control 'drawing-canvas').Current.HasKeyboardFocus -and [CapyRowPointer]::GetForegroundWindow() -eq $drawingWindow} 'The third polygon vertex and owned canvas focus did not become ready for Enter'
 [CapyRowPointer]::Key([uint32]$review.Id,0x0d)
 Wait-Until {(Model).state.layer_tools.has_selection -and (Model).state.canvas_bar.context.kind -eq 'selection'} 'Navigated polygon did not complete'
 Capture "navigation-polygon-$Device-$Theme" -Composed -WithModel
 $selectionCamera=Camera;$selectionState=Navigation-PaintState
 $selectionBefore=(Model).state.layer_tools|Select-Object tool,has_selection,quick_mask|ConvertTo-Json -Depth 8 -Compress
 $selectionViews=[ordered]@{before=$selectionCamera}
 foreach($command in @('zoom_selection','previous_view')){
  $cameraBefore=Camera;Navigation-Command $command
  $cameraAfter=Changed-Camera $cameraBefore $command {
   param($Camera)
   if($command -eq 'zoom_selection'){return $Camera.zoom -gt $selectionCamera.zoom}
   (Near $Camera.zoom $selectionCamera.zoom) -and (Near $Camera.rotation $selectionCamera.rotation)
  }
  if($command -eq 'previous_view'){
   Assert-Camera $selectionCamera $cameraAfter
   if(($cameraAfter.flipped|ConvertTo-Json -Compress) -ne ($selectionCamera.flipped|ConvertTo-Json -Compress)){throw 'Previous View did not restore canvas reflections'}
  }
  $selectionAfter=(Model).state.layer_tools|Select-Object tool,has_selection,quick_mask|ConvertTo-Json -Depth 8 -Compress
  if((Navigation-PaintState) -ne $selectionState -or $selectionAfter -ne $selectionBefore -or (Model).state.canvas_bar.context.kind -ne 'selection'){throw "$command changed selection, artwork or document history"}
  $selectionViews[$command]=$cameraAfter
  Capture "navigation-$command-$Device-$Theme" -Composed -WithModel
 }
 $checks.navigation_selection_views=@{camera=$selectionViews;document_history_before=$selectionState;selection_before=$selectionBefore;selection_artwork_history='preserved'}
 $revision=(Model).state.document_file.revision
 [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),0x5a)
 Wait-Until {!(Model).state.layer_tools.has_selection -and (Model).state.document_file.revision -gt $revision} 'One Undo did not remove the navigated polygon selection'
 if((@((Model).state.layers|Select-Object id,paint_revision,mask_revision)|ConvertTo-Json -Compress) -ne $paint){throw 'Polygon navigation or selection history changed paint'}
 $checks.navigation_polygon=@{vertices=$vertexDevice;navigation=$Device;camera_after_fit=$polygonFit;mode=if($Device -eq 'touch'){'held-Z zoom'}else{'held-Space pan'};result='Unfinished polygon survives navigation and Fit; native completion creates selection; one Undo removes it; paint preserved';raw_visual_review='triangle required'}
}
function Navigation-Journey {
 $kept=Navigation-PaintState
 foreach($pair in @(@{key=0x48;command='hand'},@{key=0x5a;command='zoom'},@{key=0x52;command='rotate_view'})){
  Navigation-Focus;[CapyRowPointer]::Key([uint32]$review.Id,[uint16]$pair.key)
  Wait-Until {Navigation-Selected $pair.command} 'H/Z/R tap did not select its navigation tool'
 }
 foreach($case in @(@{keys=@(0x48);mode='pan';name='held-h'},@{keys=@(0x5a);mode='zoom';name='held-z'},@{keys=@(0x52);mode='rotate';name='held-r'},@{keys=@(0x11,0x20);mode='zoom';name='control-space'},@{keys=@(0x10,0x20);mode='rotate';name='shift-space'})){
  Navigation-Held ([uint16[]]$case.keys) $case.mode $case.name
 }
 Navigation-Stationary
 Navigation-Command 'pen';Navigation-Focus;$point=Navigation-Point;$before=Camera
 try{
  foreach($key in @(0x11,0x12,0x20)){[CapyRowPointer]::Hold([uint16]$key,$true)}
  Wait-Until {(Model).navigation_cursor -eq 'zoom_out'} 'Ctrl+Alt+Space did not select temporary zoom out'
  [CapyRowPointer]::Down($Device,$point[0],$point[1]);[CapyRowPointer]::Up()
  $null=Changed-Camera $before 'Ctrl+Alt+Space click' {param($Camera)$Camera.zoom -lt $before.zoom}
 }finally{foreach($key in @(0x20,0x12,0x11)){[CapyRowPointer]::Hold([uint16]$key,$false)};[CapyRowPointer]::Cancel()}
 Navigation-Command 'zoom';Wait-Until {Navigation-Selected 'zoom'} 'Zoom did not select'
 Navigation-Focus;$point=Navigation-Point
 foreach($click in 1..2){$before=Camera;[CapyRowPointer]::Down($Device,$point[0],$point[1]);[CapyRowPointer]::Up();$null=Changed-Camera $before 'Selected Zoom click' {param($Camera)$Camera.zoom -gt $before.zoom}}
 $plus=Navigation-Cursor 'zoom-in' 'zoom'
 try{
  [CapyRowPointer]::Hold(0x12,$true);Wait-Until {(Model).navigation_cursor -eq 'zoom_out'} 'Alt hover did not reverse Zoom cursor'
  $minus=Navigation-Cursor 'zoom-out' 'zoom_out'
  if($Device -eq 'mouse' -and $plus -eq $minus){throw 'Native zoom-in and zoom-out cursor glyphs are identical'}
 }finally{[CapyRowPointer]::Hold(0x12,$false)}
 Wait-Until {(Model).navigation_cursor -eq 'zoom'} 'Alt release did not restore Zoom cursor'
 $before=Camera
 try{[CapyRowPointer]::Down($Device,$point[0],$point[1]);[CapyRowPointer]::Move(($point[0]+48),($point[1]+24));$null=Navigation-Changed $before 'zoom' 'Selected Zoom drag';[CapyRowPointer]::Up()}finally{[CapyRowPointer]::Cancel()}
 Navigation-Command 'reset_view';Navigation-Focus;$point=Navigation-Point;$before=Camera
 $edges=@(@($point[0],($point[1]-50)),@($point[0],($point[1]+50)))
 $edgeBefore=@(foreach($edge in $edges){Wait-StablePixels {Screen-Pixels $edge[0] $edge[1]}})
 foreach($index in 0..1){$null=Screen-Pixels $edges[$index][0] $edges[$index][1] (Join-Path $run "navigation-rectangle-edge-$index-before.png")}
 try{
  [CapyRowPointer]::Hold(0x10,$true);[CapyRowPointer]::Down($Device,($point[0]-70),($point[1]-50))
  [CapyRowPointer]::Move(($point[0]+70),($point[1]+50))
  Wait-Until {
   Assert-Camera $before (Camera)
   (Screen-Pixels $edges[0][0] $edges[0][1]) -ne $edgeBefore[0] -and (Screen-Pixels $edges[1][0] $edges[1][1]) -ne $edgeBefore[1]
  } 'Shift Zoom rectangle did not present both edges before release'
  $edgeHeld=@(foreach($edge in $edges){Wait-StablePixels {Screen-Pixels $edge[0] $edge[1]}})
  foreach($index in 0..1){
   if($edgeHeld[$index] -eq $edgeBefore[$index] -or (Screen-Pixels $edges[$index][0] $edges[$index][1] (Join-Path $run "navigation-rectangle-edge-$index-held.png")) -ne $edgeHeld[$index]){throw 'Shift Zoom rectangle edge did not remain presented'}
  }
  $heldCamera=Camera;Assert-Camera $before $heldCamera
  if(($heldCamera|ConvertTo-Json -Depth 8 -Compress) -ne ($before|ConvertTo-Json -Depth 8 -Compress)){throw 'Shift Zoom rectangle changed camera before release'}
  $checks.navigation_rectangle=@{camera_before=$before;camera_before_up=$heldCamera;edge_points=$edges;edge_before=$edgeBefore;edge_held=$edgeHeld;acknowledgment='Two composed edge patches away from contact/cursor';raw_visual_review='required'}
  Capture "navigation-rectangle-$Device-$Theme" -Composed -WithModel
  [CapyRowPointer]::Up();$null=Changed-Camera $before 'Shift Zoom rectangle' {param($Camera)$Camera.zoom -gt $before.zoom}
 }finally{[CapyRowPointer]::Hold(0x10,$false);[CapyRowPointer]::Cancel()}
 if($Device -ne 'mouse'){
  Navigation-Focus;$point=Navigation-Point;$before=Camera
  [CapyRowPointer]::Down($Device,$point[0],$point[1]);[CapyRowPointer]::Cancel()
  Navigation-Command 'save_view';Assert-Camera $before (Camera)
  $checks.navigation_cancel='Actual pen capture loss or canceled touch; no Zoom click on cancellation'
 }
 Navigation-Command 'hand';Wait-Until {Navigation-Selected 'hand'} 'Hand did not select'
 Navigation-Focus
 foreach($key in @(0x27,0x22)){$before=Camera;[CapyRowPointer]::Key([uint32]$review.Id,[uint16]$key);$null=Navigation-Changed $before 'pan' 'Hand keyboard pan'}
 foreach($alias in @(@{mods=@(0x11);key=0xba},@{mods=@(0x11);key=0xbb},@{mods=@(0x11,0x10);key=0xbb},@{mods=@(0x11);key=0x6b})){
  Navigation-Command 'reset_view';Navigation-Focus
  $before=Camera;[CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]$alias.mods,[uint16]$alias.key)
  $null=Changed-Camera $before 'Native zoom-in alias' {param($Camera)$Camera.zoom -gt $before.zoom}
 }
 $before=Camera;[CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),0x6d)
 $null=Changed-Camera $before 'Numpad minus zoom out' {param($Camera)$Camera.zoom -lt $before.zoom}
 Navigation-Command 'save_view';$saved=Camera
 Navigation-Command 'reset_view';Navigation-Command 'restore_view'
 Wait-Until {$now=Camera;(Near $now.zoom $saved.zoom) -and (Near $now.rotation $saved.rotation)} 'Restore View did not restore the saved camera'
 Assert-Camera $saved (Camera)
 foreach($command in @('fit_canvas','fit_width','fill_view','reset_view')){Navigation-Command $command}
 $headerId=Navigation-Header
 foreach($command in @('zoom','rotate_view','hand')){Navigation-Double $command;Navigation-Double $command $headerId}
 if((Navigation-PaintState) -ne $kept){throw 'Navigation changed artwork or Undo history'}
 Capture "navigation-final-$Device-$Theme" -Composed -WithModel
 Navigation-Polygon
 Navigation-Drawings
 Paint-Wheel
 $checks.navigation_scope=@{device=$Device;physical_hardware='unverified';touch_scope='Selected Hand for one-finger held-H pan; Pen for held Zoom/Rotate; mouse polygon vertices with touch held-Z navigation; no finger drawing setting or claim';alt_space='OS-owned Windows alternative Ctrl+Alt+Space exercised';mouse_cancel='mouse driver Cancel releases rather than issuing cancellation; actual cancellation covered by pen/touch variants';cursor_visual_review='mouse OS glyph captures required';default_zoom_journey='separate unchanged mode'}
}

try {
 Enter-CapyEnvironment
 $env:CAPY_STORAGE_DIR=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1'
 [IO.File]::WriteAllText((Settings-File),(@{language=@{Explicit='en'};theme=$Theme}|ConvertTo-Json -Depth 4))
 $review=Start-Process -FilePath $Executable -WorkingDirectory $run -WindowStyle Hidden -PassThru -RedirectStandardError (Join-Path $run 'stderr.log')
 $null=$review.Handle
 Write-Output "Owned zoom readout review $($review.Id): $run"
 $native=@{window=$null}
 Wait-Until {$native.window=Owned-DrawingWindow $review;$null -ne $native.window} 'The zoom drawing window did not open' 45
 $drawingWindow=$native.window.Handle;$root=$native.window.Root
 Wait-Until {(Model).brush_ready -and (Model).windows_workspace.ready -and !(Model).windows_workspace.busy} 'Zoom review did not start' 45
 $root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Maximized)
 [CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
 [CapyRowPointer]::SetForegroundWindow($drawingWindow)|Out-Null
 [CapyRowPointer]::Initialize([uint32]$review.Id)
 (Control 'drawing-canvas').SetFocus()
 Wait-Until {Focused} 'The canvas did not take focus'

 if($Navigation){Navigation-Journey}else{
 Wait-Until {Readout-State $false} 'The readout did not acknowledge its initial closed state'
 Check-ReadoutCamera
 Open-Readout 'mouse'
 Check-ZoomCaptions
 Capture "zoom-menu-$Theme"
 $order=@('zoom-field','zoom-zoom_in','zoom-200','zoom-lock-zoom','rotation-field','zoom-reset_rotation','zoom-lock-rotation','zoom-button-zoom_out')
 $tops=@($order|ForEach-Object {(Item $_).Current.BoundingRectangle.Top})
 for($i=1;$i -lt $tops.Count;$i++){if($tops[$i] -le $tops[$i-1]){throw "Zoom menu shows $($order[$i]) above $($order[$i-1])"}}
 $buttons=@('zoom_out','zoom_in','rotate_left','rotate_right','flip_horizontal','flip_vertical'|ForEach-Object {Item ('zoom-button-'+$_)})
 if(@($buttons|ForEach-Object {[int]$_.Current.BoundingRectangle.Top}|Select-Object -Unique).Count -ne 1){throw 'Navigation buttons are not one row'}
 $hint=((Model).state.commands|Where-Object id -eq 'zoom_in').shortcut
 $texts=(Item 'zoom-zoom_in').FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::Text))
 if($hint -and !@($texts|Where-Object {$_.Current.Name -eq $hint}).Count){throw 'Zoom In does not show its shortcut'}
 $checks.order_buttons_and_hints='passed'
 Close-Menu
 $at=Center (Control 'canvas-view-info');[CapyRowPointer]::RightClick($at[0],$at[1])
 Wait-Until {Readout-State $true} 'Right-click did not open the zoom menu'
 if(!(Focused)){throw 'Right-click took focus from the canvas'}
 $checks.right_click_opens='passed'
 Close-Menu
 Open-Readout 'mouse'
 Choose 'mouse' 'zoom-200'
 Wait-Until {Near (Zoom) 2} '200% did not set the zoom to 2'
 if(!(Focused)){throw 'Choosing 200% took focus from the canvas'}
 $checks.mouse_preset='passed'

 Open-Readout 'touch'
 Choose 'touch' 'zoom-actual_pixels'
 Wait-Until {Near (Zoom) 1} 'Actual Pixels did not set the zoom to 1'
 $checks.touch_actual_pixels='passed'

 Open-Readout 'pen'
 Choose 'pen' 'zoom-50'
 Wait-Until {Near (Zoom) .5} '50% did not set the zoom to 0.5'
 $checks.pen_preset='passed'

 Open-Readout 'mouse'
 $field=Control 'zoom-field'
 $field.SetFocus()
 $field.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('250')
 [CapyRowPointer]::Key([uint32]$review.Id,0x0D)
 Wait-Until {Near (Zoom) 2.5} 'Typing 250 did not set the zoom to 2.5'
 $checks.typed_zoom='passed'
 [CapyRowPointer]::Key([uint32]$review.Id,0x1B)
 Wait-Until {Readout-State $false} 'Escape did not close the zoom menu'
 (Control 'drawing-canvas').SetFocus()

 Open-Readout 'mouse'
 Tap 'mouse' (Control 'canvas-view-info')
 Wait-Until {Readout-State $false} 'A second press did not close the zoom menu'
 $checks.second_press_closes='passed'
 Open-Readout 'touch'
 [CapyRowPointer]::Key([uint32]$review.Id,0x1B)
 Wait-Until {(Readout-State $false) -and (Focused)} 'Escape did not close the zoom menu with focus on the canvas'
 $checks.escape_keeps_canvas_focus='passed'

 Open-Readout 'mouse'
 $field=Item 'rotation-field';$field.SetFocus()
 $field.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('45')
 [CapyRowPointer]::Key([uint32]$review.Id,0x0D)
 Wait-Until {Near (Camera).rotation ([Math]::PI/4)} 'Typing 45 did not rotate the view to 45 degrees'
 Wait-Until {(Find 'canvas-camera').Current.Name -match ' 45°$'} 'The readout did not show 45 degrees'
 $checks.typed_rotation='passed'
 $slider=(Item 'rotation-field-slider').Current.BoundingRectangle;$before=(Camera).rotation
 [CapyRowPointer]::Down('mouse',[int]($slider.X+$slider.Width*.5),[int]($slider.Y+$slider.Height/2))
 [CapyRowPointer]::Move([int]($slider.X+$slider.Width*.7),[int]($slider.Y+$slider.Height/2));[CapyRowPointer]::Up()
 Wait-Until {!(Near (Camera).rotation $before)} 'Dragging the rotation slider did not rotate the view'
 if(!(Zoom-Item 'zoom-fit_canvas')){throw 'The rotation slider closed the zoom menu'}
 $checks.rotation_slider='passed'
 $before=(Camera).rotation
 Tap 'touch' (Item 'zoom-button-rotate_left')
 Wait-Until {!(Near (Camera).rotation $before)} 'Rotate Left did not rotate the view'
 Tap 'pen' (Item 'zoom-button-flip_horizontal')
 Wait-Until {(Camera).flipped[0]} 'Flip did not mirror the view'
 Wait-Until {(Item 'zoom-button-flip_horizontal').Current.ItemStatus -ne ''} 'Flip button did not show as selected'
 if(!(Zoom-Item 'zoom-fit_canvas')){throw 'A navigation button closed the zoom menu'}
 Tap 'mouse' (Item 'zoom-button-flip_horizontal')
 Wait-Until {!(Camera).flipped[0]} 'Flip did not restore the view'
 $checks.navigation_buttons_keep_menu='passed'
 Choose 'touch' 'zoom-reset_rotation'
 Wait-Until {Near (Camera).rotation 0} 'Reset rotation did not restore the view'
 $checks.reset_rotation='passed'

 $before=Zoom;Control-Wheel -Acknowledge {Wait-Until {!(Near (Zoom) $before)} 'Ctrl+wheel did not zoom the unlocked view'}
 Toggle-Lock 'zoom-lock-zoom' 'zoom_locked' $true
 $before=Camera;Control-Wheel
 $at=Center (Control 'drawing-canvas' -Arranged);Wheel-At $at -Horizontal
 $after=Changed-Camera $before 'Horizontal pan after locked Ctrl+wheel' {param($Camera) $Camera.translation[0] -lt $before.translation[0]}
 if(!(Near $after.zoom $before.zoom)){throw 'Ctrl+wheel zoomed a locked view'}
 Open-Readout 'pen';Choose 'pen' 'zoom-200'
 Wait-Until {Near (Zoom) 2} 'A zoom choice did not apply while zoom was locked'
 Toggle-Lock 'zoom-lock-zoom' 'zoom_locked' $false
 Toggle-Lock 'zoom-lock-rotation' 'rotation_locked' $true
 Open-Readout 'mouse';$before=(Camera).rotation
 Tap 'mouse' (Item 'zoom-button-rotate_right')
 Wait-Until {!(Near (Camera).rotation $before)} 'Rotate Right did not apply while rotation was locked'
 Close-Menu
 Toggle-Lock 'zoom-lock-rotation' 'rotation_locked' $false
 $checks.locks='passed'
 foreach($button in @('middle','right')){Held-Wheel $button}
 Toggle-Lock 'zoom-lock-zoom' 'zoom_locked' $true
 foreach($button in @('middle','right')){Locked-Held-Wheel $button}
 Toggle-Lock 'zoom-lock-zoom' 'zoom_locked' $false
 Paint-Wheel
 Capture "wheel-contacts-$Theme" -WithModel
 }

 [CapyRowPointer]::Dispose()
 & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow -Action Close -DiscardUnsaved -StateDirectory $run
 if((Get-Item (Join-Path $run 'stderr.log')).Length){throw 'Native stderr requires review'}
 $checks.evidence=$run
 [pscustomobject]$checks|ConvertTo-Json -Depth 10|Tee-Object -FilePath (Join-Path $run 'results.json')
} catch {
 if($review -and !$review.HasExited){try{Capture 'failure' -WithModel}catch{}}
 [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
} finally {
 [CapyRowPointer]::Dispose()
 Exit-CapyEnvironment
}
