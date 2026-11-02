param([Parameter(Mandatory)][string]$Executable,[ValidateSet('dark','light')][string]$Theme='dark')
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
if(!$env:CAPY_PRIVATE_DESKTOP){throw 'Run popup fixture through test-desktop.ps1'}
[CapyRowPointer]::VerifyPrivateDesktop($env:CAPY_PRIVATE_DESKTOP)
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$run=Join-Path $repo ('artifacts/windows/popup-fixture/'+[guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
Update-CapyFixtureContext {param($context);$context.Clear();$context.run=$run;$context.windows=@();$context.closes=@()}
$Executable=(Resolve-Path -LiteralPath $Executable).Path
@{executable=$Executable;sha256=(Get-FileHash -LiteralPath $Executable).Hash;theme=$Theme;desktop=$env:CAPY_PRIVATE_DESKTOP}|ConvertTo-Json|Set-Content (Join-Path $run 'invocation.json')
$beforePath=$env:CAPY_POPUP_FIXTURE;$beforeTheme=$env:CAPY_POPUP_THEME;$process=$null
try {
 $env:CAPY_POPUP_FIXTURE=Join-Path $run 'input.json';$env:CAPY_POPUP_THEME=$Theme
 $process=Start-Process -FilePath $Executable -PassThru -RedirectStandardOutput (Join-Path $run 'stdout.log') -RedirectStandardError (Join-Path $run 'stderr.log')
 $null=$process.Handle
 Update-CapyFixtureContext {param($context);$context.process_id=$process.Id;$context.trace_directory=$run}
 if(!$process.WaitForExit(30000)){throw 'Popup fixture timed out after 30 seconds'}
 if($process.ExitCode -ne 0){throw "Popup fixture exited $($process.ExitCode); see $run"}
 $result=Get-Content -LiteralPath (Join-Path $run 'popup-fixture-result.json') -Raw|ConvertFrom-Json
 if($result.exit_code -ne 0 -or $result.passed -ne 14 -or $result.ime_passed -ne 16){throw "Popup fixture assertions incomplete; see $run"}
 Write-Output "Popup fixture passed 14 popup and 16 workspace/default IME cases ($Theme); artifacts: $run"
} finally {
 $env:CAPY_POPUP_FIXTURE=$beforePath;$env:CAPY_POPUP_THEME=$beforeTheme
 if($process -and !$process.HasExited){Stop-Process -Id $process.Id -Force}
}
