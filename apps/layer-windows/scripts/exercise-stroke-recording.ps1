param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class CapyRecordingPicker {
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern IntPtr SendMessageTimeout(IntPtr h,uint message,UIntPtr w,IntPtr l,uint flags,uint timeout,out UIntPtr result);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern IntPtr SendMessageTimeout(IntPtr h,uint message,UIntPtr w,System.Text.StringBuilder text,uint flags,uint timeout,out UIntPtr result);
 [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h,uint message,UIntPtr w,IntPtr l);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint process);
 public static void TypePath(IntPtr edit,uint owner,string path) {
  uint process;GetWindowThreadProcessId(edit,out process);if(process!=owner)throw new Exception("Wrong recording filename owner");
  UIntPtr result;
  if(SendMessageTimeout(edit,0xB1,UIntPtr.Zero,new IntPtr(-1),2,2000,out result)==IntPtr.Zero)throw new Exception("Cannot select picker text");
  if(SendMessageTimeout(edit,0x303,UIntPtr.Zero,IntPtr.Zero,2,2000,out result)==IntPtr.Zero)throw new Exception("Cannot clear picker text");
  foreach(char c in path)if(SendMessageTimeout(edit,0x102,new UIntPtr(c),IntPtr.Zero,2,2000,out result)==IntPtr.Zero)throw new Exception("Cannot type picker text");
  var actual=new System.Text.StringBuilder(32768);
  if(SendMessageTimeout(edit,13,new UIntPtr((uint)actual.Capacity),actual,2,2000,out result)==IntPtr.Zero||actual.ToString()!=path)throw new Exception("Recording filename did not match the owned path");
 }
}
"@
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path;$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/stroke-recording/'+[Guid]::NewGuid().ToString('N'));[IO.Directory]::CreateDirectory($run)|Out-Null
$names=@('CAPY_SETTINGS_DIRECTORY','CAPY_TRACE_UI','CAPY_SMOKE_TEST','CAPY_TEST_DISPLAY','CAPY_TEST_PRIMARY','CAPY_PRESENT_PROBE')
$previous=@{};foreach($name in $names){$previous[$name]=[Environment]::GetEnvironmentVariable($name,'Process')}
function Model {try{$s=Get-Content -LiteralPath (Join-Path $directory 'ui-state.json') -Raw|ConvertFrom-Json;if($s.process_id -eq $app.Id -and $s.model.windows_isolated_settings){$s.model}}catch{}}
function Wait-Until([scriptblock]$Condition,[string]$Message,[int]$Seconds=10){
 $watch=[Diagnostics.Stopwatch]::StartNew()
 do{if(& $Condition){return};$app.Refresh();if($app.HasExited){throw 'Owned recording review exited'};Start-Sleep -Milliseconds 60}while($watch.Elapsed.TotalSeconds -lt $Seconds)
 throw $Message
}
function Find([string]$Value,[switch]$Name){$property=if($Name){[System.Windows.Automation.AutomationElement]::NameProperty}else{[System.Windows.Automation.AutomationElement]::AutomationIdProperty};$root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new($property,$Value))}
function Control([string]$Value,[switch]$Name){$hit=@{item=$null};Wait-Until {$hit.item=Find $Value -Name:$Name;$null -ne $hit.item} "Missing control: $Value";$hit.item}
function Invoke([string]$Value,[switch]$Name){(Control $Value -Name:$Name).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()}
function Label {(Control 'stroke-recording').Current.Name}
function Picker {
 $dialog=@{item=$null}
 Wait-Until {
  $dialog.item=$root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.AndCondition]::new(
   [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ClassNameProperty,'#32770'),
   [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty,$app.Id)))
  $null -ne $dialog.item
 } 'Save stroke recording dialog did not open'
 $dialog.item
}
function Save-To([string]$Path){
 $picker=Picker
 $entry=@{value=$null};Wait-Until {
  $entry.value=$picker.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.AndCondition]::new(
   [System.Windows.Automation.OrCondition]::new(
    [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'1001'),
    [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'1148')),
   [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ClassNameProperty,'Edit')))
  $null -ne $entry.value
 } 'Recording filename field did not appear'
 [CapyRecordingPicker]::TypePath([IntPtr]$entry.value.Current.NativeWindowHandle,[uint32]$app.Id,$Path)
 Picker-Button $picker '1'
}
function Picker-Button($Picker,[string]$Id){
 $button=@{value=$null};Wait-Until {
  $button.value=$Picker.FindFirst([System.Windows.Automation.TreeScope]::Children,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,$Id))
  $button.value -and $button.value.Current.IsEnabled -and $button.value.Current.ClassName -eq 'Button'
 } "Picker button $Id did not become ready"
 if(![CapyRecordingPicker]::PostMessage([IntPtr]$button.value.Current.NativeWindowHandle,245,[UIntPtr]::Zero,[IntPtr]::Zero)){throw "Cannot invoke picker button $Id"}
}
function Recording-File([string]$Path){
 $file=@{bytes=$null}
 Wait-Until {(Test-Path -LiteralPath $Path) -and ($file.bytes=[IO.File]::ReadAllBytes($Path)).Length -ge 10} "Stroke recording was not written to $Path" 20
 $bytes=$file.bytes
 if([Text.Encoding]::ASCII.GetString($bytes,0,8) -ne 'CAPYPEN2' -or $bytes[8] -ne 0x1f -or $bytes[9] -ne 0x8b){throw 'Stroke recording is not CAPYPEN2 followed by gzip'}
 $bytes.Length
}
try{
 foreach($name in $names){Remove-Item -LiteralPath ('Env:'+$name) -ErrorAction SilentlyContinue}
 $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1';$env:CAPY_SMOKE_TEST='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
 $stderr=Join-Path $run 'stderr.log'
 $app=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr;$null=$app.Handle
 Write-Output "Owned stroke recording review $($app.Id): $run"
 Wait-Until {$app.Refresh();$app.MainWindowHandle -ne [IntPtr]::Zero -and (Model).brush_ready} 'Recording review did not start' 45
 $root=[System.Windows.Automation.AutomationElement]::FromHandle($app.MainWindowHandle)
 Invoke 'panel-tab-stats'
 Wait-Until {(Label) -eq 'Start stroke recording'} 'Diagnostics did not offer stroke recording'
 $first=Join-Path $run 'first.capystrokes';$second=Join-Path $run 'second.capystrokes'
 Invoke 'stroke-recording'
 Wait-Until {(Label) -eq 'Stop stroke recording'} 'Recording did not start'
 Invoke 'Test pen' -Name
 Wait-Until {(Model).state.document_file.modified} 'Controlled pen stroke did not reach the drawing'
 Invoke 'stroke-recording'
 Save-To $first
 $size=Recording-File $first
 Wait-Until {(Label) -eq 'Start stroke recording'} 'Saved recording was not released'
 Invoke 'stroke-recording'
 Wait-Until {(Label) -eq 'Stop stroke recording'} 'Second recording did not start'
 Invoke 'stroke-recording'
 Picker-Button (Picker) '2'
 Wait-Until {(Label) -eq 'Save stroke recording'} 'Cancelled save did not retain the recording'
 Invoke 'stroke-recording'
 Save-To $second
 $null=Recording-File $second
 Wait-Until {(Label) -eq 'Start stroke recording'} 'Second saved recording was not released'
 & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $app.Id -Action Close -DiscardUnsaved
 if(!$app.WaitForExit(15000)){throw 'Recording review did not close'}
 @{start_stop_save='passed';capypen2_gzip='passed';release_after_delivery='passed';cancel_retains='passed';first_bytes=$size;zero_exit='passed'}|ConvertTo-Json|Set-Content (Join-Path $run 'results.json')
 Get-Content (Join-Path $run 'results.json')
}catch{
 [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace)
 if($app -and !$app.HasExited){& (Join-Path $PSScriptRoot 'inspect-window.ps1') -ProcessId $app.Id -ClientOnly -Output (Join-Path $run 'failure.png') *> (Join-Path $run 'failure.json')}
 throw
}finally{
 if($app -and !$app.HasExited){Stop-Process -Id $app.Id -Force}
 foreach($name in $names){if($null -eq $previous[$name]){Remove-Item -LiteralPath ('Env:'+$name) -ErrorAction SilentlyContinue}else{[Environment]::SetEnvironmentVariable($name,$previous[$name],'Process')}}
}
