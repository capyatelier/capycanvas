param([Parameter(Mandatory)][string]$Executable,[ValidateSet('dark','light')][string]$Theme='dark',[switch]$Simple,[switch]$SimpleLight,[switch]$ThreeFinger)
if($SimpleLight){$Simple=$true;$Theme='light'}
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyPopups=$true;$CapyFind='prefer-visible'
Add-Type -Path (Join-Path $PSScriptRoot 'CanvasTouchDriver.cs')
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
$tokens=$null;$errors=$null;$preferences=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot 'exercise-navigation-controls.ps1'),[ref]$tokens,[ref]$errors)
if($errors.Count){throw 'Navigation preference helpers did not parse'}
foreach($name in @('Menu-Command','Preference','Open-CanvasPreferences','Preference-Switch','Set-PinchPreferences')){
 $function=@($preferences.FindAll({param($node)$node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name}.GetNewClosure(),$true))
 if($function.Count -ne 1){throw "Missing original navigation helper: $name"};. ([scriptblock]::Create($function[0].Extent.Text))
}
$CapyEach={
 [CapyCanvasTouch]::Verify()
 if($root){
  Assert-RendererHealthy $review.Id $native.window
  $status=Find 'canvas-status' -Visible
  if($status -and $status.Current.Name -like 'Canvas input queue is full.*'){throw $status.Current.Name}
 }
}
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path;$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/touch-gestures/'+[Guid]::NewGuid().ToString('N'));[IO.Directory]::CreateDirectory($run)|Out-Null
$checks=[Collections.Generic.List[object]]::new()
function Row($Id){@((Model).state.layers|Where-Object id -eq $Id)[0]}
function Point([string]$Id){$b=(Control $Id -Arranged).Current.BoundingRectangle;@{x=[int]($b.X+$b.Width*.5);y=[int]($b.Y+$b.Height*.5)}}
function Pair-OnRow($Id){$b=(Control "layer-$Id-name" -Arranged).Current.BoundingRectangle;if($b.Width -lt 70){throw 'Layer name is too narrow for two contacts'};@{ax=[int]($b.X+10);bx=[int]($b.X+55);y=[int]($b.Y+$b.Height*.5)}}
function Begin-Pair($p){[CapyCanvasTouch]::PairDown($p.ax,$p.y,$p.bx,$p.y)}
function End-Pair {[CapyCanvasTouch]::PairUp()}
function Record([string]$Name){$checks.Add(@{name=$Name;revision=(Model).state.document_file.revision;layers=(Model).state.layers});Write-Output "$Name passed"}
function Fit-GestureCanvas {
 & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'View'
 Invoke 'fit_canvas'
 Wait-Until {$m=Model;$tab=@($m.state.tabs|Where-Object active)[0];$c=$m.state.camera;$zoom=.9*[Math]::Min($c.work_area[2]/$tab.width,$c.work_area[3]/$tab.height);!(Find 'fit_canvas' -Visible) -and [Math]::Abs($c.zoom-$zoom) -lt .001} 'Fit Canvas did not acknowledge its camera'
}
function Simple-Layers {
 $choice=Find 'workspace-switch-builtin:workspace:painter' -Visible
 if(!$choice){Invoke 'header-workspace-menu';$choice=Control 'Sketch' -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)}
 $choice.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
 Wait-Until {(Model).windows_workspace.id -eq 'builtin:workspace:painter' -and (Model).windows_workspace.ready -and !(Model).windows_workspace.busy -and !(Model).windows_workspace.saving} 'Sketch workspace did not open'
 $bounds=(Control 'drawing-canvas' -Arranged).Current.BoundingRectangle
 $entry=@((Model).header.model.zones|ForEach-Object {$_}|Where-Object {$_.item.kind -eq 'tool' -and $_.item.control.kind -eq 'panel' -and $_.item.control.panel -eq 'layers'})[-1]
 $from=Point ('header-item-'+$entry.id);$to=@{x=[int]($bounds.Right-370);y=[int]($bounds.Y+400)}
 [CapyRowPointer]::Down('mouse',$from.x,$from.y);[CapyRowPointer]::Move($from.x,($from.y+80));[CapyRowPointer]::Move($to.x,$to.y);[CapyRowPointer]::Up()
 Wait-Until {@((Model).bubbles|Where-Object {$_.view.kind -eq 'layers' -and $_.view.card -and !$_.view.card.full}).Count -eq 1} 'Simple Layers bubble did not open'
 $bubble=@((Model).bubbles|Where-Object {$_.view.kind -eq 'layers'})[-1].view.id;$script:layerBubble=$bubble
 Control "bubble-card-$bubble" -Arranged|Out-Null
}
function Open-SimpleLayers {
 if(!$Simple){return}
 if(!@((Model).bubbles|Where-Object {$_.view.id -eq $script:layerBubble})[0].view.card){$p=Point "tool-bubble-$script:layerBubble";[CapyRowPointer]::Down('mouse',$p.x,$p.y);[CapyRowPointer]::Up()}
 Wait-Until {$packet=@((Model).bubbles|Where-Object {$_.view.id -eq $script:layerBubble})[0];$packet.view.card -and !$packet.view.card.full} 'Simple Layers card did not reopen'
 Control "bubble-card-$script:layerBubble" -Arranged|Out-Null
}
function Undo-To([scriptblock]$Expected,[string]$Message){$revision=(Model).state.document_file.revision;Invoke 'Undo' -Name;Wait-Until {(Model).state.document_file.revision -gt $revision -and (& $Expected)} $Message}
function Gesture-Camera{$c=(Model).state.camera;@($c.viewport,$c.work_area,$c.translation,$c.zoom,$c.rotation,$c.flipped)|ConvertTo-Json -Depth 8 -Compress}
function Gesture-History{
 $m=Model;$stamp=@($m.windows_tabs.session_stamps|Where-Object id -eq $m.windows_tabs.selected)
 if($stamp.Count -ne 1 -or $null -eq $stamp[0].stamp.checkpoint){throw 'Gesture history has no unique current drawing checkpoint'}
 [ordered]@{checkpoint=$stamp[0].stamp.checkpoint;layers=@($m.state.layers.id)}|ConvertTo-Json -Depth 30 -Compress
}
function Gesture-Pixels{
 if([CapyRowPointer]::GetForegroundWindow() -ne $handle){throw 'Gesture pixels lost the owned foreground window'}
 Screen-Pixels $cx $cy
}
function Assert-OrdinaryPinch($Before,$After,[double]$Ratio){
 foreach($value in @($Before.zoom,$After.zoom,$Before.rotation,$After.rotation,$Ratio)){if($null -eq $value -or [double]::IsNaN($value) -or [double]::IsInfinity($value)){throw 'Pinch camera comparison is not finite'}}
 if($Before.zoom -le 0 -or $After.zoom -le 0 -or [Math]::Abs($After.zoom/$Before.zoom-$Ratio) -gt .01 -or [Math]::Abs($After.rotation-$Before.rotation) -gt .001 -or ($After.flipped -join ',') -cne ($Before.flipped -join ',') -or ($After.viewport -join ',') -cne ($Before.viewport -join ',') -or ($After.work_area -join ',') -cne ($Before.work_area -join ',')){throw 'Disabled quick pinch did not retain ordinary zoom, rotation, flips and canvas allocation'}
}
$failed=$false
try{
 Enter-CapyEnvironment
 $env:CAPY_STORAGE_DIR=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1';$env:CAPY_SMOKE_TEST='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
 [IO.File]::WriteAllText((Settings-File),(@{language=@{Explicit='en'};theme=$Theme}|ConvertTo-Json -Depth 4))
 $stderr=Join-Path $run 'stderr.log'
 $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr -RedirectStandardOutput (Join-Path $run 'stdout.log');$null=$review.Handle
 $native=@{window=$null};Wait-Until {$native.window=Owned-DrawingWindow $review;$native.window -and (Model).brush_ready -and (Model).windows_workspace.ready -and !(Model).windows_workspace.busy} 'Gesture review did not start' 45
 $root=$native.window.Root;$handle=$native.window.Handle;$drawingWindow=$handle
 [CapyCanvasTouch]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
 [CapyCanvasTouch]::SetForegroundWindow($handle)|Out-Null
 [CapyRowPointer]::Initialize([uint32]$review.Id)
 Use-PaintWorkspace;Maximize-DrawingWindow
 if(Find 'column-icon-layers'){$p=Point 'column-icon-layers';[CapyRowPointer]::RightClick($p.x,$p.y);Invoke 'Expand column' -Name;Wait-Until {$null -ne (Find 'panel-tab-layers')} 'Layers column did not expand'}
 if(!@((Model).layout.groups|Where-Object active -eq 'layers').Count){Invoke 'Layers' -Name}
 if($Simple){Simple-Layers}
 $base=(Model).state.layer_tools.editing_layer.id
 foreach($i in 1..2){$count=(Model).state.layers.Count;Invoke 'layer-new';Wait-Until {(Model).state.layers.Count -eq $count+1} 'Gesture seed layer did not appear'}
 $id=(Model).state.layer_tools.editing_layer.id
 (Control 'drawing-canvas' -Arranged).SetFocus();[CapyRowPointer]::Key([uint32]$review.Id,0x1b)
 Wait-Until {(Control 'drawing-canvas').Current.HasKeyboardFocus -and (Model).canvas_ready -and !(Model).state.document_file.busy -and !(Model).state.customization.drawer} 'Canvas did not receive focus and finish preparation'
 $canvas=Point 'drawing-canvas';$area=(Model).state.camera.work_area;$bounds=(Control 'drawing-canvas' -Arranged).Current.BoundingRectangle
 $cx=[int]($bounds.X+$area[0]+$area[2]/2);$cy=[int]($bounds.Y+$area[1]+$area[3]/2)
 [CapyCanvasTouch]::Initialize([uint32]$review.Id)
 if($ThreeFinger){
 $allLayers=@((Model).state.layers.id)
 $historyCamera=Gesture-Camera
 [CapyCanvasTouch]::PairDown(($cx-80),$cy,($cx+80),$cy)
 Wait-Until {(Model).state.layers.Count -eq $allLayers.Count-2} 'Held two fingers did not repeat Undo'
 End-Pair
 Start-Sleep -Milliseconds 150
 if((Model).state.layers.Count -ne $allLayers.Count-2){throw 'Held Undo release added another tap'};Record 'held two-finger canvas Undo repeats'
 [CapyCanvasTouch]::ThreeDown($cx,$cy)
 Wait-Until {(@((Model).state.layers.id) -join ',') -eq ($allLayers -join ',')} 'Held three fingers did not repeat Redo'
 [CapyCanvasTouch]::ThreeUp()
 $restoredHistory=Gesture-History
 Record 'held canvas Undo and Redo repeat'
 [CapyCanvasTouch]::PairDown(($cx-80),$cy,($cx+80),$cy)
 Wait-Until {(Model).state.layers.Count -lt $allLayers.Count} 'Cancellation did not begin from effective held Undo'
 [CapyCanvasTouch]::CancelAll()
 [CapyCanvasTouch]::ThreeDown($cx,$cy)
 Wait-Until {(@((Model).state.layers.id) -join ',') -eq ($allLayers -join ',')} 'Redo after cancelled held Undo did not restore exact layer IDs'
 Wait-Until {(Gesture-History) -ceq $restoredHistory} 'New native Redo after CancelAll did not restore the exact drawing checkpoint and layer order'
 if((Gesture-Camera) -cne $historyCamera){throw 'New native Redo after CancelAll changed the camera'}
 [CapyCanvasTouch]::ThreeUp()
 Record 'CancelAll followed by effective native Redo and exact checkpoint recovery'
 }
 Fit-GestureCanvas
 $pixelBounds=(Control 'drawing-canvas' -Arranged).Current.BoundingRectangle
 $pixelCamera=(Model).state.camera;$pixelTab=@((Model).state.tabs|Where-Object active)[0]
 $factor=$pixelBounds.Width/$pixelCamera.viewport[0]
 $paper=[Windows.Rect]::new(($pixelBounds.X+$pixelCamera.translation[0]*$factor),($pixelBounds.Y+$pixelCamera.translation[1]*$factor),($pixelTab.width*$pixelCamera.zoom*$factor),($pixelTab.height*$pixelCamera.zoom*$factor))
 $sample=[Windows.Rect]::new(($cx-48),($cy-48),96,96)
 if(!$pixelBounds.Contains($sample) -or !$paper.Contains($sample)){throw 'Gesture seed sample is outside the visible owned paper'}
 [CapyRowPointer]::Hover([int]$pixelBounds.X+4,[int]$pixelBounds.Y+4)
 $blankPixels=Wait-StablePixels {Gesture-Pixels}
 $revision=(Model).state.document_file.revision;$paintRevision=(Row $id).paint_revision
 [CapyRowPointer]::Down('pen',($cx-30),$cy);for($i=1;$i -le 12;$i++){[CapyRowPointer]::Move(($cx-30+$i*5),$cy);Start-Sleep -Milliseconds 10};[CapyRowPointer]::Up()
 Wait-Until {(Model).state.document_file.revision -gt $revision -and (Row $id).paint_revision -ne $paintRevision -and @((Model).state.commands|Where-Object id -eq 'add_layer')[0].enabled} 'Content-selection seed stroke did not finish its painted revision and release'
 [CapyRowPointer]::Hover([int]$pixelBounds.X+4,[int]$pixelBounds.Y+4)
 $seedPixels=Wait-StablePixels {Gesture-Pixels}
 if($seedPixels -ceq $blankPixels){throw 'Gesture seed changed history without effective artwork pixels'}
 Open-SimpleLayers
 $p=Pair-OnRow $id;$seedIdle=@((Model).state.commands|Where-Object id -eq 'add_layer')[0].enabled;$workspaceBeforeHold=(Model).windows_workspace|Select-Object ready,busy,saving,owner_lost,page,prompt;Begin-Pair $p
 Wait-Until {(Model).state.layer_tools.has_selection} 'Held two fingers on layer did not load painted contents'
 End-Pair;Record 'two-finger layer hold loads contents'
 (Control "layer-$id-name" -Arranged).SetFocus();[CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),[uint16]0x44);Wait-Until {!(Model).state.layer_tools.has_selection} 'Deselect did not finish after layer hold'
 [CapyCanvasTouch]::Dispose()
 [CapyCanvasTouch]::Initialize([uint32]$review.Id)
 $p=Pair-OnRow $id;$revision=(Model).state.document_file.revision
 Begin-Pair $p;Start-Sleep -Milliseconds 60;End-Pair
 Wait-Until {(Model).state.layer_tools.opacity_popup -eq $id -and $null -ne (Find 'layer-gesture-opacity' -Visible)} 'Two-finger layer tap did not open native opacity'
 if((Model).state.document_file.revision -ne $revision){throw 'Opening opacity changed document history'}
 $opacity=(Model).state.layer_tools.editing_layer.opacity
 $field=Control 'layer-gesture-opacity' -Arranged
 $field.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('42');(Control 'drawing-canvas').SetFocus()
 Wait-Until {[Math]::Abs((Model).state.layer_tools.editing_layer.opacity-.42) -lt .001 -and (Model).state.document_file.revision -gt $revision} 'Gesture opacity edit did not commit'
 [CapyRowPointer]::Key([uint32]$review.Id,0x1b)
 Wait-Until {$null -eq (Model).state.layer_tools.opacity_popup} 'Opacity flyout did not close'
 Undo-To {[Math]::Abs((Model).state.layer_tools.editing_layer.opacity-$opacity) -lt .001} 'Undo did not restore gesture opacity';Record 'two-finger tap opacity edit and Undo'
 $p=Pair-OnRow $id;$selected=(Row $id).selected;$alpha=(Row $id).alpha_locked
 $swipeOpacity=(Row $id).opacity;$swipeHistory=Gesture-History;$swipeCamera=Gesture-Camera
 Begin-Pair $p
 for($i=1;$i -le 8;$i++){[CapyCanvasTouch]::Pair(($p.ax+$i*7),$p.y,($p.bx+$i*7),$p.y);Start-Sleep -Milliseconds 10}
 End-Pair
 if((Row $id).selected -ne $selected -or (Row $id).alpha_locked -ne $alpha -or (Row $id).opacity -ne $swipeOpacity -or (Gesture-History) -cne $swipeHistory -or (Gesture-Camera) -cne $swipeCamera){throw 'Two-finger swipe changed selection, opacity, Alpha Lock, history or camera at the pair boundary'}
 $p=Point "layer-$id-name";[CapyCanvasTouch]::Down(1,$p.x,$p.y)
 for($i=1;$i -le 8;$i++){[CapyCanvasTouch]::Move(1,($p.x+$i*7),$p.y);Start-Sleep -Milliseconds 10};[CapyCanvasTouch]::Up(1)
 Wait-Until {(Row $id).alpha_locked -ne $alpha} 'One-finger right swipe did not toggle Alpha Lock'
 if((Row $id).selected -ne $selected){throw 'Two-finger right swipe changed row selection'}
 if((Row $id).opacity -ne $swipeOpacity -or (Gesture-Camera) -cne $swipeCamera){throw 'Acknowledged Alpha Lock after the pair changed opacity or camera'}
 Record 'two-finger right swipe leaves selection unchanged'
 Undo-To {(Row $id).alpha_locked -eq $alpha} 'Undo did not restore Alpha Lock';Record 'one-finger Alpha Lock remains available'
 if((Gesture-History) -cne $swipeHistory -or (Gesture-Camera) -cne $swipeCamera){throw 'One Alpha Lock Undo did not restore the exact pre-pair history and camera'}
 $all=@((Model).state.layers.id);$upper=Pair-OnRow $id;$lower=Pair-OnRow $base
 $x=$upper.ax;$ay=$upper.y;$by=$lower.y;$distance=$by-$ay
 if([Math]::Abs($distance) -lt 60){throw 'Inclusive merge endpoints are too close'}
 $revision=(Model).state.document_file.revision
 $mergeCamera=Gesture-Camera;$mergePixels=Wait-StablePixels {Gesture-Pixels}
 [CapyCanvasTouch]::PairDown($x,$ay,$x,$by)
 for($i=1;$i -le 10;$i++){[CapyCanvasTouch]::Pair($x,($ay+[int]($distance*.4*$i/10)),$x,($by-[int]($distance*.4*$i/10)));Start-Sleep -Milliseconds 10};End-Pair
 Wait-Until {(Model).state.layers.Count -eq $all.Count-2 -and (Model).state.document_file.revision -gt $revision} 'Layer pinch did not merge the inclusive three-row range'
 $merged=@((Model).state.layers.id)
 if((Wait-StablePixels {Gesture-Pixels}) -cne $mergePixels -or (Gesture-Camera) -cne $mergeCamera){throw 'Inclusive merge changed composed artwork or camera'}
 Undo-To {(@((Model).state.layers.id) -join ',') -eq ($all -join ',')} 'One Undo did not restore inclusive merge range';Record 'inclusive pinch merge and one-step Undo'
 if((Wait-StablePixels {Gesture-Pixels}) -cne $mergePixels){throw 'Merge Undo did not restore exact composed artwork'}
 Invoke 'Redo' -Name
 Wait-Until {(@((Model).state.layers.id) -join ',') -eq ($merged -join ',')} 'One Redo did not restore the exact merged layer IDs'
 if((Wait-StablePixels {Gesture-Pixels}) -cne $mergePixels -or (Gesture-Camera) -cne $mergeCamera){throw 'Merge Redo changed composed artwork or camera'}
 Undo-To {(@((Model).state.layers.id) -join ',') -eq ($all -join ',')} 'Final Undo did not restore inclusive merge range'
 if((Wait-StablePixels {Gesture-Pixels}) -cne $mergePixels){throw 'Final merge Undo did not restore exact composed artwork'}
 Record 'inclusive merge exact pixels and one-step Undo Redo'
 Fit-GestureCanvas;Wait-Until {$m=Model;$tab=@($m.state.tabs|Where-Object active)[0];$c=$m.state.camera;$zoom=.9*[Math]::Min($c.work_area[2]/$tab.width,$c.work_area[3]/$tab.height);[Math]::Abs($c.zoom-$zoom) -lt .001} 'Fit baseline did not acknowledge its camera';$fitted=(Model).state.camera
 [CapyCanvasTouch]::PairDown(($cx-100),$cy,($cx+100),$cy)
 for($i=1;$i -le 20;$i++){$angle=$i*[Math]::PI/40;$dx=[int](100*[Math]::Cos($angle));$dy=[int](100*[Math]::Sin($angle));[CapyCanvasTouch]::Pair(($cx-$dx),($cy-$dy),($cx+$dx),($cy+$dy));Start-Sleep -Milliseconds 12};End-Pair
 Wait-Until {[Math]::Abs((Model).state.camera.rotation-[Math]::PI/2) -lt .02} 'Canvas did not rotate before quick fit'
 $rotated=(Model).state.camera
 [CapyRowPointer]::RightDrag(($cx-40),$cy,($cx+40),($cy+40))
 Wait-Until {[Math]::Abs((Model).state.camera.translation[0]-$rotated.translation[0]) -gt 30} 'Canvas did not pan before quick fit'
 $prior=(Model).state.camera
 $duration=[CapyCanvasTouch]::QuickPinch($cx,$cy,$false)
 Wait-Until {[Math]::Abs((Model).state.camera.translation[0]-$fitted.translation[0]) -lt 1 -and [Math]::Abs((Model).state.camera.zoom-$fitted.zoom) -lt .001 -and [Math]::Abs((Model).state.camera.rotation) -lt .001} 'Quick inward pinch did not fit canvas'
 if([Math]::Abs((Model).state.camera.translation[1]-$fitted.translation[1]) -ge 1 -or ((Model).state.camera.flipped -join ',') -cne ($fitted.flipped -join ',') -or ((Model).state.camera.work_area -join ',') -cne ($fitted.work_area -join ',')){throw 'Quick fit changed the vertical fit, flips or work area'}
 $quickFitted=(Model).state.camera
 $reverseDuration=[CapyCanvasTouch]::QuickPinch($cx,$cy,$true)
 Wait-Until {[Math]::Abs((Model).state.camera.translation[0]-$prior.translation[0]) -lt 1 -and [Math]::Abs((Model).state.camera.zoom-$prior.zoom) -lt .001 -and [Math]::Abs((Model).state.camera.rotation-$prior.rotation) -lt .001} 'Reverse pinch did not restore prior view'
 if([Math]::Abs((Model).state.camera.translation[1]-$prior.translation[1]) -ge 1 -or ((Model).state.camera.flipped -join ',') -cne ($prior.flipped -join ',') -or ((Model).state.camera.work_area -join ',') -cne ($prior.work_area -join ',')){throw 'Reverse pinch changed vertical translation, flips or work area'}
 $checks.Add(@{name='quick fit and reverse restore';inward_injection_ms=$duration;outward_injection_ms=$reverseDuration;before=$prior;fitted=$quickFitted;restored=(Model).state.camera})
 if(!$Simple){
  if((Model).state.settings.quick_pinch_fit -ne $true -or (Model).state.settings.quick_pinch_restore -ne $true){throw 'Full touch profile did not start with both pinch preferences enabled'}
  $pinchHistory=Gesture-History
  Set-PinchPreferences $false $true
  $beforePinch=(Model).state.camera;$null=[CapyCanvasTouch]::QuickPinch($cx,$cy,$false)
  Wait-Until {[Math]::Abs((Model).state.camera.zoom/$beforePinch.zoom-.5) -lt .01} 'Disabled fit did not keep ordinary inward zoom'
  Assert-OrdinaryPinch $beforePinch (Model).state.camera .5
  Set-PinchPreferences $true $false
  $null=[CapyCanvasTouch]::QuickPinch($cx,$cy,$false)
  Wait-Until {[Math]::Abs((Model).state.camera.zoom-$fitted.zoom) -lt .001 -and [Math]::Abs((Model).state.camera.rotation) -lt .001 -and [Math]::Abs((Model).state.camera.translation[0]-$fitted.translation[0]) -lt 1 -and [Math]::Abs((Model).state.camera.translation[1]-$fitted.translation[1]) -lt 1} 'Fit did not remain enabled with Restore disabled'
  if(((Model).state.camera.flipped -join ',') -cne ($fitted.flipped -join ',')){throw 'Preference-controlled fit changed the fitted orientation'}
  $beforePinch=(Model).state.camera;$null=[CapyCanvasTouch]::QuickPinch($cx,$cy,$true)
  Wait-Until {[Math]::Abs((Model).state.camera.zoom/$beforePinch.zoom-2) -lt .01} 'Disabled restore did not keep ordinary outward zoom'
  Assert-OrdinaryPinch $beforePinch (Model).state.camera 2
  Set-PinchPreferences $true $true
  if((Gesture-History) -cne $pinchHistory){throw 'Preference-controlled pinches changed authored history'}
  Record 'independent pinch preferences retain ordinary zoom and enabled fit'
 }
 Capture ('touch-gestures-'+$Theme) -WithModel -Composed
 [CapyCanvasTouch]::Dispose();[CapyRowPointer]::Dispose()
 & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $handle.ToInt64() -Action Close -DiscardUnsaved
 if((Get-Item $stderr).Length){throw 'Gesture runtime stderr requires inspection'}
 @{theme=$Theme;simple=[bool]$Simple;three_finger=[bool]$ThreeFinger;held_cancel_scope=$(if($ThreeFinger){'CancelAll plus successful next native Redo and exact recovery; absence after cancellation acknowledgement unverified'}else{'not run'});system_touch_gestures=(Get-ItemProperty 'HKCU:\Control Panel\Desktop').TouchGestureSetting;checks=$checks;scope='OS-injected WinUI touch; hardware touch and performance unverified'}|ConvertTo-Json -Depth 20|Set-Content (Join-Path $run 'result.json')
}catch{$failed=$true;$failure=$_;$CapyEach=$null;($failure.Exception.ToString()+[Environment]::NewLine+$failure.ScriptStackTrace)|Set-Content (Join-Path $run 'failure.txt');@{model=(Model);fitted=$fitted;prior=$prior;duration=$duration;reverse_duration=$reverseDuration;seed_revision=$revision;seed_paint_revision=$paintRevision;seed_point=@($cx,$cy);seed_idle_before_hold=$seedIdle;workspace_before_hold=$workspaceBeforeHold;seed_thumbnail_status=(Find "layer-$id-thumbnail").Current.ItemStatus;gesture_status=(Find "layer-list").Current.ItemStatus;canvas_has_focus=(Find "drawing-canvas").Current.HasKeyboardFocus;row_has_focus=(Find "layer-$id-name").Current.HasKeyboardFocus;checks=$checks}|ConvertTo-Json -Depth 90|Set-Content (Join-Path $run 'failure-state.json');try{Capture 'failure-screen' -Composed}catch{$_.Exception.ToString()|Set-Content (Join-Path $run 'capture-error.txt')};throw $failure}
finally{[CapyCanvasTouch]::Dispose();[CapyRowPointer]::Dispose();if($review){$review.Refresh();if(!$review.HasExited){$review.Kill();$review.WaitForExit(8000)|Out-Null}};Exit-CapyEnvironment}
