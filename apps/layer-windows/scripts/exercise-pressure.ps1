param([Parameter(Mandatory)][string]$Executable,[ValidateSet('dark','light')][string]$Theme='dark')
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
Add-Type -AssemblyName System.Drawing
$CapyCaptureDelay=250
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/pressure/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
function Json($Value){ConvertTo-Json -InputObject $Value -Compress -Depth 40}
function Same-Curve($Left,$Right){
    if($null -eq $Left -or $null -eq $Right -or $Left.Count -ne $Right.Count){return $false}
    for($i=0;$i -lt $Left.Count;$i++){
        if($Left[$i].Count -ne 2 -or $Right[$i].Count -ne 2){return $false}
        for($axis=0;$axis -lt 2;$axis++){if([single]$Left[$i][$axis] -ne [single]$Right[$i][$axis]){return $false}}
    }
    $true
}
function Pressure{(Model).state.pressure_calibration}
function Points{(Pressure).editor.points}
function Curve{Json (Points)}
function Status-Checkpoint([string]$Name){
    $status=Find 'canvas-status'
    @{stage=$Name;exists=$null -ne $status;offscreen=$(if($status){$status.Current.IsOffscreen}else{$true});text=$(if($status){$status.Current.Name}else{''})}|ConvertTo-Json -Compress|Add-Content (Join-Path $run 'native-status.jsonl')
    Capture $Name -WithModel
}
function Open-Pressure{
    $script:pressureLaunch++
    Invoke 'settings-button'
    Control 'Preferences' -Name -Type ([System.Windows.Automation.ControlType]::Window)|Out-Null
    Status-Checkpoint "open-$pressureLaunch-preferences"
    Invoke 'Pen & Input' -Name
    $adjust=Control 'setting-action-pen_pressure'
    $scroll=$null
    if($adjust.TryGetCurrentPattern([System.Windows.Automation.ScrollItemPattern]::Pattern,[ref]$scroll)){$scroll.ScrollIntoView()}
    Status-Checkpoint "open-$pressureLaunch-before-adjust"
    Invoke 'setting-action-pen_pressure'
    Wait-Until {(Pressure) -and !(Find 'Preferences' -Name -Type ([System.Windows.Automation.ControlType]::Window))} 'Adjust did not atomically replace Preferences with Pen pressure'
    Control 'pen-pressure-dialog' -Arranged|Out-Null
    Control 'pen-pressure-curve' -Arranged|Out-Null
    Status-Checkpoint "open-$pressureLaunch-after-adjust"
    $status=Find 'canvas-status'
    if($status -and !$status.Current.IsOffscreen -and $status.Current.Name){throw "Opening Pen pressure left a native canvas status: $($status.Current.Name)"}
}
function At([double]$X,[double]$Y){
    $r=(Control 'pen-pressure-curve' -Arranged).Current.BoundingRectangle
    $inset=(Pressure).editor.controls.inset*[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
    @([int]($r.Left+$inset+$X*($r.Width-2*$inset)),[int]($r.Top+$inset+(1-$Y)*($r.Height-2*$inset)))
}
function Drag([string]$Device,[int[]]$From,[int[]]$To,[switch]$Escape){
    [CapyRowPointer]::Down($Device,$From[0],$From[1])
    for($i=1;$i -le 8;$i++){[CapyRowPointer]::Move([int]($From[0]+($To[0]-$From[0])*$i/8),[int]($From[1]+($To[1]-$From[1])*$i/8));Start-Sleep -Milliseconds 30}
    if($Escape){[CapyRowPointer]::Key(0x1B)}
    [CapyRowPointer]::Up();Start-Sleep -Milliseconds 200
}
function Native-Stroke([string]$Device){
    $bounds=(Control 'drawing-canvas' -Arranged).Current.BoundingRectangle
    $camera=(Model).state.camera;$work=$camera.work_area;$tab=@((Model).state.tabs|Where-Object active)[0]
    if($camera.rotation -ne 0 -or $camera.flipped[0] -or $camera.flipped[1]){throw 'The stroke fixture requires the initial unrotated document camera'}
    $left=[Math]::Max($work[0],$camera.translation[0]);$right=[Math]::Min($work[0]+$work[2],$camera.translation[0]+$tab.width*$camera.zoom)
    $top=[Math]::Max($work[1],$camera.translation[1]);$bottom=[Math]::Min($work[1]+$work[3],$camera.translation[1]+$tab.height*$camera.zoom)
    if($right-$left -lt 150 -or $bottom-$top -lt 150){throw 'The visible document is too small for the authored pixel comparison'}
    $from=@([int]($bounds.X+($left+($right-$left)*.25)*$bounds.Width/$camera.viewport[0]),[int]($bounds.Y+($top+($bottom-$top)*.8)*$bounds.Height/$camera.viewport[1]))
    $to=@([int]($from[0]+40),[int]($from[1]+8))
    $panel=(Control 'pen-pressure-dialog').Current.BoundingRectangle
    if(!$bounds.Contains([double]$from[0],[double]$from[1]) -or !$bounds.Contains([double]$to[0],[double]$to[1]) -or $panel.Contains([double]$from[0],[double]$from[1]) -or $panel.Contains([double]$to[0],[double]$to[1])){throw 'Stroke projection intersects the utility or leaves the owned canvas'}
    $before=Wait-StablePixels {Screen-Pixels $from[0] $from[1]}
    [CapyRowPointer]::Down($Device,$from[0],$from[1])
    for($i=1;$i -le 8;$i++){[CapyRowPointer]::Move([int]($from[0]+($to[0]-$from[0])*$i/8),[int]($from[1]+($to[1]-$from[1])*$i/8));Start-Sleep -Milliseconds 30}
    if($Device -eq 'pen'){
        Wait-Until {$editor=(Pressure).editor;$marker=$editor.marker;$marker -and [Math]::Abs($marker[0]-.5) -lt .002 -and [Math]::Abs($marker[1]-$editor.plot[[int][Math]::Round($marker[0]*256)][1]) -lt .004} 'The live native pen marker did not match raw pressure and the mapped curve output'
        Capture 'live-pen' -WithModel
    }
    [CapyRowPointer]::Up()
    [CapyRowPointer]::Hover([int]($bounds.Left+20),[int]($bounds.Top+20))
    Wait-Until {(Model).state.document_file.modified -and ((Model).state.commands|Where-Object id -eq 'undo').enabled} "$Device could not paint while the utility remained open"
    $after=Wait-StablePixels {Screen-Pixels $from[0] $from[1]}
    if($after -eq $before){throw "$Device stroke did not change authored canvas pixels"}
    Wait-Until {$null -eq (Pressure).editor.marker} 'Pen pressure marker did not clear on release'
    Invoke 'Undo' -Name
    Wait-Until {!(Model).state.document_file.modified} "$Device stroke did not undo to the clean checkpoint"
    $restored=Wait-StablePixels {Screen-Pixels $from[0] $from[1]}
    if($restored -ne $before){throw "$Device Undo did not restore the exact sampled canvas pixels"}
}
try{
    Enter-CapyEnvironment
    $env:CAPY_STORAGE_DIR=Join-Path $run 'profile'
    [IO.File]::WriteAllText((Settings-File),(@{theme=$Theme}|ConvertTo-Json))
    $env:CAPY_TRACE_UI='1';$env:CAPY_SMOKE_TEST='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
    $stderr=Join-Path $run 'stderr.log'
    $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
    $null=$review.Handle
    $owned=@{window=$null}
    Wait-Until {$owned.window=Owned-DrawingWindow $review;$null -ne $owned.window -and (Model).brush_ready} 'Pressure review did not start' 45
    $root=$owned.window.Root;$drawingWindow=$owned.window.Handle
    if(!(Model).windows_isolated_settings){throw 'Refusing to use non-isolated preferences'}
    [CapyWindowApi]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
    Status-Checkpoint 'startup'
    [CapyRowPointer]::SetForegroundWindow($drawingWindow)|Out-Null
    [CapyRowPointer]::Initialize([uint32]$review.Id)
    Open-Pressure
    $original=Curve;$saved=Json (Model).state.settings.pressure_curve
    if(@(Points).Count -ne 3 -or (Pressure).editor.controls.coordinate_readouts -or !(Pressure).editor.controls.control_polygon){throw 'Pressure editor did not present its shared three-control graph metadata'}
    Capture 'default' -WithModel
    Invoke 'pen-pressure-lighter'
    Wait-Until {(Points)[0][1] -gt 0} 'Lighter did not raise the output floor'
    Invoke 'pen-pressure-firmer'
    Wait-Until {(Curve) -eq $original} 'Firmer did not restore the output floor'
    foreach($device in @('mouse','touch','pen')){
        $middle=(Points)[1];$before=Curve
        Drag $device (At $middle[0] $middle[1]) (At .62 .68)
        Wait-Until {(Curve) -ne $before -and [Math]::Abs((Points)[1][0]-.62) -lt .02 -and [Math]::Abs((Points)[1][1]-.68) -lt .02} "$device did not drag the interior control"
        Invoke 'pen-pressure-reset';Wait-Until {(Curve) -eq $original} "$device reset did not restore the default curve"
    }
    $middle=(Points)[1]
    Drag 'mouse' (At $middle[0] $middle[1]) (At .62 .68) -Escape
    Wait-Until {(Curve) -eq $original -and (Pressure)} 'Escape did not roll back the graph contact while retaining the utility'
    $r=(Control 'pen-pressure-curve').Current.BoundingRectangle;$scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
    Drag 'mouse' (At $middle[0] $middle[1]) @([int]($r.Right+30*$scale),[int]($r.Top+$r.Height/2))
    Wait-Until {@(Points).Count -eq 2} 'Dragging an interior point out did not delete it on release'
    Invoke 'pen-pressure-reset';Wait-Until {(Curve) -eq $original} 'Reset did not restore the deleted interior point'
    $frame=(Control 'pen-pressure-dialog').Current.BoundingRectangle;$layout=Json (Model).layout
    Drag 'touch' @([int]($frame.Left+60*$scale),[int]($frame.Top+17*$scale)) @([int]($frame.Left+20*$scale),[int]($frame.Top+27*$scale))
    Wait-Until {[Math]::Abs((Control 'pen-pressure-dialog').Current.BoundingRectangle.Left-$frame.Left) -gt 10} 'The title did not move the modeless utility'
    if((Json (Model).layout) -ne $layout){throw 'Moving the utility changed the dock layout'}
    Native-Stroke 'mouse';Native-Stroke 'pen'
    Capture 'moved-and-painted' -WithModel
    Invoke 'pen-pressure-lighter';Invoke 'pen-pressure-cancel'
    Wait-Until {$null -eq (Pressure)} 'Cancel did not close the utility'
    if((Json (Model).state.settings.pressure_curve) -ne $saved){throw 'Cancel changed the applied pressure curve'}
    Open-Pressure;$beforeApply=Curve;Invoke 'pen-pressure-lighter'
    Wait-Until {(Curve) -ne $beforeApply} 'Lighter did not publish its preview before Apply'
    $applied=@(Points);Invoke 'pen-pressure-apply'
    Wait-Until {$null -eq (Pressure) -and (Same-Curve (Model).state.settings.pressure_curve $applied)} 'Apply did not commit the calibrated curve'
    Wait-Until {Same-Curve (Read-Snapshot (Settings-File)).pressure_curve (Model).state.settings.pressure_curve} 'Apply did not persist the calibrated curve'
    Open-Pressure
    if(!(Same-Curve (Points) $applied)){throw 'Reopening did not restore the applied controls'}
    Invoke 'pen-pressure-reset';Invoke 'pen-pressure-close'
    Wait-Until {$null -eq (Pressure)} 'Close did not dismiss the utility'
    Open-Pressure
    if(!(Same-Curve (Points) $applied)){throw 'Close committed an unapplied Reset'}
    Capture 'applied' -WithModel
    Invoke 'pen-pressure-close'
    Wait-Until {$canvas=Find 'drawing-canvas' -Visible;$null -eq (Pressure) -and !(Find 'pen-pressure-dialog') -and $canvas -and $canvas.Current.IsEnabled} 'Closing Pen pressure did not restore the owned drawing canvas'
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Close -DiscardUnsaved
    if((Get-Item -LiteralPath $stderr).Length){throw 'Native pressure stderr requires inspection'}
    @{theme=$Theme;modeless_pressure='passed';graph_mouse_touch_pen='passed';drag_out_delete='passed';escape_cancel='passed';canvas_pixels_and_undo='passed';live_pen_marker='passed';apply_cancel_reset_and_storage='passed';scope='isolated injected native input on WARP; physical input, hardware rendering and presentation remain unverified';run=$run}|ConvertTo-Json
}catch{
    try{Capture 'failure' -WithModel}catch{}
    [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{
    [CapyRowPointer]::Dispose()
    Exit-CapyEnvironment
}
