$ErrorActionPreference='Stop'
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$run=Join-Path $repo ('artifacts/windows/native-test-process/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$helper=(Resolve-Path (Join-Path $PSScriptRoot '../scripts/NativeTestProcess.ps1')).Path
$release=Join-Path $run 'release'
$wrapper=$null
$previousEncoding=[Console]::OutputEncoding
try {
    [Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
    @'
$ErrorActionPreference='Stop'
[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
[Console]::Out.WriteLine('early stdout')
[Console]::Error.WriteLine('early stderr')
[Console]::Out.Flush();[Console]::Error.Flush()
$deadline=[DateTime]::UtcNow.AddSeconds(30)
while(!(Test-Path -LiteralPath (Join-Path $PSScriptRoot 'release'))) {
    if([DateTime]::UtcNow -ge $deadline) {exit 99}
    Start-Sleep -Milliseconds 50
}
$unicode=[char]::ConvertFromUtf32(0x1f3a8)+[char]0x2026
[Console]::Out.WriteLine('final stdout '+('x'*8178)+$unicode)
[Console]::Error.Write('final stderr '+('x'*8178)+$unicode)
exit 7
'@ | Set-Content -LiteralPath (Join-Path $run 'child.ps1') -Encoding utf8
    @'
param([string]$Helper)
$ErrorActionPreference='Stop'
[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
. $Helper
$launch=@{
    FilePath=(Get-Process -Id $PID).Path
    WorkingDirectory=$PSScriptRoot
    ArgumentList=@('-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',('"{0}"' -f (Join-Path $PSScriptRoot 'child.ps1')))
    PassThru=$true
    WindowStyle='Hidden'
    RedirectStandardOutput=Join-Path $PSScriptRoot 'child-stdout.log'
    RedirectStandardError=Join-Path $PSScriptRoot 'child-stderr.log'
}
exit (Invoke-NativeTestProcess $launch)
'@ | Set-Content -LiteralPath (Join-Path $run 'wrapper.ps1') -Encoding utf8
    $stdout=Join-Path $run 'stdout.log';$stderr=Join-Path $run 'stderr.log'
    $wrapper=Start-Process (Get-Process -Id $PID).Path -ArgumentList @('-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',('"{0}"' -f (Join-Path $run 'wrapper.ps1')),('"{0}"' -f $helper)) -WindowStyle Hidden -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    $null=$wrapper.Handle
    $deadline=[DateTime]::UtcNow.AddSeconds(15)
    do {
        if($wrapper.HasExited -or [DateTime]::UtcNow -ge $deadline) {throw 'Both child streams must be visible before the child exits.'}
        $earlyOutput=(Get-Content -LiteralPath $stdout) -contains 'early stdout'
        $earlyError=(Get-Content -LiteralPath $stderr) -contains 'early stderr'
        if(!$earlyOutput -or !$earlyError) {Start-Sleep -Milliseconds 50}
    } until($earlyOutput -and $earlyError)
    New-Item -ItemType File -Path $release|Out-Null
    if(!$wrapper.WaitForExit(15000)) {throw 'Native test wrapper did not exit after its child.'}
    $wrapper.WaitForExit()
    if($wrapper.ExitCode -ne 7) {throw "Native test wrapper lost child exit 7: $($wrapper.ExitCode)"}
    $unicode=[char]::ConvertFromUtf32(0x1f3a8)+[char]0x2026
    foreach($name in @('stdout','stderr')) {
        $actual=(Get-Content -LiteralPath (Join-Path $run "$name.log")) -join "`n"
        $retained=(Get-Content -LiteralPath (Join-Path $run "child-$name.log")) -join "`n"
        if($actual -cne ("early $name`nfinal $name "+('x'*8178)+$unicode) -or $actual -cne $retained) {throw "Native test wrapper duplicated, lost or changed $name output."}
    }
    Write-Output 'Native test wrapper streams both files before its child exits, drains final Unicode output and retains exit 7.'
} finally {
    if($wrapper) {
        if(!$wrapper.HasExited) {New-Item -ItemType File -Path $release -Force|Out-Null;if(!$wrapper.WaitForExit(30000)) {$wrapper.Kill();$wrapper.WaitForExit()}}
        $wrapper.Dispose()
    }
    [Console]::OutputEncoding=$previousEncoding
    Remove-Item -LiteralPath $run -Recurse -Force
}
