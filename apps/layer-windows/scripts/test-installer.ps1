param([Parameter(Mandatory)][string]$ResultFile)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyWaitSeconds=45
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$result=Get-Content -LiteralPath (Resolve-Path -LiteralPath $ResultFile).Path -Raw|ConvertFrom-Json
if(!$result.test_identity){throw 'Install only an installer built with -TestIdentity.'}
$target=Join-Path $env:LOCALAPPDATA ('Programs\'+$result.name)
$uninstall='HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\'+$result.key
$progid='HKCU:\Software\Classes\'+$result.progid
$shortcut=Join-Path ([Environment]::GetFolderPath('Programs')) ($result.name+'.lnk')
if((Test-Path -LiteralPath $target) -or (Test-Path -LiteralPath $uninstall)){throw 'Remove the previous installer test first.'}
$run=Join-Path $repo ('artifacts/windows/installer-review/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$review=$null
function Default([string]$Key){(Get-Item -LiteralPath $Key).GetValue('')}
function Install{(Start-Process -FilePath $result.installer -ArgumentList '/S' -Wait -PassThru).ExitCode}
function Assert-Installed{
    $manifest=Get-Content -LiteralPath (Join-Path $target 'package-manifest.json') -Raw|ConvertFrom-Json
    foreach($file in $manifest.files){
        $path=Join-Path $target $file.path
        if((Get-Item -LiteralPath $path).Length -ne $file.bytes -or (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash -ne $file.sha256){throw "Installed file does not match its hash: $($file.path)"}
    }
}
try {
    if((Install) -ne 0){throw 'The installer failed.'}
    Assert-Installed
    if($result.signed -and (Get-AuthenticodeSignature -LiteralPath (Join-Path $target 'Uninstall.exe')).Status -ne 'Valid'){throw 'The installed uninstaller is not signed.'}
    $entry=Get-ItemProperty -LiteralPath $uninstall
    if($entry.DisplayName -ne $result.name -or $entry.DisplayVersion -ne $result.version -or $entry.InstallLocation -ne $target){throw 'The uninstall entry does not describe the installed app.'}
    if((Default 'HKCU:\Software\Classes\.capy') -ne $result.progid){throw '.capy drawings do not open with the installed app.'}
    if((Default ($progid+'\shell\open\command')) -ne ('"'+$target+'\CapyCanvas.exe" "%1"')){throw 'The .capy open command does not start the installed app.'}
    if(!(Test-Path -LiteralPath $shortcut)){throw 'The Start menu shortcut is missing.'}
    $drawing=Join-Path $run 'Installed drawing.capy'
    Copy-Item -LiteralPath (Join-Path $repo 'crates/layer-core/src/package/codec/fixtures/authored-filters.capy') -Destination $drawing
    Enter-CapyEnvironment
    $env:CAPY_STORAGE_DIR=Join-Path $run 'profile'
    $review=Start-Process -FilePath $drawing -WorkingDirectory $run -PassThru
    $null=$review.Handle
    Write-Output "Owned installer review $($review.Id): $run"
    Wait-Until {(Get-CimInstance Win32_Process -Filter "ProcessId=$($review.Id)").CommandLine} 'The drawing did not start the installed app'
    if((Get-CimInstance Win32_Process -Filter "ProcessId=$($review.Id)").CommandLine -ne ('"'+$target+'\CapyCanvas.exe" "'+$drawing+'"')){throw 'The drawing did not open with the installed app.'}
    if((Install) -eq 0){throw 'Setup replaced the app while it was running.'}
    Assert-Installed
    Stop-Process -Id $review.Id -Force;$review.WaitForExit()
    if((Install) -ne 0){throw 'Setup did not replace the closed app.'}
    Assert-Installed
    $remove=Start-Process -FilePath (Join-Path $target 'Uninstall.exe') -ArgumentList '/S',('_?='+$target) -Wait -PassThru
    if($remove.ExitCode -ne 0){throw "The uninstaller failed: $($remove.ExitCode)"}
    if(Test-Path -LiteralPath $uninstall){throw 'The uninstall entry remains.'}
    if(Test-Path -LiteralPath $progid){throw 'The .capy file type remains.'}
    if((Test-Path -LiteralPath 'HKCU:\Software\Classes\.capy') -and (Default 'HKCU:\Software\Classes\.capy') -eq $result.progid){throw '.capy still opens with the removed app.'}
    if(Test-Path -LiteralPath $shortcut){throw 'The Start menu shortcut remains.'}
    $left=@(Get-ChildItem -LiteralPath $target -Recurse -Force|ForEach-Object Name)
    if($left.Count -ne 1 -or $left[0] -ne 'Uninstall.exe'){throw "The uninstaller left app files: $($left -join ', ')"}
    Remove-Item -LiteralPath $target -Recurse -Force
    [ordered]@{installer_sha256=(Get-FileHash -LiteralPath $result.installer -Algorithm SHA256).Hash.ToLowerInvariant();source_commit=$result.source_commit;development=$result.development;install='passed';files='passed';association='passed';running_app_kept='passed';upgrade='passed';uninstall='passed';scope='opening the drawing needs a hardware GPU and is covered by exercise-file-activation.ps1'}|ConvertTo-Json|Tee-Object -FilePath (Join-Path $run 'result.json')
}catch{
    $_.ToString()|Set-Content (Join-Path $run 'failure.txt')
    if($review -and !$review.HasExited){Stop-Process -Id $review.Id -Force;$review.WaitForExit()}
    if(Test-Path -LiteralPath (Join-Path $target 'Uninstall.exe')){
        Start-Process -FilePath (Join-Path $target 'Uninstall.exe') -ArgumentList '/S',('_?='+$target) -Wait
        Remove-Item -LiteralPath $target -Recurse -Force
    }
    throw
}finally{
    Exit-CapyEnvironment
}
