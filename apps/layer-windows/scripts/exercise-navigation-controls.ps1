param([Parameter(Mandatory)][string]$Executable,[ValidateSet('dark','light')][string]$Theme='dark')
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
$CapyPopups=$true;$CapyFind='prefer-visible'
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$run=Join-Path $repo ('artifacts/windows/navigation-controls/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$CapyTraceDirectory=$run
$checks=[ordered]@{}
function Menu-Command([string]$Menu,[string]$Id){
 $caption=@((Model).application_menus|Where-Object id -eq $Menu)[0].label
 & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name $Menu -Caption $caption
 Invoke-Id $Id
}
function Choice([string]$Id){@((Model).state.tool_extra|ForEach-Object {$_.Choice}|Where-Object id -eq $Id)[0]}
function Choice-Context{
 $model=Model;$state=$model.state
 @{workspace=$model.windows_workspace;dialog=$state.dialog;requests=$state.host_requests;tools=@($state.tool_set.groups)+@($state.tool_set.subtools)|Where-Object selected|ForEach-Object action;preset=$state.brush.preset;target=$state.layer_tools.editing_layer;settings=$state.settings.zoom_tool;choices=$state.tool_extra}
}
function Pick([string]$Id,[int]$Index){
 Choice-Context|ConvertTo-Json -Depth 15|Set-Content (Join-Path $run ("$Id-$Index-before.json"))
 $box=(Control "tool-choice-$Id-$Index" -Arranged).Current.BoundingRectangle
 [CapyRowPointer]::Down('mouse',[int]($box.X+$box.Width/2),[int]($box.Y+$box.Height/2));Start-Sleep -Milliseconds 45;[CapyRowPointer]::Up()
 try{Wait-Until {(Choice $Id).items[$Index].selected} "Zoom choice $Id/$Index did not reach shared state"}finally{Choice-Context|ConvertTo-Json -Depth 15|Set-Content (Join-Path $run ("$Id-$Index-after.json"))}
 Wait-Until {(Control "tool-choice-$Id-$Index").Current.ItemStatus -ne ''} "Zoom choice $Id/$Index did not show selection"
}
function Canvas-Point{
 $box=(Control 'drawing-canvas' -Arranged).Current.BoundingRectangle
 @([int]($box.X+$box.Width*.6),[int]($box.Y+$box.Height*.55))
}
function Reset-Zoom{
 Invoke-Id 'canvas-view-info'
 Wait-Until {Zoom-Item 'zoom-actual_pixels'} 'Zoom menu did not open for reset'
 $item=Zoom-Item 'zoom-actual_pixels'
 $item.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
 Wait-Until {[Math]::Abs((Model).state.camera.zoom-1) -lt .001 -and !(Zoom-Item 'zoom-actual_pixels')} 'Actual Pixels did not reset zoom'
}
function Canvas-Tap([string]$Device){
 $at=Canvas-Point;[CapyRowPointer]::Down($Device,$at[0],$at[1]);Start-Sleep -Milliseconds 50;[CapyRowPointer]::Up()
}
function Drag([int]$Dx,[int]$Dy){
 $at=Canvas-Point;[CapyRowPointer]::Down('mouse',$at[0],$at[1]);[CapyRowPointer]::Move($at[0]+$Dx,$at[1]+$Dy);[CapyRowPointer]::Up()
}
function Wheel([int]$Modifier=0){
 $at=Canvas-Point;[CapyRowPointer]::Hover($at[0],$at[1]);$revision=(Model).state.camera.revision
 if($Modifier){[CapyRowPointer]::Hold($Modifier,$true)}
 try{
  [CapyRowPointer]::Wheel($at[0],$at[1],120)
  Wait-Until {(Model).state.camera.revision -gt $revision} 'Wheel input did not advance the camera'
 }finally{if($Modifier){[CapyRowPointer]::Hold($Modifier,$false)}}
}
function Preference([string]$Id){@((Model).preferences.pages.groups.rows|Where-Object id -eq $Id)[0]}
try{
 Enter-CapyEnvironment
 $env:CAPY_STORAGE_DIR=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1'
 [IO.File]::WriteAllText((Settings-File),(@{language=@{Explicit='en'};theme=$Theme}|ConvertTo-Json -Depth 4))
 $review=Start-Process -FilePath $Executable -WorkingDirectory $run -PassThru -RedirectStandardError (Join-Path $run 'stderr.log')
 $null=$review.Handle
 Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero -and (Model).brush_ready -and (Model).windows_workspace.ready} 'Navigation review did not start' 120
 $root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
 $root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Maximized)
 [CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
 [CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)|Out-Null
 [CapyRowPointer]::Initialize([uint32]$review.Id)
 $tiles=@((Model).panels|Where-Object id -eq 'toolbar')[0].tiles
 $zoom=@($tiles|Where-Object {$_.control.command -eq 'zoom' -or $_.resolved_control.command -eq 'zoom'})[0]
 $hand=@($tiles|Where-Object {$_.control.command -eq 'hand' -or $_.resolved_control.command -eq 'hand' -or $_.control.slot -eq 'navigation'})[0]
 if(!$zoom -or !$hand -or $zoom.id -eq $hand.id -or $zoom.has_variants -or !$hand.has_variants){throw 'Zoom is not separate from the grouped Hand/Rotate tools'}
 Invoke-Id "tile-toolbar-$($zoom.id)"
 Wait-Until {@((Model).state.commands|Where-Object {$_.id -eq 'zoom' -and $_.selected}).Count -eq 1} 'Zoom did not activate'
 if(!@((Model).layout.groups|Where-Object {$_.panels -contains 'tool_settings'}).Count){
  & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'Window'
  $menu=@((Model).application_menus|Where-Object id -eq 'window')[0].model.sections|ForEach-Object {$_}|Where-Object {$_.action.type -eq 'customize' -and $_.action.action.panel -eq 'tool_settings'}
  (Control $menu.label -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
  Wait-Until {@((Model).layout.groups|Where-Object {$_.panels -contains 'tool_settings'}).Count -gt 0} 'Tool panel did not open'
 }
 if(!@((Model).layout.groups|Where-Object active -eq 'tool_settings').Count){Invoke-Id 'panel-tab-tool_settings'}
 foreach($id in 'zoom-click','zoom-drag','zoom-direction'){
  $spec=Choice $id
  if(!$spec.labeled){throw "Zoom choice $id is missing its shared label"}
  $bar=Control "tool-choice-$id" -Arranged
  if($bar.Current.Name -ne $spec.label){throw "Zoom choice $id has the wrong accessible label"}
  $row=[System.Windows.Automation.TreeWalker]::ControlViewWalker.GetParent($bar)
  $caption=$row.FindFirst([System.Windows.Automation.TreeScope]::Children,[System.Windows.Automation.AndCondition]::new(
   [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::Text),
   [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,$spec.label)))
  if(!$caption -or $caption.Current.IsOffscreen -or $caption.Current.BoundingRectangle.Right -gt $bar.Current.BoundingRectangle.Left){throw "Zoom choice $id does not display its label to the left"}
  for($i=0;$i -lt $spec.items.Count;$i++){
   $button=Control "tool-choice-$id-$i" -Arranged
   $label=$spec.items[$i].label
   if($button.Current.Name -ne $label){throw "Zoom segment $id/$i has the wrong accessible label"}
   $text=$button.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,$label))
   if(!$text -or $text.Current.IsOffscreen){throw "Zoom segment $id/$i does not display its text"}
   Pick $id $i
  }
 }
 Pick 'zoom-click' 0;Pick 'zoom-drag' 2
 foreach($device in 'mouse','pen','touch'){
  Reset-Zoom
  $before=(Model).state.camera.zoom;Canvas-Tap $device
  Wait-Until {(Model).state.camera.zoom -gt $before} "$device Zoom In did not zoom"
 }
 Pick 'zoom-click' 1
 $before=(Model).state.camera.zoom;Canvas-Tap 'mouse'
 Wait-Until {(Model).state.camera.zoom -lt $before} 'Zoom Out did not zoom out'
 Pick 'zoom-click' 0;Pick 'zoom-drag' 0;Pick 'zoom-direction' 0
 Reset-Zoom
 $before=(Model).state.camera.zoom;Drag 80 0
 Wait-Until {(Model).state.camera.zoom -gt $before} 'Horizontal smooth drag did not zoom in'
 Pick 'zoom-direction' 1
 Reset-Zoom
 $before=(Model).state.camera.zoom;Drag 0 -80
 Wait-Until {(Model).state.camera.zoom -gt $before} 'Vertical smooth drag did not zoom in'
 Pick 'zoom-drag' 1
 Reset-Zoom
 $before=(Model).state.camera.zoom;Drag 120 100
 Wait-Until {[Math]::Abs((Model).state.camera.zoom-$before) -gt .001} 'Area drag did not zoom'
 $center=Control 'tool-action-center_zoom_clicks' -Arranged
 $center.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
 Wait-Until {(Model).state.settings.zoom_tool.center_clicked_point} 'Center clicked point did not reach shared settings'
 Capture "zoom-settings-$Theme" -WithModel -Composed
 $checks.zoom_controls='passed';$checks.zoom_pointer_journeys=@('mouse','pen','touch');$checks.smooth_and_area='passed'
 if(Find 'settings-button' -Visible){Invoke-Id 'settings-button'}else{Menu-Command 'edit' 'settings'}
 Invoke-Id 'preference-page-canvas'
 Wait-Until {(Model).preferences.page -eq 'canvas'} 'Canvas preferences did not open'
 $wheel=Preference 'wheel_behavior'
 if($wheel.kind.options.Count -ne 2){throw 'Mouse wheel preference is not a two-choice dropdown'}
 $combo=Control 'preference-choice-wheel_behavior' -Arranged
 $combo.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
 (Control $wheel.kind.options[1] -Name -Type ([System.Windows.Automation.ControlType]::ListItem)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
 Wait-Until {(Model).state.settings.wheel_zoom} 'Wheel Zoom preference did not reach shared settings'
 $rotation=Preference 'rotate_with_two_fingers'
 $switch=@{pattern=$null}
 Wait-Until {
  $matches=$root.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,$rotation.title))
  foreach($match in $matches){$pattern=$null;if(!$match.Current.IsOffscreen -and $match.TryGetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern,[ref]$pattern)){$switch.pattern=$pattern;return $true}}
  $false
 } 'Two-finger rotation switch did not arrange'
 $switch.pattern.Toggle()
 Wait-Until {!(Model).state.settings.rotate_with_two_fingers} 'Two-finger rotation preference did not reach shared settings'
 Capture "canvas-preferences-$Theme" -WithModel -Composed
 Invoke-Id 'CloseButton';Wait-Until {!(Model).preferences} 'Preferences did not close'
 Reset-Zoom
 $before=(Model).state.camera.zoom;Wheel
 Wait-Until {(Model).state.camera.zoom -gt $before} 'Plain wheel did not zoom after selecting Zoom'
 Reset-Zoom
 $before=(Model).state.camera.zoom;Wheel 0x11
 Wait-Until {(Model).state.camera.zoom -gt $before} 'Ctrl+wheel did not zoom with Wheel Zoom selected'
 $before=(Model).state.camera
 Wheel 0x10
 Wait-Until {[Math]::Abs((Model).state.camera.translation[0]-$before.translation[0]) -gt .001} 'Shift+wheel did not pan horizontally'
 if([Math]::Abs((Model).state.camera.zoom-$before.zoom) -gt .001){throw 'Shift+wheel changed zoom'}
 if(Find 'settings-button' -Visible){Invoke-Id 'settings-button'}else{Menu-Command 'edit' 'settings'}
 Invoke-Id 'preference-page-canvas';Wait-Until {(Model).preferences.page -eq 'canvas'} 'Canvas preferences did not reopen'
 $wheel=Preference 'wheel_behavior'
 (Control 'preference-choice-wheel_behavior').GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
 (Control $wheel.kind.options[0] -Name -Type ([System.Windows.Automation.ControlType]::ListItem)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
 Wait-Until {!(Model).state.settings.wheel_zoom} 'Wheel Pan preference did not reach shared settings'
 Invoke-Id 'CloseButton';Wait-Until {!(Model).preferences} 'Preferences did not close after Wheel Pan'
 $before=(Model).state.camera;Wheel
 Wait-Until {[Math]::Abs((Model).state.camera.translation[1]-$before.translation[1]) -gt .001} 'Plain wheel did not pan after selecting Pan'
 if([Math]::Abs((Model).state.camera.zoom-$before.zoom) -gt .001){throw 'Wheel Pan changed zoom'}
 Reset-Zoom
 $before=(Model).state.camera.zoom;Wheel 0x11
 Wait-Until {(Model).state.camera.zoom -gt $before} 'Ctrl+wheel did not zoom with Wheel Pan selected'
 $checks.navigation_preferences='passed'
 [CapyRowPointer]::Dispose()
 & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved -StateDirectory $run
 $saved=Get-Content (Settings-File) -Raw|ConvertFrom-Json
 if(!$saved.zoom_tool.center_clicked_point -or $saved.wheel_zoom -or $saved.rotate_with_two_fingers){throw 'Navigation settings did not persist'}
 $checks.persistence='passed';$checks.theme=$Theme;$checks.evidence=$run
 [pscustomobject]$checks|ConvertTo-Json|Tee-Object -FilePath (Join-Path $run 'results.json')
}catch{
 if($review -and !$review.HasExited){try{Capture 'failure' -WithModel}catch{}}
 [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{[CapyRowPointer]::Dispose();Exit-CapyEnvironment}
