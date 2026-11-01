param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
[CapyRowPointer]::VerifyPrivateDesktop($env:CAPY_PRIVATE_DESKTOP)
$repo=(Resolve-Path(Join-Path $PSScriptRoot '../../..')).Path
$run=Join-Path $repo ('artifacts/windows/navigator-first-allocation/'+[guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null;$owned=$null
try{
 Enter-CapyEnvironment @('CAPY_NAVIGATOR_LAYOUT')
 $env:CAPY_STORAGE_DIR=Join-Path $run 'profile';$env:CAPY_NAVIGATOR_LAYOUT=Join-Path $run 'allocation.json'
 $Executable=(Resolve-Path -LiteralPath $Executable).Path
 $owned=Start-Process -FilePath $Executable -WorkingDirectory (Split-Path $Executable) -WindowStyle Hidden -PassThru -RedirectStandardError (Join-Path $run 'stderr.log');$null=$owned.Handle
 if(!$owned.WaitForExit(30000)){throw 'First-allocation fixture timed out'}
 if(!(Test-Path -LiteralPath $env:CAPY_NAVIGATOR_LAYOUT)){throw 'First-allocation result missing'}
 $result=Get-Content -LiteralPath $env:CAPY_NAVIGATOR_LAYOUT -Raw|ConvertFrom-Json
 if((Get-Item -LiteralPath (Join-Path $run 'stderr.log')).Length){throw 'First-allocation fixture wrote stderr'}
 if($owned.ExitCode-ne0-or!$result.passed-or$result.samples.Count-ne3-or@($result.samples|Where-Object {!$_.passed}).Count){throw 'Navigator first-allocation invariant failed'}
 @{executable=$Executable;sha256=(Get-FileHash -LiteralPath $Executable).Hash;exit_code=$owned.ExitCode;samples=$result.samples.Count}|ConvertTo-Json|Set-Content (Join-Path $run 'result.json')
 Write-Output "Navigator first allocation verified; artifacts: $run"
}finally{
 if($owned-and!$owned.HasExited){'Forced cleanup after failed allocation check'|Set-Content (Join-Path $run 'cleanup-failure.txt');Stop-Process -Id $owned.Id}
 Exit-CapyEnvironment
}
