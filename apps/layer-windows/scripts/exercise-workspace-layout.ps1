param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/workspace/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$names=@('CAPY_SETTINGS_DIRECTORY','CAPY_TRACE_UI','CAPY_SMOKE_TEST','CAPY_TEST_DISPLAY','CAPY_TEST_PRIMARY','CAPY_PRESENT_PROBE')
$previous=@{}
foreach($name in $names){$previous[$name]=[Environment]::GetEnvironmentVariable($name,'Process')}
function Model {
    try{$s=Get-Content (Join-Path $directory 'ui-state.json') -Raw|ConvertFrom-Json;if($s.process_id -eq $review.Id -and $s.model.windows_isolated_settings){$s.model}}catch{}
}
function Wait-Until([scriptblock]$Predicate,[string]$Message,[int]$Seconds=5){
    $watch=[Diagnostics.Stopwatch]::StartNew()
    do{if(& $Predicate){return};$review.Refresh();if($review.HasExited){throw 'Workspace review exited unexpectedly'};Start-Sleep -Milliseconds 50}while($watch.Elapsed.TotalSeconds -lt $Seconds)
    throw $Message
}
function Find([string]$Value,[switch]$Name,$Type){
    $property=if($Name){[System.Windows.Automation.AutomationElement]::NameProperty}else{[System.Windows.Automation.AutomationElement]::AutomationIdProperty}
    $match=[System.Windows.Automation.PropertyCondition]::new($property,$Value)
    if($Type){$match=[System.Windows.Automation.AndCondition]::new($match,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,$Type))}
    $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$match)
}
function Control([string]$Value,[switch]$Name,$Type){
    $hit=@{item=$null};Wait-Until {$hit.item=Find $Value -Name:$Name -Type $Type;$null -ne $hit.item} "Missing $Value"
    $hit.item
}
function Invoke([string]$Value,[switch]$Name){
    (Control $Value -Name:$Name).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
}
function Command([string]$Id){(Model).state.commands|Where-Object id -eq $Id}
try{
    foreach($name in $names){[Environment]::SetEnvironmentVariable($name,$null,'Process')}
    $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'profile'
    $env:CAPY_TRACE_UI='1';$env:CAPY_SMOKE_TEST='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
    $stderr=Join-Path $run 'stderr.log'
    $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
    $null=$review.Handle
    [IO.File]::WriteAllText((Join-Path $repo 'artifacts/windows/workspace-review.json'),(@{process_id=$review.Id;run=$run}|ConvertTo-Json))
    Write-Output "Owned workspace review $($review.Id)"
    Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero -and (Model).brush_ready} 'Review did not start' 45
    $root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
    $ids=@('file','edit','layer','select','filter','view','window','help')
    $last=-1
    foreach($id in $ids){
        $button=Control "application-menu-$id"
        $spec=(Model).application_menus|Where-Object id -eq $id
        if($button.Current.Name -ne $spec.label){throw "Menu label differs from shared model: $id"}
        if($button.Current.BoundingRectangle.X -le $last){throw 'Application menus are not in shared order'}
        $last=$button.Current.BoundingRectangle.X
    }
    Invoke 'application-menu-select'
    $all=Control 'select_all'
    if($all.Current.IsEnabled -ne (Command 'select_all').enabled){throw 'Selection menu enabled state differs from model'}
    Invoke 'select_all'
    Wait-Until {(Command 'deselect').enabled} 'Select All did not update shared selection'
    Invoke 'application-menu-select';Invoke 'deselect'
    Wait-Until {!(Command 'deselect').enabled} 'Deselect did not clear selection'
    Invoke 'application-menu-help'
    foreach($id in @('website','source_code')){if(!(Control $id).Current.IsEnabled){throw "Shared help link is disabled: $id"}}
    Invoke 'about'
    Wait-Until {$null -ne (Model).preferences -and (Model).preferences.page -eq 'about'} 'About did not open shared settings page'
    $dialog=Control 'Preferences' -Name -Type ([System.Windows.Automation.ControlType]::Window)
    $close=$dialog.FindFirst([System.Windows.Automation.TreeScope]::Descendants,
        [System.Windows.Automation.AndCondition]::new(
            [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,'Close'),
            [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::Button)))
    $close.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    Wait-Until {$null -eq (Model).preferences -and (Control 'Drawing canvas' -Name).Current.IsEnabled} 'About did not close'
    Invoke 'application-menu-window'
    (Control 'Diagnostics panel' -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
    Wait-Until {@((Model).layout.groups|Where-Object active -eq 'stats').Count -gt 0} 'Diagnostics panel did not appear'
    Wait-Until {$null -ne (Find 'renderer-stat-6')} 'Diagnostics query did not populate all shared rows'
    $cpu=Control 'renderer-stat-0'
    if($cpu.Current.HelpText -notmatch 'Not GPU completion or input latency'){throw 'Diagnostics lost measurement scope'}
    $cpuIdentity=$cpu.GetRuntimeId() -join ':'
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action 'Test stroke'
    Wait-Until {[uint64](Control 'renderer-stat-2').Current.Name -gt 0} 'Visible Diagnostics did not collect drawing submissions'
    if(((Control 'renderer-stat-0').GetRuntimeId() -join ':') -ne $cpuIdentity){throw 'Stats updates replaced native metric rows'}
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Resize -Width 1500 -Height 1000
    Wait-Until {$null -ne (Find 'renderer-stat-6')} 'Resize removed Diagnostics content'
    & (Join-Path $PSScriptRoot 'inspect-window.ps1') -ProcessId $review.Id -Output (Join-Path $run 'diagnostics.png') -ClientOnly *> (Join-Path $run 'diagnostics.json')
    Invoke 'application-menu-window'
    (Control 'Diagnostics panel' -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
    Wait-Until {$null -eq (Find 'renderer-stat-0')} 'Hidden Diagnostics retained visible widgets'
    Invoke 'application-menu-window';Invoke 'undo_workspace'
    Wait-Until {$null -ne (Find 'renderer-stat-6')} 'Workspace Undo did not restore Diagnostics'
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved
    if((Get-Item -LiteralPath $stderr).Length){throw 'Native stderr requires inspection'}
    [pscustomobject]@{eight_shared_menus='passed';selection_commands='passed';about_page='passed';diagnostics_query_and_row_retention='passed';diagnostics_visibility_and_workspace_undo='passed';native_resize='passed';zero_exit='passed';scope='native UI Automation; physical docking, drawers, full layout parity and presentation remain separate'}|ConvertTo-Json
}catch{
    [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{
    foreach($name in $names){[Environment]::SetEnvironmentVariable($name,$previous[$name],'Process')}
}
