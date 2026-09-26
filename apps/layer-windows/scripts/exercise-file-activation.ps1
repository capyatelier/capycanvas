param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName System.Drawing
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$run=Join-Path $repo ('artifacts/windows/file-activation/'+[Guid]::NewGuid().ToString('N'))
$elsewhere=Join-Path $run 'elsewhere'
[IO.Directory]::CreateDirectory($elsewhere)|Out-Null
$names=@('CAPY_SETTINGS_DIRECTORY','CAPY_TRACE_UI','CAPY_SMOKE_TEST','CAPY_TEST_DISPLAY','CAPY_TEST_PRIMARY')
$previous=@{};foreach($name in $names){$previous[$name]=[Environment]::GetEnvironmentVariable($name,'Process')}
$review=$null;$other=$null
function Model {
    try{$s=Get-Content -LiteralPath (Join-Path $run 'ui-state.json') -Raw|ConvertFrom-Json;if($s.process_id -eq $review.Id -and $s.model.windows_isolated_settings){$s.model}}catch{}
}
function Tabs {@((Model).windows_tabs.tabs)}
function Wait-Until([scriptblock]$Check,[string]$Message,[int]$Seconds=45){
    $watch=[Diagnostics.Stopwatch]::StartNew()
    do{if(& $Check){return};$review.Refresh();if($review.HasExited){throw "Application exited: $($review.ExitCode)"};Start-Sleep -Milliseconds 80}while($watch.Elapsed.TotalSeconds -lt $Seconds)
    throw $Message
}
function Image([string]$Name,[Drawing.Color]$Color){
    $path=Join-Path $run $Name
    $bitmap=[Drawing.Bitmap]::new(24,16,[Drawing.Imaging.PixelFormat]::Format32bppArgb)
    try{for($x=0;$x -lt 24;$x++){for($y=0;$y -lt 16;$y++){$bitmap.SetPixel($x,$y,$Color)}};$bitmap.Save($path,[Drawing.Imaging.ImageFormat]::Png)}finally{$bitmap.Dispose()}
    $path
}
function Forward([string]$Directory,[string[]]$Arguments){
    $launch=Start-Process -FilePath $Executable -WorkingDirectory $Directory -ArgumentList ($Arguments|ForEach-Object {'"'+$_+'"'}) -PassThru
    if(!$launch.WaitForExit(20000)){Stop-Process -Id $launch.Id -Force;throw 'A file launch did not hand its files to the running window'}
    if($launch.ExitCode -ne 0){throw "A forwarding launch failed: $($launch.ExitCode)"}
}
try {
    foreach($name in $names){Remove-Item -LiteralPath ('Env:'+$name) -ErrorAction SilentlyContinue}
    $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1'
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
    $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'other-profile'
    $other=Start-Process -FilePath $Executable -WorkingDirectory $elsewhere -ArgumentList ('"'+$first+'"') -PassThru
    Start-Sleep -Seconds 6
    if($other.HasExited){throw 'A launch with another preferences profile was forwarded to this window'}
    Stop-Process -Id $other.Id -Force;$other.WaitForExit()
    $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'profile'
    if((Tabs).Count -ne $before+3){throw 'Another profile changed this window'}
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved -StateDirectory $run
    if(!$review.WaitForExit(20000)){throw 'File activation review did not close'}
    if((Get-Item (Join-Path $run 'stderr.log')).Length){throw 'Native stderr requires review'}
    [pscustomobject]@{forwarded_in_order='passed';relative_path='passed';profiles_isolated='passed';evidence=$run}|ConvertTo-Json|Tee-Object -FilePath (Join-Path $run 'results.json')
} finally {
    if($other -and !$other.HasExited){Stop-Process -Id $other.Id -Force}
    if($review){$review.Refresh();if(!$review.HasExited){Stop-Process -Id $review.Id -Force}}
    foreach($name in $names){if($null -eq $previous[$name]){Remove-Item -LiteralPath ('Env:'+$name) -ErrorAction SilentlyContinue}else{[Environment]::SetEnvironmentVariable($name,$previous[$name],'Process')}}
}
