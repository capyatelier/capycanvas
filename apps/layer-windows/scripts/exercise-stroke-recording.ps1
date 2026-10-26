param([Parameter(Mandatory)][string]$Executable,[ValidateSet('dark','light')][string]$Theme)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyWaitSeconds=10
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class CapyRecordingPicker {
 [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr window);
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
function Label {(Control 'stroke-recording').Current.Name}
$geometry=[Collections.Generic.List[object]]::new()
function Recording-Geometry([string]$Phase){
 $state=Read-Snapshot (State-File);$path=State-File
 if((Split-Path -Leaf $path) -notmatch '^ui-state-\d+-\d+\.json$'){throw 'Recording geometry needs the owned drawing trace'}
 $metricsPath=Join-Path (Split-Path -Parent $path) ((Split-Path -Leaf $path) -replace '^ui-state-','panel-metrics-')
 Wait-Until {Test-Path -LiteralPath $metricsPath} 'Shared recording control metrics were not published'
 $metrics=Read-Snapshot $metricsPath
 if($state.process_id -ne $review.Id -or !$state.model.windows_isolated_settings -or $state.window_id -le 0 -or $metrics.process_id -ne $review.Id -or $metrics.window_id -ne $state.window_id){throw 'Recording geometry has foreign drawing metrics'}
 $height=[double]$metrics.panel_metrics.control_height
 if([double]::IsNaN($height) -or [double]::IsInfinity($height) -or $height -le 0){throw 'Recording control height is not a finite positive shared metric'}
 $button=Control 'stroke-recording' -Type ([System.Windows.Automation.ControlType]::Button) -Arranged
 $bounds=$button.Current.BoundingRectangle;$handle=[IntPtr]$root.Current.NativeWindowHandle;$owner=[uint32]0
 [CapyRecordingPicker]::GetWindowThreadProcessId($handle,[ref]$owner)|Out-Null
 $dpi=[CapyRecordingPicker]::GetDpiForWindow($handle)
 if($owner -ne $review.Id -or $button.Current.ProcessId -ne $review.Id -or !$dpi -or !$root.Current.BoundingRectangle.Contains($bounds)){throw 'Recording control is outside its owned arranged window'}
 $record=@{phase=$Phase;theme=(Model).state.theme;label=$button.Current.Name;process_id=$owner;window_id=$state.window_id;dpi=$dpi;shared_control_height=$height;bounds=@($bounds.X,$bounds.Y,$bounds.Width,$bounds.Height);height_dip=($bounds.Height*96/$dpi);placement='docked'}
 $geometry.Add($record)
 [IO.File]::WriteAllText((Join-Path $run 'recording-geometry.json'),(@($geometry)|ConvertTo-Json -Depth 6),[Text.UTF8Encoding]::new($false))
 if([Math]::Abs($bounds.Height-$height*$dpi/96) -gt 1){throw 'Recording button allocation differs from shared compact control height'}
 Capture ('recording-'+$Phase) -WithModel -Composed
}
function Picker {
 $dialog=@{item=$null}
 Wait-Until {
  $dialog.item=$root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.AndCondition]::new(
   [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ClassNameProperty,'#32770'),
   [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty,$review.Id)))
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
 [CapyRecordingPicker]::TypePath([IntPtr]$entry.value.Current.NativeWindowHandle,[uint32]$review.Id,$Path)
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
 if([Text.Encoding]::ASCII.GetString($bytes,0,8) -ne 'CAPYPEN4' -or $bytes[8] -ne 0x1f -or $bytes[9] -ne 0x8b){throw 'Stroke recording is not CAPYPEN4 followed by gzip'}
 $bytes.Length
}
try{
 Enter-CapyEnvironment
 $env:CAPY_STORAGE_DIR=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1';$env:CAPY_SMOKE_TEST='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
 if($Theme){[IO.File]::WriteAllText((Settings-File),(@{language=@{Explicit='en'};theme=$Theme}|ConvertTo-Json -Depth 4),[Text.UTF8Encoding]::new($false))}
 $stderr=Join-Path $run 'stderr.log'
 $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr;$null=$review.Handle
 Write-Output "Owned stroke recording review $($review.Id): $run"
 Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero -and (Model).brush_ready} 'Recording review did not start' 45
 $root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
 if($Theme){Wait-Until {(Model).state.theme -eq $Theme} 'Recording theme did not apply'}
 Invoke 'panel-tab-stats'
 Wait-Until {(Label) -eq 'Start stroke recording'} 'Diagnostics did not offer stroke recording'
 Recording-Geometry 'start'
 $first=Join-Path $run 'first.capystrokes';$second=Join-Path $run 'second.capystrokes'
 Invoke 'stroke-recording'
 Wait-Until {(Label) -eq 'Stop stroke recording'} 'Recording did not start'
 Recording-Geometry 'stop'
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
 Recording-Geometry 'save'
 Invoke 'stroke-recording'
 Save-To $second
 $null=Recording-File $second
 Wait-Until {(Label) -eq 'Start stroke recording'} 'Second saved recording was not released'
 & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved
 if(!$review.WaitForExit(15000)){throw 'Recording review did not close'}
 if($review.ExitCode -ne 0){throw 'Recording review did not exit zero'}
 [IO.File]::WriteAllText((Join-Path $run 'results.json'),(@{theme=$geometry[0].theme;start_stop_save='passed';compact_docked_geometry=@($geometry);capypen4_gzip='passed';release_after_delivery='passed';cancel_retains='passed';first_bytes=$size;zero_exit='passed';unverified=@('floating and drawer placement','flat background and border pixels')}|ConvertTo-Json -Depth 8),[Text.UTF8Encoding]::new($false))
 Get-Content (Join-Path $run 'results.json')
}catch{
 [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace)
 if($review -and !$review.HasExited){& (Join-Path $PSScriptRoot 'inspect-window.ps1') -ProcessId $review.Id -ClientOnly -Output (Join-Path $run 'failure.png') *> (Join-Path $run 'failure.json')}
 throw
}finally{
 if($review -and !$review.HasExited){Stop-Process -Id $review.Id -Force}
 Exit-CapyEnvironment
}
