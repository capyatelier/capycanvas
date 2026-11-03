param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
. (Join-Path $PSScriptRoot 'LocalizationProfiles.ps1')
$CapyCacheModel=$true
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/persistence/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
function Launch([string]$Profile,[string]$Label) {
    $env:CAPY_STORAGE_DIR=$Profile
    $script:stderr=Join-Path $run ($Label+'-stderr.log')
    $script:review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
    $null=$review.Handle
    [IO.File]::WriteAllText((Join-Path $repo 'artifacts/windows/persistence-review.json'),(@{process_id=$review.Id;run=$run;profile=$Profile}|ConvertTo-Json))
    Write-Output "Owned workspace persistence review $($review.Id) ($Label)"
    $owned=@{window=$null}
    Wait-Until {$review.Refresh();if(!(Model).brush_ready){return $false};$owned.window=Owned-DrawingWindow $review;$null -ne $owned.window} 'Canvas did not start' 45
    $script:root=$owned.window.Root
    Wait-Until {(Model).windows_workspace.ready -and !(Model).windows_workspace.busy} 'Workspace did not open' 45
    Wait-Until {@((Model).panel_measurements|Where-Object {$_.panel -eq 'brushes' -and $_.content_height -gt 0 -and $_.content_height -ne 320}).Count -eq 1} 'Adopted workspace did not receive native measurements'
}
function Close([string]$ExpectedDiagnostic='') {
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close
    if(![string]::Equals(([Text.Encoding]::UTF8.GetString([IO.File]::ReadAllBytes($stderr))).Replace("`r`n","`n"),$ExpectedDiagnostic,[StringComparison]::Ordinal)){throw 'Native stderr requires inspection'}
}
function Layout { (Model).state.workspace | ConvertTo-Json -Depth 80 -Compress }
try {
    Enter-CapyEnvironment
    $env:CAPY_TRACE_UI='1';$env:CAPY_SMOKE_TEST='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
    $profile=Join-Path $run 'profile'
    Launch $profile 'initial'
    $before=Layout
    (Control 'tool-setting-size-slider' -Type ([System.Windows.Automation.ControlType]::Slider)).GetCurrentPattern([System.Windows.Automation.RangeValuePattern]::Pattern).SetValue(0.5)
    Wait-Until {(Model).state.brush.diameter -eq 32} 'Brush value was not applied'
    Invoke 'panel-tab-stats'
    Wait-Until {$null -ne (Find 'renderer-stat-6') -and (Layout) -ne $before} 'Workspace layout did not change'
    $expected=Layout
    Wait-Until {!(Model).windows_workspace.dirty -and !(Model).windows_workspace.saving} 'Workspace autosave did not complete'
    if(!(Test-Path -LiteralPath (Join-Path $profile 'data/workspaces/workspaces.sqlite3'))){throw 'Workspace database was not created'}
    (Control 'tool-setting-size-slider' -Type ([System.Windows.Automation.ControlType]::Slider)).GetCurrentPattern([System.Windows.Automation.RangeValuePattern]::Pattern).SetValue(0.61)
    Wait-Until {(Model).state.brush.diameter -ne 32} 'Final tool edit did not apply'
    $expectedSize=(Model).state.brush.diameter
    Close
    Launch $profile 'restart'
    Wait-Until {(Model).state.brush.diameter -eq $expectedSize} 'Restart did not restore the final tool value'
    if((Layout) -ne $expected){throw 'Restart did not restore the saved layout'}
    if(!(Find 'renderer-stat-6') -and (Find 'column-icon-stats')){Invoke 'column-icon-stats'}
    Wait-Until {$null -ne (Find 'renderer-stat-6')} 'Restored Diagnostics did not appear natively'
    Close
    $broken=Join-Path $run 'unreadable'
    $database=Join-Path $broken 'data/workspaces/workspaces.sqlite3'
    [IO.Directory]::CreateDirectory((Split-Path -Parent $database))|Out-Null
    [IO.File]::WriteAllText($database,'isolated fixture: unreadable workspace database')
    $unreadableHash=(Get-FileHash -LiteralPath $database -Algorithm SHA256).Hash
    $unreadableDiagnostic="Workspace storage DamagedStorage: Workspace storage: file is not a database`n"
    $script:startupNotice=$null
    $CapyEach={if(!$script:startupNotice -and $review){$text=(Model).state.notice.text;if($text){$script:startupNotice=$text}}}
    Launch $broken 'unreadable'
    Wait-Until {$null -ne $script:startupNotice} 'Unreadable storage did not raise the startup notice' 15
    $CapyEach=$null
    $tag=(Model).windows_active_tag
    $expectedNotice=(Catalog-Text $tag 'workspace-storage-damaged-notice' 'workspace').Replace('{ common-app-name }',(Catalog-Text $tag 'common-app-name' 'common'))
    if($script:startupNotice -cne $expectedNotice){throw "Unexpected startup notice: $($script:startupNotice)"}
    if((Model).windows_workspace.error){throw 'Unreadable storage still reported a workspace error'}
    $damaged=@(Get-ChildItem -LiteralPath (Split-Path -Parent $database) -Filter 'workspaces.damaged.*.sqlite3' -File)
    if($damaged.Count -ne 1 -or (Get-FileHash -LiteralPath $damaged[0].FullName -Algorithm SHA256).Hash -ne $unreadableHash){throw 'The damaged workspace database was not set aside byte for byte'}
    if(!(Test-Path -LiteralPath $database)){throw 'A fresh workspace database was not created'}
    $freshSize=(Model).state.brush.diameter
    (Control 'tool-setting-size-slider' -Type ([System.Windows.Automation.ControlType]::Slider)).GetCurrentPattern([System.Windows.Automation.RangeValuePattern]::Pattern).SetValue(0.8)
    Wait-Until {(Model).state.brush.diameter -ne $freshSize} 'Editing was refused with unreadable storage'
    $recoveredSize=(Model).state.brush.diameter;$recoveredBefore=Layout
    Invoke 'panel-tab-stats'
    Wait-Until {$null -ne (Find 'renderer-stat-6') -and (Layout) -ne $recoveredBefore} 'The fresh workspace layout did not change'
    $recoveredLayout=Layout
    Wait-Until {!(Model).windows_workspace.dirty -and !(Model).windows_workspace.saving} 'The fresh workspace autosave did not complete'
    Close $unreadableDiagnostic
    if((Get-FileHash -LiteralPath $damaged[0].FullName -Algorithm SHA256).Hash -ne $unreadableHash){throw 'The damaged workspace copy was overwritten'}
    Launch $broken 'reopened'
    if((Model).windows_workspace.error){throw 'Unreadable storage did not reopen'}
    Wait-Until {(Model).state.brush.diameter -eq $recoveredSize} 'The fresh workspace did not restore its brush value'
    if((Layout) -ne $recoveredLayout){throw 'The fresh workspace did not restore its saved layout'}
    if(!(Find 'renderer-stat-6') -and (Find 'column-icon-stats')){Invoke 'column-icon-stats'}
    Wait-Until {$null -ne (Find 'renderer-stat-6')} 'The fresh workspace did not restore native Diagnostics'
    if((Model).state.notice){throw 'A fresh workspace restart repeated a startup notice'}
    Close
    if(@(Get-ChildItem -LiteralPath (Split-Path -Parent $database) -Filter 'workspaces.damaged.*.sqlite3' -File).Count -ne 1 -or (Get-FileHash -LiteralPath $damaged[0].FullName -Algorithm SHA256).Hash -ne $unreadableHash){throw 'Restart changed the damaged workspace copy'}
    [pscustomobject]@{autosave='passed';restart_layout='passed';restart_tool_values='passed';native_measurements='passed';final_edit_close='passed';damaged_storage_preserved_with_notice='passed';fresh_workspace_editing='passed';fresh_workspace_restart='passed';zero_exit='passed';scope='native UI Automation and isolated SQLite persistence; full manager UI and physical input remain separate'}|ConvertTo-Json
}catch{
    [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{
    Exit-CapyEnvironment
}
