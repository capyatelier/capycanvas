param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyWaitSeconds=20
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$run=Join-Path $repo ('artifacts/windows/retouching/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$CapyTraceDirectory=$run
$ControlType=[System.Windows.Automation.ControlType]
$checks=[ordered]@{}
function Draft{(Model).state.layer_tools.frequency_separation}
function Layers{@((Model).state.layers)}
function Command([string]$Id){(Model).state.commands|Where-Object id -eq $Id|Select-Object -First 1}
function Menu-Item([string]$Id){
    $condition=[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,$Id)
    foreach($item in [System.Windows.Automation.AutomationElement]::RootElement.FindAll([System.Windows.Automation.TreeScope]::Descendants,$condition)){
        if($item.Current.ProcessId -eq $review.Id -and !$item.Current.IsOffscreen){return $item}
    }
}
function Menu-Command([string]$Menu,[string]$Id,[string]$Submenu=''){
    Wait-Until {(Command $Id).enabled} "Command $Id is not enabled"
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name $Menu
    if($Submenu){
        $parent=@{item=$null};Wait-Until {$parent.item=Menu-Item $Submenu;$parent.item} "$Menu menu has no $Submenu"
        $parent.item.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    }
    $entry=@{item=$null};Wait-Until {$entry.item=Menu-Item $Id;$entry.item} "$Menu menu has no $Id"
    $entry.item.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
}
function Undo{(Control 'drawing-canvas').SetFocus();[CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),0x5A)}
function Open-Separation{
    Menu-Command 'Filter' 'frequency_separation'
    Wait-Until {(Draft) -and (Find 'frequency-separation-value')} 'Frequency Separation did not open its preview panel'
}
try {
    Enter-CapyEnvironment
    $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1'
    $review=Start-Process -FilePath $Executable -WorkingDirectory $run -PassThru -RedirectStandardError (Join-Path $run 'stderr.log')
    $null=$review.Handle
    Write-Output "Owned retouching review $($review.Id): $run"
    $watch=[Diagnostics.Stopwatch]::StartNew()
    do{$review.Refresh();if($review.HasExited){throw 'Review exited at startup'};Start-Sleep -Milliseconds 100}while($review.MainWindowHandle -eq [IntPtr]::Zero -and $watch.Elapsed.TotalSeconds -lt 45)
    $root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
    Wait-Until {(Model).brush_ready -and (Model).windows_workspace.ready -and !(Model).windows_workspace.busy} 'Retouching review did not start' 45
    $root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Maximized)
    [CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
    [CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)|Out-Null
    [CapyRowPointer]::Initialize([uint32]$review.Id)
    Wait-Until {$b=(Find 'drawing-canvas').Current.BoundingRectangle;$b.Width -gt 1200} 'Maximized canvas did not settle' 10
    Fit-Canvas;Start-Sleep -Milliseconds 300

    $canvas=(Control 'drawing-canvas').Current.BoundingRectangle
    $x=[int]($canvas.X+$canvas.Width*.4);$y=[int]($canvas.Y+$canvas.Height*.45)
    $revision=(Model).state.document_file.revision
    [CapyRowPointer]::Down('mouse',$x,$y)
    try{for($i=1;$i -le 30;$i++){[CapyRowPointer]::Move($x+$i*8,$y+[int]([Math]::Sin($i/4)*30));Start-Sleep -Milliseconds 8}}finally{[CapyRowPointer]::Up()}
    Wait-Until {(Model).state.document_file.revision -gt $revision -and (Model).state.document_file.modified} 'The seed stroke did not finish'
    Start-Sleep -Milliseconds 1500
    Capture 'stroke' -Composed
    $before=(Layers).Count

    $painting=(Model).state.layer_tools.tool
    $focused=Control 'settings-button';$focused.SetFocus()
    Wait-Until {$focused.Current.HasKeyboardFocus} 'The settings button did not take focus'
    [CapyRowPointer]::Hold(0x12,$true)
    try{Wait-Until {(Model).state.layer_tools.tool -like 'pick_*'} 'Holding Alt on a focused button did not pick colors'}
    finally{[CapyRowPointer]::Hold(0x12,$false)}
    Wait-Until {(Model).state.layer_tools.tool -eq $painting} 'Releasing Alt did not return to the painting tool'
    $checks.alt_hold_with_focused_button='passed'

    Menu-Command 'Layer' 'new_dodge_burn_layer' 'menu-new'
    Wait-Until {(Layers).Count -eq $before+1} 'New Dodge & Burn Layer did not add a layer'
    Undo
    Wait-Until {(Layers).Count -eq $before} 'Undo did not remove the Dodge & Burn layer in one step'
    $checks.dodge_burn_layer='passed'

    Open-Separation
    $panel=Control 'frequency-separation-panel'
    $scale=[CapyRowPointer]::GetDpiForWindow($review.MainWindowHandle)/96
    if((Find 'frequency-separation-panel').Current.BoundingRectangle.Width/$scale -gt 361){throw 'The preview panel is wider than the shared panel width'}
    if(!(Control 'drawing-canvas').Current.IsEnabled){throw 'Frequency Separation blocked the canvas'}
    $radius=[int](Draft).radius+6
    $value=Control 'frequency-separation-value' -Type $ControlType::Edit
    $value.SetFocus()
    $value.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue([string]$radius)
    [CapyRowPointer]::Key([uint32]$review.Id,0x0D)
    Wait-Until {(Draft) -and [Math]::Abs((Draft).radius-$radius) -lt .01} 'The radius did not reach the shared draft'
    Start-Sleep -Milliseconds 1500
    Capture 'separation-open' -Composed
    $checks.separation_panel_edits_radius='passed'

    $panel.SetFocus()
    [CapyRowPointer]::Key([uint32]$review.Id,0x1B)
    Wait-Until {!(Draft) -and !(Find 'frequency-separation-panel' -Visible)} 'Escape did not cancel Frequency Separation'
    if((Layers).Count -ne $before){throw 'Cancel changed the layers'}
    Wait-Until {(Control 'drawing-canvas').Current.HasKeyboardFocus -or [System.Windows.Automation.AutomationElement]::FocusedElement.Current.AutomationId -notlike 'frequency-separation-*'} 'Cancel left focus in the hidden panel'
    $checks.separation_escape_cancels='passed'

    Open-Separation
    Invoke 'frequency-separation-apply'
    Wait-Until {!(Draft) -and (Layers).Count -gt $before} 'Apply did not insert the separated layers' 30
    $after=(Layers).Count
    Undo
    Wait-Until {(Layers).Count -eq $before} 'Undo did not remove the separated layers in one step'
    $checks.separation_apply_undo="passed ($($after-$before) layers)"

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
