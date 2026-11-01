param([Parameter(Mandatory)][string]$Executable,[ValidateSet('mouse','pen','touch')][string]$Device='pen',[ValidateSet('dark','light')][string]$Theme='dark',[ValidateSet('all','header','preset','tool','panel','palette','layer')][string]$Family='all')
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
if($env:CAPY_PRIVATE_DESKTOP){[CapyRowPointer]::VerifyPrivateDesktop($env:CAPY_PRIVATE_DESKTOP)}
$CapyFind='prefer-visible';$CapyPopups=$true
$CapyEach={if($env:CAPY_PRIVATE_DESKTOP){[CapyRowPointer]::VerifyPrivateDesktop($env:CAPY_PRIVATE_DESKTOP)};[CapyRowPointer]::Verify()}
$newCaption='New'+[char]0x2026
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$run=Join-Path $repo ('artifacts/windows/input-activation/fixture-'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$cancellationMethods=@{header=$(if($Device -eq 'touch'){'canceled-UP'}else{'Escape+normal-UP'});palette=$(if($Device -eq 'pen'){'synthetic-device-destruction'}elseif($Device -eq 'touch'){'canceled-UP'}else{'Escape+normal-UP'});layer=$(if($Device -eq 'pen'){'synthetic-device-destruction'}elseif($Device -eq 'touch'){'canceled-UP'}else{'outside-release-only'})}
@{executable=$Executable;sha256=(Get-FileHash -LiteralPath $Executable -Algorithm SHA256).Hash;device=$Device;theme=$Theme;family=$Family;desktop=$env:CAPY_PRIVATE_DESKTOP;cancellation_methods=$cancellationMethods}|ConvertTo-Json|Set-Content -LiteralPath (Join-Path $run 'invocation.json')
$directory=$run
$CapyTraceDirectory=$run
$cases=[Collections.Generic.List[object]]::new()
function Save-Cases {$cases|ConvertTo-Json -Depth 15|Set-Content -LiteralPath (Join-Path $run 'cases.json')}
function Ready {Wait-Until {(Model).brush_ready -and (Model).windows_workspace.ready -and !(Model).windows_workspace.busy} 'Activation target workspace is not ready' 45}
function Target-State([string]$Id,$Element=$null){
    if($Element){try{$bounds=$Element.Current.BoundingRectangle;if($Element.Current.IsOffscreen -or $bounds.IsEmpty -or $bounds.Width -le 0 -or $bounds.Height -le 0){$Element=$null}}catch [System.Windows.Automation.ElementNotAvailableException]{$Element=$null}}
    $target=if($Element){$Element}else{Control $Id}
    $projection=if($Id -like 'tool-subtool-*'){@((Model).state.tool_set.subtools)[[int]($Id.Replace('tool-subtool-',''))]}else{$null}
    @{runtime_id=@($target.GetRuntimeId());name=$target.Current.Name;enabled=$target.Current.IsEnabled;bounds=$target.Current.BoundingRectangle.ToString();item_status=$target.Current.ItemStatus;help_text=$target.Current.HelpText;has_keyboard_focus=$target.Current.HasKeyboardFocus;projection=$projection}
}
function Native-HeaderCancel {
    if($Device -eq 'pen'){[CapyRowPointer]::PenLeave()}
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'window'
    Invoke 'customize_workspace_ui'
    Wait-Until {(Model).header.editing} 'Header cancellation setup did not open customization'
    $source=(Control 'header-component-space' -Arranged).Current.BoundingRectangle
    $origin=(Control 'title-bar').Current.BoundingRectangle
    $zone=((Control 'title-bar').Current.ItemStatus|ConvertFrom-Json).geometry.zones[1]
    $x=[int]($source.X+$source.Width/2);$y=[int]($source.Y+$source.Height/2)
    $toX=[int]($origin.X+($zone.x+$zone.width/2)*$scale);$toY=[int]($origin.Y+($zone.y+$zone.height/2)*$scale)
    $script:lastInput=@{id='header-component-space';point=@($x,$y);target=@($toX,$toY);cancellation=$(if($Device -eq 'touch'){'canceled-up'}else{'Escape/release'})}
    [CapyRowPointer]::Down($Device,$x,$y);[CapyRowPointer]::Move($toX,$toY)
    Wait-Until {$gesture=(Control 'title-bar').Current.HelpText|ConvertFrom-Json;$gesture.phase -eq 'dragging' -and $gesture.preview.target} 'Header cancellation did not reach an admitted placement preview'
    if($Device -eq 'touch'){[CapyRowPointer]::Cancel()}else{[CapyRowPointer]::Key(0x1b);[CapyRowPointer]::Up($Device -eq 'pen')}
}
function Native-Tap([string]$Id,[double]$Drift,[string]$End='up',[switch]$Disabled,[ValidateSet('diagonal','vertical','horizontal')][string]$Direction='diagonal') {
    if($env:CAPY_PRIVATE_DESKTOP){[CapyRowPointer]::VerifyPrivateDesktop($env:CAPY_PRIVATE_DESKTOP)}
    $stable=@{bounds=$null;count=0;item=$null}
    Wait-Until {
        $element=$stable.item
        if(!$element){$element=Find $Id;$stable.item=$element}
        $script:lastInput=@{id=$Id;found=($null -ne $element)}
        if(!$element){return $false}
        try{$current=$element.Current;$box=$current.BoundingRectangle;$enabled=$current.IsEnabled;$offscreen=$current.IsOffscreen}catch [System.Windows.Automation.ElementNotAvailableException]{$stable.item=$null;$stable.bounds=$null;$stable.count=0;return $false}
        $script:lastInput.target=@{enabled=$enabled;offscreen=$offscreen;bounds=$box.ToString();stable_samples=$stable.count}
        if($offscreen -or (!$Disabled -and !$enabled)){$stable.item=$null;$stable.bounds=$null;$stable.count=0;return $false}
        if($box.IsEmpty -or $box.Width -le 0 -or $box.Height -le 0){$stable.item=$null;$stable.bounds=$null;$stable.count=0;return $false}
        if($box -eq $stable.bounds){$stable.count++}else{$stable.bounds=$box;$stable.count=0}
        $stable.count -ge 2
    } "Invalid run: target $Id is not enabled and stably arranged"
    $box=$stable.bounds;$x=[int](($box.Left+$box.Right)/2);$y=[int](($box.Top+$box.Bottom)/2)
    if($Id -like 'workspace-switch-*'){
        $viewport=(Control 'workspace-switcher').Current.BoundingRectangle
        $left=[Math]::Max($box.Left,$viewport.Left);$right=[Math]::Min($box.Right,$viewport.Right-27)
        if($right-$left -lt 16){throw "Invalid run: target $Id is clipped"}
        $x=[int](($left+$right)/2)
    }
    $dx=[int]($Drift*$scale);$dy=$dx
    if($Direction -eq 'vertical'){$dx=0}elseif($Direction -eq 'horizontal'){$dy=0}
    if($x+$dx -ge $box.Right -or $y+$dy -ge $box.Bottom){throw "Invalid run: drift leaves $Id"}
    $script:lastInput=@{id=$Id;name=$stable.item.Current.Name;bounds=$box.ToString();point=@($x,$y);drift_dip=$Drift;direction=$Direction;foreground=[CapyRowPointer]::GetForegroundWindow().ToInt64();owned_window=$drawingWindow.ToInt64()}
    $script:lastInput.before_hover=Target-State $Id $stable.item
    if($Family -eq 'preset'){Capture ($script:caseName+'-before-hover') -Composed}
    if($Device -eq 'pen'){[CapyRowPointer]::PenHover($x,$y);Start-Sleep -Milliseconds 35}
    $script:lastInput.before_down=Target-State $Id $stable.item
    if($script:lastInput.before_hover.bounds -ne $box.ToString() -or $script:lastInput.before_down.bounds -ne $box.ToString()){throw "Invalid run: target $Id moved before native input"}
    if($Disabled -and $script:lastInput.before_down.enabled){throw "Invalid run: $Id became enabled before disabled-control input"}
    if($Family -eq 'preset'){Capture ($script:caseName+'-before-down') -Composed}
    if($End -eq 'context' -and $Device -eq 'mouse'){[CapyRowPointer]::RightClick($x,$y);return}
    [CapyRowPointer]::Down($Device,$x,$y);Start-Sleep -Milliseconds 35
    if($End -eq 'context'){Wait-Until {$null -ne (Find 'palette-command-rename_color' -Visible)} 'Palette hold did not open its native color menu' 5}
    if($Drift){[CapyRowPointer]::Move($x+$dx,$y+$dy)}
    Start-Sleep -Milliseconds 35
    if($End -eq 'cancel'){[CapyRowPointer]::Cancel()}
    elseif($End -eq 'outside'){
        $canvas=(Control 'drawing-canvas' -Arranged).Current.BoundingRectangle
        [CapyRowPointer]::Move([int]($canvas.X+$canvas.Width/2),[int]($canvas.Y+$canvas.Height/2));[CapyRowPointer]::Up($Device -eq 'pen')
    }else{[CapyRowPointer]::Up($Device -eq 'pen')}
    $script:lastInput.after_up=Target-State $Id $stable.item
}
function Native-PaletteDrag($Before,$Expected,[switch]$Cancel,$FinalId=$null) {
    $source=Control ('palette-swatch-'+$Before[0]) -Arranged
    $target=Control ('palette-swatch-'+$Before[2]) -Arranged
    $a=$source.Current.BoundingRectangle;$b=$target.Current.BoundingRectangle
    if($null -ne $FinalId){$final=(Control ('palette-swatch-'+$FinalId) -Arranged).Current.BoundingRectangle}
    $x=[int]($a.X+$a.Width/2);$y=[int]($a.Y+$a.Height/2);$toX=[int]($b.X+$b.Width/2);$toY=[int]($b.Y+$b.Height/2)
    $panel=Control 'palettes-panel';$grid=Control 'palette-swatches'
    $script:lastInput=@{id='palette-swatch-'+$Before[0];before=@($Before);expected_preview=@($Expected);source=Target-State ('palette-swatch-'+$Before[0]) $source;target_bounds=$b.ToString();point=@($x,$y);to=@($toX,$toY);cancel=[bool]$Cancel}
    if($Device -eq 'pen'){[CapyRowPointer]::PenHover($x,$y)}
    [CapyRowPointer]::Down($Device,$x,$y)
    if($Device -ne 'mouse'){Wait-Until {$null -ne (Find 'palette-command-rename_color' -Visible)} 'Palette hold did not open its native menu'}
    for($step=1;$step -le 12;$step++){[CapyRowPointer]::Move([int]($x+($toX-$x)*$step/12),[int]($y+($toY-$y)*$step/12));Start-Sleep -Milliseconds 15}
    Wait-Until {$script:lastInput.phase=$panel.Current.ItemStatus;$script:lastInput.preview=$grid.Current.ItemStatus;$preview=$script:lastInput.preview|ConvertFrom-Json;$script:lastInput.phase -eq 'Dragging' -and (($preview -join ',') -eq ($Expected -join ','))} 'Palette contact did not admit the exact shared drag preview'
    if($null -ne $FinalId){
        $script:lastInput.observed_preview_before_final=$script:lastInput.preview;$script:lastInput.final_target=$FinalId;$script:lastInput.final_bounds=$final.ToString()
        $script:lastInput.final_point=@([int]($final.X+$final.Width/2),[int]($final.Y+$final.Height/2));$script:lastInput.final_move_called_utc=[DateTime]::UtcNow.ToString('o');$timer=[Diagnostics.Stopwatch]::StartNew()
        [CapyRowPointer]::Move($script:lastInput.final_point[0],$script:lastInput.final_point[1]);[CapyRowPointer]::Up($Device -eq 'pen')
        $script:lastInput.final_up_returned_utc=[DateTime]::UtcNow.ToString('o');$script:lastInput.final_move_up_ms=$timer.Elapsed.TotalMilliseconds;$script:lastInput.contact_ms=[CapyRowPointer]::ContactDurationMilliseconds;$script:lastInput.max_gap_ms=[CapyRowPointer]::MaxGapMilliseconds
        return
    }
    if($Cancel){if($Device -eq 'mouse'){[CapyRowPointer]::Key(0x1b);[CapyRowPointer]::Up()}else{[CapyRowPointer]::Cancel()}}
    else{[CapyRowPointer]::Up($Device -eq 'pen')}
}
function Native-PaletteSecondaryDrag($Order) {
    $sourceId=@((Model).palette_panel.swatches|Where-Object {!$_.current -and $_.id -in @($Order|Select-Object -First 3)})[0].id
    if($null -eq $sourceId -or $Order.Count -lt 4){throw 'Invalid run: secondary palette drag requires four visible swatches and an inactive source'}
    $targetId=if($sourceId -eq $Order[0]){$Order[3]}else{$Order[0]}
    $script:lastInput=@{id=('palette-swatch-'+$sourceId);qualification=@()}
    $points=@(foreach($id in @($sourceId,$targetId)){
        $controlId='palette-swatch-'+$id;$item=Control $controlId -Arranged;$bounds=$item.Current.BoundingRectangle
        $x=[int]($bounds.X+$bounds.Width/2);$y=[int]($bounds.Y+$bounds.Height/2)
        $native=[CapyRowPointer+Point]::new();$native.x=$x;$native.y=$y;$hwnd=[CapyRowPointer]::WindowFromPoint($native);$owner=[uint32]0
        [CapyRowPointer]::GetWindowThreadProcessId($hwnd,[ref]$owner)|Out-Null
        $runtime=@($item.GetRuntimeId())
        $activated=@($cases|Where-Object {$_.passed -and $_.case -match '^palette-[013]$' -and $_.input.id -eq $controlId -and $_.input.bounds -eq $bounds.ToString() -and ($_.input.before_down.runtime_id -join ',') -eq ($runtime -join ',') -and $_.input.point[0] -eq $x -and $_.input.point[1] -eq $y})
        $qualified=@{visible=!$item.Current.IsOffscreen;inside=$bounds.Contains($x,$y);owned=($owner -eq $review.Id);foreground=([CapyRowPointer]::GetForegroundWindow() -eq $drawingWindow);activated=($activated.Count -gt 0);stable=$item.Current.BoundingRectangle.Equals($bounds)}
        $script:lastInput.qualification+=@(@{id=$controlId;bounds=$bounds.ToString();point=@($x,$y);qualified=$qualified;activation=@($activated.case)})
        if($qualified.Values -contains $false){throw 'Invalid run: secondary palette drag lacks its acknowledged native swatch target'}
        @{id=$controlId;x=$x;y=$y;bounds=$bounds.ToString();runtime_id=$runtime;hwnd=$hwnd.ToInt64();activation=@($activated.case)}
    })
    $script:lastInput=@{id=$points[0].id;button=$(if($Device -eq 'pen'){'barrel'}else{'middle'});from=$points[0];to=$points[1];order=@($Order);phase_before=(Control 'palettes-panel').Current.ItemStatus;preview_before=(Control 'palette-swatches').Current.ItemStatus}
    $previous=0L;$requestGap=0.
    try {
        if($Device -eq 'pen'){[CapyRowPointer]::PenHover($points[0].x,$points[0].y);[CapyRowPointer]::Barrel($true)}
        $script:lastInput.down_called_utc=[DateTime]::UtcNow.ToString('o')
        if($Device -eq 'pen'){[CapyRowPointer]::Down('pen',$points[0].x,$points[0].y)}else{[CapyRowPointer]::Down('mouse',$points[0].x,$points[0].y,'middle')}
        $previous=[Diagnostics.Stopwatch]::GetTimestamp()
        for($step=1;$step -le 12;$step++){
            [CapyRowPointer]::Move([int]($points[0].x+($points[1].x-$points[0].x)*$step/12),[int]($points[0].y+($points[1].y-$points[0].y)*$step/12))
            $now=[Diagnostics.Stopwatch]::GetTimestamp();$requestGap=[Math]::Max($requestGap,($now-$previous)*1000./[Diagnostics.Stopwatch]::Frequency);$previous=$now
            Start-Sleep -Milliseconds 15
        }
        [CapyRowPointer]::Up($Device -eq 'pen')
        $now=[Diagnostics.Stopwatch]::GetTimestamp();$requestGap=[Math]::Max($requestGap,($now-$previous)*1000./[Diagnostics.Stopwatch]::Frequency)
        $script:lastInput.up_returned_utc=[DateTime]::UtcNow.ToString('o');$script:lastInput.contact_ms=[CapyRowPointer]::ContactDurationMilliseconds;$script:lastInput.max_gap_ms=[CapyRowPointer]::MaxGapMilliseconds;$script:lastInput.request_max_gap_ms=$requestGap
    } finally {
        if([CapyRowPointer]::Active){[CapyRowPointer]::Cancel()}
        if($Device -eq 'pen'){[CapyRowPointer]::Barrel($false);[CapyRowPointer]::PenLeave()}
    }
    $script:lastInput.phase_after=(Control 'palettes-panel').Current.ItemStatus;$script:lastInput.preview_after=(Control 'palette-swatches').Current.ItemStatus
    $script:lastInput.context_visible_after_release=$null -ne (Find 'palette-command-rename_color' -Visible)
    if($script:lastInput.context_visible_after_release){[CapyRowPointer]::Key(0x1b);Wait-Until {!(Find 'palette-command-rename_color' -Visible)} 'Secondary palette context menu did not dismiss'}
}
function Check([string]$Name,[scriptblock]$Action,[scriptblock]$Expected) {
    $script:caseName=$Name
    $script:lastInput=$null
    $row=[ordered]@{case=$Name;device=$Device;theme=$Theme;passed=$false;expected=$script:caseExpected;before=@{brush_ready=(Model).brush_ready;brush=(Model).state.brush;active=@((Model).layout.groups.active);layers=@((Model).state.layers).Count}}
    try {
        Ready
        & $Action
        Wait-Until {(& $Expected) -and (Model).brush_ready} "$Name did not reach its expected state with a ready renderer" 15
        Start-Sleep -Milliseconds 200
        if(!(& $Expected) -or !(Model).brush_ready){throw "$Name did not retain its expected state with a ready renderer"}
        $row.passed=$true
    }catch {
        $row.error=$_.Exception.ToString()
        if($script:lastInput.id){
            try{$row.candidates=@($root.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,$script:lastInput.id))|ForEach-Object {$parent=[System.Windows.Automation.TreeWalker]::ControlViewWalker.GetParent($_);@{runtime_id=@($_.GetRuntimeId());bounds=$_.Current.BoundingRectangle.ToString();offscreen=$_.Current.IsOffscreen;enabled=$_.Current.IsEnabled;parent=$parent.Current.AutomationId}})}catch{}
        }
        try{$row.workspace_gesture=(Workspace-Root).Current.HelpText;$row.customization=(Model).state.customization;Capture ($Name+'-failure') -Composed -WithModel}catch{}
        try{if([CapyRowPointer]::Active){[CapyRowPointer]::Cancel()}}catch{}
        if(!(Model).brush_ready){$row.application_unready=$true}
        try{$row.header=(Control 'title-bar').Current.HelpText;$row.brush=(Model).state.brush;$row.workspace=(Model).windows_workspace.id;$row.menus=@([System.Windows.Automation.AutomationElement]::RootElement.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.AndCondition]::new([System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty,$review.Id),[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::MenuItem)))|ForEach-Object {@{id=$_.Current.AutomationId;name=$_.Current.Name}})}catch{}
        try{$row.zoom_readout=Target-State 'canvas-view-info';$row.zoom_buttons=@([System.Windows.Automation.AutomationElement]::RootElement.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.AndCondition]::new([System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty,$review.Id),[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::Button)))|Where-Object {$_.Current.AutomationId -like 'zoom-*' -and !$_.Current.IsOffscreen -and !$_.Current.BoundingRectangle.IsEmpty}|ForEach-Object {@{id=$_.Current.AutomationId;state=(Target-State $_.Current.AutomationId $_)}})}catch{}
        if($row.error -match 'Invalid run:|foreground input|outside the owned|keepalive|Inject|desktop must match|Win32Exception'){$row.invalid_run=$true}
        Write-Output "FAILED $Name : $($row.error)"
        throw
    }finally{$row.input=$script:lastInput;$row.actual=@{brush_ready=(Model).brush_ready;brush=(Model).state.brush;active=@((Model).layout.groups.active);layers=@((Model).state.layers).Count};$cases.Add([pscustomobject]$row);Save-Cases}
    Write-Output "$Name passed=$($row.passed)"
}
try {
    Enter-CapyEnvironment
    $env:CAPY_STORAGE_DIR=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1'
    [IO.File]::WriteAllText((Settings-File),(@{language=@{Explicit='en'};theme=$Theme}|ConvertTo-Json -Compress))
    $review=Start-Process -FilePath $Executable -WorkingDirectory $run -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $run 'stdout.log') -RedirectStandardError (Join-Path $run 'stderr.log')
    $null=$review.Handle;$native=@{window=$null}
    Wait-Until {$native.window=Owned-DrawingWindow $review;$native.window -and (Model).brush_ready -and (Model).windows_workspace.ready} 'Owned activation window did not become ready' 75
    $root=$native.window.Root;$drawingWindow=$native.window.Handle
    $null=[CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))
    $null=[CapyWindowApi]::ShowWindow($drawingWindow,3)
    $null=[CapyRowPointer]::SetForegroundWindow($drawingWindow);$canvas=Control 'drawing-canvas';$canvas.SetFocus()
    Wait-Until {$owner=[uint32]0;$null=[CapyRowPointer]::GetWindowThreadProcessId([CapyRowPointer]::GetForegroundWindow(),[ref]$owner);$owner -eq $review.Id -and $canvas.Current.HasKeyboardFocus} 'Invalid run: owned canvas did not acquire foreground and focus' 10
    $scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
    if($scale -le 0 -or [double]::IsNaN($scale) -or [double]::IsInfinity($scale)){throw 'Invalid run: owned window DPI is unavailable'}
    [CapyRowPointer]::Initialize([uint32]$review.Id)
    Write-Output "Owned activation process $($review.Id), $Device, $Theme, $run"
    if($Family -in @('all','header')){foreach($drift in @(0,1,3)){
        Ready
        $before=(Model).windows_workspace.id
        $target=@((Model).windows_workspace.switcher_display|Where-Object id -ne $before)[0].id
        $script:caseExpected=$target
        Check "workspace-$drift" {Native-Tap ('workspace-switch-'+$target) $drift} {(Model).windows_workspace.id -eq $target -and !(Model).windows_workspace.busy}
    }}
    (Control 'workspace-switch-builtin:workspace:illustrator').GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
    Wait-Until {(Model).windows_workspace.id -eq 'builtin:workspace:illustrator'} 'Paint workspace setup failed' 45
    Ready
    if($Family -in @('all','header')){foreach($drift in @(0,1,3)){
        $first=(Model).windows_workspace.order[0]
        $script:caseExpected='visible workspace visibility menu'
        Check "options-$drift" {Native-Tap 'workspace-switcher-options' $drift} {$null -ne (Find ('workspace-switcher-show-'+$first))}
        [CapyRowPointer]::Key(0x1b);Wait-Until {!(Find ('workspace-switcher-show-'+$first))} 'Options did not dismiss'
        Start-Sleep -Milliseconds 250
        $script:caseExpected='visible native File menu'
        Check "menu-$drift" {Native-Tap 'application-menu-file' $drift} {$null -ne (Find $newCaption -Name -Type ([System.Windows.Automation.ControlType]::MenuItem))}
        [CapyRowPointer]::Key(0x1b)
        Wait-Until {!(Find $newCaption -Name -Type ([System.Windows.Automation.ControlType]::MenuItem))} 'File menu did not dismiss'
        Start-Sleep -Milliseconds 250
    }}
    if($Family -in @('all','header')){
        foreach($drift in @(0,1,3)){
            $zoom=if([Math]::Abs((Model).state.camera.zoom-1) -lt .001){2}else{1};$zoomId='zoom-'+($zoom*100)
            $script:caseExpected='native zoom menu selection '+$zoom
            Check "zoom-menu-$drift" {
                Native-Tap 'canvas-view-info' $drift;$trigger=$script:lastInput
                Wait-Until {(Control 'canvas-view-info').Current.ItemStatus -eq 'Open' -and $null -ne (Find $zoomId -Visible)} 'Zoom menu did not acknowledge opening'
                Native-Tap $zoomId 0;$script:lastInput=@{trigger=$trigger;selection=$script:lastInput}
            } {[Math]::Abs((Model).state.camera.zoom-$zoom) -lt .001 -and !(Find $zoomId -Visible) -and (Control 'canvas-view-info').Current.ItemStatus -eq 'Closed'}
        }
        $headerBefore=(Model).header.model|ConvertTo-Json -Depth 40 -Compress;$script:caseExpected='unchanged authored header after native cancellation'
        Check 'header-placement-cancel' {Native-HeaderCancel} {(Model).header.editing -and (((Model).header.model|ConvertTo-Json -Depth 40 -Compress) -eq $headerBefore) -and (((Control 'title-bar').Current.HelpText|ConvertFrom-Json).phase -eq 'idle')}
        if((Model).header.editing){Invoke 'header-edit-cancel'}
        Wait-Until {!(Model).header.editing -and (((Model).header.model|ConvertTo-Json -Depth 40 -Compress) -eq $headerBefore)} 'Header cancellation setup did not restore the original header'
    }
    if($Family -in @('all','preset')){foreach($drift in @(0,1,3)){
        if(!@((Model).panels|Where-Object id -eq 'toolbar')[0].tiles[0].selected){Invoke 'tile-toolbar-1'}
        Wait-Until {@((Model).panels|Where-Object id -eq 'toolbar')[0].tiles[0].selected -and (Find 'tool-subtool-1').Current.IsEnabled -and (Model).brush_ready} 'Pen preset setup failed'
        if((Model).state.customization.drawer -or (Model).state.customization.expanded){[CapyRowPointer]::Key(0x1b)}
        Wait-Until {$null -eq (Model).state.customization.drawer -and $null -eq (Model).state.customization.expanded} 'Preset setup retained an overlapping control surface'
        $subtools=@((Model).state.tool_set.subtools);$index=0
        while($index -lt $subtools.Count -and ($subtools[$index].selected -or !$subtools[$index].enabled)){$index++}
        if($index -ge $subtools.Count){throw 'Invalid run: no inactive preset is available'}
        $preset=$subtools[$index].action.id;$script:caseExpected=$preset
        Check "preset-$drift" {Native-Tap ('tool-subtool-'+$index) $drift} {(Model).state.brush.preset -eq $preset}
    }}
    if($Family -in @('all','tool')){foreach($drift in @(0,1,3)){
        $choice=if(@((Model).panels|Where-Object id -eq 'toolbar')[0].tiles[3].selected){1}else{4}
        $script:caseExpected='selected toolbar tile '+$choice
        Check "tool-$drift" {Native-Tap ('tile-toolbar-'+$choice) $drift} {@((Model).panels|Where-Object id -eq 'toolbar')[0].tiles[$choice-1].selected}
    }}
    if($Family -in @('all','panel')){foreach($drift in @(0,1,3)){
        $panel=if(@((Model).layout.groups|Where-Object {$_.panels -contains 'sizes'})[0].active -eq 'sizes'){'tool_settings'}else{'sizes'}
        $script:caseExpected=$panel
        Check "panel-$drift" {Native-Tap ('panel-tab-'+$panel) $drift} {@((Model).layout.groups|Where-Object {$_.panels -contains $panel})[0].active -eq $panel}
    }}
    if($Family -in @('all','palette')){
    Invoke 'panel-tab-palettes';Wait-Until {@((Model).layout.groups|Where-Object {$_.panels -contains 'palettes'})[0].active -eq 'palettes'} 'Palette setup failed'
    foreach($drift in @(0,1,3)){
        $swatch=@((Model).palette_panel.swatches|Where-Object {!$_.current})[0].id
        $script:caseExpected='selected swatch '+$swatch
        Check "palette-$drift" {Native-Tap ('palette-swatch-'+$swatch) $drift} {(Find ('palette-swatch-'+$swatch)).Current.ItemStatus -eq 'Selected'}
    }
    $paletteBefore=@((Model).palette_panel.swatches.id)
    $colorBefore=(Model).state.brush.color|ConvertTo-Json -Compress
    $swatch=@((Model).palette_panel.swatches|Where-Object {!$_.current})[0].id;$script:caseExpected='unchanged paint color and swatch order after native context release and acknowledged Escape dismissal'
    Check 'palette-context-release' {
        Native-Tap ('palette-swatch-'+$swatch) 0 'context'
        Wait-Until {
            $state=Model
            $script:lastInput.context=@{menu_visible=[bool](Find 'palette-command-rename_color' -Visible);expected_color=$colorBefore;actual_color=($state.state.brush.color|ConvertTo-Json -Compress);expected_order=($paletteBefore -join ',');actual_order=($state.palette_panel.swatches.id -join ',')}
            $observed=$script:lastInput.context
            $observed.menu_visible -and $observed.actual_color -eq $observed.expected_color -and $observed.actual_order -eq $observed.expected_order
        } 'Palette context release did not preserve its visible menu, color and swatch order'
        $script:lastInput.context_visible_after_release=$true;$script:lastInput.dismissal='Escape after visible native menu'
        [CapyRowPointer]::Key(0x1b)
    } {!(Find 'palette-command-rename_color' -Visible) -and ((Model).state.brush.color|ConvertTo-Json -Compress) -eq $colorBefore -and (@((Model).palette_panel.swatches.id) -join ',') -eq ($paletteBefore -join ',')}
    if($Device -in @('pen','mouse')){
        $script:caseExpected='unchanged paint color, swatch order and custom drag presentation after secondary-button motion'
        Check $(if($Device -eq 'pen'){'palette-barrel-drag-rejected'}else{'palette-middle-drag-rejected'}) {Native-PaletteSecondaryDrag $paletteBefore} {
            ((Model).state.brush.color|ConvertTo-Json -Compress) -eq $colorBefore -and (@((Model).palette_panel.swatches.id) -join ',') -eq ($paletteBefore -join ',') -and
                (Control 'palettes-panel').Current.ItemStatus -eq $script:lastInput.phase_before -and (Control 'palette-swatches').Current.ItemStatus -eq $script:lastInput.preview_before -and !(Find 'palette-command-rename_color' -Visible)
        }
    }
    $paletteExpected=[Collections.Generic.List[long]]::new();foreach($id in $paletteBefore){$paletteExpected.Add($id)};$paletteExpected.RemoveAt(0);$paletteExpected.Insert(2,$paletteBefore[0])
    $script:caseExpected='exact shared swatch preview and reorder, restored by one Undo'
    Check 'palette-held-drag-undo' {
        Native-PaletteDrag $paletteBefore @($paletteExpected)
        Wait-Until {(@((Model).palette_panel.swatches.id) -join ',') -eq ($paletteExpected -join ',')} 'Palette release did not commit the exact shared preview'
        $script:lastInput.committed_order=@((Model).palette_panel.swatches.id)
        (Control ('palette-swatch-'+$paletteBefore[0])).SetFocus();[CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),0x5a)
    } {(@((Model).palette_panel.swatches.id) -join ',') -eq ($paletteBefore -join ',')}
    if($paletteBefore.Count -lt 4){throw 'Invalid run: final palette release requires four arranged swatches'}
    $paletteFinal=[Collections.Generic.List[long]]::new();foreach($id in $paletteBefore){$paletteFinal.Add($id)};$paletteFinal.RemoveAt(0);$paletteFinal.Insert(3,$paletteBefore[0])
    $script:caseExpected='final pointer slot replaces acknowledged earlier preview, restored by one Undo'
    Check 'palette-final-position-undo' {
        Native-PaletteDrag $paletteBefore @($paletteExpected) -FinalId $paletteBefore[3]
        $script:lastInput.expected_final_order=@($paletteFinal)
        Wait-Until {(@((Model).palette_panel.swatches.id) -join ',') -eq ($paletteFinal -join ',')} 'Palette release committed an earlier preview instead of its final pointer slot'
        $script:lastInput.committed_order=@((Model).palette_panel.swatches.id)
        (Control ('palette-swatch-'+$paletteBefore[0])).SetFocus();[CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),0x5a)
    } {(@((Model).palette_panel.swatches.id) -join ',') -eq ($paletteBefore -join ',')}
    $colorBefore=(Model).state.brush.color|ConvertTo-Json -Compress;$script:caseExpected='unchanged swatch order and paint color after canceled active drag'
    Check 'palette-drag-cancel' {Native-PaletteDrag $paletteBefore @($paletteExpected) -Cancel} {(@((Model).palette_panel.swatches.id) -join ',') -eq ($paletteBefore -join ',') -and ((Model).state.brush.color|ConvertTo-Json -Compress) -eq $colorBefore}
    if(Find 'palette-command-rename_color' -Visible){[CapyRowPointer]::Key(0x1b);Wait-Until {!(Find 'palette-command-rename_color' -Visible)} 'Canceled palette menu did not dismiss'}
    }
    if($Family -in @('all','layer')){
    $layerButton=Find 'layer-new'
    if(!$layerButton -or $layerButton.Current.IsOffscreen){Invoke 'column-icon-layers'}
    Wait-Until {$button=Find 'layer-new';$button -and !$button.Current.IsOffscreen} 'Layer drawer setup failed'
    $redo=Find 'tile-commands-27'
    if(!$redo -or $redo.Current.IsEnabled){throw 'Invalid run: expected disabled Redo control is unavailable'}
    $before=(Model).state.layers|ConvertTo-Json -Depth 40 -Compress;$script:caseExpected='unchanged layer state'
    Check 'disabled-redo' {Native-Tap 'tile-commands-27' 0 -Disabled} {((Model).state.layers|ConvertTo-Json -Depth 40 -Compress) -eq $before}
    $owner=@((Model).state.layers)[0].id
    $layerDrifts=if($Device -eq 'mouse'){@(0,1,3)}else{@(0,1,3,6)}
    foreach($drift in $layerDrifts){
        $direction=if($drift -eq 6){'vertical'}else{'diagonal'}
        $visible=@((Model).state.layers|Where-Object id -eq $owner)[0].visible;$script:caseExpected=!$visible
        Check "layer-visibility-$drift" {Native-Tap ('layer-'+$owner+'-visibility') $drift -Direction $direction} {@((Model).state.layers|Where-Object id -eq $owner)[0].visible -ne $visible}
        if(@((Model).state.layers|Where-Object id -eq $owner)[0].visible -ne $visible){Invoke 'Undo' -Name;Wait-Until {@((Model).state.layers|Where-Object id -eq $owner)[0].visible -eq $visible} 'Visibility setup undo failed'}
        $other=@((Model).state.layers|Where-Object {!$_.editing})[0].id;$script:caseExpected='editing layer '+$other
        Check "layer-name-$drift" {Native-Tap ('layer-'+$other+'-name') $drift -Direction $direction} {(Model).state.layer_tools.editing_layer.id -eq $other}
    }
    Invoke ('layer-'+$owner+'-content');Invoke 'layer-add-mask'
    Wait-Until {@((Model).state.layers|Where-Object id -eq $owner)[0].has_mask} 'Mask setup failed'
    foreach($drift in $layerDrifts){
        $direction=if($drift -eq 6){'vertical'}else{'diagonal'}
        Invoke ('layer-'+$owner+'-content');Wait-Until {!@((Model).state.layers|Where-Object id -eq $owner)[0].mask_selected} 'Mask content setup failed'
        $script:caseExpected='selected mask on layer '+$owner
        Check "layer-mask-$drift" {Native-Tap ('layer-'+$owner+'-mask') $drift -Direction $direction} {@((Model).state.layers|Where-Object id -eq $owner)[0].mask_selected}
    }
    Invoke 'Undo' -Name;Wait-Until {!@((Model).state.layers|Where-Object id -eq $owner)[0].has_mask} 'Mask setup undo failed'
    foreach($drift in @(0,1,3)){
        $count=@((Model).state.layers).Count
        $script:caseExpected=$count+1
        Check "layer-once-$drift" {Native-Tap 'layer-new' $drift} {@((Model).state.layers).Count -eq $count+1}
        Invoke 'Undo' -Name;Wait-Until {@((Model).state.layers).Count -eq $count} 'Layer setup undo failed'
    }
    $endings=if($Device -eq 'mouse'){@('outside')}else{@('cancel','outside')}
    foreach($end in $endings){
        $count=@((Model).state.layers).Count
        $script:caseExpected=$count
        Check "layer-$end" {Native-Tap 'layer-new' 0 $end;Start-Sleep -Milliseconds 300} {@((Model).state.layers).Count -eq $count}
    }
    }
    $failed=@($cases|Where-Object {!$_.passed}).Count
    [CapyRowPointer]::Dispose()
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -StateDirectory $run -Action Close -DiscardUnsaved
    if(!$review.WaitForExit(15000)){throw 'Owned activation window did not close'}
    if($review.ExitCode -ne 0){throw "Owned activation process exited $($review.ExitCode)"}
    @{device=$Device;theme=$Theme;family=$Family;passed=$cases.Count-$failed;failed=$failed;run=$run;cancellation_methods=$cancellationMethods;scope='OS-injected activation; physical digitizers, pen canceled-only-UP and latency unverified'}|ConvertTo-Json|Set-Content -LiteralPath (Join-Path $run 'result.json')
    if($failed){throw "$failed activation cases failed; see $run"}
    Write-Output "Activation passed $($cases.Count) cases: $run"
} catch {
    ($_.Exception.ToString()+[Environment]::NewLine+$_.ScriptStackTrace)|Set-Content -LiteralPath (Join-Path $run 'failure.txt')
    throw
} finally {
    try{[CapyRowPointer]::Dispose()}catch{}
    if($review -and !$review.HasExited){if($drawingWindow){$null=[CapyWindowApi]::PostMessage($drawingWindow,0x10,[UIntPtr]::Zero,[IntPtr]::Zero)};if(!$review.WaitForExit(10000)){$review.Kill();$review.WaitForExit()}}
    Exit-CapyEnvironment
}
