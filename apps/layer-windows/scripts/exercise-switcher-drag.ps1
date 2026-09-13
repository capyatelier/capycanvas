param([Parameter(Mandatory)][string]$Executable,[ValidateSet('touch','pen','mouse')][string]$Device='touch',[switch]$Overflow)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/switcher-drag/'+[Guid]::NewGuid().ToString('N'))
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
function Storage {(Model).windows_workspace}
function Manager {(Model).windows_workspace_manager}
function Layout($Value=(Model)) {$Value.state.workspace|ConvertTo-Json -Depth 80 -Compress}
function Wait-Until([scriptblock]$Predicate,[string]$Message,[int]$Seconds=8){
    $watch=[Diagnostics.Stopwatch]::StartNew()
    do {
        if(& $Predicate){return}
        $review.Refresh();if($review.HasExited){throw 'Owned switcher review exited unexpectedly'}
        Start-Sleep -Milliseconds 50
    }while($watch.Elapsed.TotalSeconds -lt $Seconds)
    throw $Message
}
function Find([string]$Value,[switch]$Name,$Within=$root,$Type){
    $property=if($Name){[System.Windows.Automation.AutomationElement]::NameProperty}else{[System.Windows.Automation.AutomationElement]::AutomationIdProperty}
    $condition=[System.Windows.Automation.PropertyCondition]::new($property,$Value)
    if($Type){$condition=[System.Windows.Automation.AndCondition]::new($condition,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,$Type))}
    $found=$Within.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$condition)
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
function Choose([string]$Name){
    (Control $Name -Name -Within (Control 'workspace-manager') -Type ([System.Windows.Automation.ControlType]::Button)).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
}
function Settled {
    Wait-Until {$v=Manager;$s=Storage;$null -ne $v -and !$v.loading -and !$v.busy -and !$s.switcher_busy} 'Manager did not settle'
    if((Manager).error){throw (Manager).error}
    if((Storage).switcher_error){throw (Storage).switcher_error}
}
function Closed {
    Wait-Until {$null -eq (Manager) -and $null -eq (Find 'workspace-manager')} 'Manager did not close'
}
function Open-Manager {
    Closed
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'window'
    (Control 'Workspaces' -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)).GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    Invoke 'Manage Workspaces…' -Name
    Wait-Until {$null -ne (Find 'workspace-manager')} 'Manager did not open'
    Settled
}
function Preference([string]$Id,[string]$Action){
    Settled
    $revision=(Storage).switcher_revision
    Invoke ('workspace-manager-options-'+$Id)
    $item=Control ('workspace-manager-'+$Action)
    if($Action -eq 'show'){$item.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()}
    else{$item.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()}
    Wait-Until {(Storage).switcher_revision -gt $revision -or (Storage).switcher_error} 'Preference was not acknowledged'
    Settled
}
function Capture([string]$Label){
    & (Join-Path $PSScriptRoot 'inspect-window.ps1') -ProcessId $review.Id -Output (Join-Path $run ($Label+'.png')) -ClientOnly *> (Join-Path $run ($Label+'.json'))
}
function Launch([string]$Label){
    $script:statePath=$null;$script:windowId=$null
    $script:stderr=Join-Path $run ($Label+'-stderr.log')
    $script:review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
    $null=$review.Handle
    @{process_id=$review.Id;run=$run}|ConvertTo-Json|Set-Content (Join-Path $repo 'artifacts/windows/switcher-drag-review.json')
    Write-Output "Owned switcher review $($review.Id) ($Label)"
    Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero -and (Model).brush_ready -and (Storage).ready -and !(Storage).switcher_busy} 'Switcher review did not start' 45
    $script:root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
    if(Find 'Test stroke' -Name){throw 'Switcher fixture requires the production UI without smoke controls'}
    $probe=Get-Item -LiteralPath (Join-Path $directory 'presentation-probe.json') -ErrorAction SilentlyContinue
    if($probe -and $probe.LastWriteTime -ge $review.StartTime){throw 'Switcher fixture must not run a presentation probe'}
}
function Close {
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close
    if((Get-Item -LiteralPath $stderr).Length){throw 'Native stderr requires inspection'}
}
function Gesture {
    $control=Find 'workspace-manager-items'
    if($control){try{$control.Current.ItemStatus|ConvertFrom-Json}catch{}}
}
function Rows-Arranged {
    $stable=@{signature=$null;count=0}
    Wait-Until {
        $manager=Manager
        $list=Find 'workspace-manager-items'
        if(!$manager -or !$list -or $manager.loading -or $manager.busy){return $false}
        $viewport=$list.Current.BoundingRectangle;$previousTop=[double]::NegativeInfinity;$visible=@()
        foreach($row in $manager.rows){
            $item=Find ('workspace-manager-row-'+$row.id)
            if(!$item){continue}
            $bounds=$item.Current.BoundingRectangle
            if($bounds.IsEmpty -or $bounds.Height -le 0 -or $bounds.Bottom -le $viewport.Top -or $bounds.Top -ge $viewport.Bottom){continue}
            if($bounds.Top -lt $previousTop){$stable.count=0;return $false}
            $previousTop=$bounds.Top
            $visible+=($row.id+':'+$bounds.ToString())
        }
        if(!$visible.Count){return $false}
        $signature=$visible -join '|'
        if($signature -eq $stable.signature){$stable.count++}else{$stable.signature=$signature;$stable.count=0}
        $stable.count -ge 2
    } 'Native row arrangement did not match saved order'
}
function Point([string]$Id,[switch]$Grip,[switch]$Top){
    Rows-Arranged
    $control=Control ($(if($Grip){'workspace-manager-grip-'}else{'workspace-manager-row-'})+$Id)
    $b=$control.Current.BoundingRectangle
    @{x=[int]($b.X+$b.Width*$(if($Grip){.5}else{.4}));y=[int]($b.Y+$b.Height*$(if($Top){.12}else{.5}))}
}
function Press([string]$Id,[switch]$Grip){
    $at=Point $Id -Grip:$Grip
    [CapyRowPointer]::Down($Device,$at.x,$at.y)
}
function Move-To($At){
    [CapyRowPointer]::Move($At.x,$At.y)
}
function Preview-Intact {
    $current=Model
    if(!$current){throw 'Owned atomic snapshot is unavailable'}
    if($current.windows_workspace_manager.selected -ne $preview){throw 'Pointer activity changed the selected workspace row'}
    $actual=Layout $current
    if($actual -ne $previewLayout){
        [IO.File]::WriteAllText((Join-Path $run 'expected-preview.json'),$previewLayout)
        [IO.File]::WriteAllText((Join-Path $run 'actual-preview.json'),$actual)
        throw 'Pointer activity changed the workspace preview layout'
    }
}
function Order-Intact($Order,$Revision){
    $current=(Model).windows_workspace
    if(!$current){throw 'Owned atomic snapshot is unavailable'}
    if(($current.order -join '|') -ne ($Order -join '|') -or $current.switcher_revision -ne $Revision){
        @{case=$script:gestureCase;expected_order=$Order;expected_revision=$Revision;actual_order=$current.order;
            actual_revision=$current.switcher_revision;gesture=(Gesture)}|ConvertTo-Json -Depth 8|
            Set-Content (Join-Path $run 'order-failure.json')
        throw 'Incomplete gesture persisted a reorder'
    }
}
function Preview-Select {
    (Control ('workspace-manager-row-'+$preview)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
    Wait-Until {(Manager).selected -eq $preview -and !(Manager).loading} 'Preview did not settle'
    $stable=@{layout=$null;count=0}
    Wait-Until {
        $layout=Layout
        if($layout -eq $stable.layout){$stable.count++}else{$stable.layout=$layout;$stable.count=0}
        $stable.count -ge 3
    } 'Native preview measurements did not settle'
    $script:previewLayout=$stable.layout
}
function Source-Id {@((Storage).order|Where-Object {$_ -ne $preview})[-1]}

function No-Drop([switch]$Outside){
    $script:gestureCase=if($Outside){'outside'}else{'no-op'}
    $source=Source-Id;$saved=@((Storage).order);$revision=(Storage).switcher_revision
    if($Outside){
        $bounds=(Control 'workspace-manager-items').Current.BoundingRectangle
        $at=@{x=[int]($bounds.X+$bounds.Width*.4);y=[int]($bounds.Y-12)}
    }else{$at=Point $source}
    Press $source -Grip;Start-Sleep -Milliseconds 45;Move-To $at
    Wait-Until {$g=Gesture;$g.phase -eq 'dragging' -and !$g.can_drop} 'No-op or outside drop was offered as a reorder'
    $hit=Gesture
    if($hit.source -ne $source -or $hit.device -ne $Device -or !$hit.grip){throw 'No-op fixture hit the wrong source or device'}
    [CapyRowPointer]::Up()
    Wait-Until {(Gesture).phase -eq 'idle'} 'Rejected drop did not release its state'
    Preview-Intact;Order-Intact $saved $revision
}

function Drag([switch]$Grip,[switch]$Hold,[switch]$Cancel){
    $source=Source-Id;$saved=@((Storage).order);$revision=(Storage).switcher_revision
    $target=Point $saved[0] -Top
    Press $source -Grip:$Grip
    if($Hold){
        Wait-Until {(Gesture).phase -eq 'held'} "$Device hold was not recognized" 4
        if(!(Find 'workspace-manager-move-up')){throw 'Hold did not open a native context menu'}
    }else{Start-Sleep -Milliseconds 45}
    Preview-Intact;Order-Intact $saved $revision
    Move-To $target
    Wait-Until {$g=Gesture;$g.phase -eq 'dragging' -and $g.can_drop} "$Device did not drag with the original contact"
    $gesture=Gesture
    if($gesture.device -ne $Device -or $gesture.grip -ne [bool]$Grip -or $gesture.source -ne $source){throw 'Native pointer device or actual row hit differs from the fixture'}
    if(Find 'workspace-manager-move-up'){throw 'Dragging left the context menu open'}
    Preview-Intact;Order-Intact $saved $revision
    if($Cancel){
        $previousRelease=(Gesture).last_release.generation
        $previousCancel=(Gesture).last_cancel.generation
        if($Device -eq 'mouse'){[CapyRowPointer]::Key(27);[CapyRowPointer]::Up()}else{[CapyRowPointer]::Cancel()}
        Wait-Until {(Gesture).phase -eq 'idle'} 'Canceled drag did not release its state'
        $terminal=(Gesture).last_release
        if($terminal.generation -ne $previousRelease -and $terminal.commit){
            $terminal|ConvertTo-Json -Depth 6|Set-Content (Join-Path $run 'canceled-drop.json')
            throw 'Canceled pointer was submitted as a drop'
        }
        $cancellation=(Gesture).last_cancel
        $expectedReasons=if($Device -eq 'mouse'){@('escape')}else{@('pointer_canceled','capture_lost')}
        if(!$cancellation -or $cancellation.generation -eq $previousCancel -or $cancellation.reason -notin $expectedReasons){
            throw 'The fixture did not deliver the expected native cancellation'
        }
        $cancellation|ConvertTo-Json|Set-Content (Join-Path $run 'cancellation.json')
        Preview-Intact;Order-Intact $saved $revision
    }else{
        [CapyRowPointer]::Up()
        Wait-Until {(Storage).switcher_revision -gt $revision} 'Drop was not persisted'
        Settled;Preview-Intact
        if((Storage).switcher_revision -ne $revision+1 -or (Storage).order[0] -ne $source){throw 'Drop did not commit one reorder'}
    }
}
try {
    foreach($name in $names){Remove-Item -LiteralPath ('Env:'+$name) -ErrorAction SilentlyContinue}
    $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'profile'
    $env:CAPY_TRACE_UI='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
    Launch $Device
    [CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
    [CapyRowPointer]::Initialize([uint32]$review.Id)
    if(![CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)){throw 'Could not focus owned input review'}
    $active=(Storage).id
    $preview=@((Storage).order|Where-Object {$_ -ne $active})[0]
    Open-Manager;Preview-Select
    if(!(Find ('workspace-manager-current-'+$active))){throw 'Current workspace checkmark is missing'}
    $wrong=Find ('workspace-manager-current-'+$preview)
    if($wrong -and !$wrong.Current.IsOffscreen){throw 'Dialog preview moved the current workspace checkmark'}
    $source=Source-Id
    Press $source;Start-Sleep -Milliseconds 45;[CapyRowPointer]::Up()
    Wait-Until {(Manager).selected -eq $source -and !(Manager).loading} "$Device short click no longer previews"
    Preview-Select
    Write-Output "$Device short click passed"
    if($Device -ne 'mouse'){
        $saved=@((Storage).order);$revision=(Storage).switcher_revision;$source=Source-Id;$at=Point $source
        Press $source;Start-Sleep -Milliseconds 45
        Move-To @{x=$at.x;y=$at.y-24}
        [CapyRowPointer]::Up()
        Wait-Until {!(Gesture).panning} 'Native panning did not settle before the next gesture'
        Preview-Intact;Order-Intact $saved $revision
        if(Find 'workspace-manager-move-up'){throw 'Motion before hold opened a context menu'}
        Write-Output "$Device motion before hold passed"
        Press $source
        Wait-Until {(Gesture).phase -eq 'held'} "$Device hold was not recognized" 4
        [CapyRowPointer]::Up()
        Wait-Until {$null -ne (Find 'workspace-manager-move-up')} 'Held release did not retain its menu'
        Preview-Intact;Order-Intact $saved $revision
        [CapyRowPointer]::Key(27)
        Wait-Until {$null -eq (Find 'workspace-manager-move-up')} 'Escape did not dismiss the held menu'
        if(!(Manager)){throw 'Dismissing the menu also closed the manager'}
        Write-Output "$Device held menu release passed"
    }else{
        $saved=@((Storage).order);$revision=(Storage).switcher_revision
        Press (Source-Id);Start-Sleep -Milliseconds 1100
        if(Find 'workspace-manager-move-up'){throw 'Mouse hold opened a menu'}
        Preview-Intact;Order-Intact $saved $revision
        [CapyRowPointer]::Up()
        Wait-Until {!(Manager).loading} 'Mouse click did not settle'
        Preview-Select
    }
    Drag -Grip
    Write-Output "$Device immediate grip passed"
    Drag -Hold:($Device -ne 'mouse')
    Write-Output "$Device body reorder passed"
    Drag -Grip -Cancel
    Write-Output "$Device cancellation passed"
    No-Drop;No-Drop -Outside
    Write-Output "$Device no-op and outside drops passed"
    $source=Source-Id;$revision=(Storage).switcher_revision
    Invoke ('workspace-manager-grip-'+$source)
    Invoke 'workspace-manager-move-up'
    Wait-Until {(Storage).switcher_revision -gt $revision} 'Accessible grip command did not persist'
    Settled;Preview-Intact
    (Control ('workspace-manager-row-'+$preview)).SetFocus()
    [CapyRowPointer]::Key(38)
    Wait-Until {(Manager).selected -ne $preview -and !(Manager).loading} 'Keyboard navigation no longer previews'
    [CapyRowPointer]::Key(13)
    if(!(Manager)){throw 'Enter unexpectedly applied and closed the manager'}
    Preview-Select
    Write-Output "$Device accessible grip and keyboard preview passed"
    $at=Point (Source-Id)
    [CapyRowPointer]::RightClick($at.x,$at.y)
    Wait-Until {$null -ne (Find 'workspace-manager-move-up')} 'Secondary click did not open the row menu'
    Preview-Intact
    [CapyRowPointer]::Key(27)
    Wait-Until {$null -eq (Find 'workspace-manager-move-up')} 'Secondary menu did not dismiss'
    (Control ('workspace-manager-row-'+$preview)).SetFocus()
    [CapyRowPointer]::Key(93)
    Wait-Until {$null -ne (Find 'workspace-manager-move-up')} 'Keyboard menu action did not open the row menu'
    Preview-Intact
    [CapyRowPointer]::Key(27)
    Wait-Until {$null -eq (Find 'workspace-manager-move-up')} 'Keyboard menu did not dismiss'
    Write-Output "$Device secondary and keyboard menus passed"
    Capture ($Device+'-rows')
    Choose 'Cancel';Closed
    if((Storage).id -ne $active){throw 'Cancel changed the active workspace'}

    if($Overflow){
        for($i=1;$i -le 6;$i++){
            Open-Manager;Invoke 'workspace-manager-create'
            Wait-Until {(Manager).prompt.title -eq 'New Workspace'} 'Creation prompt missing'
            (Control 'workspace-manager-name').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue("Row overflow workspace $i")
            Choose 'Create and Switch';Closed
            Wait-Until {$s=Storage;$s.ready -and !$s.busy -and !$s.switcher_busy} 'Created workspace did not settle'
        }
        $active=(Storage).id
        Open-Manager;Preview-Select
        $scroller=(Control 'workspace-manager-items').GetCurrentPattern([System.Windows.Automation.ScrollPattern]::Pattern)
        Wait-Until {$scroller.Current.VerticallyScrollable} 'Rows did not overflow'
        $scroller.SetScrollPercent([System.Windows.Automation.ScrollPattern]::NoScroll,0)
        Wait-Until {$scroller.Current.VerticalScrollPercent -le 1} 'Rows did not scroll to the beginning'
        $saved=@((Storage).order);$revision=(Storage).switcher_revision
        if($Device -ne 'mouse'){
            $at=Point $saved[2]
            Press $saved[2];Start-Sleep -Milliseconds 45
            Move-To @{x=$at.x;y=$at.y-100}
            Start-Sleep -Milliseconds 45;[CapyRowPointer]::Up()
            Wait-Until {$scroller.Current.VerticalScrollPercent -gt 1 -and !(Gesture).panning} "$Device did not preserve ordinary row scrolling"
            Preview-Intact;Order-Intact $saved $revision
            $scroller.SetScrollPercent([System.Windows.Automation.ScrollPattern]::NoScroll,0)
            Wait-Until {$scroller.Current.VerticalScrollPercent -le 1} 'Rows did not return to the beginning'
        }
        $source=$saved[0]
        $bounds=(Control 'workspace-manager-items').Current.BoundingRectangle
        Press $source -Grip;Start-Sleep -Milliseconds 45
        Move-To @{x=[int]($bounds.X+$bounds.Width*.4);y=[int]($bounds.Bottom-5)}
        Wait-Until {$scroller.Current.VerticalScrollPercent -ge 99 -and (Gesture).phase -eq 'dragging' -and (Gesture).can_drop} 'Drag did not retain its source while scrolling to the end'
        Preview-Intact;Order-Intact $saved $revision
        [CapyRowPointer]::Up()
        Wait-Until {(Storage).switcher_revision -gt $revision} 'Scrolled drop was not persisted'
        Settled;Preview-Intact
        if((Storage).order[-1] -ne $source -or (Storage).switcher_revision -ne $revision+1){throw 'Scrolled drop did not commit one move to the end'}
        Write-Output "$Device scroll overflow and retained drag passed"
        $scroller.SetScrollPercent([System.Windows.Automation.ScrollPattern]::NoScroll,0)
        Wait-Until {$scroller.Current.VerticalScrollPercent -le 1} 'Rows did not return to the beginning'
        $script:gestureCase='source-invalidation'
        $saved=@((Storage).order);$revision=(Storage).switcher_revision;$source=$saved[0]
        $at=Point $saved[2]
        Press $source -Grip;Start-Sleep -Milliseconds 45;Move-To $at
        Wait-Until {(Gesture).phase -eq 'dragging'} 'Source invalidation review did not begin dragging'
        $search=Control 'workspace-manager-search'
        $search.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('No matching row for input review')
        Wait-Until {(Gesture).phase -eq 'idle' -and @((Manager).rows).Count -eq 0} 'Filtering away the source did not cancel the drag'
        [CapyRowPointer]::Cancel()
        Order-Intact $saved $revision
        $search.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('')
        Settled;Preview-Select
        Write-Output "$Device source invalidation passed"
        $script:gestureCase='minimize'
        $saved=@((Storage).order);$revision=(Storage).switcher_revision
        $at=Point $saved[2]
        Press $saved[0] -Grip;Start-Sleep -Milliseconds 45;Move-To $at
        Wait-Until {(Gesture).phase -eq 'dragging'} 'Blur review did not begin dragging'
        $previousRelease=(Gesture).last_release.generation;$previousCancel=(Gesture).last_cancel.generation
        $window=$root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern)
        $window.SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Minimized)
        Wait-Until {(Gesture).phase -eq 'idle'} 'Losing the window did not cancel the drag'
        $terminal=Gesture
        $terminal|ConvertTo-Json -Depth 8|Set-Content (Join-Path $run 'blur-cancellation.json')
        if(($terminal.last_release.generation -ne $previousRelease -and $terminal.last_release.commit) -or
            !$terminal.last_cancel -or $terminal.last_cancel.generation -eq $previousCancel){throw 'Losing the window did not cancel before committing a drop'}
        [CapyRowPointer]::Cancel()
        $window.SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Normal)
        Wait-Until {$window.Current.WindowVisualState -eq [System.Windows.Automation.WindowVisualState]::Normal} 'Review window did not restore'
        Settled;Preview-Intact;Order-Intact $saved $revision
        Write-Output "$Device minimize and blur cancellation passed"
        Capture ($Device+'-overflow')
        Choose 'Cancel';Closed
        if((Storage).id -ne $active){throw 'Overflow review changed the active workspace'}
    }
    Close
    @{device=$Device;short_click='passed';pickup_and_menu='passed';grip_and_body_drop='passed';preview_preserved='passed';
        single_reorder_commit='passed';cancellation='passed';no_op_and_outside_drops='passed';accessible_grip_and_keyboard='passed';secondary_and_keyboard_menus='passed';overflow_source_invalidation_and_blur=[bool]$Overflow;zero_exit='passed';
        scope='OS-injected native input; physical digitizers and performance require separate acceptance'}|
        ConvertTo-Json|Set-Content (Join-Path $run 'results.json')
    Get-Content (Join-Path $run 'results.json')
}catch{
    [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace)
    throw
}finally{
    [CapyRowPointer]::Dispose()
    foreach($name in $names){
        if($null -eq $previous[$name]){Remove-Item -LiteralPath ('Env:'+$name) -ErrorAction SilentlyContinue}
        else{[Environment]::SetEnvironmentVariable($name,$previous[$name],'Process')}
    }
}
