param([Parameter(Mandatory)][string]$Executable,[ValidateSet('touch','pen','mouse')][string]$Device='touch',[string]$DebuggerPath,[ValidateSet('drawers')][string]$ColumnMode='drawers')
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/workspace-pickup/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$names=@('CAPY_SETTINGS_DIRECTORY','CAPY_TRACE_UI','CAPY_SMOKE_TEST','CAPY_TEST_DISPLAY','CAPY_TEST_PRIMARY','CAPY_PRESENT_PROBE')
$previous=@{}
foreach($name in $names){$previous[$name]=[Environment]::GetEnvironmentVariable($name,'Process')}
function Read-Snapshot([string]$Path){
    $stream=[IO.File]::Open($Path,[IO.FileMode]::Open,[IO.FileAccess]::Read,([IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete))
    $reader=[IO.StreamReader]::new($stream)
    try{$reader.ReadToEnd()}finally{$reader.Dispose()}
}
function Model {
    try {
        if(!$script:statePath){
            # Per-window snapshots use atomic replacement. The compatibility
            # ui-state.json path is a direct write and can be read mid-frame.
            foreach($candidate in [IO.Directory]::EnumerateFiles($directory,("ui-state-"+$review.Id+"-*.json"))){
                if([IO.File]::GetLastWriteTimeUtc($candidate) -lt $review.StartTime.ToUniversalTime()){continue}
                $initial=Read-Snapshot $candidate|ConvertFrom-Json
                if($initial.process_id -eq $review.Id -and $initial.model.windows_isolated_settings){
                    $script:statePath=$candidate;$script:windowId=$initial.window_id;break
                }
            }
        }
        if(!$script:statePath){return}
        $value=Read-Snapshot $script:statePath|ConvertFrom-Json
        if($value.process_id -eq $review.Id -and $value.window_id -eq $script:windowId -and $value.model.windows_isolated_settings){$value.model}
    }catch{}
}
function Wait-Until([scriptblock]$Predicate,[string]$Message,[int]$Seconds=8){
    $watch=[Diagnostics.Stopwatch]::StartNew()
    do {
        if(& $Predicate){return}
        $review.Refresh();if($review.HasExited){throw 'Owned workspace review exited unexpectedly'}
        Start-Sleep -Milliseconds 50
    }while($watch.Elapsed.TotalSeconds -lt $Seconds)
    throw $Message
}
function Find([string]$Value,[switch]$Name,$Within=$root,$Type){
    $property=if($Name){[System.Windows.Automation.AutomationElement]::NameProperty}else{[System.Windows.Automation.AutomationElement]::AutomationIdProperty}
    $condition=[System.Windows.Automation.PropertyCondition]::new($property,$Value)
    if($Type){$condition=[System.Windows.Automation.AndCondition]::new($condition,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,$Type))}
    $candidates=$Within.FindAll([System.Windows.Automation.TreeScope]::Descendants,$condition)
    $found=$null
    foreach($candidate in $candidates){
        if(!$candidate.Current.IsOffscreen){$found=$candidate;break}
    }
    if(!$found -and $candidates.Count){$found=$candidates[0]}
    if(!$found -and $Within -eq $root){
        # A cascaded WinUI menu can live in a separate UIA fragment.
        # Search only this fixture's owned process, including its popup HWNDs.
        $owned=[System.Windows.Automation.AndCondition]::new($condition,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty,$review.Id))
        $found=[System.Windows.Automation.AutomationElement]::RootElement.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$owned)
    }
    $found
}
function Control([string]$Value,[switch]$Name,$Within=$root,$Type){
    $hit=@{item=$null}
    Wait-Until {$hit.item=Find $Value -Name:$Name -Within $Within -Type $Type;$null -ne $hit.item} "Missing $Value"
    $hit.item
}
function Invoke([string]$Value,[switch]$Name,$Within=$root){
    (Control $Value -Name:$Name -Within $Within).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
}

function Gesture {
    $workspace=Find 'Drawing workspace' -Name
    if($workspace){try{$workspace.Current.HelpText|ConvertFrom-Json}catch{}}
}
function Presentation {
    $workspace=Find 'Drawing workspace' -Name
    if($workspace){try{$workspace.Current.ItemStatus|ConvertFrom-Json}catch{}}
}
function Layout { (Model).state.workspace.layout|ConvertTo-Json -Depth 90 -Compress }
function Settled-Layout {
    $state=@{value=$null;count=0}
    Wait-Until {
        $next=Layout
        if($next -eq $state.value){$state.count++}else{$state.value=$next;$state.count=0}
        $state.count -ge 3
    } 'Workspace layout did not settle'
    $state.value
}
function Intact([string]$Expected) {
    $actual=Settled-Layout
    if($actual -ne $Expected){
        [IO.File]::WriteAllText((Join-Path $run 'expected-layout.json'),$Expected)
        [IO.File]::WriteAllText((Join-Path $run 'actual-layout.json'),$actual)
        throw 'Incomplete gesture changed workspace layout'
    }
}
function Point([string]$Id,[switch]$Name) {
    $stable=@{bounds=$null;count=0}
    Wait-Until {
        $item=Find $Id -Name:$Name
        if(!$item -or $item.Current.IsOffscreen){return $false}
        $bounds=$item.Current.BoundingRectangle
        if($bounds.IsEmpty -or $bounds.Width -le 0 -or $bounds.Height -le 0){return $false}
        if($bounds -eq $stable.bounds){$stable.count++}else{$stable.bounds=$bounds;$stable.count=0}
        $stable.count -ge 2
    } "Source $Id did not arrange visibly"
    @{x=[int]($stable.bounds.X+$stable.bounds.Width*.5);y=[int]($stable.bounds.Y+$stable.bounds.Height*.5)}
}
function Move-To($At) {[CapyRowPointer]::Move([int]$At.x,[int]$At.y)}
function Tap([string]$Id) {
    $at=Point $Id
    [CapyRowPointer]::Down($Device,$at.x,$at.y)
    Start-Sleep -Milliseconds 35
    [CapyRowPointer]::Up()
}
function Drifting-Taps {
    # A short button tap may drift past drag slop while remaining in its bounds.
    # Disarm reordering without swallowing the native Button.Click. Keep the
    # pen in range between contacts to cover physical tablet hover routing.
    $script:case='drifting-short-taps'
    $before=Settled-Layout
    foreach($choice in @(1,2,1,2)){
        $at=Point ('tile-toolbar-'+$choice)
        if($Device -eq 'pen'){[CapyRowPointer]::PenHover($at.x,$at.y);Start-Sleep -Milliseconds 35}
        [CapyRowPointer]::Down($Device,$at.x,$at.y)
        Start-Sleep -Milliseconds 35
        [CapyRowPointer]::Move($at.x+8,$at.y)
        Start-Sleep -Milliseconds 35
        [CapyRowPointer]::Up($Device -eq 'pen')
        Wait-Until {@((Model).panels|Where-Object id -eq 'toolbar')[0].tiles[$choice-1].selected} 'Small movement inside a button swallowed its short tap'
        Wait-Until {(Gesture).phase -eq 'idle' -and !(Gesture).ignore_click} 'Short tap left gesture suppression active'
        Intact $before
    }
}
function Use-Drawers([string]$Id) {
    $at=Point $Id
    [CapyRowPointer]::RightClick($at.x,$at.y)
    $item=Control 'Open individual panels' -Name
    $toggle=$item.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern)
    if($toggle.Current.ToggleState -eq [System.Windows.Automation.ToggleState]::Off){$toggle.Toggle()}
    Dismiss
}
function Source-Matches($Source,[string]$Id) {
    $tile=[regex]::Match($Id,'^(?:zen-)?tile-(.+)-(\d+)$')
    if($tile.Success){return $Source.kind -eq 'tile' -and $Source.panel -eq $tile.Groups[1].Value -and $Source.tile -eq [int]$tile.Groups[2].Value}
    $panel=[regex]::Match($Id,'^(?:column-icon-|ribbon-grip-)(.+)$')
    $panel.Success -and $Source.kind -eq 'panel' -and $Source.panel -eq $panel.Groups[1].Value
}
function Press([string]$Id,[switch]$Immediate) {
    $at=Point $Id
    $prior=(Gesture).generation
    [CapyRowPointer]::Down($Device,$at.x,$at.y)
    Wait-Until {$g=Gesture;$g.generation -gt $prior -and $g.phase -in @('pressed','held')} 'Native source did not receive pointer down'
    $hit=Gesture
    if(!(Source-Matches $hit.source $Id)){throw "Pointer hit another workspace source instead of $Id"}
    if($hit.device -ne $Device -or $hit.requires_hold -eq [bool]$Immediate){throw 'Native source misclassified the device or pickup surface'}
    if(!$Immediate -and $hit.phase -eq 'pressed' -and $hit.scroll_claimed){throw 'Tile stole native scrolling before hold'}
    $at
}
function Hold([string]$Id) {
    $null=Press $Id
    Wait-Until {(Gesture).phase -eq 'held'} "$Device did not reach a native stationary hold" 4
    $held=Gesture
    if(!$held.captured){throw 'Held contact did not transfer to stable workspace capture'}
    if($Device -eq 'mouse'){
        if($held.menu_open){throw 'Mouse hold opened a menu'}
    }else{
        Wait-Until {(Gesture).menu_open} 'Touch/pen hold did not show its native menu'
    }
    $held|ConvertTo-Json -Depth 8|Set-Content (Join-Path $run ($script:case+'-held.json'))
}
function Dismiss {
    [CapyRowPointer]::Key(0x1b)
    Wait-Until {!(Gesture).menu_open} 'Native context menu did not close'
}
function WindowCommand([string]$Id) {
    # Keep mouse hover away from the compact flyout while UIA opens its submenu.
    $bounds=(Control 'Drawing canvas' -Name).Current.BoundingRectangle
    [CapyRowPointer]::Hover([int]($bounds.Right-16),[int]($bounds.Bottom-16))
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'window'
    Invoke $Id
}
function Toggle-Zen {
    $command=@((Model).state.commands|Where-Object id -eq 'zen_mode')[0]
    $button=Find $command.label -Name -Type ([System.Windows.Automation.ControlType]::Button)
    if($button){$button.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()}
    else{(Control 'Drawing canvas' -Name).SetFocus();[CapyRowPointer]::Key(0x09)}
}
function Undo-Redo([string]$Before,[string]$After) {
    WindowCommand 'undo_workspace'
    Wait-Until {(Layout) -eq $Before} 'One Undo did not restore the original workspace'
    WindowCommand 'redo_workspace'
    Wait-Until {(Layout) -eq $After} 'One Redo did not restore the completed move'
    WindowCommand 'undo_workspace'
    Wait-Until {(Layout) -eq $Before} 'Final Undo did not restore the fixture'
}
function Early-Motion([string]$Id,$To) {
    $before=Settled-Layout
    $at=Point $Id
    $prior=(Gesture).generation
    # Do not block on UIA between these frames: this motion must precede hold.
    [CapyRowPointer]::Down($Device,$at.x,$at.y)
    Start-Sleep -Milliseconds 35
    Move-To $To
    [CapyRowPointer]::Up()
    Wait-Until {$g=Gesture;$g.phase -eq 'idle' -and $g.last_cancel.generation -gt $prior} 'Early motion did not retire the pending hold'
    $terminal=Gesture
    if(!(Source-Matches $terminal.last_cancel.source $Id)){throw "Early motion hit another workspace source instead of $Id"}
    if($terminal.last_cancel.reason -notin @('motion_before_hold','capture_lost','pointer_canceled','native_scroll')){throw 'Early motion ended for an unrelated reason'}
    if($terminal.menu_open){throw 'Early motion left a menu'}
    $terminal|ConvertTo-Json -Depth 8|Set-Content (Join-Path $run ($script:case+'-early.json'))
    Intact $before
}
function Held-Release([string]$Id) {
    $before=Settled-Layout
    Hold $Id
    [CapyRowPointer]::Up()
    Wait-Until {(Gesture).phase -eq 'idle'} 'Held release retained a pointer'
    if($Device -ne 'mouse'){
        if(!(Gesture).menu_open){throw 'Held release dismissed its menu'}
        Dismiss
    }elseif((Gesture).menu_open){throw 'Mouse release opened a menu'}
    Intact $before
}
function Held-Blur([string]$Id) {
    $before=Settled-Layout
    Hold $Id
    $held=Gesture
    $window=$root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern)
    $window.SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Minimized)
    try {
        Wait-Until {$g=Gesture;$g.phase -eq 'idle' -and !$g.menu_open -and $g.last_cancel.generation -ge $held.generation} 'Minimizing left the held contact or menu active'
        [CapyRowPointer]::Cancel()
    }finally{
        $window.SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Normal)
        $null=[CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)
    }
    Intact $before
}
function Held-Drag([string]$Id,$To,[switch]$Cancel) {
    $before=Settled-Layout
    Hold $Id
    Move-To $To
    Wait-Until {
        $g=Gesture
        if($g.phase -ne 'dragging' -or $g.menu_open){return $false}
        if($g.source.kind -eq 'tile'){return $g.can_drop}
        # Empty canvas accepts floating panels without a docking indicator.
        # Require the shared live floating placement, not just native capture.
        $null -ne (Presentation).workspace_update.drag.group
    } 'Same-contact movement did not close the menu and reach a shared drop or floating placement'
    $moving=Gesture
    if($Cancel){
        if($Device -eq 'mouse'){[CapyRowPointer]::Key(0x1b);[CapyRowPointer]::Up()}
        else{[CapyRowPointer]::Cancel()}
        Wait-Until {$g=Gesture;$g.phase -eq 'idle' -and $g.last_cancel.generation -ge $moving.generation} 'Canceled drag did not receive a fresh native terminal event'
        if((Gesture).menu_open){throw 'Canceled drag left a menu'}
        Intact $before
    }else{
        [CapyRowPointer]::Up()
        Wait-Until {(Gesture).phase -eq 'idle' -and (Layout) -ne $before} 'Completed drag did not change the shared layout'
        $after=Settled-Layout
        Undo-Redo $before $after
    }
    (Gesture)|ConvertTo-Json -Depth 8|Set-Content (Join-Path $run ($script:case+'-terminal.json'))
}
try {
    foreach($name in $names){Remove-Item -LiteralPath ('Env:'+$name) -ErrorAction SilentlyContinue}
    $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'profile'
    $env:CAPY_TRACE_UI='1'
    $stderr=Join-Path $run 'stderr.log'
    $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $run 'stdout.log') -RedirectStandardError $stderr
    $null=$review.Handle
    @{process_id=$review.Id;run=$run;device=$Device}|ConvertTo-Json|Set-Content (Join-Path $repo 'artifacts/windows/workspace-pickup-review.json')
    Write-Output "Owned workspace pickup review $($review.Id) ($Device)"
    Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero -and (Model).brush_ready -and (Model).windows_workspace.ready -and !(Model).windows_workspace.busy} 'Workspace review did not start' 45
    $root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
    if($DebuggerPath){
        $DebuggerPath=(Resolve-Path -LiteralPath $DebuggerPath).Path
        $debugCommands=Join-Path $run 'debugger.txt'
        $debugLog=Join-Path $run 'debugger.log'
        $symbols=Join-Path $repo 'artifacts/windows/symbols'
        [IO.Directory]::CreateDirectory($symbols)|Out-Null
        @(
            ('.symfix "'+$symbols+'"')
            ('.sympath+ "'+$directory+'"')
            '.symopt+ 0x10'
            'sxe -c ".ecxr; kv 60; .dump /ma crash.dmp; q" av'
            '.echo CAPY_DEBUGGER_READY'
            'g'
        )|Set-Content -LiteralPath $debugCommands
        $debugger=Start-Process -FilePath $DebuggerPath -WorkingDirectory $run -WindowStyle Hidden -PassThru -ArgumentList @(
            '-p',[string]$review.Id,'-G','-cf',('"'+$debugCommands+'"'),'-logo',('"'+$debugLog+'"')
        ) -RedirectStandardOutput (Join-Path $run 'debugger-console.log') -RedirectStandardError (Join-Path $run 'debugger-stderr.log')
        $null=$debugger.Handle
        @{process_id=$review.Id;debugger_id=$debugger.Id;run=$run;device=$Device}|ConvertTo-Json|Set-Content (Join-Path $repo 'artifacts/windows/workspace-pickup-review.json')
        Wait-Until {(Test-Path -LiteralPath $debugLog) -and (Select-String -LiteralPath $debugLog -Pattern '^CAPY_DEBUGGER_READY' -Quiet)} 'Console debugger did not attach' 30
    }
    if(Find 'Test stroke' -Name){throw 'Workspace fixture requires the production editor'}
    $null=[CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))
    $null=[CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)
    [CapyRowPointer]::Initialize([uint32]$review.Id)
    $null=Settled-Layout
    $script:case='canvas-native-cursor'
    $canvas=(Control 'Drawing canvas' -Name).Current.BoundingRectangle
    $model=Model;$area=$model.layout.work_area;$density=$canvas.Width/$model.layout.viewport[0]
    $canvasX=[int]($canvas.X+($area.x+$area.width*.5)*$density)
    $canvasY=[int]($canvas.Y+($area.y+$area.height*.5)*$density)
    [CapyRowPointer]::Hover($canvasX,$canvasY)
    Wait-Until {![CapyRowPointer]::CursorVisible()} 'Canvas left the Windows mouse cursor visible'
    $at=Point 'tile-toolbar-2'
    [CapyRowPointer]::Hover($at.x,$at.y)
    Wait-Until {[CapyRowPointer]::CursorVisible()} 'Toolbar did not restore the Windows cursor'
    $script:case='short-click'
    Tap 'tile-toolbar-2'
    Wait-Until {@((Model).panels|Where-Object id -eq 'toolbar')[0].tiles[1].selected} 'Short pointer tap did not activate Pencil'
    Drifting-Taps
    $destination=Point 'tile-toolbar-4'
    $script:case='toolbar-early';Early-Motion 'tile-toolbar-1' $destination
    $script:case='toolbar-release';Held-Release 'tile-toolbar-1'
    $script:case='toolbar-drag';Held-Drag 'tile-toolbar-1' $destination
    $script:case='toolbar-cancel';Held-Drag 'tile-toolbar-1' $destination -Cancel
    $script:case='divider-early';Early-Motion 'tile-toolbar-9' $destination
    $script:case='divider-release';Held-Release 'tile-toolbar-9'
    $script:case='divider-drag';Held-Drag 'tile-toolbar-9' $destination
    $script:case='divider-keyboard'
    (Control 'tile-toolbar-9').SetFocus()
    [CapyRowPointer]::Key(0x5d)
    Wait-Until {(Gesture).menu_open} 'Divider did not expose its keyboard context menu'
    Dismiss
    $disabled='tile-commands-25'
    if((Control $disabled).Current.IsEnabled){throw 'Undo must be disabled in the empty review document'}
    $disabledDestination=Point 'tile-commands-28'
    $script:case='disabled-early';Early-Motion $disabled $disabledDestination
    $script:case='disabled-release';Held-Release $disabled
    $script:case='disabled-drag';Held-Drag $disabled $disabledDestination
    if((Control $disabled).Current.IsEnabled){throw 'Customization unexpectedly enabled a disabled command'}
    $script:case='immediate-grip'
    $before=Settled-Layout
    $at=Point 'ribbon-grip-toolbar'
    $gripDestination=Point 'Drawing canvas' -Name
    [CapyRowPointer]::Down($Device,$at.x,$at.y)
    Start-Sleep -Milliseconds 35
    Move-To $gripDestination
    Wait-Until {(Gesture).phase -eq 'dragging'} 'Toolbar grip did not start immediately'
    if((Gesture).requires_hold){throw 'Toolbar grip incorrectly requires a hold'}
    if((Gesture).menu_open){throw 'Immediate grip drag opened a menu'}
    if($Device -eq 'mouse'){[CapyRowPointer]::Key(0x1b);[CapyRowPointer]::Up()}else{[CapyRowPointer]::Cancel()}
    Wait-Until {(Gesture).phase -eq 'idle'} 'Grip cancel retained a pointer'
    Intact $before
    $script:case='floating-toolbar'
    $at=Point 'ribbon-grip-toolbar'
    [CapyRowPointer]::Down($Device,$at.x,$at.y)
    Start-Sleep -Milliseconds 35
    Move-To $gripDestination
    Wait-Until {(Gesture).phase -eq 'dragging' -and $null -ne (Presentation).workspace_update.drag.group} 'Toolbar did not reach shared floating placement'
    [CapyRowPointer]::Up()
    Wait-Until {@((Model).layout.groups|Where-Object {$_.panels -contains 'toolbar' -and $_.floating}).Count -eq 1} 'Toolbar grip did not commit a floating panel'
    Drifting-Taps
    $destination=Point 'tile-toolbar-4'
    $script:case='floating-early';Early-Motion 'tile-toolbar-1' $destination
    $script:case='floating-release';Held-Release 'tile-toolbar-1'
    $script:case='floating-drag';Held-Drag 'tile-toolbar-1' $destination
    WindowCommand 'undo_workspace'
    Wait-Until {(Layout) -eq $before} 'One Undo did not restore the toolbar dock'
    $script:case='zen-visibility'
    $before=Settled-Layout
    Toggle-Zen
    Wait-Until {(Model).chrome_hidden} 'Zen did not hide chrome'
    Intact $before
    Toggle-Zen
    Wait-Until {!(Model).chrome_hidden} 'Zen did not restore chrome'
    Intact $before
    $script:case='collapsed-column'
    $at=Point 'panel-tab-sizes'
    [CapyRowPointer]::RightClick($at.x,$at.y)
    if(Find 'Collapse column' -Name){Invoke 'Collapse column' -Name}
    elseif(Find 'Expand column' -Name){Dismiss}
    else{throw 'Column menu did not expose its collapse state'}
    Wait-Until {$null -ne (Find 'column-icon-sizes')} 'Panel context did not collapse its column'
    Use-Drawers 'column-icon-sizes'
    Tap 'column-icon-sizes'
    Wait-Until {@((Model).state.customization.column_drawers).Count -gt 0} 'Short icon tap did not open its drawer'
    Tap 'column-icon-sizes'
    Wait-Until {@((Model).state.customization.column_drawers).Count -eq 0} 'Short icon tap did not close its drawer'
    $at=Point 'column-icon-sizes'
    $destination=Point 'Drawing canvas' -Name
    $script:case='column-early';Early-Motion 'column-icon-sizes' $destination
    $script:case='column-release';Held-Release 'column-icon-sizes'
    $script:case='column-drag';Held-Drag 'column-icon-sizes' $destination
    $script:case='column-cancel';Held-Drag 'column-icon-sizes' $destination -Cancel
    $script:case='toolbar-drawer'
    $group=@((Model).layout.groups|Where-Object {$_.panels -contains 'properties'})[0]
    $at=Point ("group-grip-"+$group.id)
    [CapyRowPointer]::RightClick($at.x,$at.y)
    (Control 'Add Toolbar' -Name).GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    (Control 'Tools toolbar' -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
    Wait-Until {@((Model).layout.groups|Where-Object {$_.id -eq $group.id -and $_.panels -contains 'toolbar'}).Count -eq 1} 'Group menu did not place Tools in the column'
    $at=Point 'panel-tab-toolbar'
    [CapyRowPointer]::RightClick($at.x,$at.y)
    if(Find 'Collapse column' -Name){Invoke 'Collapse column' -Name}
    elseif(Find 'Expand column' -Name){Dismiss}
    else{throw 'Column menu did not expose its collapse state'}
    Wait-Until {$null -ne (Find 'column-icon-toolbar')} 'Toolbar context did not collapse its column'
    Use-Drawers 'column-icon-toolbar'
    Start-Sleep -Milliseconds 300
    Tap 'column-icon-toolbar'
    Wait-Until {$null -ne (Find 'drawer-panel-toolbar')} 'Collapsed toolbar did not open its contents'
    Drifting-Taps
    $destination=Point 'tile-toolbar-4'
    $script:case='drawer-early';Early-Motion 'tile-toolbar-1' $destination
    $script:case='drawer-release';Held-Release 'tile-toolbar-1'
    $script:case='drawer-drag';Held-Drag 'tile-toolbar-1' $destination
    $script:case='nested-drawer'
    # History dismisses transient drawers but retains attached group panels.
    # Reopen only when the current native presentation actually needs it.
    $null=Point 'column-icon-toolbar'
    if(!(Find 'drawer-panel-toolbar')){Tap 'column-icon-toolbar'}
    Wait-Until {$null -ne (Find 'drawer-panel-toolbar')} 'Toolbar contents did not return after workspace history'
    $toolbar=@((Model).panels|Where-Object id -eq 'toolbar')[0]
    $color=@($toolbar.tiles|Where-Object {$_.control.kind -eq 'color'})[0]
    Tap ('tile-toolbar-'+$color.id)
    Wait-Until {$null -ne (Model).state.customization.drawer -and $null -ne (Find 'tool-drawer')} 'Color tile did not open its nested tool drawer'
    if(!@((Model).state.customization.column_drawers).Count){throw 'Opening a nested tool drawer discarded its toolbar column drawer'}
    # The color drawer can cover the other tiles. Its exposed origin remains
    # a tile source; holding it must not activate the ordinary close toggle.
    Held-Release ('tile-toolbar-'+$color.id)
    if($null -eq (Model).state.customization.drawer){throw 'Held release activated the nested drawer origin'}
    $script:case='held-blur';Held-Blur ('tile-toolbar-'+$color.id)
    if((Model).state.document_file.modified){throw 'Workspace gestures unexpectedly modified the drawing'}
    [CapyRowPointer]::Dispose()
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close
    if((Get-Item -LiteralPath $stderr).Length){throw 'Native stderr requires review'}
    [pscustomobject]@{device=$Device;column_mode=$ColumnMode;canvas_cursor='passed';short_click='passed';drifting_taps='passed';early_rejection='passed';hold_release_menu='passed';same_contact_drag='passed';immediate_grip='passed';native_cancellation='passed';one_step_undo_redo='passed';collapsed_icons='passed';floating_tiles='passed';divider_tiles='passed';disabled_commands='passed';drawer_tiles='passed';nested_drawer='passed';zen_visibility='passed';divider_keyboard='passed';held_blur='passed';native_submenu='passed';zero_exit='passed';scope='OS-delivered synthetic input; physical devices, full presentation matrix and timing remain separate'}|ConvertTo-Json
}catch{
    $failure=$_
    @{case=$script:case;error=$failure.ToString();gesture=(Gesture)}|ConvertTo-Json -Depth 10|Set-Content (Join-Path $run 'failure.json')
    if($review -and !$review.HasExited){
        & (Join-Path $PSScriptRoot 'inspect-window.ps1') -ProcessId $review.Id -Output (Join-Path $run 'failure.png') -ClientOnly *> (Join-Path $run 'failure-window.json')
    }
    throw $failure
}finally{
    [CapyRowPointer]::Dispose()
    foreach($name in $names){
        if($null -eq $previous[$name]){Remove-Item -LiteralPath ('Env:'+$name) -ErrorAction SilentlyContinue}
        else{[Environment]::SetEnvironmentVariable($name,$previous[$name],'Process')}
    }
}
