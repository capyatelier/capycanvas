param([Parameter(Mandatory)][string]$Helper,[Parameter(Mandatory)][string]$Output)
$ErrorActionPreference='Stop'
if(Test-Path $Output){throw 'Fresh owned output required'}
New-Item -ItemType Directory $Output|Out-Null
$before=$env:CAPY_FIXTURE_CONTEXT
try {
 Add-Type -TypeDefinition @'
using System;using System.Collections.Generic;
namespace CapyRegisteredDiscoveryTest {
 public class Process {public int Id=33;public bool HasExited;public DateTime StartTime=new DateTime(2020,1,2,3,4,5,DateTimeKind.Utc);public string Path;public Process(string path){Path=path;}}
 public class Node {public int ProcessId=33;public long NativeWindowHandle;public Node(long handle){NativeWindowHandle=handle;}public Node Current{get{return this;}}public object FindFirst(object scope,object condition){throw new InvalidOperationException("Identity discovery inspected descendants");}}
 public static class Native {
  public static Dictionary<long,uint> Owners=new Dictionary<long,uint>();
  public static uint GetWindowThreadProcessId(IntPtr handle,out uint owner){return Owners.TryGetValue(handle.ToInt64(),out owner)?1u:0u;}
  public static IntPtr[] Windows(uint process){throw new InvalidOperationException("Native desktop enumeration is forbidden");}
 }
 public static class AutomationElement {
  public static Dictionary<long,Node> Nodes=new Dictionary<long,Node>();public static List<long> Calls=new List<long>();
  public static Node RootElement{get{throw new InvalidOperationException("Desktop automation discovery is forbidden");}}
  public static Node FromHandle(IntPtr handle){Calls.Add(handle.ToInt64());Node node;if(!Nodes.TryGetValue(handle.ToInt64(),out node))throw new InvalidOperationException("Unexpected HWND");return node;}
 }
}
'@
 $tokens=$null;$errors=$null;$ast=[Management.Automation.Language.Parser]::ParseFile((Resolve-Path $Helper),[ref]$tokens,[ref]$errors)
 if($errors.Count){throw 'Helper parse failed'}
 foreach($name in @('Update-CapyFixtureContext','Register-CapyFixtureWindow','Owned-ProcessWindows','Owned-DrawingWindow')){
  $node=$ast.Find({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq $name}.GetNewClosure(),$true)
  if(!$node){throw "Missing actual helper: $name"}
  $source=$node.Extent.Text.Replace('[Diagnostics.Process]','[CapyRegisteredDiscoveryTest.Process]').Replace('CapyWindowApi','CapyRegisteredDiscoveryTest.Native').Replace('System.Windows.Automation.AutomationElement','CapyRegisteredDiscoveryTest.AutomationElement')
  . ([scriptblock]::Create($source))
 }
 $script:snapshotReads=0
 function Read-Snapshot($Path){$script:snapshotReads++;Get-Content -LiteralPath $Path -Raw|ConvertFrom-Json}
 $process=[CapyRegisteredDiscoveryTest.Process]::new((Join-Path $Output 'review.exe'))
 $script:CapyTraceDirectory=$Output;$env:CAPY_FIXTURE_CONTEXT=Join-Path $Output 'context.json'
 $registry=Join-Path $Output 'windows-33.json';$handle=123456;$second=123457;$checks=0
 foreach($window in @($handle,$second)){
  [CapyRegisteredDiscoveryTest.Native]::Owners[$window]=33
  [CapyRegisteredDiscoveryTest.AutomationElement]::Nodes[$window]=[CapyRegisteredDiscoveryTest.Node]::new($window)
 }
 function Registry($Windows,[int]$Owner=33){foreach($window in $Windows){if(!$window.ContainsKey('ready')){$window.ready=$true}};@{process_id=$Owner;windows=@($Windows)}|ConvertTo-Json -Depth 8|Set-Content -LiteralPath $registry;[IO.File]::SetLastWriteTimeUtc($registry,$process.StartTime.ToUniversalTime())}
 function Expect-Failure([scriptblock]$Action){$failed=$false;try{&$Action|Out-Null}catch{$failed=$true};if(!$failed){throw 'Expected rejection'};$script:checks++}
 function Assert-NoDiscovery {
  if(@(Owned-ProcessWindows $process).Count -or $null -ne (Owned-DrawingWindow $process $handle $Output) -or [CapyRegisteredDiscoveryTest.AutomationElement]::Calls.Count){throw 'Missing registration reached UI Automation'}
  $script:checks++
 }
 Assert-NoDiscovery
 Expect-Failure {Register-CapyFixtureWindow $process $handle $Output}
 Registry @();Assert-NoDiscovery
 Registry @(@{id=77;hwnd=$handle;ready=$false})
 Assert-NoDiscovery
 Registry @(@{id=77;hwnd=$handle;ready=$null})
 Assert-NoDiscovery
 @{process_id=33;windows=@(@{id=77;hwnd=$handle})}|ConvertTo-Json -Depth 8|Set-Content -LiteralPath $registry
 Assert-NoDiscovery
 Registry @(@{id=77;hwnd=$handle})
 [IO.File]::SetLastWriteTimeUtc($registry,$process.StartTime.ToUniversalTime().AddTicks(-1));$script:snapshotReads=0
 Assert-NoDiscovery
 Expect-Failure {Register-CapyFixtureWindow $process $handle $Output}
 if($script:snapshotReads){throw 'Stale prior-process registration was read'}
 [IO.File]::SetLastWriteTimeUtc($registry,$process.StartTime.ToUniversalTime())
 $fresh=@(Owned-ProcessWindows $process)
 if($fresh.Count -ne 1 -or [CapyRegisteredDiscoveryTest.AutomationElement]::Calls.Count -ne 1){throw 'Registration at exact process start time was rejected'};$checks++
 [CapyRegisteredDiscoveryTest.AutomationElement]::Calls.Clear()
 Registry @(@{id=77;hwnd=$handle}) 34;Expect-Failure {Owned-ProcessWindows $process}
 Registry @(@{id=77;hwnd=$handle},@{id=78;hwnd=$handle});Expect-Failure {Owned-ProcessWindows $process}
 Registry @(@{id=77;hwnd=$handle},@{id=77;hwnd=$second});Expect-Failure {Owned-ProcessWindows $process}
 Registry @(@{id=77;hwnd=0});Expect-Failure {Owned-ProcessWindows $process}
 Registry @(@{id=0;hwnd=$handle});Expect-Failure {Owned-ProcessWindows $process}
 if([CapyRegisteredDiscoveryTest.AutomationElement]::Calls.Count){throw 'Invalid registration reached UI Automation'}
 Registry @(@{id=77;hwnd=$handle})
 $process.HasExited=$true
 if(@(Owned-ProcessWindows $process).Count -or [CapyRegisteredDiscoveryTest.AutomationElement]::Calls.Count){throw 'Exited owner reached UI Automation'};$checks++
 $process.HasExited=$false
 Expect-Failure {Owned-ProcessWindows $process $second}
 [CapyRegisteredDiscoveryTest.Native]::Owners[$handle]=34
 if(@(Owned-ProcessWindows $process).Count -or [CapyRegisteredDiscoveryTest.AutomationElement]::Calls.Count){throw 'Foreign native HWND reached UI Automation'};$checks++
 Expect-Failure {Owned-ProcessWindows $process $handle}
 [CapyRegisteredDiscoveryTest.Native]::Owners[$handle]=33
 $drawing=Owned-DrawingWindow $process $handle $Output
 $entry=(Get-Content $env:CAPY_FIXTURE_CONTEXT -Raw|ConvertFrom-Json).windows[0]
 if($drawing.Handle.ToInt64() -ne $handle -or $drawing.Root -ne [CapyRegisteredDiscoveryTest.AutomationElement]::Nodes[$handle] -or $entry.window_id -ne 77 -or $entry.executable -ne $process.Path){throw 'Registered identity or census mismatch'};$checks++
 [CapyRegisteredDiscoveryTest.AutomationElement]::Calls.Clear()
 $drawing=Owned-DrawingWindow $process
 if($drawing.Handle.ToInt64() -ne $handle -or [CapyRegisteredDiscoveryTest.AutomationElement]::Calls.Count -ne 1){throw 'Default trace directory did not resolve registration'};$checks++
 [CapyRegisteredDiscoveryTest.AutomationElement]::Calls.Clear()
 $missing=Join-Path $Output 'other';New-Item -ItemType Directory $missing|Out-Null
 if(@(Owned-ProcessWindows $process 0 $missing).Count -or [CapyRegisteredDiscoveryTest.AutomationElement]::Calls.Count){throw 'Supplied trace directory fell back to another registry'};$checks++
 Registry @(@{id=77;hwnd=$handle},@{id=78;hwnd=$second})
 Registry @(@{id=77;hwnd=$handle;ready=$false},@{id=78;hwnd=$second;ready=$true})
 $readyWindows=@(Owned-ProcessWindows $process)
 if($readyWindows.Count -ne 1 -or $readyWindows[0].Handle.ToInt64() -ne $second){throw 'Window readiness was not scoped to its registered identity'};$checks++
 Registry @(@{id=77;hwnd=$handle},@{id=78;hwnd=$second})
 $windows=@(Owned-ProcessWindows $process)
 if($windows.Count -ne 2 -or $windows[0].Handle.ToInt64() -ne $handle -or $windows[1].Handle.ToInt64() -ne $second){throw 'Registered multiwindow identities were not preserved'};$checks++
 Expect-Failure {Owned-DrawingWindow $process}
 $drawing=Owned-DrawingWindow $process $second
 if($drawing.Handle.ToInt64() -ne $second){throw 'Explicit registered window was not selected'};$checks++
 [CapyRegisteredDiscoveryTest.AutomationElement]::Nodes[$second].ProcessId=34
 Expect-Failure {Owned-ProcessWindows $process $second}
 [CapyRegisteredDiscoveryTest.AutomationElement]::Nodes[$second].ProcessId=33
 [CapyRegisteredDiscoveryTest.AutomationElement]::Nodes[$second].NativeWindowHandle=$handle
 Expect-Failure {Owned-ProcessWindows $process $second}
 [CapyRegisteredDiscoveryTest.AutomationElement]::Nodes[$second].NativeWindowHandle=$second
 Registry @(@{id=79;hwnd=$handle})
 Expect-Failure {Register-CapyFixtureWindow $process $handle $Output}
 Registry @(@{id=77;hwnd=$handle})
 $context=Get-Content $env:CAPY_FIXTURE_CONTEXT -Raw|ConvertFrom-Json;$context.windows[0].executable='foreign.exe';$context|ConvertTo-Json -Depth 8|Set-Content $env:CAPY_FIXTURE_CONTEXT
 Expect-Failure {Register-CapyFixtureWindow $process $handle $Output}
 Set-Content $registry '{';Expect-Failure {Owned-ProcessWindows $process}
 @{status='passed';cases=$checks;ps=$PSVersionTable.PSVersion.ToString();native_stub_scope='Registered HWND/native-owner/FromHandle only; desktop and descendant identity discovery throw; no app/window/input';helper_sha256=(Get-FileHash $Helper).Hash}|ConvertTo-Json|Set-Content (Join-Path $Output 'result.json')
} finally {$env:CAPY_FIXTURE_CONTEXT=$before}
