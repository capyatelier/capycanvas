param([Parameter(Mandatory)][string]$Executable,[ValidateSet('dark','light')][string]$Theme='dark')
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
function Drop-Files([string[]]$Paths,[string]$Target){
    $bounds=(Control $Target -Arranged).Current.BoundingRectangle
    $scale=[CapyWindowApi]::GetDpiForWindow($drawingWindow)/96.
    $targetName=switch($Target){'drawing-canvas'{'canvas'} 'drawing-tabs'{'tabs'} default{throw "Unknown file drop target: $Target"}}
    $payload=@{request=[guid]::NewGuid().ToString('N');paths=@($Paths);target=$targetName;point=@(($bounds.Width/2/$scale),($bounds.Height/2/$scale))}|ConvertTo-Json -Compress
    $arguments=Control 'test-file-drop-arguments'
    $arguments.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($payload)
    Invoke 'Test file drop' -Name
    $result=@{value=$null}
    Wait-Until {
        $status=$arguments.Current.ItemStatus
        if(!$status){return $false}
        $reply=$status|ConvertFrom-Json
        if($reply.request -cne $payload){return $false}
        if($reply.error){throw $reply.error}
        $result.value=$reply;$true
    } 'The file drop command did not acknowledge its request'
    [string]$result.value.effect
}
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
    [IO.File]::WriteAllText((Settings-File),(@{language=@{Explicit='en'};theme=$Theme}|ConvertTo-Json -Depth 4))
    $smoke=$env:CAPY_SMOKE_TEST;$env:CAPY_SMOKE_TEST='1'
    try{
    $review=Start-Process -FilePath $Executable -WorkingDirectory $run -PassThru -RedirectStandardError (Join-Path $run 'stderr.log')
    }finally{Restore-CapyEnvironmentValue 'CAPY_SMOKE_TEST' $smoke}
    $null=$review.Handle
    Write-Output "Owned file activation review $($review.Id): $run"
    Wait-Until {$m=Model;(Review-Ready $m) -and $m.state.theme -eq $Theme -and !$m.windows_workspace.busy -and @($m.state.commands|Where-Object {$_.id -eq 'open_document' -and $_.enabled}).Count} 'File activation review did not start' 90
    $before=@(Tabs).Count
    $first=Image 'First 日本語.png' ([Drawing.Color]::FromArgb(255,200,40,40))
    $second=Image 'Second drawing.png' ([Drawing.Color]::FromArgb(255,40,60,200))
    Forward $elsewhere @($first,$second)
    Wait-Until {@(Tabs).Count -eq $before+2 -and !(Model).state.document_file.busy} 'Forwarded files did not open as drawings in the running window' 60
    $titles=@(Tabs|ForEach-Object title)
    if($titles[-2] -notlike 'First*' -or $titles[-1] -notlike 'Second drawing*'){throw "Forwarded drawings opened out of order: $($titles -join ', ')"}
    if(@((Model).windows_tabs.tabs)[-1].id -ne (Model).windows_tabs.selected){throw 'The last forwarded drawing is not selected'}
    $null=Image 'Relative.png' ([Drawing.Color]::FromArgb(255,40,160,60))
    Forward $run @('Relative.png')
    Wait-Until {@(Tabs).Count -eq $before+3 -and (@(Tabs)[-1].title -like 'Relative*')} 'A relative path was not resolved by the launching process' 60
    $original=@(Windows)
    Forward $elsewhere @()
    Wait-Until {$windows=@(Windows);$added=@($windows|Where-Object {$_.hwnd -notin $original.hwnd});$windows.Count -eq 2 -and $added.Count -eq 1 -and $added[0].ready -eq $true} 'A launch without files did not open a new window in the running app' 60
    if(@(Tabs).Count -ne $before+3){throw 'A launch without files changed the existing window'}
    $added=@(Windows|Where-Object {$_.hwnd -notin $original.hwnd})[0]
    (Owned-DrawingWindow $review -WindowHandle $added.hwnd -TraceDirectory $run).Root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).Close()
    Wait-Until {@(Windows).Count -eq 1} 'The window opened by a later launch did not close' 30
    $env:CAPY_STORAGE_DIR=Join-Path $run 'other-profile'
    $other=Start-Process -FilePath $Executable -WorkingDirectory $elsewhere -ArgumentList ('"'+$first+'"') -PassThru
    Start-Sleep -Seconds 6
    if($other.HasExited){throw 'A launch with other app storage was forwarded to this app'}
    Stop-Process -Id $other.Id -Force;$other.WaitForExit()
    $env:CAPY_STORAGE_DIR=Join-Path $run 'profile'
    if(@(Tabs).Count -ne $before+3){throw 'Another profile changed this window'}
    $drawing=@{window=$null}
    Wait-Until {$drawing.window=Owned-DrawingWindow $review;$null -ne $drawing.window} 'The drawing window is unavailable'
    $root=$drawing.window.Root;$drawingWindow=$drawing.window.Handle
    [CapyWindowApi]::SetForegroundWindow($drawingWindow)|Out-Null
    $saved=Join-Path $run 'Dropped drawing.capy'
    Save-ProjectAs $saved
    $photo=Image 'Dropped photo.png' ([Drawing.Color]::FromArgb(255,220,160,30))
    $source=Model;$layers=@($source.state.layers|Select-Object id,name,paint_revision,mask_revision)|ConvertTo-Json -Compress
    $revision=$source.state.document_file.revision;$tab=$source.windows_tabs.selected;$count=@(Tabs).Count
    $effect=Drop-Files @($photo) 'drawing-canvas'
    if($effect -ne 'Copy'){throw "The canvas refused the dropped photo: $effect"}
    Wait-Until {$m=Model;$m.state.layer_tools.tool -eq 'transform' -and @($m.state.layers).Count -eq @($source.state.layers).Count+1 -and !$m.state.document_file.busy} 'The canvas photo drop did not open Place' 60
    if(@(Tabs).Count -ne $count -or (Model).windows_tabs.selected -ne $tab){throw 'Canvas placement opened or selected another drawing'}
    Invoke-Id 'canvas-bar-apply_transform'
    Wait-Until {$m=Model;$m.state.layer_tools.tool -ne 'transform' -and $m.state.document_file.revision -gt $revision -and @($m.state.layers).Count -eq @($source.state.layers).Count+1} 'Place did not accept the dropped photo' 60
    $placed=(Model).state.document_file.revision
    Invoke 'Undo' -Name
    Wait-Until {$m=Model;$m.state.document_file.revision -gt $placed -and (@($m.state.layers|Select-Object id,name,paint_revision,mask_revision)|ConvertTo-Json -Compress) -ceq $layers} 'One Undo did not restore the drawing before the photo drop' 60
    $effect=Drop-Files @($photo,$saved) 'drawing-tabs'
    if($effect -ne 'Copy'){throw "The drawing tabs refused an image and a drawing: $effect"}
    Wait-Until {@(Tabs).Count -eq $before+5 -and !(Model).state.document_file.busy} 'Files dropped on the drawing tabs did not open as drawings' 60
    $titles=@(Tabs|Select-Object -Last 2|ForEach-Object title)
    if($titles[0] -notlike 'Dropped photo*' -or $titles[1] -notlike 'Dropped drawing*'){throw "Dropped files opened out of order: $($titles -join ', ')"}
    if(@(Tabs)[-1].id -ne (Model).windows_tabs.selected){throw 'The last dropped drawing is not selected'}
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved -StateDirectory $run
    if(!$review.WaitForExit(20000)){throw 'File activation review did not close'}
    if((Get-Item (Join-Path $run 'stderr.log')).Length){throw 'Native stderr requires review'}
    $env:CAPY_STORAGE_DIR=Join-Path $run 'cold-profile'
    [IO.File]::WriteAllText((Settings-File),(@{language=@{Explicit='en'};theme=$Theme}|ConvertTo-Json -Depth 4))
    $review=Start-Process -FilePath $Executable -WorkingDirectory $run -ArgumentList ('"'+$saved+'"') -PassThru -RedirectStandardError (Join-Path $run 'cold-stderr.log')
    $null=$review.Handle
    Wait-Until {$m=Model;$m.state.document_file.location.uri -eq $saved -and (Review-Ready $m) -and $m.state.theme -eq $Theme} 'Launching the closed app with a saved drawing did not open it' 90
    $opened=Model
    if($opened.error -or $opened.state.host_error -or @($opened.state.requests).Count -or $opened.state.document_file.busy){throw 'The drawing passed at launch left an error or pending file operation'}
    Wait-Until {$drawing.window=Owned-DrawingWindow $review;$null -ne $drawing.window} 'The launched drawing window is unavailable'
    $root=$drawing.window.Root
    Capture 'cold-launch' -WithModel -Composed
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -StateDirectory $run
    if(!$review.WaitForExit(20000)){throw 'The app launched with a saved drawing did not close'}
    if((Get-Item (Join-Path $run 'cold-stderr.log')).Length){throw 'Cold launch stderr requires review'}
    [pscustomobject]@{cold_launch_drawing='passed';forwarded_in_order='passed';relative_path='passed';launch_opens_window='passed';storage_isolated='passed';tab_drop='passed';canvas_photo_drop_place_and_undo='passed';drop_transport='real DataPackage below OLE; OLE delivery unverified';theme=$Theme;evidence=$run}|ConvertTo-Json|Tee-Object -FilePath (Join-Path $run 'results.json')
} finally {
    if($other -and !$other.HasExited){Stop-Process -Id $other.Id -Force}
    if($review){$review.Refresh();if(!$review.HasExited){Stop-Process -Id $review.Id -Force}}
    Exit-CapyEnvironment
}
