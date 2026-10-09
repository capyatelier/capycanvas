param([Parameter(Mandatory)][string]$Executable,[ValidateSet('dark','light')][string]$Theme='dark')
$ErrorActionPreference='Stop'
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$scripts=$PSScriptRoot
. (Join-Path $scripts 'CapyUia.ps1')
Add-Type -Path (Join-Path $scripts 'RowPointerDriver.cs')
Add-Type -AssemblyName System.Drawing
$CapyFind='prefer-visible';$CapyPopups=$true;$CapyWaitSeconds=30
[CapyRowPointer]::VerifyPrivateDesktop($env:CAPY_PRIVATE_DESKTOP)
if($env:CAPY_WAIT_SCALE){throw 'Readiness checks require default functional deadlines'}
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$run=Join-Path $repo ('artifacts/windows/idle-brush/'+$Theme+'-'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$clock=[Diagnostics.Stopwatch]::StartNew()
$milestones=[Collections.Generic.List[object]]::new()
$checks=[ordered]@{theme=$Theme;executable_sha256=(Get-FileHash -LiteralPath $Executable).Hash}
$review=$null;$projection=$null
function Shader-Lines{
 if(!(Test-Path -LiteralPath $stderr)){return @()}
 $stream=[IO.File]::Open($stderr,[IO.FileMode]::Open,[IO.FileAccess]::Read,([IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete))
 $reader=[IO.StreamReader]::new($stream)
 try{@($reader.ReadToEnd() -split '\r?\n'|Where-Object {$_})}finally{$reader.Dispose()}
}
function Observe([string]$Stage,$Model){
 if(!$Model){$Model=Model}
 $value=[ordered]@{stage=$Stage;elapsed_ms=$clock.Elapsed.TotalMilliseconds;process_id=$review.Id;canvas_ready=$Model.canvas_ready;brush_ready=$Model.brush_ready;shaders_ready=$Model.shaders_ready;brush=$Model.state.brush;shader_log=$stderr;shader_log_bytes=(Get-Item -LiteralPath $stderr).Length}
 $milestones.Add($value)
 $value
}
function Start-Review([string]$Name){
 $script:launch=Join-Path $run $Name;[IO.Directory]::CreateDirectory($launch)|Out-Null
 $env:CAPY_STORAGE_DIR=Join-Path $launch 'profile'
 [IO.File]::WriteAllText((Settings-File),(@{language=@{Explicit='en'};theme=$Theme}|ConvertTo-Json -Depth 4))
 $script:CapyTraceDirectory=$launch;$script:stderr=Join-Path $launch 'shader-jobs.log'
 $script:review=Start-Process -FilePath $Executable -WorkingDirectory $launch -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
 $null=$review.Handle
 Write-Output "Owned idle brush review $($review.Id): $run"
 $start=@{window=$null;first=$null}
 Wait-Until {
  $m=Model
  if(!$m.canvas_ready -or !$m.brush_ready -or !$m.windows_workspace.ready -or $m.windows_workspace.busy){return $false}
  if(!$start.first){$start.first=Observe "$Name-first-ready" $m}
  $start.window=Owned-DrawingWindow $review
  $null -ne $start.window
 } 'The selected brush and native drawing did not become ready' 90
 $script:root=$start.window.Root;$script:drawingWindow=$start.window.Handle
 $script:firstReady=$start.first
 [CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
 if(![CapyRowPointer]::SetForegroundWindow($drawingWindow)){throw 'The owned drawing did not accept foreground input'}
 [CapyRowPointer]::Initialize([uint32]$review.Id)
}
function Close-Review([switch]$Discard){
 [CapyRowPointer]::Dispose()
 & (Join-Path $scripts 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Close -DiscardUnsaved:$Discard -StateDirectory $launch
 $review.Refresh()
 if(!$review.HasExited -or $review.ExitCode -ne 0){throw 'The owned review did not close with exit0'}
 $other=@(Shader-Lines|Where-Object {$_ -notmatch '^pipeline (begin|end) label='})
 if($other.Count){$other|Set-Content -LiteralPath (Join-Path $launch 'unexpected-stderr.txt');throw 'Native stderr contains non-shader output'}
}
function Point([double]$X,[double]$Y,$Snapshot){
 if($Snapshot){$c=$Snapshot.camera_state;$b=$Snapshot.bounds}else{$c=(Model).state.camera;$b=(Control 'drawing-canvas' -Arranged).Current.BoundingRectangle}
 if($c.rotation -ne 0 -or $c.flipped[0] -or $c.flipped[1]){throw 'The fixture requires an unrotated canvas'}
 @([int]($b.X+($X*$c.zoom+$c.translation[0])*$b.Width/$c.viewport[0]),[int]($b.Y+($Y*$c.zoom+$c.translation[1])*$b.Height/$c.viewport[1]))
}
function Park{
 $b=(Control 'layer-new' -Arranged).Current.BoundingRectangle
 [CapyRowPointer]::Hover([int]($b.X+$b.Width/2),[int]($b.Y+$b.Height/2))
}
function Stroke([int[]]$From,[int[]]$To){
 (Control 'drawing-canvas').SetFocus()
 [CapyRowPointer]::Down('mouse',$From[0],$From[1])
 try{for($i=1;$i -le 12;$i++){
  [CapyRowPointer]::Move([int]($From[0]+($To[0]-$From[0])*$i/12),[int]($From[1]+($To[1]-$From[1])*$i/12))
  Start-Sleep -Milliseconds 12
 };[CapyRowPointer]::Up()}finally{[CapyRowPointer]::Cancel()}
 Park
}
function Camera-Key($Camera){@($Camera.viewport,$Camera.translation,$Camera.zoom,$Camera.rotation,$Camera.flipped)|ConvertTo-Json -Depth 5 -Compress}
function Pixels{
 if([CapyRowPointer]::GetForegroundWindow() -ne $drawingWindow){throw 'The owned drawing must be foreground for composed samples'}
 $bounds=(Control 'drawing-canvas').Current.BoundingRectangle
 if($bounds -ne $projection.bounds -or (Camera-Key (Model).state.camera) -ne $projection.camera){throw 'The canvas or camera moved during composed history comparisons'}
 $bitmap=[Drawing.Bitmap]::new([int]$bounds.Width,[int]$bounds.Height);$g=[Drawing.Graphics]::FromImage($bitmap)
 try{
  $g.CopyFromScreen([int]$bounds.X,[int]$bounds.Y,0,0,$bitmap.Size)
  (@(foreach($point in $samplePoints){$color=$bitmap.GetPixel([int]($point[0]-$bounds.X),[int]($point[1]-$bounds.Y));$color.ToArgb()}) -join ',')
 }finally{$g.Dispose();$bitmap.Dispose()}
}
function Revision{@((Model).state.layers|ForEach-Object {"$($_.id):$($_.paint_revision)"}) -join ','}
function Shortcut([uint16[]]$Modifiers,[uint16]$Key){
 (Control 'drawing-canvas').SetFocus();[CapyRowPointer]::Chord([uint32]$review.Id,$Modifiers,$Key)
}
function Select-Preset([string]$Label,[int]$Id){
 Shortcut @(0x11) 0x4b
 Wait-Until {Find 'command-search' -Visible} 'Command search did not open'
 (Control 'command-search').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($Label)
 Wait-Until {$item=Find 'command-result-0' -Visible;$item -and $item.Current.Name.StartsWith($Label)} "Command search did not find $Label"
 [CapyRowPointer]::Key([uint32]$review.Id,0x0d)
 Wait-Until {!(Find 'command-search' -Visible) -and (Model).state.brush.preset -eq $Id -and (Model).brush_ready} "The requested brush did not become ready: $Label/$Id" 90
 $size=Control 'tool-setting-size';$size.SetFocus()
 $size.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('64')
 [CapyRowPointer]::Key([uint32]$review.Id,0x0d)
 (Control 'drawing-canvas').SetFocus()
 Wait-Until {(Model).state.brush.diameter -eq 64} 'The brush diameter did not commit'
 if((Model).state.customization.drawer){
  [CapyRowPointer]::Key([uint32]$review.Id,0x1b)
  Wait-Until {$null -eq (Model).state.customization.drawer} 'The tool drawer did not close'
 }
 Park
}
function Assert-History([uint16]$Key,[string]$Expected,[string]$Paint,[string]$Name){
 Shortcut @(0x11) $Key
 Wait-Until {(Revision) -eq $Paint} "$Name did not restore its paint state"
 Park;Wait-Until {(Pixels) -eq $Expected} "$Name did not restore the exact composed samples"
 if((Wait-StablePixels {Pixels}) -ne $Expected){throw "$Name composed samples did not remain exact"}
}
try{
 Enter-CapyEnvironment @('CAPY_TRACE_SHADER_JOBS')
 $env:CAPY_TRACE_UI='1';$env:CAPY_TRACE_SHADER_JOBS='1'
 Start-Review 'functional'
 $checks.first_ready=$firstReady
 $canvas=Control 'drawing-canvas' -Arranged
 Wait-Until {$canvas.Current.IsEnabled} 'The selected-brush canvas stayed disabled'
 $tab=@((Model).state.tabs|Where-Object active)[0]
 $from=Point ($tab.width*.45) ($tab.height*.45);$to=Point ($tab.width*.55) ($tab.height*.55)
 $before=Revision;$contact=Observe 'starter-contact' $null
 Stroke $from $to
 Wait-Until {(Revision) -ne $before -and ((Model).state.commands|Where-Object id -eq 'undo').enabled} 'The selected brush did not paint its starter stroke'
 $checks.starter_stroke=[ordered]@{pending_at_first_ready=!$firstReady.shaders_ready;pending_at_contact=!$contact.shaders_ready;commit=Observe 'starter-commit' $null}
 Capture 'starter-stroke' -WithModel -Composed
 $root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Maximized)
 $seed=Join-Path $run 'Seeded edges.png'
 $bitmap=[Drawing.Bitmap]::new(320,240)
 try{
  $g=[Drawing.Graphics]::FromImage($bitmap)
  try{
   $g.Clear([Drawing.Color]::White)
   foreach($y in 0..3){foreach($x in 0..4){
    $color=if(($x+$y)%2){[Drawing.Color]::FromArgb(255,220,40,30)}else{[Drawing.Color]::FromArgb(255,30,60,220)}
    $fill=[Drawing.SolidBrush]::new($color)
    try{$g.FillRectangle($fill,$x*64,$y*60,64,60)}finally{$fill.Dispose()}
   }}
  }finally{$g.Dispose()}
  $bitmap.Save($seed,[Drawing.Imaging.ImageFormat]::Png)
 }finally{$bitmap.Dispose()}
 Open-Project $seed
 Wait-Until {$m=Model;$tab=@($m.state.tabs|Where-Object active)[0];$tab.width -eq 320 -and $tab.height -eq 240 -and $m.brush_ready -and !$m.state.document_file.busy -and !@($m.state.requests).Count} 'The seeded paint drawing did not open' 90
 if((Model).state.layer_tools.editing_layer.object){throw 'The seed must be an editable paint layer'}
 Fit-Canvas;Park
 Wait-Until {(Model).shaders_ready} 'Idle preparation did not reach the complete shader catalogue' 180
 $checks.idle_complete=Observe 'idle-catalogue-complete' $null
 $projection=@{bounds=(Control 'drawing-canvas' -Arranged).Current.BoundingRectangle;camera=(Camera-Key (Model).state.camera);camera_state=(Model).state.camera}
 $samplePoints=@(foreach($y in 0..23){foreach($x in 0..31){,(Point (($x+.5)*10) (($y+.5)*10) $projection)}})
 foreach($point in $samplePoints){if(!$projection.bounds.Contains([double]$point[0],[double]$point[1])){throw 'A composed sample lies outside the arranged canvas'}}
 @{projection=$projection;points=$samplePoints}|ConvertTo-Json -Depth 6|Set-Content -LiteralPath (Join-Path $run 'sample-grid.json')
 $cases=[Collections.Generic.List[object]]::new()
 foreach($preset in @(@('Wet Round',11),@('Smudge',10),@('Liquify',12),@('G-Pen',1))){
  $traceBefore=@(Shader-Lines|Where-Object {$_ -match '^pipeline begin '}).Count
  Select-Preset $preset[0] $preset[1]
  $selected=Observe ("selected-"+$preset[1]) $null
  $before=Wait-StablePixels {Pixels};$paintBefore=Revision
  $from=Point 110 110;$to=Point 215 140
  Stroke $from $to
  Wait-Until {(Revision) -ne $paintBefore -and ((Model).state.commands|Where-Object id -eq 'undo').enabled} "$($preset[0]) did not commit a stroke"
  Wait-Until {(Pixels) -ne $before} "$($preset[0]) did not change composed seeded artwork"
  $after=Wait-StablePixels {Pixels};$paintAfter=Revision
  Capture ("preset-"+$preset[1]) -WithModel -Composed
  Assert-History 0x5a $before $paintBefore ($preset[0]+' Undo')
  Assert-History 0x59 $after $paintAfter ($preset[0]+' Redo')
  Assert-History 0x5a $before $paintBefore ($preset[0]+' final Undo')
  $traceAfter=@(Shader-Lines|Where-Object {$_ -match '^pipeline begin '}).Count
  $case=[ordered]@{label=$preset[0];preset=$preset[1];selected=$selected;before_pixels=$before;after_pixels=$after;before_paint=$paintBefore;after_paint=$paintAfter;new_pipeline_begins=$traceAfter-$traceBefore;history='exact'}
  $cases.Add($case)
  $cases|ConvertTo-Json -Depth 8|Set-Content -LiteralPath (Join-Path $run 'brush-cases.json')
 }
 $checks.brush_cases='passed';Close-Review -Discard
 Start-Review 'close-pending'
 $pending=Observe 'before-close' $null
 $checks.close_pending_observation=!$pending.shaders_ready
 Close-Review
 $checks.close_exit=0
 $checks.pending_scope='Readiness and pending observations are runtime evidence; deterministic admission ordering is covered by shared tests. Elapsed values do not qualify speed.'
 $checks.evidence=$run
 $checks|ConvertTo-Json -Depth 10|Tee-Object -FilePath (Join-Path $run 'results.json')
}catch{
 if($review -and !$review.HasExited -and $root){try{Capture 'failure' -WithModel -Composed}catch{}}
 ($_|Out-String)+$_.ScriptStackTrace|Set-Content -LiteralPath (Join-Path $run 'failure.txt')
 throw
}finally{
 [CapyRowPointer]::Dispose()
 $milestones|ConvertTo-Json -Depth 10|Set-Content -LiteralPath (Join-Path $run 'milestones.json')
 Exit-CapyEnvironment
}
