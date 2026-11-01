$ErrorActionPreference='Stop'
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
function Import-Function([string]$Path,[string]$Name,[string]$As){
 $tokens=$null;$errors=$null;$ast=[Management.Automation.Language.Parser]::ParseFile($Path,[ref]$tokens,[ref]$errors)
 if($errors.Count){throw 'Function source parse failed'}
 $functions=@($ast.FindAll({param($n)$n-is[Management.Automation.Language.FunctionDefinitionAst]-and$n.Name-eq$Name},$true))
 if($functions.Count-ne1){throw 'Expected exactly one actual function'}
 $body=$functions[0].Extent.Text
 Invoke-Expression ('function script:'+$As+$body.Substring(('function '+$Name).Length))
}
Import-Function (Join-Path $repo 'apps/layer-windows/scripts/CapyUia.ps1') 'Read-Snapshot' 'Read-Snapshot'
Import-Function (Join-Path $repo 'apps/layer-windows/scripts/exercise-tools.ps1') 'Model' 'Model'
Import-Function (Join-Path $repo 'apps/layer-windows/scripts/exercise-multiwindow.ps1') 'Model' 'Read-MultiwindowModel'
Import-Function (Join-Path $repo 'apps/layer-windows/scripts/exercise-multiwindow.ps1') 'Windows' 'Read-MultiwindowWindows'
Add-Type -TypeDefinition @'
using System;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;
using Microsoft.Win32.SafeHandles;
public sealed class CapyAtomicPublisher : IDisposable {
 [StructLayout(LayoutKind.Sequential)] struct RenameInfo { public uint Flags; public IntPtr RootDirectory; public uint FileNameLength; public ushort FileName; }
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern SafeFileHandle CreateFileW(string path,uint access,uint sharing,IntPtr security,uint creation,uint attributes,IntPtr template);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool SetFileInformationByHandle(SafeFileHandle file,int informationClass,IntPtr information,uint size);
 const uint DeleteAccess=0x10000,ShareReadWriteDelete=7,OpenExisting=3,NormalAttributes=0x80,ReplaceIfExists=1,PosixSemantics=2;
 const int FileRenameInfoEx=22;
 readonly string path; readonly byte[] data; readonly Thread thread; readonly IntPtr rename; readonly uint renameSize; volatile bool stop;
 public long Published; public long Errors; public int LastError; public string LastException;
 public CapyAtomicPublisher(string path,string payload){
  this.path=path;data=new UTF8Encoding(false).GetBytes(payload);
  byte[] target=Encoding.Unicode.GetBytes(Path.GetFullPath(path));int offset=Marshal.OffsetOf(typeof(RenameInfo),"FileName").ToInt32();
  byte[] information=new byte[offset+target.Length+2];BitConverter.GetBytes(ReplaceIfExists|PosixSemantics).CopyTo(information,0);BitConverter.GetBytes(target.Length).CopyTo(information,offset-4);target.CopyTo(information,offset);
  renameSize=(uint)information.Length;rename=Marshal.AllocHGlobal(information.Length);Marshal.Copy(information,0,rename,information.Length);
  thread=new Thread(Run);thread.IsBackground=true;thread.Start();
 }
 void Run(){while(!stop){string pending=path+".pending";try{
  File.WriteAllBytes(pending,data);bool ok;int error;
  using(var source=CreateFileW(pending,DeleteAccess,ShareReadWriteDelete,IntPtr.Zero,OpenExisting,NormalAttributes,IntPtr.Zero)){ok=!source.IsInvalid&&SetFileInformationByHandle(source,FileRenameInfoEx,rename,renameSize);error=Marshal.GetLastWin32Error();}
  if(ok)Interlocked.Increment(ref Published);else{LastError=error;Interlocked.Increment(ref Errors);File.Delete(pending);}
 }catch(Exception error){LastError=error.HResult;LastException=error.ToString();Interlocked.Increment(ref Errors);}}}
 public void Dispose(){stop=true;thread.Join();Marshal.FreeHGlobal(rename);}
}
'@
$out=Join-Path $repo ('artifacts/windows/model-snapshot-'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($out)|Out-Null
$ProcessId=$PID;$directory=$out;$current=[pscustomobject]@{id=101;hwnd=101}
$review=[pscustomobject]@{Id=$ProcessId;StartTime=[DateTime]::Now.AddMinutes(-1)}
$StateFile=Join-Path $out ("ui-state-$ProcessId-"+$current.id+'.json')
$registryFile=Join-Path $out ("windows-$ProcessId.json")
$payload=@{process_id=$ProcessId;window_id=$current.id;model=@{process_id=$ProcessId;window_id=$current.id;windows_isolated_settings=$true;state=@{camera=@{rotation=0;flipped=@($false,$false)}};padding=('x'*524288)}}|ConvertTo-Json -Depth 8 -Compress
$registry=@{process_id=$ProcessId;windows=@(@{process_id=$ProcessId;id=$current.id;hwnd=$current.hwnd});padding=('x'*524288)}|ConvertTo-Json -Depth 8 -Compress
[IO.File]::WriteAllText($StateFile,$payload);[IO.File]::WriteAllText($registryFile,$registry)
if((Read-Snapshot $StateFile -Raw) -cne $payload){throw 'Raw snapshot reader did not preserve the exact unparsed frame'}
$publisher=[CapyAtomicPublisher]::new($StateFile,$payload)
$registryPublisher=[CapyAtomicPublisher]::new($registryFile,$registry)
$reads=0;$nulls=0;$errors=0;$misclassified=0;$multiwindowNulls=0;$multiwindowReads=0;$multiwindowMismatches=0;$registryNulls=0;$registryReads=0;$registryMismatches=0
$readFailures=[Collections.Generic.List[object]]::new()
$readDurations=[Collections.Generic.List[double]]::new()
$watch=[Diagnostics.Stopwatch]::StartNew()
try{
 while($reads-lt100){
  $reads++
  $started=$watch.Elapsed.TotalMilliseconds
  try{
   $value=Model
   $multiwindow=Read-MultiwindowModel
   $windows=@(Read-MultiwindowWindows)
   if($null -eq $multiwindow){$multiwindowNulls++}else{
    $multiwindowReads++
    if(!$multiwindow.windows_isolated_settings -or $multiwindow.process_id -ne $review.Id -or $multiwindow.window_id -ne $current.id){$multiwindowMismatches++}
   }
   if(!$windows.Count){$registryNulls++}
   foreach($window in $windows){
    $registryReads++
    if($window.process_id -ne $review.Id -or $window.id -ne $current.id -or $window.hwnd -ne $current.hwnd){$registryMismatches++}
   }
   if($null-eq$value){$nulls++}
   if($value.state.camera.rotation-ne0-or$value.state.camera.flipped[0]-or$value.state.camera.flipped[1]){$misclassified++}
  }catch{
   $errors++
   $exception=$_.Exception.GetBaseException()
   $readFailures.Add(@{read=$reads;elapsed_ms=$watch.Elapsed.TotalMilliseconds;type=$exception.GetType().FullName;message=$exception.Message;hresult=$exception.HResult})
  }finally{$readDurations.Add($watch.Elapsed.TotalMilliseconds-$started)}
 }
}finally{$publisher.Dispose();$registryPublisher.Dispose()}
$invalidOwnerRejected=$false
[IO.File]::WriteAllText($StateFile,('{"process_id":'+($PID+1)+',"model":{}}'))
try{$null=Model}catch{$invalidOwnerRejected=$_.Exception.Message-eq'Review snapshot process owner mismatch'}
$malformedRejected=$false
[IO.File]::WriteAllText($StateFile,'{invalid')
try{$null=Model}catch{$malformedRejected=$true}
$missingRejected=$false
Remove-Item -LiteralPath $StateFile
try{$null=Model}catch{$missingRejected=$true}
$pass=$reads-eq100-and$nulls-eq0-and$errors-eq0-and$misclassified-eq0-and$multiwindowReads-gt0-and$multiwindowMismatches-eq0-and$registryReads-gt0-and$registryMismatches-eq0-and$publisher.Published-gt0-and$publisher.Errors-eq0-and$registryPublisher.Published-gt0-and$registryPublisher.Errors-eq0-and$invalidOwnerRejected-and$malformedRejected-and$missingRejected
@{pass=$pass;multiwindow_nulls=$multiwindowNulls;multiwindow_reads=$multiwindowReads;multiwindow_mismatches=$multiwindowMismatches;registry_nulls=$registryNulls;registry_reads=$registryReads;registry_mismatches=$registryMismatches;registry_published=$registryPublisher.Published;registry_replace_errors=$registryPublisher.Errors;registry_last_replace_error=$registryPublisher.LastError;registry_writer_exception=$registryPublisher.LastException;reads=$reads;nulls=$nulls;read_errors=$errors;read_failures=$readFailures.ToArray();read_durations_ms=$readDurations.ToArray();elapsed_ms=$watch.Elapsed.TotalMilliseconds;false_rotation_preconditions=$misclassified;published=$publisher.Published;replace_errors=$publisher.Errors;last_replace_error=$publisher.LastError;last_writer_exception=$publisher.LastException;invalid_owner_rejected=$invalidOwnerRejected;malformed_rejected=$malformedRejected;missing_rejected=$missingRejected;writer_joined=$true;pid=$PID}|ConvertTo-Json -Depth 6|Set-Content -LiteralPath (Join-Path $out 'result.json')
Get-Content -LiteralPath (Join-Path $out 'result.json')
if(!$pass){throw 'Atomic snapshot model reader regression failed'}

Import-Function (Join-Path $repo 'apps/layer-windows/scripts/exercise-localization.ps1') 'Model' 'Localization-Model'
$run=$out;$review=[pscustomobject]@{Id=$PID};$window=[pscustomobject]@{id=7}
$captured=@{process_id=$PID;window_id=7;model=@{windows_isolated_settings=$true;state=@{camera=@{rotation=0}}}}
if((Localization-Model $window ($captured|ConvertTo-Json -Depth 6 -Compress)).state.camera.rotation -ne 0){throw 'Captured localization snapshot did not preserve its owned state'}
foreach($invalid in @(@{process_id=($PID+1);window_id=7;isolated=$true},@{process_id=$PID;window_id=8;isolated=$true},@{process_id=$PID;window_id=7;isolated=$false})){
 $captured.process_id=$invalid.process_id;$captured.window_id=$invalid.window_id;$captured.model.windows_isolated_settings=$invalid.isolated
 if(Localization-Model $window ($captured|ConvertTo-Json -Depth 6 -Compress)){throw 'Captured localization snapshot bypassed its process, window or isolation guard'}
}
Write-Output 'Captured snapshot parsing preserves exact raw data and the existing process, window and isolation guards.'
