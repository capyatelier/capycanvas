param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyWaitSeconds=45
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$run=Join-Path $repo ('artifacts/windows/photo-tools/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$CapyTraceDirectory=$run
$checks=[ordered]@{}
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
function Reveal([string]$Id){
 $walker=[System.Windows.Automation.TreeWalker]::ControlViewWalker
 $node=$walker.GetParent((Control $Id));$scroll=$null
 while($node -and !$scroll){
  $pattern=$null
  if($node.TryGetCurrentPattern([System.Windows.Automation.ScrollPattern]::Pattern,[ref]$pattern) -and $pattern.Current.VerticallyScrollable){$scroll=$pattern}
  $node=$walker.GetParent($node)
 }
 if(!$scroll){throw "$Id has no scrolling ancestor"}
 Wait-Until {
  $item=Find $Id
  if($item -and !$item.Current.IsOffscreen){return $true}
  $scroll.Scroll([System.Windows.Automation.ScrollAmount]::NoAmount,[System.Windows.Automation.ScrollAmount]::LargeIncrement);Start-Sleep -Milliseconds 200;$false
 } "$Id did not scroll into view" 10
}
function Touch([string]$Device,[int[]]$At){[CapyRowPointer]::Down($Device,$At[0],$At[1]);Start-Sleep -Milliseconds 40;[CapyRowPointer]::Up();Start-Sleep -Milliseconds 150}
function Drag([string]$Device,[int[]]$From,[int[]]$To){
 [CapyRowPointer]::Down($Device,$From[0],$From[1])
 for($step=1;$step -le 12;$step++){[CapyRowPointer]::Move([int]($From[0]+($To[0]-$From[0])*$step/12),[int]($From[1]+($To[1]-$From[1])*$step/12));Start-Sleep -Milliseconds 12}
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
try {
 Enter-CapyEnvironment
 $env:CAPY_STORAGE_DIR=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1';$env:CAPY_SMOKE_TEST='1'
 $review=Start-Process -FilePath $Executable -WorkingDirectory $run -PassThru -RedirectStandardError (Join-Path $run 'stderr.log')
 $null=$review.Handle
 Write-Output "Owned photo tools review $($review.Id): $run"
 $watch=[Diagnostics.Stopwatch]::StartNew()
 do{$review.Refresh();if($review.HasExited){throw 'Review exited at startup'};Start-Sleep -Milliseconds 100}while($review.MainWindowHandle -eq [IntPtr]::Zero -and $watch.Elapsed.TotalSeconds -lt 45)
 $root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
 Wait-Until {(Model).brush_ready -and (Model).windows_workspace.ready -and !(Model).windows_workspace.busy} 'Photo tools review did not start'
 $root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Maximized)
 [CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
 [CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)|Out-Null
 [CapyRowPointer]::Initialize([uint32]$review.Id)
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
 $null=Tool 'move'
 Wait-Until {(Model).state.layer_tools.tool -eq 'move' -and (Find 'canvas-bar-move_leave_copy')} 'Leave Copy did not join the selection bar under Move'
 $before=Paint;$layers=@(Layers).Count
 [CapyRowPointer]::Hover($center[0],$center[1]);Start-Sleep -Milliseconds 200
 Drag 'mouse' $center @(($center[0]+90),($center[1]+40))
 Idle
 Wait-Until {(Paint) -ne $before -and (Model).state.layer_tools.has_selection} 'Dragging with Move did not move the selected pixels'
 if(@(Layers).Count -ne $layers){throw 'Moving selected pixels changed the layer count'}
 Undo
 Wait-Until {(Paint) -eq $before} 'One Undo did not return the moved pixels'
 Tap 'canvas-bar-move_leave_copy' 'touch'
 Wait-Until {(Command 'move_leave_copy').selected} 'Leave Copy did not turn on from the bar'
 Tap 'canvas-bar-move_leave_copy' 'pen'
 Wait-Until {!(Command 'move_leave_copy').selected} 'Leave Copy did not turn off from the bar'
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

 Run 'Smudge'
 Wait-Until {(Command 'color_mix_linear').enabled} 'The Smudge brush did not offer color mixing'
 Reveal 'tool-action-color_mix_linear'
 Tap 'tool-action-color_mix_linear' 'pen'
 Wait-Until {(Command 'color_mix_linear').selected -and !(Command 'color_mix_oklab').selected} 'Linear mixing did not take effect from Tool Settings'
 Tap 'tool-action-color_mix_oklab' 'touch'
 Wait-Until {(Command 'color_mix_oklab').selected -and !(Command 'color_mix_linear').selected} 'OKLab mixing did not take effect from Tool Settings'
 $checks.color_mixing_tool_settings='passed'

 $null=Tool 'brush'
 Shortcut @(0x11) 0x54
 Wait-Until {(Model).state.layer_tools.tool -eq 'transform' -and (Bar)} 'Ctrl+T did not start a transform' 10
 $modeId=@((Model).state.canvas_bar.items|Where-Object {$_.option.Choice.segmented})[0].option.Choice.id
 $warp=[array]::IndexOf(@((Choice $modeId).items|ForEach-Object label),'Warp')
 Tap ("canvas-bar-choice-$modeId-$warp") 'touch'
 Wait-Until {@((Choice $modeId).items)[$warp].selected -and (Visible 'canvas-bar-choice-transform-warp-split')} 'Warp did not offer its split choice' 10
 $split=Find 'canvas-bar-choice-transform-warp-split'
 $text=$split.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::Text))
 if(!$text -or !$text.Current.Name){throw 'The Warp split choice is blank'}
 Tap 'canvas-bar-choice-transform-warp-split' 'mouse'
 Wait-Until {MenuOpen} 'The Warp split choice did not open its menu' 5
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

 [CapyRowPointer]::Dispose()
 & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved -StateDirectory $run
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
