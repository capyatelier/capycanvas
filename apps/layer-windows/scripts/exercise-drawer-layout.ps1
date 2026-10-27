param([Parameter(Mandatory)][string]$Executable,[ValidateSet('light','dark')][string]$Theme='light')
$ErrorActionPreference='Stop'
$repo=(Resolve-Path(Join-Path $PSScriptRoot '../../..')).Path;$scripts=$PSScriptRoot
. (Join-Path $scripts 'CapyUia.ps1')
Add-Type -Path (Join-Path $scripts 'RowPointerDriver.cs')
Add-Type -Path (Join-Path $scripts 'BubbleFixtureNative.cs')
[CapyRowPointer]::VerifyPrivateDesktop($env:CAPY_PRIVATE_DESKTOP)
$run=Join-Path $repo ('artifacts/windows/drawer-layout/'+$Theme+'-'+[guid]::NewGuid().ToString('N'));[IO.Directory]::CreateDirectory($run)|Out-Null
$review=$null;$window=[IntPtr]::Zero;$checks=[Collections.Generic.List[object]]::new()
function Check($condition,[string]$name,$data){$checks.Add(@{name=$name;pass=[bool]$condition;data=$data});if(!$condition){throw $name}}
function Presentation {(Workspace-Root).Current.ItemStatus|ConvertFrom-Json}
function Header { (Control 'title-bar' -Arranged).Current.ItemStatus|ConvertFrom-Json }
function Drawer { (Control 'tool-drawer' -Arranged).Current.ItemStatus|ConvertFrom-Json }
function Tap([string]$id){$b=(Control $id -Arranged).Current.BoundingRectangle;[CapyRowPointer]::Down('mouse',[int]($b.Left+$b.Width/2),[int]($b.Top+$b.Height/2));[CapyRowPointer]::Up()}
function Header-Entry([string]$kind,[string]$panel=''){@((Model).header.model.zones|ForEach-Object {$_}|Where-Object {$_.item.control.kind-eq$kind-and(!$panel-or$_.item.control.panel-eq$panel)})[0]}
function Open-Header($entry){
 $id='header-item-'+$entry.id;$zone=-1
 for($i=0;$i-lt3;$i++){if(@((Model).header.model.zones[$i]|Where-Object id -eq $entry.id).Count){$zone=$i;break}}
 if(!(Find $id -Visible)){Tap ('header-overflow-'+$zone);$id='header-overflow-item-'+$entry.id}
 Tap $id;Wait-Until {(Find 'tool-drawer' -Visible)-and(Drawer).placement.bounds.height-gt0} 'Drawer did not present'
 Start-Sleep -Milliseconds 700
}
function Snapshot([string]$name){
 Capture $name -Composed -WithModel
 @{header=Header;drawer=if(Find 'tool-drawer' -Visible){Drawer}else{$null};presentation=Presentation}|ConvertTo-Json -Depth 45|Set-Content(Join-Path $run ($name+'-native.json'))
}
function Bridge-Check($entry){
 $geometry=Drawer;$anchor=$geometry.placement.anchor;$header=Header
 $actual=@($header.actual_items|Where-Object id -eq $entry.id)[0]
 $same=$actual.id-eq$entry.id
 foreach($key in @('x','y','width','height')){$same=$same-and[math]::Abs($actual.bounds.$key-$anchor.$key)-le.6}
 Check $same 'Presented drawer source matches actual header allocation' @{anchor=$anchor;actual=$actual}
 $bridge=$geometry.connection.bounds;$bounds=$geometry.placement.bounds
 Check ($geometry.placement.direction-eq'bottom'-and[math]::Abs($anchor.y+$anchor.height-$bridge.y)-le.6-and[math]::Abs($bridge.y+$bridge.height-$bounds.y)-le.6) 'Header source, bridge and drawer meet without gaps' @{placement=$geometry.placement;connection=$geometry.connection}
}
function Color-Check([string]$phase){
 Wait-Until {(Find 'color-wheel' -Visible)-and(Find 'drawer-panel-color' -Visible)} 'Color drawer controls missing'
 $frame=(Control 'tool-drawer' -Arranged).Current.BoundingRectangle;$panel=(Control 'drawer-panel-color' -Arranged).Current.BoundingRectangle
 $controls=(Control 'color-controls' -Arranged -Within (Control 'drawer-panel-color')).Current.BoundingRectangle
 $wheel=(Control 'color-wheel' -Arranged -Within (Control 'drawer-panel-color')).Current.BoundingRectangle
 $scale=[CapyRowPointer]::GetDpiForWindow($window)/96.;$placement=(Drawer).placement.bounds
 $layout=[CapyBubbleFixtureNative]::ColorQuery((Join-Path $directory 'layer_windows.dll'),(@{type='layout';size=([math]::Floor($controls.Width)/$scale);hdr=[bool](Model).color_panel.hdr}|ConvertTo-Json -Compress))|ConvertFrom-Json
 Check ([math]::Abs($wheel.Width/$scale-$layout.wheel[2])-le1-and[math]::Abs($controls.Height/$scale-$layout.height)-le1) ($phase+' shared height-for-width and wheel geometry') @{shared=$layout;controls=$controls;wheel=$wheel;scale=$scale}
 Check ([math]::Abs($wheel.Width-$wheel.Height)-le2) ($phase+' wheel square') @{wheel=$wheel}
 Check ($wheel.Top-ge$panel.Top-2-and$wheel.Bottom-le$panel.Bottom+2-and$controls.Bottom-le$panel.Bottom+2) ($phase+' color not clipped') @{wheel=$wheel;controls=$controls;panel=$panel}
 Check ([math]::Abs($frame.Height/$scale-$placement.height)-le1.1) ($phase+' native drawer matches publication') @{frame=$frame;placement=$placement}
 Snapshot $phase
}
function Scroll-Check([string]$phase){
 $model=Model;$presentation=Presentation
 foreach($panel in @('navigator','brushes','tool_settings')){
  $group=@($model.layout.groups|Where-Object active -eq $panel)[0];if(!$group){throw "Missing active $panel group"}
  $actual=@($presentation.groups|Where-Object id -eq $group.id)[0].body_scroll
  $anchor=switch($panel){navigator{'navigator-overview'} brushes{'tool-subtool-0'} tool_settings{'number-root-tool-setting-size'}}
  $outer=Control $anchor -Arranged;$pattern=$null
  while($outer-and!$outer.TryGetCurrentPattern([System.Windows.Automation.ScrollPattern]::Pattern,[ref]$pattern)){$outer=[System.Windows.Automation.TreeWalker]::RawViewWalker.GetParent($outer)}
  if(!$outer){throw "$panel has no ancestor ScrollPattern provider"}
  $scale=[CapyRowPointer]::GetDpiForWindow($window)/96.
  Check ([math]::Abs($outer.Current.BoundingRectangle.Height/$scale-$actual.viewport_height)-le1.1) ($phase+' '+$panel+' actual ScrollPresenter viewport') @{provider=$outer.Current.AutomationId;bounds=$outer.Current.BoundingRectangle;actual=$actual}
  if($panel-eq'navigator'){
   $fits=$actual.viewport_height-ge$group.min_body_height-.6
   Check $fits ($phase+' Navigator fixture allocates its published minimum') @{minimum=$group.min_body_height;actual=$actual}
   Check (!$pattern.Current.VerticallyScrollable-and$actual.extent_height-le$actual.viewport_height+.6) ($phase+' Navigator fits without artificial scrolling') @{minimum=$group.min_body_height;actual=$actual;scroll=$pattern.Current;provider_id=$outer.Current.AutomationId;provider_bounds=$outer.Current.BoundingRectangle}
  }else{
   $scroll=$pattern
   $before=$scroll.Current.VerticalScrollPercent
   if($scroll.Current.VerticallyScrollable){$scroll.Scroll([System.Windows.Automation.ScrollAmount]::NoAmount,[System.Windows.Automation.ScrollAmount]::LargeIncrement);Start-Sleep -Milliseconds 150;$after=$scroll.Current.VerticalScrollPercent;Check ($after-ne$before) ($phase+' '+$panel+' genuine overflow scroll moves') @{before=$before;after=$after;outer=$actual};$scroll.SetScrollPercent(-1,0)}
   else{Check ($panel-ne'brushes') ($phase+' Brushes expected genuine overflow') @{outer=$actual;scroll=$scroll.Current}}
  }
 }
 Snapshot $phase
}
try{
 Enter-CapyEnvironment;$env:CAPY_STORAGE_DIR=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
 [IO.File]::WriteAllText((Settings-File),(@{language=@{Explicit='en'};theme=$Theme}|ConvertTo-Json))
 $Executable=(Resolve-Path $Executable).Path;$directory=Split-Path $Executable
 $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError(Join-Path $run 'stderr.log');$null=$review.Handle
 $owned=@{window=$null};Wait-Until {$owned.window=Owned-DrawingWindow $review;$owned.window-and(Model).brush_ready-and(Model).windows_workspace.ready-and!(Model).windows_workspace.busy} 'Owned drawer review did not start' 90
 $root=$owned.window.Root;$window=$owned.window.Handle;[CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
 & (Join-Path $scripts 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $window.ToInt64() -Action Resize -Width 1800 -Height 1200
 [CapyWindowApi]::ShowWindow($window,9)|Out-Null;[CapyWindowExercise]::MoveWindow($window,40,40,1800,1200,$true)|Out-Null
 [CapyWindowApi]::SetForegroundWindow($window)|Out-Null;[CapyRowPointer]::Initialize([uint32]$review.Id)
 Use-PaintWorkspace
 Tap 'workspace-switch-builtin:workspace:painter';Wait-Until {(Model).windows_workspace.id-eq'builtin:workspace:painter'-and!(Model).windows_workspace.busy} 'Sketch switch failed'
 Open-Header (Header-Entry 'color');Color-Check 'sketch-color'
 [CapyWindowExercise]::MoveWindow($window,40,40,1801,1199,$true)|Out-Null;Start-Sleep -Milliseconds 700;Color-Check 'sketch-color-fractional'
 $layers=Header-Entry 'panel' 'layers';Open-Header $layers
 $frame=(Control 'tool-drawer' -Arranged).Current.BoundingRectangle;$footer=(Control 'layer-properties-disclosure' -Arranged).Current.BoundingRectangle;$scale=[CapyRowPointer]::GetDpiForWindow($window)/96.
 Check ($footer.Bottom-le$frame.Bottom+2-and$frame.Height/$scale-le(Model).layout.viewport[1]*.85+1) 'Layers footer contained and viewport height capped' @{frame=$frame;footer=$footer;geometry=(Drawer)}
 Bridge-Check $layers
 Snapshot 'sketch-layers'
 $context=(Model).state.toolbar_context_generation
 Tap 'workspace-switch-builtin:workspace:illustrator';Wait-Until {(Model).windows_workspace.id-eq'builtin:workspace:illustrator'-and!(Model).windows_workspace.busy-and!(Find 'tool-drawer' -Visible)} 'Context switch left stale drawer'
 Check ((Model).state.toolbar_context_generation-ne$context-and!(Model).state.customization.drawer) 'Context generation discards the old drawer' @{before=$context;after=(Model).state.toolbar_context_generation}
 Snapshot 'paint-context-reset'
 if(!@((Model).layout.groups|Where-Object active -eq 'navigator').Count){
  (Control 'column-icon-navigator' -Arranged).SetFocus();[CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x10),0x79)
  Invoke 'Expand column' -Name
  Wait-Until {@((Model).layout.groups|Where-Object active -eq 'navigator').Count-gt0} 'Navigator column did not expand'
 }
 Wait-Until {Find 'navigator-overview' -Visible} 'Expanded Navigator overview did not arrange'
 Scroll-Check 'paint-scroll'
 [CapyWindowExercise]::MoveWindow($window,40,40,1601,1001,$true)|Out-Null;Start-Sleep -Milliseconds 800;Scroll-Check 'paint-scroll-fractional'
 & (Join-Path $scripts 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $window.ToInt64() -Action Close -DiscardUnsaved
 if(!$review.WaitForExit(8000)-or$review.ExitCode-ne0){throw 'Drawer review failed clean close'}
 if((Get-Item(Join-Path $run 'stderr.log')).Length){throw 'Drawer review stderr requires inspection'}
 @{checks=$checks.Count;passed=$true;executable=$Executable;exe_sha256=(Get-FileHash $Executable).Hash;dll_sha256=(Get-FileHash(Join-Path $directory 'layer_windows.dll')).Hash;exit_code=$review.ExitCode}|ConvertTo-Json|Set-Content(Join-Path $run 'result.json')
 Write-Output "Drawer/scroll focused pass $Theme $run"
}catch{$_|Out-String|Set-Content(Join-Path $run 'failure.txt');try{Snapshot 'failure'}catch{};throw}
finally{$checks|ConvertTo-Json -Depth 15|Set-Content(Join-Path $run 'checks.json');[CapyRowPointer]::Dispose();if($review-and!$review.HasExited){[CapyWindowApi]::PostMessage($window,0x10,[UIntPtr]::Zero,[IntPtr]::Zero)|Out-Null;if(!$review.WaitForExit(8000)){Stop-Process -Id $review.Id}};Exit-CapyEnvironment}
