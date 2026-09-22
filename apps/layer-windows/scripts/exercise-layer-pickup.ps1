param([Parameter(Mandatory)][string]$Executable,[ValidateSet('touch','pen','mouse')][string]$Device='touch',[ValidateSet('drawers')][string]$ColumnMode='drawers')
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/layer-pickup/'+[Guid]::NewGuid().ToString('N'))
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
        $review.Refresh();if($review.HasExited){throw 'Owned layer review exited unexpectedly'}
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


function Gesture {try{(Find 'layer-list').Current.ItemStatus|ConvertFrom-Json}catch{}}
function Rows {
    @((Model).state.layers|ForEach-Object {[ordered]@{id=$_.id;depth=$_.depth;visible=$_.visible;mask_linked=$_.mask_linked}})|ConvertTo-Json -Compress
}
function Dismiss {
    [CapyRowPointer]::Key(0x1b)
    Wait-Until {!(Gesture).menu_open} 'Layer context menu did not close'
}
function Layer-History([string]$Name) {Invoke $Name -Name}
function Undo-Redo([string]$Before,[string]$After) {
    Layer-History 'Undo';Wait-Until {(Rows) -eq $Before} 'One Undo did not restore the original layer order'
    Layer-History 'Redo';Wait-Until {(Rows) -eq $After} 'One Redo did not restore the completed layer move'
    Layer-History 'Undo';Wait-Until {(Rows) -eq $Before} 'Final Undo did not restore the fixture'
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
    # Use row padding outside child buttons.
    $row=$Id -like 'layer-row-*'
    $x=if($row){$stable.bounds.X+2}else{$stable.bounds.X+$stable.bounds.Width*.5}
    $y=$stable.bounds.Y+$stable.bounds.Height*.5
    @{x=[int]$x;y=[int]$y}
}
function Move-To($At) {[CapyRowPointer]::Move([int]$At.x,[int]$At.y)}
function Tap([string]$Id) {
    $at=Point $Id
    [CapyRowPointer]::Down($Device,$at.x,$at.y)
    Start-Sleep -Milliseconds 35
    [CapyRowPointer]::Up()
}

function Focus-Key([string]$Id,[ushort]$Key) {
    (Control $Id).SetFocus()
    Wait-Until {
        $at=Point $Id
        try{
            [CapyRowPointer]::KeyAt($Key,$at.x,$at.y)
            $true
        }catch{
            # Input-pane animations can move the HWND between UIA measurement
            # and its hit test. This rejection happens before any key is sent.
            if($_.Exception.InnerException.Message -eq 'Input point is outside the owned review.'){$false}else{throw}
        }
    } 'Native text-input layout did not settle on an owned control'
}
function Destination([double]$Layer,[double]$Fraction=.6) {
    $null=Point "layer-row-$Layer"
    $bounds=(Control "layer-row-$Layer").Current.BoundingRectangle
    @{x=[int]($bounds.X+$bounds.Width*.5);y=[int]($bounds.Y+$bounds.Height*$Fraction)}
}
function Press([string]$Id,[double]$Layer,[switch]$Grip) {
    $at=Point $Id
    [CapyRowPointer]::Down($Device,$at.x,$at.y)
    Wait-Until {$g=Gesture;$g.source -eq $Layer -and $g.phase -in @('pressed','held')} 'Layer row did not receive the contact'
    $g=Gesture
    if($g.device -ne $Device -or $g.grip -ne [bool]$Grip -or $g.mask -ne ($Id -eq "layer-$Layer-mask")){throw 'Layer source or pointer device was misclassified'}
    if(!$Grip -and $g.phase -eq 'pressed' -and $g.scroll_claimed){throw 'Pending row hold stole scrolling'}
}
function Pick-Up([string]$Id,[double]$Layer,[switch]$Grip) {
    Press $Id $Layer -Grip:$Grip
    if($Device -ne 'mouse' -and !$Grip){
        Wait-Until {(Gesture).phase -eq 'held' -and (Gesture).menu_open} 'Native pen/touch row hold did not open its menu' 4
        if(!(Gesture).captured){throw 'Held row did not retain contact capture'}
    }
}
function Drag-Row([string]$Id,[double]$Layer,[double]$Target,[switch]$Grip,[switch]$Cancel,[double]$Fraction=.6) {
    $before=Rows
    $to=Destination $Target $Fraction
    if($Grip){
        $at=Point $Id
        # No UIA wait may accidentally satisfy the hold timeout.
        [CapyRowPointer]::Down($Device,$at.x,$at.y);Start-Sleep -Milliseconds 35
        Move-To $to
    }else{
        Pick-Up $Id $Layer
        Move-To $to
    }
    Wait-Until {$g=Gesture;$g.phase -eq 'dragging' -and $g.can_drop -and $g.target -eq $Target} 'Layer move did not reach a validated drop target'
    $admitted=Gesture
    if($admitted.source -ne $Layer -or $admitted.device -ne $Device -or $admitted.grip -ne [bool]$Grip){throw 'The admitted drag used another source or device'}
    if($admitted.menu_open){throw 'Dragging retained the held context menu'}
    $editing=(Model).state.layer_tools.editing_layer
    $selected=@((Model).state.layers|Where-Object id -eq $Layer)[0].selected
    if($Cancel){
        if($Device -eq 'mouse'){[CapyRowPointer]::Key(0x1b);[CapyRowPointer]::Up()}else{[CapyRowPointer]::Cancel()}
    }else{[CapyRowPointer]::Up()}
    Wait-Until {
        (Gesture).phase -eq 'idle'
    } 'Layer contact did not finish'
    # Ordinary drawers retain their row surface during the closing animation.
    if($Cancel){
        if((Rows) -ne $before){throw 'Cancelled row move changed layers'}
        if((Gesture).menu_open){throw 'Cancelled row move retained its menu'}
    }else{
        Wait-Until {(Rows) -ne $before} 'Completed layer move did not change order'
        $after=Rows
        $current=(Model).state.layer_tools.editing_layer
        if($current.id -ne $editing.id -or $current.mask_selected -ne $editing.mask_selected -or
            @((Model).state.layers|Where-Object id -eq $Layer)[0].selected -ne $selected){throw 'Drop activated a child control or changed the drawing target'}
        if(!(Gesture).last_release.commit){throw 'Released layer move was not accepted'}
        Undo-Redo $before $after
    }
}
try {
    foreach($name in $names){Remove-Item -LiteralPath ('Env:'+$name) -ErrorAction SilentlyContinue}
    $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1'
    $stderr=Join-Path $run 'stderr.log'
    $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $run 'stdout.log') -RedirectStandardError $stderr
    $null=$review.Handle
    @{process_id=$review.Id;run=$run;device=$Device}|ConvertTo-Json|Set-Content (Join-Path $repo 'artifacts/windows/layer-pickup-review.json')
    Write-Output "Owned layer pickup review $($review.Id) ($Device)"
    Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero -and (Model).brush_ready -and (Model).windows_workspace.ready -and !(Model).windows_workspace.busy} 'Layer review did not start' 45
    $root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
    $null=[CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))
    $null=[CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)
    [CapyRowPointer]::Initialize([uint32]$review.Id)
    # Current Paint defaults retain Layers in an open collapsed-column stack.
    # Begin with a fully expanded column before the fixture tests collapse/expand.
    if(Find 'column-icon-layers'){
        $at=Point 'column-icon-layers';[CapyRowPointer]::RightClick($at.x,$at.y)
        Invoke 'Expand column' -Name
        Wait-Until {$null -ne (Find 'panel-tab-layers') -and !(Find 'column-icon-layers')} 'Initial Layers column did not expand'
    }
    $paint=(Model).state.layer_tools.editing_layer.id
    $count=(Model).state.layers.Count;Invoke 'layer-new'
    Wait-Until {(Model).state.layers.Count -eq $count+1} 'New layer did not appear'
    $created=(Model).state.layer_tools.editing_layer.id
    $script:case='visibility-short-click'
    Tap "layer-$created-visibility"
    Wait-Until {!@((Model).state.layers|Where-Object id -eq $created)[0].visible} 'Short click did not toggle visibility'
    Tap "layer-$created-visibility"
    Wait-Until {@((Model).state.layers|Where-Object id -eq $created)[0].visible} 'Second short click did not restore visibility'
    $before=Rows
    $script:case='row-hold-release'
    Press "layer-$created-visibility" $created
    if($Device -eq 'mouse'){
        Start-Sleep -Milliseconds 800
        if((Gesture).menu_open -or (Gesture).phase -eq 'held'){throw 'Stationary mouse row hold opened a menu'}
        [CapyRowPointer]::Key(0x1b);[CapyRowPointer]::Up()
    }else{
        Wait-Until {(Gesture).phase -eq 'held' -and (Gesture).menu_open} 'Row hold did not show its context menu'
        [CapyRowPointer]::Up()
        Wait-Until {(Gesture).phase -eq 'idle'} 'Held release retained a pointer'
        if(!(Gesture).menu_open){throw 'Held release dismissed its menu'}
        Dismiss
    }
    if((Rows) -ne $before){throw 'Held/cancelled release activated visibility'}
    if($Device -ne 'mouse'){
        $script:case='early-row-motion'
        $at=Point "layer-$created-name";$to=Destination $paint
        # Keep UIA calls out of the timing-critical sequence.
        [CapyRowPointer]::Down($Device,$at.x,$at.y);Start-Sleep -Milliseconds 35
        Move-To $to;[CapyRowPointer]::Up()
        Wait-Until {(Gesture).phase -eq 'idle'} 'Early motion retained a contact'
        if((Rows) -ne $before -or (Gesture).menu_open){throw 'Early row motion reordered layers or opened a menu'}
    }
    $script:case='row-drag';Drag-Row "layer-$created-name" $created $paint
    $script:case='row-whitespace';Drag-Row "layer-row-$created" $created $paint
    $script:case='child-control-drag';Drag-Row "layer-$created-visibility" $created $paint
    $script:case='immediate-grip';Drag-Row "layer-$created-drag" $created $paint -Grip
    $script:case='row-cancel';Drag-Row "layer-$created-name" $created $paint -Cancel
    $script:case='keyboard-context'
    Focus-Key "layer-$created-name" 0x5d
    Wait-Until {(Gesture).menu_open} 'Keyboard context action did not open the layer menu'
    Dismiss
    if($Device -eq 'mouse'){
        $script:case='secondary-context'
        $at=Point "layer-$created-name"
        [CapyRowPointer]::RightClick($at.x,$at.y)
        Wait-Until {(Gesture).menu_open} 'Secondary click did not open the layer menu'
        Dismiss
    }
    # Reuse the same row controller after native panel reparenting and drawer
    # creation. Workspace history is independent of each document reorder.
    # Verify collapse/expand before taking the history baseline. Both
    # presentations then exercise the same row controller and one-step Undo.
    Focus-Key 'panel-tab-layers' 0x5d;Invoke 'Collapse column' -Name
    Start-Sleep -Milliseconds 300
    $column=@((Model).layout.collapsed|Where-Object {($_.groups.icons.panel) -contains 'layers'})[0].id
    $at=Point 'column-icon-layers';[CapyRowPointer]::RightClick($at.x,$at.y)
    $individual=(Control 'Open individual panels' -Name).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern)
    if($individual.Current.ToggleState -ne [System.Windows.Automation.ToggleState]::On){
        $individual.Toggle()
        Wait-Until {@((Model).state.workspace.layout.column_stacks|Where-Object column -eq $column)[0].drawers} 'Individual panel drawers did not enable'
        $at=Point 'column-icon-layers';[CapyRowPointer]::RightClick($at.x,$at.y)
    }
    Invoke 'Expand column' -Name
    Wait-Until {$null -ne (Find 'panel-tab-layers')} 'Column mode setup did not restore Layers'
    $layoutBefore=(Model).layout|ConvertTo-Json -Depth 80 -Compress
    foreach($presentation in @('floating','drawer')){
        $script:case="$presentation-setup"
        if($presentation -eq 'floating'){
            $at=Point 'panel-tab-layers';$to=Point 'Drawing canvas' -Name
            # Arrange the host with mouse input; each row below is tested with
            # the requested device. Tab tear-off has its own acceptance fixture.
            [CapyRowPointer]::Down('mouse',$at.x,$at.y);Start-Sleep -Milliseconds 35
            Move-To $to
            Wait-Until {
                try{$g=(Find 'Drawing workspace' -Name).Current.HelpText|ConvertFrom-Json;$g.phase -eq 'dragging'}catch{$false}
            } 'Layer tab did not begin its native drag'
            [CapyRowPointer]::Up()
            Wait-Until {@((Model).layout.groups|Where-Object {$_.panels -contains 'layers' -and $_.floating}).Count -eq 1} 'Layer tab did not tear off into a floating panel'
        }else{
            Focus-Key 'panel-tab-layers' 0x5d
            Invoke 'Collapse column' -Name
            Wait-Until {$tile=Find 'column-icon-layers';$tile -and !$tile.Current.IsOffscreen} 'Collapsed column did not expose Layers'
            Invoke 'column-icon-layers'
            Wait-Until {@((Model).state.customization.column_drawers|Where-Object {$_.anchor.origin -eq 'layers'}).Count -eq 1} 'Layer column drawer did not open'
        }
        $null=Point "layer-$created-name"
        $script:case="$presentation-row";Drag-Row "layer-$created-name" $created $paint
        $script:case="$presentation-whitespace";Drag-Row "layer-row-$created" $created $paint
        $script:case="$presentation-child";Drag-Row "layer-$created-visibility" $created $paint
        $script:case="$presentation-grip";Drag-Row "layer-$created-drag" $created $paint -Grip
        $script:case="$presentation-cancel";Drag-Row "layer-$created-name" $created $paint -Cancel
        if($presentation -eq 'drawer' -and @((Model).state.customization.column_drawers).Count -eq 0){
            # Row cancellation can consume Escape before workspace chrome.
            # Reopen only if the containing drawer was dismissed too.
            Invoke 'column-icon-layers'
            Wait-Until {@((Model).state.customization.column_drawers|Where-Object {$_.anchor.origin -eq 'layers'}).Count -eq 1} 'Layer drawer did not reopen after cancellation'
        }
        $script:case="$presentation-release"
        $before=Rows
        Pick-Up "layer-$created-visibility" $created
        if($Device -eq 'mouse'){
            Start-Sleep -Milliseconds 800
            if((Gesture).menu_open){throw 'Mouse row hold opened a presentation menu'}
            [CapyRowPointer]::Key(0x1b);[CapyRowPointer]::Up()
        }else{
            [CapyRowPointer]::Up()
            Wait-Until {(Gesture).phase -eq 'idle' -and (Gesture).menu_open} 'Held presentation release did not retain its menu'
            Dismiss
        }
        if((Rows) -ne $before){throw 'Held presentation release activated visibility'}
        if($presentation -eq 'drawer'){
            if(@((Model).state.customization.column_drawers).Count){Invoke 'column-icon-layers'}
            Wait-Until {@((Model).state.customization.column_drawers).Count -eq 0} 'Layer drawer did not close'
        }
        & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'window'
        Invoke 'undo_workspace'
        Wait-Until {((Model).layout|ConvertTo-Json -Depth 80 -Compress) -eq $layoutBefore} 'Workspace Undo did not restore docked Layers'
        $null=Point "layer-$created-name"
    }
    $script:case='remaining-child-controls'
    Invoke "layer-$created-name";Invoke 'layer-add-mask'
    Wait-Until {@((Model).state.layers|Where-Object id -eq $created)[0].has_mask} 'Mask did not appear for child pickup tests'
    Invoke "layer-$created-content"
    foreach($child in @('selection','content','mask','link')){
        Drag-Row ("layer-$created-"+$child) $created $paint
    }
    Invoke 'layer-new-group'
    Wait-Until {
        $active=(Model).state.layer_tools.editing_layer
        $active.group -and $active.id -ne $created -and $null -ne (Find ("layer-"+$active.id+"-content"))
    } 'New group did not appear'
    $group=(Model).state.layer_tools.editing_layer.id
    $script:case='group-content'
    Drag-Row "layer-$group-content" $group $paint
    if(@((Model).state.layers|Where-Object id -eq $group)[0].collapsed){throw 'Group content drag activated the collapse button'}
    Invoke "layer-$group-content"
    Wait-Until {@((Model).state.layers|Where-Object id -eq $group)[0].collapsed} 'Group did not collapse before an into-group drop'
    $script:case='drop-into-group'
    Drag-Row "layer-$created-name" $created $group -Fraction .5
    if(@((Model).state.layers|Where-Object id -eq $group)[0].collapsed){throw 'Dropping into a group did not expose its child'}
    $script:case='source-removal'
    Invoke "layer-$created-name"
    $before=Rows
    Pick-Up "layer-$created-name" $created
    Invoke 'layer-delete'
    Wait-Until {!@((Model).state.layers|Where-Object id -eq $created).Count -and (Gesture).phase -eq 'idle'} 'Deleting the source did not cancel its contact'
    [CapyRowPointer]::Up()
    if((Gesture).menu_open){throw 'Source removal left a context menu'}
    Layer-History 'Undo';Wait-Until {(Rows) -eq $before} 'Undo did not restore the removed source'
    $script:case='native-scrolling'
    for($i=0;$i -lt 12;$i++){
        $count=(Model).state.layers.Count;Invoke 'layer-new'
        Wait-Until {(Model).state.layers.Count -eq $count+1} 'Overflow layer did not appear'
    }
    $scroll=(Control 'layer-list').GetCurrentPattern([System.Windows.Automation.ScrollPattern]::Pattern)
    if(!$scroll.Current.VerticallyScrollable){throw 'Layer fixture did not overflow its native list'}
    $scroll.SetScrollPercent(-1,0)
    Wait-Until {$scroll.Current.VerticalScrollPercent -lt .1} 'Native list did not return to its top'
    if($Device -ne 'mouse'){
        $viewport=(Control 'layer-list').Current.BoundingRectangle
        $visible=@((Model).state.layers|Where-Object {
            $row=Find ("layer-row-"+$_.id)
            $row -and !$row.Current.IsOffscreen -and $row.Current.BoundingRectangle.Y -gt $viewport.Y+25 -and $row.Current.BoundingRectangle.Bottom -lt $viewport.Bottom-10
        })
        if(!$visible.Count){throw 'No interior layer row is visible for the scrolling test'}
        $middle=$visible[[int][Math]::Floor($visible.Count/2)].id
        $at=Point "layer-$middle-name"
        $before=Rows
        [CapyRowPointer]::Down($Device,$at.x,$at.y)
        # Allow InteractionTracker to consume movement before release. A single
        # teleport followed immediately by Up need not produce a scroll frame.
        # All movement still precedes a stationary hold; no UIA calls intervene.
        foreach($dy in @(25,55,90)){
            Start-Sleep -Milliseconds 35
            Move-To @{x=$at.x;y=[int][Math]::Max($viewport.Y+4,$at.y-$dy)}
        }
        Start-Sleep -Milliseconds 35
        [CapyRowPointer]::Up()
        Wait-Until {$scroll.Current.VerticalScrollPercent -gt .5} 'Early pen/touch row movement did not scroll the native list'
        Wait-Until {(Gesture).phase -eq 'idle'} 'Native scrolling retained a reorder contact'
        if((Rows) -ne $before -or (Gesture).menu_open){throw 'Native scrolling changed the document or opened a menu'}
    }
    $script:case='edge-scroll-retention'
    $scroll.SetScrollPercent(-1,0)
    Wait-Until {$scroll.Current.VerticalScrollPercent -lt .1} 'Native list did not return to its top'
    $latest=(Model).state.layer_tools.editing_layer.id
    $at=Point "layer-$latest-drag"
    $viewport=(Control 'layer-list').Current.BoundingRectangle
    $before=Rows
    [CapyRowPointer]::Down($Device,$at.x,$at.y);Start-Sleep -Milliseconds 35
    Move-To @{x=[int]($viewport.X+$viewport.Width*.5);y=[int]($viewport.Bottom-4)}
    Wait-Until {(Gesture).phase -eq 'dragging' -and $scroll.Current.VerticalScrollPercent -gt 60} 'Accepted drag did not keep scrolling after its source left the viewport'
    if((Gesture).source -ne $latest -or !(Gesture).captured){throw 'Auto-scroll lost the original source or capture'}
    if($Device -eq 'mouse'){[CapyRowPointer]::Key(0x1b);[CapyRowPointer]::Up()}else{[CapyRowPointer]::Cancel()}
    Wait-Until {(Gesture).phase -eq 'idle'} 'Edge-scroll cancellation retained a contact'
    if((Rows) -ne $before){throw 'Canceled edge-scroll changed layer order'}
    $script:case='minimize-cancellation'
    $scroll.SetScrollPercent(-1,0)
    Wait-Until {$scroll.Current.VerticalScrollPercent -lt .1} 'Native list did not return to its top after cancellation'
    Pick-Up "layer-$latest-name" $latest
    $held=Gesture
    $window=$root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern)
    $window.SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Minimized)
    try{
        Wait-Until {$g=Gesture;$g.phase -eq 'idle' -and !$g.menu_open -and $g.last_cancel.generation -ge $held.generation} 'Minimizing left the row contact or menu active'
        [CapyRowPointer]::Cancel()
    }finally{
        [CapyRowPointer]::Dispose()
        $window.SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Normal)
        $null=[CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)
        [CapyRowPointer]::Initialize([uint32]$review.Id)
    }
    if((Rows) -ne $before){throw 'Minimizing committed the row gesture'}
    # Finish pointer arbitration before text input: the system touch keyboard
    # legitimately changes the HWND and viewport while opening or closing.
    $scroll.SetScrollPercent(-1,100)
    Wait-Until {$scroll.Current.VerticalScrollPercent -gt 99.9} 'Native list did not reveal its original rows'
    $script:case='rename-ownership'
    Focus-Key "layer-$created-name" 0x71
    Wait-Until {$field=Find "layer-$created-rename";$field -and !$field.Current.IsOffscreen} 'F2 did not start native name editing'
    $field=Control "layer-$created-rename"
    $field.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('Renamed layer fixture')
    $at=Point "layer-$created-rename"
    [CapyRowPointer]::Down($Device,$at.x,$at.y);Start-Sleep -Milliseconds 35
    Move-To @{x=$at.x+12;y=$at.y};[CapyRowPointer]::Up()
    if((Gesture).phase -ne 'idle'){throw 'Layer pickup stole input from the native rename field'}
    Focus-Key "layer-$created-rename" 0x0d
    Wait-Until {(Control "layer-$created-name").Current.Name -eq 'Renamed layer fixture'} 'Enter did not commit the native rename'
    Focus-Key "layer-$created-name" 0x71
    Wait-Until {$field=Find "layer-$created-rename";$field -and !$field.Current.IsOffscreen} 'Rename did not reopen'
    (Control "layer-$created-rename").GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('Canceled name')
    Focus-Key "layer-$created-rename" 0x1b
    Wait-Until {(Control "layer-$created-name").Current.Name -eq 'Renamed layer fixture' -and !(Find "layer-$created-name").Current.IsOffscreen} 'Escape did not cancel only the rename'
    [CapyRowPointer]::Dispose()
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved
    if((Get-Item -LiteralPath $stderr).Length){throw 'Native stderr requires review'}
    [pscustomobject]@{device=$Device;column_mode=$ColumnMode;short_click='passed';hold_release='passed';row_drag='passed';child_control_drag='passed';immediate_grip='passed';cancel='passed';one_step_undo_redo='passed';row_whitespace='passed';rename_ownership='passed';source_removal='passed';mask_and_selection_children='passed';group_content_and_into_drop='passed';keyboard_context='passed';edge_scroll_retention='passed';minimize_cancellation='passed';native_scroll=$(if($Device -eq 'mouse'){'not_applicable'}else{'passed'});floating_and_drawer_rows='passed';scope='OS-delivered synthetic input; physical-device and presentation-performance acceptance remain separate'}|ConvertTo-Json
}catch{
    $failure=$_
    @{case=$script:case;error=$failure.ToString();gesture=(Gesture);rows=(Rows);workspace_presentation=(Find 'Drawing workspace' -Name).Current.ItemStatus}|ConvertTo-Json -Depth 10|Set-Content (Join-Path $run 'failure.json')
    if($review -and !$review.HasExited){
        & (Join-Path $PSScriptRoot 'inspect-window.ps1') -ProcessId $review.Id -Output (Join-Path $run 'failure.png') -ClientOnly *> (Join-Path $run 'failure-window.json')
    }
    throw $failure
}finally{
    if($Device -eq 'mouse' -and [CapyRowPointer]::Active){try{[CapyRowPointer]::Key(0x1b)}catch{}}
    [CapyRowPointer]::Dispose()
    foreach($name in $names){
        if($null -eq $previous[$name]){Remove-Item -LiteralPath ('Env:'+$name) -ErrorAction SilentlyContinue}
        else{[Environment]::SetEnvironmentVariable($name,$previous[$name],'Process')}
    }
}
