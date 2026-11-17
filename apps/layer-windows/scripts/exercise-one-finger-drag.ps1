param([Parameter(Mandatory)][string]$Executable,[ValidateSet('dark','light')][string]$Theme='dark')
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
Add-Type -Path (Join-Path $PSScriptRoot 'CanvasTouchDriver.cs')
[CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
$CapyPopups=$true;$CapyFind='prefer-visible'
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$run=Join-Path $repo ('artifacts/windows/one-finger-drag/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$CapyTraceDirectory=$run
$labels=@('Auto','Draw','Move canvas','Off');$values=@('auto','on','move_canvas','off')
function Window-Command([string]$Id){
 & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'Window'
 Invoke $Id
}
function At([string]$Id){$b=(Control $Id -Arranged).Current.BoundingRectangle;@([int]($b.X+$b.Width/2),[int]($b.Y+$b.Height/2))}
function Click([string]$Id){$p=At $Id;[CapyRowPointer]::Down('mouse',$p[0],$p[1]);[CapyRowPointer]::Up()}
function Park{$p=At 'drawing-canvas';[CapyRowPointer]::Hover($p[0],$p[1])}
function Context([string]$Id){$p=At $Id;[CapyRowPointer]::RightClick($p[0],$p[1])}
function Dismiss{[CapyRowPointer]::Key([uint32]$review.Id,0x1b);Wait-Until {@(@('Auto','Customize Title Bar…','Insert Tools…')|Where-Object {Find $_ -Name -Type ([System.Windows.Automation.ControlType]::MenuItem) -Visible}).Count -eq 0} 'One-finger menu did not dismiss'}
function Header{(Model).header.model}
function Entries{@((Header).zones|ForEach-Object {$_})}
function Finger-Tile{@((Model).panels|Where-Object id -eq 'toolbar')[0].tiles|Where-Object {$_.control.command -eq 'one_finger_drag'}}
function Add-Finger{
 Wait-Until {(Model).picker -and (Find 'tool-picker-search' -Visible)} 'Add Tools picker did not open'
 (Control 'tool-picker-search').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('One-finger drag')
 Wait-Until {(Model).picker.query -eq 'One-finger drag' -and (Find 'picker-choice-command-one_finger_drag' -Visible)} 'One-finger drag is missing from Add Tools'
 Invoke 'picker-choice-command-one_finger_drag'
 Wait-Until {(Model).picker.can_confirm} 'One-finger drag could not be selected'
 Invoke ((Model).picker.confirm_label) -Name -Within (Control 'tool-picker') -Type ([System.Windows.Automation.ControlType]::Button)
 Wait-Until {!(Model).picker -and !(Find 'tool-picker' -Visible) -and (Control 'drawing-canvas').Current.IsEnabled} 'Add Tools picker did not close and release the canvas'
}
function Menu-Choices([string]$Name){
 $rows=@($labels|ForEach-Object {Control $_ -Name -Type ([System.Windows.Automation.ControlType]::MenuItem) -Arranged})
 $selected=$values.IndexOf((Model).state.settings.finger_drawing)
 for($i=0;$i -lt 4;$i++){
  $toggle=$rows[$i].GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern)
  $expected=if($i -eq $selected){[System.Windows.Automation.ToggleState]::On}else{[System.Windows.Automation.ToggleState]::Off}
  if($toggle.Current.ToggleState -ne $expected){throw "Wrong native checkmark for $($labels[$i])"}
  if($i -gt 0 -and $rows[$i].Current.BoundingRectangle.Top -lt $rows[$i-1].Current.BoundingRectangle.Bottom){throw 'One-finger menu rows overlap or change order'}
 }
 $more=Control 'More Touch Settings…' -Name -Type ([System.Windows.Automation.ControlType]::MenuItem) -Arranged
 $walker=[System.Windows.Automation.TreeWalker]::ControlViewWalker;$menu=$walker.GetParent($more)
 while($menu -and $menu.Current.ControlType -ne [System.Windows.Automation.ControlType]::Menu){$menu=$walker.GetParent($menu)}
 if(!$menu){throw 'One-finger choices have no native menu owner'}
 $items=@($menu.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::MenuItem)))
 if($items.Count -ne 5 -or ($items.Current.Name -join '|') -ne (($labels+@('More Touch Settings…')) -join '|')){throw 'One-finger menu must contain four choices and More Touch Settings in order'}
 $ordinaryGap=0
 for($i=1;$i -lt $rows.Count;$i++){$ordinaryGap=[Math]::Max($ordinaryGap,$rows[$i].Current.BoundingRectangle.Top-$rows[$i-1].Current.BoundingRectangle.Bottom)}
 if($more.Current.BoundingRectangle.Top-$rows[-1].Current.BoundingRectangle.Bottom -le $ordinaryGap+1){throw 'One-finger choices lack their native section divider before More Touch Settings'}
 Capture $Name -WithModel -Composed
}
function Customization-Menu([string]$Name){
 $null=Control $(if($Name.StartsWith('header')){'Customize Title Bar…'}else{'Insert Tools…'}) -Name -Type ([System.Windows.Automation.ControlType]::MenuItem) -Arranged
 foreach($label in $labels){if(Find $label -Name -Type ([System.Windows.Automation.ControlType]::MenuItem) -Visible){throw "Customization menu contains the primary choice: $label"}}
 Capture $Name -WithModel -Composed
}
function Check-Tile([string]$Id,[string]$Name){
 foreach($i in 0..3){
  Click $Id;Menu-Choices "$Name-choice-$i"
  if($Name -eq 'header'){
   $anchor=(Control $Id -Arranged).Current.BoundingRectangle
   $first=(Control 'Auto' -Name -Type ([System.Windows.Automation.ControlType]::MenuItem) -Arranged).Current.BoundingRectangle
   if($first.Top -lt $anchor.Bottom){throw 'The primary title-bar menu overlaps its tile'}
  }
  Invoke $labels[$i] -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)
  Wait-Until {(Model).state.settings.finger_drawing -eq $values[$i] -and !(Find 'Auto' -Name -Type ([System.Windows.Automation.ControlType]::MenuItem) -Visible)} 'Tile choice did not update and close'
  $expected='finger-draw'
  $view=if($Name -eq 'header'){@((Model).header.items|Where-Object id -eq $script:fingerHeader)}else{Finger-Tile}
  if(@($view).Count -ne 1 -or $view.icon -ne $expected){throw "One-finger tile has the wrong icon for $($labels[$i]): $($view.icon)"}
  Park;Capture "$Name-icon-$i" -Composed
 }
 $button=Control $Id -Arranged
 if($button.Current.ControlType -ne [System.Windows.Automation.ControlType]::Button){$button=Control 'One-finger drag' -Name -Type ([System.Windows.Automation.ControlType]::Button) -Within $button -Arranged}
 $button.SetFocus();Wait-Until {$button.Current.HasKeyboardFocus} 'One-finger tile did not receive native focus'
 [CapyRowPointer]::Key([uint32]$review.Id,0x0D)
 Menu-Choices "$Name-keyboard";Dismiss
 Context $Id;Customization-Menu "$Name-secondary";Dismiss
 $p=At $Id;[CapyRowPointer]::Down('touch',$p[0],$p[1]);Start-Sleep -Milliseconds 900
 try{Customization-Menu "$Name-hold"}finally{[CapyRowPointer]::Up()}
 $null=Control $(if($Name -eq 'header'){'Customize Title Bar…'}else{'Insert Tools…'}) -Name -Type ([System.Windows.Automation.ControlType]::MenuItem) -Arranged
 Dismiss
 Click $Id;Invoke 'More Touch Settings…' -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)
 Wait-Until {(Model).preferences.page -eq 'touch' -and (Find 'preference-choice-finger_drawing' -Visible)} 'More Touch Settings did not open Touch'
 Capture "$Name-more-touch-settings" -WithModel -Composed
 Close-Preferences
}
function Open-Preferences{
 (Control 'drawing-canvas').SetFocus();[CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),0xBC)
 Wait-Until {(Model).preferences -and (Find 'preference-page-touch' -Visible)} 'Preferences did not open'
 Invoke 'preference-page-touch'
 Wait-Until {(Model).preferences.page -eq 'touch' -and (Find 'preference-choice-finger_drawing' -Visible)} 'One-finger drag did not appear in Touch'
}
function Preference-Choice([int]$Index){
 $combo=Control 'preference-choice-finger_drawing' -Arranged
 $combo.SetFocus();[CapyRowPointer]::Key([uint32]$review.Id,0x73)
 Wait-Until {Find $labels[$Index] -Name -Type ([System.Windows.Automation.ControlType]::ListItem) -Visible} 'One-finger dropdown did not open'
 $rows=@($labels|ForEach-Object {Control $_ -Name -Type ([System.Windows.Automation.ControlType]::ListItem) -Arranged})
 Capture "settings-dropdown-$Index" -WithModel -Composed
 $box=$rows[$Index].Current.BoundingRectangle
 [CapyRowPointer]::Down('mouse',[int]($box.X+$box.Width/2),[int]($box.Y+$box.Height/2));[CapyRowPointer]::Up()
 Wait-Until {(Model).state.settings.finger_drawing -eq $values[$Index] -and $combo.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Current.ExpandCollapseState -eq [System.Windows.Automation.ExpandCollapseState]::Collapsed} 'Native Settings choice did not update and close'
}
function Close-Preferences{Invoke 'CloseButton';Wait-Until {!(Model).preferences -and (Control 'drawing-canvas').Current.IsEnabled} 'Preferences did not close'}
function Select-Tool([string]$Command){
 $tile=@((Model).panels.tiles|Where-Object {$_.control.command -eq $Command -or $_.resolved_control.command -eq $Command})[0]
 if(!$tile){throw "Missing native $Command tile"}
 Click "tile-toolbar-$($tile.id)"
 Wait-Until {@((Model).state.commands|Where-Object {$_.id -eq $Command -and $_.selected}).Count -eq 1} 'Tool did not select'
}
function Camera{(Model).state.camera}
function Pose($Camera){$Camera|Select-Object translation,zoom,rotation|ConvertTo-Json -Compress}
function Touch-Drag{
 $b=(Control 'drawing-canvas' -Arranged).Current.BoundingRectangle
 $x=[int]($b.X+$b.Width*.6);$y=[int]($b.Y+$b.Height*.55)
 [CapyCanvasTouch]::Down(1,$x,$y)
 try{for($step=1;$step -le 12;$step++){[CapyCanvasTouch]::Move(1,($x+$step*5),($y+$step*3));Start-Sleep -Milliseconds 12}}finally{[CapyCanvasTouch]::Up(1)}
}
try{
 Enter-CapyEnvironment
 $env:CAPY_STORAGE_DIR=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1'
 [IO.File]::WriteAllText((Settings-File),(@{language=@{Explicit='en'};theme=$Theme}|ConvertTo-Json -Depth 4))
 $review=Start-Process -FilePath $Executable -WorkingDirectory $run -PassThru -RedirectStandardError (Join-Path $run 'stderr.log');$null=$review.Handle
 $native=@{window=$null}
 Wait-Canvas {$native.window=Owned-DrawingWindow $review;$native.window -and (Review-Ready)} 'One-finger review did not start' 120
 $root=$native.window.Root;$handle=$native.window.Handle
 $root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Maximized)
 [CapyRowPointer]::Initialize([uint32]$review.Id)
 [CapyRowPointer]::SetForegroundWindow($handle)|Out-Null
 Wait-Until {[CapyRowPointer]::GetForegroundWindow() -eq $handle} 'The owned one-finger window did not take foreground'
 $commands=@((Model).panels|Where-Object id -eq 'commands')[0].tiles
 if($commands.Count -lt 2 -or $commands[0].control.command -ne 'one_finger_drag' -or $commands[0].icon -ne 'finger-draw' -or $commands[1].control.kind -ne 'divider'){throw 'Fresh Paint Commands must start with One-finger drag and a divider'}
 Park;Capture 'paint-default' -WithModel -Composed
 Open-Preferences
 $row=@((Model).preferences.pages.groups.rows|Where-Object id -eq 'finger_drawing')[0]
 if($row.title -ne 'One-finger drag' -or ($row.kind.options -join '|') -ne ($labels -join '|')){throw 'Windows exposes the wrong one-finger choices'}
 Capture 'settings-row' -WithModel -Composed
 foreach($i in 0..3){Preference-Choice $i}
 Close-Preferences
 (Control 'drawing-canvas').SetFocus();[CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),0x4B)
 Wait-Until {Find 'command-search' -Visible} 'Command search did not open'
 (Control 'command-search').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('One-finger drag')
 Wait-Until {(Control 'command-result-0').Current.Name -match 'One-finger drag'} 'Command search did not find One-finger drag'
 [CapyRowPointer]::Key([uint32]$review.Id,0x0D)
 Wait-Until {(Model).preferences.page -eq 'touch' -and (Find 'preference-choice-finger_drawing' -Visible)} 'One-finger command did not reveal its Settings row'
 Capture 'command-settings-row' -WithModel -Composed
 Close-Preferences
 Window-Command 'customize_workspace_ui'
 Wait-Until {(Model).header.editing -and (Find 'header-component-tools' -Visible)} 'Customize Title Bar did not open'
 $from=At 'header-component-tools';$presentation=(Control 'title-bar').Current.ItemStatus|ConvertFrom-Json;$zone=$presentation.geometry.zones[1]
 $scale=[CapyRowPointer]::GetDpiForWindow($handle)/96.;$origin=[CapyRowPointer+Point]::new()
 if(![CapyRowPointer]::ClientToScreen($handle,[ref]$origin)){throw 'Owned client origin is unavailable'}
 [CapyRowPointer]::Down('mouse',$from[0],$from[1]);[CapyRowPointer]::Move([int]($origin.x+($zone.x+$zone.width/2)*$scale),[int]($origin.y+($zone.y+$zone.height/2)*$scale))
 Wait-Until {$gesture=(Control 'title-bar').Current.HelpText|ConvertFrom-Json;$gesture.phase -eq 'dragging' -and $gesture.preview.target} 'Add Tools did not receive its title-bar drop target'
 [CapyRowPointer]::Up()
 Add-Finger
 $script:fingerHeader=@(Entries|Where-Object {$_.item.control.command -eq 'one_finger_drag'})[0].id
 if(!$script:fingerHeader){throw 'Add Tools did not insert the title-bar tile'}
 foreach($size in 'small','medium','large'){
  Invoke "header-size-$size";Wait-Until {(Header).size -eq $size} 'Title-bar size did not update'
  Invoke 'header-edit-done';Wait-Until {!(Model).header.editing} 'Title-bar editing did not finish'
  Park;Capture "header-$size" -WithModel -Composed
  if($size -eq 'medium'){Check-Tile "header-item-$script:fingerHeader" 'header'}
  Window-Command 'customize_workspace_ui';Wait-Until {(Model).header.editing} 'Title-bar editor did not reopen'
 }
 Invoke 'header-edit-done';Wait-Until {!(Model).header.editing} 'Title-bar editor did not close'
 Context 'ribbon-grip-toolbar';Invoke 'Add Tools…' -Name -Type ([System.Windows.Automation.ControlType]::MenuItem);Add-Finger
 $tile=Finger-Tile;$tileId="tile-toolbar-$($tile.id)"
 Check-Tile $tileId 'toolbar'
 foreach($size in 'small','medium','large'){
  Context 'ribbon-grip-toolbar';Invoke "$((Get-Culture).TextInfo.ToTitleCase($size)) Tiles" -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)
  Wait-Until {@((Model).panels|Where-Object id -eq 'toolbar')[0].tile_style -eq $size} 'Toolbar size did not update'
  Park;Capture "toolbar-$size" -WithModel -Composed
 }
 Fit-Canvas
 [CapyCanvasTouch]::Initialize([uint32]$review.Id)
 foreach($i in 3,2,1){
  Open-Preferences;Preference-Choice $i;Close-Preferences
  Select-Tool $(if($i -eq 2){'zoom'}else{'pen'})
  Wait-Canvas {(Model).canvas_ready -and (Model).brush_ready} 'Canvas was not ready for finger input'
  $before=Model;$camera=$before.state.camera;$pose=Pose $camera;$revision=$before.state.document_file.revision
  Touch-Drag
  if($i -eq 1){Wait-Canvas {(Model).state.document_file.revision -gt $revision} 'Draw did not paint with injected Windows touch'}
  elseif($i -eq 2){
   Wait-Until {$current=Camera;[Math]::Abs($current.translation[0]-$camera.translation[0]-60) -lt 1 -and [Math]::Abs($current.translation[1]-$camera.translation[1]-36) -lt 1 -and [Math]::Abs($current.zoom-$camera.zoom) -lt .0001 -and [Math]::Abs($current.rotation-$camera.rotation) -lt .0001} 'Move canvas with Zoom selected did not pan by the injected touch delta'
   if((Model).state.document_file.revision -ne $revision){throw 'Move canvas painted'}
  }
  else{Wait-PresentedFrame -Idle;$after=Model;if((Pose $after.state.camera) -ne $pose -or $after.state.document_file.revision -ne $revision){throw 'Off changed the drawing or view'}}
  Capture "touch-$($values[$i])" -WithModel -Composed
 }
 Select-Tool 'fill'
 $revision=(Model).state.document_file.revision;$at=At 'drawing-canvas'
 [CapyRowPointer]::Down('mouse',$at[0],$at[1]);[CapyRowPointer]::Up()
 Wait-Canvas {$m=Model;$m.state.document_file.revision -gt $revision -and !$m.state.document_file.busy -and $m.canvas_ready} 'Opaque picker sample did not finish filling'
 Select-Tool 'pen';Park;Capture 'picker-sample' -WithModel -Composed
 foreach($i in 3,2){
  Open-Preferences;Preference-Choice $i;Close-Preferences
  $before=Model;$pose=Pose $before.state.camera;$revision=$before.state.document_file.revision
  $b=(Control 'drawing-canvas' -Arranged).Current.BoundingRectangle
  $x=[int]($b.X+$b.Width*.6)+30;$y=[int]($b.Y+$b.Height*.55)+18
  [CapyCanvasTouch]::Down(1,$x,$y)
  try{
   Wait-Until {(Model).state.layer_tools.tool -like 'pick_*'} 'Holding the finger did not acquire the color picker'
   Wait-Until {$m=Model;$preview=$m.color_preview.picker.preview;$m.state.layer_tools.tool -like 'pick_*' -and $preview.rgba -and ($preview.rgba[0]+$preview.rgba[1]+$preview.rgba[2]) -lt 2.4} 'Touch hold did not sample the opaque paint'
   Capture "touch-hold-$($values[$i])" -WithModel -Composed
  }catch{Capture "touch-hold-$($values[$i])-failure" -WithModel -Composed;throw}finally{[CapyCanvasTouch]::Up(1)}
  Wait-Until {(Model).state.layer_tools.tool -eq 'paint'} 'Touch hold did not return to Pen after lift'
  $after=Model
  if((Pose $after.state.camera) -ne $pose -or $after.state.document_file.revision -ne $revision){throw 'Touch hold changed the drawing or camera'}
  $rgba=$after.state.colors.foreground.rgba
  if(!$rgba -or ($rgba[0]+$rgba[1]+$rgba[2]) -ge 2.4){throw 'Touch hold did not retain its painted color'}
 }
 @{theme=$Theme;settings='passed';title_bar='passed';toolbar='passed';touch='off/pan/draw; Off/Move hold picker';physical_input='unverified'}|ConvertTo-Json|Set-Content (Join-Path $run 'result.json')
 & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $handle.ToInt64() -StateDirectory $run -Action Close -DiscardUnsaved
}catch{
 ($_|Out-String)+$_.ScriptStackTrace|Set-Content (Join-Path $run 'failure.txt')
 if($review -and !$review.HasExited -and $root){try{Capture 'failure' -WithModel -Composed}catch{}}
 throw
}finally{
 [CapyCanvasTouch]::Dispose();[CapyRowPointer]::Dispose();Exit-CapyEnvironment
}
