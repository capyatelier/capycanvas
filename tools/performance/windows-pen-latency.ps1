param(
 [Parameter(Mandatory)][string]$Executable,
 [Parameter(Mandatory)][string]$Project,
 [Parameter(Mandatory)][string]$OutputDirectory,
 [string]$PresentMon=(Join-Path $env:USERPROFILE '.local/tools/presentmon/2.5.1/PresentMon-2.5.1-x64.exe'),
 [int]$Diameter=18,
 [int]$Seconds=10,
 [switch]$SkipPresentMon,
 [ValidateSet('hand','zoom','rotate_view')][string]$NavigationMode,
 [ValidateSet('move','scale','rotate','placement')][string]$ObjectMotionMode,
 [string]$ObjectPreflight
)
$ErrorActionPreference='Stop'
if($NavigationMode -and $ObjectMotionMode){throw 'Choose one motion workload'}
if($ObjectMotionMode -and (!$ObjectPreflight -or !$SkipPresentMon)){throw 'Object motion requires its preflight and -SkipPresentMon'}
if($NavigationMode -and !$SkipPresentMon){throw 'Navigation capture uses bounded host traces; specify -SkipPresentMon'}
. (Join-Path $PSScriptRoot '../../apps/layer-windows/scripts/CapyUia.ps1')
$CapyWaitSeconds=45
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$Executable=(Resolve-Path $Executable).Path;$Project=(Resolve-Path $Project).Path
$directory=Split-Path -Parent $Executable
if($ObjectMotionMode){
 $ObjectPreflight=(Resolve-Path -LiteralPath $ObjectPreflight).Path
 $objectPlan=Get-Content -LiteralPath $ObjectPreflight -Raw|ConvertFrom-Json -Depth 100
 $projectHash=if($ObjectMotionMode -eq 'placement'){$objectPlan.placement_base_sha256}else{$objectPlan.project_sha256}
 if($objectPlan.schema -ne 1 -or (Get-FileHash $Executable).Hash -ne $objectPlan.exe_sha256 -or (Get-FileHash $Project).Hash -ne $projectHash -or (Get-FileHash $objectPlan.project).Hash -ne $objectPlan.project_sha256 -or (Get-FileHash $objectPlan.placement_base).Hash -ne $objectPlan.placement_base_sha256 -or (Get-FileHash (Join-Path $directory 'layer_windows.dll')).Hash -ne $objectPlan.dll_sha256 -or (Get-FileHash $objectPlan.image).Hash -ne $objectPlan.image_sha256){throw 'Object preflight source or project identity changed'}
 Add-Type -Path (Join-Path $repo 'apps/layer-windows/scripts/PackageFixture.cs')
 Add-Type -AssemblyName System.Drawing
 . (Join-Path $repo 'apps/layer-windows/scripts/ObjectFixture.ps1')
 if((Get-FileHash (Join-Path $repo 'crates/layer-ui/src/object_editing.rs')).Hash -ne $objectPlan.object_source_sha256 -or (Get-FileHash (Join-Path $repo 'crates/layer-ui/src/rulers.rs')).Hash -ne $objectPlan.ruler_source_sha256){throw 'Object handle geometry source changed since preflight'}
 $objectSource=Package $objectPlan.project;$objectBase=Package $Project
 $bytes=(Get-Item $objectPlan.project).Length*24+32MB
 if((Get-PSDrive -Name ([IO.Path]::GetPathRoot([IO.Path]::GetFullPath($OutputDirectory)).TrimEnd('\').TrimEnd(':'))).Free -lt $bytes+256MB){throw 'Insufficient space for bounded Object evidence; preserve existing data and recover storage before measuring'}
}

$OutputDirectory=[IO.Path]::GetFullPath($OutputDirectory)
if(Test-Path $OutputDirectory){throw 'Use a fresh result directory'}
New-Item -ItemType Directory $OutputDirectory|Out-Null
try{
 Enter-CapyEnvironment @('CAPY_LATENCY_TRACE')
 $env:CAPY_STORAGE_DIR=Join-Path $OutputDirectory 'profile'
 $env:CAPY_LATENCY_TRACE='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
 if($NavigationMode -or $ObjectMotionMode){@{language=@{Explicit='en'};theme=$(if($ObjectMotionMode){$objectPlan.theme}else{'dark'})}|ConvertTo-Json|Set-Content (Settings-File)}
 $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError (Join-Path $OutputDirectory 'stderr.log')
}finally{Exit-CapyEnvironment}
$review.Id|Set-Content (Join-Path $OutputDirectory 'process-id.txt')
Wait-Until {$script:drawing=Owned-DrawingWindow $review;$null -ne $drawing} 'The owned drawing window did not open' 60
[CapyWindowApi]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
$window=$drawing.Handle;$root=$drawing.Root
if($NavigationMode -or $ObjectMotionMode){$root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Maximized)}
Wait-Until {$script:probe=Trace-File 'presentation-probe';$null -ne $probe} 'Renderer did not become ready' 90
Open-Project $Project
$projectTitle=[IO.Path]::GetFileName($Project)+' · Capy Canvas'
Wait-Until {
 $canvas=Find 'drawing-canvas' -Visible;$field=Find 'tool-setting-size'
 $root.Current.Name -eq $projectTitle -and $canvas -and $canvas.Current.IsEnabled -and ($ObjectMotionMode -or ($field -and $field.Current.IsEnabled)) -and !(Find 'canvas-status' -Visible)
} 'The requested drawing did not become ready for pen input' 90
if($NavigationMode -or $ObjectMotionMode){
 Add-Type -Path (Join-Path $repo 'apps/layer-windows/scripts/RowPointerDriver.cs')
 Add-Type -AssemblyName WindowsBase
 $run=$OutputDirectory;$CapyPopups=$true
 $navigationLabel=@{hand='Hand';zoom='Zoom';rotate_view='Rotate view'}[$NavigationMode]
 function Motion-Canvas {
  $canvas=Control 'drawing-canvas' -Arranged
  $excluded=[Collections.Generic.HashSet[string]]::new()
  foreach($node in $canvas.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition)){$null=$excluded.Add(($node.GetRuntimeId() -join ':'))}
  $node=$canvas;$bridge=$null
  while($node){
   $null=$excluded.Add(($node.GetRuntimeId() -join ':'))
   if(!$bridge -and $node.Current.NativeWindowHandle -and !$node.Current.IsOffscreen){$bridge=$node}
   if(($node.GetRuntimeId() -join ':') -eq ($root.GetRuntimeId() -join ':')){break}
   $node=[System.Windows.Automation.TreeWalker]::RawViewWalker.GetParent($node)
  }
  if(!$bridge -or !$node -or $bridge.Current.ProcessId -ne $review.Id){throw 'The canvas has no owned visible native ancestor'}
  $parent=[System.Windows.Automation.TreeWalker]::RawViewWalker.GetParent($canvas)
  $parentId=$parent.GetRuntimeId() -join ':'
  $workspace=Control 'canvas-view-info' -Arranged
  while($workspace){
   $container=[System.Windows.Automation.TreeWalker]::RawViewWalker.GetParent($workspace)
   if($container -and ($container.GetRuntimeId() -join ':') -eq $parentId){break}
   $workspace=$container
  }
  if(!$workspace -or ($workspace.GetRuntimeId() -join ':') -eq ($canvas.GetRuntimeId() -join ':') -or $workspace.Current.FrameworkId -ne 'XAML' -or $workspace.Current.ControlType -ne [System.Windows.Automation.ControlType]::Group -or $workspace.Current.AutomationId -or $workspace.Current.NativeWindowHandle -or $workspace.Current.Name -ne $parent.Current.Name){throw 'The transparent workspace container could not be identified'}
  $null=$excluded.Add(($workspace.GetRuntimeId() -join ':'))
  $bounds=$canvas.Current.BoundingRectangle
  $obstacles=@(foreach($item in $root.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition)){
   if($excluded.Contains(($item.GetRuntimeId() -join ':')) -or $item.Current.IsOffscreen){continue}
   $rect=$item.Current.BoundingRectangle
   if(!$rect.IsEmpty -and $rect.Width -gt 0 -and $rect.Height -gt 0 -and $bounds.IntersectsWith($rect)){
    [pscustomobject]@{id=$item.Current.AutomationId;name=$item.Current.Name;bounds=$rect}
   }
  })
  [pscustomobject]@{bounds=$bounds;bridge_hwnd=$bridge.Current.NativeWindowHandle;obstacles=$obstacles}
 }
 function Motion-Point($Region,[Windows.Point]$Point) {
  if(!$Region.bounds.Contains($Point)){throw "Motion point $Point is outside the arranged canvas"}
  foreach($item in $Region.obstacles){if($item.bounds.Contains($Point)){throw "Motion point $Point is covered by '$($item.id)' ('$($item.name)')"}}
  $hit=[System.Windows.Automation.AutomationElement]::FromPoint($Point);$node=$hit
  while($node -and $node.Current.NativeWindowHandle -ne $Region.bridge_hwnd){$node=[System.Windows.Automation.TreeWalker]::RawViewWalker.GetParent($node)}
  if(!$node -or $node.Current.ProcessId -ne $review.Id){throw "Motion point $Point is outside the owned canvas native subtree"}
  $hit
 }
 function Navigation-Focus {
  [CapyWindowApi]::SetForegroundWindow($window)|Out-Null
  (Control 'drawing-canvas').SetFocus()
  Wait-Until {(Control 'drawing-canvas').Current.HasKeyboardFocus -and [CapyRowPointer]::GetForegroundWindow() -eq $window} 'Navigation canvas did not acquire owned keyboard focus'
 }
 function Navigation-Command([string]$Label) {
  Navigation-Focus;[CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),0x4b)
  Wait-Until {$search=Find 'command-search' -Visible;$search -and $search.Current.HasKeyboardFocus} 'Command search did not open'
  (Control 'command-search').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($Label)
  Wait-Until {$row=Find 'command-result-0' -Visible;$row -and $row.Current.Name -eq $Label -and $row.Current.IsEnabled} "Command search did not resolve $Label"
  Invoke-Id 'command-result-0'
  Wait-Until {!(Find 'command-search' -Visible)} "Command search did not close after $Label"
 }
 function Navigation-State {
  $chosen=@{items=@()}
  Wait-Until {$chosen.items=@($root.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,$navigationLabel))|Where-Object {$_.Current.AutomationId -like 'tool-subtool-*' -and $_.Current.ItemStatus -eq 'Selected'});$chosen.items.Count -eq 1} 'The selected navigation tool is not exposed by its native tool-set state'
  $selected=$chosen.items[0]
  $entry=& (Join-Path $repo 'apps/layer-windows/scripts/open-application-menu.ps1') -Root $root -Name 'Edit' -PassThru
  $undo=Control 'undo' -Arranged
  $redo=Control 'redo' -Arranged
  $state=[ordered]@{title=$root.Current.Name;tool=$selected.Current.Name;tool_id=$selected.Current.AutomationId;tool_status=$selected.Current.ItemStatus;undo=$undo.Current.IsEnabled;redo=$redo.Current.IsEnabled;camera_readout=(Control 'canvas-camera').Current.Name}
  [CapyRowPointer]::Key([uint32]$review.Id,0x1b)
  Wait-Until {$item=Find 'undo' -Visible;!$item} 'Edit commands did not close'
  $parentOpen=try{$entry.Current.ControlType -eq [System.Windows.Automation.ControlType]::MenuItem -and !$entry.Current.IsOffscreen}catch [System.Windows.Automation.ElementNotAvailableException]{$false}
  if($parentOpen){[CapyRowPointer]::Key([uint32]$review.Id,0x1b);Wait-Until {try{$entry.Current.IsOffscreen}catch [System.Windows.Automation.ElementNotAvailableException]{$true}} 'The compact application menu did not close'}
  if($state.title -ne $projectTitle -or $state.undo -or $state.redo){throw 'Navigation changed the accessible clean drawing or empty Undo/Redo history'}
  Navigation-Focus
  [pscustomobject]$state
 }
 if($NavigationMode){
  Navigation-Command $navigationLabel
  Navigation-Command 'Reset view'
  Wait-Until {$tool=Find $navigationLabel -Name -Visible;$tool -and !(Find 'canvas-status' -Visible)} 'Navigation did not become ready'
 }else{
  [CapyRowPointer]::Initialize([uint32]$review.Id)
  $rootBounds=(Control 'drawing-canvas' -Arranged).Current.BoundingRectangle
  if((@($rootBounds.X,$rootBounds.Y,$rootBounds.Width,$rootBounds.Height) -join ',') -ne ($objectPlan.canvas_bounds -join ',') -or [CapyRowPointer]::GetDpiForWindow($window) -ne $objectPlan.dpi){throw 'Object preflight canvas bounds or DPI changed'}
  $objectProjection=@{camera=$objectPlan.camera;bounds=$rootBounds}
  $objectSamples=@($objectPlan.sample_points)
  $objectSave=0
  function Object-Pixels {
   if([CapyRowPointer]::GetForegroundWindow() -ne $window -or (Control 'drawing-canvas').Current.BoundingRectangle -ne $rootBounds -or (Control 'canvas-camera').Current.Name -ne $objectPlan.camera_readout){throw 'Object measurement lost its qualified camera or owned window'}
   Sample-ObjectArtwork $rootBounds $objectSamples
  }
  function Object-Park {
   $b=(Control 'settings-button' -Arranged).Current.BoundingRectangle
   [CapyRowPointer]::Hover([int]($b.X+$b.Width/2),[int]($b.Y+$b.Height/2))
  }
  function Object-Save([string]$Label){
   $script:objectSave++;$path=Join-Path $OutputDirectory ("object-$objectSave-$Label.capy")
   Save-ProjectAs $path -NativeSaved {param($Saved)try{$null=Package $Saved;$true}catch{$false}}
   Navigation-Focus;Object-Park
   Package $path
  }
  function Object-Select {
   $row=Control ($objectPlan.layer_label+' layer row') -Name -Arranged
   $id=$row.Current.AutomationId -replace '^layer-row-',''
   $name=Control ('layer-'+$id+'-name') -Arranged;$b=$name.Current.BoundingRectangle
   [CapyRowPointer]::Down('mouse',[int]($b.X+$b.Width/2),[int]($b.Y+$b.Height/2));[CapyRowPointer]::Up()
   Navigation-Command 'Operation'
   Wait-Until {(Control ('layer-'+$id+'-selection')).Current.ItemStatus -eq 'Selected'} 'The measured Object layer did not select'
   Object-Park
  }
  function Object-History([string]$Command,$Expected,[string]$Pixels){
   Navigation-Command $Command
   $saved=Object-Save $Command
   if((Identity $saved) -ne (Identity $Expected)){throw "$Command did not restore the exact Object graph in one step"}
   Wait-Until {(Object-Pixels) -eq $Pixels} "$Command did not restore the exact composed Object artwork"
  }
  function Object-Prepare {
   if($ObjectMotionMode -eq 'placement'){
    Navigation-Command 'Operation';Object-Park;$script:placementPixels=Wait-StablePixels {Object-Pixels}
    Open-Project $objectPlan.image -Command import_image
    Wait-Until {$apply=Find 'canvas-bar-apply_transform' -Visible;$apply -and $apply.Current.IsEnabled} 'Image import did not enter placement'
   }else{
    Object-Select
    Wait-Until {(Object-Pixels) -eq $objectPlan.initial_pixels} 'The measured image does not match its preflight geometry and pixels'
    if($ObjectMotionMode -ne 'move'){Navigation-Command 'Transform'}
   }
   Navigation-Focus;Object-Park
  }
  $corners=@(foreach($at in @(@(0,0),@(1,1),@(.5,.5),@(.5,0))){,(Screen-Point (Image-Point $objectSource $at[0] $at[1]) $objectProjection)})
  $width=$corners[1][0]-$corners[0][0];$height=$corners[1][1]-$corners[0][1]
  if($width -lt 160 -or $height -lt 120 -or $objectPlan.camera.rotation -ne 0 -or $objectPlan.camera.flipped[0] -or $objectPlan.camera.flipped[1]){throw 'Object path requires a sufficiently sized unrotated preflight'}
  $objectPath=@{cx=0;cy=0;rx=[int]($width*.28);ry=[int]($height*.25);start=0.0;sweep=1.5*[Math]::PI}
  $start=$corners[2]
  if($ObjectMotionMode -eq 'scale'){$start=$corners[1];$objectPath.rx=[int]($width*.15);$objectPath.ry=[int]($height*.15)}
  if($ObjectMotionMode -eq 'rotate'){
   $objectPath.cx=$corners[2][0];$objectPath.cy=$corners[2][1];$objectPath.rx=$objectPath.ry=[int]($height/2+30*$objectPlan.dpi/96);$objectPath.start=-[Math]::PI/2
  }else{$objectPath.cx=$start[0]-$objectPath.rx;$objectPath.cy=$start[1]}
  $objectRegion=Motion-Canvas
  $objectRegion|ConvertTo-Json -Depth 8|Set-Content (Join-Path $OutputDirectory 'object-path-region.json')
  foreach($step in 0..1439){
   $angle=$objectPath.start+$objectPath.sweep*$step/1439
   $at=[Windows.Point]::new($objectPath.cx+[int][Math]::Truncate($objectPath.rx*[Math]::Cos($angle)),$objectPath.cy+[int][Math]::Truncate($objectPath.ry*[Math]::Sin($angle)))
   $null=Motion-Point $objectRegion $at
  }
 }
 $brushName=$null
}else{
Invoke-Id 'tool-subtool-0'
$brushName=(Control 'tool-subtool-0').Current.Name
if($brushName -notmatch 'G[- ]?Pen'){throw "Expected G-Pen, got $brushName"}
$size=Control 'tool-setting-size';$size.SetFocus();$size.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue([string]$Diameter)
(Control 'tool-setting-opacity').SetFocus();Fit-Canvas
Wait-Until {$committedSize=Find 'tool-setting-size';$committedSize -and $committedSize.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value -eq "$Diameter.0 px"} 'Brush size was not committed'
[CapyWindowApi]::SetForegroundWindow($window)|Out-Null
Start-Sleep -Seconds 3
}
$canvas=Control 'drawing-canvas' -Arranged
Wait-Until {$canvas.Current.IsEnabled -and !(Find 'canvas-status' -Visible)} 'The canvas stopped being ready before the stroke'
$bounds=$canvas.Current.BoundingRectangle
$cx=[int]($bounds.X+$bounds.Width/2);$cy=[int]($bounds.Y+$bounds.Height/2)
if(!$ObjectMotionMode -and ($cx-200 -le $bounds.Left -or $cx+200 -ge $bounds.Right -or $cy-120 -le $bounds.Top -or $cy+120 -ge $bounds.Bottom)){throw 'The pen path exceeds the arranged canvas'}
$meta=Read-Snapshot $probe
$capture=[ordered]@{process_id=$review.Id;brush_name=$brushName;diameter=$Diameter;project_sha256=(Get-FileHash $Project).Hash;exe_sha256=(Get-FileHash $Executable).Hash;dll_sha256=(Get-FileHash (Join-Path $directory 'layer_windows.dll')).Hash;surface=$meta;seconds=$Seconds;rate_hz=240;center=@($cx,$cy);radii=@(200,120);qpc_frequency=[Diagnostics.Stopwatch]::Frequency;display=(& (Join-Path $repo 'apps/layer-windows/scripts/probe-displays.ps1')|ConvertFrom-Json)}
if($NavigationMode){$capture.navigation=[ordered]@{mode=$NavigationMode;contacts=@();observations=@();ui_trace_enabled=$false;observation_scope='Native selected tool, clean title and disabled Undo/Redo before/after each contact; exact camera translation/work-area/session checkpoint unavailable';raw_visual_review='required'}}
if($ObjectMotionMode){$capture.object_motion=[ordered]@{mode=$ObjectMotionMode;preflight_sha256=(Get-FileHash $ObjectPreflight).Hash;scope=$objectPlan.scope;path=$objectPath;contacts=@();ui_trace_enabled=$false;raw_visual_review='required'};if(Trace-File 'ui-state'){throw 'Object motion forbids per-frame UI tracing'}}
$capture|ConvertTo-Json -Depth 12|Set-Content (Join-Path $OutputDirectory 'capture.json')
Add-Type -Path (Join-Path $PSScriptRoot 'WindowsPenMotion.cs')
if(!$SkipPresentMon){
$pm=Start-Process -FilePath $PresentMon -ArgumentList @('--process_id',$review.Id,'--timed',($Seconds+5),'--terminate_after_timed','--no_console_stats','--no_track_input','--v1_metrics','--qpc_time_ms','--session_name',"CapyPen-$($review.Id)",'--output_file',('"'+(Join-Path $OutputDirectory 'presents.csv')+'"')) -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $OutputDirectory 'presentmon.log') -RedirectStandardError (Join-Path $OutputDirectory 'presentmon-error.log')
Start-Sleep -Seconds 1
if($pm.HasExited -and $pm.ExitCode -ne 0){throw 'PresentMon capture could not start; inspect its log'}
}
if($NavigationMode){
 $pathHits=@{};$navigationRegion=Motion-Canvas
 $navigationRegion|ConvertTo-Json -Depth 8|Set-Content (Join-Path $OutputDirectory 'navigation-path-region.json')
 foreach($step in 0..239){
  $angle=2*[Math]::PI*$step/240;$point=[Windows.Point]::new($cx+[int][Math]::Truncate(200*[Math]::Cos($angle)),$cy+[int][Math]::Truncate(120*[Math]::Sin($angle)))
  $hit=Motion-Point $navigationRegion $point
  $pathHits[$hit.Current.AutomationId]=$hit.Current.Name
 }
 $capture.navigation.path_hits=$pathHits
 $capture.seconds=19;$capture.diameter=$null
 $combined=[Collections.Generic.List[string]]::new();$combined.Add('index,qpc_before,qpc_after,x,y');$offset=0
 foreach($contact in 0..3){
  Navigation-Command 'Reset view'
  $before=Navigation-State
  Capture ("navigation-$contact-before") -Composed
  Navigation-Focus
  $duration=if($contact -eq 0){1}else{6};$file="injected-$contact.csv"
  [WindowsPenMotion]::Run([uint32]$review.Id,$cx,$cy,200,120,$duration,240,(Join-Path $OutputDirectory $file))
  $after=Navigation-State
  Capture ("navigation-$contact-after") -Composed
  $rows=@(Import-Csv (Join-Path $OutputDirectory $file))
  if($rows.Count -ne $duration*240+1){throw 'Navigation injection did not complete its contact'}
  foreach($row in $rows){$combined.Add("$offset,$($row.qpc_before),$($row.qpc_after),$($row.x),$($row.y)");$offset++}
  $capture.navigation.contacts+=@{first_index=$offset-$rows.Count;count=$rows.Count;seconds=$duration;measured=$contact -gt 0;file=$file}
  $capture.navigation.observations+=@{contact=$contact;before=$before;after=$after}
  $capture|ConvertTo-Json -Depth 12|Set-Content (Join-Path $OutputDirectory 'capture.json')
 }
 [IO.File]::WriteAllLines((Join-Path $OutputDirectory 'injected.csv'),$combined)
 if((Get-FileHash $Project).Hash -ne $capture.project_sha256){throw 'The source drawing changed during navigation'}
 if(Trace-File 'ui-state'){throw 'Per-frame UI tracing invalidates this navigation measurement'}
}elseif($ObjectMotionMode){
 $capture.seconds=19;$capture.diameter=$null
 $combined=[Collections.Generic.List[string]]::new();$combined.Add('index,qpc_before,qpc_after,x,y');$offset=0
 foreach($contact in 0..3){
  Object-Prepare
  $before=Wait-StablePixels {Object-Pixels};Capture ("object-$contact-before") -Composed
  Navigation-Focus
  $duration=if($contact -eq 0){1}else{6};$file="injected-$contact.csv"
  [WindowsPenMotion]::Run([uint32]$review.Id,$objectPath.cx,$objectPath.cy,$objectPath.rx,$objectPath.ry,$duration,240,(Join-Path $OutputDirectory $file),$objectPath.sweep,$objectPath.start)
  Object-Park;Wait-Until {Opaque-ImageChanged $before (Object-Pixels)} 'Object contact did not move opaque artwork'
  $after=Wait-StablePixels {Object-Pixels};Capture ("object-$contact-after") -Composed
  if($ObjectMotionMode -eq 'placement'){Invoke-Id 'canvas-bar-apply_transform';Wait-Until {!(Find 'canvas-bar-apply_transform' -Visible)} 'Placement did not apply'}
  $edited=Object-Save 'edited';$committedPixels=Wait-StablePixels {Object-Pixels}
  if($ObjectMotionMode -eq 'placement'){
   $added=@(Object-Layers $edited|Where-Object id -NotIn @(Object-Layers $objectBase).id)
   if($added.Count -ne 1){throw 'Placement did not add one Object layer'}
   $object=@($edited.objects|Where-Object id -eq $added[0].data.content.objects.ref)[0]
   $image=@($edited.objects|Where-Object id -eq $object.data.image.ref)[0]
   if(($image.data.extent -join ',') -ne ($objectPlan.source_extent -join ',')){throw 'Placement changed source extent'}
   if((Json (Effective-Affine $edited $added[0])) -eq (Json (Effective-Affine $objectSource (Front-Layer $objectSource)))){throw 'Placement did not preserve its moved affine'}
   $originalIds=@($objectBase.objects.id);$normalized=(Json $edited)|ConvertFrom-Json -Depth 100
   (Root-Stack $normalized).data.entries=@((Root-Stack $normalized).data.entries|Where-Object {$_.ref -ne $added[0].id})
   $normalized.objects=@($normalized.objects|Where-Object id -In $originalIds)
   if((Identity $normalized) -ne (Identity $objectBase)){throw 'Placement changed existing artwork'}
   Navigation-Command 'Undo';$restored=Object-Save 'undo'
   if((Identity $restored) -ne (Identity $objectBase)){throw 'Placement Undo did not restore its exact source graph'}
   $basePixels=Wait-StablePixels {Object-Pixels}
   if($basePixels -ne $placementPixels){throw 'Placement Undo did not restore exact composed baseline pixels'}
   Navigation-Command 'Redo';$redone=Object-Save 'redo'
   if((Identity $redone) -ne (Identity $edited)){throw 'Placement Redo did not restore its exact graph'}
   Object-Park;Wait-Until {(Object-Pixels) -eq $committedPixels} 'Placement Redo did not restore exact committed pixels'
   Navigation-Command 'Undo';$restored=Object-Save 'restored'
   if((Identity $restored) -ne (Identity $objectBase) -or (Wait-StablePixels {Object-Pixels}) -ne $basePixels){throw 'Placement did not restore its measured baseline'}
  }else{
   Assert-ObjectMotion $objectSource $edited $ObjectMotionMode
   Object-History 'Undo' $objectSource $before
   Object-History 'Redo' $edited $after
   Object-History 'Undo' $objectSource $before
  }
  $rows=@(Import-Csv (Join-Path $OutputDirectory $file))
  if($rows.Count -ne $duration*240+1){throw 'Object injection did not complete its contact'}
  foreach($row in $rows){$combined.Add("$offset,$($row.qpc_before),$($row.qpc_after),$($row.x),$($row.y)");$offset++}
  $capture.object_motion.contacts+=@{first_index=$offset-$rows.Count;count=$rows.Count;seconds=$duration;measured=$contact -gt 0;file=$file;artwork_changed=$true;package_validated=$true;restored=$true}
  $capture|ConvertTo-Json -Depth 20|Set-Content (Join-Path $OutputDirectory 'capture.json')
 }
 [IO.File]::WriteAllLines((Join-Path $OutputDirectory 'injected.csv'),$combined)
 [CapyRowPointer]::Dispose()
 if((Get-FileHash $Project).Hash -ne $capture.project_sha256 -or (Trace-File 'ui-state')){throw 'Object measurement modified its source or enabled UI tracing'}
}else{
[WindowsPenMotion]::Run([uint32]$review.Id,$cx,$cy,200,120,$Seconds,240,(Join-Path $OutputDirectory 'injected.csv'))
}
if(!$SkipPresentMon){
$pm.WaitForExit(30000)|Out-Null
if(!$pm.HasExited){throw 'PresentMon did not finish'}
}else{Start-Sleep -Seconds 2}
$root.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition)|ForEach-Object {$_.Current.Name}|Where-Object {$_ -match 'Invalid|normalized|chronological|failed|overflow|panic|unavailable|Nonfinite'}|Set-Content (Join-Path $OutputDirectory 'errors.txt')
$windowOwner=[uint32]0;[CapyWindowApi]::GetWindowThreadProcessId($window,[ref]$windowOwner)|Out-Null
if($windowOwner -ne $review.Id){throw 'The measured window no longer belongs to the owned process'}
if(![CapyWindowApi]::PostMessage($window,0x10,[UIntPtr]::Zero,[IntPtr]::Zero)){throw 'The measured window did not accept Close'}
$review.WaitForExit(90000)|Out-Null
if(!$review.HasExited){throw 'Benchmark app did not finish'}
$prefix='latency-'+$review.Id+'-'+$meta.window_id
Get-ChildItem (Join-Path $directory ($prefix+'-*'))|Copy-Item -Destination $OutputDirectory
Get-ChildItem (Join-Path $directory ('prediction-'+$review.Id+'-*.json'))|Copy-Item -Destination $OutputDirectory
if($review.ExitCode -ne 0){throw "Benchmark app exited with $($review.ExitCode)"}
if((Test-Path (Join-Path $OutputDirectory 'errors.txt')) -and (Get-Item (Join-Path $OutputDirectory 'errors.txt')).Length -gt 0){throw 'Benchmark application reported an error; inspect errors.txt'}
Write-Output "Captured $OutputDirectory"
