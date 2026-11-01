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
    Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero -and (Model).windows_workspace.ready -and ($Name -eq 'damaged' -or (Model).brush_ready)} 'Saved-data window did not become ready' 120
    [CapyWindowApi]::ShowWindow($review.MainWindowHandle,3)|Out-Null
    $owned=@{window=$null}
    Wait-Until {$owned.window=Owned-DrawingWindow $review;$null -ne $owned.window} 'Owned saved-data window is unavailable'
    $script:root=$owned.window.Root
}
function Close {
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
    @{theme=$Theme;damaged_notice='passed';preserved_bytes='passed';persistent_restart='passed'}|ConvertTo-Json
}catch{
    if($review -and !$review.HasExited){try{Capture 'failure' -WithModel}catch{}}
    throw
}finally{
    if($review -and !$review.HasExited){$null=$review.CloseMainWindow();$null=$review.WaitForExit(30000)}
    if($review){$review.Dispose()}
    Exit-CapyEnvironment
}
