param(
 [Parameter(Mandatory)][string]$Executable,
 [Parameter(Mandatory)][string]$Project,
 [Parameter(Mandatory)][string]$OutputDirectory,
 [string]$PresentMon=(Join-Path $env:USERPROFILE '.local/tools/presentmon/2.5.1/PresentMon-2.5.1-x64.exe'),
 [int]$Diameter=18,
 [ValidateSet('G-Pen','Pencil')][string]$BrushPreset='G-Pen',
 [int]$Seconds=10,
 [switch]$SkipPresentMon,
 [ValidateSet('hand','zoom','rotate_view')][string]$NavigationMode,
 [ValidateSet('move','scale','rotate','placement')][string]$ObjectMotionMode,
 [string]$ObjectPreflight,
 [string]$ColdPaperPreflight
)
$ErrorActionPreference='Stop'
$brushSeries=$PSBoundParameters.ContainsKey('BrushPreset')
if($brushSeries -and ($NavigationMode -or $ObjectMotionMode -or !$SkipPresentMon -or $Seconds -lt 5)){throw 'Brush series requires pen input, -SkipPresentMon and at least five measured seconds per contact'}
if($NavigationMode -and $ObjectMotionMode){throw 'Choose one motion workload'}
if($ObjectMotionMode -and (!$ObjectPreflight -or !$SkipPresentMon)){throw 'Object motion requires its preflight and -SkipPresentMon'}
if($NavigationMode -and !$SkipPresentMon){throw 'Navigation capture uses bounded host traces; specify -SkipPresentMon'}
if($ColdPaperPreflight -and ($NavigationMode -or $ObjectMotionMode -or $brushSeries -or !$SkipPresentMon -or $Seconds -lt 5 -or $Seconds -gt 10)){throw 'Cold paper requires one five-to-ten-second contact, -SkipPresentMon and no other workload'}
. (Join-Path $PSScriptRoot '../../apps/layer-windows/scripts/CapyUia.ps1')
$CapyWaitSeconds=45
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$Executable=(Resolve-Path $Executable).Path;$Project=(Resolve-Path $Project).Path
$directory=Split-Path -Parent $Executable
if($ObjectMotionMode){
 $ObjectPreflight=(Resolve-Path -LiteralPath $ObjectPreflight).Path
 $objectPlan=Get-Content -LiteralPath $ObjectPreflight -Raw|ConvertFrom-Json
 $projectHash=if($ObjectMotionMode -eq 'placement'){$objectPlan.placement_base_sha256}else{$objectPlan.project_sha256}
 if($objectPlan.schema -ne 1 -or (Get-FileHash $Executable).Hash -ne $objectPlan.exe_sha256 -or (Get-FileHash $Project).Hash -ne $projectHash -or (Get-FileHash $objectPlan.project).Hash -ne $objectPlan.project_sha256 -or (Get-FileHash $objectPlan.placement_base).Hash -ne $objectPlan.placement_base_sha256 -or (Get-FileHash (Join-Path $directory 'layer_windows.dll')).Hash -ne $objectPlan.dll_sha256 -or (Get-FileHash $objectPlan.image).Hash -ne $objectPlan.image_sha256){throw 'Object preflight source or project identity changed'}
    Add-Type -Path (Join-Path $repo 'apps/layer-windows/scripts/PackageFixture.cs') -ReferencedAssemblies @(
        'System.IO.Compression.dll','System.IO.Compression.FileSystem.dll'
        if($PSVersionTable.PSEdition -eq 'Core'){'System.Collections.dll';'System.IO.Compression.ZipFile.dll'}
    )
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
if($ColdPaperPreflight){
 $ColdPaperPreflight=(Resolve-Path -LiteralPath $ColdPaperPreflight).Path
 $plan=Get-Content -LiteralPath $ColdPaperPreflight -Raw|ConvertFrom-Json
 if($plan.schema -ne 1 -or $plan.project_sha256 -ne (Get-FileHash -LiteralPath $Project).Hash -or $plan.qualification.relief -le 0 -or $plan.qualification.empty_editable_paint -ne $true -or $plan.qualification.brush -ne 'G-Pen' -or $plan.qualification.diameter -ne $Diameter -or @($plan.offstroke_points).Count -lt 4 -or @($plan.stroke_points).Count -lt 4){throw 'Cold paper requires a qualified saved relief seed and separated reference samples'}
 foreach($field in 'qualification_path','source_manifest_path'){
  if((Get-FileHash -LiteralPath $plan.$field).Hash -ne $plan.($field.Replace('_path','_sha256'))){throw "Cold seed provenance changed: $field"}
 }
 if($plan.offstroke_unlit.Count -ne $plan.offstroke_points.Count -or $plan.offstroke_lit.Count -ne $plan.offstroke_points.Count -or $plan.stroke_lit.Count -ne $plan.stroke_points.Count){throw 'Cold seed sample references are incomplete'}
 if(@(0..($plan.offstroke_points.Count-1)|Where-Object {$plan.offstroke_lit[$_] -ne $plan.offstroke_unlit[$_]}).Count -lt 4){throw 'Four off-stroke points must distinguish real lighting'}
 Add-Type -Path (Join-Path $PSScriptRoot 'WindowsPenMotion.cs')
 Add-Type -Path (Join-Path $repo 'apps/layer-windows/scripts/RowPointerDriver.cs')
 Add-Type -AssemblyName WindowsBase
 $motionTokens=$null;$motionErrors=$null;$motionAst=[Management.Automation.Language.Parser]::ParseFile($PSCommandPath,[ref]$motionTokens,[ref]$motionErrors)
 if(@($motionErrors).Count){throw 'The existing motion guards did not parse'}
 foreach($name in 'Motion-Canvas','Motion-Point','Motion-Path'){
  $definition=@($motionAst.FindAll({param($node)$node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name},$true))
  if($definition.Count -ne 1){throw "The existing $name guard is ambiguous"}
  . ([scriptblock]::Create($definition[0].Extent.Text))
 }
 function Cold-Samples([int[]]$Coordinates){
  for($i=0;$i -lt $Coordinates.Length;$i+=2){$null=Motion-Point $script:paperRegion ([Windows.Point]::new($Coordinates[$i],$Coordinates[$i+1]))}
  [WindowsPenMotion]::Sample([uint32]$review.Id,$Coordinates)
 }
 function Complete-ColdPaper([int]$WaitMilliseconds=30000){
  $failures=[Collections.Generic.List[string]]::new()
  try{
   try{if($producer){if(!$producer.Wait($WaitMilliseconds)){throw 'Producer wait timed out'};$producer.GetAwaiter().GetResult()}}catch{$failures.Add('producer: '+$_.Exception.ToString())}
  }finally{
   try{try{if($script:paperReader){$script:paperReader.Dispose()}}catch{$failures.Add('reader: '+$_.Exception.ToString())}}finally{
    try{
     try{if($review -and !$review.HasExited){& (Join-Path $repo 'apps/layer-windows/scripts/exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved;if(!$review.WaitForExit(90000) -or !$review.HasExited){throw 'Owned cold-paper app did not close'}}}catch{
      $failures.Add('close: '+$_.Exception.ToString())
      try{if($review -and !$review.HasExited){Stop-Process -Id $review.Id -Force -ErrorAction Stop;if(!$review.WaitForExit(5000) -or !$review.HasExited){throw 'Owned app remained active after cleanup'}}}catch{$failures.Add('owned-process cleanup: '+$_.Exception.ToString())}
     }
    }finally{
     try{if($review -and $meta){Get-ChildItem -LiteralPath $directory -Filter ('latency-'+$review.Id+'-'+$meta.window_id+'-*')|Copy-Item -Destination $OutputDirectory}}catch{$failures.Add('evidence: '+$_.Exception.ToString())}
     try{if($producer -and !$producer.IsCompleted){if(!$producer.Wait([Math]::Min(5000,$WaitMilliseconds))){throw 'Producer remained unfinished after owned-app cleanup'}}}catch{$failures.Add('producer completion: '+$_.Exception.ToString())}
     $primary=Join-Path $OutputDirectory 'failure.txt'
     @{failures=$failures.ToArray();producer_completed=(!$producer -or $producer.IsCompleted);primary_failure=$(if(Test-Path -LiteralPath $primary){[IO.File]::ReadAllText($primary)}else{$null})}|ConvertTo-Json -Depth 6|Set-Content -LiteralPath (Join-Path $OutputDirectory 'cleanup.json')
    }
   }
  }
  if($failures.Count){throw ($failures -join "`n")}
 }
 function Cold-Jobs {
  $script:paperTail+=$script:paperReader.ReadToEnd()
  $last=$script:paperTail.LastIndexOf("`n")
  if($last -ge 0){
   foreach($rawLine in $script:paperTail.Substring(0,$last+1).Split("`n")){
    $line=$rawLine.TrimEnd("`r")
    if($line -eq 'pipeline begin label=Some("viewport presentation with paper lighting")'){$script:paperBegins++}
    if($line -match '^pipeline end label=Some\("viewport presentation with paper lighting"\) elapsed_ms='){$script:paperEnds++}
   }
   $script:paperTail=$script:paperTail.Substring($last+1)
  }
  if($script:paperEnds -gt $script:paperBegins -or $script:paperReader.BaseStream.Position -gt 2MB){throw 'Cold paper shader log is invalid or exceeds its bound'}
  @($script:paperBegins,$script:paperEnds)
 }
 $script:paperBegins=0;$script:paperEnds=0;$script:paperTail='';$script:paperReader=$null;$producer=$null;$review=$null
 $observations=[Collections.Generic.List[object]]::new();$stderr=Join-Path $OutputDirectory 'stderr.log'
 try{
  try{
   Enter-CapyEnvironment @('CAPY_LATENCY_TRACE','CAPY_TRACE_SHADER_JOBS')
   $env:CAPY_STORAGE_DIR=Join-Path $OutputDirectory 'profile';$env:CAPY_LATENCY_TRACE='1';$env:CAPY_TRACE_SHADER_JOBS='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
   $plan.settings|ConvertTo-Json -Depth 30|Set-Content (Settings-File)
   if($Project.Contains('"')){throw 'Project path contains a quote'}
   $launch=[Diagnostics.Stopwatch]::GetTimestamp()
   $review=Start-Process -FilePath $Executable -ArgumentList @('"'+$Project+'"') -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
   $launchReturned=[Diagnostics.Stopwatch]::GetTimestamp();$null=$review.Handle
  }finally{Exit-CapyEnvironment}
  $review.Id|Set-Content (Join-Path $OutputDirectory 'process-id.txt')
  $script:paperReader=[IO.StreamReader]::new([IO.File]::Open($stderr,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::ReadWrite))
  Wait-Until {$script:drawing=Owned-DrawingWindow $review;$drawing} 'The cold-paper owned window did not open' 60
  [CapyWindowApi]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
  $root=$drawing.Root;$window=$drawing.Handle
  Wait-Until {$script:probe=Trace-File 'presentation-probe';$probe} 'The native joint-ready stage was not recorded' 90
  $meta=Read-Snapshot $probe;$canvas=Control 'drawing-canvas' -Arranged;$bounds=$canvas.Current.BoundingRectangle
  $title=[IO.Path]::GetFileName($Project)+' '+[char]0x00b7+' Capy Canvas';$modified=[char]0x2022+' '+$title
  if($root.Current.Name -ne $title -or !$canvas.Current.IsEnabled -or (Find 'canvas-status' -Visible) -or (Control 'canvas-camera').Current.Name -ne $plan.camera_readout -or ((@($bounds.X,$bounds.Y,$bounds.Width,$bounds.Height) -join ',') -ne ($plan.canvas_bounds -join ',')) -or [CapyRowPointer]::GetDpiForWindow($window) -ne $plan.dpi -or (Control 'tool-setting-size').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value -ne "$Diameter px"){throw 'Cold seed viewport, clean title, readiness, camera, DPI or brush size differs from preflight'}
  $selected=@($root.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,'G-Pen'))|Where-Object {$_.Current.AutomationId -like 'tool-subtool-*' -and $_.Current.ItemStatus -eq 'Selected' -and $_.Current.IsEnabled})
  if($selected.Count -ne 1){throw 'Cold-paper seed did not select the native G-Pen preset'}
  if(Trace-File 'ui-state'){throw 'Cold paper measurement forbids per-frame UI tracing'}
  $cx=[int]$plan.path.cx;$cy=[int]$plan.path.cy;$rx=[int]$plan.path.rx;$ry=[int]$plan.path.ry
  $pad=$Diameter*[double]$plan.camera_zoom/2+4
  if($rx -le 0 -or $ry -le 0 -or $cx-$rx-$pad -le $bounds.Left -or $cx+$rx+$pad -ge $bounds.Right -or $cy-$ry-$pad -le $bounds.Top -or $cy+$ry+$pad -ge $bounds.Bottom){throw 'Cold-paper contact exceeds the qualified canvas'}
  $points=[int[]]@($plan.offstroke_points|ForEach-Object {$_[0];$_[1]});$strokePoints=[int[]]@($plan.stroke_points|ForEach-Object {$_[0];$_[1]})
  foreach($point in @($plan.offstroke_points)+@($plan.stroke_points)){
   if(!$bounds.Contains([Windows.Point]::new($point[0],$point[1]))){throw 'Cold-paper sample exceeds the qualified canvas'}
  }
  foreach($point in $plan.offstroke_points){if([Math]::Abs($point[0]-$cx) -le $rx+$pad -and [Math]::Abs($point[1]-$cy) -le $ry+$pad){throw 'Lighting sample overlaps the complete brush-expanded path'} }
  $script:paperRegion=Motion-Canvas;$coldPath=Motion-Path $script:paperRegion $cx $cy $rx $ry $Seconds
  foreach($point in @($plan.offstroke_points)+@($plan.stroke_points)){$null=Motion-Point $script:paperRegion ([Windows.Point]::new($point[0],$point[1]))}
  $script:paperRegion|ConvertTo-Json -Depth 8|Set-Content -LiteralPath (Join-Path $OutputDirectory 'cold-canvas-region.json')
  $compiler=@($review.Modules|Where-Object ModuleName -eq 'dxcompiler.dll')
  if($compiler.Count -ne 1 -or $compiler[0].FileName -ne (Join-Path $directory 'dxcompiler.dll') -or (Get-FileHash -LiteralPath $compiler[0].FileName).Hash -ne $plan.dxcompiler_sha256 -or $meta.scope -ne 'steady_canvas_presentation_probe' -or $meta.swap_chain_addresses.Count -ne 2 -or $meta.process_id -ne $review.Id){throw 'Cold-paper DX12 surface or loaded compiler is unqualified'}
  [CapyWindowApi]::SetForegroundWindow($window)|Out-Null;$canvas.SetFocus()
  $before=[Diagnostics.Stopwatch]::GetTimestamp();$jobs=Cold-Jobs;$pixels=Cold-Samples $points;$after=[Diagnostics.Stopwatch]::GetTimestamp()
  $armed=@{qpc_before=$before;qpc_after=$after;begins=$jobs[0];ends=$jobs[1];pixels=$pixels}
  $capture=[ordered]@{process_id=$review.Id;diameter=$Diameter;project_sha256=(Get-FileHash -LiteralPath $Project).Hash;exe_sha256=(Get-FileHash -LiteralPath $Executable).Hash;dll_sha256=(Get-FileHash -LiteralPath (Join-Path $directory 'layer_windows.dll')).Hash;surface=$meta;seconds=$Seconds;rate_hz=240;center=@($cx,$cy);radii=@($rx,$ry);qpc_frequency=[Diagnostics.Stopwatch]::Frequency;cold_paper=@{path_qualification=$coldPath;preflight_sha256=(Get-FileHash -LiteralPath $ColdPaperPreflight).Hash;launch_qpc=$launch;launch_returned_qpc=$launchReturned;joint_ready_ns=[double]$meta.ready_qpc_ns;armed=$armed;offstroke_unlit=$plan.offstroke_unlit;offstroke_lit=$plan.offstroke_lit;contacts=@();observations=@();ui_trace_enabled=$false;adapter_observation='DX12/DXGI native probe; named adapter is qualified separately, not exposed by this probe';loaded_compiler=@{path=$compiler[0].FileName;sha256=$plan.dxcompiler_sha256;version=$compiler[0].FileVersionInfo.FileVersion};true_presented_fps=$null;target_met=$null}}
  if($jobs[0] -le $jobs[1]){
   @{status='unmeasured';reason='No actual viewport-paper job remained pending at the owned joint-ready contact gate';capture=$capture}|ConvertTo-Json -Depth 30|Set-Content (Join-Path $OutputDirectory 'cold-paper-result.json')
   return
  }
  $producer=[WindowsPenMotion]::RunAsync([uint32]$review.Id,$cx,$cy,$rx,$ry,$Seconds,240,(Join-Path $OutputDirectory 'injected.csv'))
  $deadline=[DateTime]::UtcNow.AddSeconds($Seconds+30)
  do{
   $before=[Diagnostics.Stopwatch]::GetTimestamp();$jobs=Cold-Jobs;$pixels=Cold-Samples $points;$after=[Diagnostics.Stopwatch]::GetTimestamp()
   $observations.Add(@{qpc_before=$before;qpc_after=$after;begins=$jobs[0];ends=$jobs[1];pixels=$pixels})
   $lit=($pixels -join ',') -eq ($plan.offstroke_lit -join ',')
   if([DateTime]::UtcNow -gt $deadline){throw 'Cold paper observations exceeded the bounded contact window'}
   if(!$producer.IsCompleted -or $jobs[0] -gt $jobs[1] -or !$lit){Start-Sleep -Milliseconds 20}
  }while(!$producer.IsCompleted -or $jobs[0] -gt $jobs[1] -or !$lit)
  $producer.GetAwaiter().GetResult()
  Wait-Until {$root.Current.Name -eq $modified -and !(Find 'canvas-status' -Visible)} 'Cold-paper contact did not commit paint'
  $painted=Cold-Samples $strokePoints
  if(($painted -join ',') -eq ($plan.stroke_lit -join ',')){throw 'The actual cold contact changed no stroke sample'}
  [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),0x5a)
  Wait-Until {$root.Current.Name -eq $title -and ((Cold-Samples $strokePoints) -join ',') -eq ($plan.stroke_lit -join ',')} 'One Undo did not restore the exact lit seed samples'
  [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),0x59)
  Wait-Until {$root.Current.Name -eq $modified -and ((Cold-Samples $strokePoints) -join ',') -eq ($painted -join ',')} 'One Redo did not restore the exact contact samples'
  [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),0x5a)
  Wait-Until {$root.Current.Name -eq $title} 'Final Undo did not restore the clean seed'
  if((Control 'canvas-camera').Current.Name -ne $plan.camera_readout -or (Control 'drawing-canvas' -Arranged).Current.BoundingRectangle -ne $bounds -or (Get-FileHash -LiteralPath $Project).Hash -ne $capture.project_sha256 -or (Trace-File 'ui-state')){throw 'Cold contact changed its camera, source drawing or trace mode'}
  $capture.cold_paper.contacts=@(@{first_index=0;count=$Seconds*240+1;seconds=$Seconds;measured=$true;artwork_changed=$true;restored=$true});$capture.cold_paper.observations=$observations.ToArray()
  $capture|ConvertTo-Json -Depth 30|Set-Content (Join-Path $OutputDirectory 'capture.json')
 }catch{[IO.File]::WriteAllText((Join-Path $OutputDirectory 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw}finally{
  Complete-ColdPaper
 }
 return
}
try{
 Enter-CapyEnvironment @('CAPY_LATENCY_TRACE')
 $env:CAPY_STORAGE_DIR=Join-Path $OutputDirectory 'profile'
 $env:CAPY_LATENCY_TRACE='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
 if($NavigationMode -or $ObjectMotionMode -or $brushSeries){@{language=@{Explicit='en'};theme=$(if($ObjectMotionMode){$objectPlan.theme}else{'dark'})}|ConvertTo-Json|Set-Content (Settings-File)}
 $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError (Join-Path $OutputDirectory 'stderr.log')
 $null=$review.Handle
}finally{Exit-CapyEnvironment}
$review.Id|Set-Content (Join-Path $OutputDirectory 'process-id.txt')
Wait-Until {$script:drawing=Owned-DrawingWindow $review;$null -ne $drawing} 'The owned drawing window did not open' 60
[CapyWindowApi]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
$window=$drawing.Handle;$root=$drawing.Root
if($NavigationMode -or $ObjectMotionMode -or $brushSeries){$root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Maximized)}
Wait-Until {$script:probe=Trace-File 'presentation-probe';$null -ne $probe} 'Renderer did not become ready' 90
Open-Project $Project
$projectTitle=[IO.Path]::GetFileName($Project)+' '+[char]0x00b7+' Capy Canvas'
$modifiedProjectTitle=[char]0x2022+' '+$projectTitle
Wait-Until {
 $canvas=Find 'drawing-canvas' -Visible;$field=Find 'tool-setting-size'
 $root.Current.Name -eq $projectTitle -and $canvas -and $canvas.Current.IsEnabled -and ($ObjectMotionMode -or ($field -and $field.Current.IsEnabled)) -and !(Find 'canvas-status' -Visible)
} 'The requested drawing did not become ready for pen input' 90
if($NavigationMode -or $ObjectMotionMode -or $brushSeries){
 Add-Type -Path (Join-Path $repo 'apps/layer-windows/scripts/RowPointerDriver.cs')
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
 function Navigation-State([bool]$ExpectedUndo=$false,[bool]$ExpectedRedo=$false,[bool]$Modified=$false) {
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
  $expectedTitle=if($Modified){$modifiedProjectTitle}else{$projectTitle}
  if($state.title -ne $expectedTitle -or $state.undo -ne $ExpectedUndo -or $state.redo -ne $ExpectedRedo){throw 'The selected tool, drawing title or Undo/Redo history differs from the expected contact state'}
  Navigation-Focus
  [pscustomobject]$state
 }

if($NavigationMode -or $ObjectMotionMode -or $brushSeries){
 Add-Type -AssemblyName WindowsBase
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
 function Motion-Path($Region,[int]$Cx,[int]$Cy,[int]$Rx,[int]$Ry,[int]$Seconds){
  if($Seconds -lt 1 -or $Seconds -gt 30){throw 'Motion duration exceeds the producer bounds'}
  $seen=[Collections.Generic.HashSet[string]]::new();$points=[Collections.Generic.List[object]]::new();$hits=@{};$last=$null
  for($step=0;$step -lt $Seconds*240;$step++){
   $angle=2*[Math]::PI*$step/240;$x=$Cx+[int][Math]::Truncate($Rx*[Math]::Cos($angle));$y=$Cy+[int][Math]::Truncate($Ry*[Math]::Sin($angle));$last=@($x,$y)
   if($seen.Add("$x,$y")){
    $hit=Motion-Point $Region ([Windows.Point]::new($x,$y));$points.Add($last)
    $key=$hit.Current.AutomationId;if(!$key){$key='runtime:'+($hit.GetRuntimeId() -join ':')}
    $hits[$key]=$hit.Current.Name
   }
  }
  [pscustomobject]@{process_id=$review.Id;root_id=($root.GetRuntimeId() -join ':');region=$Region;center=@($Cx,$Cy);radii=@($Rx,$Ry);seconds=$Seconds;rate_hz=240;distinct_points=$points.ToArray();final_up_point=$last;hits=$hits}
 }
}
if($NavigationMode -or $ObjectMotionMode){
 $run=$OutputDirectory;$CapyPopups=$true
 $navigationLabel=@{hand='Hand';zoom='Zoom';rotate_view='Rotate view'}[$NavigationMode]
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
if($brushSeries){
 $CapyPopups=$true;$navigationLabel=$BrushPreset
 Navigation-Command $BrushPreset
 Wait-Until {$chosen=@($root.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,$BrushPreset))|Where-Object {$_.Current.AutomationId -like 'tool-subtool-*' -and $_.Current.ItemStatus -eq 'Selected'});$chosen.Count -eq 1 -and $chosen[0].Current.IsEnabled -and (Control 'drawing-canvas').Current.IsEnabled -and !(Find 'canvas-status' -Visible)} 'The selected brush did not become ready'
 $brushName=$BrushPreset
}else{
 Invoke-Id 'tool-subtool-0'
 $brushName=(Control 'tool-subtool-0').Current.Name
 if($brushName -notmatch 'G[- ]?Pen'){throw "Expected G-Pen, got $brushName"}
}
$size=Control 'tool-setting-size';$size.SetFocus();$size.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue([string]$Diameter)
(Control 'tool-setting-opacity').SetFocus();Fit-Canvas
Wait-Until {$committedSize=Find 'tool-setting-size';$committedSize -and $committedSize.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value -eq "$Diameter px"} 'Brush size was not committed'
[CapyWindowApi]::SetForegroundWindow($window)|Out-Null
Start-Sleep -Seconds 3
}
$canvas=Control 'drawing-canvas' -Arranged
Wait-Until {$canvas.Current.IsEnabled -and !(Find 'canvas-status' -Visible)} 'The canvas stopped being ready before the stroke'
$bounds=$canvas.Current.BoundingRectangle
$cx=[int]($bounds.X+$bounds.Width/2);$cy=[int]($bounds.Y+$bounds.Height/2)
if($brushSeries){$brushCamera=(Control 'canvas-camera').Current.Name}
if(!$ObjectMotionMode -and ($cx-200 -le $bounds.Left -or $cx+200 -ge $bounds.Right -or $cy-120 -le $bounds.Top -or $cy+120 -ge $bounds.Bottom)){throw 'The pen path exceeds the arranged canvas'}
$meta=Read-Snapshot $probe
$capture=[ordered]@{process_id=$review.Id;brush_name=$brushName;diameter=$Diameter;project_sha256=(Get-FileHash $Project).Hash;exe_sha256=(Get-FileHash $Executable).Hash;dll_sha256=(Get-FileHash (Join-Path $directory 'layer_windows.dll')).Hash;surface=$meta;seconds=$Seconds;rate_hz=240;center=@($cx,$cy);radii=@(200,120);qpc_frequency=[Diagnostics.Stopwatch]::Frequency;display=(& (Join-Path $repo 'apps/layer-windows/scripts/probe-displays.ps1')|ConvertFrom-Json)}
if($brushSeries){$capture.brush_motion=[ordered]@{preset=$BrushPreset;contacts=@();observations=@();ui_trace_enabled=$false};if(Trace-File 'ui-state'){throw 'Brush measurement forbids per-frame UI tracing'}}
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
 $navigationRegion=Motion-Canvas
 $navigationRegion|ConvertTo-Json -Depth 8|Set-Content (Join-Path $OutputDirectory 'navigation-path-region.json')
 $navigationPath=Motion-Path $navigationRegion $cx $cy 200 120 6
 $capture.navigation.path_hits=$navigationPath.hits
 $capture.navigation.path_qualification=$navigationPath
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
}elseif($brushSeries){
 $capture.seconds=1+3*$Seconds
 $combined=[Collections.Generic.List[string]]::new();$combined.Add('index,qpc_before,qpc_after,x,y');$offset=0
 foreach($contact in 0..3){
  $before=Navigation-State $false ($contact -gt 0)
  Wait-Until {(Control 'tool-setting-size').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value -eq "$Diameter px" -and !(Find 'canvas-status' -Visible)} 'The measured brush size or readiness changed'
  if((Control 'drawing-canvas' -Arranged).Current.BoundingRectangle -ne $bounds -or (Control 'canvas-camera').Current.Name -ne $brushCamera){throw 'The measured brush camera or arranged canvas changed'}
  Navigation-Focus
  $duration=if($contact -eq 0){1}else{$Seconds};$file="injected-$contact.csv"
  $pathQualification=Motion-Path (Motion-Canvas) $cx $cy 200 120 $duration
  $qualificationFile="brush-$contact-path.json"
  [IO.File]::WriteAllText((Join-Path $OutputDirectory $qualificationFile),($pathQualification|ConvertTo-Json -Depth 12),[Text.UTF8Encoding]::new($false))
  [WindowsPenMotion]::Run([uint32]$review.Id,$cx,$cy,200,120,$duration,240,(Join-Path $OutputDirectory $file))
  Wait-Until {$root.Current.Name -eq $modifiedProjectTitle -and !(Find 'canvas-status' -Visible)} 'The brush contact did not change the drawing'
  $painted=Navigation-State $true $false $true
  Navigation-Command 'Undo'
  Wait-Until {$root.Current.Name -eq $projectTitle -and !(Find 'canvas-status' -Visible)} 'Brush Undo did not restore the clean drawing'
  $restored=Navigation-State $false $true
  if($restored.camera_readout -ne $before.camera_readout -or (Control 'drawing-canvas' -Arranged).Current.BoundingRectangle -ne $bounds){throw 'Brush contact or Undo changed the camera or arranged canvas'}
  $rows=@(Import-Csv (Join-Path $OutputDirectory $file))
  if($rows.Count -ne $duration*240+1){throw 'Brush injection did not complete its contact'}
  foreach($row in $rows){$combined.Add("$offset,$($row.qpc_before),$($row.qpc_after),$($row.x),$($row.y)");$offset++}
  $capture.brush_motion.contacts+=@{first_index=$offset-$rows.Count;count=$rows.Count;seconds=$duration;measured=$contact -gt 0;file=$file;path_qualification=$qualificationFile;artwork_changed=$true;restored=$true}
  $capture.brush_motion.observations+=@{contact=$contact;before=$before;painted=$painted;restored=$restored}
  $capture|ConvertTo-Json -Depth 12|Set-Content (Join-Path $OutputDirectory 'capture.json')
 }
 [IO.File]::WriteAllLines((Join-Path $OutputDirectory 'injected.csv'),$combined)
 if((Get-FileHash $Project).Hash -ne $capture.project_sha256 -or (Trace-File 'ui-state')){throw 'Brush measurement changed its source or enabled UI tracing'}
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
