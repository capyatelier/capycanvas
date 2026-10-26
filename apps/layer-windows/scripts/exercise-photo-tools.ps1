param([Parameter(Mandatory)][string]$Executable,[ValidateSet('dark','light','mix-dark','mix-light')][string]$Theme='dark')
$ErrorActionPreference='Stop'
$mixOnly=$Theme.StartsWith('mix-');if($mixOnly){$Theme=$Theme.Substring(4)}
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyWaitSeconds=45
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$run=Join-Path $repo ('artifacts/windows/photo-tools/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$CapyTraceDirectory=$run
$checks=[ordered]@{}
$CapyPopups=$true
function Visible([string]$Id){$item=Find $Id;if($item -and !$item.Current.IsOffscreen){$item}}
function Bar {Visible 'canvas-action-bar'}
function Kind {(Model).state.canvas_bar.context.kind}
function Paint {@((Model).state.layers|ForEach-Object {"$($_.id):$($_.paint_revision)"}) -join ','}
function Layers {@((Model).state.layers)}
function Value([string]$Id){((Model).state.tool_settings|Where-Object id -eq $Id).value}
function Command([string]$Id){(Model).state.commands|Where-Object id -eq $Id|Select-Object -First 1}
function Choice([string]$Id){@((Model).state.canvas_bar.items|Where-Object {$_.option.Choice.id -eq $Id})[0].option.Choice}
function Idle {Wait-Until {$m=Model;$m -and $m.brush_ready -and !$m.state.document_file.busy -and !@($m.state.requests).Count} 'The document did not settle' 60}
function MenuOpen{
 $condition=[System.Windows.Automation.AndCondition]::new(
  [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty,$review.Id),
  [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::FrameworkIdProperty,'XAML'),
  [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::MenuItem))
 $null -ne [System.Windows.Automation.AutomationElement]::RootElement.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$condition)
}
function Pick([string]$Name){
 $match=[System.Windows.Automation.AndCondition]::new(@(
  [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty,$review.Id),
  [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,$Name),
  [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::MenuItem)))
 $item=@{value=$null};Wait-Until {$item.value=[System.Windows.Automation.AutomationElement]::RootElement.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$match);$null -ne $item.value} "No menu item $Name" 5
 $pattern=$null
 if($item.value.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern,[ref]$pattern)){$pattern.Invoke()}
 elseif($item.value.TryGetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern,[ref]$pattern)){$pattern.Toggle()}
 else{$item.value.GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()}
}
function Center($Element){$r=$Element.Current.BoundingRectangle;@([int]($r.X+$r.Width/2),[int]($r.Y+$r.Height/2))}
function Screen([double]$X,[double]$Y){$c=(Model).state.camera;$r=(Find 'drawing-canvas').Current.BoundingRectangle
 @([int]($r.X+$c.translation[0]+$c.zoom*$X),[int]($r.Y+$c.translation[1]+$c.zoom*$Y))}
function Tap([string]$Id,[string]$Device){
 $item=@{value=$null};Wait-Until {$item.value=Find $Id;$item.value -and !$item.value.Current.IsOffscreen -and $item.value.Current.IsEnabled} "Missing bar control $Id" 10
 $at=Center (Control $Id -Arranged);[CapyRowPointer]::Down($Device,$at[0],$at[1]);Start-Sleep -Milliseconds 40;[CapyRowPointer]::Up()
}
function Pick-ToolAction([string]$Group,[string]$Command,[string]$Device){
 $id="tool-action-group-$Group"
 $null=Reveal $id -Focus
 $box=Control $id -Arranged
 $scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
 $bounds=Native-Bounds $id $scale
 $trace=[ordered]@{device=$Device;id=$id;native_bounds=$bounds;control=@(Control-Trace $box)}
 $ancestors=@($trace.control|Where-Object {$_.process -eq $review.Id -and $_.id -match '^workspace-group-\d+$'})
 if($ancestors.Count -ne 1){$trace|ConvertTo-Json -Depth 12|Set-Content (Join-Path $run ($id+'-'+$Device+'-input.json'));throw "Tool action group $Group has no unique owned panel ancestor"}
 $viewport=Native-Bounds ($ancestors[0].id -replace '^workspace-group-','panel-body-scroll-') $scale
 $trace.panel_viewport=$viewport;$trace.owned_root=$root.Current.BoundingRectangle
 if(!$viewport.Contains($bounds) -or !$trace.owned_root.Contains($bounds) -or $box.Current.IsOffscreen -or !$box.Current.IsEnabled -or $box.Current.ProcessId -ne $review.Id){$trace|ConvertTo-Json -Depth 12|Set-Content (Join-Path $run ($id+'-'+$Device+'-input.json'));throw "Tool action group $Group is outside its owned panel viewport"}
 $trace.point_source='native_bounds_center'
 $x=[int]($bounds.X+$bounds.Width/2);$y=[int]($bounds.Y+$bounds.Height/2)
 $point=[System.Windows.Point]::new($x,$y)
 $trace.before=Input-Trace $x $y;$trace.expand_before=$box.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Current.ExpandCollapseState.ToString()
 if(!$bounds.Contains($point) -or $trace.before.foreground -ne $trace.before.owned_window -or !@($trace.before.hit|Where-Object {$_.process -eq $review.Id}).Count){$trace|ConvertTo-Json -Depth 12|Set-Content (Join-Path $run ($id+'-'+$Device+'-input.json'));throw "Input candidate is outside the owned Tool action group $Group"}
 [CapyRowPointer]::Down($Device,$x,$y);Start-Sleep -Milliseconds 40;[CapyRowPointer]::Up()
 try{Wait-Until {$box.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Current.ExpandCollapseState -eq [System.Windows.Automation.ExpandCollapseState]::Expanded} "Tool action group $Group did not expand from $Device input"}finally{
  $trace.after=Input-Trace $x $y;$trace.expand_after=$box.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Current.ExpandCollapseState.ToString()
  $trace|ConvertTo-Json -Depth 12|Set-Content (Join-Path $run ($id+'-'+$Device+'-input.json'))
 }
 $caption=(Command $Command).label
 $option=Control $caption -Name -Type ([System.Windows.Automation.ControlType]::ListItem) -Arranged
 $at=Center $option
 [CapyRowPointer]::Down($Device,$at[0],$at[1]);Start-Sleep -Milliseconds 40;[CapyRowPointer]::Up()
 Wait-Until {(Command $Command).selected} "Tool Settings did not select $Command"
}
function Touch([string]$Device,[int[]]$At){[CapyRowPointer]::Down($Device,$At[0],$At[1]);Start-Sleep -Milliseconds 40;[CapyRowPointer]::Up();Start-Sleep -Milliseconds 150}
function Drag([string]$Device,[int[]]$From,[int[]]$To,[scriptblock]$BeforeLast){
 [CapyRowPointer]::Down($Device,$From[0],$From[1])
 for($step=1;$step -le 12;$step++){[CapyRowPointer]::Move([int]($From[0]+($To[0]-$From[0])*$step/12),[int]($From[1]+($To[1]-$From[1])*$step/12));Start-Sleep -Milliseconds 12;if($step -eq 11 -and $BeforeLast){& $BeforeLast}}
 [CapyRowPointer]::Up();Start-Sleep -Milliseconds 150
}
function Tool([string]$Command){
 $id=Tool-Tile $Command;Invoke-Id $id
 $id
}
function Subtool([string]$Command){
 $choice=Tool-Choice $Command;Invoke-Id $choice.id
 Wait-Until {@((Model).state.tool_set.($choice.list))[$choice.index].selected} "$Command did not activate"
}
function Run([string]$Label){
 (Control 'drawing-canvas').SetFocus()
 [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),[uint16]0x4B)
 Wait-Until {Visible 'command-search'} 'Command search did not open' 10
 (Find 'command-search').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($Label)
 Wait-Until {$row=Visible 'command-result-0';$row -and $row.Current.Name.StartsWith($Label)} "Command search did not rank $Label first" 10
 [CapyRowPointer]::Key([uint32]$review.Id,0x0D)
 Wait-Until {!(Visible 'command-search')} 'Command search did not close' 5
}
function Shortcut([uint16[]]$Modifiers,[uint16]$Key){(Control 'drawing-canvas').SetFocus();[CapyRowPointer]::Chord([uint32]$review.Id,$Modifiers,$Key)}
function Undo{Wait-Until {(Command 'undo').enabled} 'Undo did not become available' 60;Shortcut @(0x11) 0x5A}
function Width{[int]@((Model).state.tabs)[0].width}
function Check-Mixing{
 Run 'Smudge'
 Wait-Until {(Command 'color_mix_linear').enabled} 'The Smudge brush did not offer color mixing'
 $null=Reveal 'tool-action-group-color-mixing' -Focus
 Compact-Choice 'tool-action-group-color-mixing' ([CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.)|Out-Null
 Pick-ToolAction 'color-mixing' 'color_mix_linear' 'pen'
 Wait-Until {(Command 'color_mix_linear').selected -and !(Command 'color_mix_oklab').selected} 'Linear mixing did not take effect from Tool Settings'
 Pick-ToolAction 'color-mixing' 'color_mix_oklab' 'touch'
 Wait-Until {(Command 'color_mix_oklab').selected -and !(Command 'color_mix_linear').selected} 'OKLab mixing did not take effect from Tool Settings'
 Capture ('mixing-'+$Theme) -WithModel
 $checks.color_mixing_tool_settings='passed'
}
function Footprint-Corners{
 $corners=@()
 foreach($index in @(0,2,8,6)){
  Invoke-Id ("tool-choice-transform-reference-$index")
  Wait-Until {@((Model).state.tool_extra|Where-Object {$_.Choice.id -eq 'transform-reference'})[0].Choice.items[$index].selected} 'The native position anchor did not select'
  $corners+=,@([double](Value 'transform_x'),[double](Value 'transform_y'))
 }
 return ,$corners
}
function Footprint-Pixels{
 $at=Center (Control 'settings-button' -Arranged);[CapyRowPointer]::Hover($at[0],$at[1]);Start-Sleep -Milliseconds 150
 $bounds=(Control 'drawing-canvas' -Arranged).Current.BoundingRectangle
 $points=@(foreach($point in @(@(100,100),@(1540,1230))){,(Screen-Point $point)})
 foreach($point in $points){if(!$bounds.Contains([Windows.Point]::new($point[0],$point[1]))){throw 'A Distort sample lies outside the arranged canvas'}}
 Sample-ObjectArtwork $bounds $points
}
function Footprint-Checkpoint([string]$Name){
 Idle
 $path=Join-Path $run ("distort-$Name.capy");Save-ProjectAs $path
 $manifest=Package $path
 [pscustomobject]@{path=$path;authored=(Json @($manifest.objects|Sort-Object id));raster=(Json @($manifest.objects|Where-Object type -in @('capy.paint-source/2','capy.coverage-source/2')|Sort-Object id))}
}
function Footprint-History([uint16]$Key,$Expected,[string]$Pixels,[string]$Name){
 Shortcut @(0x11) $Key;Idle
 $restored=Footprint-Checkpoint $Name
 if($restored.authored -cne $Expected.authored -or $restored.raster -cne $Expected.raster){throw "$Name did not restore exact authored and raster identities"}
 if((Wait-StablePixels {Footprint-Pixels}) -cne $Pixels){throw "$Name did not restore the exact composed pixels"}
}
function Footprint-Endpoint{
 $camera=(Model).state.camera;$bounds=(Control 'drawing-canvas' -Arranged).Current.BoundingRectangle
 if($camera.rotation -ne 0 -or $camera.flipped[0] -or $camera.flipped[1]){throw 'Near-line Distort requires the fitted unrotated view'}
 $scale=$camera.zoom*$bounds.Width/$camera.viewport[0]
 if($scale -lt .5){throw 'Near-line Distort needs at least one physical pixel for two source pixels'}
 $requested=@(1022.9555,1022.9555);$center=Screen-Point $requested;$candidates=@()
 foreach($dx in -1..1){foreach($dy in -1..1){
  $screen=@(($center[0]+$dx),($center[1]+$dy))
  $source=@((($screen[0]-$bounds.X)*$camera.viewport[0]/$bounds.Width-$camera.translation[0])/$camera.zoom,(($screen[1]-$bounds.Y)*$camera.viewport[1]/$bounds.Height-$camera.translation[1])/$camera.zoom)
  $sum=$source[0]+$source[1]
  if($sum -gt 2043.1 -and $sum -lt 2047.9 -and [Math]::Abs($source[0]-$requested[0]) -lt 3 -and [Math]::Abs($source[1]-$requested[1]) -lt 3 -and $bounds.Contains([Windows.Point]::new($screen[0],$screen[1]))){
   $candidates+=@{screen=$screen;source=$source;distance=([Math]::Pow($source[0]-$requested[0],2)+[Math]::Pow($source[1]-$requested[1],2))}
  }
 }}
 if(!$candidates.Count){throw 'The native pixel grid has no nonfolding near-line Distort endpoint'}
 $chosen=@($candidates|Sort-Object { $_.distance })[0]
 [pscustomobject]@{requested=$requested;screen=$chosen.screen;actual_source=$chosen.source;camera=$camera;canvas=@($bounds.X,$bounds.Y,$bounds.Width,$bounds.Height);physical_pixels_per_source_pixel=$scale}
}
function Check-TransformFootprint{
 Add-Type -AssemblyName System.Drawing
 Add-Type -Path (Join-Path $PSScriptRoot 'PackageFixture.cs') -ReferencedAssemblies @(
  'System.IO.Compression.dll','System.IO.Compression.FileSystem.dll'
  if($PSVersionTable.PSEdition -eq 'Core'){'System.Collections.dll';'System.IO.Compression.ZipFile.dll'}
 )
 . (Join-Path $PSScriptRoot 'ObjectFixture.ps1')
 $source=Join-Path $run 'Dense Distort 2048.png';$bitmap=[Drawing.Bitmap]::new(2048,2048,[Drawing.Imaging.PixelFormat]::Format32bppArgb)
 try{
  $graphics=[Drawing.Graphics]::FromImage($bitmap)
  try{foreach($y in 0..31){foreach($x in 0..31){
   $brush=[Drawing.SolidBrush]::new([Drawing.Color]::FromArgb(255,(32+(($x*37+$y*17)%192)),(32+(($x*19+$y*53)%192)),(32+(($x*71+$y*11)%192))))
   try{$graphics.FillRectangle($brush,$x*64,$y*64,64,64)}finally{$brush.Dispose()}
  }}}finally{$graphics.Dispose()}
  $bitmap.Save($source,[Drawing.Imaging.ImageFormat]::Png)
 }finally{$bitmap.Dispose()}
 $reports=@()
 try{foreach($filter in @('transform_bilinear','transform_bicubic')){
  Open-Project $source;Idle;Fit-Canvas;Idle
  if((Width) -ne 2048 -or [int]@((Model).state.tabs|Where-Object active)[0].height -ne 2048){throw 'The dense Distort source did not open at its exact extent'}
  $original=Footprint-Checkpoint ($filter+'-original');$originalPixels=Wait-StablePixels {Footprint-Pixels}
  foreach($value in $originalPixels.Split(',')){$color=[Drawing.Color]::FromArgb([int]$value);if($color.A -ne 255 -or $color.R -lt 25 -or $color.R -gt 230 -or $color.G -lt 25 -or $color.G -gt 230 -or $color.B -lt 25 -or $color.B -gt 230){throw 'Actual canvas samples did not display the dense opaque photo'}}
  Run (Command 'scale_rotate').label;Wait-Until {(Kind) -eq 'transform'} 'Transform did not start for the dense photo'
  Run (Command 'transform_distort').label;Run (Command $filter).label
  if((Command 'transform_snapping').selected){Run (Command 'transform_snapping').label}
  $initial=Footprint-Corners
  $expected=@(@(0,0),@(2048,0),@(2048,2048),@(0,2048))
  foreach($corner in 0..3){foreach($axis in 0..1){if($initial[$corner][$axis] -ne $expected[$corner][$axis]){throw 'The dense source did not start at all four exact corners'}}}
  $endpoint=Footprint-Endpoint;$last=@{corners=$null}
  Drag 'pen' (Screen-Point @(0,0)) $endpoint.screen {$last.corners=Footprint-Corners}
  $notice=(Model).state.notice;$accepted=Footprint-Corners
  $report=[ordered]@{filter=$filter;endpoint=$endpoint;initial=$initial;last_valid=$last.corners;accepted=$accepted;notice=$notice;completed=$false};$reports+=$report
  if((Model).state.host_error -or (Kind) -ne 'transform' -or !(Control 'drawing-canvas').Current.IsEnabled){throw 'The near-line drag stopped the native canvas'}
  if((Json $accepted) -cne (Json $last.corners)){throw 'The refused near-line pose did not retain all four exact last-valid corners'}
  if($notice.text -cne "The pixels can't be squeezed this far"){throw 'The near-line drag did not publish the shared squeeze refusal'}
  Capture ($filter+'-refused') -WithModel -Composed
  Drag 'pen' (Screen-Point $accepted[0]) (Screen-Point @(512,512));Idle
  $back=Footprint-Corners;$report.backed_out=$back
  $tolerance=1.5/$endpoint.physical_pixels_per_source_pixel
  if([Math]::Abs($back[0][0]-512) -gt $tolerance -or [Math]::Abs($back[0][1]-512) -gt $tolerance){throw 'A native pointer drag did not back out to the valid corner'}
  Tap 'canvas-bar-apply_transform' 'touch';Wait-Until {(Kind) -ne 'transform'} 'The backed-out Distort did not Apply';Idle
  $applied=Footprint-Checkpoint ($filter+'-applied');$appliedPixels=Wait-StablePixels {Footprint-Pixels}
  if($applied.authored -ceq $original.authored -or $appliedPixels -ceq $originalPixels){throw 'Applying the backed-out Distort did not change artwork and canvas pixels'}
  Footprint-History 0x5A $original $originalPixels ($filter+'-undo')
  Footprint-History 0x59 $applied $appliedPixels ($filter+'-redo')
  Run 'Pencil';$null=Reveal 'tool-setting-size' -Focus;(Control 'tool-setting-size').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('24');(Control 'drawing-canvas').SetFocus()
  Wait-Until {(Value 'size') -eq 24} 'The follow-up Pencil size did not commit'
  Drag 'pen' (Screen-Point @(1500,1200)) (Screen-Point @(1580,1260));Idle
  $painted=Footprint-Checkpoint ($filter+'-painted');$paintedPixels=Wait-StablePixels {Footprint-Pixels}
  if($painted.raster -ceq $applied.raster -or $paintedPixels -ceq $appliedPixels){throw 'The next native paint stroke did not publish changed raster pixels'}
  Footprint-History 0x5A $applied $appliedPixels ($filter+'-paint-undo')
  Footprint-History 0x59 $painted $paintedPixels ($filter+'-paint-redo')
  $saved=Footprint-Checkpoint ($filter+'-saved');$epoch=(Model).state.document_file.epoch
  & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'File';Invoke-Id 'close_document'
  Wait-Until {(Model).state.document_file.epoch -ne $epoch} 'The saved Distort drawing did not close'
  Open-Project $saved.path;Idle;Fit-Canvas;Idle
  $reopened=Footprint-Checkpoint ($filter+'-reopened')
  if($reopened.authored -cne $painted.authored -or $reopened.raster -cne $painted.raster -or (Wait-StablePixels {Footprint-Pixels}) -cne $paintedPixels){throw 'Save/reopen changed exact Distort and subsequent paint artwork or pixels'}
  $report.completed=$true;Capture ($filter+'-reopened') -WithModel -Composed
 }}finally{[IO.File]::WriteAllText((Join-Path $run 'transform-footprint.json'),([pscustomobject]@{theme=$Theme;source=$source;source_sha256=(Get-FileHash -LiteralPath $source).Hash;reports=$reports}|ConvertTo-Json -Depth 30),[Text.UTF8Encoding]::new($false))}
 $checks.distort_near_line_last_valid_backout_apply_paint_history_reopen='passed'
}

try {
 Enter-CapyEnvironment
 $env:CAPY_STORAGE_DIR=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1';$env:CAPY_SMOKE_TEST='1'
 [IO.File]::WriteAllText((Settings-File),(@{language=@{Explicit='en'};theme=$Theme}|ConvertTo-Json -Depth 4))
 $review=Start-Process -FilePath $Executable -WorkingDirectory $run -PassThru -RedirectStandardError (Join-Path $run 'stderr.log')
 $null=$review.Handle
 Write-Output "Owned photo tools review $($review.Id): $run"
 $native=@{window=$null}
 Wait-Until {$native.window=Owned-DrawingWindow $review;$null -ne $native.window} 'Review did not start' 45
 $root=$native.window.Root;$drawingWindow=$native.window.Handle
 Wait-Until {(Model).brush_ready -and (Model).windows_workspace.ready -and !(Model).windows_workspace.busy} 'Photo tools review did not start'
 $root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Maximized)
 [CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
 [CapyRowPointer]::SetForegroundWindow($drawingWindow)|Out-Null
 [CapyRowPointer]::Initialize([uint32]$review.Id)
 if($mixOnly){
  Check-Mixing
  [pscustomobject]$checks|ConvertTo-Json|Set-Content (Join-Path $run 'results.json')
  & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Close -DiscardUnsaved -StateDirectory $run
  return
 }
 Wait-Until {$b=(Find 'drawing-canvas').Current.BoundingRectangle;$b.Width -gt 1200} 'Maximized canvas did not settle' 10
 Fit-Canvas;Start-Sleep -Milliseconds 300
 (Find 'Test stroke' -Name).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
 Wait-Until {(Model).state.document_file.modified} 'Controlled drawing did not finish'
 Idle
 $width=Width;$height=[int]@((Model).state.tabs)[0].height

 Run 'Crop'
 Wait-Until {(Kind) -eq 'crop' -and (Bar) -and (Find 'canvas-bar-apply_transform')} 'Crop did not show its bar'
 if((Value 'crop_width') -ne $width -or (Value 'crop_height') -ne $height){throw 'Crop did not start at the canvas bounds'}
 $corner=Screen $width $height
 Drag 'pen' $corner @(($corner[0]-120),($corner[1]-90))
 Wait-Until {(Value 'crop_width') -lt $width -and (Value 'crop_height') -lt $height} 'A pen handle drag did not shrink the crop'
 Tap 'canvas-bar-choice-crop-ratio' 'mouse'
 Wait-Until {MenuOpen} 'Ratio did not open its menu' 5
 Pick @((Choice 'crop-ratio').items)[2].label
 Wait-Until {@((Choice 'crop-ratio').items)[2].selected -and [Math]::Abs((Value 'crop_width')-(Value 'crop_height')) -lt 1} 'The square ratio did not reach the crop'
 Capture 'crop'
 $cropped=[int](Value 'crop_width')
 Tap 'canvas-bar-apply_transform' 'touch'
 Wait-Until {(Kind) -ne 'crop' -and (Width) -eq $cropped} 'Apply on the bar did not crop the canvas' 20
 Idle
 Undo
 Wait-Until {(Width) -eq $width -and [int]@((Model).state.tabs)[0].height -eq $height} 'One Undo did not restore the canvas before the crop' 20
 Shortcut @(0x11) 0x59
 Wait-Until {(Width) -eq $cropped} 'Redo did not reapply the crop' 20
 Idle;Undo
 Wait-Until {(Width) -eq $width} 'Undo after Redo did not restore the canvas' 20
 Idle
 foreach($layer in @(Layers)){
  $label=Find "layer-$($layer.id)-label"
  if(!$label -or $label.Current.Name -ne $layer.label){throw "Layer row $($layer.id) lost its name after the crop"}
 }
 $checks.crop_handle_ratio_apply_one_undo='passed'

 $lasso=Tool 'lasso';Subtool 'rectangle_select'
 $center=Screen ($width/2) ($height/2)
 Drag 'mouse' @(($center[0]-260),($center[1]-200)) @(($center[0]+260),($center[1]+200))
 Wait-Until {(Model).state.layer_tools.has_selection -and (Kind) -eq 'selection'} 'The selection did not finish'
 Run (Command 'move').label
 Wait-Until {(Model).state.layer_tools.tool -eq 'move' -and (Kind) -eq 'selection' -and (Bar)} 'The selection bar did not stay under Move'
 $before=Paint;$layers=@(Layers).Count
 [CapyRowPointer]::Hover($center[0],$center[1]);Start-Sleep -Milliseconds 200
 Drag 'mouse' $center @(($center[0]+90),($center[1]+40))
 Idle
 Wait-Until {(Paint) -ne $before -and (Model).state.layer_tools.has_selection} 'Dragging with Move did not move the selected pixels'
 if(@(Layers).Count -ne $layers){throw 'Moving selected pixels changed the layer count'}
 Undo
 Wait-Until {(Paint) -eq $before} 'One Undo did not return the moved pixels'
 foreach($step in @(@('touch',$true),@('pen',$false))){
  Tap 'canvas-bar-more' $step[0]
  Wait-Until {MenuOpen} "More did not open from $($step[0])" 5
  Pick (Command 'move_leave_copy').label
  Wait-Until {[bool](Command 'move_leave_copy').selected -eq $step[1] -and !(MenuOpen)} "Leave Copy in More did not turn $(if($step[1]){'on'}else{'off'})"
 }
 Shortcut @(0x11) 0x44
 Wait-Until {!(Model).state.layer_tools.has_selection} 'Deselect did not clear the selection'
 $checks.move_selected_pixels_and_leave_copy='passed'

 Run 'Clone Stamp'
 Wait-Until {(Model).state.brush.tool -eq 'clone'} 'Clone Stamp did not become the brush'
 $source=Screen ($width*.3) ($height*.35)
 $button=Find $lasso;$button.SetFocus()
 Wait-Until {[System.Windows.Automation.AutomationElement]::FocusedElement.Current.AutomationId -eq $lasso} 'A toolbar button did not take focus'
 [CapyRowPointer]::Hover($source[0],$source[1]);Start-Sleep -Milliseconds 80
 [CapyRowPointer]::Hold(0x12,$true)
 try{
  Wait-Until {(Command 'clone_source_arm').selected} 'Holding Alt beside a focused button did not arm Set Source' 5
  Touch 'mouse' $source
 }finally{[CapyRowPointer]::Hold(0x12,$false)}
 Wait-Until {!(Command 'clone_source_arm').selected} 'Releasing Alt did not disarm Set Source' 5
 $before=Paint
 Touch 'mouse' $source
 Wait-Until {(Kind) -eq 'clone_source' -and (Bar)} 'Tapping the source set by Alt did not show the source bar'
 if((Paint) -ne $before){throw 'Tapping the clone source painted'}
 Tap 'canvas-bar-clone_flip_horizontal' 'touch'
 Wait-Until {(Command 'clone_flip_horizontal').selected} 'Flip Source did not turn on from the bar'
 Tap 'canvas-bar-clone_flip_horizontal' 'mouse'
 Wait-Until {!(Command 'clone_flip_horizontal').selected} 'Flip Source did not turn off from the bar'
 Capture 'clone-source'
 $stroke=Screen ($width*.65) ($height*.55)
 Drag 'pen' $stroke @(($stroke[0]+140),($stroke[1]+30))
 Idle
 Wait-Until {(Paint) -ne $before} 'A Clone Stamp stroke did not paint'
 $cloned=Paint
 Undo
 Wait-Until {(Paint) -eq $before} 'One Undo did not remove the clone stroke'
 $checks.clone_alt_source_beside_focused_button_bar_and_stroke='passed'

 foreach($brush in @(@('Healing Brush','heal'),@('Spot Healing Brush','spot_heal'))){
  Run $brush[0]
  Wait-Until {(Model).state.brush.tool -eq $brush[1]} "$($brush[0]) did not become the brush"
  $before=Paint
  $at=Screen ($width*.5) ($height*.6)
  Drag 'mouse' $at @(($at[0]+120),($at[1]-20))
  Idle
  Wait-Until {(Paint) -ne $before} "$($brush[0]) did not paint" 60
  Idle;Undo
  Wait-Until {(Paint) -eq $before} "One Undo did not remove the $($brush[0]) stroke"
 }
 $checks.healing_and_spot_healing_one_undo='passed'

 Check-Mixing

 $null=Tool 'brush'
 Shortcut @(0x11) 0x54
 Wait-Until {(Model).state.layer_tools.tool -eq 'transform' -and (Bar)} 'Ctrl+T did not start a transform' 10
 $modeId=@((Model).state.canvas_bar.items|Where-Object {$_.option.Choice.segmented})[0].option.Choice.id
 $warp=[array]::IndexOf(@((Choice $modeId).items|ForEach-Object label),'Warp')
 Tap ("canvas-bar-choice-$modeId-$warp") 'touch'
 Wait-Until {@((Choice $modeId).items)[$warp].selected -and (Visible 'canvas-bar-menu-warp_grid')} 'Warp did not offer its Grid menu' 10
 $grid=Find 'canvas-bar-menu-warp_grid'
 $text=$grid.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::Text))
 if(!$text -or !$text.Current.Name){throw 'The Warp Grid menu does not show its grid'}
 Tap 'canvas-bar-menu-warp_grid' 'mouse'
 Wait-Until {MenuOpen} 'The Warp Grid menu did not open' 5
 Pick (Command 'warp_split_vertical').label
 Wait-Until {(Command 'warp_split_vertical').selected} 'Split Vertically did not arm'
 Capture 'warp-split'
 (Control 'drawing-canvas').SetFocus();[CapyRowPointer]::Key([uint32]$review.Id,0x1B)
 Wait-Until {!(Command 'warp_split_vertical').selected -and (Model).state.layer_tools.tool -eq 'transform'} 'Escape did not disarm the split and keep the Warp' 5
 Tap 'canvas-bar-cancel_transform' 'touch'
 Wait-Until {(Model).state.layer_tools.tool -ne 'transform'} 'Cancel did not end the Warp transform' 5
 $checks.warp_split_choice='passed'

 $count=@(Layers).Count
 Invoke-Id 'layer-new'
 Wait-Until {@(Layers).Count -eq $count+1} 'A new layer was not added'
 (Find 'Test stroke' -Name).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
 Idle
 $stacked=@(Layers).Count;$order=(@(Layers)|ForEach-Object id) -join ','
 Wait-Until {(Command 'merge_down').enabled} 'Merge Down was not enabled'
 Shortcut @(0x11) 0x45
 Wait-Until {@(Layers).Count -eq $stacked-1} 'Ctrl+E did not merge down'
 Idle
 Undo
 Wait-Until {((@(Layers)|ForEach-Object id) -join ',') -eq $order} 'One Undo did not restore the merged layers'
 Run 'Stamp Visible'
 Wait-Until {@(Layers).Count -eq $stacked+1} 'Stamp Visible did not add a layer'
 Idle
 Undo
 Wait-Until {((@(Layers)|ForEach-Object id) -join ',') -eq $order} 'One Undo did not remove the stamped layer'
 $hidden=@(Layers)[0].id
 Invoke-Id "layer-$hidden-visibility"
 Wait-Until {!(@(Layers)|Where-Object id -eq $hidden).visible} 'The top layer did not hide'
 Run 'Flatten Image'
 Wait-Until {(Visible 'canvas-notice') -and (Visible 'canvas-notice-action-flatten')} 'Flatten did not ask before discarding the hidden layer'
 if(@(Layers).Count -ne $stacked){throw 'Flatten changed layers before it was confirmed'}
 Tap 'canvas-notice-action-flatten' 'touch'
 Wait-Until {@(Layers).Count -lt $stacked} 'Confirming Flatten did not flatten'
 Idle
 Undo
 Wait-Until {((@(Layers)|ForEach-Object id) -join ',') -eq $order} 'One Undo did not restore the flattened layers'
 $checks.merge_down_stamp_visible_and_flatten_notice='passed'

 Check-TransformFootprint

 [CapyRowPointer]::Dispose()
 & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Close -DiscardUnsaved -StateDirectory $run
 if((Get-Item (Join-Path $run 'stderr.log')).Length){throw 'Native stderr requires review'}
 $checks.evidence=$run
 [pscustomobject]$checks|ConvertTo-Json|Tee-Object -FilePath (Join-Path $run 'results.json')
} catch {
 if($review -and !$review.HasExited){try{Capture 'failure' -WithModel}catch{}}
 [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
} finally {
 [CapyRowPointer]::Dispose()
 if($review -and !$review.HasExited){Stop-Process -Id $review.Id -Force}
 Exit-CapyEnvironment
}
