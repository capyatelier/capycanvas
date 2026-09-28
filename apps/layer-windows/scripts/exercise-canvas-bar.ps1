param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyWaitSeconds=45
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$run=Join-Path $repo ('artifacts/windows/canvas-bar/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$CapyTraceDirectory=$run
$checks=[ordered]@{}
function Owned([string]$Id,[switch]$Name){
 $property=if($Name){[System.Windows.Automation.AutomationElement]::NameProperty}else{[System.Windows.Automation.AutomationElement]::AutomationIdProperty}
 $condition=[System.Windows.Automation.AndCondition]::new([System.Windows.Automation.PropertyCondition]::new($property,$Id),
  [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty,$review.Id))
 [System.Windows.Automation.AutomationElement]::RootElement.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$condition)
}
function Bar {$bar=Find 'canvas-action-bar';if($bar -and !$bar.Current.IsOffscreen){$bar}}
function Kind {(Model).state.canvas_bar.context.kind}
function Paint {@((Model).state.layers|ForEach-Object {"$($_.id):$($_.paint_revision)"}) -join ','}
function Value([string]$Id){((Model).state.tool_settings|Where-Object id -eq $Id).value}
function Glass {[int](Model).windows_glass.regions}
function Choice([string]$Id){@((Model).state.canvas_bar.items|Where-Object {$_.option.Choice.id -eq $Id})[0].option.Choice}
function MenuOpen{
 $condition=[System.Windows.Automation.AndCondition]::new(
  [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty,$review.Id),
  [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::MenuItem))
 $null -ne [System.Windows.Automation.AutomationElement]::RootElement.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$condition)
}
function AnchorCenter{$m=Model;$a=$m.state.canvas_bar.anchor;$c=$m.state.camera;$r=(Find 'Drawing canvas' -Name).Current.BoundingRectangle
 @([int]($r.X+$c.translation[0]+$c.zoom*($a[0]+$a[2])/2),[int]($r.Y+$c.translation[1]+$c.zoom*($a[1]+$a[3])/2))}
function Center($Element){$r=$Element.Current.BoundingRectangle;@([int]($r.X+$r.Width/2),[int]($r.Y+$r.Height/2))}
function Tap([string]$Id,[string]$Device){
 $item=@{value=$null};Wait-Until {$item.value=Find $Id;$item.value -and !$item.value.Current.IsOffscreen -and $item.value.Current.IsEnabled} "Missing bar control $Id" 10
 $at=Center (Control $Id -Arranged);[CapyRowPointer]::Down($Device,$at[0],$at[1]);Start-Sleep -Milliseconds 40;[CapyRowPointer]::Up()
}
function Tool([string]$Command){
 $target=@{id=$null};Wait-Until {foreach($panel in (Model).panels){foreach($tile in $panel.tiles){if($tile.control.command -eq $Command){$target.id="tile-$($panel.id)-$($tile.id)";return $true}}};$false} "No $Command tile"
 Invoke-Id $target.id
}
function Subtool([string]$Command){
 $index=@{value=-1}
 Wait-Until {$subtools=@((Model).state.tool_set.subtools);for($i=0;$i -lt $subtools.Count;$i++){if($subtools[$i].action.command -eq $Command){$index.value=$i;return $true}};$false} "No $Command subtool"
 Invoke-Id ('tool-subtool-'+$index.value)
 Wait-Until {@((Model).state.tool_set.subtools)[$index.value].selected} "$Command did not activate"
}
function Drag([string]$Device,[int[]]$From,[int[]]$To){
 [CapyRowPointer]::Down($Device,$From[0],$From[1])
 for($step=1;$step -le 12;$step++){[CapyRowPointer]::Move([int]($From[0]+($To[0]-$From[0])*$step/12),[int]($From[1]+($To[1]-$From[1])*$step/12));Start-Sleep -Milliseconds 12}
 [CapyRowPointer]::Up();Start-Sleep -Milliseconds 150
}
try {
 Enter-CapyEnvironment
 $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1';$env:CAPY_SMOKE_TEST='1'
 $review=Start-Process -FilePath $Executable -WorkingDirectory $run -PassThru -RedirectStandardError (Join-Path $run 'stderr.log')
 $null=$review.Handle
 Write-Output "Owned canvas bar review $($review.Id): $run"
 $watch=[Diagnostics.Stopwatch]::StartNew()
 do{$review.Refresh();if($review.HasExited){throw 'Review exited at startup'};Start-Sleep -Milliseconds 100}while($review.MainWindowHandle -eq [IntPtr]::Zero -and $watch.Elapsed.TotalSeconds -lt 45)
 $root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
 Wait-Until {(Model).brush_ready -and (Model).windows_workspace.ready -and !(Model).windows_workspace.busy} 'Canvas bar review did not start'
 $root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Maximized)
 [CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
 [CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)|Out-Null
 [CapyRowPointer]::Initialize([uint32]$review.Id)
 Wait-Until {$b=(Find 'Drawing canvas' -Name).Current.BoundingRectangle;$b.Width -gt 1200} 'Maximized canvas did not settle' 10
 Invoke-Id 'canvas-fit';Start-Sleep -Milliseconds 300
 (Find 'Test stroke' -Name).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
 Wait-Until {(Model).state.document_file.modified} 'Controlled drawing did not finish'
 $canvas=(Find 'Drawing canvas' -Name).Current.BoundingRectangle
 $cx=[int]($canvas.X+$canvas.Width*.5);$cy=[int]($canvas.Y+$canvas.Height*.45)
 if(Bar){throw 'The bar showed before any selection or transform'}

 Tool 'lasso';Subtool 'rectangle_select'
 $glass=@{count=-1;stable=0}
 Wait-Until {$count=Glass;if($count -eq $glass.count){$glass.stable++}else{$glass.count=$count;$glass.stable=0};$glass.stable -ge 3} 'Selection tool glass did not settle'
 $glassBefore=$glass.count
 $selection=@(@(($cx-160),($cy-110)),@(($cx+160),($cy+60)))
 Drag 'mouse' $selection[0] $selection[1]
 Wait-Until {(Model).state.layer_tools.has_selection -and (Kind) -eq 'selection' -and (Bar)} 'The selection bar did not appear' 10
 $bar=(Bar).Current.BoundingRectangle
 if($bar.Top -lt $selection[1][1]){throw "The selection bar is not below the selection: $($bar.Top) < $($selection[1][1])"}
 if([Math]::Abs(($bar.X+$bar.Width/2)-$cx) -gt 40){throw 'The selection bar is not centred under the selection'}
 Wait-Until {(Glass) -ge $glassBefore+1} 'The bar did not register its glass region' 5
 Capture 'selection-dark'
 $checks.selection_below_object_with_glass='passed'

 $paint=Paint
 foreach($device in @('mouse','touch','pen')){
  Tap 'canvas-bar-invert_selection' $device
  Wait-Until {(Model).state.canvas_bar.placement -eq 'bottom_edge'} "$device Invert did not move the bar to the bottom edge" 5
  Tap 'canvas-bar-invert_selection' $device
  Wait-Until {(Model).state.canvas_bar.placement -eq 'near_object'} "$device second Invert did not restore the selection" 5
 }
 if((Paint) -ne $paint){throw 'Bar taps painted on the canvas'}
 $checks.mouse_touch_pen_taps_do_not_paint='passed'

 foreach($device in @('mouse','touch','pen')){
  Tap 'canvas-bar-menu-refine' $device
  Wait-Until {MenuOpen} "$device did not open the Refine menu" 5
  [CapyRowPointer]::Key([uint32]$review.Id,0x1B)
  Wait-Until {!(MenuOpen)} "Escape did not close the Refine menu" 5
 }
 $layers=@((Model).state.layers).Count
 Tap 'canvas-bar-menu-copy_to_layer' 'touch'
 Wait-Until {$null -ne (Owned 'Copy Selection to New Layer' -Name)} 'The Copy to Layer menu has no Copy Selection to New Layer' 5
 (Owned 'Copy Selection to New Layer' -Name).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
 Wait-Until {@((Model).state.layers).Count -eq $layers+1 -and !(MenuOpen)} 'Copy Selection to New Layer did not add a layer through the bar menu' 5
 (Find 'Drawing canvas' -Name).SetFocus()
 [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),[uint16]0x5A)
 Wait-Until {@((Model).state.layers).Count -eq $layers -and (Paint) -eq $paint} 'One Undo did not remove the copied layer' 5
 $checks.selection_menus_copy_to_layer_one_undo_step='passed'

 Tap 'canvas-bar-scale_rotate' 'mouse'
 Wait-Until {(Kind) -eq 'transform' -and (Bar) -and (Find 'canvas-bar-apply_transform') -and (Find 'canvas-bar-cancel_transform')} 'Transform from the bar did not open the transform bar' 10
 $width=Value 'transform_width'
 foreach($device in @('touch','pen','mouse')){
  $before=Value 'transform_width'
  Tap 'canvas-bar-transform_flip_horizontal' $device
  Wait-Until {[Math]::Sign((Value 'transform_width')) -eq -[Math]::Sign($before)} "$device Flip H did not flip the transform" 5
 }
 if((Kind) -ne 'transform'){throw 'Flipping left the transform'}
 $choices=@((Model).state.canvas_bar.items|Where-Object {$_.option.Choice}|ForEach-Object {$_.option.Choice})
 $mode=@($choices|Where-Object segmented)[0]
 if(!$mode){throw 'The transform bar has no segmented mode choice'}
 Tap ("canvas-bar-choice-"+$mode.id+"-1") 'touch'
 Wait-Until {@((Choice $mode.id).items)[1].selected} 'Touch did not select the second transform mode' 5
 Tap ("canvas-bar-choice-"+$mode.id+"-0") 'pen'
 Wait-Until {@((Choice $mode.id).items)[0].selected} 'Pen did not restore the first transform mode' 5
 foreach($dropdown in @($choices|Where-Object {!$_.segmented})){
  $control=Find ("canvas-bar-choice-"+$dropdown.id)
  if(!$control -or $control.Current.IsOffscreen){continue}
  Tap ("canvas-bar-choice-"+$dropdown.id) 'mouse'
  Wait-Until {MenuOpen} "The $($dropdown.label) choice did not open its menu" 5
  [CapyRowPointer]::Key([uint32]$review.Id,0x1B)
  Wait-Until {!(MenuOpen)} "Escape did not close the $($dropdown.label) menu" 5
 }
 $checks.transform_mode_choices='passed'
 $x=Value 'transform_x'
 $inside=AnchorCenter
 Drag 'touch' $inside @(($inside[0]+80),$inside[1])
 Wait-Until {[Math]::Abs((Value 'transform_x')-$x) -gt 1 -and (Bar)} 'A finger drag did not move the transform body' 5
 if((Kind) -ne 'transform'){throw 'The finger drag ended the transform'}
 $checks.finger_moves_transform='passed'
 if((Paint) -ne $paint){throw 'Transform bar taps painted on the canvas'}
 Capture 'transform-dark'
 $checks.transform_bar_and_flip='passed'

 [CapyRowPointer]::Down('pen',$cx,($cy-25))
 for($step=1;$step -le 8;$step++){[CapyRowPointer]::Move($cx+10*$step,$cy-25);Start-Sleep -Milliseconds 15}
 Wait-Until {!(Bar)} 'The bar stayed visible during a canvas contact' 3
 [CapyRowPointer]::Up()
 Wait-Until {(Bar)} 'The bar did not return after the contact' 5
 if((Kind) -ne 'transform'){throw 'The contact ended the transform'}
 $checks.hides_during_contact_and_returns='passed'

 Tap 'canvas-bar-more' 'mouse'
 Wait-Until {$null -ne (Owned 'show_canvas_action_bar')} 'More did not open the bar menu' 5
 [CapyRowPointer]::Key([uint32]$review.Id,0x1B)
 Wait-Until {$null -eq (Owned 'show_canvas_action_bar')} 'Escape did not close the bar menu' 5
 if((Kind) -ne 'transform'){throw 'Closing More cancelled the transform'}
 $checks.more_menu='passed'

 $revision=(Model).state.document_file.revision
 Tap 'canvas-bar-apply_transform' 'touch'
 Wait-Until {(Kind) -ne 'transform' -and (Model).state.document_file.revision -ne $revision} 'Apply on the bar did not commit the transform' 10
 $applied=Paint
 (Find 'Drawing canvas' -Name).SetFocus()
 [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),[uint16]0x5A)
 Wait-Until {(Paint) -eq $paint} 'One Undo did not restore the pixels before the transform' 5
 [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),[uint16]0x59)
 Wait-Until {(Paint) -ne $paint -and !((Model).state.commands|Where-Object id -eq 'redo').enabled} 'Redo did not reapply the transform' 5
 $checks.apply_one_undo_step='passed'

 $before=Paint;$revision=(Model).state.document_file.revision
 (Find 'Drawing canvas' -Name).SetFocus();[CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),[uint16]0x54)
 Wait-Until {(Kind) -eq 'transform' -and (Bar)} 'Ctrl+T did not start a transform' 10
 $modeId=@((Model).state.canvas_bar.items|Where-Object {$_.option.Choice.segmented})[0].option.Choice.id
 $warp=[array]::IndexOf(@((Choice $modeId).items|ForEach-Object label),'Warp')
 if($warp -lt 0){throw 'The transform mode choice has no Warp'}
 Tap ("canvas-bar-choice-$modeId-$warp") 'touch'
 Wait-Until {@((Choice $modeId).items)[$warp].selected -and @((Model).state.canvas_bar.items|Where-Object {$_.option.Choice -and !$_.option.Choice.segmented -and $_.option.Choice.label -eq 'Grid'}).Count} 'Warp did not offer its Grid choice' 5
 Capture 'warp-dark'
 (Find 'Drawing canvas' -Name).SetFocus();[CapyRowPointer]::Key([uint32]$review.Id,0x1B)
 Wait-Until {(Kind) -ne 'transform'} 'Escape did not cancel the Warp transform' 5
 if((Paint) -ne $before -or (Model).state.document_file.revision -ne $revision){throw 'Cancelling the transform changed the drawing'}
 $checks.warp_mode_and_escape_cancel='passed'

 (Find 'Drawing canvas' -Name).SetFocus();[CapyRowPointer]::Key([uint32]$review.Id,0x09)
 Wait-Until {(Model).chrome_hidden -or (Model).state.workspace.zen_mode} 'Tab did not enter Zen' 5
 Wait-Until {(Bar)} 'The bar did not stay visible in Zen' 5
 Capture 'zen'
 (Find 'Drawing canvas' -Name).SetFocus();[CapyRowPointer]::Key([uint32]$review.Id,0x09)
 Wait-Until {!(Model).state.workspace.zen_mode} 'Tab did not leave Zen' 5
 $checks.visible_in_zen='passed'

 Tool 'lasso';Subtool 'polygon_select'
 foreach($point in @(@(($cx-120),($cy-80)),@(($cx+120),($cy-80)),@(($cx+140),($cy+90)))){
  [CapyRowPointer]::Down('mouse',$point[0],$point[1]);Start-Sleep -Milliseconds 30;[CapyRowPointer]::Up();Start-Sleep -Milliseconds 120
 }
 Wait-Until {(Kind) -eq 'polygon' -and (Model).state.canvas_bar.placement -eq 'bottom_edge' -and (Bar)} 'The polygon bar did not appear at the bottom edge' 5
 Tap 'canvas-bar-remove_selection_point' 'touch'
 Wait-Until {(Kind) -eq 'polygon'} 'Remove Point ended the polygon' 3
 [CapyRowPointer]::Down('mouse',($cx-130),($cy+100));Start-Sleep -Milliseconds 30;[CapyRowPointer]::Up();Start-Sleep -Milliseconds 120
 Capture 'polygon-dark'
 Tap 'canvas-bar-complete_selection' 'pen'
 Wait-Until {(Kind) -eq 'selection' -and (Model).state.layer_tools.has_selection} 'Finish on the bar did not complete the polygon' 5
 $checks.polygon_bar='passed'

 & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'Edit'
 Invoke-Id 'settings'
 Wait-Until {$null -ne (Find 'Color theme' -Name)} 'Preferences did not open' 10
 $theme=$root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.AndCondition]::new(
  [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,'Color theme'),
  [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::ComboBox)))
 $theme.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
 Wait-Until {$null -ne (Owned 'Light' -Name)} 'Light theme choice missing' 5
 (Owned 'Light' -Name).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
 Wait-Until {(Model).state.theme -eq 'light'} 'The theme did not switch to light' 5
 (Find 'CloseButton').GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
 Wait-Until {!(Model).preferences} 'Preferences did not close' 5
 Wait-Until {(Bar)} 'The bar did not return after the theme change' 5
 Start-Sleep -Milliseconds 300
 Capture 'selection-light'
 $checks.light_theme='passed'

 [CapyRowPointer]::Dispose()
 & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved -StateDirectory $run
 if((Get-Item (Join-Path $run 'stderr.log')).Length){throw 'Native stderr requires review'}
 $checks.evidence=$run
 [pscustomobject]$checks|ConvertTo-Json|Tee-Object -FilePath (Join-Path $run 'results.json')
} catch {
 if($review -and !$review.HasExited){try{Capture 'failure'}catch{}}
 [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
} finally {
 [CapyRowPointer]::Dispose()
 if($review -and !$review.HasExited){Stop-Process -Id $review.Id -Force}
 Exit-CapyEnvironment
}
