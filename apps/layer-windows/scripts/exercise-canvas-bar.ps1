param([Parameter(Mandatory)][string]$Executable,[ValidateSet('dark','light','invert-dark','invert-light','tail-dark','tail-light','parity','parity-light')][string]$Theme='dark')
$ErrorActionPreference='Stop'
$parityCapture=$Theme -in @('parity','parity-light');if($parityCapture){$Theme=if($Theme -eq 'parity-light'){'light'}else{'dark'}}
$Theme=$Theme.ToLowerInvariant();$invertOnly=$Theme.StartsWith('invert-');if($invertOnly){$Theme=$Theme.Substring(7)}
$tailOnly=$Theme.StartsWith('tail-');if($tailOnly){$Theme=$Theme.Substring(5)}
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyWaitSeconds=45
Add-Type -AssemblyName System.Drawing
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
$inkHelpers=[System.Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot 'capture-number-controls.ps1'),[ref]$null,[ref]$null)
foreach($helper in @('Measure-RenderedInk','Assert-ControlCaptionInk','Save-ControlCaptionInk')){
 $definition=$inkHelpers.Find({param($node)$node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $helper},$true)
 if(!$definition){throw "Missing rendered ink helper: $helper"}
 . ([scriptblock]::Create($definition.Extent.Text))
}
if(!('CapyBarInvokeTrace' -as [type])){Add-Type -ReferencedAssemblies @([System.Windows.Automation.AutomationElement].Assembly.Location;[System.Windows.Automation.AutomationEvent].Assembly.Location;'WindowsBase';if($PSVersionTable.PSEdition -eq 'Core'){'System.Collections.dll';'System.Threading.dll'}) -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Windows.Automation;
public sealed class CapyBarInvokeReceipt {
 public long Sequence {get;set;}
 public long TimeTicks {get;set;}
 public string Id {get;set;}
 public int[] RuntimeId {get;set;}
 public int Process {get;set;}
}
public sealed class CapyBarInvokeTrace {
 readonly object gate=new object();
 readonly List<CapyBarInvokeReceipt> events=new List<CapyBarInvokeReceipt>();
 AutomationElement root;
 AutomationEventHandler handler;
 int process;
 long sequence;
 public string Registration {get;private set;}
 public string Error {get;private set;}
 public void Start(AutomationElement ownedRoot,int owner) {
  root=ownedRoot;process=owner;handler=OnInvoke;
  try{Automation.AddAutomationEventHandler(InvokePattern.InvokedEvent,root,TreeScope.Subtree,handler);Registration="registered";}
  catch(Exception error){Registration="unsupported";Error=error.ToString();handler=null;}
 }
 void OnInvoke(object sender,AutomationEventArgs args) {
  try{
   var element=sender as AutomationElement;if(element==null)return;
   var current=element.Current;if(current.ProcessId!=process||current.AutomationId!="canvas-bar-invert_selection")return;
   var receipt=new CapyBarInvokeReceipt{TimeTicks=Stopwatch.GetTimestamp(),Id=current.AutomationId,RuntimeId=element.GetRuntimeId(),Process=current.ProcessId};
   lock(gate){receipt.Sequence=++sequence;events.Add(receipt);if(events.Count>128)events.RemoveAt(0);}
  }catch(Exception error){lock(gate){Error=error.ToString();}}
 }
 public CapyBarInvokeReceipt[] Snapshot(){lock(gate){return events.ToArray();}}
 public void Stop(){if(handler==null)return;try{Automation.RemoveAutomationEventHandler(InvokePattern.InvokedEvent,root,handler);}catch(Exception error){Error=error.ToString();}handler=null;}
}
'@}
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$run=Join-Path $repo ('artifacts/windows/canvas-bar/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$CapyTraceDirectory=$run
$checks=[ordered]@{scope=$(if($tailOnly){'remaining canvas bar controls'}elseif($invertOnly){'Invert input diagnostic'}else{'all canvas bar journeys'})}
$barTaps=[Collections.Generic.List[object]]::new()
$toolChoices=[Collections.Generic.List[object]]::new()
$invokeTrace=[CapyBarInvokeTrace]::new()
function Native-ContactReceipt{
 $items=@(foreach($element in @((Workspace-Root),(Find 'drawing-canvas'))){if($element){$current=$element.Current;[ordered]@{id=$current.AutomationId;help=$current.HelpText;status=$current.ItemStatus}}})
 $m=Model
 [ordered]@{time_ticks=[Diagnostics.Stopwatch]::GetTimestamp();revision=$m.state.document_file.revision;bar=$m.state.canvas_bar|Select-Object context,anchor,placement;surfaces=$items}
}
function Save-InvokeDiagnostic{
 $events=@($invokeTrace.Snapshot())
 $first=@($barTaps|Where-Object {$_.id -eq 'canvas-bar-invert_selection' -and $_.acknowledged_ticks})[0]
 $firstSeen=$first -and @($events|Where-Object {$_.Id -eq $first.id -and $_.TimeTicks -ge $first.down_ticks -and $_.TimeTicks -le $first.acknowledged_ticks}).Count -gt 0
 $usable=$invokeTrace.Registration -eq 'registered' -and $firstSeen
 [ordered]@{registration=$invokeTrace.Registration;usable=[bool]$usable;reason=$(if($usable){'observed the first acknowledged Invert'}else{'unusable unless the first acknowledged Invert raises an observed Invoke event'});error=$invokeTrace.Error;stopwatch_frequency=[Diagnostics.Stopwatch]::Frequency;events=$events}|ConvertTo-Json -Depth 8|Set-Content -LiteralPath (Join-Path $run 'native-invoke-events.json')
 $barTaps|ConvertTo-Json -Depth 16|Set-Content -LiteralPath (Join-Path $run 'native-taps.json')
}
function Close-CanvasReview{
 $invokeTrace.Stop()
 [CapyRowPointer]::Dispose()
 & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Close -DiscardUnsaved -StateDirectory $run
 if((Get-Item (Join-Path $run 'stderr.log')).Length){throw 'Native stderr requires review'}
 $checks.evidence=$run
 [pscustomobject]$checks|ConvertTo-Json|Tee-Object -FilePath (Join-Path $run 'results.json')
}
function Owned([string]$Id,[switch]$Name,$Type){
 $property=if($Name){[System.Windows.Automation.AutomationElement]::NameProperty}else{[System.Windows.Automation.AutomationElement]::AutomationIdProperty}
 $condition=[System.Windows.Automation.AndCondition]::new([System.Windows.Automation.PropertyCondition]::new($property,$Id),
  [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty,$review.Id))
 if($Type){$condition=[System.Windows.Automation.AndCondition]::new($condition,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,$Type))}
 [System.Windows.Automation.AutomationElement]::RootElement.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$condition)
}
function Bar {$bar=Find 'canvas-action-bar';if($bar -and !$bar.Current.IsOffscreen){$bar}}
function Kind {(Model).state.canvas_bar.context.kind}
function Paint {@((Model).state.layers|ForEach-Object {"$($_.id):$($_.paint_revision)"}) -join ','}
function Value([string]$Id){((Model).state.tool_settings|Where-Object id -eq $Id).value}
function Glass-State($Snapshot){
 [ordered]@{selection=$Snapshot.state.layer_tools.has_selection;paint=@($Snapshot.state.layers|Select-Object id,paint_revision);document=$Snapshot.state.document_file|Select-Object epoch,revision,modified;viewport=$Snapshot.state.camera.viewport;chrome=$Snapshot.state.customization}|ConvertTo-Json -Depth 20 -Compress
}
function Bar-Visibility([bool]$Visible){
 $choice=@((Model).state.commands|Where-Object id -eq 'show_canvas_action_bar')[0]
 if(!$choice.enabled -or [bool]$choice.selected -eq $Visible){throw 'Canvas bar visibility did not start in the expected state'}
 & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'View'
 $item=Control 'show_canvas_action_bar' -Type ([System.Windows.Automation.ControlType]::MenuItem) -Arranged
 if($item.Current.ProcessId -ne $review.Id){throw 'Canvas bar menu item belongs to another process'}
 $pattern=$null
 if($item.TryGetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern,[ref]$pattern)){$pattern.Toggle()}
 else{$item.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()}
 Wait-Until {$m=Model;[bool]@($m.state.commands|Where-Object id -eq 'show_canvas_action_bar')[0].selected -eq $Visible -and [bool]$m.state.workspace.layout.canvas_bar -eq $Visible -and [bool](Bar) -eq $Visible -and !(MenuOpen)} 'Canvas bar visibility did not acknowledge its native menu action' 5
}
function Choice([string]$Id){@((Model).state.canvas_bar.items|Where-Object {$_.option.Choice.id -eq $Id})[0].option.Choice}
function MenuOpen{
 $condition=[System.Windows.Automation.AndCondition]::new(
  [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty,$review.Id),
  [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::FrameworkIdProperty,'XAML'),
  [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::MenuItem))
 $null -ne [System.Windows.Automation.AutomationElement]::RootElement.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$condition)
}
function AnchorQuadrant{$m=Model;$a=$m.state.canvas_bar.anchor;$c=$m.state.camera;$r=(Find 'drawing-canvas').Current.BoundingRectangle
 $point=@([int]($r.X+$c.translation[0]+$c.zoom*($a[0]+3*$a[2])/4),[int]($r.Y+$c.translation[1]+$c.zoom*($a[1]+3*$a[3])/4))
 $reach=12*[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
 foreach($u in @(0.,.5,1.)){foreach($v in @(0.,.5,1.)){
  $x=$r.X+$c.translation[0]+$c.zoom*($a[0]+$u*($a[2]-$a[0]));$y=$r.Y+$c.translation[1]+$c.zoom*($a[1]+$v*($a[3]-$a[1]))
  if([Math]::Sqrt([Math]::Pow($point[0]-$x,2)+[Math]::Pow($point[1]-$y,2)) -le $reach){throw 'Transform body target intersects a handle hit region'}
 }}
 $point}
function Center($Element){$r=$Element.Current.BoundingRectangle;@([int]($r.X+$r.Width/2),[int]($r.Y+$r.Height/2))}
function Tap([string]$Id,[string]$Device){
 $target=@{point=$null}
 $record=[ordered]@{id=$Id;device=$Device;parent=$null;bounds=$null;before=$null;after=$null;context=(Model).state.canvas_bar.context;down_ticks=$null;up_ticks=$null;acknowledged_ticks=$null;contact=$null;released=$null}
 $barTaps.Add($record)
 try{
  Wait-Until {
   $parent=if($Id -like 'canvas-bar-*'){Bar}else{$root}
   if(!$parent){return $false}
   $item=Find $Id -Within $parent
   if(!$item -or $item.Current.IsOffscreen -or !$item.Current.IsEnabled){return $false}
   $bounds=$item.Current.BoundingRectangle;$parentBounds=$parent.Current.BoundingRectangle
   $record.parent=$parentBounds;$record.bounds=$bounds
   if($bounds.IsEmpty -or $bounds.Width -le 0 -or $bounds.Height -le 0 -or !$parentBounds.Contains($bounds)){return $false}
   $view=(Model).state.canvas_bar|Select-Object context,anchor,placement|ConvertTo-Json -Depth 5 -Compress
   Wait-PresentedFrame
   $parent=if($Id -like 'canvas-bar-*'){Bar}else{$root}
   if(!$parent -or $parent.Current.BoundingRectangle -ne $parentBounds){return $false}
   $item=Find $Id -Within $parent
   if(!$item -or $item.Current.IsOffscreen -or !$item.Current.IsEnabled -or $item.Current.BoundingRectangle -ne $bounds){return $false}
   if(((Model).state.canvas_bar|Select-Object context,anchor,placement|ConvertTo-Json -Depth 5 -Compress) -cne $view){return $false}
   $target.point=Center $item;$record.before=Input-Trace $target.point[0] $target.point[1]
   $record.before.foreground -eq $drawingWindow.ToInt64() -and @($record.before.hit|Where-Object {$_.process -eq $review.Id}).Count -gt 0
  } "Native pointer target $Id did not arrange inside its current foreground owner" 10
  $record.down_ticks=[Diagnostics.Stopwatch]::GetTimestamp()
  [CapyRowPointer]::Down($Device,$target.point[0],$target.point[1]);Start-Sleep -Milliseconds 40
  [CapyRowPointer]::Up();$record.up_ticks=[Diagnostics.Stopwatch]::GetTimestamp()
  $record.after=Input-Trace $target.point[0] $target.point[1]
  $record.released=Native-ContactReceipt
 }finally{Save-InvokeDiagnostic}
}
function Subtool([string]$Command){
 $choice=Tool-Choice $Command
 $expected=@((Model).state.tool_set.($choice.list))[$choice.index]
 $action=$expected.action|ConvertTo-Json -Depth 10 -Compress
 $record=[ordered]@{command=$Command;id=$choice.id;label=$expected.label;action=$action;native_name=$null;after=$null}
 $toolChoices.Add($record)
 $target=@{item=$null}
 try{
  Wait-Until {
   $item=@((Model).state.tool_set.($choice.list))[$choice.index]
   $target.item=Find $choice.id
   if(!$target.item){return $false}
   $record.native_name=$target.item.Current.Name
   ($item.action|ConvertTo-Json -Depth 10 -Compress) -ceq $action -and $record.native_name -ceq $expected.label -and !$target.item.Current.IsOffscreen -and $target.item.Current.IsEnabled
  } "$Command did not receive its current native tool row"
  $target.item.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
  Wait-Until {
   $m=Model;$item=@($m.state.tool_set.($choice.list))[$choice.index]
   $record.after=@($m.state.commands|Where-Object id -eq $Command)[0]
   $record.after.selected -and $item.selected -and ($item.action|ConvertTo-Json -Depth 10 -Compress) -ceq $action
  } "$Command did not activate its requested shared tool"
 }finally{$toolChoices|ConvertTo-Json -Depth 12|Set-Content -LiteralPath (Join-Path $run 'tool-choices.json')}
}
function Capture-CanvasParity([string]$Context){
 $captureTransparency=(Model).state.settings.transparency
 foreach($captureGlass in @('off','low')){
  Set-CapyTransparency $captureGlass
  Check-BarGeometry $Context
  $scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
  $metrics=@()
  foreach($item in @((Model).state.canvas_bar.items)+@((Model).state.canvas_bar.completion)){
   if($item.option.Choice){
    $choice=$item.option.Choice
    for($i=0;$i -lt @($choice.items).Count;$i++){
     $id="canvas-bar-choice-$($choice.id)-$i"
     $box=(Control $id -Arranged).Current.BoundingRectangle
     if([Math]::Abs($box.Height/$scale-26) -gt 1){throw "$id segment is not 26 DIP high"}
     $metrics+=@{id=$id;width_dip=$box.Width/$scale;height_dip=$box.Height/$scale}
    }
    continue
   }
   $id=if($item.menu){"canvas-bar-menu-$($item.menu)"}elseif($item.option.Action){"canvas-bar-$($item.option.Action.state.id)"}else{$null}
   if(!$id){continue};$control=Find $id;if(!$control -or $control.Current.IsOffscreen){continue}
   $box=$control.Current.BoundingRectangle
   $metrics+=@{id=$id;width_dip=$box.Width/$scale;height_dip=$box.Height/$scale;icon_only=[bool]$item.icon_only}
   if($item.menu){
    $images=$control.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::Image))
    foreach($image in $images){$bounds=$image.Current.BoundingRectangle;if($image.Current.IsOffscreen -or $bounds.IsEmpty){continue};if([Math]::Abs($bounds.Width/$scale-16) -gt 1 -or [Math]::Abs($bounds.Height/$scale-16) -gt 1){throw "$id menu glyph is not 16 DIP"};$metrics+=@{id=$id+'-glyph';width_dip=$bounds.Width/$scale;height_dip=$bounds.Height/$scale}}
   }
  }
  $metrics|ConvertTo-Json|Set-Content (Join-Path $run "$Context-metrics-$Theme-glass-$captureGlass.json")
  Capture ("$Context-$Theme-glass-$captureGlass") -WithModel -Composed
  if($Context -eq 'transform-segments'){
   $caption=Control 'canvas-bar-choice-transform-mode-0' -Arranged
   $null=Save-ControlCaptionInk $caption $drawingWindow (Join-Path $run "$Context-$Theme-glass-$captureGlass.png") (Model).state.palette.text (Join-Path $run "scale-caption-$Theme-glass-$captureGlass")
  }
 }
 Set-CapyTransparency $captureTransparency
}
function Check-BarGeometry([string]$Context){
 $scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
 $owner=Control 'canvas-action-bar' -Arranged
 $bar=$owner.Current.BoundingRectangle
 if([Math]::Abs($bar.Height/$scale-36) -gt 1){throw "The $Context bar is not one 36-point tile tall: $($bar.Height/$scale)"}
 foreach($item in @((Model).state.canvas_bar.items)+@((Model).state.canvas_bar.completion)){
  if($item.menu){$id="canvas-bar-menu-$($item.menu)"}elseif($item.option.Action){$id="canvas-bar-$($item.option.Action.state.id)"}else{continue}
  $control=Find $id;if(!$control -or $control.Current.IsOffscreen){continue}
  $r=$control.Current.BoundingRectangle;$want=if($item.icon_only){36}else{26}
  if([Math]::Abs($r.Height/$scale-$want) -gt 1){throw "$id in the $Context bar is $($r.Height/$scale) points tall, not $want"}
  if($item.icon_only -and [Math]::Abs($r.Width/$scale-36) -gt 1){throw "$id in the $Context bar is not a square tile"}
  if(!$item.icon_only -and ([Math]::Abs(($r.Top-$bar.Top)/$scale-5) -gt 1 -or [Math]::Abs(($bar.Bottom-$r.Bottom)/$scale-5) -gt 1)){throw "$id in the $Context bar does not keep five points clear"}
 }
 $checks["${Context}_bar_geometry"]='passed'
}
function Assert-AdjustProjection($Before,$After){
 $owner=@($Before.state.layers|Where-Object editing)
 $ids=@($Before.state.layers|ForEach-Object id)
 $added=@($After.state.layers|Where-Object {$_.id -notin $ids})
 if($owner.Count -ne 1 -or !$Before.state.layer_tools.has_selection -or @($After.state.layers).Count -ne $ids.Count+1 -or $added.Count -ne 1){throw 'Adjust did not add exactly one effect for the selected editing layer'}
 $effect=$added[0]
 if(!$effect.adjustment_effect -or !$effect.has_mask -or $null -eq $effect.mask_id -or !$effect.editing -or $effect.relationship.kind -ne 'effect' -or $effect.relationship.target -ne $owner[0].id -or $After.state.layer_tools.has_selection -or !@($After.state.commands|Where-Object id -eq 'undo')[0].enabled -or @($After.state.commands|Where-Object id -eq 'redo')[0].enabled){throw 'Adjust did not attach the selection-masked effect to its editing owner in one new history step'}
 foreach($id in $ids){if(@($After.state.layers|Where-Object id -eq $id).Count -ne 1){throw 'Adjust replaced an existing layer'}}
 $effect
}
function Bake-Layers($Snapshot){ConvertTo-Json -InputObject @($Snapshot.state.layers|Select-Object * -ExcludeProperty paint_revision,mask_revision) -Depth 20 -Compress}
function Assert-LockProjection($Model,[bool]$Expected,[int]$NativeState,[bool]$NativeEnabled){
 $command=@($Model.state.commands|Where-Object id -eq 'transform_uniform')
 $item=@($Model.state.canvas_bar.items|Where-Object {$_.option.Action.state.id -eq 'transform_uniform'})
 if($command.Count -ne 1 -or $item.Count -ne 1 -or !$command[0].enabled -or !$item[0].option.Action.checkable -or [bool]$command[0].selected -ne $Expected -or [bool]$item[0].option.Action.state.selected -ne $Expected -or !$NativeEnabled -or $NativeState -ne [int]$Expected){throw 'Keep proportions differs between the shared command, bar and native toggle'}
}
function Assert-ModeProjection($Model,[bool]$Segmented,[int]$Selected){
 $modes=@($Model.state.canvas_bar.items|Where-Object {$_.option.Choice.id -eq 'transform-mode'})
 if($modes.Count -ne 1){throw 'Transform did not publish one mode choice'}
 $mode=$modes[0].option.Choice;$items=@($mode.items)
 if([bool]$mode.segmented -ne $Segmented -or ($items.label -join '|') -cne 'Scale|Distort|Warp' -or @($items|Where-Object selected).Count -ne 1 -or !$items[$Selected].selected){throw 'The shared transform mode projection differs'}
 $mode
}
function Check-Lock([bool]$Expected){
 $native=Control 'canvas-bar-transform_uniform' -Arranged
 Assert-LockProjection (Model) $Expected ([int]$native.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Current.ToggleState) $native.Current.IsEnabled
}
function Drag([string]$Device,[int[]]$From,[int[]]$To){
 [CapyRowPointer]::Down($Device,$From[0],$From[1])
 for($step=1;$step -le 12;$step++){[CapyRowPointer]::Move([int]($From[0]+($To[0]-$From[0])*$step/12),[int]($From[1]+($To[1]-$From[1])*$step/12));Start-Sleep -Milliseconds 12}
 [CapyRowPointer]::Up();Start-Sleep -Milliseconds 150
}
try {
 Enter-CapyEnvironment
 $env:CAPY_STORAGE_DIR=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1';$env:CAPY_SMOKE_TEST='1'
 [IO.File]::WriteAllText((Settings-File),(@{language=@{Explicit='en'};theme=$Theme}|ConvertTo-Json -Compress))
 $review=Start-Process -FilePath $Executable -WorkingDirectory $run -PassThru -RedirectStandardError (Join-Path $run 'stderr.log')
 $null=$review.Handle
 Write-Output "Owned canvas bar review $($review.Id): $run"
 $native=@{window=$null}
 Wait-Until {$native.window=Owned-DrawingWindow $review;$null -ne $native.window} 'Review did not start' 45
 $root=$native.window.Root;$drawingWindow=$native.window.Handle
 $invokeTrace.Start($root,[int]$review.Id)
 if(!$parityCapture){Use-PaintWorkspace}
 Wait-Until {(Model).brush_ready -and (Model).windows_workspace.ready -and !(Model).windows_workspace.busy -and (Model).state.theme -eq $Theme} 'Canvas bar review did not start in the requested theme'
 $checks.initial_theme=$Theme
 $root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Maximized)
 [CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
 [CapyRowPointer]::SetForegroundWindow($drawingWindow)|Out-Null
 [CapyRowPointer]::Initialize([uint32]$review.Id)
 Wait-Until {$b=(Find 'drawing-canvas').Current.BoundingRectangle;$b.Width -gt 1200} 'Maximized canvas did not settle' 10
 Fit-Canvas;Start-Sleep -Milliseconds 300
 (Find 'Test stroke' -Name).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
 Wait-Until {(Model).state.document_file.modified} 'Controlled drawing did not finish'
 $settled=@{rect=$null};Wait-Until {$settled.rect=(Find 'drawing-canvas').Current.BoundingRectangle;$settled.rect.Width -gt 1200} 'The canvas bounds did not settle after drawing' 10
 $canvas=$settled.rect
 $cx=[int]($canvas.X+$canvas.Width*.5);$cy=[int]($canvas.Y+$canvas.Height*.45)
 if(Bar){throw 'The bar showed before any selection or transform'}
 $preparedPaint=Paint
 Drag 'mouse' @(($cx-110),($cy-75)) @(($cx+110),($cy+45))
 Wait-Until {(Paint) -ne $preparedPaint} 'Controlled two-dimensional drawing did not finish'

 $selectionTile=Tool-Tile 'lasso';Invoke-Id $selectionTile;Subtool 'rectangle_select'
 $selection=@(@(($cx-160),($cy-110)),@(($cx+160),($cy+60)))
 Drag 'mouse' $selection[0] $selection[1]
 Wait-Until {(Model).state.layer_tools.has_selection -and (Kind) -eq 'selection' -and (Bar)} 'The selection bar did not appear' 10
 $bar=(Bar).Current.BoundingRectangle
 if($bar.Top -lt $selection[1][1]){throw "The selection bar is not below the selection: $($bar.Top) < $($selection[1][1])"}
 if([Math]::Abs(($bar.X+$bar.Width/2)-$cx) -gt 40){throw 'The selection bar is not centred under the selection'}
 $paint=Paint
 if($parityCapture){
  Capture-CanvasParity 'selection'
  Tap 'canvas-bar-scale_rotate' 'mouse'
  Wait-Until {(Kind) -eq 'transform' -and (Bar)} 'Transform did not open for parity capture'
  $null=Assert-ModeProjection (Model) $true 0
  Capture-CanvasParity 'transform-segments'
  Tap 'canvas-bar-cancel_transform' 'mouse'
  Wait-Until {(Kind) -ne 'transform'} 'Parity transform did not cancel'
  Close-CanvasReview
  return
 }
 if(!$tailOnly){
 $glass=@{snapshot=$null;key='';stable=0}
 Wait-Until {$m=Model;$key=($m.windows_glass|Select-Object regions,chrome_revision|ConvertTo-Json -Compress);if($key -ceq $glass.key){$glass.stable++}else{$glass.key=$key;$glass.stable=0};$glass.snapshot=$m;$glass.stable -ge 3} 'Selection bar glass did not settle' 5
 $visibleGlass=[int]$glass.snapshot.windows_glass.regions;$glassState=Glass-State $glass.snapshot;$glassAnchor=$glass.snapshot.state.canvas_bar.anchor -join ','
 $glassProbe=[ordered]@{visible=$glass.snapshot.windows_glass;state=$glassState;anchor=$glassAnchor}
 try{
  Bar-Visibility $false
  Wait-Until {$m=Model;(Glass-State $m) -ceq $glassState -and [int]$m.windows_glass.regions -eq $visibleGlass-1} 'Hiding the selection bar did not remove exactly its glass region' 5
  $glassProbe.hidden=(Model).windows_glass
  Bar-Visibility $true
  Wait-Until {$m=Model;(Glass-State $m) -ceq $glassState -and ($m.state.canvas_bar.anchor -join ',') -ceq $glassAnchor -and [int]$m.windows_glass.regions -eq $visibleGlass} 'Showing the selection bar did not restore exactly its glass region and selection' 5
  $glassProbe.restored=(Model).windows_glass
 }finally{$glassProbe.current=(Model).windows_glass;$glassProbe|ConvertTo-Json -Depth 6|Set-Content (Join-Path $run 'selection-bar-glass-registration.json')}
 Capture-CanvasParity 'selection'
 Capture ('selection-'+$Theme)
 $checks.selection_below_object_with_glass='passed'

 $invertAnchor=(Model).state.canvas_bar.anchor -join ',';$invertCheckpoint=Drawing-Checkpoint
 $invertBounds=(Control 'canvas-action-bar' -Arranged).Current.BoundingRectangle
 $invertPixels=Wait-StablePixels {((Screen-Pixels $cx ($cy-25))+':'+(Screen-Pixels ($cx+240) ($cy-25)))}
 foreach($device in @('mouse','touch','pen')){
  Wait-Until {$b=Bar;$b -and $b.Current.BoundingRectangle.Top -eq $invertBounds.Top} "$device selection bar did not arrange at its original anchor" 5
  $invertRevision=(Model).state.document_file.revision
  Tap 'canvas-bar-invert_selection' $device
  Wait-Until {$m=Model;$b=Bar;$m.state.document_file.revision -eq $invertRevision+1 -and $m.state.canvas_bar.placement -eq 'bottom_edge' -and $b -and $b.Current.BoundingRectangle.Top -gt $invertBounds.Bottom} "$device Invert did not arrange the bar at the bottom edge" 5
  $barTaps[$barTaps.Count-1].acknowledged_ticks=[Diagnostics.Stopwatch]::GetTimestamp()
  Tap 'canvas-bar-invert_selection' $device
  Wait-Until {$m=Model;$m.state.document_file.revision -eq $invertRevision+2 -and $m.state.layer_tools.has_selection -and $m.state.canvas_bar.placement -eq 'near_object' -and ($m.state.canvas_bar.anchor -join ',') -ceq $invertAnchor} "$device second Invert did not restore the exact selection anchor" 5
  $barTaps[$barTaps.Count-1].acknowledged_ticks=[Diagnostics.Stopwatch]::GetTimestamp()
  (Control 'drawing-canvas').SetFocus()
  [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),[uint16]0x5A)
  Wait-Until {$m=Model;$m.state.canvas_bar.placement -eq 'bottom_edge' -and $null -eq $m.state.canvas_bar.anchor -and @($m.state.commands|Where-Object id -eq 'redo')[0].enabled} "$device Invert Undo did not restore the inverted selection" 5
  [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),[uint16]0x59)
  Wait-Until {$m=Model;$m.state.layer_tools.has_selection -and $m.state.canvas_bar.placement -eq 'near_object' -and ($m.state.canvas_bar.anchor -join ',') -ceq $invertAnchor -and !@($m.state.commands|Where-Object id -eq 'redo')[0].enabled} "$device Invert Redo did not restore the exact selection and history" 5
  if((Drawing-Checkpoint) -cne $invertCheckpoint -or (Paint) -cne $paint -or (Wait-StablePixels {((Screen-Pixels $cx ($cy-25))+':'+(Screen-Pixels ($cx+240) ($cy-25)))}) -cne $invertPixels){throw "$device Invert history changed authored artwork or exact composed pixels"}
 }
 if((Paint) -ne $paint){throw 'Bar taps painted on the canvas'}
 $checks.mouse_touch_pen_taps_do_not_paint='passed'
 }
 if($invertOnly){Close-CanvasReview;return}

 foreach($device in @('mouse','touch','pen')){
  Tap 'canvas-bar-menu-refine' $device
  Wait-Until {MenuOpen} "$device did not open the Refine menu" 5
  [CapyRowPointer]::Key([uint32]$review.Id,0x1B)
  Wait-Until {!(MenuOpen)} "Escape did not close the Refine menu" 5
 }
 $layers=@((Model).state.layers).Count
 Tap 'canvas-bar-menu-copy' 'touch'
 Wait-Until {$null -ne (Owned 'Copy Selection to New Layer' -Name)} 'The Copy menu has no Copy Selection to New Layer' 5
 (Owned 'Copy Selection to New Layer' -Name).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
 Wait-Until {@((Model).state.layers).Count -eq $layers+1 -and !(MenuOpen)} 'Copy Selection to New Layer did not add a layer through the bar menu' 5
 (Control 'drawing-canvas').SetFocus()
 [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),[uint16]0x5A)
 Wait-Until {@((Model).state.layers).Count -eq $layers -and (Paint) -eq $paint} 'One Undo did not remove the copied layer' 5
 $checks.selection_menus_copy_to_layer_one_undo_step='passed'
 Check-BarGeometry 'selection'

 $adjustBefore=Model;$selectionCorners=@($adjustBefore.state.canvas_bar.anchor);$selectionAnchor=$selectionCorners -join ','
 $adjustLayers=$adjustBefore.state.layers|ConvertTo-Json -Depth 20 -Compress
 $adjustUndo=[bool]@($adjustBefore.state.commands|Where-Object id -eq 'undo')[0].enabled
 $away=Center (Control $selectionTile -Arranged);[CapyRowPointer]::Hover($away[0],$away[1])
 $sampleY=$cy-25
 $adjustPixels=Wait-StablePixels {((Screen-Pixels $cx $sampleY)+':'+(Screen-Pixels ($cx+240) $sampleY))}
 Tap 'canvas-bar-menu-adjust' 'mouse'
 Wait-Until {$null -ne (Owned 'Tone' -Name -Type ([System.Windows.Automation.ControlType]::MenuItem))} 'Adjust did not offer Tone' 5
 (Owned 'Tone' -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)).GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
 Wait-Until {$null -ne (Owned 'Curves' -Name -Type ([System.Windows.Automation.ControlType]::MenuItem))} 'Adjust did not offer Curves' 5
 (Owned 'Curves' -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
 Wait-Until {@((Model).state.layers).Count -eq @($adjustBefore.state.layers).Count+1 -and !(MenuOpen) -and !(Model).state.layer_tools.has_selection} 'Adjust did not consume the selection into one new effect' 10
 $adjusted=Model;$adjustment=Assert-AdjustProjection $adjustBefore $adjusted
 $effectLayers=$adjusted.state.layers|ConvertTo-Json -Depth 20 -Compress
 $mask=Control ("layer-$($adjustment.id)-mask") -Arranged
 $at=Center $mask;[CapyRowPointer]::Hover($at[0],$at[1]);[CapyRowPointer]::Hold(0x11,$true)
 try{Tap ("layer-$($adjustment.id)-mask") 'mouse'}finally{[CapyRowPointer]::Hold(0x11,$false)}
 Wait-Until {
  $m=Model;$corners=@($m.state.canvas_bar.anchor)
  $m.state.layer_tools.has_selection -and $m.state.canvas_bar.context.kind -eq 'selection' -and $corners.Count -eq 4 -and @((0..3)|Where-Object {
   [double]::IsNaN($corners[$_]) -or [double]::IsInfinity($corners[$_]) -or $corners[$_] -ne [Math]::Truncate($corners[$_]) -or [Math]::Abs($corners[$_]-$selectionCorners[$_]) -gt 1
  }).Count -eq 0
 } 'The new effect mask did not reload the rectangle within pixel sampling bounds' 10
 (Control 'drawing-canvas').SetFocus()
 [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),[uint16]0x5A)
 Wait-Until {$m=Model;!$m.state.layer_tools.has_selection -and ($m.state.layers|ConvertTo-Json -Depth 20 -Compress) -ceq $effectLayers -and @($m.state.commands|Where-Object id -eq 'undo')[0].enabled -and @($m.state.commands|Where-Object id -eq 'redo')[0].enabled} 'One Undo did not remove only the loaded mask selection and preserve the effect history' 5
 [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),[uint16]0x5A)
 Wait-Until {$m=Model;($m.state.layers|ConvertTo-Json -Depth 20 -Compress) -ceq $adjustLayers -and (Paint) -eq $paint -and $m.state.layer_tools.has_selection -and ($m.state.canvas_bar.anchor -join ',') -ceq $selectionAnchor -and [bool]@($m.state.commands|Where-Object id -eq 'undo')[0].enabled -eq $adjustUndo -and @($m.state.commands|Where-Object id -eq 'redo')[0].enabled} 'One Adjust Undo did not restore the exact layers, editing owner, selection and history availability' 5
 [CapyRowPointer]::Hover($away[0],$away[1])
 if((Wait-StablePixels {((Screen-Pixels $cx $sampleY)+':'+(Screen-Pixels ($cx+240) $sampleY))}) -cne $adjustPixels){throw 'One Adjust Undo did not restore the exact composed interior and exterior pixels'}
 Capture ('selection-adjust-undo-'+$Theme) -WithModel -Composed
 $checks.adjust_attached_selection_mask_one_undo='passed'
 $bakeBefore=Model;$bakeLayers=Bake-Layers $bakeBefore
 $bakeUndo=[bool]@($bakeBefore.state.commands|Where-Object id -eq 'undo')[0].enabled
 $bakePixels=Wait-StablePixels {((Screen-Pixels $cx $sampleY)+':'+(Screen-Pixels ($cx+240) $sampleY))}
 Tap 'canvas-bar-menu-adjust' 'mouse'
 Wait-Until {$null -ne (Owned 'Color' -Name -Type ([System.Windows.Automation.ControlType]::MenuItem))} 'Adjust did not offer Color' 5
 (Owned 'Color' -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)).GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
 Wait-Until {$null -ne (Owned 'Invert' -Name -Type ([System.Windows.Automation.ControlType]::MenuItem))} 'Adjust did not offer Invert' 5
 (Owned 'Invert' -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
 Wait-Until {$m=Model;@($m.state.layers).Count -eq @($bakeBefore.state.layers).Count+1 -and !$m.state.layer_tools.has_selection -and $m.brush_ready} 'Selection-masked Invert did not settle'
 $bakeEffect=Model;$null=Assert-AdjustProjection $bakeBefore $bakeEffect
 $bakeEffectLayers=Bake-Layers $bakeEffect
 [CapyRowPointer]::Hover($away[0],$away[1])
 $bakeEffectPixels=Wait-StablePixels {((Screen-Pixels $cx $sampleY)+':'+(Screen-Pixels ($cx+240) $sampleY))}
 if($bakeEffectPixels.Split(':')[0] -ceq $bakePixels.Split(':')[0] -or $bakeEffectPixels.Split(':')[1] -cne $bakePixels.Split(':')[1]){throw 'Selection-masked Invert did not change only the composed interior sample'}
 $merge=@($bakeEffect.state.commands|Where-Object id -eq 'merge_visible')
 if($merge.Count -ne 1 -or !$merge[0].enabled){throw 'The masked effect does not offer Merge Visible'}
 $bakeOwner=@($bakeEffect.state.layers|Where-Object editing)[0].id
 (Control ("layer-$bakeOwner-name") -Arranged).SetFocus();[CapyRowPointer]::Key([uint32]$review.Id,0x5D)
 Wait-Until {$null -ne (Owned $merge[0].label -Name -Type ([System.Windows.Automation.ControlType]::MenuItem))} 'Layer menu did not expose Merge Visible' 5
 (Owned $merge[0].label -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
 Wait-Until {$m=Model;$m.brush_ready -and !$m.state.document_file.busy -and !@($m.state.requests).Count -and !(MenuOpen) -and @($m.state.layers).Count -lt @($bakeEffect.state.layers).Count -and !@($m.state.layers|Where-Object adjustment_effect).Count} 'Merge Visible did not settle the masked effect into paint' 60
 if(((Model).state.camera|ConvertTo-Json -Compress) -cne ($bakeEffect.state.camera|ConvertTo-Json -Compress)){throw 'Merge Visible changed the sampled camera'}
 if((Wait-StablePixels {((Screen-Pixels $cx $sampleY)+':'+(Screen-Pixels ($cx+240) $sampleY))}) -cne $bakeEffectPixels){throw 'Merge Visible changed exact composed masked-effect pixels'}
 $bakedOrder=@((Model).state.layers|ForEach-Object id) -join ','
 (Control 'drawing-canvas').SetFocus();[CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),0x5A)
 Wait-Until {$m=Model;$m.brush_ready -and (Bake-Layers $m) -ceq $bakeEffectLayers -and @($m.state.commands|Where-Object id -eq 'undo')[0].enabled -and @($m.state.commands|Where-Object id -eq 'redo')[0].enabled} 'One Merge Undo did not restore exact effect/mask layers and history'
 if((Wait-StablePixels {((Screen-Pixels $cx $sampleY)+':'+(Screen-Pixels ($cx+240) $sampleY))}) -cne $bakeEffectPixels){throw 'Merge Undo changed exact masked-effect pixels'}
 [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11,0x10),0x5A)
 Wait-Until {$m=Model;$m.brush_ready -and ((@($m.state.layers)|ForEach-Object id) -join ',') -ceq $bakedOrder -and @($m.state.commands|Where-Object id -eq 'undo')[0].enabled -and !@($m.state.commands|Where-Object id -eq 'redo')[0].enabled} 'Merge Redo did not restore the baked layer identities and history'
 if((Wait-StablePixels {((Screen-Pixels $cx $sampleY)+':'+(Screen-Pixels ($cx+240) $sampleY))}) -cne $bakeEffectPixels){throw 'Merge Redo changed exact masked-effect pixels'}
 $bakedOwner=@((Model).state.layers|Where-Object editing)
 if($bakedOwner.Count -ne 1 -or $bakedOwner[0].adjustment_effect -or !$bakedOwner[0].visible){throw 'Merge did not select one visible paint owner'}
 $bakedVisibility="layer-$($bakedOwner[0].id)-visibility"
 Tap $bakedVisibility 'mouse';[CapyRowPointer]::Hover($away[0],$away[1])
 Wait-Until {!@((Model).state.layers|Where-Object {$_.id -eq $bakedOwner[0].id})[0].visible} 'Baked paint did not hide'
 if((Wait-StablePixels {((Screen-Pixels $cx $sampleY)+':'+(Screen-Pixels ($cx+240) $sampleY))}) -ceq $bakeEffectPixels){throw 'Hiding baked paint did not replace its composed pixels'}
 Tap $bakedVisibility 'mouse';[CapyRowPointer]::Hover($away[0],$away[1])
 Wait-Until {@((Model).state.layers|Where-Object {$_.id -eq $bakedOwner[0].id})[0].visible -and (Model).brush_ready} 'Baked paint did not show'
 if((Wait-StablePixels {((Screen-Pixels $cx $sampleY)+':'+(Screen-Pixels ($cx+240) $sampleY))}) -cne $bakeEffectPixels){throw 'Fresh baked paint redraw did not restore exact masked-effect pixels'}
 (Control 'drawing-canvas').SetFocus();[CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),0x5A)
 Wait-Until {!@((Model).state.layers|Where-Object {$_.id -eq $bakedOwner[0].id})[0].visible} 'One visibility Undo did not hide baked paint'
 [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),0x5A)
 Wait-Until {@((Model).state.layers|Where-Object {$_.id -eq $bakedOwner[0].id})[0].visible} 'Second visibility Undo did not restore baked paint'
 [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),0x5A)
 Wait-Until {(Bake-Layers (Model)) -ceq $bakeEffectLayers} 'Merge cleanup Undo did not restore the effect'
 [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),0x5A)
 Wait-Until {$m=Model;(Bake-Layers $m) -ceq $bakeLayers -and $m.state.layer_tools.has_selection -and ($m.state.canvas_bar.anchor -join ',') -ceq $selectionAnchor -and [bool]@($m.state.commands|Where-Object id -eq 'undo')[0].enabled -eq $bakeUndo -and @($m.state.commands|Where-Object id -eq 'redo')[0].enabled} 'Masked-bake cleanup did not restore selection, owner and history'
 if((Wait-StablePixels {((Screen-Pixels $cx $sampleY)+':'+(Screen-Pixels ($cx+240) $sampleY))}) -cne $bakePixels){throw 'Masked-bake cleanup changed original pixels'}
 Capture ('selection-masked-bake-undo-redo-'+$Theme) -WithModel -Composed
 $checks.selection_masked_invert_merge_visible_pixels_undo_redo='passed'
 $paint=Paint
 Tap 'canvas-bar-scale_rotate' 'mouse'
 Wait-Until {(Kind) -eq 'transform' -and (Bar) -and (Find 'canvas-bar-apply_transform') -and (Find 'canvas-bar-cancel_transform')} 'Transform from the bar did not open the transform bar' 10
 $width=Value 'transform_width'
 foreach($device in @('touch','pen','mouse')){
  $before=Value 'transform_width'
  Tap 'canvas-bar-transform_flip_horizontal' $device
  Wait-Until {[Math]::Sign((Value 'transform_width')) -eq -[Math]::Sign($before)} "$device Flip H did not flip the transform" 5
 }
 if((Kind) -ne 'transform'){throw 'Flipping left the transform'}
 $choices=@((Model).state.canvas_bar.items|Where-Object {$_.option.Choice}|ForEach-Object {$_.option.Choice})
 $mode=@($choices|Where-Object segmented)[0]
 if(!$mode){throw 'The transform bar has no segmented mode choice'}
 $null=Assert-ModeProjection (Model) $true 0
 Capture-CanvasParity 'transform-segments'
 $lockBefore=[bool]@((Model).state.commands|Where-Object id -eq 'transform_uniform')[0].selected
 if(!$lockBefore){Tap 'canvas-bar-transform_uniform' 'mouse';Wait-Until {@((Model).state.commands|Where-Object id -eq 'transform_uniform')[0].selected} 'Keep proportions did not turn on' 5}
 Check-Lock $true
 Tap ("canvas-bar-choice-"+$mode.id+"-1") 'touch'
 Wait-Until {@((Choice $mode.id).items)[1].selected -and !(Find 'canvas-bar-transform_uniform')} 'Distort did not hide the Scale lock' 5
 Tap ("canvas-bar-choice-"+$mode.id+"-0") 'pen'
 Wait-Until {@((Choice $mode.id).items)[0].selected -and (Find 'canvas-bar-transform_uniform')} 'Scale did not restore its lock' 5
 Check-Lock $true
 Tap 'canvas-bar-transform_uniform' 'touch'
 Wait-Until {!@((Model).state.commands|Where-Object id -eq 'transform_uniform')[0].selected} 'Keep proportions did not turn off' 5
 Check-Lock $false
 if($lockBefore){Tap 'canvas-bar-transform_uniform' 'pen';Wait-Until {@((Model).state.commands|Where-Object id -eq 'transform_uniform')[0].selected} 'Keep proportions did not restore its original value' 5;Check-Lock $true}
 $checks.keep_proportions_independent_lock='passed'

 $wideBounds=(Find 'drawing-canvas').Current.BoundingRectangle
 $scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
 $root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Normal)
 & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -StateDirectory $run -Action Resize -Width ([int](700*$scale)) -Height ([int](800*$scale))
 Wait-Until {(Choice $mode.id) -and !(Choice $mode.id).segmented -and (Bar)} 'The narrow transform did not publish its mode dropdown' 10
 $narrow=Assert-ModeProjection (Model) $false 0
 Tap ("canvas-bar-choice-"+$narrow.id) 'mouse'
 Wait-Until {$null -ne (Owned $narrow.items[1].label -Name -Type ([System.Windows.Automation.ControlType]::MenuItem))} 'The narrow mode menu has no Distort' 5
 $nativeMode=Owned $narrow.items[1].label -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)
 $modeMenu=[System.Windows.Automation.TreeWalker]::RawViewWalker.GetParent($nativeMode)
 Invoke $narrow.items[1].label -Name -Within $modeMenu
 Wait-Until {@((Choice $mode.id).items)[1].selected -and !(MenuOpen)} 'The narrow menu did not select Distort' 5
 $null=Assert-ModeProjection (Model) $false 1
 Capture ('transform-narrow-'+$Theme) -WithModel -Composed
 $root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Maximized)
 Wait-Until {(Choice $mode.id).segmented -and (Find 'drawing-canvas').Current.BoundingRectangle -eq $wideBounds -and (Bar)} 'The restored window did not retain its wide transform bar and canvas bounds' 10
 $null=Assert-ModeProjection (Model) $true 1
 Tap ("canvas-bar-choice-"+$mode.id+"-0") 'pen'
 Wait-Until {@((Choice $mode.id).items)[0].selected} 'The wide mode row did not restore Scale' 5
 Check-Lock $lockBefore
 if((Paint) -ne $paint){throw 'Lock and narrow mode controls changed paint revisions'}
 $checks.narrow_mode_dropdown_and_wide_restore='passed'
 Tap ("canvas-bar-choice-"+$mode.id+"-1") 'touch'
 Wait-Until {@((Choice $mode.id).items)[1].selected} 'Touch did not select the second transform mode' 5
 Tap ("canvas-bar-choice-"+$mode.id+"-0") 'pen'
 Wait-Until {@((Choice $mode.id).items)[0].selected} 'Pen did not restore the first transform mode' 5
 foreach($dropdown in @($choices|Where-Object {!$_.segmented})){
  $control=Find ("canvas-bar-choice-"+$dropdown.id)
  if(!$control -or $control.Current.IsOffscreen){continue}
  Tap ("canvas-bar-choice-"+$dropdown.id) 'mouse'
  Wait-Until {MenuOpen} "The $($dropdown.label) choice did not open its menu" 5
  [CapyRowPointer]::Key([uint32]$review.Id,0x1B)
  Wait-Until {!(MenuOpen)} "Escape did not close the $($dropdown.label) menu" 5
 }
 $checks.transform_mode_choices='passed'
 Check-BarGeometry 'transform'
 $x=Value 'transform_x'
 $inside=AnchorQuadrant
 Drag 'touch' $inside @(($inside[0]+80),$inside[1])
 Wait-Until {[Math]::Abs((Value 'transform_x')-$x) -gt 1 -and (Bar)} 'A finger drag did not move the transform body' 5
 if((Kind) -ne 'transform'){throw 'The finger drag ended the transform'}
 $checks.finger_moves_transform='passed'
 if((Paint) -ne $paint){throw 'Transform bar taps painted on the canvas'}
 Capture ('transform-'+$Theme)
 $checks.transform_bar_and_flip='passed'

 [CapyRowPointer]::Down('pen',$cx,($cy-25))
 for($step=1;$step -le 8;$step++){[CapyRowPointer]::Move($cx+10*$step,$cy-25);Start-Sleep -Milliseconds 15}
 Wait-Until {!(Bar)} 'The bar stayed visible during a canvas contact' 3
 [CapyRowPointer]::Up()
 Wait-Until {(Bar)} 'The bar did not return after the contact' 5
 if((Kind) -ne 'transform'){throw 'The contact ended the transform'}
 $checks.hides_during_contact_and_returns='passed'

 Tap 'canvas-bar-more' 'mouse'
 Wait-Until {$null -ne (Owned 'show_canvas_action_bar')} 'More did not open the bar menu' 5
 [CapyRowPointer]::Key([uint32]$review.Id,0x1B)
 Wait-Until {$null -eq (Owned 'show_canvas_action_bar')} 'Escape did not close the bar menu' 5
 if((Kind) -ne 'transform'){throw 'Closing More cancelled the transform'}
 $checks.more_menu='passed'

 $revision=(Model).state.document_file.revision
 Tap 'canvas-bar-apply_transform' 'touch'
 Wait-Until {(Kind) -ne 'transform' -and (Model).state.document_file.revision -ne $revision} 'Apply on the bar did not commit the transform' 10
 $applied=Paint
 (Control 'drawing-canvas').SetFocus()
 [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),[uint16]0x5A)
 Wait-Until {(Paint) -eq $paint} 'One Undo did not restore the pixels before the transform' 5
 [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),[uint16]0x59)
 Wait-Until {(Paint) -ne $paint -and !((Model).state.commands|Where-Object id -eq 'redo').enabled} 'Redo did not reapply the transform' 5
 $checks.apply_one_undo_step='passed'

 $before=Paint;$revision=(Model).state.document_file.revision
 (Control 'drawing-canvas').SetFocus();[CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),[uint16]0x54)
 Wait-Until {(Kind) -eq 'transform' -and (Bar)} 'Ctrl+T did not start a transform' 10
 $modeId=@((Model).state.canvas_bar.items|Where-Object {$_.option.Choice.segmented})[0].option.Choice.id
 $warp=[array]::IndexOf(@((Choice $modeId).items|ForEach-Object label),'Warp')
 if($warp -lt 0){throw 'The transform mode choice has no Warp'}
 Tap ("canvas-bar-choice-$modeId-$warp") 'touch'
 Wait-Until {@((Choice $modeId).items)[$warp].selected -and @((Model).state.canvas_bar.items|Where-Object {$_.option.Choice -and !$_.option.Choice.segmented -and $_.option.Choice.label -eq 'Grid'}).Count} 'Warp did not offer its Grid choice' 5
 Capture 'warp-dark'
 (Control 'drawing-canvas').SetFocus();[CapyRowPointer]::Key([uint32]$review.Id,0x1B)
 Wait-Until {(Kind) -ne 'transform'} 'Escape did not cancel the Warp transform' 5
 if((Paint) -ne $before -or (Model).state.document_file.revision -ne $revision){throw 'Cancelling the transform changed the drawing'}
 $checks.warp_mode_and_escape_cancel='passed'

 (Control 'drawing-canvas').SetFocus();[CapyRowPointer]::Key([uint32]$review.Id,0x09)
 Wait-Until {(Model).chrome_hidden -or (Model).state.workspace.zen_mode} 'Tab did not enter Zen' 5
 Wait-Until {(Bar)} 'The bar did not stay visible in Zen' 5
 Capture 'zen'
 (Control 'drawing-canvas').SetFocus();[CapyRowPointer]::Key([uint32]$review.Id,0x09)
 Wait-Until {!(Model).state.workspace.zen_mode} 'Tab did not leave Zen' 5
 $checks.visible_in_zen='passed'

 Subtool 'polygon_select'
 foreach($point in @(@(($cx-120),($cy-80)),@(($cx+120),($cy-80)),@(($cx+140),($cy+90)))){
  [CapyRowPointer]::Down('mouse',$point[0],$point[1]);Start-Sleep -Milliseconds 30;[CapyRowPointer]::Up();Start-Sleep -Milliseconds 120
 }
 Wait-Until {(Kind) -eq 'polygon' -and (Model).state.canvas_bar.placement -eq 'bottom_edge' -and (Bar)} 'The polygon bar did not appear at the bottom edge' 5
 Tap 'canvas-bar-remove_selection_point' 'touch'
 Wait-Until {(Kind) -eq 'polygon'} 'Remove Point ended the polygon' 3
 [CapyRowPointer]::Down('mouse',($cx-130),($cy+100));Start-Sleep -Milliseconds 30;[CapyRowPointer]::Up();Start-Sleep -Milliseconds 120
 Capture 'polygon-dark'
 Tap 'canvas-bar-complete_selection' 'pen'
 Wait-Until {(Kind) -eq 'selection' -and (Model).state.layer_tools.has_selection} 'Finish on the bar did not complete the polygon' 5
 $checks.polygon_bar='passed'

 $openColumns={@((Model).state.workspace.layout.column_stacks|Where-Object {$null -ne $_.open_column}).Count}
 $columnsBefore=& $openColumns
 $quickMask=((Model).state.commands|Where-Object id -eq 'quick_mask').label
 Tap 'canvas-bar-menu-mask' 'mouse'
 Wait-Until {$null -ne (Owned $quickMask -Name)} 'The Mask menu has no Quick Mask' 5
 (Owned $quickMask -Name).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
 Wait-Until {(Kind) -eq 'quick_mask' -and (Bar) -and !(MenuOpen)} 'Quick Mask from the Mask menu did not show its bar' 5
 Start-Sleep -Milliseconds 300
 Capture ('quick-mask-'+$Theme)
 Check-BarGeometry 'quick_mask'
 Tap 'canvas-bar-more' 'mouse'
 Wait-Until {MenuOpen} 'More did not open in Quick Mask' 5
 [CapyRowPointer]::Key([uint32]$review.Id,0x1B)
 Wait-Until {!(MenuOpen)} 'Escape did not close More in Quick Mask' 5
 if((Kind) -ne 'quick_mask'){throw 'Escape with a menu open also left Quick Mask'}
 (Control 'drawing-canvas').SetFocus();[CapyRowPointer]::Key([uint32]$review.Id,0x1B)
 Wait-Until {(Kind) -eq 'selection'} 'Escape did not leave Quick Mask for the selection' 5
 if((& $openColumns) -ne $columnsBefore){throw 'The Escape that left Quick Mask also changed the open columns'}
 if($columnsBefore -gt 0){
  (Control 'drawing-canvas').SetFocus();[CapyRowPointer]::Key([uint32]$review.Id,0x1B)
  Wait-Until {(& $openColumns) -eq 0} 'A second Escape did not close the open column' 5
  if((Kind) -ne 'selection'){throw 'Closing the column with Escape dropped the selection bar'}
 }
 $checks.escape_leaves_quick_mask_before_open_columns='passed'

 & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'Edit'
 Invoke-Id 'settings'
 Wait-Until {$null -ne (Find 'Color theme' -Name)} 'Preferences did not open' 10
 $themeControl=$root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.AndCondition]::new(
  [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,'Color theme'),
  [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::ComboBox)))
 $themeControl.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
 Wait-Until {$null -ne (Owned 'Light' -Name)} 'Light theme choice missing' 5
 (Owned 'Light' -Name).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
 Wait-Until {(Model).state.theme -eq 'light'} 'The theme did not switch to light' 5
 (Find 'CloseButton').GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
 Wait-Until {!(Model).preferences} 'Preferences did not close' 5
 Wait-Until {(Bar)} 'The bar did not return after the theme change' 5
 Start-Sleep -Milliseconds 300
 Capture 'selection-light'
 $checks.light_theme='passed'

 Close-CanvasReview
} catch {
 if($review -and !$review.HasExited){try{Capture 'failure'}catch{}}
 [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
} finally {
 $invokeTrace.Stop()
 Save-InvokeDiagnostic
 [CapyRowPointer]::Dispose()
 if($review -and !$review.HasExited){Stop-Process -Id $review.Id -Force}
 Exit-CapyEnvironment
}
