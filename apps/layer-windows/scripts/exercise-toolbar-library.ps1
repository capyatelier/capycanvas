param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyCacheModel=$true
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/toolbar-library/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
function Manager {(Model).windows_workspace_manager}
function Layout {(Model).state.workspace | ConvertTo-Json -Depth 80 -Compress}
function Edit([string]$Id,[string]$Value){(Control $Id).GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($Value)}
function Button([string]$Name){
    Control $Name -Name -Within (Control 'workspace-manager') -Type ([System.Windows.Automation.ControlType]::Button)
}
function Choose([string]$Name){(Button $Name).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()}
function Settled {
    Wait-Until {$v=Manager;$null -ne $v -and !$v.loading -and !$v.busy} 'Manager did not settle'
    if((Manager).error){throw (Manager).error}
}
function Prompt-Done {
    Wait-Until {$v=Manager;$null -ne $v -and !$v.loading -and !$v.busy -and ($null -eq $v.prompt -or $v.error)} 'Toolbar operation did not finish'
    Settled
}
function Closed {
    Wait-Until {$null -eq (Manager) -and $null -eq (Find 'workspace-manager')} 'Manager did not close'
}
function Menu([string]$Name){
    Closed
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'window'
    (Control 'Quick Access Toolbars' -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)).GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    Invoke $Name -Name
    Wait-Until {$null -ne (Find 'workspace-manager')} 'Native manager did not open'
    Settled
}
function Select-Row([string]$Id){
    $item=Control ('workspace-manager-row-'+$Id)
    $item.GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
    Wait-Until {(Manager).selected -eq $Id -and !(Manager).loading} 'Selected preview did not settle'
    if((Manager).error){throw (Manager).error}
    $item
}
function Row([string]$Title){(Manager).rows|Where-Object title -eq $Title|Select-Object -First 1}
function Launch([string]$Label){
    $script:stderr=Join-Path $run ($Label+'-stderr.log')
    $script:review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
    $null=$review.Handle
    [IO.File]::WriteAllText((Join-Path $repo 'artifacts/windows/toolbar-library-review.json'),(@{process_id=$review.Id;run=$run}|ConvertTo-Json))
    Write-Output "Owned toolbar library review $($review.Id) ($Label)"
    Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero -and (Model).brush_ready -and (Model).windows_workspace.ready} 'Manager review did not start' 45
    $script:root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
}
$script:closeFailures=@()
function Close {
    try { & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close }
    catch {
        if($_.Exception.Message -ne 'Close exceeded five seconds after authorization.'){throw}
        $script:closeFailures+=$review.Id
        Write-Warning 'Five-second close gate failed; checking the same process before continuing the remaining functional checks.'
        if(!$review.WaitForExit(20000)){throw 'Owned review is still running after the close failure.'}
    }
    $review.Refresh()
    if(!$review.HasExited -or $review.ExitCode -ne 0){throw 'Owned review did not exit successfully'}
    if((Get-Item -LiteralPath $stderr).Length){throw 'Native stderr requires inspection'}
}

function Saved-Page {
    (Control 'workspace-toolbar-library').GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
    Wait-Until {(Manager).page -eq 'toolbar_library'} 'Saved Toolbars tab did not open'; Settled
}
function Toolbar-Action([string]$Type) {
    Invoke 'workspace-toolbar-actions'; Invoke ('workspace-toolbar-'+$Type)
}
function Source-Choice([string]$Name) {
    (Control 'workspace-manager-choice').GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    (Control $Name -Name -Type ([System.Windows.Automation.ControlType]::ListItem)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
}
function Config([string]$Name){(Model).state.workspace.layout.panels|Where-Object {$_.content.kind -eq 'toolbar' -and $_.content.name -eq $Name}|Select-Object -First 1}
function Controls-Of($Config){@($Config.content.tiles|ForEach-Object {$_.control})|ConvertTo-Json -Depth 40 -Compress}
function Check-Copy($Source,$Copy) {
    if(!$Copy -or $Source.id -eq $Copy.id -or (Controls-Of $Source) -ne (Controls-Of $Copy) -or $Source.tile_style -ne $Copy.tile_style -or $Source.hide_tab -ne $Copy.hide_tab){throw 'Saved toolbar copy lost contents/options or independent identity'}
}
try {
    Enter-CapyEnvironment
    $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'profile'
    $env:CAPY_TRACE_UI='1';$env:CAPY_SMOKE_TEST='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
    Launch 'save'
    Menu 'Manage Toolbars…'
    if((Manager).page -ne 'this_workspace'){throw 'Toolbar manager opened the wrong page'}
    $sourceRow=(Manager).rows|Select-Object -First 1
    $sourceId=$sourceRow.id|ConvertFrom-Json
    $source=(Model).state.workspace.layout.panels|Where-Object id -eq $sourceId
    if(@($source.content.tiles).Count -lt 1){throw 'Fixture source needs existing toolbar contents'}
    $null=Select-Row $sourceRow.id
    Capture 'this-workspace'
    Toolbar-Action 'save_toolbar'; Settled
    Edit 'workspace-manager-name' 'Shared review tools'
    Choose (Manager).prompt.confirm; Prompt-Done
    Saved-Page
    $saved=Row 'Shared review tools'
    if(!$saved){throw 'Saved toolbar did not appear in the library'}
    $savedId=$saved.id
    $null=Select-Row $savedId
    Capture 'saved-library'
    Choose 'Cancel'; Closed; Close
    Launch 'library-restart'
    Menu 'New Toolbar…'
    if(@((Manager).prompt.choices|Where-Object id -eq $savedId).Count -ne 1){throw 'Restart did not restore the saved toolbar choice'}
    Edit 'workspace-manager-name' 'Independent review tools'
    Source-Choice 'Shared review tools'
    Capture 'new-from-library'
    Choose 'Add to Workspace'; Closed
    $copy=Config 'Independent review tools'; Check-Copy $source $copy
    $copyId=$copy.id
    Close
    Launch 'copy-restart'
    $restored=Config 'Independent review tools'; Check-Copy $source $restored
    if($restored.id -ne $copyId){throw 'Restart changed the installed toolbar identity'}
    Menu 'Manage Toolbars…'; Saved-Page
    $null=Select-Row $savedId
    Toolbar-Action 'rename'; Settled
    Edit 'workspace-manager-name' 'Renamed library review'
    Choose 'Rename'; Prompt-Done
    if(!(Row 'Renamed library review')){throw 'Library rename did not update the native row'}
    if(!(Config 'Independent review tools')){throw 'Library rename altered an installed copy'}
    $null=Select-Row $savedId
    Choose 'Add to Workspace'; Closed
    Check-Copy $source (Config 'Renamed library review')
    Menu 'Manage Toolbars…'; Saved-Page
    $null=Select-Row $savedId
    Toolbar-Action 'delete'; Settled
    Choose 'Cancel'; Prompt-Done
    if(!(Row 'Renamed library review')){throw 'Cancelling delete changed the library'}
    $null=Select-Row $savedId
    Toolbar-Action 'delete'; Settled
    Choose 'Delete'; Prompt-Done
    if(@((Manager).rows|Where-Object id -eq $savedId).Count){throw 'Confirmed delete kept the saved toolbar'}
    Choose 'Cancel'; Closed
    if(!(Config 'Independent review tools') -or !(Config 'Renamed library review')){throw 'Library deletion changed installed copies'}
    Close
    Launch 'deletion-restart'
    Check-Copy $source (Config 'Independent review tools')
    Check-Copy $source (Config 'Renamed library review')
    Menu 'New Toolbar…'
    if(@((Manager).prompt.choices|Where-Object id -eq $savedId).Count){throw 'Deleted library item reappeared after restart'}
    Choose 'Cancel'; Closed
    Close
    [pscustomobject]@{save_to_library='passed';library_restart='passed';new_from_saved='passed';copy_restart='passed';independent_identity_and_contents='passed';library_rename='passed';add_to_workspace='passed';delete_cancel_confirm='passed';copies_survive_library_deletion='passed';deletion_restart='passed';zero_exit='passed';scope='native UI Automation and isolated SQLite profiles; visual parity, physical input and presentation acceptance remain separate'}|ConvertTo-Json
    if($closeFailures.Count){throw ('Five-second close acceptance failed for '+$closeFailures.Count+' launch(es); functional results above do not waive this gate.')}
}catch{
    if($review -and !$review.HasExited){try{Capture 'failure'}catch{}}
    [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{
    Exit-CapyEnvironment
}
