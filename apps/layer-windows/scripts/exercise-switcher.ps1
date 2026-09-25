param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/switcher/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$names=@('CAPY_SETTINGS_DIRECTORY','CAPY_TRACE_UI','CAPY_SMOKE_TEST','CAPY_TEST_DISPLAY','CAPY_TEST_PRIMARY','CAPY_PRESENT_PROBE')
$previous=@{}
foreach($name in $names){$previous[$name]=[Environment]::GetEnvironmentVariable($name,'Process')}
function Model {
    try {
        $value=Get-Content (Join-Path $directory 'ui-state.json') -Raw|ConvertFrom-Json
        if($value.process_id -eq $review.Id -and $value.model.windows_isolated_settings){$value.model}
    }catch{}
}
function Storage {(Model).windows_workspace}
function Manager {(Model).windows_workspace_manager}
function Layout {(Model).state.workspace|ConvertTo-Json -Depth 80 -Compress}
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
    $Within.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$condition)
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
    $script:stderr=Join-Path $run ($Label+'-stderr.log')
    $script:review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
    $null=$review.Handle
    @{process_id=$review.Id;run=$run}|ConvertTo-Json|Set-Content (Join-Path $repo 'artifacts/windows/switcher-review.json')
    Write-Output "Owned switcher review $($review.Id) ($Label)"
    Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero -and (Model).brush_ready -and (Storage).ready -and !(Storage).switcher_busy} 'Switcher review did not start' 45
    $script:root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
    $root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Maximized)
    Wait-Until {$null -ne (Find 'workspace-switcher')} 'Maximized header did not show the workspace switcher'
    if(Find 'Test stroke' -Name){throw 'Switcher fixture requires the production UI without smoke controls'}
    $probe=Get-Item -LiteralPath (Join-Path $directory 'presentation-probe.json') -ErrorAction SilentlyContinue
    if($probe -and $probe.LastWriteTime -ge $review.StartTime){throw 'Switcher fixture must not run a presentation probe'}
}
function Close {
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close
    if((Get-Item -LiteralPath $stderr).Length){throw 'Native stderr requires inspection'}
}
try {
    foreach($name in $names){Remove-Item -LiteralPath ('Env:'+$name) -ErrorAction SilentlyContinue}
    $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'profile'
    $env:CAPY_TRACE_UI='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
    Launch 'initial'
    $active=(Storage).id;$order=@((Storage).order);$original=Layout
    $preview=@($order|Where-Object {$_ -ne $active})[0]
    Open-Manager
    (Control ('workspace-manager-row-'+$preview)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
    Wait-Until {(Manager).selected -eq $preview -and !(Manager).loading} 'Preview did not settle'
    $previewLayout=Layout
    $retained=(Control ('workspace-manager-row-'+$preview)).GetRuntimeId() -join ':'
    Preference $active 'show'
    if((Storage).switcher.id -contains $active -or (Storage).switcher_display[0].id -ne $active){throw 'Hidden current workspace fallback is incorrect'}
    if(((Storage).order -join '|') -ne ($order -join '|')){throw 'Visibility changed dialog order'}
    Preference $order[2] 'move-up'
    Preference $order[2] 'move-up'
    if((Manager).rows[0].id -ne $order[2]){throw 'Move Up did not reorder rows'}
    if((Manager).selected -ne $preview -or (Layout) -ne $previewLayout){throw 'Preferences disturbed the selected preview'}
    if(((Control ('workspace-manager-row-'+$preview)).GetRuntimeId() -join ':') -ne $retained){throw 'Reorder replaced a retained row'}
    Capture 'configured-preview'
    Choose 'Cancel';Closed
    if((Layout) -ne $original){throw 'Cancel did not restore the original layout'}
    $key=((Storage).switcher_display|Where-Object id -eq $preview).key
    (Control ('workspace-switch-'+$key)).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
    Wait-Until {(Storage).id -eq $preview -and !(Storage).busy} 'Header switch did not complete'
    if((Storage).switcher_display.id -contains $active){throw 'Unpinned previous workspace remained in header'}
    Open-Manager
    foreach($id in @((Storage).switcher.id)){Preference $id 'show'}
    if(@((Storage).switcher_display).Count -ne 1 -or (Storage).switcher_display[0].id -ne $preview){throw 'Empty pins did not retain the current workspace'}
    Choose 'Cancel';Closed
    $emptyOrder=@((Storage).order)
    Close
    Launch 'empty-pins-restart'
    if(@((Storage).switcher).Count -ne 0 -or ((Storage).order -join '|') -ne ($emptyOrder -join '|')){throw 'Preferences did not survive restart'}
    for($i=1;$i -le 6;$i++){
        Open-Manager
        Invoke 'workspace-manager-create'
        Wait-Until {(Manager).prompt.title -eq 'New Workspace'} 'Creation prompt missing'
        (Control 'workspace-manager-name').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue("Switcher overflow workspace $i")
        Choose 'Create and Switch';Closed
        Wait-Until {$s=Storage;$s.switcher.id -contains $s.id -and $s.switcher_display.id -contains $s.id} 'Created workspace was not pinned'
    }
    $inline=@{item=$null};Wait-Until {$inline.item=Find 'workspace-switcher';$inline.item -or (Find 'header-workspace-menu')} 'Header lost both the workspace switcher and its menu'
    $scroller=$null
    if($inline.item){
        $scroller=$inline.item.GetCurrentPattern([System.Windows.Automation.ScrollPattern]::Pattern)
        Wait-Until {$scroller.Current.HorizontallyScrollable} 'Header overflow is not scrollable'
        $scroller.SetScrollPercent(100,[System.Windows.Automation.ScrollPattern]::NoScroll)
        Wait-Until {$scroller.Current.HorizontalScrollPercent -ge 99} 'Header did not scroll to its last choices'
    }
    Capture 'header-overflow'
    $overflowCurrent=(Storage).id
    Open-Manager
    Preference $overflowCurrent 'show'
    if($scroller){Wait-Until {$scroller.Current.HorizontalScrollPercent -le 1} 'Unpinned current workspace did not scroll into view'}
    if((Storage).switcher_display[0].id -ne $overflowCurrent){throw 'Overflow lost the unpinned current workspace'}
    Capture 'overflow-current-fallback'
    Choose 'Cancel';Closed
    Close
    @{pinning='passed';complete_order='passed';preview_cancel_and_row_retention='passed';current_fallback='passed';empty_pins_restart='passed';created_workspaces_pinned='passed';header_overflow='passed';current_fallback_scrolled_into_view='passed';zero_exit='passed';scope='native UI Automation; physical row pickup and input timing are separate'}|ConvertTo-Json|Set-Content (Join-Path $run 'results.json')
    Get-Content (Join-Path $run 'results.json')
}catch{
    [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace)
    throw
}finally{
    foreach($name in $names){
        if($null -eq $previous[$name]){Remove-Item -LiteralPath ('Env:'+$name) -ErrorAction SilentlyContinue}
        else{[Environment]::SetEnvironmentVariable($name,$previous[$name],'Process')}
    }
}
