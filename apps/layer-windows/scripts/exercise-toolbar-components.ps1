param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/toolbar-components/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$names=@('CAPY_SETTINGS_DIRECTORY','CAPY_TRACE_UI','CAPY_SMOKE_TEST','CAPY_TEST_DISPLAY','CAPY_TEST_PRIMARY','CAPY_PRESENT_PROBE')
$previous=@{};foreach($name in $names){$previous[$name]=[Environment]::GetEnvironmentVariable($name,'Process')}
function Model {
    try {
        if(!$script:statePath){
            foreach($path in [IO.Directory]::EnumerateFiles($directory,('ui-state-'+$review.Id+'-*.json'))){
                if([IO.File]::GetLastWriteTimeUtc($path) -lt $review.StartTime.ToUniversalTime()){continue}
                $value=Get-Content -LiteralPath $path -Raw|ConvertFrom-Json
                if($value.process_id -eq $review.Id -and $value.model.windows_isolated_settings){$script:statePath=$path;break}
            }
        }
        if($script:statePath){$value=Get-Content -LiteralPath $script:statePath -Raw|ConvertFrom-Json;if($value.process_id -eq $review.Id){return $value.model}}
    }catch{}
}
function Wait-Until([scriptblock]$Condition,[string]$Message,[int]$Seconds=8,[switch]$Closing){
    $watch=[Diagnostics.Stopwatch]::StartNew()
    do{if(& $Condition){return};$review.Refresh();if($review.HasExited){if($Closing){return};throw 'Owned toolbar review exited unexpectedly'};Start-Sleep -Milliseconds 50}while($watch.Elapsed.TotalSeconds -lt $Seconds)
    throw $Message
}
function Find([string]$Id,[switch]$Name,$Type){
    $property=if($Name){[System.Windows.Automation.AutomationElement]::NameProperty}else{[System.Windows.Automation.AutomationElement]::AutomationIdProperty}
    $condition=[System.Windows.Automation.PropertyCondition]::new($property,$Id)
    if($Type){$condition=[System.Windows.Automation.AndCondition]::new($condition,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,$Type))}
    $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$condition)
}
function Control([string]$Id,[switch]$Name,$Type){
    $hit=@{item=$null};Wait-Until {$hit.item=Find $Id -Name:$Name -Type $Type;$null -ne $hit.item -and !$hit.item.Current.IsOffscreen} "Missing native control: $Id";$hit.item
}
function Invoke([string]$Id){(Control $Id).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()}
function Box([string]$Id){
    $ready=@{box=$null}
    Wait-Until {$c=Find $Id;if(!$c -or $c.Current.IsOffscreen){return $false};$ready.box=$c.Current.BoundingRectangle;$ready.box.Width -gt 0 -and $ready.box.Height -gt 0} "Unarranged $Id"
    $ready.box
}
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
function Focus-Review{$null=[CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle);Start-Sleep -Milliseconds 120}
function Capture([string]$Name){
    & (Join-Path $PSScriptRoot 'inspect-window.ps1') -ProcessId $review.Id -Output (Join-Path $run ($Name+'.png')) -ClientOnly *> (Join-Path $run ($Name+'.json'))
    (Model)|ConvertTo-Json -Depth 80|Set-Content (Join-Path $run ($Name+'-model.json'))
}
function Switch-Workspace([string]$Name,[string]$Id){
    $switch=Find ('workspace-switch-'+$Name.ToLowerInvariant())
    if(!$switch -or $switch.Current.IsOffscreen){Invoke 'header-workspace-menu';$switch=Control $Name -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)}
    $switch.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
    Wait-Until {(Model).windows_workspace.id -eq $Id -and !(Model).windows_workspace.busy} "$Name did not open" 20
}
try {
    foreach($name in $names){Remove-Item -LiteralPath ('Env:'+$name) -ErrorAction SilentlyContinue}
    $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1'
    $stderr=Join-Path $run 'stderr.log'
    $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
    $null=$review.Handle
    Write-Output "Owned toolbar review $($review.Id): $run"
    Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero -and (Model).brush_ready -and (Model).windows_workspace.ready -and !(Model).windows_workspace.busy} 'Toolbar review did not start' 90
    $root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
    $null=[CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4));Focus-Review
    [CapyRowPointer]::Initialize([uint32]$review.Id)

    Switch-Workspace 'Sketch' 'builtin:workspace:painter'
    $size=Tile 'brush_size_slider';$opacity=Tile 'brush_opacity_slider'
    if(!$size -or !$opacity -or !$size.component -or !$opacity.component){throw 'Sketch lacks the shared brush sliders'}
    $band=@((Model).state.workspace.layout.bands)
    if($band.Count -ne 1 -or $band[0].edge -ne 'left' -or $band[0].alignment -ne 'center'){throw 'Sketch sliders are not docked in the centered left compact region'}
    Capture 'sketch-dark'

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
    Capture 'size-preview'
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

    $opacityTrack="component-slider-$($opacity.id)"
    foreach($device in @('touch','pen')){
        Focus-Review
        Press $device (Along (Box $opacityTrack) .9)
        Wait-Until {(Brush).opacity -gt .8} "$device press did not set a high opacity"
        Drag (Along (Box $opacityTrack) .9) (Along (Box $opacityTrack) .3);Release
        Wait-Until {(Brush).opacity -lt .45} "$device drag did not lower opacity"
        Wait-Until {$null -eq (Find "slider-bookmark-$($opacity.id)")} "$device preview stayed open after a drag"
        Write-Output "PASS: $device opacity drag to $([Math]::Round((Brush).opacity,2))"
    }

    $layout=(Model).state.workspace.layout|ConvertTo-Json -Depth 40 -Compress
    $cap=Box "slider-cap-$($size.id)"
    $at=@{x=[int]($cap.X+$cap.Width/2);y=[int]($cap.Y+$cap.Height/2)}
    Press 'mouse' $at;Drag $at @{x=$at.x+120;y=$at.y};Release
    if(((Model).state.workspace.layout|ConvertTo-Json -Depth 40 -Compress) -ne $layout){throw 'A quick cap drag moved the toolbar component'}
    Write-Output 'PASS: A quick cap drag without a hold does not reorder the component'

    Switch-Workspace 'Photo' 'builtin:workspace:photographer'
    $options=Tile 'tool_options'
    if(!$options -or !$options.component){throw 'Photo lacks Tool Options'}
    $commands=@((Model).panels|Where-Object id -eq 'commands')[0]
    if(@($commands.tiles)[-1].control.kind -ne 'tool_options'){throw 'Tool Options is not the last Photo command'}
    $pen=@(@((Model).panels|Where-Object id -eq 'toolbar')[0].tiles|Where-Object {$_.control.command -eq 'pen'})[0]
    Invoke "tile-toolbar-$($pen.id)"
    Wait-Until {$null -ne (Find 'toolbar-setting-size')} 'Brush Tool Options did not show a size field'
    $entry=Control 'toolbar-setting-size'
    $entry.SetFocus();$entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('37')
    [CapyRowPointer]::Key(0x0D)
    Wait-Until {[Math]::Abs((Brush).diameter-37) -lt 0.001} 'Tool Options size entry did not apply'
    Capture 'photo-brush-options'
    Invoke "toolbar-more-$($options.id)"
    Wait-Until {$null -ne (Model).state.customization.drawer} 'More did not open the Tool Options drawer'
    Capture 'photo-more-drawer'
    [CapyRowPointer]::Key(0x1B);Start-Sleep -Milliseconds 300
    Write-Output 'PASS: Photo Tool Options field edit and More drawer'

    [CapyRowPointer]::Dispose()
    $null=$review.CloseMainWindow();$lastDecision=$null
    Wait-Until {
        $review.Refresh();if($review.HasExited){return $true}
        $m=Model;$discard=Find 'Discard Changes' -Name -Type ([System.Windows.Automation.ControlType]::Button)
        if($m.windows_isolated_settings -and $discard -and $discard.Current.IsEnabled -and $lastDecision -ne $m.state.document_file.epoch){
            $discard.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke();$lastDecision=$m.state.document_file.epoch
        }
        return $false
    } 'Owned review did not close' 20 -Closing
    if($review.ExitCode -ne 0){throw "Native review exit code $($review.ExitCode)"}
    if((Get-Item $stderr).Length){throw 'Review stderr is not empty'}
    @{status='passed';captures=$run;scope='Guarded OS-delivered synthetic mouse, touch and pen input. Physical digitizers are not tested.'}|ConvertTo-Json
} catch {
    $_|Out-String|Set-Content (Join-Path $run 'failure.txt');$_.ScriptStackTrace|Add-Content (Join-Path $run 'failure.txt')
    if($review){$review.Refresh();if(!$review.HasExited){try{Capture 'failure'}catch{$_|Out-String|Set-Content (Join-Path $run 'capture-error.txt')}}};throw
} finally {
    [CapyRowPointer]::Dispose()
    if($review){$review.Refresh();if(!$review.HasExited){Stop-Process -Id $review.Id -Force}}
    foreach($name in $names){[Environment]::SetEnvironmentVariable($name,$previous[$name],'Process')}
}
