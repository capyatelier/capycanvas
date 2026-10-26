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
$ProcessId=$PID;$StateFile=Join-Path $out 'ui-state.json'
$payload=@{process_id=$ProcessId;model=@{state=@{camera=@{rotation=0;flipped=@($false,$false)}};padding=('x'*524288)}}|ConvertTo-Json -Depth 8 -Compress
[IO.File]::WriteAllText($StateFile,$payload)
$publisher=[CapyAtomicPublisher]::new($StateFile,$payload)
$reads=0;$nulls=0;$errors=0;$misclassified=0
$readFailures=[Collections.Generic.List[object]]::new()
$readDurations=[Collections.Generic.List[double]]::new()
$watch=[Diagnostics.Stopwatch]::StartNew()
try{
 while($reads-lt100){
  $reads++
  $started=$watch.Elapsed.TotalMilliseconds
  try{
   $value=Model
   if($null-eq$value){$nulls++}
   if($value.state.camera.rotation-ne0-or$value.state.camera.flipped[0]-or$value.state.camera.flipped[1]){$misclassified++}
  }catch{
   $errors++
   $exception=$_.Exception.GetBaseException()
   $readFailures.Add(@{read=$reads;elapsed_ms=$watch.Elapsed.TotalMilliseconds;type=$exception.GetType().FullName;message=$exception.Message;hresult=$exception.HResult})
  }finally{$readDurations.Add($watch.Elapsed.TotalMilliseconds-$started)}
 }
}finally{$publisher.Dispose()}
$invalidOwnerRejected=$false
[IO.File]::WriteAllText($StateFile,('{"process_id":'+($PID+1)+',"model":{}}'))
try{$null=Model}catch{$invalidOwnerRejected=$_.Exception.Message-eq'Review snapshot process owner mismatch'}
$malformedRejected=$false
[IO.File]::WriteAllText($StateFile,'{invalid')
try{$null=Model}catch{$malformedRejected=$true}
$missingRejected=$false
Remove-Item -LiteralPath $StateFile
try{$null=Model}catch{$missingRejected=$true}
$pass=$reads-eq100-and$nulls-eq0-and$errors-eq0-and$misclassified-eq0-and$publisher.Published-gt0-and$publisher.Errors-eq0-and$invalidOwnerRejected-and$malformedRejected-and$missingRejected
@{pass=$pass;reads=$reads;nulls=$nulls;read_errors=$errors;read_failures=$readFailures.ToArray();read_durations_ms=$readDurations.ToArray();elapsed_ms=$watch.Elapsed.TotalMilliseconds;false_rotation_preconditions=$misclassified;published=$publisher.Published;replace_errors=$publisher.Errors;last_replace_error=$publisher.LastError;last_writer_exception=$publisher.LastException;invalid_owner_rejected=$invalidOwnerRejected;malformed_rejected=$malformedRejected;missing_rejected=$missingRejected;writer_joined=$true;pid=$PID}|ConvertTo-Json -Depth 6|Set-Content -LiteralPath (Join-Path $out 'result.json')
Get-Content -LiteralPath (Join-Path $out 'result.json')
if(!$pass){throw 'Atomic snapshot model reader regression failed'}
