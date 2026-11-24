param(
 [Parameter(Mandatory)][string]$Executable,
 [Parameter(Mandatory)][string]$Chain,
 [Parameter(Mandatory)][int]$Pass,
 [switch]$Cold,
 [Parameter(Mandatory)][string]$Device,
 [Parameter(Mandatory)][string]$Version,
 [Parameter(Mandatory)][hashtable]$Documents,
 [Parameter(Mandatory)][string]$OutputDirectory,
 [string[]]$Steps,
 [string]$PresentMon=(Join-Path $env:USERPROFILE '.local/tools/presentmon/2.5.1/PresentMon-2.5.1-x64.exe')
)
$ErrorActionPreference='Stop'
$scripts=Join-Path $PSScriptRoot '../../apps/layer-windows/scripts'
. (Join-Path $scripts 'CapyUia.ps1')
. (Join-Path $PSScriptRoot 'WindowsCanvasMotion.ps1')
Add-Type -Path (Join-Path $PSScriptRoot 'WindowsPenMotion.cs')
Add-Type -Path (Join-Path $scripts 'RowPointerDriver.cs')
if(!('CapyBenchScreen' -as [type])){Add-Type -TypeDefinition @'
using System;using System.Diagnostics;using System.IO;using System.Runtime.InteropServices;using System.Text;using System.Threading;
public static class CapyBenchScreen {
 [DllImport("user32.dll")] static extern IntPtr GetDC(IntPtr window);
 [DllImport("user32.dll")] static extern int ReleaseDC(IntPtr window,IntPtr dc);
 [DllImport("gdi32.dll")] static extern IntPtr CreateCompatibleDC(IntPtr dc);
 [DllImport("gdi32.dll")] static extern IntPtr CreateCompatibleBitmap(IntPtr dc,int width,int height);
 [DllImport("gdi32.dll")] static extern IntPtr SelectObject(IntPtr dc,IntPtr value);
 [DllImport("gdi32.dll")] static extern bool DeleteObject(IntPtr value);
 [DllImport("gdi32.dll")] static extern bool DeleteDC(IntPtr dc);
 [DllImport("gdi32.dll")] static extern int SetStretchBltMode(IntPtr dc,int mode);
 [DllImport("gdi32.dll")] static extern bool StretchBlt(IntPtr target,int x,int y,int width,int height,IntPtr source,int sx,int sy,int sw,int sh,uint operation);
 [DllImport("gdi32.dll")] static extern int GetDIBits(IntPtr dc,IntPtr bitmap,uint start,uint lines,byte[] bits,byte[] info,uint usage);
 static byte[] Capture(int x,int y,int width,int height,int w,int h,int mode) {
  if(width<1||height<1)throw new ArgumentOutOfRangeException("width");
  IntPtr screen=GetDC(IntPtr.Zero),memory=CreateCompatibleDC(screen),bitmap=CreateCompatibleBitmap(screen,w,h),old=SelectObject(memory,bitmap);
  try {
   SetStretchBltMode(memory,mode);
   if(!StretchBlt(memory,0,0,w,h,screen,x,y,width,height,0x00CC0020))throw new Exception("Screen capture of the canvas failed");
   SelectObject(memory,old);
   var info=new byte[64];BitConverter.GetBytes(40).CopyTo(info,0);BitConverter.GetBytes(w).CopyTo(info,4);BitConverter.GetBytes(-h).CopyTo(info,8);
   BitConverter.GetBytes((short)1).CopyTo(info,12);BitConverter.GetBytes((short)32).CopyTo(info,14);
   var bits=new byte[w*h*4];
   if(GetDIBits(memory,bitmap,0,(uint)h,bits,info,0)!=h)throw new Exception("Screen capture readback failed");
   return bits;
  } finally {SelectObject(memory,old);DeleteObject(bitmap);DeleteDC(memory);ReleaseDC(IntPtr.Zero,screen);}
 }
 public static ulong Hash(int x,int y,int width,int height) {
  int w=Math.Min(width,384);
  ulong hash=14695981039346656037UL;foreach(var b in Capture(x,y,width,height,w,Math.Max(1,height*w/Math.Max(1,width)),3))hash=(hash^b)*1099511628211UL;
  return hash;
 }
 public static string Signature(int x,int y,int width,int height,out int rows) {
  rows=Math.Max(1,height*32/Math.Max(1,width));
  var bits=Capture(x,y,width,height,32,rows,4);var text=new StringBuilder(bits.Length/4*6);
  for(int i=0;i<bits.Length;i+=4)text.Append(bits[i+2].ToString("x2")).Append(bits[i+1].ToString("x2")).Append(bits[i].ToString("x2"));
  return text.ToString();
 }
}
public sealed class CapyBenchRefused:Exception {public CapyBenchRefused(string reason):base(reason){}}
public sealed class CapyBenchMemory:IDisposable {
 [DllImport("pdh.dll",CharSet=CharSet.Unicode)] static extern uint PdhOpenQuery(string source,IntPtr user,out IntPtr query);
 [DllImport("pdh.dll",CharSet=CharSet.Unicode)] static extern uint PdhAddEnglishCounter(IntPtr query,string path,IntPtr user,out IntPtr counter);
 [DllImport("pdh.dll")] static extern uint PdhCollectQueryData(IntPtr query);
 [DllImport("pdh.dll",CharSet=CharSet.Unicode)] static extern uint PdhGetFormattedCounterArray(IntPtr counter,uint format,ref uint size,out uint count,IntPtr items);
 [DllImport("pdh.dll")] static extern uint PdhCloseQuery(IntPtr query);
 readonly IntPtr query;readonly IntPtr[] counters=new IntPtr[3];readonly Process process;readonly StreamWriter output;readonly Timer timer;bool closed;
 public string Last {get;private set;}
 public string Error {get;private set;}
 public CapyBenchMemory(int pid,string path) {
  process=Process.GetProcessById(pid);
  if(PdhOpenQuery(null,IntPtr.Zero,out query)!=0)throw new Exception("Performance counters are unavailable");
  var names=new[]{@"\Memory\Available MBytes",@"\GPU Process Memory(pid_"+pid+@"_*)\Dedicated Usage",@"\GPU Process Memory(pid_"+pid+@"_*)\Shared Usage"};
  for(int i=0;i<names.Length;i++)if(PdhAddEnglishCounter(query,names[i],IntPtr.Zero,out counters[i])!=0)counters[i]=IntPtr.Zero;
  output=new StreamWriter(path,false,new UTF8Encoding(false));
  timer=new Timer(Sample,null,0,1000);
 }
 static double? Sum(IntPtr counter) {
  if(counter==IntPtr.Zero)return null;
  uint size=0,count;PdhGetFormattedCounterArray(counter,0x200,ref size,out count,IntPtr.Zero);
  if(size==0)return null;
  var items=Marshal.AllocHGlobal((int)size);
  try {
   if(PdhGetFormattedCounterArray(counter,0x200,ref size,out count,items)!=0||count==0)return null;
   double sum=0;int stride=IntPtr.Size+16;
   for(int i=0;i<count;i++){var item=new IntPtr(items.ToInt64()+i*stride);if(Marshal.ReadInt32(item,IntPtr.Size)>1)return null;sum+=BitConverter.Int64BitsToDouble(Marshal.ReadInt64(item,IntPtr.Size+8));}
   return sum;
  } finally {Marshal.FreeHGlobal(items);}
 }
 void Sample(object state) {
  lock(output) {
   if(closed)return;
   try {
   long ticks=Stopwatch.GetTimestamp(),frequency=Stopwatch.Frequency;
   PdhCollectQueryData(query);
   var line=new StringBuilder("{\"t_ns\":"+(ticks/frequency*1000000000+ticks%frequency*1000000000/frequency));
   var available=Sum(counters[0]);if(available.HasValue)line.Append(",\"os_available_kib\":"+(long)(available.Value*1024));
   try{process.Refresh();if(!process.HasExited)line.Append(",\"app_rss_kib\":"+process.WorkingSet64/1024);}catch(InvalidOperationException){}
   var dedicated=Sum(counters[1]);var shared=Sum(counters[2]);
   if(dedicated.HasValue&&shared.HasValue)line.Append(",\"gpu_kib\":"+(long)((dedicated.Value+shared.Value)/1024));
   Last=line.Append("}").ToString();output.WriteLine(Last);output.Flush();
   } catch(Exception error) {Error=error.ToString();}
  }
 }
 public void Dispose() {lock(output){if(closed)return;closed=true;timer.Dispose();output.Dispose();PdhCloseQuery(query);}}
}
'@}
[CapyWindowApi]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
$CapyWaitSeconds=30
$Executable=(Resolve-Path -LiteralPath $Executable).Path;$Chain=(Resolve-Path -LiteralPath $Chain).Path
foreach($name in @($Documents.Keys)){$Documents[$name]=(Resolve-Path -LiteralPath $Documents[$name]).Path}
$OutputDirectory=[IO.Path]::GetFullPath($OutputDirectory)
$passDirectory=Join-Path $OutputDirectory "pass-$Pass";$storage=Join-Path $OutputDirectory 'storage';$trace=Join-Path $passDirectory 'trace'
if(Test-Path -LiteralPath $passDirectory){throw "Pass directory already exists; move it away first: $passDirectory"}
$chainData=[IO.File]::ReadAllText($Chain)|ConvertFrom-Json
$Steps=@($Steps|ForEach-Object {$_ -split ','}|ForEach-Object {$_.Trim()}|Where-Object {$_})
$selected=$null
if($Steps.Count){
 $selected=@{launch=$true};$opened=@{};$lastOpen=$null
 foreach($step in $chainData.steps){if($step.'do' -eq 'open'){$lastOpen=$step.id};$opened[$step.id]=$lastOpen}
 foreach($id in $Steps){if(!$opened.ContainsKey($id)){throw "Unknown chain step: $id"};$selected[$id]=$true;if($opened[$id]){$selected[$opened[$id]]=$true}}
}
$displays=(& (Join-Path $scripts 'probe-displays.ps1'))|ConvertFrom-Json
$primary=@($displays|Where-Object {$_.primary})
if($primary.Count -ne 1 -or [double]$primary[0].refreshHz -lt 30){throw "Display probe found no single primary display with a refresh rate: $($displays|ConvertTo-Json -Compress)"}
$refreshHz=[double]$primary[0].refreshHz
function Image-Size([string]$Path){
 $stream=[IO.File]::OpenRead($Path)
 try{
  $head=[byte[]]::new(24);$null=$stream.Read($head,0,24)
  if([BitConverter]::ToUInt64($head,0) -eq 0x0A1A0A0D474E5089){return @([Net.IPAddress]::NetworkToHostOrder([BitConverter]::ToInt32($head,16)),[Net.IPAddress]::NetworkToHostOrder([BitConverter]::ToInt32($head,20)))}
  if($head[0] -ne 0xFF -or $head[1] -ne 0xD8){throw "Only PNG and JPEG photos have a Windows size reader: $Path"}
  $stream.Position=2;$marker=[byte[]]::new(9)
  while($stream.Read($marker,0,4) -eq 4 -and $marker[0] -eq 0xFF){
   if($marker[1] -in 0xC0,0xC1,0xC2,0xC3,0xC5,0xC6,0xC7,0xC9,0xCA,0xCB,0xCD,0xCE,0xCF){$null=$stream.Read($marker,4,5);return @((([int]$marker[7] -shl 8)+$marker[8]),(([int]$marker[5] -shl 8)+$marker[6]))}
   $stream.Position+=([int]$marker[2] -shl 8)+$marker[3]-2
  }
  throw "No JPEG frame header in $Path"
 }finally{$stream.Dispose()}
}
$photoDocument=@($chainData.documents.PSObject.Properties|Where-Object {$_.Value.kind -eq 'photo' -and $Documents.ContainsKey($_.Name)}|ForEach-Object Name)[0]
$sizes=@{};foreach($name in @($chainData.documents.PSObject.Properties|Where-Object {$_.Value.kind -eq 'photo' -and $Documents.ContainsKey($_.Name)}|ForEach-Object Name)){$sizes[$name]=Image-Size $Documents[$name]}
$canvasSize=if($photoDocument){$sizes[$photoDocument]}
$pixels=if($canvasSize){[long]$canvasSize[0]*$canvasSize[1]}else{0}
$guaranteeSize=if($pixels -lt 20000000){1024}elseif($pixels -lt 40000000){1536}else{2048}
if($Cold -and (Test-Path -LiteralPath $storage)){Remove-Item -LiteralPath $storage -Recurse -Force}
foreach($folder in $trace,(Join-Path $passDirectory 'input'),(Join-Path $storage 'config')){[IO.Directory]::CreateDirectory($folder)|Out-Null}
$settings=Join-Path $storage 'config/settings.json'
if(!(Test-Path -LiteralPath $settings)){[IO.File]::WriteAllText($settings,(@{language=@{Explicit='en'};theme='dark'}|ConvertTo-Json -Depth 4))}
$CapyTraceDirectory=$trace
$presets=@{1='G-Pen';2='Pencil';4='Paintbrush';11='Wet Round';12='Liquify Push';40='Clone Stamp';45='Soft Blender'}
$brushTools=@{
 'G-Pen'=@('Pen','');'Pencil'=@('Pencil','Pencil');'Paintbrush'=@('Paint Brush','Paint')
 'Wet Round'=@('Paint Brush','Oil paint');'Soft Blender'=@('Blend','Blend')
 'Liquify Push'=@('Liquify','');'Clone Stamp'=@('Clone Stamp','')
}
$tools=@{lasso='Lasso selection';magic_wand='Auto select';fill='Fill';move='Operation'}
$workspaceIds=@{photo='builtin:workspace:photographer'}
$review=$null;$root=$null;$window=$null;$drawing=$null;$probe=$null;$surfaceInfo=$null;$windowId=$null;$sampler=$null;$presentMonProcess=$null;$area=$null;$canvasRect=$null;$inputs=0
$openDocuments=@{};$saved=@{};$active=$null
$checks=[ordered]@{start=$null;end=$null};$memoryEnd=$null;$failure=$null;$framesNote=$null
$entries=[Collections.Generic.List[object]]::new();$strokeIds=@($chainData.steps|Where-Object {$_.'do' -eq 'stroke'}|ForEach-Object {$_.id})
$qpcFrequency=[Diagnostics.Stopwatch]::Frequency
function Ns([long]$Ticks){[long][Math]::Truncate([decimal]$Ticks*1000000000/$qpcFrequency)}
function Now{Ns ([Diagnostics.Stopwatch]::GetTimestamp())}
function Rect-List($Rect){@([int]$Rect.X,[int]$Rect.Y,[int]$Rect.Width,[int]$Rect.Height)}
function Center($Element){$b=$Element.Current.BoundingRectangle;@([int]($b.X+$b.Width/2),[int]($b.Y+$b.Height/2))}
function Dip{[CapyWindowApi]::GetDpiForWindow($window)/96.0}
function Document-Path([string]$Name){
 if($saved.ContainsKey($Name)){return $saved[$Name]}
 if(!$Documents.ContainsKey($Name)){throw "No Windows file for chain document '$Name'"}
 $Documents[$Name]
}
function Free-Bytes{[IO.DriveInfo]::new($storage).AvailableFreeSpace}
function Motion-Ms($Step){
 if($Step.duration_ms){return [double]$Step.duration_ms}
 $frames=if($Step.frames){$Step.frames}else{$chainData.motion_frames}
 [double]$frames*1000/$refreshHz
}
function Start-App([string[]]$Arguments,[switch]$Logged){
 Enter-CapyEnvironment @('CAPY_LATENCY_TRACE')
 try{
  $env:CAPY_STORAGE_DIR=$storage;$env:CAPY_LATENCY_TRACE='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
  $start=@{FilePath=$Executable;WorkingDirectory=$trace;PassThru=$true}
  if($Arguments){$start.ArgumentList=@($Arguments|ForEach-Object {'"'+$_+'"'})}
  if($Logged){$start.RedirectStandardError=Join-Path $passDirectory 'stderr.log'}
  $process=Start-Process @start;$null=$process.Handle;$process
 }finally{Exit-CapyEnvironment}
}
function Bench-Window{
 $registered=@(Owned-DrawingWindow $review)
 if($registered.Count){return $registered[0]}
 $owned=[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty,$review.Id)
 $canvas=[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'drawing-canvas')
 foreach($top in [System.Windows.Automation.AutomationElement]::RootElement.FindAll([System.Windows.Automation.TreeScope]::Children,$owned)){
  $handle=[IntPtr]$top.Current.NativeWindowHandle;$owner=[uint32]0;[CapyWindowApi]::GetWindowThreadProcessId($handle,[ref]$owner)|Out-Null
  if($owner -eq $review.Id -and $top.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$canvas)){return [pscustomobject]@{Root=$top;Handle=$handle}}
 }
}
function Canvas-Ready{$canvas=Find 'drawing-canvas';$canvas -and $canvas.Current.IsEnabled -and !(Find 'launch-splash' -Visible) -and !(Find 'canvas-status' -Visible)}
function Screen-Hash([Windows.Rect]$Rect=$script:canvasRect){[CapyBenchScreen]::Hash([int]$Rect.X,[int]$Rect.Y,[int]$Rect.Width,[int]$Rect.Height)}
function Observe([scriptblock]$Ready,[string]$Message,[double]$Seconds=30,[switch]$Stable,$Changed,[switch]$Quiet){
 $oWatch=[Diagnostics.Stopwatch]::StartNew();$oSince=$null;$oLast=$null
 do{
  $oRect=try{if(!$Ready -or [bool](& $Ready)){(Find 'drawing-canvas' -Visible).Current.BoundingRectangle}}catch [System.Windows.Automation.ElementNotAvailableException]{$null}
  $oOk=$oRect -and !$oRect.IsEmpty -and $oRect.Width -gt 0 -and $oRect.Height -gt 0 -and !(@($oRect.X,$oRect.Y,$oRect.Width,$oRect.Height)|Where-Object {[double]::IsNaN($_) -or [double]::IsInfinity($_)})
  $oHash=$null
  if($oOk){
   if($oRect -ne $script:canvasRect){$oLast=$null}
   $script:canvasRect=$oRect
   if($Stable -or $null -ne $Changed){$oHash=Screen-Hash $oRect}
  }
  $oAt=Now
  if($oOk -and ($null -eq $Changed -or $oHash -ne $Changed)){
   if(!$Stable){return $oAt}
   if($null -eq $oLast -or $oHash -ne $oLast){$oLast=$oHash;$oSince=$oAt}elseif($oAt-$oSince -ge 200000000){return $oSince}
  }else{$oLast=$null}
  $review.Refresh();if($review.HasExited){throw "The app exited with code $($review.ExitCode) while waiting: $Message"}
  Start-Sleep -Milliseconds 5
 }while($oWatch.Elapsed.TotalSeconds -lt $Seconds)
 if($Quiet){return $null}
 throw [TimeoutException]::new("$Message (waited $Seconds s)")
}
function Wait-Idle{$null=Observe {Canvas-Ready} 'The canvas did not settle before the next input' 60 -Stable}
function Chord([uint16[]]$Modifiers,[uint16]$Key){Navigation-Focus;[CapyRowPointer]::Chord([uint32]$review.Id,$Modifiers,$Key)}
function Clear-Work-Area([Windows.Rect]$Bounds,[Windows.Rect[]]$Obstacles){
 $minWidth=$Bounds.Width/4;$minHeight=$Bounds.Height/4
 $candidates=@($Bounds)
 foreach($obstacle in ($Obstacles|Sort-Object {$_.Width*$_.Height} -Descending)){
  $next=@{}
  foreach($rect in $candidates){
   $left=[Math]::Max($rect.Left,$obstacle.Left);$right=[Math]::Min($rect.Right,$obstacle.Right)
   $top=[Math]::Max($rect.Top,$obstacle.Top);$bottom=[Math]::Min($rect.Bottom,$obstacle.Bottom)
   if($left -ge $right -or $top -ge $bottom){$next[$rect]=$rect;continue}
   if($left-$rect.Left -ge $minWidth){$part=[Windows.Rect]::new($rect.Left,$rect.Top,$left-$rect.Left,$rect.Height);$next[$part]=$part}
   if($rect.Right-$right -ge $minWidth){$part=[Windows.Rect]::new($right,$rect.Top,$rect.Right-$right,$rect.Height);$next[$part]=$part}
   if($top-$rect.Top -ge $minHeight){$part=[Windows.Rect]::new($rect.Left,$rect.Top,$rect.Width,$top-$rect.Top);$next[$part]=$part}
   if($rect.Bottom-$bottom -ge $minHeight){$part=[Windows.Rect]::new($rect.Left,$bottom,$rect.Width,$rect.Bottom-$bottom);$next[$part]=$part}
  }
  $candidates=@($next.Values)
  if(!$candidates.Count){throw "Owned controls leave no usable canvas work area: canvas $(Rect-List $Bounds)"}
 }
 @($candidates|Sort-Object {$_.Width*$_.Height},{-$_.Top},{-$_.Left},{$_.Width} -Descending)[0]
}
function Canvas-Area([switch]$Refresh){
 $bounds=(Control 'drawing-canvas' -Arranged).Current.BoundingRectangle
 if($Refresh -or !$script:area -or $script:area.bounds -ne $bounds){
  $region=Motion-Canvas
  $script:area=[pscustomobject]@{bounds=$bounds;region=$region;rect=(Clear-Work-Area $bounds @($region.obstacles|ForEach-Object bounds))}
 }
 $script:area
}
function At([double]$X,[double]$Y){$r=(Canvas-Area).rect;@([int]($r.X+$X*$r.Width),[int]($r.Y+$Y*$r.Height))}
function Check-Geometry{
 $current=Canvas-Area -Refresh;$owner=[uint32]0;[CapyWindowApi]::GetWindowThreadProcessId($window,[ref]$owner)|Out-Null
 if($owner -ne $review.Id){throw 'Geometry check: the drawing window no longer belongs to the launched app'}
 foreach($x in .2,.35,.5,.65,.8){foreach($y in .25,.4,.5,.6,.75){$p=At $x $y;$null=Motion-Point $current.region ([Windows.Point]::new($p[0],$p[1]))}}
 [ordered]@{ok=$true;dpi=[CapyWindowApi]::GetDpiForWindow($window);canvas=Rect-List $current.bounds;work_area=Rect-List $current.rect;obstacles=@($current.region.obstacles).Count}
}
function Inject([uint32]$Kind,[int]$Contacts,[int[]]$Path,[int[]]$Pressure,[string]$Name){
 $script:inputs++;$file=Join-Path $passDirectory ('input/{0:d3}-{1}.csv' -f $script:inputs,($Name -replace '[^\w.-]','_'))
 [WindowsPenMotion]::Play([uint32]$review.Id,$Kind,$Contacts,$Path,$Pressure,$(if($Kind -eq 3){200}else{240}),$file)
 $rows=@(Import-Csv -LiteralPath $file)
 [ordered]@{down_ns=Ns $rows[0].qpc_before;up_ns=Ns $rows[-1].qpc_before}
}
function Pen([int[]]$Path,[string]$Name,[int[]]$Pressure){if(!$Pressure){$Pressure=@(1024)*[int]($Path.Length/2)};Inject 3 1 $Path $Pressure $Name}
function Contact($Entry,$Contact){$Entry.contacts+=$Contact;$Entry.motion+=[ordered]@{begin_ns=$Contact.down_ns;end_ns=$Contact.up_ns}}
function Latency($Entry,[string]$Name,[long]$InputNs,[long]$Observed,[string]$Surface){
 $Entry.latency+=[ordered]@{name=$Name;input_ns=$InputNs;ready_ns=$Observed;surface=$Surface;observed_ns=$Observed;evidence='completed'}
}
function Key-Latency($Entry,[string]$Name,[uint16[]]$Modifiers,[uint16]$Key,[scriptblock]$Ready,[switch]$Changed,[switch]$Stable,[string]$Surface='canvas'){
 Wait-Idle;Navigation-Focus;$before=Screen-Hash
 $at=Now;[CapyRowPointer]::Chord([uint32]$review.Id,$Modifiers,$Key)
 Latency $Entry $Name $at (Observe $Ready "$Name did not complete" 60 -Stable:$Stable -Changed $(if($Changed){$before})) $Surface
}
function Tap-Latency($Entry,[string]$Name,[int[]]$Point,[uint32]$Kind,[scriptblock]$Ready,[switch]$Changed,[switch]$Stable,[string]$Surface,[double]$Seconds=60){
 Wait-Idle;[CapyWindowApi]::SetForegroundWindow($window)|Out-Null;$before=Screen-Hash
 $tap=Inject $Kind 1 (@($Point)*6) $(if($Kind -eq 3){@(1024)*6}) $Name
 $at=if($Kind -eq 3){$tap.down_ns}else{$tap.up_ns}
 Latency $Entry $Name $at (Observe $Ready "$Name did not complete" $Seconds -Stable:$Stable -Changed $(if($Changed){$before})) $Surface
}
function Search-Latency($Entry,[string]$Name,[string]$Label,[scriptblock]$Ready,[string]$Surface){
 Wait-Idle;$at=Ns (Navigation-Command $Label -Enter)
 Latency $Entry $Name $at (Observe $Ready "$Name did not complete" 60 -Stable) $Surface
 Wait-Until {!(Find 'command-search' -Visible)} "Command search did not close after $Label"
}
function Skip($Entry,[string]$Reason){$Entry.status='skipped';$Entry.reason=$Reason}
function Find-ToolChoice([string]$Label,[string]$IdPattern,$Within=$root){
 $name=[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,$Label)
 $matches=@($Within.FindAll([System.Windows.Automation.TreeScope]::Descendants,$name)|Where-Object {
  $_.Current.AutomationId -match $IdPattern -and $_.Current.IsEnabled -and (Test-CapySearchItem $_ $root $review.Id)
 })
 if($matches.Count -gt 1){throw "Multiple visible native tool controls named $Label"}
 if($matches.Count -eq 1){$matches[0]}
}
function Panel-Group($Group){
 $controlId=$Group.Current.AutomationId
 while($group -and $group.Current.AutomationId -notmatch '^workspace-group-\d+$'){
  $group=[System.Windows.Automation.TreeWalker]::RawViewWalker.GetParent($group)
 }
 if(!$group){throw "$ControlId does not have an owned workspace group"}
 $group
}
function Select-Preset([string]$Label){
 $tile,$group=$brushTools[$Label]
 $toolSet=Panel-Group (Control 'panel-tab-brushes' -Arranged)
 $presetPattern='^tool-subtool-\d+$';$groupPattern='^tool-group-\d+$'
 $row=@{item=(Find-ToolChoice $Label $presetPattern $toolSet)}
 if(!$row.item){
  $choice=@{item=$(if($group){Find-ToolChoice $group $groupPattern $toolSet})}
  if(!$choice.item){
   $toolbar=Find-ToolChoice $tile '^tile-toolbar-\d+$'
   if(!$toolbar){throw "No visible native toolbar tile for $tile"}
   $toolbar.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
   if($group){Wait-Until {$choice.item=Find-ToolChoice $group $groupPattern $toolSet;$choice.item} "The $group brush group did not appear"}
  }
  if($choice.item -and $choice.item.Current.ItemStatus -ne 'Selected'){$choice.item.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()}
  Wait-Until {$row.item=Find-ToolChoice $Label $presetPattern $toolSet;$row.item} "The $Label preset did not appear in Tool Set"
 }
 if($row.item.Current.ItemStatus -ne 'Selected'){$row.item.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()}
 Wait-Until {(Find-ToolChoice $Label $presetPattern $toolSet).Current.ItemStatus -eq 'Selected'} "The $Label preset did not become the selected brush"
 if(Find 'tool-drawer' -Visible){
  [CapyRowPointer]::Key([uint32]$review.Id,0x1b)
  Wait-Until {!(Find 'tool-drawer' -Visible)} 'The Tool Set drawer did not close before canvas input'
 }
}
function Set-BrushSize([int]$Size){
 $field=Control 'tool-setting-size';$field.SetFocus();$field.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue([string]$Size)
 Navigation-Focus
 Wait-Until {((Control 'tool-setting-size').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value -replace '[^\d]','') -eq "$Size"} "Brush size $Size px was not accepted"
}
function Brush-Controls{if(!(Find 'tool-setting-size-slider' -Visible) -or !(Find 'tool-subtool-0' -Visible)){Select-Preset 'G-Pen';'G-Pen'}}
function Layers-List{
 if(!(Find 'layer-list' -Visible)){
  $anchor=Find 'panel-tab-layers' -Visible
  if(!$anchor){$anchor=Find 'column-icon-layers' -Visible}
  if(!$anchor){throw 'The Layers panel has no visible tab or column control'}
  Invoke-Id $anchor.Current.AutomationId
 }
 Control 'layer-list' -Arranged
}
function Layer-Rows($List=(Find 'layer-list' -Visible)){if($List){@($List.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition)|Where-Object {$_.Current.AutomationId -match '^layer-row-\d+$'}).Count}}
function Select-Layer([string]$Label){
 if($Label -eq 'Photo'){$Label=[IO.Path]::GetFileNameWithoutExtension((Document-Path $photoDocument))}
 $list=Layers-List
 $row=Control ($Label+' layer row') -Name -Within $list
 $id=$row.Current.AutomationId -replace '^layer-row-','';$scroll=$null
 if(!(Find "layer-$id-name" -Within $list -Visible) -and $row.TryGetCurrentPattern([System.Windows.Automation.ScrollItemPattern]::Pattern,[ref]$scroll)){$scroll.ScrollIntoView()}
 $null=Inject 2 1 (@(Center (Control "layer-$id-name" -Within $list -Arranged))*6) $null "layer-$Label"
 Wait-Until {(Find "layer-$id-selection" -Within $list -Visible).Current.ItemStatus -eq 'Selected'} "The $Label layer did not become selected"
}
function Prefixed([string]$Pattern,$Within=$root){@($Within.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition)|Where-Object {$_.Current.AutomationId -match $Pattern -and !$_.Current.IsOffscreen})}
function Tabs{$strip=Find 'drawing-tabs';if($strip){Prefixed '^drawing-tab-(?!slide$)' $strip}else{@()}}
function Discard{
 $condition=[System.Windows.Automation.AndCondition]::new([System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,'Discard Changes'),[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::Button))
 $owned=[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty,$review.Id)
 foreach($top in [System.Windows.Automation.AutomationElement]::RootElement.FindAll([System.Windows.Automation.TreeScope]::Children,$owned)){
  $button=$top.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$condition)
  if($button -and $button.Current.IsEnabled){$button.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke();return}
 }
}
function Close-App{
 if($review.HasExited){return}
 if(!$window){Stop-Process -Id $review.Id -Force;$script:framesNote='The app was force-stopped before its window opened';return}
 [CapyWindowApi]::PostMessage($window,0x10,[UIntPtr]::Zero,[IntPtr]::Zero)|Out-Null
 Wait-Until {$review.HasExited -or (Discard)} 'The app did not exit after Close and Discard Changes' 60 -Closing
}
function Navigator-Overview{
 if(!(Find 'navigator-overview' -Visible)){
  $anchor=@('panel-tab-navigator','column-icon-navigator'|ForEach-Object {Find $_ -Visible}|Where-Object {$_})[0]
  if(!$anchor){return}
  Invoke-Id $anchor.Current.AutomationId
 }
 Control 'navigator-overview' -Arranged
}
function Place($Step){
 $anchor=@($Step.anchor)
 if($null -ne $Step.zoom -and "$($Step.zoom)" -ne 'fit' -and [double]$Step.zoom -ne 1){return "Zoom $($Step.zoom) has no Windows shortcut; only fit and 1.0 do"}
 if($anchor.Count -eq 2 -and !($active -and $sizes.ContainsKey($active))){return "The Windows runner reads drawing sizes only from photo files, so it cannot anchor on $active"}
 if("$($Step.zoom)" -eq 'fit' -or $anchor.Count -eq 2){Chord @(0x11) 0x30;Wait-Idle}
 if($null -ne $Step.zoom -and "$($Step.zoom)" -ne 'fit'){Chord @(0x11) 0x31;Wait-Idle}
 if($anchor.Count -eq 2){
  $overview=Navigator-Overview;$toggled=!$overview
  if($toggled){Navigation-Command 'Navigator panel';$overview=Control 'navigator-overview' -Arranged}
  $b=$overview.Current.BoundingRectangle;$size=$sizes[$active];$fit=[Math]::Min(($b.Width-8*(Dip))/$size[0],($b.Height-8*(Dip))/$size[1])
  $point=@([int]($b.X+($b.Width-$size[0]*$fit)/2+[double]$anchor[0]*$size[0]*$fit),[int]($b.Y+($b.Height-$size[1]*$fit)/2+[double]$anchor[1]*$size[1]*$fit))
  $null=Inject 2 1 (@($point)*6) $null 'navigator-anchor';Wait-Idle
  if($toggled){Navigation-Command 'Navigator panel';Wait-Until {!(Find 'navigator-overview' -Visible)} 'The Navigator panel did not close after anchoring'}
  Wait-Idle
 }
}
function Canvas-Signature{$r=(Canvas-Area -Refresh).rect;$rows=0;$rgb=[CapyBenchScreen]::Signature([int]$r.X,[int]$r.Y,[int]$r.Width,[int]$r.Height,[ref]$rows);[ordered]@{w=32;h=$rows;rgb=$rgb}}
function Notice-Text{$notice=Find 'canvas-notice-text' -Visible;if($notice){$notice.Current.Name}}
function Status-Error{$status=Find 'canvas-status' -Visible;if($status -and $status.Current.Name -like '*exceeds the codec memory budget*'){$status.Current.Name}}
function Triangle([int]$K,[int]$N){1-[Math]::Abs(2.0*$K/[Math]::Max(1,$N-1)-1)}
function Frames($Step,[int]$Hz){[Math]::Max(2,[int]((Motion-Ms $Step)*$Hz/1000))}
function Stroke-Path($Step,[int]$Contact){
 $r=(Canvas-Area).rect;$cx=$r.X+$r.Width/2;$cy=$r.Y+$r.Height/2;$rx=.3*$r.Width;$ry=.2*$r.Height
 $count=[int]$Step.contacts;$per=[Math]::Max(2,[int]((Motion-Ms $Step)*200/1000/$count));$total=$per*$count
 $path=[Collections.Generic.List[int]]::new();$pressure=[Collections.Generic.List[int]]::new()
 foreach($k in 0..($per-1)){
  $i=$Contact*$per+$k;$t=$i/200.0;$u=$i/[double]($total-1)
  if($Step.path -eq 'zigzag'){$x=$cx+$rx*(1-4*[Math]::Abs($t%1-.5));$y=$cy+$ry*(2*$u-1)}else{$a=2*[Math]::PI*$t;$x=$cx+$rx*[Math]::Cos($a);$y=$cy+$ry*[Math]::Sin($a)}
  $path.Add([int]$x);$path.Add([int]$y);$pressure.Add([int](640+384*[Math]::Sin(2*[Math]::PI*$t)))
 }
 @{path=$path.ToArray();pressure=$pressure.ToArray()}
}
function Gesture-Path($Step){
 $scale=if($null -ne $Step.scale){[double]$Step.scale}else{2.5}
 $r=(Canvas-Area).rect;$cx=$r.X+$r.Width/2;$cy=$r.Y+$r.Height/2;$n=Frames $Step 240;$path=[Collections.Generic.List[int]]::new()
 $place={param($d,$a,$x)foreach($v in @(($x-$d*[Math]::Cos($a)),($cy-$d*[Math]::Sin($a)),($x+$d*[Math]::Cos($a)),($cy+$d*[Math]::Sin($a)))){$path.Add([int]$v)}}
 foreach($k in 0..($n-1)){
  $s=Triangle $k $n
  switch($Step.gesture){
   'pan'{& $place (.1*$r.Width) 0 ($cx-.15*$r.Width+.3*$r.Width*$s)}
   'pinch'{& $place (.08*$r.Width*(1+($scale-1)*$s)) 0 $cx}
   'rotate'{& $place (.15*[Math]::Min($r.Width,$r.Height)) ([Math]::PI/3*$s) $cx}
  }
 }
 if($Step.release -eq 'quick'){foreach($k in 1..24){& $place (.08*$r.Width*(1+.5*$k*$k/576.0)) 0 $cx}}
 $path.ToArray()
}
function Drag-Path([string]$Kind,[int[]]$Start,[int]$Frames,[double]$Extent,[int[]]$ReturnPoint){
 $r=(Canvas-Area).rect;$path=[Collections.Generic.List[int]]::new()
 foreach($k in 0..($Frames-1)){
  $u=$k/[double]($Frames-1);$s=Triangle $k $Frames
  switch($Kind){
   'loop'{$a=2*[Math]::PI*$u;$p=@(($Start[0]+.2*$r.Width*[Math]::Cos($a)),($Start[1]+.2*$r.Height*[Math]::Sin($a)))}
   'arc'{$p=@(($Start[0]+$Extent-$Extent*[Math]::Cos([Math]::PI*$u)),($Start[1]-$Extent*[Math]::Sin([Math]::PI*$u)))}
   'sweep'{$p=@(($Start[0]+$Extent*$s),$Start[1])}
   'float'{
    $f=@(($Start[0]+$Extent),$Start[1]);$radius=60*(Dip)
    $p=if($u -lt .3){$v=$u/.3;@(($Start[0]+$Extent*$v),$Start[1])}elseif($u -lt .7){$a=2*[Math]::PI*($u-.3)/.4;@(($f[0]+$radius*[Math]::Cos($a)-$radius),($f[1]+$radius*[Math]::Sin($a)))}else{$v=($u-.7)/.3;@(($f[0]+($ReturnPoint[0]-$f[0])*$v),($f[1]+($ReturnPoint[1]-$f[1])*$v))}
   }
  }
  $path.Add([int]$p[0]);$path.Add([int]$p[1])
 }
 $path.ToArray()
}
function Settle($Entry,[long]$CapMs){
 $observed=Observe {Canvas-Ready} 'settle' ($CapMs/1000.0) -Stable -Quiet
 if($null -eq $observed){$Entry.extra.settle='capped'}else{$Entry.extra.settle_observed_ns=$observed}
}
$handlers=@{
 launch={param($s,$e)
  $at=Now;$script:review=Start-App -Logged
  $review.Id|Set-Content -LiteralPath (Join-Path $passDirectory 'process-id.txt')
  Wait-Until {$script:drawing=Bench-Window;$drawing} 'Launch: the owned drawing window did not open' 120
  $script:root=$drawing.Root;$script:window=$drawing.Handle
  $script:sampler=[CapyBenchMemory]::new($review.Id,(Join-Path $passDirectory 'memory.jsonl'))
  Start-PresentMon
  Wait-Until {$script:probe=Trace-File 'presentation-probe';$probe} 'Launch: the renderer did not record a ready brush (presentation-probe trace)' 120
  $meta=Read-Snapshot $probe;$script:windowId=[long]$meta.window_id;$script:surfaceInfo=$meta
  $inputReady=Observe {Canvas-Ready} 'Launch: the canvas did not become ready for input' 120
  Latency $e 'launch' $at ([long]$meta.ready_qpc_ns) 'canvas'
  $e.extra.input_ready_ns=$inputReady
  $root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Maximized)
  Wait-Until {$root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).Current.WindowVisualState -eq [System.Windows.Automation.WindowVisualState]::Maximized} 'Launch: the window did not maximize'
  Wait-Idle
 }
 open={param($s,$e)
  if($s.reopen){
   if(!$saved.ContainsKey($s.document)){Skip $e "$($s.document) was not saved in this pass";return}
   if(@(Tabs).Count -lt 2){Skip $e "Closing the only open drawing ends the app, so $($s.document) cannot reopen";return}
   if($root.Current.Name -notlike "*$([IO.Path]::GetFileNameWithoutExtension($saved[$s.document]))*"){throw "Reopen: $($s.document) is not the active drawing ('$($root.Current.Name)')"}
   Chord @(0x11) 0x30;Wait-Idle;$e.extra.composite_before=Canvas-Signature
   $open=@(Tabs).Count;Chord @(0x11) 0x57
   Wait-Until {@(Tabs).Count -lt $open -or (Discard)} "Reopen: Close did not close $($s.document)" 60
   $openDocuments.Remove($s.document);Wait-Idle
  }
  $path=Document-Path $s.document;$stem=[IO.Path]::GetFileNameWithoutExtension($path)
  Wait-Idle;$notice=Notice-Text;$statusError=Status-Error;$free=Free-Bytes;$at=Now;$forward=Start-App @($path)
  if(!$forward.WaitForExit(30000)){throw "Open: the file activation of $path did not hand off to the running app"}
  if($forward.ExitCode -ne 0){throw "Open: the file activation of $path exited with $($forward.ExitCode)"}
  $ready={
   $package=Find 'package-view' -Visible
   if($package){$reason=$package.Current.Name;[CapyRowPointer]::Key([uint32]$review.Id,0x1b);throw [CapyBenchRefused]::new("Opening $($s.document) showed a package dialog: $reason")}
   $now=Notice-Text;if($now -and $now -ne $notice){throw [CapyBenchRefused]::new($now)}
   $now=Status-Error;if($now -and $now -ne $statusError){throw [CapyBenchRefused]::new($now)}
   $root.Current.Name -like "*$stem*" -and (Canvas-Ready)
  }
  Latency $e "open.$($s.document)" $at (Observe $ready "Open: $path did not become the ready drawing" 180 -Stable) 'canvas'
  $e.extra.disk_free_bytes=@($free,(Free-Bytes));$openDocuments[$s.document]=$true;$script:active=$s.document
  if($s.reopen){Chord @(0x11) 0x30;Wait-Idle;$e.extra.composite_after=Canvas-Signature}
  elseif($chainData.documents.($s.document).kind -eq 'photo'){
   $rows=Layer-Rows (Layers-List)
   Chord @(0x11,0x10) 0x4E
   Wait-Until {(Layer-Rows) -eq $rows+1} 'Open: New layer did not add a layer row to the photo'
   Wait-Idle
  }
 }
 stroke={param($s,$e)
  $label=$presets[[int]$s.preset];if(!$label){Skip $e "Preset $($s.preset) has no English preset name in the Windows runner";return}
  Select-Preset $label
  $size=if($s.size -eq 'guarantee'){$guaranteeSize}else{[int]$s.size};Set-BrushSize $size
  $reason=Place $s;if($reason){Skip $e $reason;return}
  if($s.layer){Select-Layer $s.layer}
  Wait-Idle;Navigation-Focus
  $e.extra.priming=Pen (@(At .5 .5)*20) "$($s.id)-priming"
  Wait-Idle;Navigation-Focus
  foreach($c in 0..([int]$s.contacts-1)){
   $stroke=Stroke-Path $s $c
   Contact $e (Pen $stroke.path $s.id $stroke.pressure)
   if($c -lt [int]$s.contacts-1){Start-Sleep -Milliseconds 100}
  }
  Settle $e $(if($s.catch_up_cap_ms){[long]$s.catch_up_cap_ms}else{3000})
  $e.extra.preset=$label;$e.extra.size=$size;$e.extra.camera=(Control 'canvas-camera').Current.Name
 }
 gesture={param($s,$e)
  if($s.gesture -notin 'pan','pinch','rotate'){Skip $e "Gesture '$($s.gesture)' has no Windows path";return}
  $reason=Place $s;if($reason){Skip $e $reason;return}
  Wait-Idle;Navigation-Focus
  Contact $e (Inject 2 2 (Gesture-Path $s) $null $s.id)
  if($s.release -eq 'quick'){Settle $e 3000}
 }
 drag={param($s,$e)
  $canvasTarget=$s.target -eq 'canvas' -or $s.target -like 'object:*'
  if($s.tool -and !$tools[$s.tool]){Skip $e "Tool '$($s.tool)' has no Windows command label";return}
  if($s.select -eq 'all'){Chord @(0x11) 0x41;Wait-Idle}
  if($s.target -like 'object:*'){Select-Layer $s.target.Substring(7)}
  if($s.tool){Navigation-Command $tools[$s.tool]}
  if($s.command -eq 'transform'){Chord @(0x11) 0x54;Wait-Until {Find 'canvas-bar-apply_transform' -Visible} 'Drag: Transform did not show its Apply control'}
  elseif($s.command){Skip $e "Command '$($s.command)' has no Windows mapping";return}
  if($s.target -in 'slider:size','list:brushes'){$e.extra.prepared=Brush-Controls}
  $start=$null;$extent=0;$returnPoint=$null;$kind=$s.path
  switch -regex ($s.target){
   '^(canvas|object:.+)$'{$start=At .5 .5;$extent=.15*(Canvas-Area).rect.Width}
   '^slider:size$'{
    $slider=Control 'tool-setting-size-slider' -Arranged;$range=$slider.GetCurrentPattern([System.Windows.Automation.RangeValuePattern]::Pattern).Current;$b=$slider.Current.BoundingRectangle
    $fraction=($range.Value-$range.Minimum)/[Math]::Max(1e-9,$range.Maximum-$range.Minimum);$start=@([int]($b.X+4+($b.Width-8)*$fraction),[int]($b.Y+$b.Height/2))
    $extent=120*(Dip)*$(if($fraction -gt .5){-1}else{1})
   }
   '^list:brushes$'{
    $node=Control 'tool-subtool-0' -Arranged;$scroll=$null
    while($node -and !$scroll){$pattern=$null;if($node.TryGetCurrentPattern([System.Windows.Automation.ScrollPattern]::Pattern,[ref]$pattern) -and $pattern.Current.VerticallyScrollable){$scroll=$node}else{$node=[System.Windows.Automation.TreeWalker]::ControlViewWalker.GetParent($node)}}
    if(!$scroll){Skip $e 'The brushes list does not scroll in this layout';return}
    $b=$scroll.Current.BoundingRectangle;$start=@([int]($b.X+$b.Width/2),[int]($b.Y+$b.Height*.7));$extent=$b.Height*.4
   }
   '^grip:layers$'{
    $source=Panel-Group (Layers-List)
    $group=$source.Current.AutomationId.Substring(16)
    $bounds=$source.Current.BoundingRectangle
    $previous=@(Prefixed '^workspace-group-\d+$'|ForEach-Object {$_.Current.BoundingRectangle}|Where-Object {
     [Math]::Abs($_.Left-$bounds.Left) -lt 8 -and [Math]::Abs($_.Right-$bounds.Right) -lt 8 -and $_.Bottom -le $bounds.Top+2
    }|Sort-Object Bottom -Descending)[0]
    if(!$previous){throw 'The Layers group has no preceding dock group for its return slot'}
    $returnPoint=@([int]($previous.Left+$previous.Width/2),[int]([Math]::Max($bounds.Bottom,$previous.Bottom)-8*(Dip)))
    $grip=Control "group-grip-$group" -Arranged;$start=Center $grip;$e.extra.grip_before=Rect-List $grip.Current.BoundingRectangle
    $extent=.25*(Canvas-Area).bounds.Width*[Math]::Sign((Canvas-Area).bounds.X+(Canvas-Area).bounds.Width/2-$start[0]);$kind='float'
   }
   '^edge:right-column$'{
    $middle=(Canvas-Area).bounds.X+(Canvas-Area).bounds.Width/2
    $edges=@(Prefixed '^divider-\d+$'|Where-Object {$b=$_.Current.BoundingRectangle;$b.Height -gt $b.Width -and $b.X+$b.Width/2 -gt $middle}|Sort-Object {$_.Current.BoundingRectangle.X})
    if(!$edges.Count){Skip $e 'No resizable column edge right of the canvas in this layout';return}
    $start=Center $edges[0];$extent=-120*(Dip)
   }
   default{Skip $e "Drag target '$($s.target)' has no Windows mapping";return}
  }
  if($kind -notin 'loop','arc','sweep','fling','float'){Skip $e "Drag path '$kind' has no Windows path";return}
  Wait-Idle;Navigation-Focus
  if($kind -eq 'fling'){
   $total=Motion-Ms $s;foreach($direction in -1,1){
    $from=if($direction -lt 0){$start}else{@($start[0],[int]($start[1]-$extent))}
    $path=[Collections.Generic.List[int]]::new();foreach($k in 0..28){$path.Add($from[0]);$path.Add([int]($from[1]+$direction*$extent*$k/28))}
    Contact $e (Inject 2 1 $path.ToArray() $null "$($s.id)-fling")
    Start-Sleep -Milliseconds ([Math]::Max(0,[int]($total/2-120)))
   }
  }else{
   $path=Drag-Path $kind $start (Frames $s $(if($canvasTarget){200}else{240})) $extent $returnPoint
   Contact $e $(if($canvasTarget){Pen $path $s.id}else{Inject 2 1 $path $null $s.id})
  }
  if($s.then -eq 'apply'){Key-Latency $e 'apply' @() 0x0D {!(Find 'canvas-bar-apply_transform' -Visible) -and (Canvas-Ready)} -Stable}
  elseif($s.then){throw "Drag: '$($s.then)' has no Windows mapping"}
  if($kind -eq 'float'){
   Wait-Idle;$after=Rect-List (Control "group-grip-$group" -Arranged).Current.BoundingRectangle;$e.extra.grip_after=$after
   if([Math]::Abs($after[0]-$e.extra.grip_before[0]) -gt 8 -or [Math]::Abs($after[1]-$e.extra.grip_before[1]) -gt 8){throw "Drag: the Layers group did not return to its place (grip $($e.extra.grip_before -join ',') -> $($after -join ','))"}
  }
 }
 tap={param($s,$e)
  if($s.tool -and !$tools[$s.tool]){Skip $e "Tool '$($s.tool)' has no Windows command label";return}
  if($s.target -ne 'canvas'){Skip $e "Tap target '$($s.target)' has no Windows mapping";return}
  $reason=Place $s;if($reason){Skip $e $reason;return}
  if($s.tool){Navigation-Command $tools[$s.tool]}
  $index=0
  foreach($point in $s.points){
   $index++;Tap-Latency $e "$($s.id)-$index" (At $point[0] $point[1]) 3 $null -Changed -Stable -Surface 'canvas' -Seconds 120
  }
 }
 scrub={param($s,$e)
  $n=Frames $s 240
  if($s.layer_opacity){
   if($s.layer){Select-Layer $s.layer}
   $null=Layers-List
   $slider=Control 'layer-opacity-slider' -Arranged;$range=$slider.GetCurrentPattern([System.Windows.Automation.RangeValuePattern]::Pattern).Current;$b=$slider.Current.BoundingRectangle
   $x={param($v)[int]($b.X+4+($b.Width-8)*($v-$range.Minimum)/[Math]::Max(1e-9,$range.Maximum-$range.Minimum))}
   $from=& $x ([double]$s.layer_opacity[0]);$to=& $x ([double]$s.layer_opacity[1]);$y=[int]($b.Y+$b.Height/2)
   $path=[Collections.Generic.List[int]]::new();foreach($k in 0..($n-1)){$path.Add([int]($from+($to-$from)*$k/($n-1)));$path.Add($y)}
   Wait-Idle;Contact $e (Inject 2 1 $path.ToArray() $null $s.id);Wait-Idle
   $e.extra.opacity=$slider.GetCurrentPattern([System.Windows.Automation.RangeValuePattern]::Pattern).Current.Value
   return
  }
  if($s.effect -ne 'curves'){Skip $e "Effect '$($s.effect)' has no Windows mapping";return}
  Select-Layer 'Photo';$rows=Layer-Rows
  Navigation-Command 'Curves'
  Wait-Until {$script:curve=@(Prefixed '-curve$')[0];$curve} 'Scrub: Curves showed no curve editor'
  $b=$curve.Current.BoundingRectangle;$cx=[int]($b.X+$b.Width/2);$cy=[int]($b.Y+$b.Height/2)
  $path=[Collections.Generic.List[int]]::new();foreach($k in 0..($n-1)){$path.Add($cx);$path.Add([int]($cy-.25*$b.Height*(Triangle $k $n)))}
  Wait-Idle;Contact $e (Inject 2 1 $path.ToArray() $null $s.id);Wait-Idle
  [CapyRowPointer]::Key([uint32]$review.Id,0x1b);Wait-Idle
  if((Layer-Rows (Layers-List)) -eq $rows+1){
   $null=Inject 2 1 (@(Center (Control 'layer-delete' -Arranged))*6) $null "$($s.id)-remove"
   Wait-Until {(Layer-Rows) -eq $rows} 'Scrub: deleting the Curves effect layer did not restore the layer rows'
  }
  $e.extra.layer_rows_after=Layer-Rows;Wait-Idle
 }
 repeat={param($s,$e)
  $items=@(foreach($field in 'commands','open','workspaces','tabs'){if($s.$field){$s.$field}})
  if($s.commands -contains 'add_layer' -or $s.commands -contains 'delete_layer'){$null=Layers-List}
  if($s.workspaces){
   $active=@(Prefixed '^workspace-switch-'|Where-Object {$_.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Current.ToggleState -eq 'On'})
   if($active.Count -ne 1){Skip $e 'The header shows no single active workspace switch button';return}
   $startWorkspace=$active[0].Current.AutomationId.Substring(17)
   if($startWorkspace -eq $workspaceIds.photo){Skip $e 'Photo is already the starting workspace, so there is no other workspace to return to';return}
  }
  foreach($round in 1..[int]$s.count){
   foreach($item in $items){
    if($s.workspaces){
     $id=if($item -eq 'start'){$startWorkspace}else{$workspaceIds[$item]};if(!$id){throw "Repeat: workspace '$item' has no Windows id"}
     $button=Control "workspace-switch-$id" -Arranged
     Tap-Latency $e "workspace:$item" (Center $button) 2 {$button.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Current.ToggleState -eq 'On' -and (Canvas-Ready)} -Stable -Surface $s.surface
     continue
    }
    if($s.tabs){
     $stem=[IO.Path]::GetFileNameWithoutExtension((Document-Path $item));$tab=@(Tabs|Where-Object {$_.Current.Name -like "*$stem*"})
     if($tab.Count -ne 1){throw "Repeat: no single drawing tab named like '$stem'"}
     Tap-Latency $e "tab:$item" (Center $tab[0]) 2 {$root.Current.Name -like "*$stem*" -and (Canvas-Ready)} -Changed -Stable -Surface $s.surface
     $script:active=$item
     continue
    }
    switch($item){
     'undo'{Key-Latency $e 'undo' @(0x11) 0x5A $null -Changed -Stable -Surface $s.surface}
     'redo'{Key-Latency $e 'redo' @(0x11) 0x59 $null -Changed -Stable -Surface $s.surface}
     'fit_canvas'{Key-Latency $e 'fit_canvas' @(0x11) 0x30 $null -Stable -Surface $s.surface}
     'add_layer'{$rows=Layer-Rows;if($null -eq $rows){throw 'Repeat: the Layers panel is not visible'};Tap-Latency $e 'add_layer' (Center (Control 'layer-new' -Arranged)) 2 {(Layer-Rows) -eq $rows+1} -Stable -Surface $s.surface}
     'delete_layer'{$rows=Layer-Rows;Tap-Latency $e 'delete_layer' (Center (Control 'layer-delete' -Arranged)) 2 {(Layer-Rows) -eq $rows-1} -Stable -Surface $s.surface}
     'panel:brushes'{
      if(Find 'tool-subtool-0' -Visible){Navigation-Command 'Tool Set panel';Wait-Until {!(Find 'tool-subtool-0' -Visible)} 'Repeat: Tool Set panel did not hide the brushes'}
      Search-Latency $e 'panel:brushes' 'Tool Set panel' {Find 'tool-subtool-0' -Visible} $s.surface
     }
     'menu:main'{
      $menu=@('application-primary-menu','application-menus','application-menu-file'|ForEach-Object {Find $_ -Visible}|Where-Object {$_})[0]
      if(!$menu){throw 'Repeat: no visible main menu button'}
      $menuItems=if($menu.Current.AutomationId -eq 'application-menu-file'){@('open_document')}else{@('application-menu-file','menu-file')}
      $shown={foreach($id in $menuItems){if(Find $id -Visible -Type ([System.Windows.Automation.ControlType]::MenuItem)){return $true}};$false}
      if(& $shown){throw 'Repeat: a menu item is already showing before the main menu opens'}
      Tap-Latency $e 'menu:main' (Center $menu) 2 $shown -Stable -Surface $s.surface
      [CapyRowPointer]::Key([uint32]$review.Id,0x1b);Wait-Until {!(& $shown)} 'Repeat: the main menu did not close'
     }
     default{throw "Repeat: '$item' has no Windows mapping"}
    }
   }
  }
 }
 save={param($s,$e)
  $stem=[IO.Path]::GetFileNameWithoutExtension((Document-Path $s.document))
  if($root.Current.Name -notlike "*$stem*"){throw "Save: $($s.document) is not the active drawing ('$($root.Current.Name)')"}
  $target=Join-Path $storage "bench-save/$($s.document)-$Pass.capy";[IO.Directory]::CreateDirectory((Split-Path -Parent $target))|Out-Null
  if(Test-Path -LiteralPath $target){Remove-Item -LiteralPath $target}
  Wait-Idle;$free=Free-Bytes;Chord @(0x11,0x10) 0x53;Submit-SavePath $target;$at=Now
  $title=[IO.Path]::GetFileName($target)+' '+[char]0x00b7+' Capy Canvas'
  Latency $e 'save' $at (Observe {$root.Current.Name -eq $title -and (Test-Path -LiteralPath $target) -and (Canvas-Ready)} 'Save: the drawing did not save to its scratch path' 180) 'window'
  Wait-Until {!(Find 'Save As' -Name)} 'Save: the Save As dialog did not close'
  $saved[$s.document]=$target;$e.extra.saved_bytes=(Get-Item -LiteralPath $target).Length;$e.extra.disk_free_bytes=@($free,(Free-Bytes))
 }
 resume={param($s,$e)
  Wait-Idle;$rect=$canvasRect;$pattern=$root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern)
  [CapyWindowApi]::ShowWindow($window,6)|Out-Null
  Wait-Until {$pattern.Current.WindowVisualState -eq [System.Windows.Automation.WindowVisualState]::Minimized} 'Resume: the window did not minimize'
  Start-Sleep -Seconds 1;$hidden=Screen-Hash $rect
  $at=Now;[CapyWindowApi]::ShowWindow($window,9)|Out-Null;[CapyWindowApi]::SetForegroundWindow($window)|Out-Null
  Latency $e 'resume' $at (Observe {$pattern.Current.WindowVisualState -ne [System.Windows.Automation.WindowVisualState]::Minimized -and (Canvas-Ready)} 'Resume: the window did not come back with a settled canvas' 60 -Stable -Changed $hidden) 'window'
 }
 close={param($s,$e)
  for($i=0;$i -lt 10 -and !$review.HasExited;$i++){
   $open=@(Tabs).Count;if(!$open){break}
   Chord @(0x11) 0x57
   Wait-Until {$review.HasExited -or @(Tabs).Count -lt $open -or (Discard)} 'Close: Close did not close the active drawing' 60 -Closing
  }
  Close-App;$e.extra.exit_code=$review.ExitCode
 }
}
function Start-PresentMon{
 $admin=([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
 if(!$admin){$script:framesNote='Not elevated, so frames come from DXGI frame statistics';return}
 if(!(Test-Path -LiteralPath $PresentMon)){$script:framesNote="PresentMon not found at $PresentMon, so frames come from DXGI frame statistics";return}
 $script:presentMonProcess=Start-Process -FilePath $PresentMon -ArgumentList @('--process_id',$review.Id,'--output_file',('"'+(Join-Path $passDirectory 'presentmon.csv')+'"'),'--v1_metrics','--qpc_time','--no_console_stats','--terminate_on_proc_exit','--session_name',"CapyBench-$($review.Id)") -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $passDirectory 'presentmon.log') -RedirectStandardError (Join-Path $passDirectory 'presentmon-error.log')
 Start-Sleep -Seconds 1
 if($presentMonProcess.HasExited){$script:presentMonProcess=$null;$script:framesNote='PresentMon exited at start (see presentmon-error.log), so frames come from DXGI frame statistics'}
}
function Run-Step($Step,[string[]]$Needs,[switch]$Drain){
 $script:area=$null
 if($Drain){$null=Observe {Canvas-Ready} 'The canvas did not settle after a timed-out step' 300 -Stable -Quiet}
 $surface=if($Step.surface){$Step.surface}elseif($Step.'do' -in 'resume','save','close' -or ($Step.'do' -eq 'drag' -and $Step.target -notmatch '^(canvas|object:)')){'window'}else{'canvas'}
 $e=[ordered]@{id=$Step.id;status='ok';reason=$null;input='real';begin_ns=(Now);end_ns=$null;surface=$surface;motion=@();contacts=@();completions=@();settled_ns=$null;latency=@();extra=[ordered]@{}}
 try{
  if($Step.'do' -ne 'launch' -and (!$review -or $review.HasExited)){throw "The app is not running (exit code $(if($review){$review.ExitCode}))"}
  $handler=$handlers[[string]$Step.'do'];$missing=@($Needs|Where-Object {$_ -and !$openDocuments.ContainsKey($_)})[0]
  if($missing){Skip $e "$missing did not open"}elseif($handler){& $handler $Step $e}else{Skip $e "No Windows mapping for '$($Step.'do')' steps"}
 }catch{
  $e.status=if($_.Exception -is [TimeoutException]){'timeout'}elseif($_.Exception -is [CapyBenchRefused]){'refused'}else{'error'}
  $e.reason=if($e.status -eq 'refused'){$_.Exception.Message}else{"$($Step.id): $($_.Exception.Message)"};$e.extra.trace=$_.ScriptStackTrace
 }
 $e.end_ns=Now
 Write-Host ('{0,-22} {1,-8} {2}' -f $Step.id,$e.status,$e.reason)
 $e
}
function Trace-Rows([string]$Kind){$file=Join-Path $trace "latency-$($review.Id)-$windowId-$Kind.csv";if(Test-Path -LiteralPath $file){@(Import-Csv -LiteralPath $file)}}
function Canvas-Frames{
 $previous=0
 foreach($row in Trace-Rows 'presentations'){$present=[long]$row.last_present;if($present -gt 0 -and $present -ne $previous){$previous=$present;[pscustomobject]@{t_ns=[long]$row.render_end_ns;surface='canvas'}}}
}
function PresentMon-Frames{
 $rows=@(Import-Csv -LiteralPath (Join-Path $passDirectory 'presentmon.csv'))
 if(!$rows.Count){throw 'PresentMon wrote no presents'}
 $column=@('TimeInQPC','QPCTime'|Where-Object {$rows[0].PSObject.Properties.Name -contains $_})[0]
 if(!$column){throw "PresentMon CSV has no QPC time column: $($rows[0].PSObject.Properties.Name -join ',')"}
 $chains=@($surfaceInfo.swap_chain_addresses|ForEach-Object {[Convert]::ToUInt64(($_ -replace '^0x',''),16)})
 foreach($row in $rows){
  $shown=0.0;if([string]$row.ProcessID -ne [string]$review.Id -or ![double]::TryParse([string]$row.MsUntilDisplayed,[Globalization.NumberStyles]::Float,[Globalization.CultureInfo]::InvariantCulture,[ref]$shown)){continue}
  $chain=[Convert]::ToUInt64(([string]$row.SwapChainAddress -replace '^0x',''),16)
  [pscustomobject]@{t_ns=(Ns ([long]$row.$column))+[long]($shown*1e6);surface=$(if($chains -contains $chain){'canvas'}else{'window'})}
 }
}
function Last-Frame([long[]]$Frames,[long]$After,[long]$Until){
 if(!$Frames.Count){return}
 $i=[Array]::BinarySearch($Frames,$Until);if($i -lt 0){$i=-$i-2}
 if($i -ge 0 -and $Frames[$i] -gt $After){$Frames[$i]}
}
try{
 $recent=$null
 foreach($step in $chainData.steps){
  if($step.'do' -eq 'open'){$recent=$step.document}
  if($selected -and !$selected.ContainsKey($step.id)){continue}
  $needs=if($step.'do' -eq 'open'){@()}else{@(@($step.tabs)+@($step.document)+@($(if($step.'do' -notin 'launch','resume','close'){$recent}))|Where-Object {$_})}
  if(!$checks.start -and $step.'do' -notin 'launch','open' -and $review -and !$review.HasExited){$checks.start=Check-Geometry}
  if($step.'do' -eq 'close'){$checks.end=try{Check-Geometry}catch{[ordered]@{ok=$false;error=$_.Exception.Message}};$memoryEnd=$sampler.Last}
  $last=if($entries.Count){$entries[$entries.Count-1]}
  $entries.Add((Run-Step $step $needs -Drain:($last -and $last.status -eq 'timeout')))
 }
}catch{$failure=($_|Out-String)+$_.ScriptStackTrace;Write-Host $failure}
finally{
 if($review -and !$review.HasExited){
  if(!$checks.end){$checks.end=try{Check-Geometry}catch{[ordered]@{ok=$false;error=$_.Exception.Message}};$memoryEnd=$sampler.Last}
  try{Close-App}catch{Write-Host "Close failed: $($_.Exception.Message)";if(!$review.WaitForExit(5000)){Stop-Process -Id $review.Id -Force;$framesNote='The app was force-stopped, so its latency trace was not written'}}
 }
 if($sampler){$sampler.Dispose()}
 if($presentMonProcess -and !$presentMonProcess.WaitForExit(30000)){Stop-Process -Id $presentMonProcess.Id -Force}
}
$frames=$null;$framesSource='dxgi'
if($presentMonProcess){try{$frames=@(PresentMon-Frames);$framesSource='presentmon'}catch{$framesNote="PresentMon frames unusable: $($_.Exception.Message)"}}
if($null -eq $frames -and $review){$frames=@(Canvas-Frames)}
$frames=@($frames|Where-Object {$_}|Sort-Object t_ns)
[IO.File]::WriteAllLines((Join-Path $passDirectory 'frames.jsonl'),[string[]]@($frames|ForEach-Object {'{"t_ns":'+$_.t_ns+',"surface":"'+$_.surface+'"}'}))
$canvasFrames=[long[]]@($frames|Where-Object {$_.surface -eq 'canvas'}|ForEach-Object {$_.t_ns})
$completions=@()
if($review){
 $samples=@{};foreach($row in Trace-Rows 'input'){$samples[$row.sequence]=[long]$row.sample_ns}
 $newest=@{};foreach($row in Trace-Rows 'consumed'){$t=$samples[$row.sequence];if($null -ne $t -and (!$newest.ContainsKey($row.frame) -or $newest[$row.frame] -lt $t)){$newest[$row.frame]=$t}}
 $completions=@(foreach($row in Trace-Rows 'frames'){if($newest.ContainsKey($row.frame)){[ordered]@{complete_ns=[long]$row.render_end_ns;input_ns=$newest[$row.frame]}}})
}
foreach($e in $entries){
 foreach($record in $e.latency){if($framesSource -eq 'presentmon' -and $record.surface -eq 'canvas'){$shown=Last-Frame $canvasFrames $record.input_ns $record.observed_ns;if($shown){$record.ready_ns=$shown;$record.evidence='presented'}}}
 if($e.contacts.Count -and $null -ne $e.extra.settle_observed_ns){
  $up=$e.contacts[-1].up_ns;$shown=Last-Frame $canvasFrames $up $e.extra.settle_observed_ns
  $e.settled_ns=if($shown){$shown}else{$e.extra.settle_observed_ns}
 }
 if($e.id -in $strokeIds -and $e.contacts.Count){
  $from=$e.contacts[0].down_ns;$to=if($e.settled_ns){$e.settled_ns}else{$e.end_ns}
  $e.completions=@($completions|Where-Object {$_.complete_ns -ge $from -and $_.complete_ns -le $to})
 }
}
$status=Join-Path $trace "latency-$(if($review){$review.Id})-$windowId-status.json"
$record=[ordered]@{
 schema=1;host='windows';device=$Device;version=$Version;commit=$null;app_id=$Executable;exe_sha256=(Get-FileHash -LiteralPath $Executable).Hash
 pass=$Pass;cold=[bool]$Cold;clock='qpc';refresh_hz=$refreshHz;canvas=$canvasSize;chain_sha256=(Get-FileHash -LiteralPath $Chain).Hash.ToLowerInvariant()
 frames_source=$framesSource;frames_evidence=$(if($framesSource -eq 'presentmon'){'presented'}else{'submitted'});frames_note=$framesNote;trace_overflow=$(if(Test-Path -LiteralPath $status){(Read-Snapshot $status).overflow});exit_code=$(if($review -and $review.HasExited){$review.ExitCode})
 display=$primary[0];surface=$surfaceInfo;checks=$checks;steps=$entries.ToArray()
 memory_end=$(if($memoryEnd){$memoryEnd|ConvertFrom-Json|Select-Object * -ExcludeProperty t_ns});memory_error=$(if($sampler){$sampler.Error})
 error=$failure
}
[IO.File]::WriteAllText((Join-Path $passDirectory 'pass.json'),($record|ConvertTo-Json -Depth 12),[Text.UTF8Encoding]::new($false))
Write-Host "Wrote $passDirectory"
if($failure){throw "Pass $Pass did not complete: $failure"}
