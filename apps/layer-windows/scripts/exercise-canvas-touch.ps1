param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
Add-Type -Path (Join-Path $PSScriptRoot 'CanvasTouchDriver.cs')
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
[CapyCanvasTouch]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path;$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/canvas-touch/'+[Guid]::NewGuid().ToString('N'));[IO.Directory]::CreateDirectory($run)|Out-Null
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
function Wait-Until([scriptblock]$Test,[string]$Message,[int]$Seconds=8){$w=[Diagnostics.Stopwatch]::StartNew();do{[CapyCanvasTouch]::Verify();if(& $Test){return};$app.Refresh();if($app.HasExited){throw 'Owned touch review exited'};Start-Sleep -Milliseconds 50}while($w.Elapsed.TotalSeconds -lt $Seconds);throw $Message}
function Find([string]$Value,[switch]$Name){$property=if($Name){[System.Windows.Automation.AutomationElement]::NameProperty}else{[System.Windows.Automation.AutomationElement]::AutomationIdProperty};$root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new($property,$Value))}
function Control([string]$Value,[switch]$Name){$hit=@{item=$null};Wait-Until {$hit.item=Find $Value -Name:$Name;$null -ne $hit.item} "Missing control: $Value";$hit.item}
function Invoke([string]$Value,[switch]$Name){(Control $Value -Name:$Name).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()}
function Select-Tool([string]$Id){
 if(((Model).state.commands|Where-Object id -eq $Id).selected){return}
 $target=@{id=$null};Wait-Until {foreach($panel in (Model).panels){foreach($tile in $panel.tiles){if($tile.control.kind -eq 'command' -and $tile.control.command -eq $Id){$target.id="tile-$($panel.id)-$($tile.id)";return $true}}};$false} "No native $Id tile"
 Invoke $target.id;Wait-Until {((Model).state.commands|Where-Object id -eq $Id).selected} "Tool did not select: $Id"
}
function Camera {(Model).state.camera}
function Same-Camera($a,$b,[double]$Pixels=.1){
 $a -and $b -and [Math]::Abs($a.zoom-$b.zoom) -lt .0001 -and [Math]::Abs($a.rotation-$b.rotation) -lt .0001 -and
 [Math]::Abs($a.translation[0]-$b.translation[0]) -lt $Pixels -and [Math]::Abs($a.translation[1]-$b.translation[1]) -lt $Pixels
}
# Compare the resulting view over the whole document, allowing one physical
# pixel for OS contact coordinates and accumulated float camera arithmetic.
function Matches-Gesture($actual,$expected){
 if(!$actual -or !$expected){return $false}
 $extent=(Model).document_options.extent
 if(!$extent){return $false}
 foreach($x in @(0,$extent[0])){foreach($y in @(0,$extent[1])){
  $ax=$actual.translation[0]+$actual.zoom*([Math]::Cos($actual.rotation)*$x-[Math]::Sin($actual.rotation)*$y)
  $ay=$actual.translation[1]+$actual.zoom*([Math]::Sin($actual.rotation)*$x+[Math]::Cos($actual.rotation)*$y)
  $ex=$expected.translation[0]+$expected.zoom*([Math]::Cos($expected.rotation)*$x-[Math]::Sin($expected.rotation)*$y)
  $ey=$expected.translation[1]+$expected.zoom*([Math]::Sin($expected.rotation)*$x+[Math]::Cos($expected.rotation)*$y)
  if([Math]::Sqrt(($ax-$ex)*($ax-$ex)+($ay-$ey)*($ay-$ey)) -ge 1){return $false}
 }}
 $true
}
function Signature {$m=$null;for($i=0;$i -lt 40 -and !$m;$i++){$m=Model;if(!$m){Start-Sleep -Milliseconds 25}};@($m.state.document_file,@($m.state.layers|Select-Object id,paint_revision,mask_revision))|ConvertTo-Json -Depth 30 -Compress}
function Capture([string]$Name){& (Join-Path $PSScriptRoot 'inspect-window.ps1') -ProcessId $app.Id -Output (Join-Path $run ($Name+'.png')) -ClientOnly *> (Join-Path $run ($Name+'-capture.json'))}
$checks=[Collections.Generic.List[object]]::new()
function Check([string]$Name,[scriptblock]$Condition){Wait-Until $Condition $Name;if((Signature) -ne $drawing){throw "Touch changed the drawing: $Name"};$checks.Add(@{name=$Name;camera=Camera});Write-Output "$Name passed"}
function Stable([string]$Name,$Expected){Start-Sleep -Milliseconds 160;Check $Name {(Same-Camera (Camera) $Expected)}}
try{
 foreach($name in $names){Remove-Item -LiteralPath ('Env:'+$name) -ErrorAction SilentlyContinue}
 $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1';$env:CAPY_SMOKE_TEST='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
 $app=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError (Join-Path $run 'stderr.log') -RedirectStandardOutput (Join-Path $run 'stdout.log');$null=$app.Handle
 @{process_id=$app.Id;executable=$Executable;sha256=(Get-FileHash -LiteralPath $Executable).Hash}|ConvertTo-Json|Set-Content -LiteralPath (Join-Path $run 'owner.json')
 Write-Output "Owned touch review $($app.Id): $run"
 Wait-Until {$app.Refresh();$app.MainWindowHandle -ne [IntPtr]::Zero -and (Model).brush_ready -and (Model).windows_filter_load.phase -eq 'ready'} 'Isolated canvas did not start' 45
 $handle=$app.MainWindowHandle;$root=[System.Windows.Automation.AutomationElement]::FromHandle($handle)
 $root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Maximized)
 Wait-Until {$c=Camera;$b=(Control 'Drawing canvas' -Name).Current.BoundingRectangle;[Math]::Abs($c.viewport[0]-$b.Width) -lt .1 -and $b.Width -gt 1600} 'Maximized canvas did not settle'
 if(@((Model).layout.groups|Where-Object active -eq 'layers').Count){Invoke 'column-icon-layers';Wait-Until {@((Model).layout.groups|Where-Object active -eq 'layers').Count -eq 0} 'Column did not close for gesture space'}
 Invoke 'canvas-fit';Select-Tool 'pen';Start-Sleep -Milliseconds 200
 Wait-Until {(Model).canvas_ready -and !(Model).state.document_file.busy} 'Canvas did not settle before input'
 $area=(Camera).work_area
 $bounds=(Control 'Drawing canvas' -Name).Current.BoundingRectangle
 $cx=[int]($bounds.X+$area[0]+$area[2]/2);$cy=[int]($bounds.Y+$area[1]+$area[3]/2)
 if($area[2] -lt 600 -or $area[3] -lt 500){throw 'Available canvas is too small for multi-touch acceptance'}
 [CapyCanvasTouch]::SetForegroundWindow($handle)|Out-Null
 [CapyRowPointer]::Initialize([uint32]$app.Id)
 [CapyRowPointer]::Down('mouse',($cx-60),($cy-50))
 try{for($i=1;$i -le 24;$i++){[CapyRowPointer]::Move(($cx-60+$i*5),($cy-50+[int](10*[Math]::Sin($i/4))));Start-Sleep -Milliseconds 8}}finally{[CapyRowPointer]::Up()}
 Wait-Until {(Model).state.document_file.modified} 'Visible mouse seed stroke did not finish'
 [CapyRowPointer]::Verify();[CapyRowPointer]::Dispose()
 $drawing=Signature;Capture 'before-touch'
 [CapyCanvasTouch]::Initialize([uint32]$app.Id)
 $before=Camera;[CapyCanvasTouch]::Down(1,($cx-100),$cy)
 for($i=1;$i -le 10;$i++){[CapyCanvasTouch]::Move(1,($cx-100+$i*4),($cy+$i*3));Start-Sleep -Milliseconds 12}
 Stable 'single finger neither paints nor navigates with Pen' $before
 [CapyCanvasTouch]::Up(1);Stable 'single-finger release does not jump' $before
 [CapyCanvasTouch]::Down(1,($cx-100),$cy);[CapyCanvasTouch]::Down(2,($cx+100),$cy)
 Stable 'adding second finger does not jump' $before
 for($i=1;$i -le 10;$i++){[CapyCanvasTouch]::Pair(($cx-100+$i*6),($cy+$i*3),($cx+100+$i*6),($cy+$i*3));Start-Sleep -Milliseconds 12}
 $expected=$before|ConvertTo-Json -Depth 8|ConvertFrom-Json;$expected.translation[0]+=60;$expected.translation[1]+=30
 Check 'two-finger pan preserves scale and rotation' {(Matches-Gesture (Camera) $expected)}
 $pan=Camera;$ax=$cx+60;$ay=$cy+30
 for($i=1;$i -le 10;$i++){[CapyCanvasTouch]::Pair(($ax-100-$i*5),$ay,($ax+100+$i*5),$ay);Start-Sleep -Milliseconds 12}
 $anchor=@(($ax-$bounds.X),($ay-$bounds.Y))
 $expected=$pan|ConvertTo-Json -Depth 8|ConvertFrom-Json;$expected.zoom*=1.5
 for($axis=0;$axis -lt 2;$axis++){$expected.translation[$axis]=$anchor[$axis]+1.5*($pan.translation[$axis]-$anchor[$axis])}
 Check 'pinch scales by 1.5 around its midpoint' {(Matches-Gesture (Camera) $expected)}
 $zoomed=Camera
 for($i=1;$i -le 20;$i++){$angle=$i*[Math]::PI/40;$dx=[int](150*[Math]::Cos($angle));$dy=[int](150*[Math]::Sin($angle));[CapyCanvasTouch]::Pair(($ax-$dx),($ay-$dy),($ax+$dx),($ay+$dy));Start-Sleep -Milliseconds 12}
 $expected=$zoomed|ConvertTo-Json -Depth 8|ConvertFrom-Json;$expected.rotation+=[Math]::PI/2
 $expected.translation[0]=$anchor[0]-($zoomed.translation[1]-$anchor[1]);$expected.translation[1]=$anchor[1]+($zoomed.translation[0]-$anchor[0])
 Check 'two-finger quarter-turn preserves midpoint and scale' {(Matches-Gesture (Camera) $expected)}
 Capture 'rotated-touch';$turned=Camera
 [CapyCanvasTouch]::Down(3,($ax+90),$ay);Stable 'third finger rebases without a jump' $turned
 [CapyCanvasTouch]::Move(3,($ax+110),($ay+20));Stable 'three fingers suspend two-finger navigation' $turned
 [CapyCanvasTouch]::Up(2);Stable 'removing an original finger rebases the replacement pair' $turned
 [CapyCanvasTouch]::Move(3,($ax+135),($ay+35));Check 'remaining two fingers resume navigation' {!(Same-Camera (Camera) $turned)}
 [CapyCanvasTouch]::Up(3);$one=Camera;[CapyCanvasTouch]::Move(1,($ax+20),($ay-140));Stable 'remaining single finger does not pan with Pen' $one
 [CapyCanvasTouch]::Up(1);Stable 'all contacts released without a jump' $one
 [CapyCanvasTouch]::Down(1,($cx-100),$cy);[CapyCanvasTouch]::Down(2,($cx+100),$cy);$cancelled=Camera
 [CapyCanvasTouch]::CancelAll();Stable 'cancelled touch contacts preserve the camera' $cancelled
 [CapyCanvasTouch]::Down(1,($cx-100),$cy);[CapyCanvasTouch]::Down(2,($cx+100),$cy)
 Stable 'fresh contact pair after cancellation does not jump' $cancelled
 [CapyCanvasTouch]::Pair(($cx-70),($cy+20),($cx+130),($cy+20))
 $expected=$cancelled|ConvertTo-Json -Depth 8|ConvertFrom-Json;$expected.translation[0]+=30;$expected.translation[1]+=20
 Check 'new contacts pan normally after cancellation' {(Matches-Gesture (Camera) $expected)}
 [CapyCanvasTouch]::Up(2);[CapyCanvasTouch]::Up(1)
 Select-Tool 'hand';$hand=Camera;[CapyCanvasTouch]::Down(1,$cx,$cy);[CapyCanvasTouch]::Move(1,($cx+40),($cy+25))
 $expected=$hand|ConvertTo-Json -Depth 8|ConvertFrom-Json;$expected.translation[0]+=40;$expected.translation[1]+=25
 Check 'Hand permits one-finger pan' {(Matches-Gesture (Camera) $expected)}
 [CapyCanvasTouch]::Up(1);[CapyCanvasTouch]::Dispose()
 Stable 'Hand release preserves the drawing and camera' (Camera)
 Select-Tool 'pen';Invoke 'canvas-fit';Start-Sleep -Milliseconds 200
 Check 'returning to Pen and Fit preserves the drawing' {$true}
 [CapyCanvasTouch]::Initialize([uint32]$app.Id)
 $resting=Camera;[CapyCanvasTouch]::Down(1,($cx-100),$cy);Start-Sleep -Milliseconds 120
 Invoke 'settings-button'
 Wait-Until {$null -ne (Find 'Preferences' -Name)} 'Preferences did not open over a resting finger'
 [CapyCanvasTouch]::Up(1);Start-Sleep -Milliseconds 150
 Invoke 'CloseButton'
 Wait-Until {$null -eq (Find 'Preferences' -Name)} 'Preferences did not close'
 [CapyCanvasTouch]::SetForegroundWindow($handle)|Out-Null;Start-Sleep -Milliseconds 150
 [CapyCanvasTouch]::Down(1,($cx+40),$cy)
 for($i=1;$i -le 10;$i++){[CapyCanvasTouch]::Move(1,($cx+40+$i*6),($cy+$i*4));Start-Sleep -Milliseconds 12}
 Stable 'a finger lifted under a modal dialog does not pair with the next finger' $resting
 [CapyCanvasTouch]::Up(1);[CapyCanvasTouch]::Dispose()
 [CapyRowPointer]::Initialize([uint32]$app.Id)
 $revision=(Model).state.document_file.revision
 [CapyRowPointer]::Down('pen',($cx-70),($cy+60))
 try{for($i=1;$i -le 28;$i++){[CapyRowPointer]::Move(($cx-70+$i*5),($cy+60+[int](12*[Math]::Sin($i/4))));Start-Sleep -Milliseconds 8}}finally{[CapyRowPointer]::Up()}
 Start-Sleep -Milliseconds 300
 Wait-Until {(Model).state.document_file.revision -gt $revision} 'Pen stroke after touch did not finish'
 @{before_pen_revision=$revision;after_pen=(Model).state.document_file}|ConvertTo-Json -Compress|Write-Output
 Capture 'pen-after-touch';[CapyRowPointer]::Verify();[CapyRowPointer]::Dispose()
 $revision=(Model).state.document_file.revision;Invoke 'Undo' -Name
 Wait-Until {(Model).state.document_file.revision -gt $revision -and (Model).state.document_file.modified} 'One Undo did not leave the seed drawing'
 $revision=(Model).state.document_file.revision;Invoke 'Undo' -Name
 Wait-Until {(Model).state.document_file.revision -gt $revision -and !(Model).state.document_file.modified} 'Two independent Undo steps did not remove pen and seed strokes'
 $app.CloseMainWindow()|Out-Null
 if(!$app.WaitForExit(5000) -or $app.ExitCode -ne 0){throw 'Touch review did not close successfully within five seconds'}
 if((Get-Item -LiteralPath (Join-Path $run 'stderr.log')).Length){throw 'Touch runtime stderr requires inspection'}
 @{checks=$checks;drawing='preserved throughout navigation';pen_after_touch='new stroke and independent Undo';close='zero exit within five seconds';scope='OS-injected multi-touch and pen; physical digitizers, pressure/tilt and 120 Hz acceptance remain separate'}|ConvertTo-Json -Depth 12|Set-Content -LiteralPath (Join-Path $run 'result.json')
 Write-Output "Canvas touch acceptance passed: $run"
}catch{@{camera=Camera;last_expected=$expected;checks=$checks;drawing=$drawing;signature=(Signature)}|ConvertTo-Json -Depth 12|Set-Content -LiteralPath (Join-Path $run 'failure-state.json');if($app -and !$app.HasExited -and $root){try{Capture 'failure'}catch{}};throw}
finally{[CapyCanvasTouch]::Dispose();[CapyRowPointer]::Dispose();foreach($name in $names){if($null -eq $previous[$name]){Remove-Item -LiteralPath ('Env:'+$name) -ErrorAction SilentlyContinue}else{[Environment]::SetEnvironmentVariable($name,$previous[$name],'Process')}}}
