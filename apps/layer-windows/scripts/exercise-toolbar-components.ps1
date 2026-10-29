param([Parameter(Mandatory)][string]$Executable,[ValidateSet('dark','light')][string]$Theme='dark',[ValidateSet('full','numeric','numeric-light')][string]$Journey='full')
$ErrorActionPreference='Stop'
if($Journey -eq 'numeric-light'){$Theme='light'}
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyPopups=$true
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
if($env:CAPY_PRIVATE_DESKTOP){[CapyRowPointer]::VerifyPrivateDesktop($env:CAPY_PRIVATE_DESKTOP)}
$CapyEach={if($env:CAPY_PRIVATE_DESKTOP){[CapyRowPointer]::VerifyPrivateDesktop($env:CAPY_PRIVATE_DESKTOP)};[CapyRowPointer]::Verify()}
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/toolbar-components/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$directory=$run;$CapyTraceDirectory=$run
function Box([string]$Id){(Control $Id -Arranged).Current.BoundingRectangle}
function Tile([string]$Kind){
    foreach($panel in @((Model).panels)){foreach($tile in @($panel.tiles)){if($tile.control.kind -eq $Kind){return $tile}}}
}
function Brush{(Model).state.brush}
function Along($box,[double]$fraction){
    if($box.Height -gt $box.Width){@{x=[int]($box.X+$box.Width/2);y=[int]($box.Y+$box.Height-$fraction*$box.Height)}}
    else{@{x=[int]($box.X+$fraction*$box.Width);y=[int]($box.Y+$box.Height/2)}}
}
function Press([string]$Device,$at){[CapyRowPointer]::Down($Device,$at.x,$at.y);Start-Sleep -Milliseconds 80}
function Drag($from,$to){
    for($i=1;$i -le 10;$i++){[CapyRowPointer]::Move([int]($from.x+($to.x-$from.x)*$i/10),[int]($from.y+($to.y-$from.y)*$i/10));Start-Sleep -Milliseconds 25}
}
function Release{[CapyRowPointer]::Up();Start-Sleep -Milliseconds 250}
function Focus-Review{$null=[CapyRowPointer]::SetForegroundWindow($drawingWindow);Start-Sleep -Milliseconds 120}
function Switch-Workspace([string]$Name,[string]$Id){
    $found=@{switch=$null;menu=$null}
    Wait-Until {$found.switch=Find ('workspace-switch-'+$Id) -Visible;$found.menu=Find 'header-workspace-menu' -Visible;$found.switch -or $found.menu} "No workspace switcher for $Name"
    $switch=$found.switch
    if(!$switch){Invoke 'header-workspace-menu';$switch=Control $Name -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)}
    $switch.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
    Wait-Until {(Model).windows_workspace.id -eq $Id -and !(Model).windows_workspace.busy} "$Name did not open" 20
}
function Finish-Review([string]$Scope){
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -StateDirectory $run -Action Close -DiscardUnsaved
    if($review.ExitCode -ne 0){throw "Native review exit code $($review.ExitCode)"}
    if((Get-Item $stderr).Length){throw 'Review stderr is not empty'}
    @{status='passed';theme=$Theme;journey=$Journey;captures=$run;scope=$Scope}|ConvertTo-Json
}
try {
    Enter-CapyEnvironment
    $env:CAPY_STORAGE_DIR=Join-Path $run 'profile'
    [IO.File]::WriteAllText((Settings-File),(@{language=@{Explicit='en'};theme=$Theme}|ConvertTo-Json -Depth 4));$env:CAPY_TRACE_UI='1'
    $stderr=Join-Path $run 'stderr.log'
    $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
    $null=$review.Handle
    Write-Output "Owned toolbar review $($review.Id): $run"
    Wait-Until {$review.Refresh();$native=Owned-DrawingWindow $review;if(!$native){return $false};$script:drawingWindow=$native.Handle;$script:root=$native.Root;(Model).brush_ready -and (Model).windows_workspace.ready -and !(Model).windows_workspace.busy} 'Toolbar review did not start' 90
    $root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Maximized)
    $null=[CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4));Focus-Review
    [CapyRowPointer]::Initialize([uint32]$review.Id)

    Switch-Workspace 'Sketch' 'builtin:workspace:painter'
    $size=Tile 'brush_size_slider';$strength=Tile 'brush_strength_slider'
    if(!$size -or !$strength -or !$size.component -or !$strength.component){throw 'Sketch lacks the shared brush sliders'}
    $band=@((Model).state.workspace.layout.bands)
    if($band.Count -ne 1 -or $band[0].edge -ne 'left' -or $band[0].alignment -ne 'center'){throw 'Sketch sliders are not docked in the centered left compact region'}
    Capture 'sketch-dark' -WithModel

    if($Journey -eq 'full'){
    $track="component-slider-$($size.id)";$preview="slider-bookmark-$($size.id)"
    Focus-Review
    $start=(Brush).diameter
    Press 'mouse' (Along (Box $track) .85)
    Wait-Until {$null -ne (Find $preview)} 'Size preview did not open on press'
    Wait-Until {(Brush).diameter -gt $start} 'Pressing high on the size track did not enlarge the brush'
    $high=(Brush).diameter
    Drag (Along (Box $track) .85) (Along (Box $track) .25);Release
    Wait-Until {(Brush).diameter -lt $high} 'Dragging down the size track did not shrink the brush'
    Wait-Until {$null -eq (Find $preview)} 'Size preview stayed open after a drag'
    Write-Output "PASS: Mouse size drag $start -> $high -> $((Brush).diameter) px; preview closes after drag"

    Press 'mouse' (Along (Box $track) .5);Release
    Wait-Until {$null -ne (Find $preview)} 'Size preview did not stay open after a tap'
    $marked=(Brush).diameter
    Invoke "slider-bookmark-$($size.id)"
    Wait-Until {@((Tile 'brush_size_slider').component.bookmarks).Count -eq 1} 'Bookmark was not added'
    Capture 'size-preview' -WithModel
    [CapyRowPointer]::Key(0x1B);Start-Sleep -Milliseconds 200
    Wait-Until {$null -eq (Find $preview)} 'Escape did not close the size preview'
    Press 'mouse' (Along (Box $track) .2);Release
    [CapyRowPointer]::Key(0x1B);Start-Sleep -Milliseconds 200
    $mark=@((Tile 'brush_size_slider').component.bookmarks)[0]
    $box=Box $track
    $recall=[Math]::Min([double]1,[double]$mark.fill+0.02)
    Write-Output "Recall tap at $recall for bookmark $($mark.value) px from $((Brush).diameter) px"
    Press 'mouse' (Along $box $recall);Release
    Wait-Until {[Math]::Abs((Brush).diameter-$marked) -lt 0.001} 'A tap near the bookmark did not recall its exact value'
    [CapyRowPointer]::Key(0x1B);Start-Sleep -Milliseconds 200
    Write-Output "PASS: Tap keeps preview open, bookmark $marked px is stored and recalled by a nearby tap"

    $strengthTrack="component-slider-$($strength.id)"
    foreach($device in @('touch','pen')){
        Focus-Review
        Press $device (Along (Box $strengthTrack) .9)
        Wait-Until {(Tile 'brush_strength_slider').component.numeric.value -gt .8} "$device press did not set a high strength"
        Drag (Along (Box $strengthTrack) .9) (Along (Box $strengthTrack) .3);Release
        Wait-Until {(Tile 'brush_strength_slider').component.numeric.value -lt .45} "$device drag did not lower strength"
        Wait-Until {$null -eq (Find "slider-bookmark-$($strength.id)")} "$device preview stayed open after a drag"
        Write-Output "PASS: $device strength drag to $([Math]::Round((Tile 'brush_strength_slider').component.numeric.value,2))"
    }

    $layout=(Model).state.workspace.layout|ConvertTo-Json -Depth 40 -Compress
    $cap=Box "slider-cap-$($size.id)"
    $at=@{x=[int]($cap.X+$cap.Width/2);y=[int]($cap.Y+$cap.Height/2)}
    Press 'mouse' $at;Drag $at @{x=$at.x+120;y=$at.y};Release
    if(((Model).state.workspace.layout|ConvertTo-Json -Depth 40 -Compress) -ne $layout){throw 'A quick cap drag moved the toolbar component'}
    Write-Output 'PASS: A quick cap drag without a hold does not reorder the component'
    }

    foreach($kind in @('brush_size_slider','brush_strength_slider')){
        $tile=Tile $kind
        foreach($option in @('Show value','Expand to fit')){
            $cap=Control "slider-cap-$($tile.id)" -Arranged
            $cap.SetFocus()
            Wait-Until {$cap.Current.HasKeyboardFocus} 'The current slider cap did not receive keyboard focus'
            [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x10),0x79)
            Invoke $option -Name
            Wait-Until {!(Find $option -Name -Visible)} "$option menu did not close"
            $field=if($option -eq 'Show value'){'show_value'}else{'expand'}
            Wait-Until {(Tile $kind).slider_style.$field} "$option did not publish its slider style"
        }
        Wait-Until {(Tile $kind).slider_style.show_value -and (Tile $kind).slider_style.expand} 'Slider display options did not persist'
        (Control "component-slider-$($tile.id)").GetCurrentPattern([System.Windows.Automation.RangeValuePattern]::Pattern).SetValue(1)
        $expected=if($kind -eq 'brush_size_slider'){'2048'}else{'100'}
        $expectedUnit=if($kind -eq 'brush_size_slider'){'px'}else{'%'}
        Wait-Until {(Control "slider-value-$($tile.id)").Current.Name -eq $expected} 'Slider value did not update'
        if((Control "slider-unit-$($tile.id)").Current.Name -ne $expectedUnit){throw 'Slider unit differs from the shared numeric unit'}
        $cap=Box "slider-cap-$($tile.id)";$number=Box "slider-value-$($tile.id)";$unit=Box "slider-unit-$($tile.id)"
        if($unit.Top -lt $number.Bottom){throw 'A long Medium vertical value did not stack its unit'}
        $scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
        foreach($text in @($number,$unit)){
            if($text.Left -lt $cap.Left+5.5*$scale -or $text.Top -lt $cap.Top+5.5*$scale -or $text.Right -gt $cap.Right-5.5*$scale -or $text.Bottom -gt $cap.Bottom-5.5*$scale){throw 'Slider value/unit touches its cap edge'}
        }
    }
    Capture "slider-values-$Theme" -WithModel -Composed
    Write-Output 'PASS: Slider display menus, persisted choices, live numbers, stacked units and clear margins'
    $root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Normal)
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -StateDirectory $run -Action Resize -Width 1200 -Height 320
    $scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
    foreach($kind in @('brush_size_slider','brush_strength_slider')){
        $tile=Tile $kind
        Wait-Until {[Math]::Abs((Box "slider-cap-$($tile.id)").Height/$scale-8) -lt 1} 'Short slider retained its value cap'
        if(Find "slider-value-$($tile.id)" -Visible){throw 'Short slider retained its visible value label'}
        if(!(Tile $kind).slider_style.show_value){throw 'Short slider cleared its saved Show value preference'}
    }
    Capture "slider-values-short-$Theme" -WithModel -Composed
    $root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Maximized)
    foreach($kind in @('brush_size_slider','brush_strength_slider')){
        $tile=Tile $kind
        Wait-Until {(Box "slider-cap-$($tile.id)").Height/$scale -gt 8 -and !(Control "slider-value-$($tile.id)").Current.IsOffscreen} 'Restoring slider length did not restore its value'
        if(!(Tile $kind).slider_style.show_value){throw 'Restoring slider length changed its Show value preference'}
    }
    Capture "slider-values-restored-$Theme" -WithModel -Composed
    if($Journey -ne 'full'){Finish-Review 'Slider display preferences, live values, short-track hiding and restoration';return}

    Switch-Workspace 'Photo' 'builtin:workspace:photographer'
    $options=Tile 'tool_options'
    if(!$options -or !$options.component){throw 'Photo lacks Tool Options'}
    $commands=@((Model).panels|Where-Object id -eq 'commands')[0]
    if(@($commands.tiles)[-1].control.kind -ne 'tool_options'){throw 'Tool Options is not the last Photo command'}
    Invoke 'toolbar-choice-tool'
    Invoke 'Pen' -Name
    Wait-Until {(Control 'toolbar-choice-tool').Current.ItemStatus -eq 'Pen' -and !(Find 'Pen' -Name -Type ([System.Windows.Automation.ControlType]::MenuItem) -Visible)} 'Tool Options did not acknowledge the Pen choice'
    Wait-Until {$null -ne (Find 'toolbar-setting-size')} 'Brush Tool Options did not show a size field'
    $entry=Control 'toolbar-setting-size'
    $entry.SetFocus();$entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('37')
    [CapyRowPointer]::Key(0x0D)
    Wait-Until {[Math]::Abs((Brush).diameter-37) -lt 0.001} 'Tool Options size entry did not apply'
    foreach($shown in @($false,$true)){
        (Control "toolbar-more-$($options.id)").SetFocus()
        [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x10),0x79)
        Invoke 'Show Sliders' -Name
        Wait-Until {(Tile 'tool_options').control.style.sliders -eq $shown -and !(Find 'Show Sliders' -Name -Visible)} 'Tool Options slider visibility did not update'
        Wait-Until {($null -ne (Find 'toolbar-setting-size-slider' -Visible)) -eq $shown} 'The native numeric track did not follow Show Sliders'
    }
    $before=(Brush).diameter;$track=Box 'toolbar-setting-size-slider'
    Press 'mouse' (Along $track .8);Release
    Wait-Until {[Math]::Abs((Brush).diameter-$before) -gt .001} 'Restored inline numeric track did not accept native pointer input'
    $entry=Control 'toolbar-setting-size';$entry.SetFocus();$entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('37')
    [CapyRowPointer]::Key(0x0D)
    Wait-Until {[Math]::Abs((Brush).diameter-37) -lt .001} 'Tool Options size did not restore after the track check'
    Write-Output 'PASS: Tool Options Show Sliders hides and restores the numeric track and native input'
    Capture 'photo-brush-options' -WithModel
    $beforeFaceLayout=(Model).state.workspace.layout|ConvertTo-Json -Depth 40 -Compress
    $grip=Box 'ribbon-grip-commands';$hostBox=$root.Current.BoundingRectangle;$scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
    if($scale -le 0 -or [double]::IsNaN($scale) -or [double]::IsInfinity($scale)){throw 'Invalid owned toolbar DPI'}
    $from=@{x=[int]($grip.X+$grip.Width/2);y=[int]($grip.Y+$grip.Height/2)};$to=@{x=[int]($hostBox.Left+8*$scale);y=[int]($hostBox.Top+$hostBox.Height/2)}
    $workspace=Workspace-Root
    if(!$workspace){throw 'Compact face setup has no native workspace trace'}
    Focus-Review;[CapyRowPointer]::Verify()
    $foreground=[CapyRowPointer]::GetForegroundWindow();[uint32]$foregroundProcess=0
    $null=[CapyRowPointer]::GetWindowThreadProcessId($foreground,[ref]$foregroundProcess)
    $gripEvidence=@{from=$from;to=$to;foreground_window=$foreground.ToInt64();foreground_process=$foregroundProcess;before=$workspace.Current.HelpText}
    $gripEvidence|ConvertTo-Json -Depth 10|Set-Content (Join-Path $run 'compact-face-setup.json')
    try{
        Press 'mouse' $from;Drag $from $to
        Wait-Until {$contact=$workspace.Current.HelpText|ConvertFrom-Json;$contact.phase -eq 'dragging' -and $contact.captured -and $contact.source.panel -eq 'commands'} 'Compact face setup did not admit the native grip drag'
    }finally{
        $gripEvidence.after=$workspace.Current.HelpText
        $gripEvidence|ConvertTo-Json -Depth 10|Set-Content (Join-Path $run 'compact-face-setup.json')
    }
    Release
    $null=Box 'toolbar-face-size'
    $faceCases=@()
    foreach($device in @('pen','touch')){foreach($activation in @('mouse','enter','space','invoke')){
        $face=Control 'toolbar-face-size';$bounds=Box 'toolbar-face-size';$x=[int]($bounds.X+$bounds.Width/2);$y=[int]($bounds.Y+$bounds.Height/2)
        $before=(Brush).diameter;$nextY=[int]($y+$(if($before -gt 100){20}else{-20})*$scale)
        $case=@{device=$device;activation=$activation;bounds=$bounds.ToString();runtime_id=@($face.GetRuntimeId());before=$before;down=@($x,$y);move=@($x,$nextY)}
        try{
            [CapyRowPointer]::Down($device,$x,$y);[CapyRowPointer]::Move($x,$nextY)
            Wait-Until {[Math]::Abs((Brush).diameter-$before) -gt .001} 'Compact face scrub did not acknowledge an incremental setting change'
            [CapyRowPointer]::Up();$case.current=(Brush).diameter
            if($activation -eq 'mouse'){[CapyRowPointer]::Down('mouse',$x,$y);[CapyRowPointer]::Up()}
            elseif($activation -eq 'invoke'){$face.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()}
            else{Focus-Keyboard $face;$case.activation_focus=$face.Current.HasKeyboardFocus;[CapyRowPointer]::Key([uint32]$review.Id,$(if($activation -eq 'enter'){0x0D}else{0x20}))}
            $popupId='number-root-'+$face.Current.Name
            Wait-Until {$null -ne (Find $popupId)} 'Compact numeric face swallowed the activation after a scrub'
            if((Brush).diameter -ne $case.current){throw 'Future face activation changed the incremental scrub value'}
            [CapyRowPointer]::Key([uint32]$review.Id,0x1B);Wait-Until {!(Find $popupId)} 'Compact numeric popup did not dismiss'
            if((Brush).diameter -ne $case.current){throw 'Dismissing the numeric popup changed the incremental scrub value'}
            $case.passed=$true
        }catch{$case.error=$_.Exception.Message;throw}finally{$faceCases+=@($case);$faceCases|ConvertTo-Json -Depth 10|Set-Content (Join-Path $run 'compact-face-cases.json')}
    }}
    (Control 'toolbar-face-size').SetFocus();[CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11,0x12),0x5A)
    Wait-Until {((Model).state.workspace.layout|ConvertTo-Json -Depth 40 -Compress) -eq $beforeFaceLayout} 'Compact face setup did not restore the original toolbar layout'
    Invoke "toolbar-more-$($options.id)"
    Wait-Until {$null -ne (Model).state.customization.drawer} 'More did not open the Tool Options drawer'
    Capture 'photo-more-drawer' -WithModel
    [CapyRowPointer]::Key(0x1B);Start-Sleep -Milliseconds 300
    Write-Output 'PASS: Photo Tool Options field edit and More drawer'

    Invoke (Tool-Tile 'gradient')
    $gradient={(@((Model).state.tool_extra|Where-Object {$_.Gradient})[0]).Gradient.value.value|ConvertTo-Json -Depth 8 -Compress}
    Wait-Until {(& $gradient) -and (Find 'toolbar-gradient')} 'Gradient Tool Options did not show the gradient button'
    $original=& $gradient;$scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
    $box=(Control 'toolbar-gradient').Current.BoundingRectangle
    if([Math]::Abs($box.Height-26*$scale) -gt 1.5){throw 'The Tool Options gradient button is not 26 DIP tall'}
    $face=@{x=[int]($box.X+$box.Width/2);y=[int]($box.Y+$box.Height/2)}
    foreach($device in @('mouse','touch','pen')){
        Focus-Review;Press $device $face;Release
        $hit=@{item=$null};Wait-Until {$hit.item=Zoom-Item 'toolbar-gradient-editor-reverse';$null -ne $hit.item} "$device did not open the Tool Options gradient editor"
        if($device -eq 'mouse'){
            Capture 'photo-gradient-options' -WithModel
            $hit.item.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
            Wait-Until {(& $gradient) -ne $original} 'Reverse in Tool Options did not change the tool gradient'
        }
        if($device -eq 'pen'){
            (Zoom-Item 'toolbar-gradient-editor-reset').GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
            Wait-Until {(& $gradient) -eq $original} 'Reset in Tool Options did not restore the tool gradient'
        }
        [CapyRowPointer]::Key(0x1B)
        Wait-Until {!(Zoom-Item 'toolbar-gradient-editor-reverse')} "$device gradient editor did not close with Escape"
    }
    Write-Output 'PASS: Photo Tool Options gradient button and editor'

    [CapyRowPointer]::Dispose()
    Invoke (Tool-Tile 'zoom')
    Wait-Until {(Model).state.layer_tools.tool -eq 'zoom'} 'Zoom did not activate for Tool Options'
    $scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
    foreach($id in 'zoom-click','zoom-drag'){
        $spec=@((Model).state.tool_extra|ForEach-Object {$_.Choice}|Where-Object id -eq $id)[0]
        $previous=$null
        for($i=0;$i -lt $spec.items.Count;$i++){
            $button=Control "toolbar-segment-$id-$i";$bounds=Box "toolbar-segment-$id-$i"
            if($button.Current.Name -ne $spec.items[$i].label){throw "Tool Options $id/$i accessible label differs from shared copy"}
            if([Math]::Abs($bounds.Width/$scale-36) -gt 1 -or [Math]::Abs($bounds.Height/$scale-26) -gt 1){throw "Tool Options $id/$i is not one Small tile wide and 26 points tall"}
            $glyph=Control ('icon-'+$spec.items[$i].icon) -Within $button;$image=$glyph.Current.BoundingRectangle
            if([Math]::Abs($image.Width/$scale-16) -gt 1 -or [Math]::Abs($image.Height/$scale-16) -gt 1){throw "Tool Options $id/$i has the wrong icon size"}
            if($previous -and [Math]::Abs($bounds.Left-$previous.Right) -gt 1){throw "Tool Options $id is not a contiguous bar"}
            $previous=$bounds
        }
    }
    $center=Control 'toolbar-action-center_zoom_clicks'
    if($center.Current.ControlType -ne [System.Windows.Automation.ControlType]::CheckBox){throw 'Iconless Tool Options action is not a native checkbox'}
    if([Math]::Abs((Box 'toolbar-action-center_zoom_clicks').Height/$scale-26) -gt 1){throw 'The Tool Options checkbox is not 26 DIP tall'}
    $center.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
    Wait-Until {(Model).state.settings.zoom_tool.center_clicked_point} 'Tool Options checkbox did not reach shared settings'
    Capture "photo-zoom-options-$Theme" -WithModel -Composed
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -StateDirectory $run -Action Resize -Width 744 -Height 760
    Invoke "toolbar-more-$($options.id)"
    Wait-Until {$null -ne (Model).state.customization.drawer} 'Narrow Tool Options overflow did not open its drawer'
    $drawer=Control 'tool-drawer'
    foreach($id in 'zoom-click','zoom-drag'){
        $spec=@((Model).state.tool_extra|ForEach-Object {$_.Choice}|Where-Object id -eq $id)[0]
        for($i=0;$i -lt $spec.items.Count;$i++){
            $button=Control "tool-choice-$id-$i" -Within $drawer
            if($button.Current.Name -ne $spec.items[$i].label){throw 'Overflow drawer lost its shared option label'}
            $glyph=Control ('icon-'+$spec.items[$i].icon) -Within $button
        }
    }
    Capture "photo-zoom-overflow-$Theme" -WithModel -Composed
    $target=(Control 'tool-choice-zoom-click-0' -Within $drawer).Current.BoundingRectangle
    [CapyRowPointer]::Hover([int]($target.X+$target.Width/2),[int]($target.Y+$target.Height/2))
    [CapyRowPointer]::Key(0x1B)
    Wait-Until {!((Model).state.customization.drawer)} 'Zoom overflow drawer did not close'
    Finish-Review 'Guarded OS-delivered synthetic mouse, touch and pen input. Physical digitizers are not tested.'
} catch {
    $_|Out-String|Set-Content (Join-Path $run 'failure.txt');$_.ScriptStackTrace|Add-Content (Join-Path $run 'failure.txt')
    if($review){$review.Refresh();if(!$review.HasExited){try{Capture 'failure' -WithModel}catch{$_|Out-String|Set-Content (Join-Path $run 'capture-error.txt')}}};throw
} finally {
    [CapyRowPointer]::Dispose()
    if($review){$review.Refresh();if(!$review.HasExited){Stop-Process -Id $review.Id -Force}}
    Exit-CapyEnvironment
}
