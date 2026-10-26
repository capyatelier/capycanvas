param([Parameter(Mandatory)][string]$Executable,[ValidateSet('dark','light')][string]$Theme='dark')
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyFind='prefer-visible';$CapyWaitSeconds=20;$CapyEach={[CapyRowPointer]::Verify()}
Add-Type -AssemblyName System.Drawing
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
$null=[CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))
Add-Type -TypeDefinition @'
using System;using System.ComponentModel;using System.Runtime.InteropServices;
public static class CapyThreadStall{
 [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr window,IntPtr process);
 [DllImport("kernel32.dll",SetLastError=true)] static extern IntPtr OpenThread(uint access,bool inherit,uint id);
 [DllImport("kernel32.dll",SetLastError=true)] static extern uint SuspendThread(IntPtr thread);
 [DllImport("kernel32.dll",SetLastError=true)] static extern uint ResumeThread(IntPtr thread);
 [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr handle);
 public static IntPtr Suspend(IntPtr window){
  var thread=OpenThread(2,false,GetWindowThreadProcessId(window,IntPtr.Zero));
  if(thread==IntPtr.Zero)throw new Win32Exception(Marshal.GetLastWin32Error());
  if(SuspendThread(thread)==uint.MaxValue){var error=Marshal.GetLastWin32Error();CloseHandle(thread);throw new Win32Exception(error);}
  return thread;
 }
 public static void Resume(IntPtr thread){try{if(ResumeThread(thread)==uint.MaxValue)throw new Win32Exception(Marshal.GetLastWin32Error());}finally{CloseHandle(thread);}}
}
'@
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$run=Join-Path $repo ('artifacts/windows/vector-edges/'+$Theme+'-'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$CapyTraceDirectory=$run
$checks=[ordered]@{theme=$Theme}
$observations=[Collections.Generic.List[object]]::new()
$findings=[Collections.Generic.List[string]]::new()
$review=$null;$projection=$null;$noticeFrame=$null;$actionWidths=@{}
. (Join-Path $PSScriptRoot 'VectorFixture.ps1')
$control=[uint16]0x11
function Start-Review([string]$Language){
 $file=Settings-File
 $settings=@{}
 if(Test-Path -LiteralPath $file){foreach($property in (Get-Content -LiteralPath $file -Raw|ConvertFrom-Json).PSObject.Properties){$settings[$property.Name]=$property.Value}}
 $settings.language=@{Explicit=$Language};$settings.theme=$Theme
 [IO.File]::WriteAllText($file,($settings|ConvertTo-Json -Depth 20))
 $script:review=Start-Process -FilePath $Executable -WorkingDirectory $run -WindowStyle Hidden -PassThru -RedirectStandardError (Join-Path $run "stderr-$Language.log")
 $null=$review.Handle
 $native=@{window=$null}
 Wait-Until {$native.window=Owned-DrawingWindow $review;$m=Model;$native.window -and $m.canvas_ready -and $m.brush_ready -and $m.windows_workspace.ready -and !$m.windows_workspace.busy} 'The vector edge review did not start' 90
 $script:root=$native.window.Root;$script:drawingWindow=$native.window.Handle;$script:noticeFrame=$null
 $root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Maximized)
 if(![CapyRowPointer]::SetForegroundWindow($drawingWindow)){throw 'The owned drawing did not accept foreground input'}
 [CapyRowPointer]::Initialize([uint32]$review.Id)
 Wait-Until {(Control 'drawing-canvas' -Arranged).Current.BoundingRectangle.Width -gt 900 -and !(Model).windows_workspace.busy} 'The maximized canvas did not settle'
 Use-PaintWorkspace
 Set-Color '#1A1A1F'
 Fit-Canvas;Park;Take-Projection
}
function Stop-Review{
 [CapyRowPointer]::Dispose()
 & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Close -DiscardUnsaved -StateDirectory $run
}
function Chord([uint16]$Key){[CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@($control),$Key)}
function Tool-Name{$t=(Snapshot).state.layer_tools.tool;if($t -is [string]){$t}else{($t|ConvertTo-Json -Compress -Depth 4)}}
function Select-Preset([string]$Query,[long]$Preset){
 $pick={param($result) $result.id -like 'action:*"select_brush"*' -and [long](($result.id -replace '^action:')|ConvertFrom-Json).id -eq $Preset}.GetNewClosure()
 $null=Search $Query $pick "brush $Preset"
 Wait-Until {Brush-Ready $Preset} "Brush $Preset did not become the ready brush" 120
}
function Refuse([string]$Place,$Points){
 for($try=1;;$try++){
  Stroke $Points -Refused
  try{Wait-Until {(Notice-Parts).Contains('canvas-notice-action-new_paint_layer')} "No $Place refusal notice" 10;return}
  catch{if($try -ge 3){throw};$checks["$Place refusal retry $try"]=$true}
 }
}
function Enabled([string]$Id){[bool]@((Snapshot).state.commands|Where-Object id -eq $Id)[0].enabled}
function Inked($At){[bool](Classified (Patch $At) 'ink')}
function Red-Near($At){
 $p=Screen-Point $At;$size=41
 $bitmap=[Drawing.Bitmap]::new($size,$size);$graphics=[Drawing.Graphics]::FromImage($bitmap)
 try{
  $graphics.CopyFromScreen($p[0]-20,$p[1]-20,0,0,$bitmap.Size);$red=0
  for($y=0;$y -lt $size;$y++){for($x=0;$x -lt $size;$x++){$c=$bitmap.GetPixel($x,$y);if($c.R -gt 170 -and $c.G -lt 90 -and $c.B -lt 90){$red++}}}
  $red
 }finally{$graphics.Dispose();$bitmap.Dispose()}
}
function Drag([string]$Device,$From,$To,[int]$Steps=8,[int]$At=-1,[scriptblock]$Between=$null,[scriptblock]$Released=$null){
 $a=Screen-Point $From;$b=Screen-Point $To
 if(![CapyRowPointer]::SetForegroundWindow($drawingWindow)){throw 'The owned drawing did not accept foreground input'}
 [CapyRowPointer]::Down($Device,$a[0],$a[1])
 try{
  for($i=1;$i -le $Steps;$i++){
   Start-Sleep -Milliseconds 16
   [CapyRowPointer]::Move([int]($a[0]+($b[0]-$a[0])*$i/$Steps),[int]($a[1]+($b[1]-$a[1])*$i/$Steps))
   if($i -eq $At -and $Between){Start-Sleep -Milliseconds 150;& $Between;Start-Sleep -Milliseconds 150}
  }
  Start-Sleep -Milliseconds 30;[CapyRowPointer]::Up()
 }finally{if($Released){& $Released};[CapyRowPointer]::Cancel()}
 Park
}
function Notice-Fits([string]$Place){
 $parts=Notice-Parts
 if(!$parts.Contains('canvas-notice')){$findings.Add("$Place notice is not shown");return}
 $name=(Model).state.notice.name
 if($parts['canvas-notice'].name -ne $name){$findings.Add("$Place notice is named '$($parts['canvas-notice'].name)', not '$name'")}
 $frame=$parts['canvas-notice'].bounds;$inside=$frame;$inside.Inflate(1,1)
 $window=$root.Current.BoundingRectangle;$window.Inflate(1,1)
 if(!$window.Contains($frame)){$findings.Add("$Place notice leaves the window: $frame of $($root.Current.BoundingRectangle)")}
 foreach($id in @($parts.Keys|Where-Object {$_ -like 'canvas-notice-*'})){
  $b=$parts[$id].bounds;$key="$id|$($parts[$id].name)"
  if(!$inside.Contains($b)){$findings.Add("$Place notice clips $id '$($parts[$id].name)'")}
  elseif($id -like 'canvas-notice-action-*'){
   if(!$parts[$id].focusable){$findings.Add("$Place notice action $id takes no keyboard focus")}
   if(!$actionWidths.Contains($key)){$actionWidths[$key]=$b.Width}
   elseif($b.Width -lt $actionWidths[$key]-2){$findings.Add("$Place notice squeezes $id '$($parts[$id].name)' to $($b.Width) of $($actionWidths[$key]) px")}
  }
 }
 if(!@($parts.Keys|Where-Object {$_ -like 'canvas-notice-action-*'}).Count){$findings.Add("$Place notice shows no actions")}
 $view=Snapshot;$picture=Grab
 try{$checks["$Place notice styles"]=Notice-Presentation $parts $view $picture}finally{Save-Grab $picture ("notice-palette-"+($Place-replace' ','-')+"-$Theme")}
 if($Place-eq'German narrow'){
  $text=$parts['canvas-notice-text'].bounds
  foreach($id in @($parts.Keys|Where-Object {$_-like'canvas-notice-action-*'})){if($parts[$id].bounds.Top+1-lt$text.Bottom){throw 'The narrow German notice did not put its actions below the long text'}}
 }
}
function Footer-Fits([string]$Place){
 $footer=Find 'layer-footer' -Visible
 if(!$footer){$findings.Add("$Place shows no Layers footer");return}
 $frame=$footer.Current.BoundingRectangle;$frame.Inflate(1,1)
 $window=$root.Current.BoundingRectangle;$window.Inflate(1,1)
 foreach($id in 'layer-new','layer-new-vector','layer-delete','layer-actions'){
  $button=Find $id -Visible
  if(!$button){$findings.Add("$Place footer hides $id");continue}
  $b=$button.Current.BoundingRectangle
  if(!$frame.Contains($b) -or !$window.Contains($b)){$findings.Add("$Place footer clips $id at $b")}
 }
}
function Modes-Fit([string]$Place){
 foreach($mode in 'erase_touched_part','erase_whole_paths','erase_to_intersection'){
  $match={param($item) $item.action.command -eq $mode}.GetNewClosure()
  try{$choice=Tool-Item 'subtools' $match (Label $mode)}catch{$findings.Add("$Place offers no $(Label $mode)");continue}
  try{$button=Reveal $choice.id}catch{$findings.Add("$Place cannot show $(Label $mode): $($_.Exception.Message)");continue}
  $b=$button.Current.BoundingRectangle;$window=$root.Current.BoundingRectangle;$window.Inflate(1,1)
  if($b.IsEmpty -or !$window.Contains($b)){$findings.Add("$Place clips $(Label $mode) at $b")}
 }
}

try{
 Enter-CapyEnvironment
 $env:CAPY_STORAGE_DIR=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1'
 Start-Review 'en'
 if((Model).state.theme -ne $Theme){throw "The review opened in $((Model).state.theme) instead of $Theme"}
 $v1=New-VectorLayer
 Wait-Until {$m=Model;$m.state.brush.preset -ne $null -and $m.brush_ready} 'G-Pen is not ready' 120
 $rows=@(300,450,600)
 Stroke (Line @(400,$rows[0]) @(1200,$rows[0]))
 Expect 'pen line' @(800,$rows[0]) 'ink'
 Drag 'mouse' @(400,$rows[1]) @(1200,$rows[1]) 20
 Expect 'mouse line' @(800,$rows[1]) 'ink'
 $revision=Revision
 Drag 'touch' @(400,$rows[2]) @(1200,$rows[2]) 12
 Expect 'one finger draws nothing' @(800,$rows[2]) 'ink' $false 5
 if((Revision) -ne $revision){$findings.Add('One finger changed the drawing')}
 Stroke (Line @(400,$rows[2]) @(1200,$rows[2]))
 Expect 'third pen line' @(800,$rows[2]) 'ink'
 Capture "edges-input-$Theme" -WithModel -Composed

 $empty=@(1500,900)
 $pencil=Select-Brush 'Pencil'
 foreach($case in @(@{name='Pencil';refused=$true},@{name='G-Pen';refused=$false})){
  if($case.name -eq 'G-Pen'){$null=Select-Brush 'G-Pen'}
  foreach($device in 'pen','mouse'){
   $p=Screen-Point $empty
   if($device -eq 'pen'){[CapyRowPointer]::PenHover($p[0],$p[1]);Start-Sleep -Milliseconds 80;[CapyRowPointer]::PenHover($p[0]+3,$p[1]+2)}else{[CapyRowPointer]::Hover($p[0],$p[1]);Start-Sleep -Milliseconds 80;[CapyRowPointer]::Hover($p[0]+3,$p[1]+2)}
   Start-Sleep -Milliseconds 400
   $watch=[Diagnostics.Stopwatch]::StartNew()
   while(!($seen=(Red-Near $empty) -gt 4) -and $case.refused -and $watch.ElapsedMilliseconds -lt 3000){Start-Sleep -Milliseconds 100}
   if($seen -ne $case.refused){$findings.Add("$($case.name) by $device over a vector layer: refusal cursor $seen, expected $($case.refused)")}
   if($case.refused -and $device -eq 'pen'){Capture "edges-cursor-refused-$Theme" -WithModel -Composed}
   if($device -eq 'pen'){[CapyRowPointer]::PenLeave()}
  }
 }
 Park


 $null=Select-Brush 'Pencil'
 Refuse 'English' (Line @(400,800) @(900,800) 4)
 Notice-Fits 'English'
 $before=(Model).state.notice.id
 $pick={param($result) $result.id -like '*toggle_theme*'}
 $null=Search (Label 'toggle_theme') $pick 'Toggle theme'
 Wait-Until {(Model).state.theme -ne $Theme} 'Toggle theme did not switch the theme'
 $checks.notice_after_theme_search=[ordered]@{shown=[bool](Notice-Parts).Contains('canvas-notice');same=((Model).state.notice.id -eq $before)}
 Capture "edges-notice-switched-$Theme" -WithModel -Composed
 $null=Search (Label 'toggle_theme') $pick 'Toggle theme'
 Wait-Until {(Model).state.theme -eq $Theme} 'Toggle theme did not switch back'
 $paint=@((Rows)|Where-Object {!$_.selected -and !(Vector $_) -and !$_.paper})[0].id
 if((Notice-Parts).Contains('canvas-notice-action-new_paint_layer')){
  $count=@(Rows).Count
  Invoke "layer-$paint-name"
  Wait-Until {(Active).id -eq $paint} 'The paint layer was not selected'
  $parts=Notice-Parts
  if($parts.Contains('canvas-notice-action-new_paint_layer')){
   $b=$parts['canvas-notice-action-new_paint_layer'].bounds
   [CapyRowPointer]::Down('mouse',[int]($b.X+$b.Width/2),[int]($b.Y+$b.Height/2));Start-Sleep -Milliseconds 40;[CapyRowPointer]::Up();Park
   Start-Sleep -Milliseconds 500
   if(@(Rows).Count -ne $count){$findings.Add('A stale notice action added a layer')}
   $said=(Notice-Parts)['canvas-notice-text'].name
   if(!$said){$findings.Add('A stale notice action showed nothing')}
   Capture "edges-notice-stale-$Theme" -WithModel -Composed
  }
 }
 Invoke "layer-$v1-name"
 Wait-Until {(Active).id -eq $v1} 'The vector layer was not selected again'

 $checks.rows=$true
 $null=Select-Brush 'G-Pen'
 $labelBefore=(Active).label
 [CapyRowPointer]::DoubleClick('mouse',(Center (Control "layer-$v1-name" -Arranged))[0],(Center (Control "layer-$v1-name" -Arranged))[1])|Out-Null
 Wait-Until {Find "layer-$v1-rename" -Visible} 'Double-clicking the name did not start renaming'
 $rename=Control "layer-$v1-rename";$rename.SetFocus()
 $rename.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('Ink');Key 0x0D
 Wait-Until {@((Rows)|Where-Object id -eq $v1)[0].label -eq 'Ink'} "Renaming $labelBefore did not take"
 Invoke "layer-$v1-visibility"
 Expect 'hiding the vector layer' @(800,$rows[0]) 'ink' $false
 Invoke "layer-$v1-visibility"
 Expect 'showing the vector layer' @(800,$rows[0]) 'ink'
 $vectors=@(Vectors).Count
 $at=Center (Control "layer-$v1-name" -Arranged)
 [CapyRowPointer]::RightClick($at[0],$at[1])
 $owned={param($name) [System.Windows.Automation.AutomationElement]::RootElement.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.AndCondition]::new([System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,$name),[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty,$review.Id)))}
 $menu=@{item=$null}
 Wait-Until {$menu.item=& $owned 'Organize';$null -ne $menu.item} 'The vector row menu did not offer Organize'
 if(!(& $owned (Label 'rasterize_layer'))){$findings.Add("The vector row menu does not offer $(Label 'rasterize_layer')")}
 Capture "edges-row-menu-$Theme" -WithModel
 $pattern=$null
 if($menu.item.TryGetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern,[ref]$pattern)){$pattern.Expand()}else{$menu.item.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()}
 Wait-Until {$menu.item=& $owned 'Duplicate';$null -ne $menu.item} 'The Organize submenu did not offer Duplicate'
 $menu.item.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
 Wait-Until {@(Vectors).Count -eq $vectors+1} 'Duplicating the vector layer did not add a vector layer'
 $copy=(Active).id
 $order=@(Rows|ForEach-Object id)
 $grip=Center (Control "layer-$copy-drag" -Arranged);$target=(Control "layer-$v1-name" -Arranged).Current.BoundingRectangle
 [CapyRowPointer]::Down('mouse',$grip[0],$grip[1])
 try{for($i=1;$i -le 10;$i++){Start-Sleep -Milliseconds 30;[CapyRowPointer]::Move($grip[0],[int]($grip[1]+($target.Bottom-6-$grip[1])*$i/10))};Start-Sleep -Milliseconds 60;[CapyRowPointer]::Up()}finally{[CapyRowPointer]::Cancel()}
 Park
 Wait-Until {(@(Rows|ForEach-Object id) -join ',') -ne ($order -join ',')} 'Dragging the vector row did not reorder it'
 $after=@(Rows|ForEach-Object id)
 if([array]::IndexOf($after,$copy) -lt [array]::IndexOf($after,$v1)){$findings.Add("Dragging the copy down left it above: $($order -join ',') -> $($after -join ',')")}
 Chord 0x5A
 Wait-Until {(@(Rows|ForEach-Object id) -join ',') -eq ($order -join ',')} 'Ctrl+Z did not restore the order'
 Invoke "layer-$copy-name";Wait-Until {(Active).id -eq $copy} 'The copy was not selected'
 Invoke 'layer-delete'
 Wait-Until {@(Vectors).Count -eq $vectors} 'Deleting the copy did not remove it'
 Invoke "layer-$v1-name";Wait-Until {(Active).id -eq $v1} 'The vector layer was not selected after the delete'
 Capture "edges-rows-$Theme" -WithModel -Composed

 Select-Tool 'move'
 Chord 0x41
 Wait-Until {(Enabled 'deselect') -and (Tool-Name) -eq 'move'} 'Ctrl+A did not select the paths and switch to Move'
 Key 0x1B
 Wait-Until {!(Enabled 'deselect')} 'Escape did not clear the paths'
 Drag 'mouse' @(800,$rows[0]) @(800,$rows[0]) 1
 Wait-Until {Enabled 'deselect'} 'A click did not select the path'
 Key 0x0D
 Wait-Until {(Tool-Name) -eq 'edit_points'} 'Enter did not open Edit Points'
 Key 0x1B
 Wait-Until {(Tool-Name) -eq 'move' -and (Enabled 'deselect')} 'Escape did not return to Move'
 Key 0x2E
 Expect 'Delete removes the selected path' @(800,$rows[0]) 'ink' $false
 Chord 0x5A
 Expect 'Ctrl+Z restores the path' @(800,$rows[0]) 'ink'
 $docked=[bool](Find 'layer-footer' -Visible)
 $stall=@{thread=[IntPtr]::Zero}
 $resume={if($stall.thread -ne [IntPtr]::Zero){Start-Sleep -Milliseconds 300;[CapyThreadStall]::Resume($stall.thread);$stall.thread=[IntPtr]::Zero}}
 Drag 'mouse' @(800,$rows[1]) @(800,($rows[1]+100)) 8 4 {$stall.thread=[CapyThreadStall]::Suspend($drawingWindow);Key 0x1B} {Key 0x1B;& $resume}
 Expect 'Escape mid-drag keeps the path' @(800,$rows[1]) 'ink'
 Expect 'Escape mid-drag leaves no preview' @(800,($rows[1]+100)) 'ink' $false
 Wait-Until {!(Enabled 'deselect')} 'A second Escape after the release did not clear the path selection'
 if($docked -and !(Find 'layer-footer' -Visible)){
  $findings.Add('Escape mid-drag also collapsed the docked Layers column')
  Invoke 'column-icon-layers';Wait-Until {Find 'layer-footer' -Visible} 'The Layers column did not reopen'
  (Control 'drawing-canvas').SetFocus()
 }
 $order=@(Rows|ForEach-Object id)
 Start-Sleep -Milliseconds 500
 Drag 'mouse' @(800,$rows[1]) @(800,($rows[1]+100)) 8 4 {$stall.thread=[CapyThreadStall]::Suspend($drawingWindow);Chord 0x5A} $resume
 Expect 'the drag that saw Ctrl+Z lands' @(800,($rows[1]+100)) 'ink'
 $changed=@(Rows|ForEach-Object id)
 if(($changed -join ',') -ne ($order -join ',')){$findings.Add("Ctrl+Z during a path drag undid an earlier step: layers $($order -join ',') -> $($changed -join ',')")}
 if(!(Inked @(800,$rows[2]))){$findings.Add('Ctrl+Z during a path drag undid an earlier line')}
 Chord 0x5A
 Expect 'Ctrl+Z after the drag undoes it' @(800,$rows[1]) 'ink'
 foreach($extra in @($changed|Where-Object {$_ -notin $order})){
  Invoke "layer-$extra-name";Wait-Until {(Active).id -eq $extra} 'The restored layer was not selected'
  Invoke 'layer-delete';Wait-Until {!@(Rows|Where-Object id -eq $extra).Count} 'The restored layer was not removed'
 }
 Invoke "layer-$v1-name";Wait-Until {(Active).id -eq $v1} 'The vector layer was not selected after the undo check'
 if(!(Inked @(800,$rows[2]))){Select-Tool 'pen';$null=Select-Brush 'G-Pen';Stroke (Line @(400,$rows[2]) @(1200,$rows[2]))}
 Capture "edges-keys-$Theme" -WithModel -Composed
 $checks.keys=$true
 Select-Tool 'move'
 Key 0x1B
 $a=Screen-Point @(800,$rows[0])
 [CapyRowPointer]::Down('mouse',$a[0],$a[1])
 try{
  for($i=1;$i -le 4;$i++){Start-Sleep -Milliseconds 16;[CapyRowPointer]::Move($a[0]+$i*5,$a[1]+$i*5)}
  $pattern=$root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern)
  $pattern.SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Normal)
  Start-Sleep -Milliseconds 800
  [CapyRowPointer]::Move($a[0]+30,$a[1]+30)
  $pattern.SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Maximized)
  Start-Sleep -Milliseconds 800
  $script:projection=$null;Take-Projection
  $b=Screen-Point @(880,($rows[0]+60))
  [CapyRowPointer]::Move($b[0],$b[1]);Start-Sleep -Milliseconds 60;[CapyRowPointer]::Up()
 }finally{[CapyRowPointer]::Cancel()}
 Park
 $landed=Inked @(880,($rows[0]+60));$left=!(Inked @(800,$rows[0]))
 if($landed -ne $left){$findings.Add("A drag across a window resize landed $landed, left its old place $left")}
 $checks.resize_drag=$(if($landed){'follows the pointer'}else{'cancelled'})
 if($landed){Chord 0x5A}
 Capture "edges-resize-drag-$Theme" -WithModel -Composed

 Select-Tool 'eraser'
 $whole=Tool-Item 'subtools' {param($item) $item.action.command -eq 'erase_whole_paths'} (Label 'erase_whole_paths')
 $null=Choose-Tool-Item $whole -Acknowledged
 Wait-Until {(Model).state.settings.vector_eraser -eq 'whole_paths'} 'Erase whole paths was not chosen'
 $file=Join-Path $run 'vector-edges.capy'
 Save-ProjectAs $file
 Open-Project $file
 Wait-Until {$m=Model;!$m.state.document_file.busy -and @($m.state.layers|Where-Object content_icon -eq 'layer-path-symbolic').Count -ge 1} 'The saved vector drawing did not reopen' 90
 Fit-Canvas;Park;$script:projection=$null;Take-Projection
 Expect 'the reopened line' @(800,$rows[0]) 'ink' $true 60
 Expect 'the reopened third line' @(800,$rows[2]) 'ink' $true 60
 Capture "edges-reopened-$Theme" -WithModel -Composed
 Stop-Review
 $settings=Get-Content -LiteralPath (Settings-File) -Raw|ConvertFrom-Json
 if($settings.vector_eraser -ne 'whole_paths'){$findings.Add("The eraser mode was not saved: $($settings.vector_eraser)")}

 Start-Review 'de'
 if((Model).state.settings.vector_eraser -ne 'whole_paths'){$findings.Add('The eraser mode did not survive a restart')}
 $null=New-VectorLayer
 Footer-Fits 'German maximized'
 Select-Tool 'eraser';Modes-Fit 'German maximized'
 Select-Preset 'Bleistift' $pencil
 Refuse 'German' (Line @(400,500) @(900,500) 4)
 Notice-Fits 'German maximized'
 Capture "edges-german-$Theme" -WithModel -Composed
 $pattern=$root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern)
 $maximum=$root.Current.BoundingRectangle;$windowScale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
 $narrowWidth=[Math]::Min(1100*$windowScale,$maximum.Width*.9);$narrowHeight=[Math]::Min(1000*$windowScale,$maximum.Height*.9)
 $pattern.SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Normal)
 $transform=$root.GetCurrentPattern([System.Windows.Automation.TransformPattern]::Pattern)
 $transform.Resize($narrowWidth,$narrowHeight);$transform.Move(($maximum.X+($maximum.Width-$narrowWidth)/2),($maximum.Y+($maximum.Height-$narrowHeight)/2))
 Wait-Until {$c=(Model).state.camera;$canvas=(Control 'drawing-canvas' -Arranged).Current.BoundingRectangle;$root.Current.BoundingRectangle.Width -lt 1200*$windowScale -and $c.work_area[2] -gt 0 -and $c.work_area[3] -gt 0 -and [Math]::Abs($c.viewport[0]-$canvas.Width) -lt 2} 'The narrow window did not acknowledge its usable canvas'
 Start-Sleep -Milliseconds 800
 Fit-Canvas;Park;$script:projection=$null;Take-Projection
 Footer-Fits 'German narrow'
 Select-Tool 'eraser';Modes-Fit 'German narrow'
 Select-Preset 'Bleistift' $pencil
 Refuse 'narrow German' (Line @(800,600) @(1400,600) 4)
 Notice-Fits 'German narrow'
 Capture "edges-german-narrow-$Theme" -WithModel -Composed
 Stop-Review
 if((Get-ChildItem -LiteralPath $run -Filter 'stderr-*.log'|Measure-Object -Property Length -Sum).Sum){throw 'Native stderr requires inspection'}
 if($findings.Count){throw "Journey findings: $($findings -join '; ')"}
 $checks.evidence=$run
 $checks|ConvertTo-Json -Depth 6|Tee-Object -FilePath (Join-Path $run 'results.json')
}catch{
 if($review -and !$review.HasExited -and $root){try{Capture 'failure' -WithModel -Composed}catch{}}
 [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{
 try{[CapyRowPointer]::Dispose()}catch{}
 ConvertTo-Json -InputObject @($observations) -Depth 6|Set-Content -LiteralPath (Join-Path $run 'pixels.json')
 ConvertTo-Json -InputObject @($findings)|Set-Content -LiteralPath (Join-Path $run 'findings.json')
 Exit-CapyEnvironment
}
