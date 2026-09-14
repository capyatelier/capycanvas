param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes,System.Drawing
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class CapyEditingCapture {
 [StructLayout(LayoutKind.Sequential)] public struct Rect {public int left,top,right,bottom;}
 [StructLayout(LayoutKind.Sequential)] public struct Point {public int x,y;}
 [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h,out Rect r);
 [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h,ref Point p);
 [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h,IntPtr dc,uint flags);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern IntPtr SendMessageTimeout(IntPtr h,uint message,UIntPtr w,IntPtr l,uint flags,uint timeout,out UIntPtr result);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern IntPtr SendMessageTimeout(IntPtr h,uint message,UIntPtr w,System.Text.StringBuilder text,uint flags,uint timeout,out UIntPtr result);
 [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h,uint message,UIntPtr w,IntPtr l);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint process);
 public static void TypePath(IntPtr edit,uint owner,string path) {
  uint process;GetWindowThreadProcessId(edit,out process);if(process!=owner)throw new Exception("Wrong export filename owner");
  UIntPtr result;
  if(SendMessageTimeout(edit,0xB1,UIntPtr.Zero,new IntPtr(-1),2,2000,out result)==IntPtr.Zero)throw new Exception("Cannot select picker text");
  if(SendMessageTimeout(edit,0x303,UIntPtr.Zero,IntPtr.Zero,2,2000,out result)==IntPtr.Zero)throw new Exception("Cannot clear picker text");
  foreach(char c in path)if(SendMessageTimeout(edit,0x102,new UIntPtr(c),IntPtr.Zero,2,2000,out result)==IntPtr.Zero)throw new Exception("Cannot type picker text");
  var actual=new System.Text.StringBuilder(32768);
  if(SendMessageTimeout(edit,13,new UIntPtr((uint)actual.Capacity),actual,2,2000,out result)==IntPtr.Zero||actual.ToString()!=path)throw new Exception("Export filename did not match the owned path");
 }
}
"@
[CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path;$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/canvas-editing/'+[Guid]::NewGuid().ToString('N'));[IO.Directory]::CreateDirectory($run)|Out-Null
$names=@('CAPY_SETTINGS_DIRECTORY','CAPY_TRACE_UI','CAPY_SMOKE_TEST','CAPY_TEST_DISPLAY','CAPY_TEST_PRIMARY','CAPY_PRESENT_PROBE')
$previous=@{};foreach($name in $names){$previous[$name]=[Environment]::GetEnvironmentVariable($name,'Process')}
function Model {
 try{
  $s=Get-Content -LiteralPath (Join-Path $directory 'ui-state.json') -Raw|ConvertFrom-Json
  if($s.process_id -ne $app.Id -or !$s.model.windows_isolated_settings){return}
  $c=Get-Content -LiteralPath (Join-Path $directory 'camera-state.json') -Raw|ConvertFrom-Json
  if($c.process_id -ne $app.Id -or $c.window_id -ne $s.window_id){return}
  if($c.camera.revision -ge $s.model.state.camera.revision){$s.model.state.camera=$c.camera};$s.model
 }catch{}
}
function Wait-Until([scriptblock]$Condition,[string]$Message,[int]$Seconds=10){
 $watch=[Diagnostics.Stopwatch]::StartNew()
 do{[CapyRowPointer]::Verify();if(& $Condition){return};$app.Refresh();if($app.HasExited){throw 'Owned editing review exited'};Start-Sleep -Milliseconds 60}while($watch.Elapsed.TotalSeconds -lt $Seconds)
 throw $Message
}
function Find([string]$Value,[switch]$Name){$property=if($Name){[System.Windows.Automation.AutomationElement]::NameProperty}else{[System.Windows.Automation.AutomationElement]::AutomationIdProperty};$root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new($property,$Value))}
function Control([string]$Value,[switch]$Name){$hit=@{item=$null};Wait-Until {$hit.item=Find $Value -Name:$Name;$null -ne $hit.item} "Missing control: $Value";$hit.item}
function Invoke([string]$Value,[switch]$Name){(Control $Value -Name:$Name).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()}
function Select-Tool([string]$Id){
 if(((Model).state.commands|Where-Object id -eq $Id).selected){return}
 $target=@{id=$null};Wait-Until {foreach($panel in (Model).panels){foreach($tile in $panel.tiles){if($tile.control.command -eq $Id){$target.id="tile-$($panel.id)-$($tile.id)";return $true}}};$false} "No native $Id tile"
 Invoke $target.id;Wait-Until {$ready=Model;if(!$ready.canvas_ready -or !$ready.brush_ready){return $false};if($Id -eq 'scale_rotate'){return (Model).state.layer_tools.tool -eq 'transform'};((Model).state.commands|Where-Object id -eq $Id).selected} "Tool did not activate: $Id"
}
function Value([string]$Id){((Model).state.tool_settings|Where-Object id -eq $Id).value}
function Signature {$m=Model;@($m.state.document_file,@($m.state.layers|Select-Object id,paint_revision,mask_revision))|ConvertTo-Json -Depth 25 -Compress}
function Capture([string]$Name){& (Join-Path $PSScriptRoot 'inspect-window.ps1') -ProcessId $app.Id -ClientOnly -Output (Join-Path $run ($Name+'.png')) *> (Join-Path $run ($Name+'-capture.json'))}
function Pixels {
 $rect=[CapyEditingCapture+Rect]::new();$origin=[CapyEditingCapture+Point]::new()
 if(![CapyEditingCapture]::GetClientRect($handle,[ref]$rect) -or ![CapyEditingCapture]::ClientToScreen($handle,[ref]$origin)){throw 'Cannot locate canvas pixels'}
 $bitmap=[Drawing.Bitmap]::new($rect.right,$rect.bottom)
 try{
  $g=[Drawing.Graphics]::FromImage($bitmap)
  try{$dc=$g.GetHdc();try{if(![CapyEditingCapture]::PrintWindow($handle,$dc,3)){throw 'Canvas capture failed'}}finally{$g.ReleaseHdc($dc)}}finally{$g.Dispose()}
  $stream=[IO.MemoryStream]::new();$hash=[Security.Cryptography.SHA256]::Create();$patches=[Collections.Generic.List[string]]::new()
  try{
   foreach($point in $sampleCenters){
    $patch=$bitmap.Clone([Drawing.Rectangle]::new(($point[0]-$origin.x-8),($point[1]-$origin.y-8),16,16),[Drawing.Imaging.PixelFormat]::Format32bppArgb)
    try{$stream.SetLength(0);$stream.Position=0;$patch.Save($stream,[Drawing.Imaging.ImageFormat]::Png);$patches.Add([Convert]::ToHexString($hash.ComputeHash($stream.ToArray())))}finally{$patch.Dispose()}
   }
   $patches -join ':'
  }finally{$hash.Dispose();$stream.Dispose()}
 }finally{$bitmap.Dispose()}
}
function Stable-Pixels {
 $watch=[Diagnostics.Stopwatch]::StartNew();$last=Pixels;$stable=0
 do{Start-Sleep -Milliseconds 100;$next=Pixels;if($next -eq $last){$stable++}else{$stable=0};$last=$next;if($stable -ge 3){return $last}}while($watch.Elapsed.TotalSeconds -lt 6)
 throw 'Artwork samples did not settle'
}
function Export-Png([string]$Name) {
 if($Name -notmatch '^[a-z0-9-]+$'){throw 'Invalid owned export name'}
 $path=Join-Path $run ($Name+'.png');if(Test-Path -LiteralPath $path){throw 'Export must use a fresh artifact path'}
 # Export waits for pending raster work; its cache identity can change without a new edit.
 $before=(Model).state.document_file|ConvertTo-Json -Compress
 Wait-Until {((Model).state.commands|Where-Object id -eq 'export_document').enabled} 'Export stayed disabled'
 & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'File';Invoke 'export_document'
 $picker=Control 'Save As' -Name
 if($picker.Current.ClassName -ne '#32770' -or $picker.Current.ProcessId -ne $app.Id){throw 'Export picker is not owned'}
 $entry=@{value=$null};Wait-Until {
  $entry.value=$picker.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.AndCondition]::new(
   [System.Windows.Automation.OrCondition]::new(
    [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'1001'),
    [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'1148')),
   [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ClassNameProperty,'Edit')))
  $null -ne $entry.value
 } 'Export filename field did not appear'
 [CapyEditingCapture]::TypePath([IntPtr]$entry.value.Current.NativeWindowHandle,[uint32]$app.Id,$path)
 $save=@{value=$null};Wait-Until {
  $save.value=$picker.FindFirst([System.Windows.Automation.TreeScope]::Children,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'1'))
  $save.value -and $save.value.Current.IsEnabled -and $save.value.Current.ClassName -eq 'Button'
 } 'Export Save button did not become ready'
 $owner=[uint32]0;$button=[IntPtr]$save.value.Current.NativeWindowHandle
 [CapyEditingCapture]::GetWindowThreadProcessId($button,[ref]$owner)|Out-Null
 if($owner -ne $app.Id -or ![CapyEditingCapture]::PostMessage($button,245,[UIntPtr]::Zero,[IntPtr]::Zero)){throw 'Cannot invoke owned export Save button'}
 Wait-Until {(Test-Path -LiteralPath $path) -and !(Model).state.document_file.busy -and (Control 'Drawing canvas' -Name).Current.IsEnabled} 'PNG export did not complete' 45
 if(((Model).state.document_file|ConvertTo-Json -Compress) -ne $before){throw 'PNG export changed the document checkpoint'}
 $bitmap=[Drawing.Bitmap]::new($path)
 try{
  $document=(Model).state.tabs[0]
  if($bitmap.Width -ne $document.width -or $bitmap.Height -ne $document.height){throw 'Export dimensions differ from the drawing'}
  $hash=(Get-FileHash -LiteralPath $path).Hash
  $exports.Add(@{name=$Name;path=$path;sha256=$hash;width=$bitmap.Width;height=$bitmap.Height})
  $hash
 }finally{$bitmap.Dispose()}
}
function Drag([string]$Device,[array]$Points){
 [CapyRowPointer]::Down($Device,$Points[0][0],$Points[0][1])
 try{for($segment=1;$segment -lt $Points.Count;$segment++){$a=$Points[$segment-1];$b=$Points[$segment];for($step=1;$step -le 12;$step++){[CapyRowPointer]::Move([int]($a[0]+($b[0]-$a[0])*$step/12),[int]($a[1]+($b[1]-$a[1])*$step/12));Start-Sleep -Milliseconds 12}}}finally{[CapyRowPointer]::Up()}
 Start-Sleep -Milliseconds 180
}
$checks=[Collections.Generic.List[object]]::new()
$exports=[Collections.Generic.List[object]]::new()
function Pass([string]$Name){$checks.Add(@{name="$device $Name";document=(Model).state.document_file});Write-Output "$device $Name passed"}
function Cancel-Preview([string]$Name){
 Invoke 'tool-action-cancel_transform';Wait-Until {(Model).state.layer_tools.tool -ne 'transform'} 'Transform cancel did not finish'
 if((Signature) -ne $selected -or (Stable-Pixels) -ne $baseline){throw "Cancel did not restore selection artwork: $Name"};Pass $Name
}
function Apply-Preview([string]$Name) {
 if((Signature) -ne $selected){throw "$Name preview committed early"}
 $revision=(Model).state.document_file.revision;Invoke 'tool-action-apply_transform'
 Wait-Until {(Model).state.layer_tools.tool -ne 'transform' -and (Model).state.document_file.revision -gt $revision} "$Name Apply did not commit"
 $applied=Export-Png ($device+'-'+$Name+'-applied-raster')
 if($applied -eq $baselinePng){throw "$Name Apply left the drawing unchanged"}
 Capture ($device+'-'+$Name+'-applied')
 foreach($step in @(
  @{command='Undo';name='undo';expected=$baselinePng},
  @{command='Redo';name='redo';expected=$applied},
  @{command='Undo';name='restore';expected=$baselinePng}
 )){
  $revision=(Model).state.document_file.revision;Invoke $step.command -Name
  Wait-Until {(Model).state.document_file.revision -ne $revision} "$Name $($step.command) did not finish"
  if($step.name -eq 'undo'){
   Select-Tool 'lasso';$checkpoint=(Model).state.document_file|ConvertTo-Json -Compress
   [CapyRowPointer]::Down($device,$sx,$cy);[CapyRowPointer]::Up();Start-Sleep -Milliseconds 180
   if(((Model).state.document_file|ConvertTo-Json -Compress) -ne $checkpoint -or !(Model).state.layer_tools.has_selection){throw 'Stationary lasso changed the selection checkpoint'}
  }
  if((Export-Png ($device+'-'+$Name+'-'+$step.name+'-raster')) -ne $step.expected){throw "$Name $($step.command) did not restore the exact full drawing"}
 }
 Pass ($Name+' Apply and full-drawing Undo/Redo')
 Pass ($Name+' stationary lasso preserves selection, export and Redo')
}
try{
 if(Get-Process CapyCanvas -ErrorAction SilentlyContinue){throw 'Close the existing app before the isolated editing review'}
 foreach($name in $names){Remove-Item -LiteralPath ('Env:'+$name) -ErrorAction SilentlyContinue}
 $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
 $app=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError (Join-Path $run 'stderr.log') -RedirectStandardOutput (Join-Path $run 'stdout.log');$null=$app.Handle
 @{process_id=$app.Id;executable=$Executable;sha256=(Get-FileHash -LiteralPath $Executable).Hash}|ConvertTo-Json|Set-Content -LiteralPath (Join-Path $run 'owner.json')
 Write-Output "Owned canvas editing review $($app.Id): $run"
 Wait-Until {$app.Refresh();$app.MainWindowHandle -ne [IntPtr]::Zero -and (Model).brush_ready -and (Model).windows_filter_load.phase -eq 'ready'} 'Isolated editing canvas did not start' 45
 $handle=$app.MainWindowHandle;$root=[System.Windows.Automation.AutomationElement]::FromHandle($handle)
 $root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Maximized)
 Wait-Until {$c=(Model).state.camera;$b=(Control 'Drawing canvas' -Name).Current.BoundingRectangle;[Math]::Abs($c.viewport[0]-$b.Width) -lt .1 -and $b.Width -gt 1600} 'Maximized canvas did not settle'
 if(@((Model).layout.groups|Where-Object active -eq 'layers').Count){Invoke 'column-icon-layers';Wait-Until {@((Model).layout.groups|Where-Object active -eq 'layers').Count -eq 0} 'Column did not close'}
 Invoke 'canvas-fit';Start-Sleep -Milliseconds 250
 $camera=(Model).state.camera;$area=$camera.work_area;$bounds=(Control 'Drawing canvas' -Name).Current.BoundingRectangle
 $cx=[int]($bounds.X+$area[0]+$area[2]/2);$cy=[int]($bounds.Y+$area[1]+$area[3]/2)
 if($area[2] -lt 900 -or $area[3] -lt 600){throw 'Canvas too small for artwork gesture checks'}
 # Sample inside the artwork and away from drag endpoints/cursor overlays.
 $sx=$cx-60;$sampleCenters=@(@($sx,($cy+25)),@(($sx+300),($cy+25)),@(($cx+70),($cy+25)))
 [CapyRowPointer]::SetForegroundWindow($handle)|Out-Null;[CapyRowPointer]::Initialize([uint32]$app.Id)
 foreach($device in @('mouse','pen')){
  if((Model).state.document_file.modified){throw 'Editing journey requires a clean document'}
  $empty=Stable-Pixels
  Select-Tool 'figure';Invoke 'tool-group-1';Wait-Until {(Model).state.tool_set.groups[1].selected} 'Rectangle did not activate'
  Invoke 'tool-subtool-1';Wait-Until {(Model).state.tool_set.subtools[1].selected} 'Filled rectangle did not activate'
  Drag $device @(@(($cx-120),($cy-80)),@(($cx+120),($cy+80)))
  Wait-Until {(Model).state.document_file.modified -and (Pixels) -ne $empty} 'Filled rectangle did not appear'
  $rectangle=Stable-Pixels;Pass 'immediate filled-rectangle drag'
  Select-Tool 'lasso'
  Drag $device @(@(($cx-100),($cy-60)),@(($cx-20),($cy-60)),@(($cx-20),($cy+60)),@(($cx-100),($cy+60)),@(($cx-100),($cy-60)))
  Wait-Until {(Model).state.layer_tools.has_selection} 'Lasso did not select artwork'
  $selected=Signature;$baseline=Stable-Pixels
  $baselinePng=Export-Png ($device+'-selected-raster')
  if($baseline -ne $rectangle){throw 'Selection changed sampled artwork'};Pass 'lasso selects without painting'
  Select-Tool 'scale_rotate'
  Drag $device @(@($sx,$cy),@(($sx+300),$cy))
  Wait-Until {[Math]::Abs((Value 'transform_x')-300/$camera.zoom) -lt 2 -and [Math]::Abs((Value 'transform_y')) -lt 2} 'Transform body did not move immediately'
  if((Signature) -ne $selected -or (Stable-Pixels) -eq $baseline){throw 'Move preview did not change pixels without committing'}
  Capture ($device+'-move-preview');Cancel-Preview 'move preview and cancel'
  Select-Tool 'scale_rotate'
  Drag $device @(@(($cx-20),($cy+60)),@(($cx+20),($cy+120)))
  Wait-Until {[Math]::Abs((Value 'transform_width')-1.5) -lt .02 -and [Math]::Abs((Value 'transform_height')-1.5) -lt .02} 'Corner handle did not scale selection'
  if((Signature) -ne $selected){throw 'Scale preview committed early'}
  Capture ($device+'-scale-preview');Cancel-Preview 'scale handle and cancel'
  Select-Tool 'scale_rotate'
  # Shared rotation handle is 2.5 times the 12-DIP hit reach above the top edge.
  $radius=60+30*[CapyEditingCapture]::GetDpiForWindow($handle)/96
  Drag $device @(@($sx,([int]($cy-$radius))),@(([int]($sx+$radius)),$cy))
  Wait-Until {[Math]::Abs((Value 'transform_angle')-[Math]::PI/2) -lt .02} 'Rotation handle did not turn selection'
  if((Signature) -ne $selected){throw 'Rotation preview committed early'}
  Capture ($device+'-rotate-preview');Cancel-Preview 'rotation handle and cancel'
  foreach($mode in @('scale','rotate')){
   Select-Tool 'scale_rotate'
   if($mode -eq 'scale'){
    Drag $device @(@(($cx-20),($cy+60)),@(($cx+20),($cy+120)))
    Wait-Until {[Math]::Abs((Value 'transform_width')-1.5) -lt .02 -and [Math]::Abs((Value 'transform_height')-1.5) -lt .02} 'Applied scale preview did not settle'
   }else{
    Drag $device @(@($sx,([int]($cy-$radius))),@(([int]($sx+$radius)),$cy))
    Wait-Until {[Math]::Abs((Value 'transform_angle')-[Math]::PI/2) -lt .02} 'Applied rotation preview did not settle'
   }
   Apply-Preview $mode
   $selected=Signature
  }
  Select-Tool 'scale_rotate';Drag $device @(@($sx,$cy),@(($sx+300),$cy))
  Wait-Until {[Math]::Abs((Value 'transform_x')-300/$camera.zoom) -lt 2} 'Final move preview did not settle'
  $revision=(Model).state.document_file.revision;Invoke 'tool-action-apply_transform'
  Wait-Until {(Model).state.layer_tools.tool -ne 'transform' -and (Model).state.document_file.revision -gt $revision} 'Transform Apply did not commit'
  $moved=Stable-Pixels;$sourcePixels=$baseline.Split(':');$movedPixels=$moved.Split(':');$blankPixels=$empty.Split(':')
  if($movedPixels[0] -ne $blankPixels[0] -or $movedPixels[1] -ne $sourcePixels[0] -or $movedPixels[2] -ne $sourcePixels[2]){throw 'Apply did not move only the selected pixels and preserve the unselected artwork'}
  Capture ($device+'-applied')
  Invoke 'Undo' -Name;Wait-Until {(Pixels) -eq $baseline} 'One Undo did not restore selected pixels'
  Invoke 'Redo' -Name;Wait-Until {(Pixels) -eq $moved} 'One Redo did not restore transformed pixels'
  Invoke 'Undo' -Name;Wait-Until {(Pixels) -eq $baseline} 'Final transform Undo did not restore pixels';Pass 'Apply and one-step raster Undo/Redo'
  Invoke 'Undo' -Name;Wait-Until {!(Model).state.layer_tools.has_selection} 'Selection Undo did not clear the lasso'
  Invoke 'Undo' -Name;Wait-Until {!(Model).state.document_file.modified -and (Pixels) -eq $empty} 'Figure Undo did not return to a clean drawing';Pass 'selection and seed Undo return clean'
 }
 [CapyRowPointer]::Dispose();$app.CloseMainWindow()|Out-Null
 if(!$app.WaitForExit(5000) -or $app.ExitCode -ne 0){throw 'Editing review did not close within five seconds'}
 if((Get-Item -LiteralPath (Join-Path $run 'stderr.log')).Length){throw 'Native editing stderr needs inspection'}
 @{checks=$checks;exports=$exports;close='zero exit within five seconds';pixel_scope='scale/rotation: exact full exported PNG history; translation: three 16x16 artwork interiors; full captures retained';scope='guarded OS mouse and synthetic pen; physical devices and complete visual/performance acceptance remain separate'}|ConvertTo-Json -Depth 10|Set-Content -LiteralPath (Join-Path $run 'result.json')
 Write-Output "Canvas editing acceptance passed: $run"
}catch{
 if($app -and !$app.HasExited -and $root){try{Capture 'failure';@{model=Model;checks=$checks}|ConvertTo-Json -Depth 80|Set-Content -LiteralPath (Join-Path $run 'failure-state.json')}catch{}}
 throw
}finally{[CapyRowPointer]::Dispose();foreach($name in $names){if($null -eq $previous[$name]){Remove-Item -LiteralPath ('Env:'+$name) -ErrorAction SilentlyContinue}else{[Environment]::SetEnvironmentVariable($name,$previous[$name],'Process')}}}