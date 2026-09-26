param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyFind='visible'
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/compact-color/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory((Join-Path $run 'profile'))|Out-Null
function Paint { (Model).state.colors.foreground|ConvertTo-Json -Compress }
function Shape([string]$Shape){
    if((Model).color_panel.shape -eq $Shape){return}
    $space=switch($Shape){circle{'Okhsv'} square{'HSV'} triangle{'HLS'}}
    $paint=Paint;Invoke "Use $space $Shape" -Name
    Wait-Until {(Model).color_panel.shape -eq $Shape} "Shape did not change to $Shape"
    if((Paint) -ne $paint){throw 'Changing picker projection changed the paint'}
}
function Point([double]$X,[double]$Y){
    $bounds=(Control 'color-wheel').Current.BoundingRectangle
    @([int][Math]::Round($bounds.X+$X*$bounds.Width),[int][Math]::Round($bounds.Y+$Y*$bounds.Height))
}
try{
    Enter-CapyEnvironment
    $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'profile'
    $env:CAPY_TRACE_UI='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
    # The smoke command strip covers the bottom swatches at this window size.
    # This fixture uses native controls only; keep the isolated profile and all
    # foreground/point ownership checks, without the unrelated overlay.
    Remove-Item Env:CAPY_SMOKE_TEST -ErrorAction SilentlyContinue
    $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError (Join-Path $run 'stderr.log')
    $null=$review.Handle
    [IO.File]::WriteAllText((Join-Path $run 'pid.txt'),[string]$review.Id)
    Write-Output "Compact color review $($review.Id), $run"
    Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero -and (Model).brush_ready -and (Model).windows_workspace.ready} 'Color fixture did not start' 60
    $root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
    [CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
    [CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)|Out-Null
    [CapyRowPointer]::Initialize([uint32]$review.Id)
    $panel=(Control 'color-panel').Current.BoundingRectangle
    foreach($id in @('color-background','color-foreground','color-transparent','color-swap','color-shape-0','color-shape-1','color-readout')){
        $bounds=(Control $id).Current.BoundingRectangle
        if($bounds.Left -lt $panel.Left-1 -or $bounds.Top -lt $panel.Top-1 -or $bounds.Right -gt $panel.Right+1 -or $bounds.Bottom -gt $panel.Bottom+1){
            throw "Compact color control is clipped: $id"
        }
    }
    $document=(Model).state.document_file|ConvertTo-Json -Compress
    $retained=(Control 'color-shape-0').GetRuntimeId() -join ':'
    foreach($shape in @('circle','square','triangle')){
        Shape $shape
        $mode=(Model).color_panel.readout
        $paint=Paint;Invoke 'color-readout'
        Wait-Until {(Model).color_panel.readout -ne $mode} 'Readout did not toggle'
        if((Paint) -ne $paint){throw 'Readout toggle changed paint'}
        Invoke 'color-readout'
        Wait-Until {(Model).color_panel.readout -eq $mode} 'Readout did not restore'
        $deviceIndex=0
        foreach($device in @('mouse','pen','touch')){
            $before=Paint;$start=Point .5 .5;$end=Point (.56+.02*$deviceIndex) (.46-.01*$deviceIndex)
            [CapyRowPointer]::Down($device,$start[0],$start[1])
            [CapyRowPointer]::Move($end[0],$end[1])
            [CapyRowPointer]::Up()
            Wait-Until {(Paint) -ne $before} "$device did not pick the $shape field"
            # This ring point is beneath the readout's bounding box. Its curved
            # native hit area must leave the hue ring available for every device.
            $hue=(Model).color_panel.wheel_components[0];$angle=@(-135,-120,-150)[$deviceIndex]*[Math]::PI/180
            $ring=Point (.5+.455*[Math]::Cos($angle)) (.5+.455*[Math]::Sin($angle))
            [CapyRowPointer]::Down($device,$ring[0],$ring[1]);[CapyRowPointer]::Up()
            Wait-Until {[Math]::Abs((Model).color_panel.wheel_components[0]-$hue) -gt 1} "$device readout intercepted the $shape hue ring"
            $start=Point .5 .5
            [CapyRowPointer]::Down($device,$start[0],$start[1]);[CapyRowPointer]::Cancel();[CapyRowPointer]::Verify();$deviceIndex++
        }
        Capture $shape
    }
    if(((Control 'color-shape-0').GetRuntimeId() -join ':') -ne $retained){throw 'Color changes replaced retained shape controls'}
    $button=Control 'color-shape-0';$target=(Model).color_panel.other_shapes[0];$button.SetFocus();[CapyRowPointer]::Key(0x20)
    Wait-Until {(Model).color_panel.shape -eq $target} 'Space did not activate the focused native shape button'
    Invoke 'color-background';Wait-Until {(Model).state.colors.slot -eq 'background'} 'Background selection failed'
    Invoke 'color-transparent';Wait-Until {(Model).state.colors.slot -eq 'transparent'} 'Transparent selection failed'
    Invoke 'color-foreground';Wait-Until {(Model).state.colors.slot -eq 'foreground'} 'Foreground selection failed'
    $fg=(Model).state.colors.foreground|ConvertTo-Json -Compress
    $bg=(Model).state.colors.background|ConvertTo-Json -Compress
    Invoke 'color-swap'
    Wait-Until {((Model).state.colors.foreground|ConvertTo-Json -Compress) -eq $bg -and ((Model).state.colors.background|ConvertTo-Json -Compress) -eq $fg} 'Color swap lost paint'
    Invoke 'color-swap'
    Wait-Until {((Model).state.colors.foreground|ConvertTo-Json -Compress) -eq $fg} 'Color swap did not restore paint'
    foreach($device in @('mouse','pen','touch','keyboard')){
        $button=Control 'color-foreground';$bounds=$button.Current.BoundingRectangle
        $x=[int]($bounds.X+$bounds.Width*.5);$y=[int]($bounds.Y+$bounds.Height*.5)
        if($device -eq 'mouse'){[CapyRowPointer]::RightClick($x,$y)}
        elseif($device -eq 'keyboard'){$button.SetFocus();[CapyRowPointer]::Key(0x5D)}
        else{[CapyRowPointer]::Down($device,$x,$y);Start-Sleep -Milliseconds 1100;[CapyRowPointer]::Up()}
        Wait-Until {$null -ne (Find 'color-swatch-swap')} "$device did not open the native paint menu"
        [CapyRowPointer]::Key(0x1B)
        Wait-Until {$null -eq (Find 'color-swatch-swap')} "$device paint menu did not dismiss"
    }
    $bounds=(Control 'color-foreground').Current.BoundingRectangle
    [CapyRowPointer]::Down('mouse',[int]($bounds.X+$bounds.Width*.5),[int]($bounds.Y+$bounds.Height*.5))
    Start-Sleep -Milliseconds 1100
    if(Find 'color-swatch-swap'){throw 'Mouse hold incorrectly opened the paint menu'}
    [CapyRowPointer]::Up()
    # Partial Zen was retired in the shared UI. Exercise the retained drawer
    # through its ordinary toolbar tile, which is the current user workflow.
    $tileId=((Model).panels|Where-Object id -eq 'toolbar').tiles|Where-Object {$_.control.kind -eq 'color'}|Select-Object -ExpandProperty id
    Invoke "tile-toolbar-$tileId"
    Wait-Until {$null -ne (Find 'tool-drawer')} 'Color drawer did not open'
    $windowRoot=$root;$root=Control 'tool-drawer'
    # A visible drawer can still be moving from its opening animation.
    # Require the complete wheel to stay at its final coordinates before input.
    $settled=@{bounds='';count=0}
    Wait-Until {
        $wheel=Find 'color-wheel';if(!$wheel){return $false}
        $bounds=$wheel.Current.BoundingRectangle
        $current=$bounds.ToString()
        if($bounds.Width -gt 0 -and [Math]::Abs($bounds.Width-$bounds.Height) -lt 1 -and $current -eq $settled.bounds){$settled.count++}else{$settled.count=0}
        $settled.bounds=$current
        $settled.count -ge 3
    } 'Color drawer wheel did not finish its opening animation'
    $drawerButton=(Control 'color-shape-0').GetRuntimeId() -join ':'
    Shape circle
    $index=0
    foreach($device in @('mouse','pen','touch')){
        $before=Paint;$start=Point .5 .5;$end=Point (.54+.04*$index) .56
        [CapyRowPointer]::Down($device,$start[0],$start[1]);[CapyRowPointer]::Move($end[0],$end[1]);[CapyRowPointer]::Up()
        Wait-Until {(Paint) -ne $before} "$device could not pick in the retained Color drawer"
        Wait-Until {(Control 'color-controls').Current.ItemStatus -eq 'Ready'} "$device did not release the drawer wheel"
        $index++
    }
    Invoke 'color-readout'
    if(((Control 'color-shape-0').GetRuntimeId() -join ':') -ne $drawerButton){throw 'Drawer edits replaced retained color controls'}
    Capture 'retained-drawer'
    $root=$windowRoot
    Invoke "tile-toolbar-$tileId"
    Wait-Until {$null -eq (Find 'tool-drawer')} 'Color drawer did not close'
    if(((Model).state.document_file|ConvertTo-Json -Compress) -ne $document){throw 'Picker input painted or changed the document'}
    [CapyRowPointer]::Dispose()
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close
    if((Get-Item -LiteralPath (Join-Path $run 'stderr.log')).Length){throw 'Native color stderr requires inspection'}
    [PSCustomObject]@{
        compact_bounds='passed';shapes_and_readouts='passed';mouse_pen_touch_fields_and_ring='passed'
        cancellation='passed';retained_controls='passed';keyboard_activation='passed';paint_slots_and_swap='passed'
        document_unchanged='passed';native_context_menus='passed';mouse_hold_no_menu='passed';retained_drawer_input='passed';scope='Synthetic native input; pixel and physical digitizer acceptance remain separate'
    }|ConvertTo-Json
}catch{
    if($review -and !$review.HasExited){try{Capture 'failure'}catch{}}
    [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{
    [CapyRowPointer]::Dispose()
    Exit-CapyEnvironment
}
