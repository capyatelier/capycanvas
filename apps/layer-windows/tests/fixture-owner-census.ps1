param([Parameter(Mandatory)][string]$Helper,[Parameter(Mandatory)][string]$Output)
$ErrorActionPreference='Stop'
if(Test-Path $Output){throw 'Fresh owned output required'}
New-Item -ItemType Directory $Output|Out-Null
$before=$env:CAPY_FIXTURE_CONTEXT
try {
 $env:CAPY_FIXTURE_CONTEXT=Join-Path $Output 'context.json'
 Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
 Add-Type -TypeDefinition 'public static class CapyWindowApi { public static uint Owner; public static uint GetWindowThreadProcessId(System.IntPtr hwnd,out uint owner){owner=Owner;return 1;} }'
 $tokens=$null;$errors=$null;$ast=[Management.Automation.Language.Parser]::ParseFile((Resolve-Path $Helper),[ref]$tokens,[ref]$errors)
 if($errors.Count){throw 'Helper parse failed'}
 foreach($name in @('Update-CapyFixtureContext','Register-CapyFixtureWindow','Owned-DrawingWindow')){
  $node=$ast.Find({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq $name},$true)
  . ([scriptblock]::Create($node.Extent.Text))
 }
 function Read-Snapshot($Path){Get-Content -LiteralPath $Path -Raw|ConvertFrom-Json}
 $process=Get-Process -Id $PID;[CapyWindowApi]::Owner=$PID
 $registry=Join-Path $Output "windows-$PID.json";$handle=123456
 function Register {Register-CapyFixtureWindow $process $handle $Output}
 function Expect-Failure([scriptblock]$Action){$failed=$false;try{&$Action}catch{$failed=$true};if(!$failed){throw 'Expected rejection'}}
 function Registry($Value){$Value|ConvertTo-Json -Depth 8|Set-Content -LiteralPath $registry}
 Register
 $entry=(Get-Content $env:CAPY_FIXTURE_CONTEXT -Raw|ConvertFrom-Json).windows[0]
 if($entry.process_id -ne $PID -or $entry.hwnd -ne $handle -or $entry.executable -ne $process.Path -or $null -ne $entry.window_id){throw 'Missing-registry native ownership mismatch'}
 Registry @{process_id=$PID;windows=@(@{hwnd=$handle;id=77})};Register
 $entry=(Get-Content $env:CAPY_FIXTURE_CONTEXT -Raw|ConvertFrom-Json).windows[0]
 if($entry.window_id -ne 77){throw 'Late registry was not enriched'}
 Remove-Item -LiteralPath $registry;Expect-Failure {Register}
 Registry @{process_id=$PID;windows=@(@{hwnd=$handle;id=77})};Register
 Set-Content $registry '{';Expect-Failure {Register}
 Registry @{process_id=($PID+1);windows=@(@{hwnd=$handle;id=77})};Expect-Failure {Register}
 Registry @{process_id=$PID;windows=@(@{hwnd=($handle+1);id=77})};Expect-Failure {Register}
 Registry @{process_id=$PID;windows=@(@{hwnd=$handle;id=77},@{hwnd=$handle;id=78})};Expect-Failure {Register}
 Registry @{process_id=$PID;windows=@(@{hwnd=$handle;id=78})};Expect-Failure {Register}
 Registry @{process_id=$PID;windows=@(@{hwnd=$handle;id=77})};[CapyWindowApi]::Owner=$PID+1;Expect-Failure {Register};[CapyWindowApi]::Owner=$PID
 $context=Get-Content $env:CAPY_FIXTURE_CONTEXT -Raw|ConvertFrom-Json -AsHashtable;$context.windows[0].executable='foreign.exe';$context|ConvertTo-Json -Depth 8|Set-Content $env:CAPY_FIXTURE_CONTEXT;Expect-Failure {Register}
 $fakeRoot=[pscustomobject]@{}
 $fakeRoot|Add-Member -MemberType ScriptMethod -Name FindFirst -Value {param($scope,$condition)[pscustomobject]@{id='drawing-canvas'}}
 $script:lookupWindow=[pscustomobject]@{Root=$fakeRoot;Handle=[IntPtr]$handle}
 function Owned-ProcessWindows([Diagnostics.Process]$Process,[long]$WindowHandle){$script:lookupWindow}
 $env:CAPY_FIXTURE_CONTEXT=Join-Path $Output 'lookup-context.json'
 Registry @{process_id=$PID;windows=@()}
 if($null -ne (Owned-DrawingWindow $process $handle $Output) -or (Test-Path $env:CAPY_FIXTURE_CONTEXT)){throw 'Empty owned registry was accepted before native registration'}
 Registry @{process_id=$PID;windows=@(@{hwnd=$handle;id=77})}
 $drawing=Owned-DrawingWindow $process $handle $Output
 $entry=(Get-Content $env:CAPY_FIXTURE_CONTEXT -Raw|ConvertFrom-Json).windows[0]
 if($drawing -ne $script:lookupWindow -or $entry.hwnd -ne $handle -or $entry.window_id -ne 77){throw 'Late owned registry did not register the drawing'}
 $env:CAPY_FIXTURE_CONTEXT=Join-Path $Output 'legacy-context.json'
 Remove-Item -LiteralPath $registry
 $drawing=Owned-DrawingWindow $process $handle $Output
 $entry=(Get-Content $env:CAPY_FIXTURE_CONTEXT -Raw|ConvertFrom-Json).windows[0]
 if($drawing -ne $script:lookupWindow -or $entry.hwnd -ne $handle -or $null -ne $entry.window_id){throw 'Absent registry changed legacy drawing lookup'}
 @{status='passed';cases=13;native_stub_scope='Own process/executable with controlled HWND owner and mocked drawing peer; no app/window/input';helper_sha256=(Get-FileHash $Helper).Hash}|ConvertTo-Json|Set-Content (Join-Path $Output 'result.json')
} finally {$env:CAPY_FIXTURE_CONTEXT=$before}
