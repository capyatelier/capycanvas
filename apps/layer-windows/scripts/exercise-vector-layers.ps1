param([Parameter(Mandatory)][string]$Executable,[ValidateSet('dark','light')][string]$Theme='dark',[switch]$ToolsOnly)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyFind='prefer-visible';$CapyWaitSeconds=20;$CapyEach={[CapyRowPointer]::Verify()}
Add-Type -AssemblyName System.Drawing
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
$null=[CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$run=Join-Path $repo ('artifacts/windows/vector-layers/'+$Theme+'-'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$CapyTraceDirectory=$run
$checks=[ordered]@{theme=$Theme;tools_only=[bool]$ToolsOnly}
$observations=[Collections.Generic.List[object]]::new()
$findings=[Collections.Generic.List[string]]::new()
$review=$null;$projection=$null;$noticeFrame=$null
. (Join-Path $PSScriptRoot 'VectorFixture.ps1')
function Tool-Samples($Points){
 Check-Projection
 $pixels=[Collections.Generic.List[int]]::new()
 foreach($point in $Points){
  $at=Screen-Point $point
  $bitmap=[Drawing.Bitmap]::new(9,9);$graphics=[Drawing.Graphics]::FromImage($bitmap)
  try{
   $graphics.CopyFromScreen($at[0]-4,$at[1]-4,0,0,$bitmap.Size)
   for($y=0;$y -lt 9;$y++){for($x=0;$x -lt 9;$x++){$pixels.Add($bitmap.GetPixel($x,$y).ToArgb())}}
  }finally{$graphics.Dispose();$bitmap.Dispose()}
 }
 $pixels -join ','
}
function Tool-History([string]$Name,$Points,[string]$Before){
 Park
 $after=Tool-Samples $Points
 if($after -eq $Before){throw "$Name did not change its artwork samples"}
 $record=[ordered]@{name=$Name;points=$Points;before=$Before;after=$after;undo=$null;redo=$null}
 try{
  $revision=Revision;Shortcut-Command 'undo';Park
  Wait-Until {$record.undo=Tool-Samples $Points;(Revision) -ne $revision -and $record.undo -eq $Before} "$Name Undo did not restore the exact artwork samples" 30
  $revision=Revision;Shortcut-Command 'redo';Park
  Wait-Until {$record.redo=Tool-Samples $Points;(Revision) -ne $revision -and $record.redo -eq $after} "$Name Redo did not restore the exact artwork samples" 30
 }finally{$record|ConvertTo-Json -Depth 6|Set-Content -LiteralPath (Join-Path $run ("tools-$Name-history.json"))}
 $checks[$Name]=[ordered]@{undo='exact';redo='exact';sample_pixels=$Points.Count*81}
}
function Tool-Panel([string]$Id){
 if(!@((Model).layout.groups|Where-Object active -eq $Id).Count){Invoke $(if(Find ('drawer-tab-'+$Id)){'drawer-tab-'+$Id}else{'panel-tab-'+$Id})}
 Wait-Until {@((Model).layout.groups|Where-Object active -eq $Id).Count -gt 0} "$Id did not become the active panel"
}
function Tool-Number([string]$Id,[string]$Text){
 $entry=Reveal $Id;$entry.SetFocus()
 Wait-Until {$entry.Current.HasKeyboardFocus} "$Id did not receive keyboard focus"
 $entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($Text)
 Key 0x0D
 (Control 'drawing-canvas').SetFocus();Park
}
function Sketch-VectorRoutes([long]$Layer){
 $switch={
  $id='builtin:workspace:painter';$choice=Find ('workspace-switch-'+$id) -Visible
  if(!$choice){Invoke 'header-workspace-menu';$choice=Control 'Sketch' -Name -Type ([System.Windows.Automation.ControlType]::MenuItem) -Arranged}
  $choice.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
  if($choice.Current.ControlType-eq[System.Windows.Automation.ControlType]::MenuItem){Key 0x1B}
  Wait-Until {$m=Model;$m.windows_workspace.id-eq$id -and !$m.windows_workspace.busy -and $m.canvas_ready} 'Sketch did not adopt its native header'
 }
 $lock=Control 'layer-lock' -Arranged;$lock.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
 Wait-Until {(Model).state.layer_tools.editing_layer.locked} 'The vector layer did not lock for its header reason'
 & $switch
 $locked=@((Model).header.items|Where-Object {$_.resolved_control.command-eq'scale_rotate'})[0]
 if(!$locked -or $locked.enabled -or !$locked.disabled_reason){throw 'The locked Sketch Scale/rotate has no shared refusal'}
 $frame=Control ('header-item-'+$locked.id) -Arranged
 if($frame.Current.HelpText-cne$locked.disabled_reason){throw 'The locked native header omitted the shared refusal'}
 Reason-Hover $frame $locked.disabled_reason
 Capture "sketch-locked-header-$Theme" -WithModel -Composed
 Use-PaintWorkspace
 (Control 'layer-lock' -Arranged).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
 Wait-Until {!(Model).state.layer_tools.editing_layer.locked} 'The vector layer did not unlock'
 & $switch
 $scale=@((Model).header.items|Where-Object {$_.resolved_control.command-eq'scale_rotate'})[0]
 $frame=Control ('header-item-'+$scale.id) -Arranged
 if(!$scale.enabled -or $scale.disabled_reason -or $frame.Current.HelpText){throw 'Unlocking did not clear the native header reason'}
 if(@((Model).state.commands|Where-Object id -eq 'deselect')[0].enabled){Shortcut-Command 'deselect'}
 Click $frame
 Wait-Until {(Selected 'move') -and (Selected 'scale_rotate')} 'Sketch Scale/rotate did not enter Move before selecting paths'
 $select=@((Model).header.items|Where-Object {$_.resolved_control.command-eq'select'})[0]
 Click (Control ('header-item-'+$select.id) -Arranged)
 Wait-Until {(Model).state.customization.drawer -and (Find 'tool-drawer' -Visible)} 'Sketch Select did not open its native choices'
 $paths=@((Model).state.tool_panels.tools.subtools)
 if($paths.Count-lt2 -or $paths[0].action.command-ne'move' -or $paths[1].action.command-ne'edit_points'){throw 'Sketch Select did not lead with Select paths and Edit points'}
 $drawer=Control 'tool-drawer' -Arranged
 foreach($index in 0,1){if((Control ('tool-subtool-'+$index) -Within $drawer -Arranged).Current.Name-cne$paths[$index].label){throw 'Sketch path choices differ from their shared labels'}}
 Click (Control 'tool-subtool-0' -Within $drawer -Arranged)
 Wait-Until {(Selected 'move') -and (Selected 'scale_rotate')} 'Select paths did not select the vector Move tool'
 if((Model).state.customization.drawer){(Control 'drawing-canvas').SetFocus();Key 0x1B}
 Wait-Until {!(Model).state.customization.drawer} 'The Sketch path drawer did not acknowledge closure'
 Use-PaintWorkspace;Fit-Canvas;Park;Take-Projection
 if((Active).id-ne$Layer){throw 'The Sketch routes changed the active vector layer'}
 $checks.sketch_paths_and_header_reason='passed'
}
function Vector-Availability([long]$Layer,[long]$Pen){
 $paint=@(Rows|Where-Object {!(Vector $_) -and !$_.paper})[0].id
 $revision=Revision;$points=@(@(650,320),@(650,520));$pixels=Tool-Samples $points
 $choice=Tool-Item 'subtools' {param($item) $item.enabled -and $item.disabled_reason -and $item.action.type-eq'select_brush'} 'a dimmed selectable pen'
 $null=Tool-Availability $choice -Hover
 $null=Choose-Tool-Item $choice -Acknowledged
 Wait-Until {(Model).state.brush.preset-eq$choice.action.id -and (Model).state.notice -and (Notice-Parts).Contains('canvas-notice')} 'Choosing the dimmed row did not explain before a stroke'
 $dim=Tool-TextPixels $choice "tool-reason-dim-$Theme"
 Invoke "layer-$paint-name"
 Wait-Until {(Active).id-eq$paint -and (Model).state.brush.preset-eq$choice.action.id -and !(Model).state.tool_set.subtools[$choice.index].disabled_reason} 'The chosen pen or its cleared availability did not survive leaving vector'
 $clear=Tool-TextPixels $choice "tool-reason-clear-$Theme"
 if($dim.identity-cne$clear.identity -or ($dim.bounds-join',')-cne($clear.bounds-join',') -or $dim.background-ne$clear.background -or $dim.contrast-ge$clear.contrast -or $clear.reason){throw 'The native tool content did not dim independently of its retained selected background, then clear'}
 Invoke "layer-$Layer-name";Wait-Until {(Active).id-eq$Layer} 'The vector layer did not return after availability'
 $null=Select-Brush 'G-Pen'
 $null=Search (Label 'sculpt') {param($result) $result.id-like'*sculpt*' -and $result.label-eq(Label 'sculpt')} 'Sculpt'
 Wait-Until {(Selected 'sculpt') -and (Model).state.notice -and (Notice-Parts).Contains('canvas-notice')} 'Sculpt did not explain its vector refusal before a stroke'
 Capture "sculpt-immediate-notice-$Theme" -WithModel -Composed
 $null=Select-Brush 'G-Pen'
 Wait-Until {(Model).state.brush.preset-eq$Pen -and !(Model).state.notice} 'A working pen did not clear the immediate refusal'
 if((Revision)-ne$revision -or (Tool-Samples $points)-cne$pixels){throw 'Checking unavailable choices changed the exact artwork or history'}
 $checks.tool_reason=[ordered]@{dim=$dim;clear=$clear;clickable_and_immediate='passed';pen_retained='passed';sculpt_before_stroke='passed'}
}
function Vector-Tools{
 $layer=New-VectorLayer
 Tool-Panel 'tool_settings'
 Tool-Number 'tool-setting-size' '24'
 Wait-Until {(Model).state.brush.diameter -eq 24} 'The vector pen size did not become 24 pixels'
 Select-Tool 'figure'
 $null=Choose-Tool-Item (Tool-Item 'groups' {param($item) $item.icon -eq 'rectangle'} 'Rectangle') -Acknowledged
 $null=Choose-Tool-Item (Tool-Item 'subtools' {param($item) $item.action.action.tool.figure.shape -eq 'rectangle' -and $item.action.action.tool.figure.paint -eq 'fill'} 'a filled rectangle') -Acknowledged
 Stroke (Line @(400,400) @(700,620))
 Expect 'the filled vector rectangle' @(550,510) 'ink'
 Expect 'beside the vector rectangle' @(780,510) 'ink' $false
 Capture "tools-figure-$Theme" -WithModel -Composed

 $null=Choose-Tool-Item (Tool-Item 'groups' {param($item) $item.icon -eq 'path'} 'Path') -Acknowledged
 $null=Choose-Tool-Item (Tool-Item 'subtools' {param($item) $item.action.action.tool.figure.shape -eq 'path' -and $item.action.action.tool.figure.paint -eq 'outline'} 'a path outline') -Acknowledged
 foreach($point in @(@(1000,400),@(1300,400))){Stroke (Line $point $point 1)}
 Stroke (Line @(1300,700) @(1400,780) 4)
 Capture "tools-path-drafting-$Theme" -WithModel -Composed
 Key 0x0D;Park
 Expect 'the completed vector path' @(1150,400) 'ink'
 Capture "tools-path-$Theme" -WithModel -Composed

 Shortcut-Command 'edit_points';Park
 Wait-Until {Selected 'edit_points'} 'Edit Points did not become the tool'
 Expect 'above the path before anchor editing' @(1150,325) 'ink' $false
 $points=@(@(1150,325),@(1150,400));$before=Tool-Samples $points
 Stroke (Line @(1000,400) @(1000,250))
 Expect 'the moved path anchor' @(1150,325) 'ink'
 Tool-History 'anchor' $points $before
 Capture "tools-edit-points-$Theme" -WithModel -Composed

 Tool-Panel 'properties'
 Wait-Until {@((Model).state.layer_properties.controls|Where-Object key -eq 'path_width').Count -eq 1} 'Selected path width did not reach Properties'
 Expect 'beside the path before restyling' @(1300,550) 'ink' $false
 $points=@(@(1300,550),@(1150,325));$before=Tool-Samples $points
 Tool-Number 'property-path_width' '160'
 Wait-Until {@((Model).state.layer_properties.controls|Where-Object key -eq 'path_width')[0].value.value -eq 160} 'The native path width did not become 160 pixels'
 Expect 'the restyled path outline' @(1300,550) 'ink'
 Tool-History 'width' $points $before
 Capture "tools-properties-$Theme" -WithModel -Composed

 Shortcut-Command 'move';Park
 Wait-Until {Selected 'move'} 'Move did not become the tool'
 Stroke (Line @(350,350) @(750,670))
 Capture "tools-frame-$Theme" -WithModel -Composed
 Expect 'outside the rectangle before scaling' @(780,700) 'ink' $false
 $points=@(@(780,700),@(550,510));$before=Tool-Samples $points
 Stroke (Line @(700,620) @(850,760))
 Expect 'the scaled vector rectangle' @(780,700) 'ink'
 Tool-History 'scale' $points $before
 Capture "tools-scaled-$Theme" -WithModel -Composed
 if((Active).id -ne $layer){throw 'The vector tools changed the active layer'}
 Tool-Panel 'layers'
 Write-Output 'PASS: Native Figure, Path, anchor editing, Properties and Move scaling with exact Undo/Redo samples'
}
try{
 Enter-CapyEnvironment
 $env:CAPY_STORAGE_DIR=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1'
 [IO.File]::WriteAllText((Settings-File),(@{language=@{Explicit='en'};theme=$Theme}|ConvertTo-Json -Depth 4))
 $stderr=Join-Path $run 'stderr.log'
 $review=Start-Process -FilePath $Executable -WorkingDirectory $run -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
 $null=$review.Handle
 Write-Output "Owned vector layer review $($review.Id): $run"
 $native=@{window=$null}
 Wait-Until {$native.window=Owned-DrawingWindow $review;$m=Model;$native.window -and $m.canvas_ready -and $m.brush_ready -and $m.windows_workspace.ready -and !$m.windows_workspace.busy} 'The vector layer review did not start' 90
 $root=$native.window.Root;$drawingWindow=$native.window.Handle
 $root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Maximized)
 if(![CapyRowPointer]::SetForegroundWindow($drawingWindow)){throw 'The owned drawing did not accept foreground input'}
 [CapyRowPointer]::Initialize([uint32]$review.Id)
 Wait-Until {(Control 'drawing-canvas' -Arranged).Current.BoundingRectangle.Width -gt 1200 -and !(Model).windows_workspace.busy} 'The maximized canvas did not settle'
 if((Model).state.theme -ne $Theme){throw "The review opened in $((Model).state.theme) instead of $Theme"}
 Use-PaintWorkspace
 if($ToolsOnly){
  & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'File';Invoke 'new_document'
  $epoch=(Model).state.document_file.epoch
  New-DrawingPixels 2000 1400;Invoke 'new-drawing-create'
  Wait-Until {$m=Model;$m.state.document_file.epoch -ne $epoch -and !$m.state.new_drawing -and !$m.state.document_file.busy -and $m.canvas_ready -and $m.brush_ready} 'The vector tools drawing did not become ready' 90
 }
 Set-Color '#1A1A1F'
 Fit-Canvas;Park;Take-Projection
 Footer-Check 'start'

 if($ToolsOnly){Vector-Tools}else{
 $pencil=Select-Brush 'Pencil'
 $v1=New-VectorLayer
 $gpen=@{id=$null}
 Wait-Until {
  $m=Model;$tool=@($m.state.tool_set.subtools|Where-Object selected)[0]
  $gpen.id=$m.state.brush.preset
  $tool.label -eq 'G-Pen' -and $tool.action.type -eq 'select_brush' -and $tool.action.id -eq $m.state.brush.preset -and $m.brush_ready
 } 'The new vector layer did not switch the brush to G-Pen' 120
 $gpen=$gpen.id
 $revision=Revision
 Stroke (Line @(400,300) @(900,340))
 Wait-Until {(Revision) -ne $revision} 'The pen stroke did not reach the vector layer'
 Expect 'discover ink' @(650,320) 'ink'
 if((Active).id -ne $v1){throw 'Inking moved away from the vector layer'}
 Capture "1-discover-$Theme" -WithModel -Composed
 $checks.discover_and_ink=[ordered]@{vector_layer=$v1;pencil=$pencil;g_pen=$gpen}
 Write-Output 'PASS: New vector layer chooses G-Pen and inks a line'

 Sketch-VectorRoutes $v1
 Vector-Availability $v1 $gpen
 $null=Select-Brush 'Pencil'
 Wait-Until {(Model).state.notice -and (Notice-Parts).Contains('canvas-notice')} 'Choosing Pencil did not explain its refusal before the stroke'
 Stroke (Line @(400,500) @(900,540) 4) -Refused
 $notice=Accept-Notice 'use_vector_pen' @('use_vector_pen','new_paint_layer','rasterize_layer') 'wrong-brush' 'Use G-Pen'
 $offer=@($notice.actions|Where-Object id -eq 'use_vector_pen')[0].name
 Wait-Until {Brush-Ready $gpen} "$offer did not choose G-Pen" 120
 Expect 'the refused pencil left the paths alone' @(650,520) 'ink' $false 5
 $revision=Revision
 Stroke (Line @(400,500) @(900,540))
 Wait-Until {(Revision) -ne $revision} 'The chosen pen did not reach the vector layer'
 Expect 'pen line after the notice' @(650,520) 'ink'
 Capture "2-use-pen-$Theme" -WithModel -Composed
 $null=Select-Brush 'Pencil'
 $before=@(Rows).Count
 Stroke (Line @(400,700) @(900,740) 4) -Refused
 $null=Accept-Notice 'new_paint_layer' @('use_vector_pen','new_paint_layer','rasterize_layer') 'paint-layer' 'New Paint Layer'
 Wait-Until {$a=Active;@(Rows).Count -eq $before+1 -and $a -and !(Vector $a)} 'New Paint Layer did not add an active paint layer'
 $p2=(Active).id
 if((Position $p2)+1 -ne (Position $v1)){throw 'The new paint layer is not directly above the vector layer'}
 if((Model).state.brush.preset -ne $pencil){throw 'New Paint Layer changed the brush'}
 Expect 'the refused pencil left nothing' @(650,720) 'mark' $false 5
 $paint=(Active).paint_revision
 Stroke (Line @(400,700) @(900,740))
 Wait-Until {$a=Active;$a.id -eq $p2 -and $a.paint_revision -ne $paint} 'The pencil did not paint the new paint layer' 60
 Expect 'pencil on the new paint layer' @(650,720) 'mark'
 Capture "2-paint-layer-$Theme" -WithModel -Composed
 $checks.wrong_brush=[ordered]@{use_pen=$offer;paint_layer=$p2}
 Write-Output 'PASS: Wrong-brush notice offers the pen and a paint layer'

 $v2=New-VectorLayer
 Wait-Until {Brush-Ready $gpen} 'The second vector layer did not switch the brush to G-Pen' 120
 Select-Tool 'figure'
 $null=Choose-Tool-Item (Tool-Item 'groups' {param($item) $item.icon -eq 'ellipse'} 'Ellipse') -Acknowledged
 $null=Choose-Tool-Item (Tool-Item 'subtools' {param($item) $item.action.action.tool.figure.shape -eq 'ellipse' -and $item.action.action.tool.figure.paint -eq 'outline'} 'an ellipse outline') -Acknowledged
 $revision=Revision
 Stroke (Line @(1100,300) @(1400,500) 8)
 Wait-Until {(Revision) -ne $revision} 'The ellipse outline did not reach the vector layer'
 Expect 'ellipse outline' @(1250,300) 'ink'
 Set-Color '#26B340'
 Select-Tool 'fill'
 $null=Choose-Tool-Item (Tool-Item 'subtools' {param($item) $item.action.action.tool.region.fill -and $item.action.action.tool.region.source -eq 'visible'} 'Visible') -Acknowledged
 $before=@(Rows).Count
 Stroke (Line @(1250,400) @(1250,400) 1)
 Expect 'filled inside the outline' @(1250,400) 'green' $true 60
 if(@(Rows).Count -ne $before -or (Active).id -ne $v2){throw 'Filling inside the outline changed the layers'}
 Stroke (Line @(1600,900) @(1600,900) 1)
 Wait-Until {$a=Active;@(Rows).Count -eq $before+1 -and $a -and !(Vector $a)} 'Filling outside the lines did not add a paint layer' 60
 $p3=(Active).id
 $back=@((Model).state.notice.actions|Where-Object id -eq 'back_to_layer')[0]
 if(!$back){throw 'The outside-path fill did not offer Back to Layer'}
 $null=Accept-Notice 'back_to_layer' @('back_to_layer') 'outside-fill' $back.label
 Wait-Until {(Active).id-eq$v2} 'Back to Layer did not return to its exact vector owner'
 if((Position $p3) -ne (Position $v2)+1){throw 'The filled paint layer is not directly below the lines'}
 Expect 'filled below the lines' @(1600,900) 'green' $true 90
 Expect 'the outline keeps its fill' @(1250,400) 'green'
 Expect 'the outline stays above the fill' @(1250,300) 'ink'
 Capture "3-color-$Theme" -WithModel -Composed
 $null=Select-Brush 'G-Pen';Invoke "layer-$p3-name"
 Wait-Until {(Active).id-eq$p3 -and (Model).state.brush.preset-eq$gpen} 'The explicitly chosen pen did not survive leaving the vector layer'
 Set-Color '#1A1A1F'
 $checks.color_lines=[ordered]@{vector_layer=$v2;paint_below=$p3}
 Write-Output 'PASS: Fill colors inside the outline and below the lines'

 $v3=New-VectorLayer
 Select-Tool 'pen'
 Wait-Until {Brush-Ready $gpen} 'The pen tool did not return to G-Pen' 120
 foreach($segment in @(@(@(300,1000),@(1100,1000)),@(@(500,850),@(500,1150)),@(@(900,850),@(900,1150)))){
  $revision=Revision
  Stroke (Line $segment[0] $segment[1])
  Wait-Until {(Revision) -ne $revision} 'A crossing line did not reach the vector layer'
 }
 foreach($point in @(@(400,1000),@(700,1000),@(1000,1000),@(500,900),@(900,900))){Expect 'crossing lines' $point 'ink'}
 Capture "4-crossing-lines-$Theme" -WithModel -Composed
 $known=$findings.Count
 Select-Tool 'eraser'
 $modes=@(foreach($mode in 'erase_touched_part','erase_whole_paths','erase_to_intersection'){
  $match={param($item) $item.action.command -eq $mode}.GetNewClosure()
  $choice=Tool-Item 'subtools' $match (Label $mode)
  if($choice.label -ne (Label $mode)){throw "The Tools list labels $mode '$($choice.label)' instead of '$(Label $mode)'"}
  $choice
 })
 if(($modes.index -join ',') -ne '0,1,2'){throw "The Tools list does not start with the vector eraser modes: $($modes.index -join ',')"}
 $chosen=Choose-Tool-Item $modes[2]
 if(!$chosen){$findings.Add("Choosing $($modes[2].label) in the Tools list did not take effect")}
 $revision=Revision
 Stroke (Line @(700,980) @(700,1020) 4)
 Wait-Until {(Revision) -ne $revision} 'The eraser did not edit the vector layer'
 Expect 'span between the crossings' @(700,1000) 'ink' $false 60
 foreach($point in @(@(560,1000),@(840,1000))){Expect 'erased up to the crossings, beyond the eraser' $point 'ink' $false -Deferred}
 foreach($point in @(@(400,1000),@(1000,1000),@(500,900),@(900,900))){Expect 'kept line parts' $point 'ink'}
 Capture "4-erase-to-intersection-$Theme" -WithModel -Composed
 $checks.erase_to_intersection=[ordered]@{vector_layer=$v3;modes=@($modes.label)}
 Write-Output $(if($findings.Count -eq $known){'PASS: Erase up to intersection removes only the span between crossings'}else{'FAIL: Erase up to intersection: '+(@($findings)[$known..($findings.Count-1)] -join '; ')})

 Select-Tool 'gradient'
 $before=@(Rows).Count
 Stroke (Line @(1500,700) @(1800,700) 4) -Refused
 $null=Accept-Notice 'new_paint_layer_below' @('new_paint_layer_below','rasterize_layer') 'gradient' 'New Paint Layer Below'
 Wait-Until {$a=Active;@(Rows).Count -eq $before+1 -and $a -and !(Vector $a)} 'New Paint Layer Below did not add an active paint layer'
 $p4=(Active).id
 if((Position $p4) -ne (Position $v3)+1){throw 'The new paint layer is not directly below the crossing lines'}
 Invoke "layer-$v3-name"
 Wait-Until {(Active).id -eq $v3} 'The crossing lines did not become the active layer again'
 $vectors=@(Vectors).Count
 Shortcut-Command 'merge_down'
 $null=Accept-Notice 'merge' @('merge') 'merge' (Label 'merge_down')
 Wait-Until {@(Vectors).Count -eq $vectors-1 -and !@(Vectors|Where-Object id -eq $v3).Count -and !(Vector (Active))} 'Merge Down did not turn the vector layer into pixels' 120
 foreach($point in @(@(400,1000),@(1000,1000),@(500,900),@(900,900))){Expect 'merged line parts' $point 'ink' $true 60}
 Expect 'merged span between the crossings' @(700,1000) 'ink' $false
 Capture "5-merged-$Theme" -WithModel -Composed
 $checks.merge=[ordered]@{paint_below=$p4;merged_into=(Active).id;vector_layers=@(Vectors).Count}
 Write-Output 'PASS: Merge Down rasterizes the lines through its notice'
 }

 Footer-Check 'end'
 [CapyRowPointer]::Dispose()
 & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Close -DiscardUnsaved -StateDirectory $run
 if((Get-Item -LiteralPath $stderr).Length){throw 'Native stderr requires inspection'}
 if($findings.Count){throw "Journey findings: $($findings -join '; ')"}
 $checks.evidence=$run
 $checks|ConvertTo-Json -Depth 6|Tee-Object -FilePath (Join-Path $run 'results.json')
}catch{
 if($review -and !$review.HasExited -and $root){try{Capture 'failure' -WithModel -Composed}catch{}}
 [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{
 try{[CapyRowPointer]::Dispose()}catch{}
 ConvertTo-Json -InputObject @($observations) -Depth 6|Set-Content -LiteralPath (Join-Path $run 'pixels.json')
 ConvertTo-Json -InputObject @($findings)|Set-Content -LiteralPath (Join-Path $run 'findings.json')
 Exit-CapyEnvironment
}
