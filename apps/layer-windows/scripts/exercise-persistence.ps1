param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/persistence/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$names=@('CAPY_SETTINGS_DIRECTORY','CAPY_TRACE_UI','CAPY_SMOKE_TEST','CAPY_TEST_DISPLAY','CAPY_TEST_PRIMARY','CAPY_PRESENT_PROBE')
$previous=@{};foreach($name in $names){$previous[$name]=[Environment]::GetEnvironmentVariable($name,'Process')}
function Model {
    try{
        $s=Get-Content (Join-Path $directory 'ui-state.json') -Raw|ConvertFrom-Json
        if($s.process_id -eq $review.Id -and $s.model.windows_isolated_settings){$script:lastModel=$s.model}
    }catch{}
    $script:lastModel
}
function Wait-Until([scriptblock]$Predicate,[string]$Message,[int]$Seconds=5) {
    $watch=[Diagnostics.Stopwatch]::StartNew()
    do{
        if(& $Predicate){return}
        $review.Refresh();if($review.HasExited){throw 'Workspace persistence review exited unexpectedly'}
        Start-Sleep -Milliseconds 50
    }while($watch.Elapsed.TotalSeconds -lt $Seconds)
    throw $Message
}
function Find([string]$Value,[switch]$Name,$Type,$Within=$root) {
    $property=if($Name){[System.Windows.Automation.AutomationElement]::NameProperty}else{[System.Windows.Automation.AutomationElement]::AutomationIdProperty}
    $condition=[System.Windows.Automation.PropertyCondition]::new($property,$Value)
    if($Type){$condition=[System.Windows.Automation.AndCondition]::new($condition,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,$Type))}
    $Within.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$condition)
}
function Control([string]$Value,[switch]$Name,$Type,$Within=$root) {
    $hit=@{item=$null};Wait-Until {$hit.item=Find $Value -Name:$Name -Type $Type -Within $Within;$null -ne $hit.item} "Missing $Value"
    $hit.item
}
function Invoke([string]$Value,[switch]$Name,$Within=$root) {
    (Control $Value -Name:$Name -Within $Within).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
}
function Dialog([string]$Name) {
    $dialog=Control 'workspace-close-error'
    (Control $Name -Name -Type ([System.Windows.Automation.ControlType]::Button) -Within $dialog).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
}
function Launch([string]$Profile,[string]$Label,[switch]$Failed) {
    $script:lastModel=$null
    $env:CAPY_SETTINGS_DIRECTORY=$Profile
    $script:stderr=Join-Path $run ($Label+'-stderr.log')
    $script:review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
    $null=$review.Handle
    [IO.File]::WriteAllText((Join-Path $repo 'artifacts/windows/persistence-review.json'),(@{process_id=$review.Id;run=$run;profile=$Profile}|ConvertTo-Json))
    Write-Output "Owned workspace persistence review $($review.Id) ($Label)"
    Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero -and (Model).brush_ready} 'Canvas did not start' 45
    $script:root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
    if($Failed){Wait-Until {(Model).windows_workspace.error -and !(Model).windows_workspace.busy} 'Storage failure was not reported'}
    else{
        Wait-Until {(Model).windows_workspace.ready -and !(Model).windows_workspace.busy} 'Workspace did not open' 45
        Wait-Until {@((Model).panel_measurements|Where-Object {$_.panel -eq 'brushes' -and $_.content_height -gt 0 -and $_.content_height -ne 320}).Count -eq 1} 'Adopted workspace did not receive native measurements'
    }
}
function Close {
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close
    if((Get-Item -LiteralPath $stderr).Length){throw 'Native stderr requires inspection'}
}
function Exit-AfterDecision {
    if(!$review.WaitForExit(5000)){throw 'Workspace close exceeded five seconds after the decision'}
    if($null -eq $review.ExitCode -or $review.ExitCode -ne 0){throw 'Workspace close did not exit successfully'}
    if((Get-Item -LiteralPath $stderr).Length){throw 'Native stderr requires inspection'}
}
function Layout { (Model).state.workspace | ConvertTo-Json -Depth 80 -Compress }
try {
    foreach($name in $names){[Environment]::SetEnvironmentVariable($name,$null,'Process')}
    $env:CAPY_TRACE_UI='1';$env:CAPY_SMOKE_TEST='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
    $profile=Join-Path $run 'profile'
    Launch $profile 'initial'
    $before=Layout
    (Control 'Brush size slider' -Name -Type ([System.Windows.Automation.ControlType]::Slider)).GetCurrentPattern([System.Windows.Automation.RangeValuePattern]::Pattern).SetValue(0.5)
    Wait-Until {(Model).state.brush.diameter -eq 32} 'Brush value was not applied'
    Invoke 'panel-tab-stats'
    Wait-Until {$null -ne (Find 'renderer-stat-6') -and (Layout) -ne $before} 'Workspace layout did not change'
    $expected=Layout
    Wait-Until {!(Model).windows_workspace.dirty -and !(Model).windows_workspace.saving} 'Workspace autosave did not complete'
    if(!(Test-Path -LiteralPath (Join-Path $profile 'workspaces.sqlite3'))){throw 'Workspace database was not created'}
    (Control 'Brush size slider' -Name -Type ([System.Windows.Automation.ControlType]::Slider)).GetCurrentPattern([System.Windows.Automation.RangeValuePattern]::Pattern).SetValue(0.61)
    Wait-Until {(Model).state.brush.diameter -ne 32} 'Final tool edit did not apply'
    $expectedSize=(Model).state.brush.diameter
    Close
    Launch $profile 'restart'
    Wait-Until {(Model).state.brush.diameter -eq $expectedSize} 'Restart did not restore the final tool value'
    if((Layout) -ne $expected){throw 'Restart did not restore the saved layout'}
    if(!(Find 'renderer-stat-6')){throw 'Restored Diagnostics did not appear natively'}
    Close
    $broken=Join-Path $run 'unreadable'
    [IO.Directory]::CreateDirectory($broken)|Out-Null
    $database=Join-Path $broken 'workspaces.sqlite3'
    [IO.File]::WriteAllText($database,'isolated fixture: unreadable workspace database')
    Launch $broken 'unreadable' -Failed
    $unchanged=(Model).state.brush.diameter
    (Control 'Brush size slider' -Name -Type ([System.Windows.Automation.ControlType]::Slider)).GetCurrentPattern([System.Windows.Automation.RangeValuePattern]::Pattern).SetValue(0.8)
    Start-Sleep -Milliseconds 250
    if((Model).state.brush.diameter -ne $unchanged){throw 'Editing was accepted before workspace recovery'}
    $review.CloseMainWindow()|Out-Null
    Wait-Until {$null -ne (Find 'workspace-close-error')} 'Close recovery dialog did not appear'
    & (Join-Path $PSScriptRoot 'inspect-window.ps1') -ProcessId $review.Id -Output (Join-Path $run 'close-recovery.png') -ClientOnly *> (Join-Path $run 'close-recovery.json')
    Dialog 'Keep open'
    Wait-Until {!(Model).windows_workspace.close_requested -and !(Model).state.document_file.close_ready} 'Keep open did not cancel window close'
    # These files were created by this fixture in its unique local directory.
    Move-Item -LiteralPath $database -Destination (Join-Path $broken 'unreadable-original')
    Copy-Item -LiteralPath (Join-Path $profile 'workspaces.sqlite3') -Destination $database
    Invoke 'workspace-storage-retry'
    Wait-Until {(Model).windows_workspace.ready -and !(Model).windows_workspace.error} 'Retry did not reopen repaired storage'
    if((Model).state.brush.diameter -ne $expectedSize){throw 'Recovery did not adopt the stored workspace'}
    Close
    $discard=Join-Path $run 'discard'
    [IO.Directory]::CreateDirectory($discard)|Out-Null
    $original='isolated fixture: preserve this unreadable database'
    [IO.File]::WriteAllText((Join-Path $discard 'workspaces.sqlite3'),$original)
    Launch $discard 'discard' -Failed
    $review.CloseMainWindow()|Out-Null
    Dialog 'Close without saving'
    Exit-AfterDecision
    if([IO.File]::ReadAllText((Join-Path $discard 'workspaces.sqlite3')) -ne $original){throw 'Closing overwrote unreadable workspace data'}
    [pscustomobject]@{autosave='passed';restart_layout='passed';restart_tool_values='passed';native_measurements='passed';final_edit_close='passed';unreadable_storage='passed';editing_guard='passed';keep_open='passed';retry_recovery='passed';explicit_discard='passed';original_preserved='passed';zero_exit='passed';scope='native UI Automation and isolated SQLite persistence; full manager UI and physical input remain separate'}|ConvertTo-Json
}catch{
    [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{
    foreach($name in $names){[Environment]::SetEnvironmentVariable($name,$previous[$name],'Process')}
}
