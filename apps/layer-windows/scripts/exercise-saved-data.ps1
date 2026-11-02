param([Parameter(Mandatory)][string]$Executable,[ValidateSet('dark','light')][string]$Theme='dark')
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$run=Join-Path $repo ('artifacts/windows/saved-data/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$review=$null
function Launch([string]$Name){
    $script:review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError (Join-Path $run ($Name+'-stderr.log'))
    $null=$review.Handle
    Wait-Until {$review.Refresh();(Owned-DrawingWindow $review) -and (Model).windows_workspace.ready -and ($Name -eq 'damaged' -or (Model).brush_ready)} 'Saved-data window did not become ready' 120
    [CapyWindowApi]::ShowWindow((Owned-DrawingWindow $review).Handle,3)|Out-Null
    $owned=@{window=$null}
    Wait-Until {$owned.window=Owned-DrawingWindow $review;$null -ne $owned.window} 'Owned saved-data window is unavailable'
    $script:root=$owned.window.Root
}
function Close {
    Wait-Until {
        $model=Model
        $status=$root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'canvas-status'))
        $model.windows_workspace.ready -and !$model.windows_workspace.busy -and !$model.windows_workspace.dirty -and !$model.windows_workspace.saving -and !$model.windows_workspace.switcher_busy -and !$model.windows_recovery.restoring -and !$model.windows_recovery.busy -and (!$status -or $status.Current.IsOffscreen)
    } 'Saved-data startup and saves did not settle before closing' 120
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close
    $review.Dispose();$script:review=$null
}
try {
    Enter-CapyEnvironment
    $env:CAPY_STORAGE_DIR=Join-Path $run 'profile'
    $env:CAPY_TRACE_UI='1';$env:CAPY_SMOKE_TEST='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
    [IO.File]::WriteAllText((Settings-File),(@{language=@{Explicit='en'};theme=$Theme}|ConvertTo-Json -Depth 4))
    $workspaces=Join-Path $env:CAPY_STORAGE_DIR 'data/workspaces'
    [IO.Directory]::CreateDirectory($workspaces)|Out-Null
    $database=Join-Path $workspaces 'workspaces.sqlite3'
    [IO.File]::WriteAllBytes($database,[Text.Encoding]::UTF8.GetBytes('damaged SQLite store'))
    $hash=(Get-FileHash -LiteralPath $database).Hash
    Launch 'damaged'
    $expected='Your saved workspaces were damaged. Capy Canvas started new ones.'
    if((Model).state.notice.text -cne $expected){throw 'The damaged-store notice did not use the approved shared text'}
    Wait-Until {$notice=Find $expected -Name;$notice -and !$notice.Current.IsOffscreen} 'The damaged-store notice was not rendered'
    $copies=@(Get-ChildItem -LiteralPath $workspaces -Filter 'workspaces.damaged.*.sqlite3' -File)
    if($copies.Count -ne 1 -or (Get-FileHash -LiteralPath $copies[0].FullName).Hash -ne $hash){throw 'The damaged database was not kept byte for byte'}
    Capture "damaged-$Theme" -WithModel
    Close
    Launch 'restart'
    if((Model).state.notice){throw 'A clean second start repeated the damaged-store notice'}
    if((Get-FileHash -LiteralPath $copies[0].FullName).Hash -ne $hash){throw 'Restart changed the damaged copy'}
    Capture "restart-$Theme" -WithModel
    Close
    foreach($restore in @($false,$true)){
        $env:CAPY_STORAGE_DIR=Join-Path $run ("restore-"+$restore)
        [IO.File]::WriteAllText((Settings-File),(@{language=@{Explicit='en'};theme=$Theme}|ConvertTo-Json -Depth 4))
        $workspaces=Join-Path $env:CAPY_STORAGE_DIR 'data/workspaces'
        [IO.Directory]::CreateDirectory($workspaces)|Out-Null
        Copy-Item -LiteralPath (Join-Path $repo 'crates/layer-workspace/tests/compat/1.0.15/workspaces.sqlite3') -Destination (Join-Path $workspaces 'workspaces.unreadable.sqlite3')
        Launch ("restore-"+$restore)
        Wait-Until {(Model).windows_workspace.prompt -and (Find 'Keep Current' -Name)} 'The restore prompt did not render'
        $prompt=(Model).windows_workspace.prompt
        if($prompt.title -cne 'Restore Workspaces?' -or $prompt.message -cne 'An older version of Capy Canvas reset your workspaces, palettes and brush settings. Restoring replaces changes you made after the reset.' -or $prompt.confirm -cne 'Restore' -or $prompt.cancel -cne 'Keep Current'){throw 'The restore prompt did not use the approved shared text'}
        Capture ("restore-$restore-$Theme") -WithModel
        Invoke $(if($restore){'PrimaryButton'}else{'CloseButton'}) -Within (Control 'workspace-manager')
        $expected=if($restore){37}else{9}
        Wait-Until {$model=Model;!$model.windows_workspace.prompt -and $model.windows_workspace.ready -and $model.state.brush.diameter -eq $expected} 'The restore choice did not adopt its editing state' 120
        if((Model).windows_workspace.error){throw 'The restore choice left a workspace error'}
        $kept=@(Get-ChildItem -LiteralPath $workspaces -Filter 'workspaces.*.sqlite3' -File|Where-Object {$_.Name -ne 'workspaces.unreadable.sqlite3'})
        if($kept.Count -ne $(if($restore){2}else{1})){throw 'Restoration did not retain the expected version copies'}
        Close
        Launch ("restore-restart-"+$restore)
        if((Model).windows_workspace.prompt -or (Model).state.brush.diameter -ne $expected){throw 'Restart repeated the restore prompt or lost the chosen settings'}
        Close
    }
    @{theme=$Theme;damaged_notice='passed';preserved_bytes='passed';persistent_restart='passed';restore='passed';keep_current='passed';replaced_store_copy='passed'}|ConvertTo-Json
}catch{
    if($review -and !$review.HasExited){try{Capture 'failure' -WithModel}catch{}}
    throw
}finally{
    if($review -and !$review.HasExited){$null=(Close-OwnedDrawing $review);$null=$review.WaitForExit(30000)}
    if($review){$review.Dispose()}
    Exit-CapyEnvironment
}
