param([Parameter(Mandatory)][string]$Executable,[ValidateSet('dark','light')][string]$Theme='dark',[ValidateSet('full','preview-dark','preview-light','release-dark','release-light','drawer-dark','drawer-light','chrome-dark','chrome-light')][string]$Journey='full')
if($Journey -ne 'full'){$Theme=$Journey -replace '^(preview|release|drawer|chrome)-',''}
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyCacheModel=$true;$CapyEach={if($env:CAPY_PRIVATE_DESKTOP){[CapyRowPointer]::VerifyPrivateDesktop($env:CAPY_PRIVATE_DESKTOP)};[CapyCanvasTouch]::Verify()}
. (Join-Path $PSScriptRoot 'ComposedFrames.ps1')
Add-Type -Path (Join-Path $PSScriptRoot 'CanvasTouchDriver.cs')
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
$ControlType=[System.Windows.Automation.ControlType]
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
function Drawer-Group([uint32]$Group){@((Model).state.customization.column_drawers|Where-Object {$_.tabs.group -eq $Group})[0]}
function Drawer-Widths([string[]]$Panels){
 $result=[ordered]@{}
 foreach($panel in $Panels){$result[$panel]=[Math]::Round((Control ('drawer-tab-'+$panel) -Arranged).Current.BoundingRectangle.Width/$scale,2)}
 $result
}
function Tab-Order([string]$Panel){
 $layout=(Model).state.workspace.layout
 $visit={param($node) if(!$node){return};if($node.kind -eq 'tabs'){if($node.panels -contains $Panel){$node.panels}}else{& $visit $node.first;& $visit $node.second}}
 foreach($node in @($layout.bands.root)+@($layout.floating.root)){& $visit $node}
}
function Check-Foot([string]$Name,[string]$Before,$From,[string]$After,$To){
 $a=[Drawing.Bitmap]::new($Before);$b=[Drawing.Bitmap]::new($After)
 try{
  if($To.Width-16*$scale -lt 8){throw "$Name has no measurable active foot"}
  $wrong=[TabFrames]::FootDifference($a,$From,$b,$To,$scale)
  if($wrong -gt 3){$script:frameFailures+=@("$Name foot does not cover its active tab ($wrong pixels)")}
  $wrong
 }finally{$a.Dispose();$b.Dispose()}
}
function Chrome-Area([double]$Top,[double]$Height){
 $bounds=$root.Current.BoundingRectangle
 [Drawing.Rectangle]::new([int]($bounds.Right-400*$scale),[int]$Top,[int](400*$scale),[int]($Height*$scale))
}
function Chrome-Ink($Image,$Bounds){
 $count=0
 for($y=[Math]::Max(0,$Bounds.Top);$y -lt [Math]::Min($Image.Height,$Bounds.Bottom);$y++){
  for($x=[Math]::Max(0,$Bounds.Left);$x -lt [Math]::Min($Image.Width,$Bounds.Right);$x++){
   $pixel=$Image.GetPixel($x,$y)
   if(($Theme -eq 'dark' -and [Math]::Min($pixel.R,[Math]::Min($pixel.G,$pixel.B)) -gt 150) -or ($Theme -eq 'light' -and [Math]::Max($pixel.R,[Math]::Max($pixel.G,$pixel.B)) -lt 110)){$count++}
  }
 };$count
}
function Chrome-Sample([string]$Name,$Area,[scriptblock]$Action,[scriptblock]$Check){
 $folder=Join-Path $run $Name;[IO.Directory]::CreateDirectory($folder)|Out-Null
 Capture ($Name+'-before') -WithModel -Composed
 $sample=Sample-Frames $Area $Action
 try{
  Capture ($Name+'-held') -WithModel -Composed
  $null=[TabFrames]::DwmFlush();$final=[TabFrames]::Grab($Area.Left,$Area.Top,$Area.Width,$Area.Height)
  try{
   $final.Save((Join-Path $folder 'held.png'))
   for($i=0;$i -lt $sample.frames.Count;$i++){$sample.frames[$i].Save((Join-Path $folder ('frame-{0:D3}.png' -f $i)))}
   @{area=@($Area.Left,$Area.Top,$Area.Width,$Area.Height);action_frame=3;motion_frames=$(if($Name -in @('chrome-edge-open','chrome-filters-pickup')){@(4,75)}else{@()});moving_window=$(if($Name -in @('chrome-edge-open','chrome-filters-pickup')){@{start=$sample.times[4].action;end=$sample.times[75].begin}});frequency=[Diagnostics.Stopwatch]::Frequency;times=$sample.times}|ConvertTo-Json -Depth 8|Set-Content (Join-Path $folder 'frames.json')
   & $Check $sample.frames $final $Area $Name
  }finally{$final.Dispose()}
 }finally{foreach($image in $sample.frames){$image.Dispose()}}
}
function Chrome-Overview($Frames,$Final,$Area,[string]$Name,$overview=$null,$header=$null){
 if(!$overview){$overview=(Control 'navigator-overview' -Arranged).Current.BoundingRectangle}
 if(!$header){$tab=Find 'panel-tab-navigator' -Visible;if(!$tab){$tab=Control 'drawer-tab-navigator' -Arranged};$header=$tab.Current.BoundingRectangle}
 $icon=[Drawing.Rectangle]::new([int]($header.Left-$Area.Left+8*$scale),[int]($header.Top-$Area.Top+($header.Height-16*$scale)/2),[int](16*$scale),[int](16*$scale))
 $ink=Chrome-Ink $Final $icon;if($ink -lt 5){throw "$Name has no composed Navigator reference glyph"}
 $x=[int]($overview.Left-$Area.Left+$overview.Width/2);$y=[int]($overview.Top-$Area.Top+$overview.Height/2)
 $fill=$Final.GetPixel([int]($header.Right-$Area.Left-5*$scale),[int]($header.Top-$Area.Top+8*$scale))
 $errors=@();for($i=3;$i -lt $Frames.Count;$i++){
  if((Chrome-Ink $Frames[$i] $icon) -lt $ink*.7){continue}
  $pixel=$Frames[$i].GetPixel($x,$y)
  if([Math]::Max($pixel.R,[Math]::Max($pixel.G,$pixel.B)) -lt [Math]::Min($fill.R,[Math]::Min($fill.G,$fill.B))-4){$errors+=@($i)}
 }
 @{overview=$overview;header=$header;reference_ink=$ink;fallback=@($fill.R,$fill.G,$fill.B);invalid=$errors}|ConvertTo-Json -Depth 8|Set-Content (Join-Path $run ($Name+'-pixels.json'))
 if($errors.Count){$script:frameFailures+=@("$Name exposed the canvas through Navigator in frames $($errors -join ',')")}
}
function Chrome-Review {
 Use-PaintWorkspace
 Set-CapyTransparency high
 $column=@((Model).layout.collapsed|Where-Object {$_.groups.icons.panel -contains 'navigator'})[0]
 if(!$column){
  $bounds=(Control 'panel-tab-navigator' -Arranged).Current.BoundingRectangle
  [CapyRowPointer]::RightClick([int]($bounds.Left+$bounds.Width/2),[int]($bounds.Top+$bounds.Height/2));Invoke 'Collapse column' -Name
  Wait-Until {@((Model).layout.collapsed|Where-Object {$_.groups.icons.panel -contains 'navigator'}).Count -gt 0} 'Chrome setup did not collapse the Paint right column'
  $column=@((Model).layout.collapsed|Where-Object {$_.groups.icons.panel -contains 'navigator'})[0]
 }
 if($column.open){Invoke 'column-icon-navigator';Wait-Until {!(@((Model).layout.collapsed|Where-Object id -eq $column.id)[0].open)} 'Navigator member did not close'}
 $origin=[CapyRowPointer+Point]::new();[CapyRowPointer]::ClientToScreen($drawingWindow,[ref]$origin)|Out-Null
 $divider=@((Model).layout.dividers|Where-Object {$_.axis -eq 'horizontal' -and !$_.fixed}|Sort-Object {$_.bounds.x} -Descending)[0]
 $x=[int]($origin.x+($divider.bounds.x+$divider.bounds.width/2)*$scale);$y=[int]($origin.y+120*$scale)
 $area=Chrome-Area ($origin.y+48*$scale) 350
 Chrome-Sample 'chrome-edge-open' $area {
  param($i)
  if($i -eq 3){[CapyRowPointer]::Down('mouse',$x,$y)}
  if($i -ge 4 -and $i -le 75){[CapyRowPointer]::Move([int]($x-110*$scale*($i-3)/72),$y)}
 } ${function:Chrome-Overview}
 $closingOverview=(Control 'navigator-overview' -Arranged).Current.BoundingRectangle;$closingHeader=(Control 'panel-tab-navigator' -Arranged).Current.BoundingRectangle
 $excluded=[Drawing.Rectangle]::FromLTRB([int][Math]::Floor($closingOverview.Left-$area.Left),[int][Math]::Floor($closingOverview.Top-$area.Top),[int][Math]::Ceiling($closingOverview.Right-$area.Left),[int][Math]::Ceiling($closingOverview.Bottom-$area.Top))
 Chrome-Sample 'chrome-edge-close' $area {param($i)if($i -eq 3){[CapyRowPointer]::Key(0x1b);[CapyRowPointer]::Up()}} {
  param($frames,$final,$area,$name)
  Chrome-Overview $frames $frames[2] $area ($name+'-hole') $closingOverview $closingHeader
  $envelope=[TabFrames+Envelope]::new($frames[2],$final);$bad=@();$fullBad=@();$scores=@()
  for($i=3;$i -lt $frames.Count;$i++){
   $full=$envelope.Difference($frames[$i]);$native=$envelope.Difference($frames[$i],$excluded);$remaining=[TabFrames]::Remaining($frames[$i],$final,$excluded)
   $scores+=@{frame=$i;full_invalid_pixels=$full;native_invalid_pixels=$native;overview_invalid_pixels=($full-$native);native_remaining_pixels=$remaining[0];overview_remaining_pixels=$remaining[1]}
   if($full -gt 8){$fullBad+=@($i)}
   if($native -gt 8){$bad+=@($i)}
  }
  @{overview=$closingOverview;excluded=$excluded;area=$area;native_invalid=$bad;full_invalid=$fullBad;scores=$scores}|ConvertTo-Json -Depth 8|Set-Content (Join-Path $run ($name+'-pixels.json'))
  if($bad.Count){$script:frameFailures+=@("$name exposed a third native chrome state in frames $($bad -join ',')")}
 }
 Wait-Until {@((Model).layout.collapsed|Where-Object id -eq $column.id).Count -gt 0} 'Edge cancellation did not restore its column'
 $icon=Control 'column-icon-layers' -Arranged;$b=$icon.Current.BoundingRectangle
 [CapyRowPointer]::RightClick([int]($b.Left+$b.Width/2),[int]($b.Top+$b.Height/2));Invoke 'Expand column' -Name
 Wait-Until {$null -ne (Find 'panel-tab-adjustments' -Visible)} 'Right column did not expand'
 Invoke 'panel-tab-layers'
 $layers=(Control 'panel-tab-layers' -Arranged).Current.BoundingRectangle;$filters=(Control 'panel-tab-adjustments' -Arranged).Current.BoundingRectangle
 $area=Chrome-Area ($layers.Top-8*$scale) 160
 $x=[int]($filters.Left+8*$scale);$y=[int]($filters.Top+$filters.Height/2)
 Chrome-Sample 'chrome-filters-pickup' $area {
  param($i)
  if($i -eq 3){[CapyRowPointer]::Down('mouse',$x,$y)}
  if($i -ge 4 -and $i -le 75){[CapyRowPointer]::Move([int]($x+40*$scale*($i-3)/72),$y)}
 } {
  param($frames,$final,$area,$name)
  $bad=@();foreach($tab in @($layers,$filters)){
   $glyph=[Drawing.Rectangle]::new([int]($tab.Left-$area.Left+8*$scale),[int]($tab.Top-$area.Top+($tab.Height-16*$scale)/2),[int](16*$scale),[int](16*$scale))
   $reference=Chrome-Ink $frames[2] $glyph
   if($reference -lt 5){throw "$name has no composed source glyph"}
   for($i=3;$i -lt $frames.Count;$i++){
    $moved=[Drawing.Rectangle]::new($glyph.X+[int](40*$scale),$glyph.Y,$glyph.Width,$glyph.Height)
    if((Chrome-Ink $frames[$i] $glyph) -lt $reference*.3 -and ($tab -eq $layers -or (Chrome-Ink $frames[$i] $moved) -lt $reference*.3)){$bad+=@($i)}
   }
  }
  @{invalid=$bad}|ConvertTo-Json|Set-Content (Join-Path $run ($name+'-pixels.json'))
  if($bad.Count){$script:frameFailures+=@("$name lost tab glyphs in frames $($bad -join ',')")}
 }
 [CapyRowPointer]::Key(0x1b);[CapyRowPointer]::Up()
 Wait-Until {$null -eq (Find-Preview)} 'Chrome pickup cancellation retained its preview'
 Invoke 'panel-tab-adjustments'
 $category=Control 'filter-category' -Arranged;$category.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
 $tone=Control 'Tone' -Name -Type $ControlType::ListItem
 $b=$category.Current.BoundingRectangle;$area=Chrome-Area ($b.Top-8*$scale) 80
 Chrome-Sample 'chrome-tone-category' $area {param($i)if($i -eq 3){$tone.GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()}} {
  param($frames,$final,$area,$name)
  $glyph=[Drawing.Rectangle]::new([int]($b.Left-$area.Left-22*$scale),[int]($b.Top-$area.Top+($b.Height-16*$scale)/2),[int](16*$scale),[int](16*$scale))
  $sourceInk=Chrome-Ink $frames[2] $glyph;$finalInk=Chrome-Ink $final $glyph
  if($sourceInk -lt 5 -or $finalInk -lt 5){throw "$name has no composed source or Tone reference glyph"}
  $reference=[Math]::Min($sourceInk,$finalInk);$bad=@();$scores=@()
  for($i=4;$i -lt $frames.Count;$i++){
   $ink=Chrome-Ink $frames[$i] $glyph;$scores+=@{frame=$i;ink=$ink}
   if($ink -lt $reference*.3){$bad+=@($i)}
  }
  @{glyph=$glyph;source_ink=$sourceInk;final_ink=$finalInk;invalid=$bad;scores=$scores}|ConvertTo-Json -Depth 6|Set-Content (Join-Path $run ($name+'-pixels.json'))
  if($bad.Count){$script:frameFailures+=@("$name lost its category glyph in frames $($bad -join ',')")}
 }
 $b=(Control 'panel-tab-navigator' -Arranged).Current.BoundingRectangle
 [CapyRowPointer]::RightClick([int]($b.Left+$b.Width/2),[int]($b.Top+$b.Height/2));Invoke 'Collapse column' -Name
 Wait-Until {$null -ne (Find 'column-icon-navigator' -Visible)} 'Drawer setup did not collapse the column'
 $b=(Control 'column-icon-navigator' -Arranged).Current.BoundingRectangle
 [CapyRowPointer]::RightClick([int]($b.Left+$b.Width/2),[int]($b.Top+$b.Height/2))
 $toggle=(Control 'Open individual panels' -Name).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern)
 if($toggle.Current.ToggleState -eq [System.Windows.Automation.ToggleState]::Off){$toggle.Toggle()}
 [CapyRowPointer]::Key(0x1b)
 $area=Chrome-Area ($origin.y+48*$scale) 350;$x=[int]($b.Left+$b.Width/2);$y=[int]($b.Top+$b.Height/2)
 Chrome-Sample 'chrome-drawer-open' $area {param($i)if($i -eq 3){[CapyRowPointer]::Down('mouse',$x,$y);[CapyRowPointer]::Up()}} ${function:Chrome-Overview}
 Capture 'chrome-final' -WithModel -Composed
 $displayPath=(State-File) -replace 'ui-state-','presentation-state-';$idleDisplay=@{snapshot=$null}
 Wait-Until {$display=Read-Snapshot $displayPath;$idleDisplay.snapshot=$display;$display.process_id -eq $review.Id -and $display.required -and $null -ne $display.displayed -and $display.displayed -ge $display.required} 'Idle chrome content did not reach the current displayed presentation'
 Wait-Until {$native=Presentation;$native.chrome_revision -and $native.presented_chrome_revision -eq $native.chrome_revision} 'Idle chrome did not reach its presented revision'
 $idle=Model;$shown=Presentation
 $reference=@{frames=@($idle.windows_glass.frames);revision=$shown.chrome_revision;presented=$shown.presented_chrome_revision}
 $script:chromeIdle=@{start=[Diagnostics.Stopwatch]::GetTimestamp();display=$idleDisplay.snapshot;frequency=[Diagnostics.Stopwatch]::Frequency;trace=((Split-Path -Leaf (State-File)) -replace '^ui-state-','latency-' -replace '\.json$','-presentations.csv')}
 $observations=@();for($i=0;$i -lt 90;$i++){
  $null=[TabFrames]::DwmFlush();$idle=Model;$shown=Presentation
  $observations+=@{sample=$i;ticks=[Diagnostics.Stopwatch]::GetTimestamp();frames=@($idle.windows_glass.frames);revision=$shown.chrome_revision;presented=$shown.presented_chrome_revision}
 }
 $script:chromeIdle.end=[Diagnostics.Stopwatch]::GetTimestamp()
 @{reference=$reference;observations=$observations;window=$script:chromeIdle}|ConvertTo-Json -Depth 8|Set-Content (Join-Path $run 'chrome-idle.json')
 if(@($observations|Where-Object {($_.frames -join ',') -ne ($reference.frames -join ',') -or $_.revision -ne $reference.revision -or $_.presented -ne $reference.presented}).Count){$script:frameFailures+=@('Idle chrome republished or produced GPU backdrop frames')}
 if($script:frameFailures.Count){throw ($script:frameFailures -join "`n")}
 @{theme=$Theme;device='mouse';transparency='high';navigator_handoff='passed';tab_glyphs='passed';tone_category='passed';drawer_handoff='passed';idle_chrome_revision='passed'}|ConvertTo-Json
}

function Chrome-IdleResult {
 $frequency=[decimal]$script:chromeIdle.frequency
 $start=[decimal]$script:chromeIdle.start;$end=[decimal]$script:chromeIdle.end
 $startNs=[long]([Math]::Floor($start/$frequency)*1000000000+[Math]::Floor(($start%$frequency)*1000000000/$frequency))
 $endNs=[long]([Math]::Floor($end/$frequency)*1000000000+[Math]::Floor(($end%$frequency)*1000000000/$frequency))
 $trace=Join-Path $run $script:chromeIdle.trace
 if(!(Test-Path -LiteralPath $trace)){throw 'Idle chrome has no presentation trace'}
 $status=Get-Content ($trace -replace '-presentations.csv$','-status.json') -Raw|ConvertFrom-Json
 if($status.overflow){throw 'Idle chrome presentation trace overflowed'}
 $rows=@(Import-Csv -LiteralPath $trace);$during=@($rows|Where-Object {[decimal]$_.render_start_ns -ge $startNs -and [decimal]$_.render_start_ns -le $endNs})
 @{window=$script:chromeIdle;start_ns=$startNs;end_ns=$endNs;trace=$trace;presentations=$rows.Count;idle_presentations=$during}|ConvertTo-Json -Depth 8|Set-Content (Join-Path $run 'chrome-idle-presentations.json')
 if($during.Count){throw "Idle chrome rendered $($during.Count) GPU presentations"}
 @{idle_presentations='passed';observed_presentations=$rows.Count}|ConvertTo-Json
}
function Release-Frames([string]$Name,$Area){
 $folder=Join-Path $run $Name;[IO.Directory]::CreateDirectory($folder)|Out-Null
 $frames=$null
 try{
  $sample=Sample-Frames $Area {param($i)if($i -eq 3){$script:releaseUp=[Diagnostics.Stopwatch]::GetTimestamp();[CapyRowPointer]::Up()}}
  $frames=$sample.frames;$times=$sample.times;$up=$script:releaseUp
  Wait-Until {$null -eq (Find-Preview)} "$Name did not hand over to its arranged controls"
  Wait-StablePixels {Screen-Pixels ([int]($Area.Left+$Area.Width/2)) ([int]($Area.Bottom-2*$scale))} -Composed|Out-Null
  $final=[TabFrames]::Grab([int]$Area.Left,[int]$Area.Top,[int]$Area.Width,[int]$Area.Height)
  $final.Save((Join-Path $folder 'final.png'))
  $envelope=[TabFrames+Envelope]::new($frames[2],$final)
  $errors=@();$scores=for($i=0;$i -lt $frames.Count;$i++){
   $frames[$i].Save((Join-Path $folder ('frame-{0:D3}.png' -f $i)))
   $invalid=$envelope.Difference($frames[$i])
   if($invalid -gt 8){$errors+=@($i)}
   @{frame=$i;invalid_pixels=$invalid}
  }
  @{area=$Area;up=$up;frequency=[Diagnostics.Stopwatch]::Frequency;times=$times;scores=$scores;invalid=$errors}|ConvertTo-Json -Depth 8|Set-Content (Join-Path $folder 'frames.json')
  if($errors.Count){$script:frameFailures+=@("$Name exposed a third strip state in frames $($errors -join ',')")}
 }finally{foreach($image in $frames){$image.Dispose()};if($final){$final.Dispose()}}
}
function Leftward-Tab([string]$Prefix,[string]$Panel,[string]$First,[string]$Device,[double]$Edge=-12){
 Invoke ($Prefix+$Panel)
 $originalOrder=(Tab-Order $Panel) -join ','
 $source=(Control ($Prefix+$Panel) -Arranged).Current.BoundingRectangle
 $firstBounds=(Control ($Prefix+$First) -Arranged).Current.BoundingRectangle
 $name="$Prefix$Panel-$Device-$Edge"
 $area=[Drawing.Rectangle]::new([int]($firstBounds.Left-8*$scale),[int]$firstBounds.Top,[int]($source.Right-$firstBounds.Left+16*$scale),[int]$firstBounds.Height)
 Wait-StablePixels {Screen-Pixels ([int]($source.Left+$source.Width/2)) ([int]($source.Bottom-5*$scale))} -Composed|Out-Null
 Capture ($name+'-before') -Composed
 [CapyRowPointer]::Down($Device,[int]($source.Left+8*$scale),[int]($source.Top+$source.Height/2))
 try{
  Walk ($source.Left+8*$scale) ($source.Top+$source.Height/2) ($firstBounds.Left+$Edge*$scale) ($source.Top+$source.Height/2)
  Wait-Until {$p=Preview;$p -and $p.insertion -eq 0} "$name did not preview slot zero"
  Wait-StablePixels {Screen-Pixels ([int]($firstBounds.Left+$source.Width/2)) ([int]($source.Bottom-5*$scale))} -Composed|Out-Null
  $preview=Preview
  Capture ($name+'-drag') -Composed
  $origin=[CapyRowPointer+Point]::new();$null=[CapyRowPointer]::ClientToScreen($drawingWindow,[ref]$origin)
  $title=@((Model).panels|Where-Object id -eq $Panel)[0].title
  $labels=(Find-Preview).FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,$title))
  $labelBounds=@();foreach($label in $labels){
   $box=$label.Current.BoundingRectangle;$labelBounds+=@($box.ToString())
   if($box.Left -lt $origin.x+$preview.bounds.x*$scale-1 -or $box.Right -gt $origin.x+($preview.bounds.x+$preview.bounds.width)*$scale+1){$script:frameFailures+=@("$name name extends outside its active tab")}
  }
  $from=[Drawing.RectangleF]::new(($source.Left-$origin.x),($source.Top-$origin.y),$source.Width,$source.Height)
  $moving=[Drawing.RectangleF]::new(($preview.bounds.x*$scale),($preview.bounds.y*$scale),($preview.bounds.width*$scale),($preview.bounds.height*$scale))
  $dragPath=Join-Path $run ($name+'-drag.png')
  $wrong=Check-Foot $name (Join-Path $run ($name+'-before.png')) $from $dragPath $moving
  Release-Frames $name $area
  $arranged=(Control ($Prefix+$Panel) -Arranged).Current.BoundingRectangle
  $settled=[Drawing.RectangleF]::new(($arranged.Left-$area.Left),($arranged.Top-$area.Top),$arranged.Width,$arranged.Height)
  $finalWrong=Check-Foot ($name+'-final') $dragPath $moving (Join-Path $run ($name+'/final.png')) $settled
  @{source=$source.ToString();preview=$preview;labels=$labelBounds;foot_mismatches=$wrong;final_foot_mismatches=$finalWrong}|ConvertTo-Json -Depth 8|Set-Content (Join-Path $run ($name+'-geometry.json'))
 }finally{if([CapyRowPointer]::Active){[CapyRowPointer]::Cancel()}}
 Wait-Until {$null -eq (Find-Preview)} "$name retained its preview"
 $order=@(Tab-Order $Panel)
 if($order[0] -ne $Panel){$script:frameFailures+=@("$name committed a different order from slot zero")}
 if(($order -join ',') -eq $originalOrder){return}
 & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'window';Invoke 'undo_workspace'
 Wait-Until {((Tab-Order $Panel) -join ',') -eq $originalOrder} "$name did not undo"
 if($Prefix -eq 'drawer-tab-'){
  if(!@((Model).state.customization.column_drawers|Where-Object {$_.tabs.panels -contains $Panel}).Count){Invoke 'column-icon-layers'}
  Wait-Until {@((Model).state.customization.column_drawers|Where-Object {$_.tabs.panels -contains $Panel}).Count -eq 1 -and (Find ($Prefix+$Panel) -Visible)} "$name did not reopen its drawer"
 }
}
function Drawer-Style([uint32]$Group,[string]$Name,[string]$Value){
 $previous=(Control 'drawer-tab-brushes' -Arranged).GetRuntimeId() -join ':'
 $grip=(Control ('drawer-grip-'+$Group) -Arranged).Current.BoundingRectangle
 [CapyRowPointer]::RightClick([int]($grip.Left+$grip.Width/2),[int]($grip.Top+$grip.Height/2))
 (Control $Name -Name -Type $ControlType::MenuItem).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
 Wait-Until {(Model).windows_tab_styles.$Group -eq $Value -and $null -ne (Find 'drawer-tab-brushes' -Visible) -and ((Control 'drawer-tab-brushes' -Arranged).GetRuntimeId() -join ':') -ne $previous} "Drawer did not rebuild with $Name style"
 Start-Sleep -Milliseconds 150
}
function Drawer-Review {
 $panels=@('layers','adjustments','brushes','sizes')
 $group=@((Model).layout.groups|Where-Object {$_.panels -contains 'layers'})[0].id
 foreach($panel in $panels){
  if(@((Model).layout.groups|Where-Object {$_.id -eq $group -and $_.panels -contains $panel}).Count){continue}
  $grip=(Control ('group-grip-'+$group) -Arranged).Current.BoundingRectangle
  [CapyRowPointer]::RightClick([int]($grip.Left+$grip.Width/2),[int]($grip.Top+$grip.Height/2))
  (Control 'Add built-in panel' -Name -Type $ControlType::MenuItem).GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
  $title=@((Model).panels|Where-Object id -eq $panel)[0].configuration_title -replace '^Configure ',''
  (Control $title -Name -Type $ControlType::MenuItem).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
  Wait-Until {@((Model).layout.groups|Where-Object {$_.id -eq $group -and $_.panels -contains $panel}).Count -eq 1} "Drawer setup did not add $panel"
 }
 $order=@((Model).layout.groups|Where-Object id -eq $group)[0].panels
 if(($order -join ',') -ne ($panels -join ',')){throw "Drawer setup has unexpected tab order: $($order -join ',')"}
 if(@((Model).layout.collapsed|Where-Object {$_.groups.icons.panel -contains 'layers'}).Count){
  (Control 'column-icon-layers' -Arranged).SetFocus();[CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x10),0x79)
  Wait-Until {$null -ne (Find 'Expand column' -Name)} 'Drawer setup did not expose Expand column'
  Invoke 'Expand column' -Name
  Wait-Until {@((Model).layout.collapsed|Where-Object {$_.groups.icons.panel -contains 'layers'}).Count -eq 0} 'Drawer setup did not expand the right column'
 }
 (Control 'panel-tab-layers' -Arranged).SetFocus();[CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x10),0x79)
 Wait-Until {$null -ne (Find 'Collapse column' -Name)} 'Drawer setup did not expose Collapse column'
 Invoke 'Collapse column' -Name
 Wait-Until {$null -ne (Find 'column-icon-layers')} 'Drawer setup did not collapse the right column'
 (Control 'column-icon-layers' -Arranged).SetFocus();[CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x10),0x79)
 Wait-Until {$null -ne (Find 'Open individual panels' -Name)} 'Drawer setup did not expose individual panels'
 $toggle=(Control 'Open individual panels' -Name).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern)
 if($toggle.Current.ToggleState -eq [System.Windows.Automation.ToggleState]::Off){$toggle.Toggle()}
 [CapyRowPointer]::Key(0x1b)
 Invoke 'column-icon-layers'
 Wait-Until {$null -ne (Find 'drawer-tab-brushes' -Visible) -and (Drawer-Group $group)} 'Drawer setup did not open its individual panel tabs'
 if((Model).windows_tab_styles.$group -ne 'automatic'){throw 'Drawer setup did not retain Automatic tab style'}
 $before=Drawer-Widths $panels
 $before|ConvertTo-Json|Set-Content (Join-Path $run 'drawer-before-widths.json')
 if(@($before.Values|Where-Object {$_ -gt 45}).Count -eq 0 -or @($before.Values|Where-Object {$_ -le 45}).Count -eq 0){throw 'Drawer setup did not fit a mixture of named and icon tabs'}
 Capture 'drawer-before-reorder' -Composed
 foreach($device in @('mouse','pen','touch')){foreach($edge in @(-12,4)){Leftward-Tab 'drawer-tab-' 'sizes' 'layers' $device $edge}}
 $bounds=(Control 'drawer-tab-brushes' -Arranged).Current.BoundingRectangle
 $first=(Control 'drawer-tab-layers' -Arranged).Current.BoundingRectangle
 $startX=$bounds.Left+8*$scale;$startY=$bounds.Top+$bounds.Height/2
 $endX=$first.Left+4*$scale
 [CapyRowPointer]::Down('mouse',[int]$startX,[int]$startY)
 try{
  Walk $startX $startY $endX $startY
  [CapyRowPointer]::Up()
 }finally{if([CapyRowPointer]::Active){[CapyRowPointer]::Cancel()}}
 $committed=@('brushes','layers','adjustments','sizes')
 Wait-Until {((Drawer-Group $group).tabs.panels -join ',') -eq ($committed -join ',') -and $null -eq (Find-Preview)} 'Drawer reorder did not commit its new order'
 Assert-TabKeyOrder @($committed|ForEach-Object {'drawer-tab-'+$_})
 $after=Drawer-Widths $panels
 $width=[Math]::Round((Control ('column-drawer-'+(Drawer-Group $group).anchor.column) -Arranged).Current.BoundingRectangle.Width/$scale,2)
 Capture 'drawer-after-reorder' -Composed
 Drawer-Style $group 'Icons only' 'icon'
 Drawer-Style $group 'Automatic' 'automatic'
 Wait-Until {((Drawer-Group $group).tabs.panels -join ',') -eq ($committed -join ',')} 'Fresh Automatic drawer changed the committed order'
 $fresh=Drawer-Widths $panels
 $freshWidth=[Math]::Round((Control ('column-drawer-'+(Drawer-Group $group).anchor.column) -Arranged).Current.BoundingRectangle.Width/$scale,2)
 $evidence=@{theme=$Theme;group=$group;order=$committed;before=$before;after=$after;fresh=$fresh;drawer_width=$width;fresh_drawer_width=$freshWidth;layout=(Model).state.workspace.layout;fresh_build='Icons only then Automatic reconstructs the native tab strip'}
 $evidence|ConvertTo-Json -Depth 90|Set-Content (Join-Path $run 'drawer-automatic.json')
 if([Math]::Abs($width-$freshWidth) -gt 1){throw 'Fresh drawer width differs from the post-drop width'}
 if(@($panels|Where-Object {[Math]::Abs($before[$_]-$fresh[$_]) -gt 1}).Count -eq 0){throw 'Fresh Automatic names did not change priority after the reorder'}
 foreach($panel in $panels){if([Math]::Abs($after[$panel]-$fresh[$panel]) -gt 1){throw "Drawer reorder left stale Automatic name width on $panel"}}
 Capture 'drawer-fresh-order' -Composed
 if($script:frameFailures.Count){throw ($script:frameFailures -join "`n")}
 & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -StateDirectory $run -Action Close -DiscardUnsaved
 if(!$review.WaitForExit(15000) -or $review.ExitCode -ne 0){throw 'Drawer review did not close cleanly'}
 [pscustomobject]@{theme=$Theme;drawer_automatic_reorder='passed';fresh_order_oracle='passed';width=$width;zero_exit='passed'}|ConvertTo-Json
}
try{
 Enter-CapyEnvironment @('CAPY_LATENCY_TRACE')
 if($Journey -like 'chrome-*' -or ($Journey -eq 'full' -and $Theme -eq 'dark')){$env:CAPY_LATENCY_TRACE='1'}
 $env:CAPY_STORAGE_DIR=Join-Path $run 'profile'
 [IO.File]::WriteAllText((Settings-File),(@{language=@{Explicit='en'};theme=$Theme}|ConvertTo-Json -Compress))
 $env:CAPY_TRACE_UI='1';$env:CAPY_SMOKE_TEST='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
 $stderr=Join-Path $run 'stderr.log'
 $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
 $null=$review.Handle
 [IO.File]::WriteAllText((Join-Path $repo 'artifacts/windows/tab-drag-review.json'),(@{process_id=$review.Id;run=$run}|ConvertTo-Json))
 Write-Output "Owned tab review $($review.Id)"
 $native=@{window=$null}
 Wait-Until {$startup=Model;if(!$startup.brush_ready -or !$startup.windows_workspace.ready -or $startup.windows_workspace.busy){return $false};$native.window=Owned-DrawingWindow $review;$null -ne $native.window} 'Review did not start' 75
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
 $script:frameFailures=@()
 $normal=$baseline.state.workspace.layout|ConvertTo-Json -Depth 80 -Compress
 $baseline|ConvertTo-Json -Depth 90|Set-Content (Join-Path $run 'before-model.json')
 if($Journey -in @('chrome-dark','chrome-light')){Chrome-Review;return}
 if($Journey -like 'drawer-*'){Drawer-Review;return}
 if($Journey -like 'release-*'){
  foreach($device in @('mouse','pen','touch')){foreach($edge in @(-12,4)){Leftward-Tab 'panel-tab-' 'sizes' 'tool_settings' $device $edge}}
  if(@((Model).layout.collapsed|Where-Object {$_.groups.icons.panel -contains 'layers'}).Count){
   (Control 'column-icon-layers' -Arranged).SetFocus();[CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x10),0x79)
   Wait-Until {$null -ne (Find 'Expand column' -Name)} 'Far-left setup did not expose Expand column'
   Invoke 'Expand column' -Name
  }
  foreach($device in @('mouse','pen','touch')){Leftward-Tab 'panel-tab-' 'adjustments' 'layers' $device}
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
   Assert-TabKeyOrder @('panel-tab-sizes','panel-tab-tool_settings')
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
  if($script:frameFailures.Count){throw ($script:frameFailures -join "`n")}
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
 Wait-Until {(Presentation).shaders_ready} 'Shader preparation did not settle before retained-control motion' 60
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
 Wait-Until {
  $m=Model;$p=Presentation
  $p.shaders_ready -and !$m.windows_recovery.busy -and $p.checkpoint_camera.zoom -eq $m.state.camera.zoom -and $p.revision -ge $m.workspace_update.revision
 } 'Camera checkpoint did not settle before Navigator motion' 60
 $cameraApplied=Presentation
 $cameraModel=Model|Select-Object shaders_ready,canvas_ready,brush_ready,windows_gpu_generation,windows_surface_ready,windows_display,windows_workspace,windows_recovery
 Walk $x $y ($x+45*$scale) ($y+12*$scale)
 Wait-Until {
  $p=Presentation;$g=$p.groups|Where-Object id -eq $navigator.id
  $old=$before.groups|Where-Object id -eq $navigator.id
  $g.bounds.x -gt $old.bounds.x+30
 } 'Native Navigator stopped moving'
 $after=Presentation
 $afterModel=Model|Select-Object shaders_ready,canvas_ready,brush_ready,windows_gpu_generation,windows_surface_ready,windows_display,windows_workspace,windows_recovery
 [IO.File]::WriteAllText((Join-Path $run 'navigator-motion.json'),(@{before=$before;camera_applied=$cameraApplied;after=$after;camera_model=$cameraModel;after_model=$afterModel}|ConvertTo-Json -Depth 60))
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
 if($Theme -eq 'dark'){[CapyRowPointer]::Initialize([uint32]$review.Id);Chrome-Review}
 & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -StateDirectory $run -Action Close -DiscardUnsaved
 if(!$review.WaitForExit(15000)){throw 'Owned tab-drag window did not close'}
 if($review.ExitCode -ne 0){throw "Owned tab-drag process exited $($review.ExitCode)"}
 if((Get-Item -LiteralPath $stderr).Length){throw 'Native stderr requires inspection'}
 [pscustomobject]@{os_touch_routing='passed';attached_preview='passed';fixed_hit_rectangles='passed';release_insertion='passed';different_grab_position='passed';pointer_cancel='passed';tear_off='passed';held_floating_drag='passed';incremental_publication='passed';retained_native_controls='passed';resize_grip_placement='passed';navigator_gpu_motion='passed';fast_tearoff_capture='passed';mixed_camera='passed';overview_occlusion_and_restore='passed';detached_cancel='passed';workspace_undo='passed';zero_exit='passed';scope='OS-injected touch; physical input and latency acceptance remain separate'}|ConvertTo-Json
}catch{
 $script:journeyFailure=$_
 try{Capture 'failure' -WithModel -Composed}catch{}
 [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{
 try{[CapyRowPointer]::Dispose()}catch{}
 try{[CapyCanvasTouch]::Dispose()}catch{}
 if($review -and !$review.HasExited){if($drawingWindow){$null=[CapyWindowApi]::PostMessage($drawingWindow,0x10,[UIntPtr]::Zero,[IntPtr]::Zero)};if(!$review.WaitForExit(10000)){$review.Kill();$review.WaitForExit()}}
 try{
  if($script:chromeIdle){
   try{Chrome-IdleResult}catch{
    $_|Out-String|Set-Content (Join-Path $run 'chrome-idle-failure.txt')
    if($script:journeyFailure){Write-Error $_ -ErrorAction Continue}else{throw}
   }
  }
 }finally{Exit-CapyEnvironment}
}
