param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyWaitSeconds=45
Add-Type -AssemblyName System.Drawing
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$run=Join-Path $repo ('artifacts/windows/file-activation/'+[Guid]::NewGuid().ToString('N'))
$elsewhere=Join-Path $run 'elsewhere'
[IO.Directory]::CreateDirectory($elsewhere)|Out-Null
$CapyTraceDirectory=$run
$review=$null;$other=$null
function Tabs {@((Model).windows_tabs.tabs)}
function Image([string]$Name,[Drawing.Color]$Color){
    $path=Join-Path $run $Name
    $bitmap=[Drawing.Bitmap]::new(24,16,[Drawing.Imaging.PixelFormat]::Format32bppArgb)
    try{for($x=0;$x -lt 24;$x++){for($y=0;$y -lt 16;$y++){$bitmap.SetPixel($x,$y,$Color)}};$bitmap.Save($path,[Drawing.Imaging.ImageFormat]::Png)}finally{$bitmap.Dispose()}
    $path
}
function Windows {$path=Join-Path $run "windows-$($review.Id).json";if([IO.File]::Exists($path)){@((Read-Snapshot $path).windows)}else{@()}}
function Forward([string]$Directory,[string[]]$Arguments){
    $start=@{FilePath=$Executable;WorkingDirectory=$Directory;PassThru=$true}
    if($Arguments){$start.ArgumentList=$Arguments|ForEach-Object {'"'+$_+'"'}}
    $launch=Start-Process @start
    if(!$launch.WaitForExit(20000)){Stop-Process -Id $launch.Id -Force;throw 'A launch did not hand itself to the running app'}
    if($launch.ExitCode -ne 0){throw "A forwarding launch failed: $($launch.ExitCode)"}
}
try {
    Enter-CapyEnvironment
    $env:CAPY_STORAGE_DIR=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1'
    $review=Start-Process -FilePath $Executable -WorkingDirectory $run -PassThru -RedirectStandardError (Join-Path $run 'stderr.log')
    $null=$review.Handle
    Write-Output "Owned file activation review $($review.Id): $run"
    Wait-Until {$m=Model;$m.brush_ready -and $m.windows_workspace.ready -and !$m.windows_workspace.busy -and @($m.state.commands|Where-Object {$_.id -eq 'open_document' -and $_.enabled}).Count} 'File activation review did not start' 90
    $before=(Tabs).Count
    $first=Image 'First 日本語.png' ([Drawing.Color]::FromArgb(255,200,40,40))
    $second=Image 'Second drawing.png' ([Drawing.Color]::FromArgb(255,40,60,200))
    Forward $elsewhere @($first,$second)
    Wait-Until {(Tabs).Count -eq $before+2 -and !(Model).state.document_file.busy} 'Forwarded files did not open as drawings in the running window' 60
    $titles=@(Tabs|ForEach-Object title)
    if($titles[-2] -notlike 'First*' -or $titles[-1] -notlike 'Second drawing*'){throw "Forwarded drawings opened out of order: $($titles -join ', ')"}
    if(@((Model).windows_tabs.tabs)[-1].id -ne (Model).windows_tabs.selected){throw 'The last forwarded drawing is not selected'}
    $null=Image 'Relative.png' ([Drawing.Color]::FromArgb(255,40,160,60))
    Forward $run @('Relative.png')
    Wait-Until {(Tabs).Count -eq $before+3 -and (@(Tabs)[-1].title -like 'Relative*')} 'A relative path was not resolved by the launching process' 60
    $original=@(Windows)
    Forward $elsewhere @()
    Wait-Until {@(Windows).Count -eq 2} 'A launch without files did not open a new window in the running app' 60
    if((Tabs).Count -ne $before+3){throw 'A launch without files changed the existing window'}
    $added=@(Windows|Where-Object {$_.hwnd -notin $original.hwnd})[0]
    [System.Windows.Automation.AutomationElement]::FromHandle([IntPtr][int64]$added.hwnd).GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).Close()
    Wait-Until {@(Windows).Count -eq 1} 'The window opened by a later launch did not close' 30
    $env:CAPY_STORAGE_DIR=Join-Path $run 'other-profile'
    $other=Start-Process -FilePath $Executable -WorkingDirectory $elsewhere -ArgumentList ('"'+$first+'"') -PassThru
    Start-Sleep -Seconds 6
    if($other.HasExited){throw 'A launch with other app storage was forwarded to this app'}
    Stop-Process -Id $other.Id -Force;$other.WaitForExit()
    $env:CAPY_STORAGE_DIR=Join-Path $run 'profile'
    if((Tabs).Count -ne $before+3){throw 'Another profile changed this window'}
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved -StateDirectory $run
    if(!$review.WaitForExit(20000)){throw 'File activation review did not close'}
    if((Get-Item (Join-Path $run 'stderr.log')).Length){throw 'Native stderr requires review'}
    [pscustomobject]@{forwarded_in_order='passed';relative_path='passed';launch_opens_window='passed';storage_isolated='passed';evidence=$run}|ConvertTo-Json|Tee-Object -FilePath (Join-Path $run 'results.json')
} finally {
    if($other -and !$other.HasExited){Stop-Process -Id $other.Id -Force}
    if($review){$review.Refresh();if(!$review.HasExited){Stop-Process -Id $review.Id -Force}}
    Exit-CapyEnvironment
}
