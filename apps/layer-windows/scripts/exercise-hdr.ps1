param([Parameter(Mandatory)][string]$Executable,[ValidateSet("F16","F32")][string]$Depth="F16",[switch]$DrawingTabsOnly,[switch]$ControlsOnly,[switch]$ArcContextOnly,[ValidateSet('dark','light')][string]$Theme='dark',[ValidateSet('full','tab-drop-dark','tab-drop-light','screen-parity','screen-parity-light')][string]$Journey='full')
$ErrorActionPreference='Stop'
$screenParity=$Journey -in @('screen-parity','screen-parity-light')
if($screenParity){$ControlsOnly=$true;if($Journey -eq 'screen-parity-light'){$Theme='light'}}
$tabDropOnly=$Journey.StartsWith('tab-drop-')
if($tabDropOnly){$DrawingTabsOnly=$true;$Theme=$Journey -replace '^tab-drop-',''}
if($ArcContextOnly){$ControlsOnly=$true}
if($ControlsOnly -and !$screenParity -and !$PSBoundParameters.ContainsKey('Theme')){
 foreach($theme in @('dark','light')){& $PSCommandPath -Executable $Executable -Depth $Depth -ControlsOnly -ArcContextOnly:$ArcContextOnly -Theme $theme}
 return
}
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
. (Join-Path $PSScriptRoot 'WindowsArchitecture.ps1')
$capyControl=${function:Control}
$CapyWaitSeconds=15;$CapyFind='visible';$CapyPopups=$true
Add-Type -AssemblyName System.Drawing
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
if($env:CAPY_PRIVATE_DESKTOP){[CapyRowPointer]::VerifyPrivateDesktop($env:CAPY_PRIVATE_DESKTOP)}
$CapyEach={if($env:CAPY_PRIVATE_DESKTOP){[CapyRowPointer]::VerifyPrivateDesktop($env:CAPY_PRIVATE_DESKTOP)};[CapyRowPointer]::Verify()}
$hdrNativeSource=@'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class CapyHdrPicker {
 public static int Main(string[] args) {try {RequireArchitecture(args[2]);Console.Write(Arc(args[0],System.IO.File.ReadAllText(args[1])));return 0;}catch(Exception error){Console.Error.Write(error);return 1;}}
 static void RequireArchitecture(string architecture) {
  int expected=architecture=="arm64"?0xAA64:architecture=="x64"?0x8664:0;
  foreach(System.Diagnostics.ProcessModule module in System.Diagnostics.Process.GetCurrentProcess().Modules) {
   if(!string.Equals(module.ModuleName,"clr.dll",StringComparison.OrdinalIgnoreCase))continue;
   using(var reader=new System.IO.BinaryReader(System.IO.File.OpenRead(module.FileName))) {
    reader.BaseStream.Position=0x3C;int pe=reader.ReadInt32();reader.BaseStream.Position=pe+4;int actual=reader.ReadUInt16();
    if(expected==0||actual!=expected)throw new Exception("HDR geometry helper expected "+architecture+" but CLR machine is 0x"+actual.ToString("X4"));
    return;
   }
  }
  throw new Exception("HDR geometry helper CLR architecture is unavailable");
 }
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern IntPtr LoadLibraryEx(string path,IntPtr file,uint flags);
 [DllImport("kernel32.dll")] static extern IntPtr GetProcAddress(IntPtr module,string name);
 [DllImport("kernel32.dll")] static extern bool FreeLibrary(IntPtr module);
 [UnmanagedFunctionPointer(CallingConvention.Cdecl)] delegate ulong Localize([MarshalAs(UnmanagedType.LPUTF8Str)]string tag);
 [UnmanagedFunctionPointer(CallingConvention.Cdecl)] delegate ulong ColorUi(ulong context,[MarshalAs(UnmanagedType.LPUTF8Str)]string json);
 [UnmanagedFunctionPointer(CallingConvention.Cdecl)] delegate void Free(ulong value);
 [UnmanagedFunctionPointer(CallingConvention.Cdecl)] delegate UIntPtr Length(ulong value);
 [UnmanagedFunctionPointer(CallingConvention.Cdecl)] [return:MarshalAs(UnmanagedType.I1)] delegate bool Copy(ulong value,UIntPtr offset,IntPtr output,UIntPtr length);
 static T Export<T>(IntPtr module,string name) {var address=GetProcAddress(module,name);if(address==IntPtr.Zero)throw new Exception("Missing "+name);return (T)(object)Marshal.GetDelegateForFunctionPointer(address,typeof(T));}
 public static string Arc(string path,string json) {
  IntPtr module=LoadLibraryEx(path,IntPtr.Zero,0x1100);ulong context=0,value=0;
  if(module==IntPtr.Zero)throw new Exception("Cannot load color geometry DLL: "+Marshal.GetLastWin32Error());
  Free freeContext=null,freeString=null;
  try {
   freeContext=Export<Free>(module,"capy_localization_free");freeString=Export<Free>(module,"capy_string_free");
   context=Export<Localize>(module,"capy_localization_for_tag")("en");if(context==0)throw new Exception("Cannot create color localization");
   value=Export<ColorUi>(module,"capy_color_ui")(context,json);if(value==0)throw new Exception("Missing color geometry");
   var bytes=new byte[checked((int)Export<Length>(module,"capy_string_length")(value).ToUInt64())];
   var copy=Export<Copy>(module,"capy_string_copy");var pinned=GCHandle.Alloc(bytes,GCHandleType.Pinned);
   try {
    for(int offset=0;offset<bytes.Length;){
     int count=Math.Min(8*1024*1024,bytes.Length-offset);
     if(!copy(value,new UIntPtr((uint)offset),IntPtr.Add(pinned.AddrOfPinnedObject(),offset),new UIntPtr((uint)count)))throw new Exception("Color geometry text copy failed");
     offset+=count;
    }
    if(!copy(value,new UIntPtr((uint)bytes.Length),IntPtr.Zero,UIntPtr.Zero))throw new Exception("Color geometry text expired");
   }finally{pinned.Free();}
   return Encoding.UTF8.GetString(bytes);
  } finally {if(value!=0)freeString(value);if(context!=0)freeContext(context);FreeLibrary(module);}
 }
 [DllImport("user32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern IntPtr SendMessageTimeout(IntPtr h,uint m,UIntPtr w,IntPtr l,uint f,uint t,out UIntPtr r);
 [DllImport("user32.dll",SetLastError=true)] public static extern bool PostMessage(IntPtr h,uint m,UIntPtr w,IntPtr l);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint p);
 [DllImport("user32.dll")] public static extern uint GetDoubleClickTime();
 public struct Rect { public int Left,Top,Right,Bottom; }
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out Rect r);
 public static void Type(IntPtr h,string text) {
  UIntPtr r;
  if(SendMessageTimeout(h,0xB1,UIntPtr.Zero,new IntPtr(-1),2,2000,out r)==IntPtr.Zero)throw new Exception("Cannot select picker text");
  if(SendMessageTimeout(h,0x303,UIntPtr.Zero,IntPtr.Zero,2,2000,out r)==IntPtr.Zero)throw new Exception("Cannot clear picker text");
  foreach(char c in text)if(SendMessageTimeout(h,0x102,new UIntPtr(c),IntPtr.Zero,2,2000,out r)==IntPtr.Zero)throw new Exception("Cannot type picker path");
 }
}
'@
Add-Type -TypeDefinition $hdrNativeSource
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$run=Join-Path $repo ('artifacts/windows/hdr-ui/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory((Join-Path $run 'profile'))|Out-Null
$directory=$run;$CapyTraceDirectory=$run
function New-ArcHelper([string]$Directory,[string]$Dll){
 $architecture=if(Test-WindowsBinaryArchitecture $Dll arm64){'arm64'}elseif(Test-WindowsBinaryArchitecture $Dll x64){'x64'}else{throw 'HDR geometry DLL has an unsupported architecture'}
 $build=Resolve-WindowsArchitecture $architecture
 if(!(Test-WindowsBinaryArchitecture $Executable $build.architecture)){throw 'HDR geometry DLL and reviewed executable have different architectures'}
 $source=Join-Path $Directory 'hdr-geometry.cs'
 [IO.File]::WriteAllText($source,$hdrNativeSource)
 if($build.architecture -eq 'arm64'){
  $launcher=Join-Path $env:WINDIR 'System32/WindowsPowerShell/v1.0/powershell.exe'
  if(!(Test-WindowsBinaryArchitecture $launcher arm64)){throw 'A native ARM64 Windows PowerShell host is required for HDR geometry checks'}
  $output=Join-Path $Directory 'hdr-geometry.ps1'
  [IO.File]::WriteAllText($output,@'
param([string]$Dll,[string]$InputPath,[string]$Architecture)
$ErrorActionPreference='Stop'
Add-Type -Path (Join-Path $PSScriptRoot 'hdr-geometry.cs')
exit ([CapyHdrPicker]::Main(@($Dll,$InputPath,$Architecture)))
'@)
  $arguments=@('-NoLogo','-NoProfile','-NonInteractive','-ExecutionPolicy','RemoteSigned','-File',('"'+$output+'"'))
 }else{
  $compiler=Join-Path $env:WINDIR 'Microsoft.NET/Framework64/v4.0.30319/csc.exe'
  $output=Join-Path $Directory 'hdr-geometry.exe'
  & $compiler /nologo /target:exe /platform:x64 "/out:$output" $source
  if($LASTEXITCODE -ne 0){throw 'HDR geometry helper compilation failed'}
  if(!(Test-WindowsBinaryArchitecture $output x64)){throw 'HDR geometry helper is not x64'}
  $launcher=$output;$arguments=@()
 }
 $helper=@{path=$launcher;arguments=$arguments;architecture=$build.architecture;source=$output;sha256=(Get-FileHash -LiteralPath $output).Hash;dll_sha256=(Get-FileHash -LiteralPath $Dll).Hash}
 $helper|ConvertTo-Json|Set-Content -LiteralPath (Join-Path $Directory 'hdr-geometry-build.json')
 $helper
}
function Arc-Geometry([string]$Dll,[string]$Json){
 if(!$script:arcHelper){$script:arcHelper=New-ArcHelper $run $Dll}
 $call=Join-Path $run ('geometry-'+[Guid]::NewGuid().ToString('N'));$inputPath=$call+'.json'
 [IO.File]::WriteAllText($inputPath,$Json)
 $arguments=@($script:arcHelper.arguments)+@( ('"'+$Dll+'"'),('"'+$inputPath+'"'),$script:arcHelper.architecture )
 $process=Start-Process -FilePath $script:arcHelper.path -ArgumentList $arguments -WindowStyle Hidden -PassThru -RedirectStandardOutput ($call+'.out.json') -RedirectStandardError ($call+'.stderr')
 $null=$process.Handle
 try{
  if(!$process.WaitForExit(10000)){$process.Kill();$process.WaitForExit();throw 'Owned HDR geometry helper timed out'}
  if($process.ExitCode -ne 0 -or (Get-Item ($call+'.stderr')).Length){throw ('HDR geometry helper failed: '+[IO.File]::ReadAllText($call+'.stderr'))}
  [IO.File]::ReadAllText($call+'.out.json')|ConvertFrom-Json
 }finally{
  @{process_id=$process.Id;exited=$process.HasExited;exit_code=$(if($process.HasExited){$process.ExitCode}else{$null});helper=$script:arcHelper;dll=$Dll}|ConvertTo-Json|Set-Content ($call+'.process.json')
  $process.Dispose()
 }
}
function Element-Evidence($Element){
 if(!$Element){return $null}
 try{$c=$Element.Current;@{id=$c.AutomationId;name=$c.Name;pid=$c.ProcessId;runtime_id=@($Element.GetRuntimeId());bounds=$c.BoundingRectangle.ToString();offscreen=$c.IsOffscreen;enabled=$c.IsEnabled;type=$c.ControlType.ProgrammaticName}}catch{@{error=$_.Exception.Message}}
}
function NativeTabStatus {
 $state=@{at=[DateTime]::UtcNow.ToString('o')}
 try{$state.active=[CapyRowPointer]::Active}catch{$state.active_error=$_.Exception.Message}
 try{$state.tab_status=$tabStrip.Current.ItemStatus}catch{$state.tab_error=$_.Exception.Message}
 try{$state.tab_gesture=$tabStrip.Current.HelpText}catch{$state.tab_gesture_error=$_.Exception.Message}
 try{$header=Find 'title-bar';if(!$header){throw 'Title bar is missing'};$state.header_gesture=$header.Current.HelpText;$state.header=$state.header_gesture|ConvertFrom-Json}catch{$state.header_error=$_.Exception.Message}
 try{$state.foreground=[CapyRowPointer]::GetForegroundWindow().ToInt64();$owner=[uint32]0;[CapyHdrPicker]::GetWindowThreadProcessId([IntPtr]$state.foreground,[ref]$owner)|Out-Null;$state.foreground_pid=$owner}catch{$state.foreground_error=$_.Exception.Message}
 try{$state.focus=Element-Evidence ([System.Windows.Automation.AutomationElement]::FocusedElement)}catch{$state.focus_error=$_.Exception.Message}
 $state
}
function Capture-Failure($Failure){
 if($script:failureCaptured){return};$script:failureCaptured=$true
 try{[IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($Failure|Out-String)+$Failure.ScriptStackTrace)}catch{}
 try{NativeTabStatus|ConvertTo-Json -Depth 8|Set-Content (Join-Path $run 'failure-native-before-dispose.json')}catch{}
 try{Model|ConvertTo-Json -Depth 90|Set-Content (Join-Path $run 'failure-model-before-close.json')}catch{}
 try{
  if($env:CAPY_PRIVATE_DESKTOP){[CapyRowPointer]::VerifyPrivateDesktop($env:CAPY_PRIVATE_DESKTOP)}
  $owner=[uint32]0;[CapyHdrPicker]::GetWindowThreadProcessId($drawingWindow,[ref]$owner)|Out-Null
  if($review -and !$review.HasExited -and $owner -eq $review.Id){
   $rect=New-Object CapyHdrPicker+Rect
   if([CapyHdrPicker]::GetWindowRect($drawingWindow,[ref]$rect)){
    $bitmap=[Drawing.Bitmap]::new($rect.Right-$rect.Left,$rect.Bottom-$rect.Top);$graphics=[Drawing.Graphics]::FromImage($bitmap)
    try{$graphics.CopyFromScreen($rect.Left,$rect.Top,0,0,$bitmap.Size);$bitmap.Save((Join-Path $run 'failure.png'))}finally{$graphics.Dispose();$bitmap.Dispose()}
   }
  }
 }catch{try{$_.Exception.Message|Set-Content (Join-Path $run 'failure-image-error.txt')}catch{}}
 try{
  $uia=[System.Windows.Automation.AutomationElement];$scope=[System.Windows.Automation.TreeScope]
  $condition=[System.Windows.Automation.OrCondition]::new([System.Windows.Automation.PropertyCondition]::new($uia::AutomationIdProperty,'drawing-order-undo'),[System.Windows.Automation.PropertyCondition]::new($uia::NameProperty,'Undo tab order'))
  $owned=[System.Collections.Generic.List[object]]::new();$owned.Add($root)
  $windows=$uia::RootElement.FindAll($scope::Children,[System.Windows.Automation.PropertyCondition]::new($uia::ProcessIdProperty,[int]$review.Id))
  foreach($window in $windows){$owned.Add($window)}
  $candidates=@(foreach($window in $owned){foreach($item in $window.FindAll($scope::Descendants,$condition)){Element-Evidence $item}})
  @{at=[DateTime]::UtcNow.ToString('o');state=(NativeTabStatus);candidates=$candidates}|ConvertTo-Json -Depth 10|Set-Content (Join-Path $run 'failure-menu-candidates.json')
 }catch{try{$_.Exception.Message|Set-Content (Join-Path $run 'failure-menu-error.txt')}catch{}}
}
function Assert-CanvasInk([string]$Label){
 $capture=Join-Path $run ($Label+'-canvas.png')
 & (Join-Path $PSScriptRoot 'inspect-window.ps1') -ProcessId $review.Id -Output $capture -ClientOnly -Composed *> (Join-Path $run ($Label+'-canvas.json'))
 $bitmap=[Drawing.Bitmap]::new($capture)
 try {
  # This fixture uses an 1800x1300 window; these bounds lie inside the canvas,
  # away from panels, title controls, overlays and the pointer. Orange HDR ink
  # must survive presentation as well as the independently checked file exports.
  $ink=0
  for($y=[int]($bitmap.Height*.2);$y -lt [int]($bitmap.Height*.8);$y+=2){
   for($x=[int]($bitmap.Width*.28);$x -lt [int]($bitmap.Width*.7);$x+=2){
    $p=$bitmap.GetPixel($x,$y)
    if($p.R -gt $p.G+12 -and $p.G -gt $p.B+8){$ink++}
   }
  }
  if($ink -lt 20){throw "Visible HDR ink is missing after $Label ($ink samples)"}
 }finally{$bitmap.Dispose()}
}
function Control([string]$Value,[switch]$Name,$Type,[switch]$Arranged,$Within=$root){
 if($Arranged){return & $capyControl $Value -Name:$Name -Type $Type -Within $Within -Arranged}
 $hit=@{item=$null};Wait-Until {
  $hit.item=Find $Value -Name:$Name -Type $Type -Within $Within
  if(!$hit.item -and !$Name -and $Value.StartsWith('proof-')){
   $entry=$root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,$Value))
   if($entry){$scroll=$null;if($entry.TryGetCurrentPattern([System.Windows.Automation.ScrollItemPattern]::Pattern,[ref]$scroll)){$scroll.ScrollIntoView()}else{$entry.SetFocus()}}
  }
  $null -ne $hit.item
 } "Missing HDR control: $Value";$hit.item
}
function Compact-Drawings {
 [CapyWindowApi]::ShowWindow($drawingWindow,9)|Out-Null
 $scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
 if($scale -le 0 -or [double]::IsNaN($scale) -or [double]::IsInfinity($scale)){throw 'Compact drawing layout DPI is unavailable'}
 Add-Type -AssemblyName System.Windows.Forms
 $area=[Windows.Forms.Screen]::FromHandle($drawingWindow).WorkingArea
 $height=[int][Math]::Min(800*$scale,$area.Height-40)
 $null=& (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -StateDirectory $run -Action Resize -Width ([int](650*$scale)) -Height $height
 Control 'drawing-selector' -Arranged
}
function Open-Drawings([switch]$Keyboard) {
 $selector=Control 'drawing-selector' -Arranged
 Wait-Until {$selector.Current.ItemStatus -eq 'Closed'} 'Previous drawing popup is still closing'
 if($Keyboard){[CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11,0x10),0x41)}else{Invoke 'drawing-selector'}
 Wait-Until {$selector.Current.ItemStatus -eq 'Open' -and (Find 'drawing-list')} 'Drawing selector did not open'
 $list=Control 'drawing-list' -Arranged
 $settled=@{bounds=''}
 Wait-Until {$now=[string]$list.Current.BoundingRectangle;$same=$now -eq $settled.bounds;$settled.bounds=$now;Start-Sleep -Milliseconds 80;$same} 'Drawing selector did not settle'
}
function Outside-Tap([string]$Id,[string]$Device,[switch]$List,[switch]$NearEdge){
 $parent=if($List){Control 'drawing-list' -Arranged}else{$tabStrip}
 $target=Control $Id -Within $parent -Arranged;$runtimeId=@($target.GetRuntimeId());$bounds=$target.Current.BoundingRectangle;$hostBounds=$root.Current.BoundingRectangle
 $region=$parent.Current.BoundingRectangle
 $x=[int]($bounds.X+$bounds.Width/2);$y=[int]($bounds.Y+$bounds.Height/2)
 $outsideX=if($List){[int]($region.Left-12)}else{$x};$outsideY=if($List){$y}else{[int]($region.Bottom+24)}
 if($NearEdge){
  if($List -or [Math]::Abs($bounds.Left-[Math]::Round($bounds.Left)) -gt .001){throw 'Invalid run: near-edge tab geometry is unqualified'}
  $x=[int][Math]::Round($bounds.Left)+2;$outsideX=$x-4;$outsideY=$y
 }
 if(!$hostBounds.Contains($outsideX,$outsideY) -or $(if($NearEdge){!$hostBounds.Contains($x,$y) -or !$bounds.Contains($x,$y) -or $bounds.Contains($outsideX,$outsideY)}else{$region.Contains($outsideX,$outsideY)})){throw 'Invalid run: outside-release point is not in the owned surround'}
 $selected=(Model).windows_tabs.selected;$order=(Model).windows_tabs.tabs.id -join ','
 $hits=@()
 if($NearEdge){
  $hits=@(foreach($point in @(@($x,$y),@($outsideX,$outsideY))){
   $native=New-Object CapyRowPointer+Point;$native.x=$point[0];$native.y=$point[1]
   $handle=[CapyRowPointer]::WindowFromPoint($native);$owner=[uint32]0;[CapyRowPointer]::GetWindowThreadProcessId($handle,[ref]$owner)|Out-Null
   @{point=$point;hwnd=$handle.ToInt64();process=$owner;uia=@(Control-Trace ([System.Windows.Automation.AutomationElement]::FromPoint([System.Windows.Point]::new($point[0],$point[1]))))}
  })
 }
 $path=Join-Path $run ($Device+'-'+$(if($NearEdge){'tab-near-edge'}elseif($List){'list'}else{'tab'})+'-outside.json')
 $evidence=@{device=$Device;id=$Id;runtime_id=$runtimeId;bounds=$bounds.ToString();region=$region.ToString();from=@($x,$y);to=@($outsideX,$outsideY);expected_selected=$selected;expected_order=$order;near_edge=[bool]$NearEdge;dpi=[CapyRowPointer]::GetDpiForWindow($drawingWindow);foreground=[CapyRowPointer]::GetForegroundWindow().ToInt64();owned_window=$drawingWindow.ToInt64();hits=$hits;observations=@()}
 $evidence|ConvertTo-Json -Depth 14|Set-Content $path
 if($NearEdge){
  if($Id -ne 'drawing-tab-2' -or $selected -ne 3 -or $order -ne '1,2,3' -or @((Model).windows_tabs.tabs|Where-Object modified).Count -or $hits[0].process -ne $review.Id -or $hits[1].process -ne $review.Id -or $hits[0].hwnd -ne $hits[1].hwnd -or $evidence.foreground -ne $evidence.owned_window){throw 'Invalid run: near-edge contact lacks the owned clean middle-tab baseline'}
  if(!$target.Current.BoundingRectangle.Equals($bounds) -or (($target.GetRuntimeId() -join ':') -ne ($runtimeId -join ':'))){throw 'Invalid run: near-edge target geometry or identity changed'}
 }
 [CapyRowPointer]::Down($Device,$x,$y);Start-Sleep -Milliseconds 35
 [CapyRowPointer]::Move($outsideX,$outsideY);[CapyRowPointer]::Up($Device -eq 'pen')
 $evidence.contact_ms=[CapyRowPointer]::ContactDurationMilliseconds;$evidence.max_gap_ms=[CapyRowPointer]::MaxGapMilliseconds
 $evidence|ConvertTo-Json -Depth 14|Set-Content $path
 if($NearEdge -and $evidence.contact_ms -ge 150){throw 'Invalid run: near-edge contact exceeded the short-tap bound'}
 if($NearEdge){
  $immediate=Model;$evidence.immediate=@{selected=$immediate.windows_tabs.selected;order=($immediate.windows_tabs.tabs.id -join ',');tab_status=$tabStrip.Current.ItemStatus;menu=[bool](Find 'drawing-order-undo');pointer_active=[CapyRowPointer]::Active};$evidence.observation_window_ms=1500
  $ack=[Diagnostics.Stopwatch]::StartNew()
  try{Wait-Until {$tabStrip.Current.ItemStatus -eq 'Ready'} 'Near-edge drawing tab did not acknowledge native contact completion'}
  finally{$ack.Stop();$evidence.native_ack_wait_ms=$ack.Elapsed.TotalMilliseconds;$evidence|ConvertTo-Json -Depth 14|Set-Content $path}
 }
 if(!$NearEdge){Start-Sleep -Milliseconds 200}
 $watch=[Diagnostics.Stopwatch]::StartNew()
 do {
  $listState=Find 'drawing-list';$gesture=if($listState){$listState.Current.ItemStatus}else{$null};$after=Model
  $status=(Control 'drawing-tabs').Current.ItemStatus;$menu=[bool](Find 'drawing-order-undo')
  $evidence.actual_selected=$after.windows_tabs.selected;$evidence.actual_order=$after.windows_tabs.tabs.id -join ',';$evidence.tab_status=$status;$evidence.list_gesture=$gesture
  if($NearEdge){$evidence.observations+=@(@{elapsed_ms=$watch.Elapsed.TotalMilliseconds;selected=$evidence.actual_selected;order=$evidence.actual_order;tab_status=$status;menu=$menu;drawing_list=[bool]$listState;pointer_active=[CapyRowPointer]::Active})}
  $evidence|ConvertTo-Json -Depth 14|Set-Content $path
  if($after.windows_tabs.selected -ne $selected -or ($after.windows_tabs.tabs.id -join ',') -ne $order){throw 'Direct outside release changed the active drawing or order'}
  if($status -ne 'Ready' -or $menu -or ($gesture -and ($gesture|ConvertFrom-Json).phase -ne 'idle') -or ($NearEdge -and ($listState -or [CapyRowPointer]::Active))){throw 'Direct outside release retained a menu or capture'}
  if($NearEdge){Start-Sleep -Milliseconds 50}
 }while($NearEdge -and $watch.Elapsed.TotalMilliseconds -lt 1500)
 if($listState){[CapyRowPointer]::Key(0x1b);Wait-Until {!(Find 'drawing-list')} 'Drawing list did not dismiss after outside release'}
}
function Invoke([string]$Value,[switch]$Name){
 $item=Control $Value -Name:$Name
 Wait-Until {$item.Current.IsEnabled} "Control disabled: $Value"
 $pattern=$null
 if($item.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern,[ref]$pattern)){$pattern.Invoke()}
 else{$item.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()}
}
function Button([string]$Name){
 (Control $Name -Name -Type ([System.Windows.Automation.ControlType]::Button)).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
}
function Command([string]$Id,[string]$Menu='View'){
 Wait-Until {((Model).state.commands|Where-Object id -eq $Id).enabled} "Command disabled: $Id" 45
 & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name $Menu
 Invoke $Id
}
function Select-Choice([string]$Id,[string]$Name){
 $box=Control $Id
 $box.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
 $item=Control $Name -Name -Type ([System.Windows.Automation.ControlType]::ListItem)
 $item.GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
}
function Idle {Wait-Until {$m=Model;$m -and !$m.windows_document -and !$m.state.document_file.busy -and !@($m.state.requests).Count} 'HDR/document operation did not finish' 60}
function Proof-Reveal([string]$Id){
 $groups=@((Model).layout.groups|Where-Object {$_.active -eq 'proof'})
 if($groups.Count -ne 1){throw 'Proof panel has no single live scroll viewport'}
 Reveal $Id -ViewportId ('panel-body-scroll-'+$groups[0].id)
}
function Sdr {
 ProofPanel
 if((Model).windows_proof_form.mode -ne 'sdr'){
  Invoke 'proof-panel-mode-sdr'
  Wait-Until {(Model).windows_proof_form.mode -eq 'sdr'} 'Proof panel did not enter SDR mode'
 }
 $proofGroup=@((Model).layout.groups|Where-Object active -eq 'proof')[0]
 $nativeProof=Control ('workspace-group-'+$proofGroup.id) -Arranged
 $null=Control 'proof-dial' -Within $nativeProof -Arranged
 $null=Proof-Reveal 'proof-panel-exposure';$null=Proof-Reveal 'proof-panel-contrast'
}
function ProofPanel {
 Expand-PanelColumn 'proof'
 if(@((Model).layout.groups|Where-Object {$_.active -eq 'proof'}).Count -eq 1){return}
 $entry=@{id=$null}
 Wait-Until {
  if(Find 'proof-panel-mode' -Visible){return $true}
  foreach($id in @('panel-tab-proof','drawer-tab-proof','column-icon-proof')){
   if(Find $id -Visible){$entry.id=$id;return $true}
  }
  $false
 } 'Retained Proof panel entry did not become visible'
 if($entry.id){Invoke $entry.id}
 Wait-Until {Find 'proof-panel-mode' -Visible} 'Retained Proof panel did not become visible'
}
function Setup {
 Command 'document_properties' 'File'
 Wait-Until {(Model).windows_document.kind -eq 'properties'} 'Document Properties did not open'
 $appearance=Control 'PrimaryButton' -Within (Control 'document-workflow') -Arranged
 if($appearance.Current.Name -ne (Model).windows_document.details.proof){throw 'Properties primary action is not Appearance'}
 $appearance.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
 Wait-Until {(Model).windows_document.kind -eq 'proof' -and (Find 'proof-profile')} 'Proof Setup did not open'
}
function Picker([string]$Name){
 $script:picker=Control $Name -Name -Type ([System.Windows.Automation.ControlType]::Window)
 if($picker.Current.ClassName -ne '#32770' -or $picker.Current.ProcessId -ne $review.Id){throw 'Picker is outside the owned HDR review'}
}
function Picker-Button([string]$Id){
 $item=$picker.FindFirst([System.Windows.Automation.TreeScope]::Children,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,$Id))
 $handle=[IntPtr]$item.Current.NativeWindowHandle;$owner=[uint32]0
 [CapyHdrPicker]::GetWindowThreadProcessId($handle,[ref]$owner)|Out-Null
 if($owner -ne $review.Id){throw 'Picker button ownership changed'}
 if(![CapyHdrPicker]::PostMessage($handle,245,[UIntPtr]::Zero,[IntPtr]::Zero)){throw 'Picker button failed'}
}
function Path-In-Picker([string]$Path){
 if(!(Split-Path -Parent $Path).Equals($run,[StringComparison]::OrdinalIgnoreCase)){throw 'HDR files must stay in the test directory'}
 $entry=$null
 foreach($id in @('1001','1148')){
  $entry=$picker.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.AndCondition]::new(
   [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,$id),
   [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ClassNameProperty,'Edit')))
  if($entry){break}
 }
 if(!$entry -or $entry.Current.ProcessId -ne $review.Id){throw 'Picker filename ownership changed'}
 [CapyHdrPicker]::Type([IntPtr]$entry.Current.NativeWindowHandle,$Path);Picker-Button '1'
}
function Set-Text([string]$Id,[string]$Text){
 $control=Control $Id
 $pattern=$null
 if($control.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern,[ref]$pattern)){$pattern.SetValue($Text);return}
 $entry=$control.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::Edit))
 if(!$entry -or $entry.Current.ProcessId -ne $review.Id){throw "No owned numeric editor: $Id"}
 $entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($Text)
}
function Delivery([string]$Name,[string]$Format){
 Command 'export_document' 'File'
 Wait-Until {(Model).windows_document.stage -eq 'options'} 'Export options did not open' 30
 $serial=(Model).windows_document.serial
 $reset=@{item=$null};Wait-Until {
  $reset.item=$root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.AndCondition]::new(
   [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,'Reset'),
   [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::Button)))
  $null -ne $reset.item
 } 'Export destination reset is missing'
 $reset.item.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
 Wait-Until {$document=(Model).windows_document;$document.serial -gt $serial -and $document.stage -eq 'options'} 'Export destination did not reset' 30
 Select-Choice 'export-format' $Format
 if($Format.StartsWith('HDR JPEG')){Select-Choice 'export-background' 'White'}
 Button 'Preview Output'
 Wait-Until {(Model).windows_document.stage -eq 'preview'} 'HDR export preparation failed' 60
 Button 'Export';Picker 'Save As';$path=Join-Path $run $Name;Path-In-Picker $path;Idle
 Wait-Until {Test-Path -LiteralPath $path} 'HDR export missing'
 (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash
}
function Capture-ScreenParity {
 if(!(Model).state.screen.chip){
  @{theme=$Theme;screen_capture='unreachable: shared screen chip absent with existing HDR mock'}|ConvertTo-Json|Set-Content (Join-Path $run 'screen-parity-status.json')
  return
 }
  $captureTransparency=(Model).state.settings.transparency
  foreach($captureGlass in @('off','low')){
   Set-CapyTransparency $captureGlass
   Invoke 'screen-status'
   Wait-Until {Find 'screen-details-headline' -Visible} 'Screen details did not open for parity capture'
   $details=(Control 'screen-details-headline' -Arranged).Current.BoundingRectangle
   @{offered=$null -ne (Model).state.screen.details.show_clipped;visible=[bool](Find 'screen-show-clipped' -Visible)}|ConvertTo-Json|Set-Content (Join-Path $run "screen-checkbox-$Theme-glass-$captureGlass.json")
   $scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
   if([Math]::Abs($details.Width/$scale-340) -gt 1){throw 'Screen details content is not 340 DIP wide'}
   @{width_dip=$details.Width/$scale;theme=$Theme;transparency=$captureGlass}|ConvertTo-Json|Set-Content (Join-Path $run "screen-metrics-$Theme-glass-$captureGlass.json")
   Capture ("screen-menu-$Theme-glass-$captureGlass") -WithModel -Composed
   [CapyRowPointer]::Key([uint32]$review.Id,0x1B)
   Wait-Until {!(Find 'screen-details-headline' -Visible)} 'Screen details did not close after parity capture'
  }
  Set-CapyTransparency $captureTransparency
 }
try {
 Enter-CapyEnvironment @('CAPY_TEST_HDR')
 $env:CAPY_STORAGE_DIR=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1'
 [IO.File]::WriteAllText((Settings-File),(@{language=@{Explicit='en'};theme=$Theme}|ConvertTo-Json -Compress))
 if(!$DrawingTabsOnly){$env:CAPY_SMOKE_TEST='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1';$env:CAPY_TEST_HDR='1'}
 $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError (Join-Path $run 'stderr.log')
 $null=$review.Handle;Write-Output "HDR review $($review.Id), $Depth, $run"
 $owned=@{window=$null}
 Wait-Until {$owned.window=Owned-DrawingWindow $review;$m=Model;$owned.window -and $m.brush_ready -and $m.canvas_ready -and $m.windows_tabs.available -and $m.windows_workspace.ready} 'Owned drawing did not start' 75
 $root=$owned.window.Root;$drawingWindow=$owned.window.Handle
 [CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
 [CapyRowPointer]::SetForegroundWindow($drawingWindow)|Out-Null
 if($DrawingTabsOnly){[CapyWindowApi]::ShowWindow($drawingWindow,3)|Out-Null}
 $title=(Control 'drawing-title').Current.Name
 if($title -notmatch ' · \d+ × \d+$' -or (Find 'drawing-tab-1') -or (Find 'drawing-selector')){throw "One drawing did not show the plain title: $title"}
 if(!$DrawingTabsOnly -and !$ControlsOnly){
  Button 'Test pen';Wait-Until {(Model).state.document_file.modified} 'Initial drawing did not become dirty' 60
  Wait-Until {$review.Refresh();$review.MainWindowTitle.StartsWith('• ')} 'Window title lacks the unsaved marker'
 }
 Command 'new_document' 'File';New-DrawingPixels 128 96
 if(!$DrawingTabsOnly){Select-Choice 'document-depth' $(if($Depth -eq 'F32'){'32-bit float HDR'}else{'16-bit float HDR'})}
 Button 'Create';Idle
 Wait-Until {(Model).brush_ready -and ($DrawingTabsOnly -or ((Model).color_panel.hdr -and (Model).color_panel.document_depth -eq $Depth -and (Model).windows_display.analysis.ready))} 'Drawing creation did not settle' 60
 if(!$DrawingTabsOnly -and (Model).windows_display.format -ne 'Rgba16Float'){throw 'Expected native floating-point swap chain'}
 if(@((Model).windows_tabs.tabs).Count -ne 2 -or (!$DrawingTabsOnly -and !$ControlsOnly -and !(Model).windows_tabs.tabs[0].modified)){throw 'New did not retain the first drawing'}
 if(!$DrawingTabsOnly){& (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Resize -Width 1800 -Height 1300}
 Wait-Until {(Find 'drawing-tab-1') -and (Find 'drawing-tab-2') -and (Model).windows_tabs.available} 'Full native drawing strip did not appear'
 if(!$DrawingTabsOnly -and !$ControlsOnly -and !(Control 'drawing-tab-1').Current.Name.StartsWith('• ')){throw 'Dirty tab lacks the unsaved marker'}
 if($screenParity){
  Capture-ScreenParity
  & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -StateDirectory $run -Action Close -DiscardUnsaved
  if(!$review.WaitForExit(15000) -or $review.ExitCode -ne 0){throw 'Owned screen parity review did not close cleanly'}
  if((Get-Item (Join-Path $run 'stderr.log')).Length){throw 'Screen parity stderr requires inspection'}
  return
 }
 if(!$ControlsOnly){
 if(!$tabDropOnly){
 (Control 'drawing-tab-2').SetFocus()
 [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),0x21)
 Wait-Until {(Model).windows_tabs.selected -eq 1 -and (Model).windows_tabs.available} 'Ctrl+PgUp did not select the previous drawing' 60
 [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),0x22)
 Wait-Until {(Model).windows_tabs.selected -eq 2 -and (Model).windows_tabs.available -and (Model).brush_ready -and ($DrawingTabsOnly -or (Model).windows_display.analysis.ready)} 'Ctrl+PgDn did not return to the drawing' 60
 }
 if($DrawingTabsOnly){
  Command 'new_document' 'File';Set-Text 'document-width' '32';Set-Text 'document-height' '24';Button 'Create';Idle
  Wait-Until {@((Model).windows_tabs.tabs).Count -eq 3 -and (Model).windows_tabs.available -and (Model).brush_ready} 'Third clean drawing did not become ready' 60
 }
 if(!$tabDropOnly){
 $keyboardTarget=Find 'drawing-selector'
 if(!$keyboardTarget -or $keyboardTarget.Current.IsOffscreen){$keyboardTarget=Control ('drawing-tab-'+(Model).windows_tabs.selected) -Arranged}
 Focus-Keyboard $keyboardTarget
 [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11,0x10),0x41)
 Wait-Until {(Find 'drawing-list') -and (Find 'drawing-row-close-1')} 'Ctrl+Shift+A did not open the drawing selector'
 [CapyRowPointer]::Key([uint32]$review.Id,0x1B);Wait-Until {!(Find 'drawing-list')} 'Drawing selector did not close'
 if($DrawingTabsOnly){
  $selector=Compact-Drawings
  Wait-Until {$selector.Current.ItemStatus -eq 'Closed'} 'Initial drawing popup did not acknowledge closure'
  [CapyWindowApi]::ShowWindow($drawingWindow,3)|Out-Null
  $null=Control 'drawing-tab-1' -Arranged
 }
 }
 $tabWidget=(Control 'drawing-tab-1').GetRuntimeId() -join ':'
 $tabStrip=Control 'drawing-tabs' -Arranged
 foreach($device in @('mouse','touch','pen')){
  Write-Output "Native contact: $device at line $($MyInvocation.ScriptLineNumber)"
  $beforeOrder=((Model).windows_tabs.tabs.id -join ',')
  $expectedReorder=if($DrawingTabsOnly){'2,1,3'}else{'2,1'}
  $from=(Control 'drawing-tab-1' -Within $tabStrip).Current.BoundingRectangle;$to=(Control 'drawing-tab-2' -Within $tabStrip).Current.BoundingRectangle
  $dpi=[CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4));[CapyRowPointer]::Initialize([uint32]$review.Id)
  try {
   if(!$tabDropOnly){
   if($DrawingTabsOnly){
    $scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
    if($scale -le 0 -or [double]::IsNaN($scale) -or [double]::IsInfinity($scale)){throw 'Invalid run: owned drawing DPI is unavailable'}
    Invoke 'drawing-tab-3'
    Wait-Until {(Model).windows_tabs.selected -eq 3 -and ((Model).windows_tabs.tabs.id -join ',') -eq '1,2,3' -and (Model).windows_tabs.available -and (Model).brush_ready} 'Near-edge middle-tab baseline did not become ready'
    Outside-Tap 'drawing-tab-2' $device -NearEdge
    $inactive=if((Model).windows_tabs.selected -eq 1){2}else{1}
    Write-Output "$device phase: tab outside release"
    Outside-Tap ('drawing-tab-'+$inactive) $device
    Write-Output "$device phase: list outside release"
    $null=Compact-Drawings
    Open-Drawings -Keyboard
    Outside-Tap ('drawing-row-'+$inactive) $device -List
    $bodyDrifts=if($device -eq 'mouse'){@(0,1)}else{@(0,3)}
    foreach($drift in $bodyDrifts){
     Write-Output "$device phase: list body $drift DIP"
     Open-Drawings -Keyboard;$listParent=Control 'drawing-list' -Arranged
     $id=if((Model).windows_tabs.selected -eq 1){2}else{1};$row=Control ('drawing-row-'+$id) -Within $listParent -Arranged;$bounds=$row.Current.BoundingRectangle;$x=[int]($bounds.X+$bounds.Width/2);$y=[int]($bounds.Y+$bounds.Height/2);$offset=[int]($drift*$scale)
     if($x+$offset -ge $bounds.Right -or $y+$offset -ge $bounds.Bottom){throw 'Invalid run: drawing-list drift leaves its row'}
     $contact=@{device=$device;drift_dip=$drift;runtime_id=@($row.GetRuntimeId());bounds=$bounds.ToString();from=@($x,$y);to=@(($x+$offset),($y+$offset));expected_selected=$id;selected_before=(Model).windows_tabs.selected}
     if($device -eq 'pen'){[CapyRowPointer]::PenHover($x,$y)}
     [CapyRowPointer]::Down($device,$x,$y);Start-Sleep -Milliseconds 35
     if($drift){[CapyRowPointer]::Move($x+$offset,$y+$offset)}
     [CapyRowPointer]::Up($device -eq 'pen')
     $contact.contact_ms=[CapyRowPointer]::ContactDurationMilliseconds;$contact.max_gap_ms=[CapyRowPointer]::MaxGapMilliseconds
     try{Wait-Until {(Model).windows_tabs.selected -eq $id -and (Model).windows_tabs.available -and (Model).brush_ready} 'Native drawing-list body tap did not select its drawing' 60}
     finally{$contact.actual_selected=(Model).windows_tabs.selected;$list=Find 'drawing-list';$contact.gesture_after=if($list){$list.Current.ItemStatus}else{$null};$contact|ConvertTo-Json -Depth 7|Set-Content (Join-Path $run ($device+'-list-tap-'+$drift+'.json'))}
     if(Find 'drawing-list'){[CapyRowPointer]::Key(0x1b);Wait-Until {!(Find 'drawing-list')} 'Drawing list did not dismiss after native selection'}
    }
    Write-Output "$device phase: list acknowledged context"
    Open-Drawings -Keyboard;$listParent=Control 'drawing-list' -Arranged
    $selected=(Model).windows_tabs.selected;$row=Control 'drawing-row-1' -Within $listParent -Arranged;$bounds=$row.Current.BoundingRectangle;$x=[int]($bounds.X+$bounds.Width/2);$y=[int]($bounds.Y+$bounds.Height/2)
    if($device -eq 'mouse'){[CapyRowPointer]::RightClick($x,$y)}else{[CapyRowPointer]::Down($device,$x,$y);Wait-Until {$null -ne (Find 'drawing-order-undo')} 'Drawing-list hold did not open its native menu';[CapyRowPointer]::Up($device -eq 'pen')}
    Wait-Until {$null -ne (Find 'drawing-order-undo')} 'Drawing-list context menu did not survive normal release'
    if((Model).windows_tabs.selected -ne $selected -or ((Model).windows_tabs.tabs.id -join ',') -ne $beforeOrder){throw 'Drawing-list context activation changed selection or order'}
    [CapyRowPointer]::Key(0x1b);Wait-Until {!(Find 'drawing-order-undo')} 'Acknowledged drawing-list menu did not dismiss'
    if(Find 'drawing-list'){[CapyRowPointer]::Key(0x1b);Wait-Until {!(Find 'drawing-list')} 'Drawing list did not dismiss after context checks'}
    $selector=Control 'drawing-selector' -Arranged
    Wait-Until {$selector.Current.ItemStatus -eq 'Closed'} 'Drawing popup did not finish closing before full-strip layout'
    [CapyWindowApi]::ShowWindow($drawingWindow,3)|Out-Null
    Wait-Until {(Model).windows_tabs.available -and (Model).brush_ready} 'Drawing strip did not become ready after compact-list checks' 60
    $null=Control 'drawing-tab-1' -Within $tabStrip -Arranged
   }
   Write-Output "$device phase: short tab tap"
   $id=if((Model).windows_tabs.selected -eq 1){2}else{1};$button=Control ('drawing-tab-'+$id) -Within $tabStrip -Arranged;$bounds=$button.Current.BoundingRectangle
   [CapyRowPointer]::Down($device,[int]($bounds.X+$bounds.Width/2),[int]($bounds.Y+$bounds.Height/2));[CapyRowPointer]::Up($device -eq 'pen')
   Wait-Until {(Model).windows_tabs.selected -eq $id -and (Model).brush_ready} 'Native short tab tap did not select its drawing' 60
   Write-Output "$device phase: tab acknowledged context"
   $selected=(Model).windows_tabs.selected;$button=Control 'drawing-tab-1' -Within $tabStrip -Arranged;$bounds=$button.Current.BoundingRectangle;$x=[int]($bounds.X+$bounds.Width/2);$y=[int]($bounds.Y+$bounds.Height/2)
   if($device -eq 'mouse'){[CapyRowPointer]::RightClick($x,$y)}else{
    [CapyRowPointer]::Down($device,$x,$y)
    Wait-Until {$null -ne (Find 'drawing-order-undo')} 'Native tab hold did not open its context menu'
    [CapyRowPointer]::Up($device -eq 'pen')
    Wait-Until {$tabStrip.Current.ItemStatus -eq 'Ready'} 'Held drawing-tab contact did not finish'
   }
   Wait-Until {$null -ne (Find 'drawing-order-undo')} 'Native tab context menu did not survive release'
   if((Model).windows_tabs.selected -ne $selected -or ((Model).windows_tabs.tabs.id -join ',') -ne $beforeOrder){throw 'Native tab context activation changed selection or order'}
   [CapyRowPointer]::Key(0x1b);Wait-Until {!(Find 'drawing-order-undo')} 'Acknowledged tab context menu did not dismiss'
   if($device -eq 'pen'){
    Write-Output "$device phase: retained hover tab drift"
    Invoke 'drawing-tab-2'
    Wait-Until {(Model).windows_tabs.selected -eq 2 -and (Model).windows_tabs.available} 'Drawing-tab drift fixture did not select its initial drawing'
    $drifts=@(@(6,0),@(0,6),@(4,4),@(-6,0),@(0,-6),@(-4,-4))
    $scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
    if($scale -le 0 -or [double]::IsNaN($scale) -or [double]::IsInfinity($scale)){throw 'Cannot read the owned drawing window DPI'}
    for($i=0;$i -lt $drifts.Count;$i++){
     $id=if($i%2 -eq 0){1}else{2}
     if((Model).windows_tabs.selected -eq $id){throw 'Drifting drawing-tab tap did not target an inactive drawing'}
     $bounds=(Control ('drawing-tab-'+$id) -Within $tabStrip -Arranged).Current.BoundingRectangle
     $x=[int]($bounds.X+$bounds.Width/2);$y=[int]($bounds.Y+$bounds.Height/2)
     $toX=[int]($x+$drifts[$i][0]*$scale);$toY=[int]($y+$drifts[$i][1]*$scale)
     if($toX -le $bounds.Left -or $toX -ge $bounds.Right -or $toY -le $bounds.Top -or $toY -ge $bounds.Bottom){throw 'Drawing-tab drift left its hit bounds'}
     [CapyRowPointer]::PenHover($x,$y);Start-Sleep -Milliseconds 35
     [CapyRowPointer]::Down('pen',$x,$y);Start-Sleep -Milliseconds 35
     [CapyRowPointer]::Move($toX,$toY);Start-Sleep -Milliseconds 35
     [CapyRowPointer]::Up($true)
     Wait-Until {(Model).windows_tabs.selected -eq $id -and (Model).windows_tabs.available -and (Model).brush_ready} 'One drifting pen tap did not select its drawing' 60
     Start-Sleep -Milliseconds 150
     if((Model).windows_tabs.selected -ne $id -or ((Model).windows_tabs.tabs.id -join ',') -ne $beforeOrder){throw 'Drifting drawing-tab tap changed the settled selection or drawing order'}
    }
   }
   Write-Output "$device phase: drag cancellation"
   $from=(Control 'drawing-tab-1' -Within $tabStrip -Arranged).Current.BoundingRectangle;$to=(Control 'drawing-tab-2' -Within $tabStrip -Arranged).Current.BoundingRectangle
   [CapyRowPointer]::Down($device,[int]($from.X+$from.Width/2),[int]($from.Y+$from.Height/2))
   [CapyRowPointer]::Move([int]($to.X+$to.Width-8),[int]($to.Y+$to.Height/2))
   Wait-Until {$tabStrip.Current.ItemStatus -eq 'Dragging'} 'Tab movement did not capture'
   $slide=@{value=$null}
   Wait-Until {$slide.value=(Control 'drawing-tab-slide' -Within $tabStrip).Current.ItemStatus|ConvertFrom-Json;$slide.value.attached -and $slide.value.offsets[1] -lt 0 -and $slide.value.offsets[0] -eq 0} 'Held tab did not slide its neighbor'
   if($device -eq 'mouse'){[CapyRowPointer]::Key([uint32]$review.Id,0x1B);[CapyRowPointer]::Up()}else{[CapyRowPointer]::Cancel()}
   Wait-Until {$tabStrip.Current.ItemStatus -eq 'Ready' -and (Control 'drawing-tab-1' -Within $tabStrip).Current.BoundingRectangle.Width -gt 0} 'Tab cancellation retained capture'
   if(((Model).windows_tabs.tabs.id -join ',') -ne $beforeOrder){throw 'Tab cancellation changed order'}
   Write-Output "$device phase: fast outside leave"
   $fastBefore=Model
   $fast=@{device=$device;from=$from.ToString();to=$to.ToString();before=@{selected=$fastBefore.windows_tabs.selected;order=($fastBefore.windows_tabs.tabs.id -join ',');native=(NativeTabStatus)};taps=@()}
   $fastPath=Join-Path $run ($device+'-tab-fast-leave.json')
   $fast|ConvertTo-Json -Depth 14|Set-Content $fastPath
   try{
    $fastClock=[Diagnostics.Stopwatch]::StartNew()
    try{
     [CapyRowPointer]::Down($device,[int]($from.X+$from.Width/2),[int]($from.Y+$from.Height/2))
     [CapyRowPointer]::Move([int]($to.X+$to.Width-8),[int]($to.Y+$to.Height/2))
     [CapyRowPointer]::Move([int]($to.X+$to.Width-8),[int]($to.Y+$to.Height*3))
    }finally{[CapyRowPointer]::Up();$fastClock.Stop();$fast.elapsed_ms=$fastClock.Elapsed.TotalMilliseconds;$fast.contact_ms=[CapyRowPointer]::ContactDurationMilliseconds;$fast.max_gap_ms=[CapyRowPointer]::MaxGapMilliseconds}
    Wait-Until {$tabStrip.Current.ItemStatus -eq 'Ready'} 'Fast outside leave retained capture after release'
    Start-Sleep -Milliseconds 300
    $fastAfter=Model;$fastSlide=Find 'drawing-tab-slide'
    $fast.after=@{selected=$fastAfter.windows_tabs.selected;order=($fastAfter.windows_tabs.tabs.id -join ',');native=(NativeTabStatus);menu=[bool](Find 'drawing-order-undo');slide_visible=($fastSlide -and !$fastSlide.Current.IsOffscreen);drawing_list=[bool](Find 'drawing-list')}
    $fast|ConvertTo-Json -Depth 14|Set-Content $fastPath
    if($fast.after.selected -ne $fast.before.selected -or $fast.after.order -ne $beforeOrder){throw 'Fast outside release changed the active drawing or order'}
    if($fast.after.native.tab_status -ne 'Ready' -or $fast.after.native.active -or $fast.after.menu -or $fast.after.slide_visible -or $fast.after.drawing_list){throw 'Fast outside release retained a menu, slide or capture'}
    if(!$fast.after.native.header -or $fast.after.native.header.phase -ne 'idle' -or $fast.after.native.header.captured){throw 'Fast outside release retained title-bar input'}
    $freshId=if($fast.before.selected -eq 1){2}else{1}
    foreach($tapId in @($freshId,$fast.before.selected)){
     $tapButton=Control ('drawing-tab-'+$tapId) -Within $tabStrip -Arranged;$tapBounds=$tapButton.Current.BoundingRectangle
     $tap=@{id=$tapId;bounds=$tapBounds.ToString();runtime_id=@($tapButton.GetRuntimeId());point=@([int]($tapBounds.X+$tapBounds.Width/2),[int]($tapBounds.Y+$tapBounds.Height/2));before_selected=(Model).windows_tabs.selected}
     $fast.taps+=@($tap)
     [CapyRowPointer]::Down($device,$tap.point[0],$tap.point[1]);[CapyRowPointer]::Up($device -eq 'pen')
     $tap.contact_ms=[CapyRowPointer]::ContactDurationMilliseconds;$tap.max_gap_ms=[CapyRowPointer]::MaxGapMilliseconds
     Wait-Until {(Model).windows_tabs.selected -eq $tapId -and (Model).windows_tabs.available -and (Model).brush_ready} 'Fresh native tab tap after fast outside release did not select its drawing' 60
     Wait-Until {$tabStrip.Current.ItemStatus -eq 'Ready'} 'Fresh native tab tap after fast outside release retained capture'
     $tap.after_selected=(Model).windows_tabs.selected;$tap.after_order=(Model).windows_tabs.tabs.id -join ',';$tap.native=NativeTabStatus
     if($tap.after_order -ne $beforeOrder -or $tap.native.active -or (Find 'drawing-order-undo')){throw 'Fresh native tab tap after fast outside release changed order or retained input'}
    }
   }finally{$fast|ConvertTo-Json -Depth 14|Set-Content $fastPath}
   Write-Output "$device phase: admitted outside drag"
   [CapyRowPointer]::Down($device,[int]($from.X+$from.Width/2),[int]($from.Y+$from.Height/2))
   [CapyRowPointer]::Move([int]($to.X+$to.Width-8),[int]($to.Y+$to.Height/2))
   Wait-Until {$tabStrip.Current.ItemStatus -eq 'Dragging'} 'Outside tab drag was not admitted inside the strip'
   [CapyRowPointer]::Move([int]($to.X+$to.Width-8),[int]($to.Y+$to.Height*3))
   Wait-Until {$slide.value=(Control 'drawing-tab-slide' -Within $tabStrip).Current.ItemStatus|ConvertFrom-Json;!$slide.value.attached -and $slide.value.offsets[1] -eq 0} 'Leaving the strip did not detach the tab'
   [CapyRowPointer]::Up()
   Wait-Until {$tabStrip.Current.ItemStatus -eq 'Ready'} 'Detached release retained capture'
   Start-Sleep -Milliseconds 300
   if(((Model).windows_tabs.tabs.id -join ',') -ne $beforeOrder){throw 'Releasing outside the strip changed order'}
   }
   Write-Output "$device phase: committed reorder"
   [CapyRowPointer]::Down($device,[int]($from.X+$from.Width/2),[int]($from.Y+$from.Height/2))
   [CapyRowPointer]::Move([int]($to.X+$to.Width-8),[int]($to.Y+$to.Height/2))
   Wait-Until {$tabStrip.Current.ItemStatus -eq 'Dragging'} 'Committed tab drag was not admitted inside the strip'
   $committedSlide=@{value=$null}
   Wait-Until {$committedSlide.value=(Control 'drawing-tab-slide' -Within $tabStrip).Current.ItemStatus|ConvertFrom-Json;$committedSlide.value.attached -and $committedSlide.value.offsets[1] -lt 0} 'Committed tab drag did not preview the reordered neighbor'
   [CapyRowPointer]::Up()
   Wait-Until {$tabStrip.Current.ItemStatus -eq 'Ready'} 'Drawing-tab drop did not finish its native presentation'
   $releasedSource=(Control 'drawing-tab-1' -Within $tabStrip).Current.BoundingRectangle
   $releasedNeighbor=(Control 'drawing-tab-2' -Within $tabStrip).Current.BoundingRectangle
   @{device=$device;source=$releasedSource.ToString();neighbor=$releasedNeighbor.ToString()}|ConvertTo-Json|Set-Content (Join-Path $run ($device+'-tab-release.json'))
   if($releasedSource.Left -le $releasedNeighbor.Left){throw 'Drawing-tab release exposed the original order before arranging its committed layout'}
   Wait-Until {((Model).windows_tabs.tabs.id -join ',') -eq $expectedReorder} 'A native tab drag did not produce the expected order on release'
  if(((Control 'drawing-tab-1' -Within $tabStrip).GetRuntimeId() -join ':') -ne $tabWidget){throw 'Reorder rebuilt the native tab'}
  Write-Output "$device phase: order history"
  $historyIndex=0
  foreach($history in @('undo','redo','undo')){
   (Control 'drawing-tab-1' -Within $tabStrip).SetFocus();[CapyRowPointer]::Key([uint32]$review.Id,0x5D)
   $expected=if($history -eq 'redo'){$expectedReorder}else{$beforeOrder}
   $historyContact=$null
   if($DrawingTabsOnly){
    $item=Control ('drawing-order-'+$history) -Arranged
    Wait-Until {$item.Current.IsEnabled} 'Native tab history item is disabled'
    $bounds=$item.Current.BoundingRectangle;$x=[int]($bounds.X+$bounds.Width/2);$y=[int]($bounds.Y+$bounds.Height/2)
    $historyContact=@{device=$device;history=$history;runtime_id=@($item.GetRuntimeId());bounds=$bounds.ToString();point=@($x,$y);expected_order=$expected;before_order=((Model).windows_tabs.tabs.id -join ',')}
    [CapyRowPointer]::Down($device,$x,$y);[CapyRowPointer]::Up($device -eq 'pen')
   }else{Invoke ('drawing-order-'+$history)}
   try{Wait-Until {((Model).windows_tabs.tabs.id -join ',') -eq $expected} 'Tab order history failed'}
   finally{if($historyContact){$historyContact.actual_order=(Model).windows_tabs.tabs.id -join ',';$historyContact|ConvertTo-Json -Depth 6|Set-Content (Join-Path $run ($device+'-history-'+$historyIndex+'-'+$history+'.json'))}}
   Wait-Until {!(Find ('drawing-order-'+$history))} 'Tab order menu did not close'
   $historyIndex++
  }
  }catch{$failure=$_;Capture-Failure $failure;throw $failure}finally{[CapyRowPointer]::Dispose();[CapyRowPointer]::SetThreadDpiAwarenessContext($dpi)|Out-Null}
 }
 if($DrawingTabsOnly){
  & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -StateDirectory $run -Action Close -DiscardUnsaved
  if(!$review.WaitForExit(15000)){throw 'Owned drawing-tabs review did not close'}
  if($review.ExitCode -ne 0){throw "Owned drawing-tabs review exited $($review.ExitCode)"}
  if((Get-Item (Join-Path $run 'stderr.log')).Length){throw 'Drawing-tabs stderr requires inspection'}
  if($tabDropOnly){
   @{theme=$Theme;devices=@('mouse','touch','pen');arranged_handoff='passed';retained_tabs='passed';reorder_history='passed';zero_exit='passed';scope='Normal-document tab drops; physical input and latency unverified'}|ConvertTo-Json|Tee-Object -FilePath (Join-Path $run 'results.json')
   return
  }
  @{theme=$Theme;devices=@('mouse','touch','pen');short_taps='passed';near_edge_outside_release='passed';context_release_and_dismissal='passed';compact_list_taps_context='passed';pen_drift='passed';drag_cancel_outside='passed';fast_leave_and_fresh_tap='passed';reorder_history='passed';zero_exit='passed';scope='Normal-document native input; physical digitizers, HDR and latency unverified'}|ConvertTo-Json|Tee-Object -FilePath (Join-Path $run 'results.json')
  return
 }
 Invoke 'drawing-close-1';Button 'Cancel';Idle
 Wait-Until {(Model).windows_tabs.selected -eq 1 -and (Model).windows_tabs.available} 'Closing an inactive dirty tab did not activate and preserve it on Cancel' 60
 if(@((Model).windows_tabs.tabs).Count -ne 2){throw 'Close cancellation removed a drawing'}
 Command 'undo' 'Edit';Wait-Until {!(Model).state.document_file.modified} 'First drawing lost independent undo history'
 Command 'redo' 'Edit';Wait-Until {(Model).state.document_file.modified} 'First drawing lost independent redo history'
 Invoke 'drawing-tab-2';Wait-Until {(Model).windows_tabs.selected -eq 2 -and (Model).brush_ready -and (Model).color_panel.document_depth -eq $Depth -and (Model).windows_display.analysis.ready} 'Returning to HDR tab failed' 60
 if((Model).windows_tabs.parked_renderers -ne 0){throw 'Inactive drawing retained a renderer'}

 # Three drawings force a compact selector at this DPI; two still fit the strip.
 Command 'new_document' 'File';New-DrawingPixels 32 24;Button 'Create';Idle
 Wait-Until {@((Model).windows_tabs.tabs).Count -eq 3 -and (Model).windows_tabs.available} 'Third drawing did not become available' 60
 & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Resize -Width 900 -Height 1100
 Wait-Until {(Find 'drawing-selector')} 'Compact drawing selector did not appear'
 foreach($device in @('mouse','touch','pen')){
  Write-Output "Native contact: $device at line $($MyInvocation.ScriptLineNumber)"
  Open-Drawings
  $grip=(Control 'drawing-grip-1').Current.BoundingRectangle;$target=(Control 'drawing-row-2').Current.BoundingRectangle
  $dpi=[CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4));[CapyRowPointer]::Initialize([uint32]$review.Id)
  try {
   [CapyRowPointer]::Down($device,[int]($grip.X+$grip.Width/2),[int]($grip.Y+$grip.Height/2))
   [CapyRowPointer]::Move([int]($target.X+$target.Width/2),[int]($target.Y+$target.Height-4));[CapyRowPointer]::Up()
  }finally{[CapyRowPointer]::Dispose();[CapyRowPointer]::SetThreadDpiAwarenessContext($dpi)|Out-Null}
  Wait-Until {((Model).windows_tabs.tabs.id -join ',') -eq '2,1,3'} 'Selector grip did not reorder immediately'
  (Control 'drawing-row-1').SetFocus();[CapyRowPointer]::Key([uint32]$review.Id,0x5D);Invoke 'drawing-order-undo'
  Wait-Until {((Model).windows_tabs.tabs.id -join ',') -eq '1,2,3'} 'Selector order undo failed'
  Wait-Until {!(Find 'drawing-order-undo')} 'Tab order menu did not close'
  (Control 'drawing-row-1').SetFocus()
  [CapyRowPointer]::Key([uint32]$review.Id,0x1B)
  Wait-Until {(Control 'drawing-selector').Current.ItemStatus -eq 'Closed' -and !(Find 'drawing-list')} 'Drawing selector did not dismiss'
 }
 Open-Drawings;(Control 'drawing-row-1').SetFocus();[CapyRowPointer]::Key([uint32]$review.Id,0x0D)
 Wait-Until {(Model).windows_tabs.selected -eq 1 -and (Model).windows_tabs.available} 'Selector keyboard activation failed' 60
 Open-Drawings;(Control 'drawing-row-2').SetFocus();[CapyRowPointer]::Key([uint32]$review.Id,0x0D)
 Wait-Until {(Model).windows_tabs.selected -eq 2 -and (Model).windows_tabs.available} 'Selector did not restore the HDR drawing' 60
 & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Resize -Width 1800 -Height 1300
 Wait-Until {(Find 'drawing-close-3')} 'Full strip did not return'
 Invoke 'drawing-close-3'
 Wait-Until {@((Model).windows_tabs.tabs).Count -eq 2 -and (Model).windows_tabs.selected -eq 2 -and (Model).windows_tabs.available} 'Closing the clean selector fixture drawing failed' 60
 }
 $display=(Model).windows_display
 Button 'Test SDR output';Wait-Until {(Model).windows_display.headroom -eq 1} 'Synthetic SDR fallback did not apply'
 if(!$ArcContextOnly){
 Sdr
 $before=(Model).state.document_file|ConvertTo-Json -Compress
 [CapyRowPointer]::SetForegroundWindow($drawingWindow)|Out-Null
 (Control 'proof-panel-exposure').SetFocus();Set-Text 'proof-panel-exposure' '-25';[CapyRowPointer]::Key([uint32]$review.Id,0x1B);Idle
 if(((Model).state.document_file|ConvertTo-Json -Compress) -ne $before){throw 'Cancelled SDR field edited the drawing'}
 (Control 'proof-panel-exposure').SetFocus();Set-Text 'proof-panel-exposure' '-25';[CapyRowPointer]::Key([uint32]$review.Id,0x0D)
 Wait-Until {(Model).windows_proof_form.rendition.exposure -eq -1} 'Saved SDR appearance was lost'
 Command 'undo' 'Edit';Wait-Until {(Model).windows_proof_form.rendition.exposure -eq 0} 'SDR appearance undo failed'
 Command 'redo' 'Edit';Wait-Until {(Model).windows_proof_form.rendition.exposure -eq -1} 'SDR appearance redo failed'
 }
 Invoke 'panel-tab-color'
 [CapyRowPointer]::SetForegroundWindow($drawingWindow)|Out-Null
 (Control 'color-readout').SetFocus();[CapyRowPointer]::Key([uint32]$review.Id,0x5D)
 Invoke 'edit-color';Wait-Until {Find 'edit-color-apply'} 'Edit Color did not open'
 Invoke 'edit-color-form-0';Invoke 'edit-color-form-0-linear_rgb'
 Wait-Until {(Control 'edit-color-form-0').Current.Name -eq 'Linear RGB'} 'Edit Color did not switch to Linear RGB'
 foreach($value in @(@('edit-color-0-0','1'),@('edit-color-0-1','0.5'),@('edit-color-0-2','0.25'),@('edit-color-intensity','not a number'))){
  Invoke $value[0];(Control ($value[0]+'-entry')).SetFocus();Set-Text ($value[0]+'-entry') $value[1];[CapyRowPointer]::Key([uint32]$review.Id,0x0D)
 }
 Wait-Until {(Find 'edit-color-intensity-entry') -and !(Control 'edit-color-apply').Current.IsEnabled} 'Invalid HDR intensity was accepted'
 Set-Text 'edit-color-intensity-entry' '2';[CapyRowPointer]::Key([uint32]$review.Id,0x0D)
 Wait-Until {!(Find 'edit-color-intensity-entry')} 'HDR intensity did not commit'
 Invoke 'edit-color-apply'
 Wait-Until {(Model).color_panel.intensity -eq 2 -and (Model).color_panel.definition.rgba[0] -gt 1} 'HDR numeric paint was not applied'
 Wait-Until {!(Find 'edit-color-apply')} 'Edit Color did not close'
 $arcCases=@();$geometryDll=Join-Path (Split-Path -Parent $Executable) 'layer_windows.dll';$geometryHash=(Get-FileHash -LiteralPath $geometryDll -Algorithm SHA256).Hash
 foreach($device in @('mouse','touch','pen')){foreach($ending in @('up','cancel')){
  Write-Output "HDR arc context: $device $ending"
  Invoke 'color-foreground';Wait-Until {(Model).state.colors.paint_slot -eq 'foreground'} 'Foreground selection did not settle'
  $stage=Control 'color-panel' -Arranged;$box=$stage.Current.BoundingRectangle
  $scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.0
  if($scale -le 0 -or [double]::IsNaN($scale) -or [double]::IsInfinity($scale)){throw 'Invalid owned color panel DPI'}
  $beforeIntensity=(Model).color_panel.intensity
  $startFraction=if($beforeIntensity -lt 1){0.625}else{0.25}
  $points=@();foreach($fraction in @($startFraction,0.875)){
   $arc=Arc-Geometry $geometryDll (@{type='arc';size=$box.Width/$scale;fraction=$fraction}|ConvertTo-Json -Compress)
   if(!$arc.point -or @($arc.point).Count -ne 2){throw 'Shared HDR arc geometry has no point'}
   $points+=,@([int]($box.X+$arc.point[0]*$scale),[int]($box.Y+$arc.point[1]*$scale))
  }
  $arcCase=@{device=$device;ending=$ending;stage=(Element-Evidence $stage);scale=$scale;points=$points;dll_sha256=$geometryHash;cancel_method=$(if($ending -eq 'up'){'normal_up'}elseif($device -eq 'mouse'){'escape_then_up'}elseif($device -eq 'pen'){'destroy_synthetic_device_no_canceled_event_guarantee'}else{'canceled_up'})}
  $dpi=[CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4));[CapyRowPointer]::Initialize([uint32]$review.Id)
  try {
   [CapyRowPointer]::Down($device,$points[0][0],$points[0][1])
   Wait-Until {[CapyRowPointer]::Active -and (Model).state.colors.paint_slot -eq 'foreground' -and [Math]::Abs((Model).color_panel.intensity-$beforeIntensity) -gt 0.5} 'Native HDR arc did not acknowledge a changed foreground intensity while held'
   $arcCase.foreground_before=$beforeIntensity;$arcCase.foreground_acknowledged=(Model).color_panel.intensity
   Invoke 'color-background';Wait-Until {(Model).state.colors.paint_slot -eq 'background'} 'Held arc did not switch to Background'
   $target=(Model).color_panel|Select-Object intensity,definition;$arcCase.target_before=$target
   [CapyRowPointer]::Move($points[1][0],$points[1][1])
   if($ending -eq 'up'){[CapyRowPointer]::Up()}elseif($device -eq 'mouse'){[CapyRowPointer]::Key([uint32]$review.Id,0x1B);[CapyRowPointer]::Up()}else{[CapyRowPointer]::Cancel()}
   $arcCase.contact_ms=[CapyRowPointer]::ContactDurationMilliseconds;$arcCase.max_gap_ms=[CapyRowPointer]::MaxGapMilliseconds
   $arcCase.target_after=(Model).color_panel|Select-Object intensity,definition
   if(($arcCase.target_before|ConvertTo-Json -Compress -Depth 12) -ne ($arcCase.target_after|ConvertTo-Json -Compress -Depth 12)){throw 'Old HDR arc contact changed the newly selected color slot'}
   $readout=(Model).color_panel.readout;$published=[IO.File]::GetLastWriteTimeUtc((State-File))
   Invoke 'color-readout'
   Wait-Until {(Model).color_panel.readout -ne $readout -and [IO.File]::GetLastWriteTimeUtc((State-File)) -gt $published} 'Owned HDR host did not publish the post-contact readout action'
   Idle
   $arcCase.target_after_publication=(Model).color_panel|Select-Object intensity,definition
   if(($arcCase.target_before|ConvertTo-Json -Compress -Depth 12) -ne ($arcCase.target_after_publication|ConvertTo-Json -Compress -Depth 12)){throw 'Queued old HDR arc work changed the newly selected color slot'}
   $arcCase.passed=$true;$arcCases+=@($arcCase);$arcCases|ConvertTo-Json -Depth 12|Set-Content (Join-Path $run 'arc-context-cases.json')
  }catch{$arcCase.error=$_.Exception.Message;$arcCases+=@($arcCase);$arcCases|ConvertTo-Json -Depth 12|Set-Content (Join-Path $run 'arc-context-cases.json');Capture-Failure $_;throw}
  finally{[CapyRowPointer]::Dispose();[CapyRowPointer]::SetThreadDpiAwarenessContext($dpi)|Out-Null}
 }}
 $wheelCases=@()
 foreach($device in @('mouse','touch','pen')){foreach($remembered in @('foreground','background')){
  Write-Output "Transparent wheel contact: $device remembered $remembered"
  Invoke ('color-'+$remembered);Wait-Until {(Model).state.colors.paint_slot -eq $remembered} 'Remembered paint selection did not settle'
  Invoke 'color-transparent';Wait-Until {(Model).state.colors.slot -eq 'transparent' -and (Model).state.colors.paint_slot -eq $remembered} 'Transparent did not retain the intended paint slot'
  $other=if($remembered -eq 'foreground'){'background'}else{'foreground'}
  $wheel=Control 'color-wheel' -Arranged;$box=$wheel.Current.BoundingRectangle;$scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
  if($scale -le 0 -or [double]::IsNaN($scale) -or [double]::IsInfinity($scale)){throw 'Invalid owned wheel DPI'}
  $points=@();foreach($location in @(@(.56,.46),@(.62,.55))){
   $point=@([int][Math]::Round($box.X+$location[0]*$box.Width),[int][Math]::Round($box.Y+$location[1]*$box.Height))
   $part=Arc-Geometry $geometryDll (@{type='wheel_hit';size=$box.Width/$scale;point=@((($point[0]-$box.X)/$scale),(($point[1]-$box.Y)/$scale));shape=(Model).color_panel.shape}|ConvertTo-Json -Compress)
   if($part -ne 'field'){throw 'Transparent wheel contact is not inside the shared color field'}
   $points+=,$point
  }
  $before=(Model).state.colors.$remembered|ConvertTo-Json -Compress -Depth 12;$otherBefore=(Model).state.colors.$other|ConvertTo-Json -Compress -Depth 12
  $wheelCase=@{device=$device;remembered=$remembered;other=$other;wheel=(Element-Evidence $wheel);scale=$scale;points=$points;before=$before;other_before=$otherBefore;dll_sha256=$geometryHash}
  $dpi=[CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4));[CapyRowPointer]::Initialize([uint32]$review.Id)
  try{
   [CapyRowPointer]::Down($device,$points[0][0],$points[0][1])
   Wait-Until {[CapyRowPointer]::Active -and (Model).state.colors.slot -eq $remembered -and (Model).state.colors.paint_slot -eq $remembered -and ((Model).state.colors.$remembered|ConvertTo-Json -Compress -Depth 12) -ne $before} 'Transparent wheel Down did not acknowledge paint and selection while held'
   $first=(Model).state.colors.$remembered|ConvertTo-Json -Compress -Depth 12;$wheelCase.first=$first
   [CapyRowPointer]::Move($points[1][0],$points[1][1])
   Wait-Until {[CapyRowPointer]::Active -and (Model).state.colors.slot -eq $remembered -and ((Model).state.colors.$remembered|ConvertTo-Json -Compress -Depth 12) -ne $first} 'Transparent transition canceled the continuing native wheel contact'
   $wheelCase.second=(Model).state.colors.$remembered|ConvertTo-Json -Compress -Depth 12
   [CapyRowPointer]::Up()
   Wait-Until {(Control 'color-controls').Current.ItemStatus -eq 'Ready'} 'Transparent wheel contact retained native capture'
   $wheelCase.other_after=(Model).state.colors.$other|ConvertTo-Json -Compress -Depth 12
   if($wheelCase.other_after -ne $otherBefore){throw 'Transparent wheel contact changed the other paint slot'}
   $wheelCase.contact_ms=[CapyRowPointer]::ContactDurationMilliseconds;$wheelCase.max_gap_ms=[CapyRowPointer]::MaxGapMilliseconds;$wheelCase.passed=$true
   $wheelCases+=@($wheelCase);$wheelCases|ConvertTo-Json -Depth 12|Set-Content (Join-Path $run 'transparent-wheel-cases.json')
  }catch{$wheelCase.error=$_.Exception.Message;$wheelCases+=@($wheelCase);$wheelCases|ConvertTo-Json -Depth 12|Set-Content (Join-Path $run 'transparent-wheel-cases.json');Capture-Failure $_;throw}
  finally{[CapyRowPointer]::Dispose();[CapyRowPointer]::SetThreadDpiAwarenessContext($dpi)|Out-Null}
 }}
 if((Get-FileHash -LiteralPath $geometryDll -Algorithm SHA256).Hash -ne $geometryHash){throw 'Color geometry DLL changed during native contacts'}
 if($ArcContextOnly){
  & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -StateDirectory $run -Action Close -DiscardUnsaved
  if(!$review.WaitForExit(15000) -or $review.ExitCode -ne 0){throw 'Owned HDR arc review did not close cleanly'}
  if((Get-Item (Join-Path $run 'stderr.log')).Length){throw 'HDR arc stderr requires inspection'}
  @{theme=$Theme;numeric_hdr_color='passed';arc_context_mouse_touch_pen=$arcCases;transparent_wheel_mouse_touch_pen=$wheelCases;scope='Native HDR color arc and wheel controls; physical HDR and performance unverified'}|ConvertTo-Json -Depth 8|Tee-Object -FilePath (Join-Path $run 'results.json')
  return
 }
 Invoke 'color-foreground';Wait-Until {(Model).state.colors.paint_slot -eq 'foreground'} 'HDR foreground did not return after context changes'
 (Control 'color-readout').SetFocus();[CapyRowPointer]::Key([uint32]$review.Id,0x5D)
 Invoke 'edit-color';Wait-Until {Find 'edit-color-apply'} 'HDR color restoration did not open'
 Invoke 'edit-color-form-0';Invoke 'edit-color-form-0-linear_rgb'
 foreach($value in @(@('edit-color-0-0','1'),@('edit-color-0-1','0.5'),@('edit-color-0-2','0.25'))){Invoke $value[0];Set-Text ($value[0]+'-entry') $value[1];[CapyRowPointer]::Key([uint32]$review.Id,0x0D)}
 Invoke 'edit-color-intensity';Set-Text 'edit-color-intensity-entry' '2';[CapyRowPointer]::Key([uint32]$review.Id,0x0D)
 Wait-Until {!(Find 'edit-color-intensity-entry')} 'HDR color restoration did not commit'
 Invoke 'edit-color-apply';Wait-Until {(Model).color_panel.intensity -eq 2 -and !(Find 'edit-color-apply')} 'HDR color restoration did not settle'
 $revision=(Model).state.document_file.revision
 Button 'Test pen';Wait-Until {(Model).state.document_file.revision -gt $revision -and (Model).windows_display.analysis.ready} 'HDR painting analysis failed' 60
 # Exercise the retained native Proof controls through the shared Window menu.
 [CapyRowPointer]::SetForegroundWindow($drawingWindow)|Out-Null
 & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'Window'
 $windowMenu=(Model).application_menus|Where-Object id -eq 'window'
 $proofEntry=$null
 foreach($section in $windowMenu.model.sections){foreach($item in $section){if($item.action.action.type -eq 'set_panel_visible' -and $item.action.action.panel -eq 'proof'){$proofEntry=$item}}}
 if(!$proofEntry){throw 'Windows has no shared Proof panel menu'}
 if($proofEntry.selected){
  [CapyRowPointer]::Key([uint32]$review.Id,0x1B)
  ProofPanel
 } else {
  $proofMenuItem=Control $proofEntry.label -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)
  $previousDpi=[CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))
  [CapyRowPointer]::Initialize([uint32]$review.Id)
  try {
   $box=$proofMenuItem.Current.BoundingRectangle
   [CapyRowPointer]::Down('mouse',[int]($box.X+$box.Width/2),[int]($box.Y+$box.Height/2));[CapyRowPointer]::Up()
  } finally {[CapyRowPointer]::Dispose();[CapyRowPointer]::SetThreadDpiAwarenessContext($previousDpi)|Out-Null}
 }
 ProofPanel
 $field=Proof-Reveal 'proof-panel-exposure';$fieldId=$field.GetRuntimeId() -join ':'
 [CapyRowPointer]::SetForegroundWindow($drawingWindow)|Out-Null
 $field.SetFocus();Set-Text 'proof-panel-exposure' 'invalid';[CapyRowPointer]::Key([uint32]$review.Id,0x0D)
 if((Model).windows_proof_form.rendition.exposure -ne -1){throw 'Invalid Proof field changed the recipe'}
 [CapyRowPointer]::Key([uint32]$review.Id,0x1B)
 $field.SetFocus();Set-Text 'proof-panel-exposure' '-10';[CapyRowPointer]::Key([uint32]$review.Id,0x0D)
 Wait-Until {[Math]::Abs((Model).windows_proof_form.rendition.exposure+0.4) -lt 0.0001} 'Proof numeric commit did not apply'
 if(((Control 'proof-panel-exposure').GetRuntimeId() -join ':') -ne $fieldId){throw 'Proof value update rebuilt its native editor'}
 Command 'undo' 'Edit';Wait-Until {(Model).windows_proof_form.rendition.exposure -eq -1} 'Proof field Undo failed'
 Command 'redo' 'Edit';Wait-Until {[Math]::Abs((Model).windows_proof_form.rendition.exposure+0.4) -lt 0.0001} 'Proof field Redo failed'
 Command 'undo' 'Edit';Wait-Until {(Model).windows_proof_form.rendition.exposure -eq -1} 'Proof field final Undo failed'
 $proofHistory=(Model).state.document_file|ConvertTo-Json -Compress
 Invoke 'proof-panel-mode-sdr';Wait-Until {(Model).state.preview_sdr} 'Proof panel SDR mode failed'
 Invoke 'proof-panel-mode-off';Wait-Until {!(Model).state.preview_sdr} 'Proof panel Off failed'
 if(((Model).state.document_file|ConvertTo-Json -Compress) -ne $proofHistory){throw 'Proof panel modes edited history'}
 # Real native capture with injected mouse/touch/pen; each edit has one history step.
 Invoke 'proof-panel-mode-sdr';Wait-Until {(Model).state.preview_sdr} 'The SDR dial did not appear in SDR mode'
 $screen=(Model).state.screen
 if($screen.chip){
  Capture-ScreenParity
  Invoke 'screen-status';Wait-Until {$null -ne (Find 'screen-details-headline')} 'Screen details did not open'
  if((Control 'screen-details-headline').Current.Name -ne $screen.details.headline){throw 'Screen details headline differs from shared assessment'}
  $clipped='not-offered'
  if($null -ne $screen.details.show_clipped){
   $before=[bool]$screen.details.show_clipped;$check=Control 'screen-show-clipped'
   $check.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
   Wait-Until {[bool](Model).state.screen.details.show_clipped -ne $before} 'Screen clipping checkbox did not update shared state'
   $check.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
   Wait-Until {[bool](Model).state.screen.details.show_clipped -eq $before} 'Screen clipping checkbox did not restore shared state'
   $clipped='passed'
  }
  [CapyRowPointer]::Key([uint32]$review.Id,0x1B)
  @{chip='passed';details='passed';clipped=$clipped;theme=$Theme}|ConvertTo-Json|Set-Content (Join-Path $run 'screen-status.json')
 }else{
  if(Find 'screen-status' -Visible){throw 'Screen chip is visible without a shared chip'}
  @{chip='correctly-hidden';details='not-exercised-no-shared-chip';theme=$Theme}|ConvertTo-Json|Set-Content (Join-Path $run 'screen-status.json')
 }
 $dialBefore=(Model).windows_proof_form.rendition|ConvertTo-Json -Compress
 foreach($device in @('mouse','touch','pen')){
  Write-Output "Native contact: $device at line $($MyInvocation.ScriptLineNumber)"
  $dial=Control 'proof-dial';$dial.SetFocus();Start-Sleep -Milliseconds 150;$box=$dial.Current.BoundingRectangle
  $x=[int]($box.X+$box.Width/2);$y=[int]($box.Y+$box.Height/2)
  $dpi=[CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4));[CapyRowPointer]::Initialize([uint32]$review.Id)
  try {
   [CapyRowPointer]::Down($device,$x,$y);[CapyRowPointer]::Move($x+25,$y-15)
   Wait-Until {((Model).windows_proof_form.rendition|ConvertTo-Json -Compress) -ne $dialBefore} 'Dial drag did not preview'
   if($device -eq 'mouse'){[CapyRowPointer]::Key([uint32]$review.Id,0x1B);[CapyRowPointer]::Up()}else{[CapyRowPointer]::Cancel()}
   Wait-Until {((Model).windows_proof_form.rendition|ConvertTo-Json -Compress) -eq $dialBefore} 'Dial cancellation did not restore the recipe'
   [CapyRowPointer]::Down($device,$x,$y);[CapyRowPointer]::Move($x+25,$y-15);[CapyRowPointer]::Up()
   Wait-Until {((Model).windows_proof_form.rendition|ConvertTo-Json -Compress) -ne $dialBefore} 'Dial release did not commit'
  } finally {[CapyRowPointer]::Dispose();[CapyRowPointer]::SetThreadDpiAwarenessContext($dpi)|Out-Null}
  $settled=@{value=''};Wait-Until {$now=(Model).windows_proof_form.rendition|ConvertTo-Json -Compress;$same=$now -eq $settled.value;$settled.value=$now;Start-Sleep -Milliseconds 150;$same} 'Dial commit did not settle'
  $dialAfter=$settled.value
  Command 'undo' 'Edit';Wait-Until {((Model).windows_proof_form.rendition|ConvertTo-Json -Compress) -eq $dialBefore} 'Dial did not undo in one step'
  Command 'redo' 'Edit';Wait-Until {((Model).windows_proof_form.rendition|ConvertTo-Json -Compress) -eq $dialAfter} 'Dial redo failed'
  Command 'undo' 'Edit';Wait-Until {((Model).windows_proof_form.rendition|ConvertTo-Json -Compress) -eq $dialBefore} 'Dial final undo failed'
 }
 $dial=Control 'proof-dial';$dial.SetFocus()
 [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x10),0x27)
 Wait-Until {[Math]::Abs((Model).windows_proof_form.rendition.balance-.1) -lt .0001} 'Proof Shift Right did not step balance ten times'
 Command 'undo' 'Edit';Wait-Until {((Model).windows_proof_form.rendition|ConvertTo-Json -Compress) -eq $dialBefore} 'Shift nudge did not undo once'
 $dial=Control 'proof-dial';$dial.SetFocus();$box=$dial.Current.BoundingRectangle
 [CapyRowPointer]::Initialize([uint32]$review.Id)
 try{
  [CapyRowPointer]::Hover([int]($box.X+$box.Width*.5),[int]($box.Y+$box.Height*.5))
  foreach($repeat in 1..3){[CapyRowPointer]::Hold(0x27,$true)}
  [CapyRowPointer]::Hold(0x27,$false)
  Wait-Until {(Model).windows_proof_form.rendition.balance -gt 0} 'Proof repeated key did not apply'
  Command 'undo' 'Edit';Wait-Until {((Model).windows_proof_form.rendition|ConvertTo-Json -Compress) -eq $dialBefore} 'Held Proof key repeats were not one Undo'
  $dial.SetFocus();[CapyRowPointer]::Hold(0x27,$true)
  Wait-Until {(Model).windows_proof_form.rendition.balance -gt 0} 'Proof held key did not preview before Escape'
  [CapyRowPointer]::Key([uint32]$review.Id,0x1B);[CapyRowPointer]::Hold(0x27,$false)
  Wait-Until {((Model).windows_proof_form.rendition|ConvertTo-Json -Compress) -eq $dialBefore} 'Proof Escape did not roll back held key'
  $dial.SetFocus();[CapyRowPointer]::Hold(0x27,$true)
  Wait-Until {(Model).windows_proof_form.rendition.balance -gt 0} 'Proof held key did not preview before focus loss'
  (Control 'drawing-canvas').SetFocus();[CapyRowPointer]::Hold(0x27,$false)
  Wait-Until {((Model).windows_proof_form.rendition|ConvertTo-Json -Compress) -eq $dialBefore} 'Proof focus loss did not roll back held key'
  $x=[int]($box.X+$box.Width/2+25);$y=[int]($box.Y+$box.Height/2-15)
  [CapyRowPointer]::Down('mouse',$x,$y);[CapyRowPointer]::Up()
  Wait-Until {[Math]::Abs((Model).windows_proof_form.rendition.balance) -gt .01} 'Proof first press did not move pad'
  $pressed=(Model).windows_proof_form.rendition|ConvertTo-Json -Compress
  Command 'undo' 'Edit';Wait-Until {((Model).windows_proof_form.rendition|ConvertTo-Json -Compress) -eq $dialBefore} 'Proof reference press did not undo once'
  $expiry=[Diagnostics.Stopwatch]::StartNew();$doubleClickTime=[CapyHdrPicker]::GetDoubleClickTime()
  Wait-Until {$expiry.Elapsed.TotalMilliseconds -gt $doubleClickTime} 'Native double-click interval did not expire'
  $doublePressMs=[CapyRowPointer]::DoubleClick($x,$y)
  @{injection_ms=$doublePressMs;point=@($x,$y)}|ConvertTo-Json|Set-Content (Join-Path $run 'proof-double-press.json')
  Wait-Until {[Math]::Abs((Model).windows_proof_form.rendition.balance) -lt .0001 -and [Math]::Abs((Model).windows_proof_form.rendition.contrast-1) -lt .0001} 'Proof double press did not reset pad'
  Command 'undo' 'Edit';Wait-Until {((Model).windows_proof_form.rendition|ConvertTo-Json -Compress) -eq $pressed} 'Proof pad reset Undo did not restore the exact first press'
  Command 'undo' 'Edit';Wait-Until {((Model).windows_proof_form.rendition|ConvertTo-Json -Compress) -eq $dialBefore} 'Proof first press did not undo once'
 }finally{[CapyRowPointer]::Dispose()}
 @{shift_ten='passed';held_repeat_single_undo='passed';escape_rollback='passed';focus_rollback='passed';double_press_reset='passed'}|ConvertTo-Json|Set-Content (Join-Path $run 'proof-dial-keys.json')
 Invoke 'proof-panel-mode-off';Wait-Until {!(Model).state.preview_sdr} 'Proof panel Off failed after the dial'
 if($ControlsOnly){
  & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -StateDirectory $run -Action Close -DiscardUnsaved
  if(!$review.WaitForExit(15000) -or $review.ExitCode -ne 0){throw 'Owned HDR controls review did not close cleanly'}
  if((Get-Item (Join-Path $run 'stderr.log')).Length){throw 'HDR controls stderr requires inspection'}
  @{theme=$Theme;numeric_hdr_color='passed';arc_context_mouse_touch_pen=$arcCases;transparent_wheel_mouse_touch_pen=$wheelCases;proof_numeric_modes_history='passed';proof_dial_contacts_keys_cancel_history='passed';scope='Native controls functional validation; physical HDR and performance unverified'}|ConvertTo-Json -Depth 8|Tee-Object -FilePath (Join-Path $run 'results.json')
  return
 }
 $plain=Delivery 'SDR.png' 'PNG'
 $pq=Delivery 'HDR.png' 'HDR PNG · BT.2020 PQ'
 $exr=Delivery 'HDR.exr' 'OpenEXR · 32-bit float'
 $jpeg=Delivery 'gain-map.jpg' 'HDR JPEG · gain map'
 $avif=Delivery 'gain-map.avif' 'HDR AVIF · gain map'
 $null=Delivery 'gain-map-clipped.jpg' 'HDR JPEG · clip to gain-map range'
 $null=Delivery 'gain-map-clipped.avif' 'HDR AVIF · clip to gain-map range'
 Command 'undo' 'Edit'
 if((Delivery 'undo-paint.exr' 'OpenEXR · 32-bit float') -eq $exr){throw 'Float painting undo did not remove the stroke'}
 Command 'redo' 'Edit'
 if((Delivery 'redo-paint.exr' 'OpenEXR · 32-bit float') -ne $exr){throw 'Float painting redo did not restore exact export'}

 $file=(Model).state.document_file|ConvertTo-Json -Compress
 Button 'Test HDR output';Wait-Until {(Model).windows_display.headroom -eq 5 -and (Model).state.hdr_display_available} 'Synthetic HDR switch did not apply'
 Invoke 'proof-panel-mode-sdr';Wait-Until {(Model).state.preview_sdr} 'SDR preview did not enable'
 Invoke 'proof-panel-mode-off';Button 'Test SDR output';Wait-Until {!(Model).state.hdr_display_available} 'Synthetic return to SDR failed'
 if(((Model).state.document_file|ConvertTo-Json -Compress) -ne $file){throw 'Display switching modified artwork/history'}
 if((Delivery 'SDR-after-switch.png' 'PNG') -ne $plain){throw 'Display switching changed SDR export'}
 Setup;Button 'Apply';Idle;Wait-Until {(Model).windows_proof.bytes -gt 0} 'HDR print proof failed' 60
 if((Delivery 'proof-on.png' 'PNG') -ne $plain){throw 'Proof changed SDR export'}
 Command 'soft_proof'
 Command 'save_document_as' 'File';Picker 'Save As';$master=Join-Path $run 'HDR 日本語.capy';Path-In-Picker $master;Idle
 Write-Output 'HDR device recovery and master reopen'
 $generation=(Model).windows_gpu_generation
 Button 'Test GPU loss'
 Wait-Until {(Model).windows_gpu_generation -gt $generation -and (Model).brush_ready -and (Model).windows_display.analysis.ready} 'HDR device recovery failed' 60
 if((Delivery 'recovered.exr' 'OpenEXR · 32-bit float') -ne $exr){throw 'GPU replacement changed float export'}
 Command 'open_document' 'File';Picker 'Open';Path-In-Picker $master;Idle
 Wait-Until {(Model).windows_display.analysis.ready} 'Reopened HDR analysis failed' 60
 if((Model).color_panel.document_depth -ne $Depth -or (Model).state.soft_proof){throw 'Reopen lost precision or retained proof viewing'}
 if((Delivery 'reopened.exr' 'OpenEXR · 32-bit float') -ne $exr){throw 'Reopen changed float export'}
 Assert-CanvasInk 'reopened-master'
 Sdr;if((Model).windows_proof_form.rendition.exposure -ne -1){throw 'Reopen lost saved SDR appearance'};Idle
 Command 'export_document' 'File';Button 'Cancel';Idle
 Write-Output 'HDR photo reopen and presentation'
 $import=Join-Path $run $(if($Depth -eq 'F32'){'HDR.exr'}else{'HDR.png'})
 Command 'open_document' 'File';Picker 'Open';Path-In-Picker $import;Idle
 Wait-Until {(Model).color_panel.document_depth -eq $Depth -and (Model).brush_ready -and (Model).windows_display.analysis.ready} 'Exported HDR photo did not reopen as HDR' 60
 foreach($name in @('gain-map.jpg','gain-map.avif')){
  Command 'open_document' 'File';Picker 'Open';Path-In-Picker (Join-Path $run $name);Idle
  Wait-Until {(Model).color_panel.hdr -and (Model).brush_ready -and (Model).windows_display.analysis.ready} 'Gain-map photo did not reopen as HDR' 60
  Assert-CanvasInk $name
 }
 & (Join-Path $PSScriptRoot 'inspect-window.ps1') -ProcessId $review.Id -Output (Join-Path $run 'hdr.png') -ClientOnly -Composed *> (Join-Path $run 'hdr.json')
 $count=@((Model).windows_tabs.tabs).Count
 [CapyWindowApi]::PostMessage($drawingWindow,0x10,[UIntPtr]::Zero,[IntPtr]::Zero)|Out-Null;Button 'Cancel';Idle
 if(@((Model).windows_tabs.tabs).Count -ne $count){throw 'Window close cancellation lost drawings'}
 Command 'close_document' 'File';Button 'Discard Changes';Idle
 Wait-Until {@((Model).windows_tabs.tabs).Count -eq $count-1 -and (Model).windows_tabs.available} 'Close drawing did not retain the other tabs' 60
 & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Close -DiscardUnsaved -StateDirectory $directory
 if((Get-Item (Join-Path $run 'stderr.log')).Length){throw 'HDR stderr requires inspection'}
 [pscustomobject]@{depth=$Depth;creation='passed';sdr_appearance_cancel_history='passed';painting='passed';painting_history='passed';numeric_hdr_color='passed';arc_context_mouse_touch_pen=$arcCases;transparent_wheel_mouse_touch_pen=$wheelCases;hdr_photo_open='passed';reopened_canvas_presentation='passed';pq_exr_sdr_exports='passed';gainmap_exports_and_open='passed';proof_panel_numeric_modes_history='passed';proof_dial_mouse_touch_pen_cancel_history='passed';drawing_tabs_reorder_history_close_cancel='passed';synthetic_display_switching='passed';proof_export_separation='passed';device_recovery='passed';save_reopen='passed';physical_display=$display;scope='Native UIA/D3D12 functional checks. Injected display reports do not qualify physical HDR, mixed-monitor behavior, or performance.'}|ConvertTo-Json -Depth 8|Tee-Object -FilePath (Join-Path $run 'results.json')
}catch{
 $failure=$_
 Capture-Failure $failure;throw $failure
}finally{
 try{[CapyRowPointer]::Dispose()}catch{}
 if($review -and !$review.HasExited){if($drawingWindow){$null=[CapyWindowApi]::PostMessage($drawingWindow,0x10,[UIntPtr]::Zero,[IntPtr]::Zero)};if(!$review.WaitForExit(10000)){$review.Kill();$review.WaitForExit()}}
 Exit-CapyEnvironment
}
