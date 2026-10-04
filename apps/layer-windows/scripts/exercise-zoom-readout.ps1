param([Parameter(Mandatory)][string]$Executable,[ValidateSet('dark','light')][string]$Theme='dark')
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyWaitSeconds=20
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
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
function Open-Readout([string]$Device){
 Tap $Device (Control 'canvas-view-info')
 Wait-Until {Zoom-Item 'zoom-fit_canvas'} "$Device did not open the zoom menu"
 if(!(Focused)){throw "Opening the zoom menu with $Device took focus from the canvas"}
}
function Choose([string]$Device,[string]$Id){
 Tap $Device (Item $Id)
 Wait-Until {!(Zoom-Item 'zoom-fit_canvas')} "Choosing $Id did not close the zoom menu"
}
function Item([string]$Id){$item=@{value=$null};Wait-Until {$item.value=Zoom-Item $Id;$item.value -and !$item.value.Current.IsOffscreen} "Missing zoom item $Id";$item.value}
function Camera{(Model).state.camera}
function Close-Menu{[CapyRowPointer]::Key([uint32]$review.Id,0x1B);Wait-Until {!(Zoom-Item 'zoom-fit_canvas')} 'Escape did not close the zoom menu'}
function Control-Wheel{
 $at=Center (Control 'drawing-canvas');[CapyRowPointer]::Hover($at[0],$at[1])
 [CapyRowPointer]::Hold(0x11,$true);try{[CapyRowPointer]::Wheel($at[0],$at[1],120)}finally{[CapyRowPointer]::Hold(0x11,$false)}
}
function Toggle-Lock([string]$Id,[string]$Field,[bool]$Locked){
 Open-Readout 'mouse';Choose 'mouse' $Id
 Wait-Until {(Camera).$Field -eq $Locked} "$Id did not set $Field to $Locked"
 Open-Readout 'mouse'
 Wait-Until {((Item $Id).Current.ItemStatus -ne '') -eq $Locked} "$Id check mark did not follow the camera"
 Close-Menu
}
try {
 Enter-CapyEnvironment
 $env:CAPY_STORAGE_DIR=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1'
 [IO.File]::WriteAllText((Settings-File),(@{language=@{Explicit='en'};theme=$Theme}|ConvertTo-Json -Depth 4))
 $review=Start-Process -FilePath $Executable -WorkingDirectory $run -PassThru -RedirectStandardError (Join-Path $run 'stderr.log')
 $null=$review.Handle
 Write-Output "Owned zoom readout review $($review.Id): $run"
 $watch=[Diagnostics.Stopwatch]::StartNew()
 do{$review.Refresh();if($review.HasExited){throw 'Review exited at startup'};Start-Sleep -Milliseconds 100}while($review.MainWindowHandle -eq [IntPtr]::Zero -and $watch.Elapsed.TotalSeconds -lt 45)
 $root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
 Wait-Until {(Model).brush_ready -and (Model).windows_workspace.ready -and !(Model).windows_workspace.busy} 'Zoom review did not start' 45
 $root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Maximized)
 [CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
 [CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)|Out-Null
 [CapyRowPointer]::Initialize([uint32]$review.Id)
 (Control 'drawing-canvas').SetFocus()
 Wait-Until {Focused} 'The canvas did not take focus'

 Open-Readout 'mouse'
 Capture "zoom-menu-$Theme"
 $order=@('zoom-field','zoom-zoom_in','zoom-200','zoom-lock-zoom','rotation-field','zoom-reset-rotation','zoom-lock-rotation','zoom-button-zoom_out')
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
 Wait-Until {Zoom-Item 'zoom-fit_canvas'} 'Right-click did not open the zoom menu'
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
 Wait-Until {!(Zoom-Item 'zoom-fit_canvas')} 'Escape did not close the zoom menu'
 (Control 'drawing-canvas').SetFocus()

 Open-Readout 'mouse'
 Tap 'mouse' (Control 'canvas-view-info')
 Wait-Until {!(Zoom-Item 'zoom-fit_canvas')} 'A second press did not close the zoom menu'
 $checks.second_press_closes='passed'
 Open-Readout 'touch'
 [CapyRowPointer]::Key([uint32]$review.Id,0x1B)
 Wait-Until {!(Zoom-Item 'zoom-fit_canvas') -and (Focused)} 'Escape did not close the zoom menu with focus on the canvas'
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
 Choose 'touch' 'zoom-reset-rotation'
 Wait-Until {Near (Camera).rotation 0} 'Reset rotation did not restore the view'
 $checks.reset_rotation='passed'

 $before=Zoom;Control-Wheel
 Wait-Until {!(Near (Zoom) $before)} 'Ctrl+wheel did not zoom the unlocked view'
 Toggle-Lock 'zoom-lock-zoom' 'zoom_locked' $true
 $before=Zoom;Control-Wheel;Start-Sleep -Milliseconds 600
 if(!(Near (Zoom) $before)){throw 'Ctrl+wheel zoomed a locked view'}
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
 Exit-CapyEnvironment
}
