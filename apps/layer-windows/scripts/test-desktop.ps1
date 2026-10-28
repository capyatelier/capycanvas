param(
    [Parameter(Mandatory)][string]$Script,
    [hashtable]$Parameters=@{},
    [ValidateRange(1,3600)][int]$TimeoutSeconds=600,
    [switch]$ConfigureVmDisplay
)
$ErrorActionPreference='Stop'
$desktopDirectory=[Environment]::GetFolderPath([Environment+SpecialFolder]::DesktopDirectory)
if(!$desktopDirectory -or ![IO.Directory]::Exists($desktopDirectory)){
    throw 'UI fixtures need an interactive Windows account with an available Desktop folder.'
}
$Script=(Resolve-Path -LiteralPath $Script).Path
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$run=Join-Path $repo ('artifacts/windows/test-desktop/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$desktop='Capy-Test-'+[Guid]::NewGuid().ToString('N')
$invocation=@{script=$Script;parameters=$Parameters;desktop=$desktop}
if($ConfigureVmDisplay){$invocation.vm_display=(Resolve-Path -LiteralPath (Join-Path $repo 'tools/windows-vm/set-display.ps1')).Path}
$invocation|Export-Clixml -LiteralPath (Join-Path $run 'invocation.clixml')
@'
$ErrorActionPreference='Stop'
$invocation=Import-Clixml -LiteralPath (Join-Path $PSScriptRoot 'invocation.clixml')
$env:CAPY_PRIVATE_DESKTOP=$invocation.desktop
$env:CAPY_FIXTURE_CONTEXT=Join-Path $PSScriptRoot 'fixture.json'
Start-Transcript -LiteralPath (Join-Path $PSScriptRoot 'transcript.log')|Out-Null
try {
    if($invocation.vm_display){& $invocation.vm_display}
    $arguments=$invocation.parameters
    & $invocation.script @arguments
    if(!$?){exit 1}
} catch {
    $_|Out-String|Set-Content -LiteralPath (Join-Path $PSScriptRoot 'failure.txt')
    Write-Error $_ -ErrorAction Continue
    exit 1
} finally {Stop-Transcript|Out-Null}
'@|Set-Content -LiteralPath (Join-Path $run 'invoke.ps1') -Encoding UTF8
if(!('CapyPrivateDesktop' -as [type])){Add-Type -Path (Join-Path $PSScriptRoot 'TestDesktop.cs')}
$powershell=Join-Path $env:WINDIR 'System32/WindowsPowerShell/v1.0/powershell.exe'
Write-Output "Private desktop artifacts: $run"
$code=1
$modulePath=$env:PSModulePath
try{$env:PSModulePath=$null;$code=[CapyPrivateDesktop]::Run($powershell,(Join-Path $run 'invoke.ps1'),$run,$desktop,$TimeoutSeconds)}
catch{$_|Out-String|Set-Content -LiteralPath (Join-Path $run 'desktop-failure.txt');throw}
finally{$env:PSModulePath=$modulePath;@{exit_code=$code;desktop=$desktop;run=$run}|ConvertTo-Json|Set-Content -LiteralPath (Join-Path $run 'result.json')}
if($code){throw "Private desktop test exited $code; see $run"}
