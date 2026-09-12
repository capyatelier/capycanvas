param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes,System.Drawing
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
 [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr window,ref Point point);
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
function Presentation {
 $workspace=Find 'Drawing workspace' -Name
 if($workspace){try{$workspace.Current.ItemStatus|ConvertFrom-Json}catch{}}
}
function Current-Group {
 $group=(Model).layout.groups|Where-Object {$_.panels -contains 'tool_settings'}
 if(!$group){return}
 # The full model deliberately retains the drag's initial placement.
 # Read the arranged native frame from the opt-in presentation diagnostics.
 $copy=$group|ConvertTo-Json -Depth 50 -Compress|ConvertFrom-Json
 $shown=(Presentation).groups|Where-Object id -eq $group.id
 if($shown){$copy.bounds=$shown.bounds}
 $copy
}
function Check-Motion($Before,$After,[uint32]$Group) {
 if($After.full_updates -ne $Before.full_updates -or $After.model_revision -ne $Before.model_revision){
  throw 'Steady drag refreshed retained UI models'
 }
 if($After.motion_updates -le $Before.motion_updates -or $After.revision -le $Before.revision){
  throw 'Steady drag did not apply incremental workspace presentation'
 }
 $position=$After.groups|Where-Object id -eq $Group
 $core=$After.workspace_update.drag.group
 if(!$position -or !$core -or $core.id -ne $Group){throw 'Missing shared/native floating placement'}
 foreach($field in @('x','y','width','height')){
  if([Math]::Abs($position.bounds.$field-$core.bounds.$field) -gt 1){throw "Native $field differs from the shared floating placement"}
 }
 $old=$Before.groups|Where-Object id -eq $Group
 $handles=@($Before.handles|Where-Object {$_.id -like "floating-$Group-*"})
 if(!$handles.Count){throw 'Fixture did not observe native floating resize grips'}
 foreach($handle in $handles){
  $moved=$After.handles|Where-Object id -eq $handle.id
  if(!$moved){throw 'Floating resize grip disappeared during motion'}
  foreach($axis in @('x','y')){
   if([Math]::Abs(($moved.bounds.$axis-$handle.bounds.$axis)-($position.bounds.$axis-$old.bounds.$axis)) -gt 1){
    throw 'Native resize grip did not move with the panel'
   }
  }
 }
}
function Capture([string]$Name){
 [CapyTabTouch]::Hold()
 & (Join-Path $PSScriptRoot 'inspect-window.ps1') -ProcessId $review.Id -Output (Join-Path $run ($Name+'.png')) -ClientOnly *> (Join-Path $run ($Name+'.json'))
 [CapyTabTouch]::Hold()
}
function Check-OverviewOverlap([string]$Before,[string]$After) {
 $overview=(Control 'navigator-overview').Current.BoundingRectangle
 $origin=[CapyTabTouch+Point]::new()
 if(![CapyTabTouch]::ClientToScreen($review.MainWindowHandle,[ref]$origin)){throw 'Cannot locate client capture'}
 $document=(Model).state.tabs[0]
 $fit=[Math]::Min(($overview.Width-8*$scale)/$document.width,($overview.Height-8*$scale)/$document.height)
 $width=$document.width*$fit;$height=$document.height*$fit
 $left=$overview.Left-$origin.x+($overview.Width-$width)/2
 $top=$overview.Top-$origin.y+($overview.Height-$height)/2
 $lower=((Model).layout.groups|Where-Object active -eq 'brushes').bounds
 $beforeImage=[Drawing.Bitmap]::new((Join-Path $run ($Before+'.png')))
 $afterImage=[Drawing.Bitmap]::new((Join-Path $run ($After+'.png')))
 try{
  $tested=0;$white=0;$point=$null
  # Sample an area: the legitimate camera outline can cross any one pixel.
  foreach($fx in @(.12,.25,.38)){foreach($fy in @(.18,.34)){
   $x=[int]($left+$width*$fx);$y=[int]($top+$height*$fy)
   if($x -le $lower.x*$scale -or $x -ge ($lower.x+$lower.width)*$scale -or
      $y -le ($lower.y+36)*$scale -or $y -ge ($lower.y+$lower.height)*$scale){continue}
   $old=$beforeImage.GetPixel($x,$y);$pixel=$afterImage.GetPixel($x,$y)
   if($old.R -gt 245 -and $old.G -gt 245 -and $old.B -gt 245){continue}
   $tested++
   if($pixel.R -ge 245 -and $pixel.G -ge 245 -and $pixel.B -ge 245){
    $white++;if(!$point){$point=@{x=$x;y=$y;before=$old.ToArgb()}}
   }
  }}
  if($tested -lt 3){throw 'Navigator motion fixture did not cover enough opaque Tool Set pixels'}
  if($white -lt [Math]::Ceiling($tested*.8)){throw 'GPU Navigator did not move above the lower native panel'}
  $point
 }finally{$beforeImage.Dispose();$afterImage.Dispose()}
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
 & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'window';Invoke 'undo_workspace'
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
 Start-Sleep -Milliseconds 800
 $floating=Current-Group;$floatingX=$floating.bounds.x
 $before=Presentation
 $retainedTab=(Control 'panel-tab-tool_settings').GetRuntimeId() -join ':'
 $retainedField=(Control 'Brush size' -Name).GetRuntimeId() -join ':'
 Walk $x $y ($x+45*$scale) $y
 Wait-Until {(Current-Group).floating -and (Current-Group).bounds.x -gt $floatingX+30} 'Floating panel stopped tracking the held contact after its source changed'
 $after=Presentation;Check-Motion $before $after $floating.id
 if(((Control 'panel-tab-tool_settings').GetRuntimeId() -join ':') -ne $retainedTab -or
    ((Control 'Brush size' -Name).GetRuntimeId() -join ':') -ne $retainedField){throw 'Steady drag replaced native controls'}
 [IO.File]::WriteAllText((Join-Path $run 'retained-motion.json'),(@{before=$before;after=$after}|ConvertTo-Json -Depth 60))
 [CapyTabTouch]::Up()
 Undo-Workspace
 $position=Start-Slide
 Walk $position.x $position.y $x $y
 Wait-Until {(Current-Group).floating -and $null -eq (Find-Preview)} 'Cancellation review did not tear off'
 Start-Sleep -Milliseconds 400
 [CapyTabTouch]::Cancel()
 Wait-Until {((Model).layout|ConvertTo-Json -Depth 80 -Compress) -eq $normal} 'Detached cancellation did not restore the workspace'
 if((Model).state.document_file.modified){throw 'Workspace dragging modified the drawing'}

 # A GPU overview must follow the same placement as its retained native controls.
 # Place it over an opaque Tool Set region so seeing white cannot be mistaken
 # for the ordinary drawing canvas behind the panel.
 (Control 'Drawing canvas' -Name).SetFocus()
 Capture 'navigator-before'
 $source=(Control 'panel-tab-navigator').Current.BoundingRectangle
 $startX=$source.Left+8*$scale;$startY=$source.Top+$source.Height/2
 $x=$canvas.Left+180*$scale;$y=$canvas.Top+130*$scale
 [CapyTabTouch]::Down([int]$startX,[int]$startY)
 Walk $startX $startY $x $y
 Wait-Until {@((Model).layout.groups|Where-Object {$_.active -eq 'navigator' -and $_.floating}).Count -eq 1} 'Navigator did not tear off'
 Start-Sleep -Milliseconds 600
 $navigator=(Model).layout.groups|Where-Object {$_.active -eq 'navigator' -and $_.floating}
 $before=Presentation
 $retained=(Control 'navigator-zoom_in').GetRuntimeId() -join ':'
 Capture 'navigator-first'
 $firstPixel=Check-OverviewOverlap 'navigator-before' 'navigator-first'
 $oldCamera=(Control 'canvas-camera').Current.Name
 Invoke 'navigator-zoom_in'
 Walk $x $y ($x+45*$scale) ($y+12*$scale)
 Wait-Until {(Control 'canvas-camera').Current.Name -ne $oldCamera} 'Mixed camera update was lost during workspace motion'
 Wait-Until {
  $p=Presentation;$g=$p.groups|Where-Object id -eq $navigator.id
  $old=$before.groups|Where-Object id -eq $navigator.id
  $g.bounds.x -gt $old.bounds.x+30
 } 'Native Navigator stopped moving'
 $after=Presentation
 [IO.File]::WriteAllText((Join-Path $run 'navigator-motion.json'),(@{before=$before;after=$after}|ConvertTo-Json -Depth 60))
 Check-Motion $before $after $navigator.id
 if(((Control 'navigator-zoom_in').GetRuntimeId() -join ':') -ne $retained){throw 'Navigator motion replaced native controls'}
 if($before.overviews.Count -ne 1 -or $after.overviews.Count -ne 1){throw 'Navigator fixture did not retain exactly one GPU overview'}
 $old=$before.groups|Where-Object id -eq $navigator.id;$moved=$after.groups|Where-Object id -eq $navigator.id
 if([Math]::Abs(($after.overviews[0].bounds[0]-$before.overviews[0].bounds[0])-($moved.bounds.x-$old.bounds.x)) -gt 1 -or
    [Math]::Abs(($after.overviews[0].bounds[1]-$before.overviews[0].bounds[1])-($moved.bounds.y-$old.bounds.y)) -gt 1){
  throw 'GPU overview allocation did not follow native Navigator placement'
 }
 Capture 'navigator-moved'
 $secondPixel=Check-OverviewOverlap 'navigator-before' 'navigator-moved'
 [CapyTabTouch]::Cancel()
 Wait-Until {((Model).layout|ConvertTo-Json -Depth 80 -Compress) -eq $normal} 'Navigator cancellation did not restore workspace layout'
 Start-Sleep -Milliseconds 200
 Capture 'navigator-restored'
 $restored=[Drawing.Bitmap]::new((Join-Path $run 'navigator-restored.png'))
 try{foreach($point in @($firstPixel,$secondPixel)){
  if($restored.GetPixel($point.x,$point.y).ToArgb() -ne $point.before){throw 'Navigator cancellation did not restore the lower native panel'}
 }}finally{$restored.Dispose()}
 if((Model).state.document_file.modified){throw 'Navigator workspace/camera motion modified the drawing'}
 & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved
 if((Get-Item -LiteralPath $stderr).Length){throw 'Native stderr requires inspection'}
 [pscustomobject]@{os_touch_routing='passed';attached_preview='passed';fixed_hit_rectangles='passed';release_insertion='passed';different_grab_position='passed';pointer_cancel='passed';tear_off='passed';held_floating_drag='passed';incremental_publication='passed';retained_native_controls='passed';resize_grip_placement='passed';navigator_gpu_motion='passed';fast_tearoff_capture='passed';mixed_camera='passed';overview_occlusion_and_restore='passed';detached_cancel='passed';workspace_undo='passed';zero_exit='passed';scope='OS-injected touch; physical input and latency acceptance remain separate'}|ConvertTo-Json
}catch{
 [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{
 [CapyTabTouch]::Cancel()
 foreach($name in $names){[Environment]::SetEnvironmentVariable($name,$previous[$name],'Process')}
}
