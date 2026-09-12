param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
# OS-delivered synthetic touch exercises XAML routing/capture. It does not
# establish physical digitizer behavior or input latency.
# https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-injecttouchinput
Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Threading;
public static class CapyTabTouch {
 [StructLayout(LayoutKind.Sequential)] public struct Point { public int x,y; }
 [StructLayout(LayoutKind.Sequential)] public struct Rect { public int left,top,right,bottom; }
 [StructLayout(LayoutKind.Sequential)] public struct PointerInfo {
  public uint type,id,frame,flags; public IntPtr device,target;
  public Point pixel,himetric,pixelRaw,himetricRaw;
  public uint time,history; public int data; public uint keys;
  public ulong performance; public uint change;
 }
 [StructLayout(LayoutKind.Sequential)] public struct TouchInfo {
  public PointerInfo pointer; public uint flags,mask;
  public Rect contact,contactRaw; public uint orientation,pressure;
 }
 [DllImport("user32.dll",SetLastError=true)] static extern bool InitializeTouchInjection(uint count,uint feedback);
 [DllImport("user32.dll",SetLastError=true)] static extern bool InjectTouchInput(uint count,TouchInfo[] contacts);
 [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr window,out uint process);
 [DllImport("user32.dll")] static extern IntPtr WindowFromPoint(Point point);
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr window);
 [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
 [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr window);
 static uint owner; static bool active; static Point last;
 static readonly object gate=new object(); static Timer pulse; static Exception failure;
 public static bool Active { get { lock(gate)return active; } }
 static void Check() { if(failure!=null)throw new Exception("Touch keepalive failed.",failure); }
 static void Pulse(object unused) {
  lock(gate){
   if(!active)return;
   try{Guard(last);Send(last,0x20006);}
   catch(Exception error){
    failure=error;
    try{Send(last,0x48000);}catch{}
    active=false;pulse.Change(Timeout.Infinite,Timeout.Infinite);
   }
  }
 }
 public static void Initialize(uint process) {
  owner=process;
  if(Marshal.SizeOf(typeof(TouchInfo))!=144)throw new Exception("Touch structure must use the x64 ABI.");
  if(!InitializeTouchInjection(1,3))throw new Win32Exception(Marshal.GetLastWin32Error());
  // A UIA query or window capture may block PowerShell beyond the 100 ms
  // contact timeout. Deliver held frames independently of those observations.
  pulse=new Timer(Pulse,null,Timeout.Infinite,Timeout.Infinite);
 }
 static void Guard(Point point) {
  uint process;GetWindowThreadProcessId(GetForegroundWindow(),out process);
  if(process!=owner)throw new Exception("Review does not own foreground input.");
  GetWindowThreadProcessId(WindowFromPoint(point),out process);
  if(process!=owner)throw new Exception("Touch point is outside the owned review.");
 }
 static void Send(Point point,uint flags) {
  var info=new TouchInfo {pointer=new PointerInfo {type=2,id=0,flags=flags,pixel=point},
   mask=7,contact=new Rect {left=point.x-2,top=point.y-2,right=point.x+2,bottom=point.y+2},
   orientation=90,pressure=512};
  for(int retry=0;retry<20;retry++){
   if(InjectTouchInput(1,new[]{info}))return;
   int error=Marshal.GetLastWin32Error();
   if(error!=21)throw new Win32Exception(error);
   Thread.Sleep(1);
  }
  throw new Exception("Windows did not accept the touch frame.");
 }
 public static void Down(int x,int y) {
  lock(gate){
   Check();if(active)throw new Exception("A review touch is already active.");
   var point=new Point{x=x,y=y};Guard(point);Send(point,0x10006);last=point;active=true;
   pulse.Change(25,25);
  }
 }
 public static void Move(int x,int y) {
  lock(gate){
   Check();if(!active)throw new Exception("No review touch is active.");
   var point=new Point{x=x,y=y};Guard(point);Send(point,0x20006);last=point;
  }
 }
 public static void Hold() { lock(gate){Check();if(active){Guard(last);Send(last,0x20006);}} }
 public static void Up() {
  lock(gate){
   Check();if(!active)return;Guard(last);Thread.Sleep(2);Send(last,0x40000);active=false;
   pulse.Change(Timeout.Infinite,Timeout.Infinite);
  }
 }
 public static void Cancel() {
  lock(gate){
   if(!active)return;
   // This ends only our existing injected contact, including after focus loss.
   Thread.Sleep(2);Send(last,0x48000);active=false;pulse.Change(Timeout.Infinite,Timeout.Infinite);
  }
 }
}
'@
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/tab-drag/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$names=@('CAPY_SETTINGS_DIRECTORY','CAPY_TRACE_UI','CAPY_SMOKE_TEST','CAPY_TEST_DISPLAY','CAPY_TEST_PRIMARY','CAPY_PRESENT_PROBE')
$previous=@{};foreach($name in $names){$previous[$name]=[Environment]::GetEnvironmentVariable($name,'Process')}
function Model {
 try{$s=Get-Content (Join-Path $directory 'ui-state.json') -Raw|ConvertFrom-Json;if($s.process_id -eq $review.Id -and $s.model.windows_isolated_settings){$script:lastModel=$s.model}}catch{}
 $script:lastModel
}
function Wait-Until([scriptblock]$Predicate,[string]$Message,[int]$Seconds=5){
 $watch=[Diagnostics.Stopwatch]::StartNew()
 do{
  if(& $Predicate){return}
  $review.Refresh();if($review.HasExited){throw 'Tab review exited unexpectedly'}
  [CapyTabTouch]::Hold();Start-Sleep -Milliseconds 35
 }while($watch.Elapsed.TotalSeconds -lt $Seconds)
 throw $Message
}
function Find([string]$Id,[switch]$Name){
 $property=if($Name){[System.Windows.Automation.AutomationElement]::NameProperty}else{[System.Windows.Automation.AutomationElement]::AutomationIdProperty}
 $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new($property,$Id))
}
function Control([string]$Id,[switch]$Name){
 $found=@{item=$null};Wait-Until {$found.item=Find $Id -Name:$Name;$null -ne $found.item} "Missing $Id";$found.item
}
function Invoke([string]$Id){(Control $Id).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()}
function Find-Preview {
 # Preview copies are excluded from the accessible control/content views.
 # Inspect only the workspace's direct raw children for this visual fixture.
 $workspace=Find 'Drawing workspace' -Name
 if(!$workspace){return}
 $walker=[System.Windows.Automation.TreeWalker]::RawViewWalker
 $child=$walker.GetFirstChild($workspace)
 while($child){
  if($child.Current.AutomationId -eq 'workspace-tab-preview'){return $child}
  $child=$walker.GetNextSibling($child)
 }
}
function Preview {
 $element=Find-Preview
 if($element){try{$element.Current.ItemStatus|ConvertFrom-Json}catch{}}
}
function Current-Group { (Model).layout.groups|Where-Object {$_.panels -contains 'tool_settings'} }
function Capture([string]$Name){
 [CapyTabTouch]::Hold()
 & (Join-Path $PSScriptRoot 'inspect-window.ps1') -ProcessId $review.Id -Output (Join-Path $run ($Name+'.png')) -ClientOnly *> (Join-Path $run ($Name+'.json'))
 [CapyTabTouch]::Hold()
}
function Walk([double]$FromX,[double]$FromY,[double]$ToX,[double]$ToY){
 for($i=1;$i -le 8;$i++){
  [CapyTabTouch]::Move([int]($FromX+($ToX-$FromX)*$i/8),[int]($FromY+($ToY-$FromY)*$i/8))
  Start-Sleep -Milliseconds 18
 }
}
function Start-Slide([double]$Grab=8){
 (Control 'Drawing canvas' -Name).SetFocus()
 $source=(Control 'panel-tab-tool_settings').Current.BoundingRectangle
 $neighbor=(Control 'panel-tab-sizes').Current.BoundingRectangle
 $startX=$source.Left+$Grab*$scale;$startY=$source.Top+$source.Height/2
 $finishX=$startX+$neighbor.Width/2+10*$scale
 [CapyTabTouch]::Down([int]$startX,[int]$startY)
 Start-Sleep -Milliseconds 20
 Walk $startX $startY $finishX $startY
 Wait-Until {$null -ne (Preview) -and (Preview).insertion -eq 2} 'Attached native tab preview did not cross the shared insertion threshold'
 $actual=(Control 'panel-tab-tool_settings').Current.BoundingRectangle
 if([Math]::Abs($actual.Left-$source.Left) -gt 1 -or [Math]::Abs($actual.Width-$source.Width) -gt 1){throw 'Tab preview moved the original hit rectangle'}
 if(((Current-Group).panels -join ',') -ne 'tool_settings,sizes'){throw 'Preview prematurely committed tab order'}
 [pscustomobject]@{x=$finishX;y=$startY}
}
function Undo-Workspace {
 Wait-Until {$null -eq (Find-Preview)} 'Tab overlay survived release'
 Wait-Until {@((Model).state.commands|Where-Object {$_.id -eq 'undo_workspace' -and $_.enabled}).Count -eq 1} 'Released drag did not become undoable'
 Start-Sleep -Milliseconds 200
 Invoke 'application-menu-window';Invoke 'undo_workspace'
 Wait-Until {((Current-Group).panels -join ',') -eq 'tool_settings,sizes' -and !(Current-Group).floating} 'Workspace Undo did not restore the source group'
}
try{
 foreach($name in $names){[Environment]::SetEnvironmentVariable($name,$null,'Process')}
 $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'profile'
 $env:CAPY_TRACE_UI='1';$env:CAPY_SMOKE_TEST='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
 $stderr=Join-Path $run 'stderr.log'
 $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
 $null=$review.Handle
 [IO.File]::WriteAllText((Join-Path $repo 'artifacts/windows/tab-drag-review.json'),(@{process_id=$review.Id;run=$run}|ConvertTo-Json))
 Write-Output "Owned tab review $($review.Id)"
 Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero -and (Model).brush_ready} 'Review did not start' 45
 [CapyTabTouch]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
 [CapyTabTouch]::SetForegroundWindow($review.MainWindowHandle)|Out-Null
 [CapyTabTouch]::Initialize([uint32]$review.Id)
 $scale=[CapyTabTouch]::GetDpiForWindow($review.MainWindowHandle)/96.
 $root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
 Start-Sleep -Milliseconds 350
 $normal=(Model).layout|ConvertTo-Json -Depth 80 -Compress
 $position=Start-Slide
 Capture 'attached'
 [CapyTabTouch]::Up()
 Wait-Until {((Current-Group).panels -join ',') -eq 'sizes,tool_settings'} 'Release did not commit the preview insertion'
 Undo-Workspace
 $position=Start-Slide 35
 Capture 'second-grab'
 [CapyTabTouch]::Cancel()
 Wait-Until {$null -eq (Find-Preview)} 'Cancelled pointer retained the overlay'
 if(((Model).layout|ConvertTo-Json -Depth 80 -Compress) -ne $normal){throw 'Cancellation changed the workspace'}
 $position=Start-Slide
 $canvas=(Control 'Drawing canvas' -Name).Current.BoundingRectangle
 $x=$canvas.Left+$canvas.Width*.55;$y=$canvas.Top+$canvas.Height*.6
 Walk $position.x $position.y $x $y
 Wait-Until {(Current-Group).floating -and $null -eq (Find-Preview)} 'Tear-off did not release the attached preview'
 Capture 'detached'
 $floatingX=(Current-Group).bounds.x
 Start-Sleep -Milliseconds 800
 Walk $x $y ($x+45*$scale) $y
 Wait-Until {(Current-Group).floating -and (Current-Group).bounds.x -gt $floatingX+30} 'Floating panel stopped tracking the held contact after its source changed'
 [CapyTabTouch]::Up()
 Undo-Workspace
 $position=Start-Slide
 Walk $position.x $position.y $x $y
 Wait-Until {(Current-Group).floating -and $null -eq (Find-Preview)} 'Cancellation review did not tear off'
 Start-Sleep -Milliseconds 400
 [CapyTabTouch]::Cancel()
 Wait-Until {((Model).layout|ConvertTo-Json -Depth 80 -Compress) -eq $normal} 'Detached cancellation did not restore the workspace'
 if((Model).state.document_file.modified){throw 'Workspace dragging modified the drawing'}
 & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved
 if((Get-Item -LiteralPath $stderr).Length){throw 'Native stderr requires inspection'}
 [pscustomobject]@{os_touch_routing='passed';attached_preview='passed';fixed_hit_rectangles='passed';release_insertion='passed';different_grab_position='passed';pointer_cancel='passed';tear_off='passed';held_floating_drag='passed';detached_cancel='passed';workspace_undo='passed';zero_exit='passed';scope='OS-injected touch; physical input and latency acceptance remain separate'}|ConvertTo-Json
}catch{
 [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{
 [CapyTabTouch]::Cancel()
 foreach($name in $names){[Environment]::SetEnvironmentVariable($name,$previous[$name],'Process')}
}
