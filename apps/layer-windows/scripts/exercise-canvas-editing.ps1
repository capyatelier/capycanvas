param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyWaitSeconds=10;$CapyEach={[CapyRowPointer]::Verify()}
Add-Type -AssemblyName System.Drawing
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
function Select-Tool([string]$Id){
 if(((Model).state.commands|Where-Object id -eq $Id).selected){return}
 Invoke (Tool-Tile $Id);Wait-Until {$ready=Model;if(!$ready.canvas_ready -or !$ready.brush_ready){return $false};if($Id -eq 'scale_rotate'){return $ready.state.layer_tools.tool -eq 'transform'};($ready.state.commands|Where-Object id -eq $Id).selected} "Tool did not activate: $Id"
}
function Value([string]$Id){((Model).state.tool_settings|Where-Object id -eq $Id).value}
function Body-Point{$m=Model;$a=$m.state.canvas_bar.anchor;$c=$m.state.camera;$r=(Control 'drawing-canvas').Current.BoundingRectangle
 @([int]($r.X+$c.translation[0]+$c.zoom*($a[0]+3*$a[2])/4),[int]($r.Y+$c.translation[1]+$c.zoom*($a[1]+3*$a[3])/4))}
function Origin{Wait-Until {$null -ne (Value 'transform_x') -and $null -ne (Value 'transform_y')} 'Transform position was not published';$script:x0=Value 'transform_x';$script:y0=Value 'transform_y'}
function Signature {$m=Model;@($m.state.document_file,@($m.state.layers|Select-Object id,paint_revision,mask_revision))|ConvertTo-Json -Depth 25 -Compress}
function Pixels {
 $rect=[CapyEditingCapture+Rect]::new();$origin=[CapyEditingCapture+Point]::new()
 if(![CapyEditingCapture]::GetClientRect($handle,[ref]$rect) -or ![CapyEditingCapture]::ClientToScreen($handle,[ref]$origin)){throw 'Cannot locate canvas pixels'}
 $bitmap=[Drawing.Bitmap]::new($rect.right,$rect.bottom)
 try{
  $g=[Drawing.Graphics]::FromImage($bitmap)
  try{$g.CopyFromScreen($origin.x,$origin.y,0,0,$bitmap.Size)}finally{$g.Dispose()}
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
 Invoke 'Preview Output' -Name
 Wait-Until {(Model).windows_document.stage -eq 'preview'} 'Export preview did not prepare' 60
 Invoke 'Export' -Name
 $picker=Control 'Save As' -Name
 if($picker.Current.ClassName -ne '#32770' -or $picker.Current.ProcessId -ne $review.Id){throw 'Export picker is not owned'}
 $entry=@{value=$null};Wait-Until {
  $entry.value=$picker.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.AndCondition]::new(
   [System.Windows.Automation.OrCondition]::new(
    [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'1001'),
    [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'1148')),
   [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ClassNameProperty,'Edit')))
  $null -ne $entry.value
 } 'Export filename field did not appear'
 [CapyEditingCapture]::TypePath([IntPtr]$entry.value.Current.NativeWindowHandle,[uint32]$review.Id,$path)
 $save=@{value=$null};Wait-Until {
  $save.value=$picker.FindFirst([System.Windows.Automation.TreeScope]::Children,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'1'))
  $save.value -and $save.value.Current.IsEnabled -and $save.value.Current.ClassName -eq 'Button'
 } 'Export Save button did not become ready'
 $owner=[uint32]0;$button=[IntPtr]$save.value.Current.NativeWindowHandle
 [CapyEditingCapture]::GetWindowThreadProcessId($button,[ref]$owner)|Out-Null
 if($owner -ne $review.Id -or ![CapyEditingCapture]::PostMessage($button,245,[UIntPtr]::Zero,[IntPtr]::Zero)){throw 'Cannot invoke owned export Save button'}
 Wait-Until {(Test-Path -LiteralPath $path) -and !(Model).state.document_file.busy -and (Control 'drawing-canvas').Current.IsEnabled} 'PNG export did not complete' 45
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
 Invoke 'canvas-bar-cancel_transform';Wait-Until {(Model).state.layer_tools.tool -ne 'transform'} 'Transform cancel did not finish'
 if((Signature) -ne $selected -or (Stable-Pixels) -ne $baseline){throw "Cancel did not restore selection artwork: $Name"};Pass $Name
}
function Apply-Preview([string]$Name) {
 if((Signature) -ne $selected){throw "$Name preview committed early"}
 $revision=(Model).state.document_file.revision;Invoke 'canvas-bar-apply_transform'
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
 Enter-CapyEnvironment
 $env:CAPY_STORAGE_DIR=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
 $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError (Join-Path $run 'stderr.log') -RedirectStandardOutput (Join-Path $run 'stdout.log');$null=$review.Handle
 @{process_id=$review.Id;executable=$Executable;sha256=(Get-FileHash -LiteralPath $Executable).Hash}|ConvertTo-Json|Set-Content -LiteralPath (Join-Path $run 'owner.json')
 Write-Output "Owned canvas editing review $($review.Id): $run"
 Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero -and (Model).brush_ready} 'Isolated editing canvas did not start' 45
 $handle=$review.MainWindowHandle;$root=[System.Windows.Automation.AutomationElement]::FromHandle($handle)
 $root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Maximized)
 Wait-Until {$c=(Model).state.camera;$b=(Control 'drawing-canvas').Current.BoundingRectangle;[Math]::Abs($c.viewport[0]-$b.Width) -lt .1 -and $b.Width -gt 1600} 'Maximized canvas did not settle'
 if(@((Model).layout.groups|Where-Object active -eq 'layers').Count){Invoke 'column-icon-layers';Wait-Until {@((Model).layout.groups|Where-Object active -eq 'layers').Count -eq 0} 'Column did not close'}
 $fitRevision=(Model).state.camera.revision
 Fit-Canvas;Wait-Until {(Model).state.camera.revision -gt $fitRevision} 'Fit did not update the camera'
 $camera=(Model).state.camera;$area=$camera.work_area;$bounds=(Control 'drawing-canvas').Current.BoundingRectangle
 $cx=[int]($bounds.X+$area[0]+$area[2]/2);$cy=[int]($bounds.Y+$area[1]+$area[3]/2)
 @{camera=$camera;canvas=$bounds}|ConvertTo-Json -Depth 8|Set-Content (Join-Path $run 'camera.json')
 if($area[2] -lt 900 -or $area[3] -lt 600){throw 'Canvas too small for artwork gesture checks'}
 # Sample inside the artwork and away from drag endpoints/cursor overlays.
 $sx=$cx-60;$sampleCenters=@(@($sx,($cy+25)),@(($sx+300),($cy+25)),@(($cx+70),($cy+25)))
 [CapyRowPointer]::SetForegroundWindow($handle)|Out-Null;[CapyRowPointer]::Initialize([uint32]$review.Id)
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
  Select-Tool 'scale_rotate';Origin;$from=Body-Point
  Drag $device @($from,@(($from[0]+300),$from[1]))
  Wait-Until {[Math]::Abs((Value 'transform_x')-$x0-300/$camera.zoom) -lt 2 -and [Math]::Abs((Value 'transform_y')-$y0) -lt 2} 'Transform body did not move immediately'
  Wait-Until {(Pixels) -ne $baseline} 'Move preview left the sampled pixels unchanged'
  $previewSignature=Signature;$previewPixels=Stable-Pixels
  @{before=$selected;after=$previewSignature;baseline=$baseline;preview=$previewPixels;centers=$sampleCenters}|ConvertTo-Json -Depth 25|Set-Content (Join-Path $run 'move-preview-state.json')
  if($previewSignature -ne $selected){throw 'Move preview changed the document signature before Apply'}
  if($previewPixels -eq $baseline){throw 'Move preview left the sampled pixels unchanged'}
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
  Select-Tool 'scale_rotate';Origin;$from=Body-Point;Drag $device @($from,@(($from[0]+300),$from[1]))
  Wait-Until {[Math]::Abs((Value 'transform_x')-$x0-300/$camera.zoom) -lt 2} 'Final move preview did not settle'
  $revision=(Model).state.document_file.revision;Invoke 'canvas-bar-apply_transform'
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
 $device='mouse'
 Select-Tool 'figure';Invoke 'tool-group-1';Invoke 'tool-subtool-1'
 Drag $device @(@(($cx-120),($cy-80)),@(($cx+120),($cy+80)))
 Wait-Until {(Model).state.document_file.modified -and (Pixels) -ne $empty} 'Filled rectangle did not appear'
 Select-Tool 'scale_rotate';Origin;Wait-Until {$a=(Model).state.canvas_bar.anchor;$a -and [Math]::Abs((Value 'transform_x')-($a[0]+$a[2])/2) -lt .5} 'The transform bounds did not match the centred position';$bounds=(Model).state.canvas_bar.anchor
 $scale=[CapyEditingCapture]::GetDpiForWindow($handle)/96;$grid=Control 'tool-choice-transform-reference'
 $cells=@(0..8|ForEach-Object {Control "tool-choice-transform-reference-$_"})
 if(@($cells|Where-Object {[Math]::Abs($_.Current.BoundingRectangle.Width-18*$scale) -gt 1.5}).Count){throw 'Position anchor cells are not 18 pixels'}
 $field=(Control 'tool-setting-transform_x').Current.BoundingRectangle;$area=$grid.Current.BoundingRectangle
 if($field.Left -le $area.Right -or $field.Top -gt $area.Bottom -or $field.Bottom -lt $area.Top){throw 'X is not beside the position anchor'}
 foreach($corner in @(@(0,0,1),@(8,2,3))){
  Invoke "tool-choice-transform-reference-$($corner[0])"
  Wait-Until {[Math]::Abs((Value 'transform_x')-$bounds[$corner[1]]) -lt .5 -and [Math]::Abs((Value 'transform_y')-$bounds[$corner[2]]) -lt .5 -and $cells[$corner[0]].Current.ItemStatus} "Anchor $($corner[0]) did not report its corner"
  if(((Model).state.canvas_bar.anchor -join ',') -ne ($bounds -join ',')){throw 'Choosing an anchor moved the transform'}
 }
 $target=[Math]::Round($bounds[2]+300/$camera.zoom);$x=Control 'tool-setting-transform_x';$x.SetFocus();$x.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue([string]$target);(Control 'tool-setting-transform_y').SetFocus()
 Wait-Until {[Math]::Abs((Value 'transform_x')-$target) -lt .01 -and [Math]::Abs((Model).state.canvas_bar.anchor[2]-$target) -lt .5} 'Typed X did not move the anchor point exactly'
 Pass 'position anchor and exact X'
 Invoke 'canvas-bar-transform_snapping';Wait-Until {((Model).state.commands|Where-Object id -eq 'transform_snapping').selected} 'Snap did not turn on'
 Invoke 'canvas-bar-transform_snapping';Wait-Until {!((Model).state.commands|Where-Object id -eq 'transform_snapping').selected} 'Snap did not turn off'
 $moved=(Model).state.canvas_bar.anchor;$c=(Model).state.camera;$r=(Control 'drawing-canvas').Current.BoundingRectangle
 $pivot=@([int]($r.X+$c.translation[0]+$c.zoom*($moved[0]+$moved[2])/2),[int]($r.Y+$c.translation[1]+$c.zoom*($moved[1]+$moved[3])/2));$x1=Value 'transform_x'
 Drag $device @($pivot,@(($pivot[0]+60),($pivot[1]+30)))
 Start-Sleep -Milliseconds 300
 if(((Model).state.canvas_bar.anchor -join ',') -ne ($moved -join ',') -or [Math]::Abs((Value 'transform_x')-$x1) -gt .01){throw 'Dragging the pivot moved the artwork'}
 Pass 'snap toggle and pivot drag'
 Capture 'transform-controls-dark'
 $revision=(Model).state.document_file.revision;Invoke 'canvas-bar-apply_transform'
 Wait-Until {(Model).state.layer_tools.tool -ne 'transform' -and (Model).state.document_file.revision -gt $revision} 'The anchored move did not apply'
 $once=Stable-Pixels;$revision=(Model).state.document_file.revision
 Wait-Until {((Model).state.commands|Where-Object id -eq 'transform_again').enabled} 'Transform Again stayed disabled after a transform'
 & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'Edit';Invoke 'transform_again'
 Wait-Until {(Model).state.document_file.revision -gt $revision -and (Pixels) -ne $once} 'Transform Again did not repeat the move'
 Invoke 'Undo' -Name;Wait-Until {(Pixels) -eq $once} 'Transform Again was not one Undo'
 Pass 'Transform Again'
 $theme=(Model).state.theme;Invoke 'settings-button'
 (Control 'Color theme' -Name -Type ([System.Windows.Automation.ControlType]::ComboBox)).GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
 (Control $(if($theme -eq 'dark'){'Light'}else{'Dark'}) -Name -Type ([System.Windows.Automation.ControlType]::ListItem)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
 Wait-Until {(Model).state.theme -ne $theme} 'Theme change not acknowledged'
 Invoke 'CloseButton';Wait-Until {!(Find 'Preferences' -Name -Type ([System.Windows.Automation.ControlType]::Window))} 'Preferences did not close'
 Select-Tool 'scale_rotate';Wait-Until {Find 'tool-choice-transform-reference'} 'The position anchor did not return'
 Capture 'transform-controls-alternate';Invoke 'canvas-bar-cancel_transform';Wait-Until {(Model).state.layer_tools.tool -ne 'transform'} 'Transform cancel did not finish'
 $switch=@{item=$null};Wait-Until {$switch.item=Find 'workspace-switch-builtin:workspace:photographer';$switch.item} 'No Photo workspace switch'
 $switch.item.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
 Wait-Until {(Model).windows_workspace.id -eq 'builtin:workspace:photographer' -and !(Model).windows_workspace.busy} 'Photo did not open' 20
 Select-Tool 'scale_rotate';Origin
 $options=@{tile=$null};Wait-Until {$options.tile=@((Model).panels|ForEach-Object {$_.tiles}|Where-Object {@($_.component.options|Where-Object {$_.Choice.id -eq 'transform-reference'}).Count})[0];$options.tile} 'Tool Options did not offer the position anchor'
 if(!(Find 'toolbar-segment-transform-reference')){Invoke "toolbar-more-$($options.tile.id)"}
 $cell=@{id=$null};Wait-Until {$cell.id=@('toolbar-segment-transform-reference-0','tool-choice-transform-reference-0')|Where-Object {Find $_}|Select-Object -First 1;$cell.id} 'Tool Options did not present the position anchor'
 Write-Output "Tool Options anchor cell=$($cell.id)";Invoke $cell.id
 Wait-Until {$a=(Model).state.canvas_bar.anchor;[Math]::Abs((Value 'transform_x')-$a[0]) -lt .5 -and [Math]::Abs((Value 'transform_y')-$a[1]) -lt .5} 'The Tool Options anchor did not set the reference'
 Capture 'transform-tool-options';Pass 'Tool Options position anchor'
 Invoke 'canvas-bar-cancel_transform';Wait-Until {(Model).state.layer_tools.tool -ne 'transform'} 'Transform cancel did not finish'
 Invoke 'Undo' -Name;Invoke 'Undo' -Name;Wait-Until {!(Model).state.document_file.modified -and (Pixels) -eq $empty} 'The transform journey did not undo to a clean drawing'
 [CapyRowPointer]::Dispose();$review.CloseMainWindow()|Out-Null
 if(!$review.WaitForExit(5000) -or $review.ExitCode -ne 0){throw 'Editing review did not close within five seconds'}
 if((Get-Item -LiteralPath (Join-Path $run 'stderr.log')).Length){throw 'Native editing stderr needs inspection'}
 @{checks=$checks;exports=$exports;close='zero exit within five seconds';pixel_scope='scale/rotation: exact full exported PNG history; translation: three 16x16 artwork interiors; full captures retained';scope='guarded OS mouse and synthetic pen; physical devices and complete visual/performance acceptance remain separate'}|ConvertTo-Json -Depth 10|Set-Content -LiteralPath (Join-Path $run 'result.json')
 Write-Output "Canvas editing acceptance passed: $run"
}catch{
 if($review -and !$review.HasExited -and $root){try{Capture 'failure';@{model=Model;checks=$checks}|ConvertTo-Json -Depth 80|Set-Content -LiteralPath (Join-Path $run 'failure-state.json')}catch{}}
 throw
}finally{[CapyRowPointer]::Dispose();Exit-CapyEnvironment}
