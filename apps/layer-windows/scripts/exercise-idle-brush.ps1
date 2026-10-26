param([Parameter(Mandatory)][string]$Executable,[ValidateSet('dark','light')][string]$Theme='dark',[int[]]$PresetIds=@(),[switch]$LiquifyOnly)
$ErrorActionPreference='Stop'
if($LiquifyOnly -and $PresetIds.Count){throw 'LiquifyOnly cannot be combined with PresetIds'}
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
$review=$null;$projection=$null;$idleCanvas=$null;$idleCanvasOwner=$null
function Canvas-Peer($View){
 if(!$View){$View=Model}
 $owner="$($review.Id)/$drawingWindow/$($View.state.document_file.epoch)/$($View.windows_workspace.id)"
 if(!$script:idleCanvas -or $script:idleCanvasOwner -cne $owner){
  $script:idleCanvas=Control 'drawing-canvas' -Arranged -NoPopups
  if($idleCanvas.Current.ProcessId -ne $review.Id){throw 'The canvas peer does not belong to the owned review'}
  $script:idleCanvasOwner=$owner
 }
 $script:idleCanvas
}
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
 $script:idleCanvas=$null;$script:idleCanvasOwner=$null
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
 if((Model).windows_workspace.id -ne 'builtin:workspace:illustrator'){
  $switch=Find 'workspace-switch-builtin:workspace:illustrator'
  if(!$switch){Invoke 'header-workspace-menu';$switch=Control 'Paint' -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)}
  $switch.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
  Wait-Until {(Model).windows_workspace.id -eq 'builtin:workspace:illustrator' -and !(Model).windows_workspace.busy} 'Paint workspace did not open' 20
 }
 [CapyRowPointer]::Initialize([uint32]$review.Id)
 $null=Canvas-Peer
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
 (Canvas-Peer).SetFocus()
 [CapyRowPointer]::Down('mouse',$From[0],$From[1])
 try{for($i=1;$i -le 12;$i++){
  [CapyRowPointer]::Move([int]($From[0]+($To[0]-$From[0])*$i/12),[int]($From[1]+($To[1]-$From[1])*$i/12))
  Start-Sleep -Milliseconds 12
 };[CapyRowPointer]::Up()}finally{[CapyRowPointer]::Cancel()}
 Park
}
function Camera-Key($Camera){@($Camera.viewport,$Camera.translation,$Camera.zoom,$Camera.rotation,$Camera.flipped)|ConvertTo-Json -Depth 5 -Compress}
function Document-Crop($Bounds,$Camera,[int]$Width,[int]$Height){
 if($Camera.rotation -ne 0 -or $Camera.flipped[0] -or $Camera.flipped[1]){throw 'The document crop requires an unrotated canvas'}
 $sx=$Bounds.Width/$Camera.viewport[0];$sy=$Bounds.Height/$Camera.viewport[1]
 $left=[Math]::Ceiling($Bounds.X+$Camera.translation[0]*$sx)
 $top=[Math]::Ceiling($Bounds.Y+$Camera.translation[1]*$sy)
 $right=[Math]::Floor($Bounds.X+($Width*$Camera.zoom+$Camera.translation[0])*$sx)
 $bottom=[Math]::Floor($Bounds.Y+($Height*$Camera.zoom+$Camera.translation[1])*$sy)
 $work=[Drawing.RectangleF]::new([single]($Bounds.X+$Camera.work_area[0]*$sx),[single]($Bounds.Y+$Camera.work_area[1]*$sy),[single]($Camera.work_area[2]*$sx),[single]($Camera.work_area[3]*$sy))
 $screen=[Drawing.Rectangle]::new([int]$left,[int]$top,[int]($right-$left),[int]($bottom-$top))
 $area=[Drawing.RectangleF]::new([single]$screen.X,[single]$screen.Y,[single]$screen.Width,[single]$screen.Height)
 if($screen.Width -le 0 -or $screen.Height -le 0 -or !$work.Contains($area) -or !$Bounds.Contains($left,$top) -or !$Bounds.Contains($right-1,$bottom-1)){throw 'The whole document crop must fit the unobstructed camera work area'}
 [Drawing.Rectangle]::new([int]($screen.X-$Bounds.X),[int]($screen.Y-$Bounds.Y),$screen.Width,$screen.Height)
}
function Document-Hash([Drawing.Bitmap]$Bitmap,[Drawing.Rectangle]$Crop){
 $bits=$Bitmap.LockBits($Crop,[Drawing.Imaging.ImageLockMode]::ReadOnly,[Drawing.Imaging.PixelFormat]::Format32bppArgb)
 try{
  $hash=[Security.Cryptography.SHA256]::Create();$row=[byte[]]::new($Crop.Width*4)
  try{
   for($y=0;$y -lt $Crop.Height;$y++){
    [Runtime.InteropServices.Marshal]::Copy([IntPtr]::Add($bits.Scan0,$y*$bits.Stride),$row,0,$row.Length)
    $null=$hash.TransformBlock($row,0,$row.Length,$row,0)
   }
   $null=$hash.TransformFinalBlock([byte[]]::new(0),0,0)
   [BitConverter]::ToString($hash.Hash).Replace('-','')
  }finally{$hash.Dispose()}
 }finally{$Bitmap.UnlockBits($bits)}
}

function Assert-DocumentSurface{
 $cache=[System.Windows.Automation.CacheRequest]::new();$cache.AutomationElementMode=[System.Windows.Automation.AutomationElementMode]::None
 $cache.TreeScope=[System.Windows.Automation.TreeScope]::Element
 foreach($property in @('BoundingRectangle','IsOffscreen','ProcessId','Name','ControlType')){$cache.Add([System.Windows.Automation.AutomationElement]::($property+'Property'))}
 $types=@(foreach($type in @([System.Windows.Automation.ControlType]::Menu,[System.Windows.Automation.ControlType]::ToolTip,[System.Windows.Automation.ControlType]::Window)){[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,$type)})
 $filter=[System.Windows.Automation.OrCondition]::new([System.Windows.Automation.Condition[]]$types)
 $owned=[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty,$review.Id)
 $surface=[Windows.Rect]::new($projection.bounds.X+$projection.document_crop.X,$projection.bounds.Y+$projection.document_crop.Y,$projection.document_crop.Width,$projection.document_crop.Height)
 $roots=@($root)+@([System.Windows.Automation.AutomationElement]::RootElement.FindAll([System.Windows.Automation.TreeScope]::Children,$owned)|Where-Object {$_ -ne $root})
 $scope=$cache.Activate()
 try{
  foreach($owner in $roots){$tree=if($owner -eq $root){[System.Windows.Automation.TreeScope]::Descendants}else{[System.Windows.Automation.TreeScope]::Subtree};foreach($element in $owner.FindAll($tree,$filter)){
   $native=$element.Cached
   if($native.ProcessId -eq $review.Id -and !$native.IsOffscreen -and $surface.IntersectsWith($native.BoundingRectangle)){throw 'Native menu, tooltip or dialog obscures the composed document crop'}
  }}
 }finally{$scope.Dispose()}
}
function Save-Pixels([string]$Name,[string]$Expected){
 Assert-DocumentSurface
 $actual=Pixels ($Name -replace '[^A-Za-z0-9-]','-') $Expected
 Assert-DocumentSurface
 if($actual -cne $Expected){throw 'The saved document crop differs from its accepted composed state'}
}
function Pixels([string]$EvidenceName,[string]$Expected){
 if([CapyRowPointer]::GetForegroundWindow() -ne $drawingWindow){throw 'The owned drawing must be foreground for composed samples'}
 $model=Model;$current=(Canvas-Peer $model).Current;$bounds=$current.BoundingRectangle
 if(!$current.IsEnabled -or $current.IsOffscreen){throw 'The owned canvas must accept unobstructed input for composed samples'}
 if($bounds -ne $projection.bounds -or (Camera-Key $model.state.camera) -ne $projection.camera -or ($model.state.camera.work_area -join ',') -cne $projection.work_area){throw 'The canvas or camera moved during composed history comparisons'}
 if($model.state.layer_tools.has_selection -or $model.state.layer_tools.quick_mask -or $model.state.settings_open -or @($model.state.requests).Count -or $model.state.customization.drawer -or $model.state.customization.control -or $model.state.command_search){throw 'A document overlay or pending native UI can obscure composed samples'}
 $cursor=[CapyRowPointer]::Cursor();$surface=[Windows.Rect]::new($bounds.X+$projection.document_crop.X,$bounds.Y+$projection.document_crop.Y,$projection.document_crop.Width,$projection.document_crop.Height)
 if($surface.Contains([double]$cursor.position.x,[double]$cursor.position.y)){throw 'The pointer must be outside the composed document crop'}
 $bitmap=[Drawing.Bitmap]::new([int]$bounds.Width,[int]$bounds.Height);$g=[Drawing.Graphics]::FromImage($bitmap)
 try{
  $g.CopyFromScreen([int]$bounds.X,[int]$bounds.Y,0,0,$bitmap.Size)
  $grid=(@(foreach($point in $samplePoints){$color=$bitmap.GetPixel([int]($point[0]-$bounds.X),[int]($point[1]-$bounds.Y));$color.ToArgb()}) -join ',')
  $dense=Document-Hash $bitmap $projection.document_crop
  $signature=$grid+';document_argb_sha256='+$dense
  if($EvidenceName){
   if($signature -cne $Expected){throw 'The evidence bitmap differs from the accepted composed signature'}
   $path=Join-Path $run ($EvidenceName+'-document.png');$crop=$bitmap.Clone($projection.document_crop,[Drawing.Imaging.PixelFormat]::Format32bppArgb)
   try{$crop.Save($path,[Drawing.Imaging.ImageFormat]::Png)}finally{$crop.Dispose()}
   $metadata=[ordered]@{name=$EvidenceName;image=$path;hwnd=$drawingWindow.ToInt64();process=$review.Id;bounds=@($bounds.X,$bounds.Y,$bounds.Width,$bounds.Height);camera=$model.state.camera;document=$model.state.document_file;crop=@($projection.document_crop.X,$projection.document_crop.Y,$projection.document_crop.Width,$projection.document_crop.Height);pixel_format='Format32bppArgb; top-to-bottom rows; width*4 bytes per row, no stride padding';grid=$grid;document_argb_sha256=$dense;signature=$signature}
   [IO.File]::WriteAllText((Join-Path $run ($EvidenceName+'-document.json')),($metadata|ConvertTo-Json -Depth 8),[Text.UTF8Encoding]::new($false))
  }
  $signature
 }finally{$g.Dispose();$bitmap.Dispose()}
}
function Revision{@((Model).state.layers|ForEach-Object {"$($_.id):$($_.paint_revision)"}) -join ','}
function Shortcut([uint16[]]$Modifiers,[uint16]$Key){
 (Canvas-Peer).SetFocus();[CapyRowPointer]::Chord([uint32]$review.Id,$Modifiers,$Key)
}
function Select-Preset([string]$Label,[int]$Id){
 Shortcut @(0x11) 0x4b
 Wait-Until {Find 'command-search' -Visible} 'Command search did not open'
 (Control 'command-search').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($Label)
 $match=@{index=-1;label=$null}
 Wait-Until {
  $search=Search-State
  if(!$search -or $search.query -ne $Label){return $false}
  for($i=0;$i -lt @($search.results).Count;$i++){
   $result=$search.results[$i]
   if($result.id -notlike 'action:*'){continue}
   $action=$result.id.Substring(7)|ConvertFrom-Json
   if($action.type -eq 'select_brush' -and $action.id -eq $Id){$match.index=$i;$match.label=$result.label;return $true}
  }
  $false
 } "Command search did not find $Label/$Id"
 $result=Control ('command-result-'+$match.index) -Arranged
 if($result.Current.Name -ne $match.label -or !$result.Current.IsEnabled){throw 'The native brush result differs from the shared search result'}
 $result.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
 Wait-Until {!(Find 'command-search' -Visible) -and (Model).state.brush.preset -eq $Id -and (Model).brush_ready} "The requested brush did not become ready: $Label/$Id" 90
 $size=Control 'tool-setting-size';$size.SetFocus()
 $size.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('64')
 [CapyRowPointer]::Key([uint32]$review.Id,0x0d)
 (Canvas-Peer).SetFocus()
 Wait-Until {(Model).state.brush.diameter -eq 64} 'The brush diameter did not commit'
 if((Model).state.customization.drawer){
  [CapyRowPointer]::Key([uint32]$review.Id,0x1b)
  Wait-Until {$null -eq (Model).state.customization.drawer} 'The tool drawer did not close'
 }
 Park
}
function Blender-Presets{
 Select-Preset 'Smudge' 10
 $null=Check-BrushProjection ([pscustomobject]@{label='Smudge';id=10;group='blend'})
 $expected=[ordered]@{blend=@(10,45,46,65,66,48,61,62);wet_paint=@(24,57,58,56,47);effects=@(50,53,49,60,52,51,55)}
 $groups=@((Model).state.tool_set.groups|Where-Object {$_.action.type -eq 'choose_tool_variant' -and $_.action.variant.type -eq 'brush_group'})
 if(($groups.action.variant.group -join ',') -ne ($expected.Keys -join ',')){throw 'Blend did not expose its three shared sets'}
 foreach($group in $expected.Keys){
  $items=@((Model).state.tool_set.groups)
  $index=0;while($index -lt $items.Count -and !($items[$index].action.type -eq 'choose_tool_variant' -and $items[$index].action.variant.type -eq 'brush_group' -and $items[$index].action.variant.group -eq $group)){$index++}
  if($index -eq $items.Count){throw "The shared $group set is missing"}
  $native=Control ('tool-group-'+$index)
  if($native.Current.Name -ne $items[$index].label -or !$native.Current.IsEnabled){throw "The native $group set differs from its shared choice"}
  $native.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
  Wait-Until {$m=Model;$m.brush_ready -and @($m.state.tool_set.groups|Where-Object {$_.selected -and $_.action.type -eq 'choose_tool_variant' -and $_.action.variant.type -eq 'brush_group' -and $_.action.variant.group -eq $group}).Count -eq 1} "The native $group set did not activate"
  $choices=@((Model).state.tool_set.subtools|Where-Object {$_.action.type -eq 'select_brush'})
  if(($choices.action.id -join ',') -ne ($expected[$group] -join ',')){throw "The $group set has missing or obsolete Blender presets"}
  foreach($choice in $choices){
   if(!$choice.enabled -or $choice.preview -ne $choice.action.id){throw "The shared Blender choice is unavailable or lacks its preview: $($choice.label)"}
   [pscustomobject]@{label=$choice.label;id=[int]$choice.action.id;group=$group}
  }
 }
}
function Check-BrushProjection($Preset){
 $check=@{view=$null}
 Wait-Until {
  $state=(Model).state
  $ids=@(foreach($set in @(@('groups','tool-group-'),@('subtools','tool-subtool-'))){$items=@($state.tool_set.($set[0]));foreach($i in 0..$items.Count){$set[1]+$i}};foreach($setting in $state.tool_settings){'tool-setting-'+$setting.id})
  $snapshot=@(Read-ControlSnapshot $ids)
  if($state.brush.preset -ne $Preset.id){return $false}
  foreach($set in @(@('groups','tool-group-'),@('subtools','tool-subtool-'))){
   $items=@($state.tool_set.($set[0]))
   for($i=0;$i -lt $items.Count;$i++){
    $item=$items[$i];$native=Snapshot-Control $snapshot ($set[1]+$i)
    if(!$native -or $native.Name -ne $item.label -or $native.IsEnabled -ne $item.enabled -or (($native.ItemStatus -eq 'Selected') -ne $item.selected)){return $false}
   }
   if(Snapshot-Control $snapshot ($set[1]+$items.Count)){return $false}
  }
  $groups=@($state.tool_set.groups|Where-Object selected)
  if($groups.Count -ne [int](@($state.tool_set.groups).Count -gt 0)){return $false}
  if($groups.Count -and $groups[0].action.type -eq 'choose_tool_variant' -and $groups[0].action.variant.type -eq 'command' -and $groups[0].action.variant.command -ne $state.brush.tool){return $false}
  $items=@($state.tool_set.groups)+@($state.tool_set.subtools)
  $categories=@($items|Where-Object {$_.selected -and $_.action.type -in @('select_tool_group','select_brush_set')})
  $categoryChoices=@($items|Where-Object {$_.action.type -in @('select_tool_group','select_brush_set')})
  if($categories.Count -ne [int]($categoryChoices.Count -gt 0) -or ($categories.Count -and $Preset.group -and $categories[0].action.group -ne $Preset.group)){return $false}
  $selected=@($items|Where-Object {$_.selected -and $_.action.type -eq 'select_brush'})
  if($selected.Count -ne 1 -or $selected[0].action.id -ne $Preset.id){return $false}
  foreach($setting in $state.tool_settings){
   $native=Snapshot-Control $snapshot ('tool-setting-'+$setting.id) -Type ([System.Windows.Automation.ControlType]::Edit)
   if(!$native -or !$native.IsEnabled -or !$native.Name.Contains($setting.label)){return $false}
  }
  $check.view=@{group=$(if($groups.Count){$groups[0]});category=$(if($categories.Count){$categories[0]});subtool=$selected[0];settings=$state.tool_settings}
  $true
 } "Native categories, preset selection or settings differ for $($Preset.label)/$($Preset.id)"
 $check.view
}
function Assert-History([uint16]$Key,[string]$Expected,[string]$Paint,[string]$Name){
 Shortcut @(0x11) $Key
 try{Wait-Until {(Revision) -eq $Paint} "$Name did not restore its paint state"}catch{
  @{name=$Name;expected_revision=$Paint;actual_revision=(Revision);expected_pixels=$Expected;actual_pixels=(Pixels)}|ConvertTo-Json -Depth 5|Set-Content -LiteralPath (Join-Path $run 'history-failure.json')
  throw
 }
 Park;Wait-Until {(Pixels) -eq $Expected} "$Name did not restore the exact composed samples"
 if((Wait-StablePixels {Pixels}) -ne $Expected){throw "$Name composed samples did not remain exact"}
 Save-Pixels $Name $Expected
}
function Assert-History-Pixels([uint16]$Key,[string]$Expected,[string]$Name){
 Shortcut @(0x11) $Key
 Park;Wait-Until {(Pixels) -eq $Expected} "$Name did not restore the exact composed samples"
 if((Wait-StablePixels {Pixels}) -ne $Expected){throw "$Name composed samples did not remain exact"}
 Save-Pixels $Name $Expected
}
try{
 Enter-CapyEnvironment @('CAPY_TRACE_SHADER_JOBS')
 $env:CAPY_TRACE_UI='1';$env:CAPY_TRACE_SHADER_JOBS='1'
 Start-Review 'functional'
 $checks.first_ready=$firstReady
 $canvas=Canvas-Peer
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
 if(!@((Model).layout.groups|Where-Object active -eq 'brushes').Count){
  Invoke $(if(Find 'column-icon-brushes' -Visible){'column-icon-brushes'}elseif(Find 'drawer-tab-brushes' -Visible){'drawer-tab-brushes'}else{'panel-tab-brushes'})
 }
 Wait-Until {@((Model).layout.groups|Where-Object active -eq 'brushes').Count -eq 1} 'The native Tool Set did not open'
 $blenders=if($LiquifyOnly){@()}else{@(Blender-Presets)}
 Fit-Canvas;Park
 Wait-Until {(Model).shaders_ready} 'Requested shader preparation did not complete' 180
 $checks.requested_preparation_complete=Observe 'requested-preparation-complete' $null
 $projection=@{bounds=(Control 'drawing-canvas' -Arranged).Current.BoundingRectangle;camera=(Camera-Key (Model).state.camera);camera_state=(Model).state.camera}
 $projection.work_area=$projection.camera_state.work_area -join ','
 $projection.document_crop=Document-Crop $projection.bounds $projection.camera_state 320 240
 $samplePoints=@(foreach($y in 0..23){foreach($x in 0..31){,(Point (($x+.5)*10) (($y+.5)*10) $projection)}})
 foreach($point in $samplePoints){if(!$projection.bounds.Contains([double]$point[0],[double]$point[1])){throw 'A composed sample lies outside the arranged canvas'}}
 @{projection=$projection;points=$samplePoints}|ConvertTo-Json -Depth 6|Set-Content -LiteralPath (Join-Path $run 'sample-grid.json')
 $seedBaseline=Wait-StablePixels {Pixels};Save-Pixels 'seed-baseline' $seedBaseline
 $cases=[Collections.Generic.List[object]]::new()
 $presets=@(@('Wet Round',11),@('Smudge',10),@('Liquify Push',12),@('G-Pen',1),@('Pencil',2),@('Charcoal',27),@('Pastel Block',17),@('Mixing Brush',59))|ForEach-Object {[pscustomobject]@{label=$_[0];id=$_[1];group=$(if($_[1] -eq 10){'blend'}elseif($_[1] -eq 59){'oil'}else{$null})}}
 $requested=@($presets)+@($blenders|Where-Object id -ne 10)
 if($LiquifyOnly){$requested=@($requested|Where-Object id -eq 12)}
 if($PresetIds.Count){
  foreach($id in $PresetIds){if($id -notin $requested.id){throw "Unknown brush preset: $id"}}
  $requested=@($requested|Where-Object id -in $PresetIds)
 }
 foreach($preset in $requested){
  $traceBefore=@(Shader-Lines|Where-Object {$_ -match '^pipeline begin '}).Count
  Select-Preset $preset.label $preset.id
  $controls=Check-BrushProjection $preset
  $selected=Observe ("selected-"+$preset.id) $null
  $before=Wait-StablePixels {Pixels};$paintBefore=Revision
  if($before -cne $seedBaseline){throw 'A preset began with composed artwork different from the original seed baseline'}
  Save-Pixels ("preset-"+$preset.id+'-before') $before
  $from=Point 110 110;$to=Point 215 140
  Stroke $from $to
  Wait-Until {(Revision) -ne $paintBefore -and ((Model).state.commands|Where-Object id -eq 'undo').enabled} "$($preset.label) did not commit a stroke"
  Wait-Until {(Pixels) -ne $before} "$($preset.label) did not change composed seeded artwork"
  $after=Wait-StablePixels {Pixels};$paintAfter=Revision
  Save-Pixels ("preset-"+$preset.id+'-after') $after
  Capture ("preset-"+$preset.id) -WithModel -Composed
  Assert-History 0x5a $before $paintBefore ($preset.label+' Undo')
  Assert-History 0x59 $after $paintAfter ($preset.label+' Redo')
  Assert-History 0x5a $before $paintBefore ($preset.label+' final Undo')
  $traceAfter=@(Shader-Lines|Where-Object {$_ -match '^pipeline begin '}).Count
  $case=[ordered]@{label=$preset.label;preset=$preset.id;selected=$selected;controls=$controls;before_pixels=$before;after_pixels=$after;before_paint=$paintBefore;after_paint=$paintAfter;new_pipeline_begins=$traceAfter-$traceBefore;history='exact'}
  $cases.Add($case)
  $cases|ConvertTo-Json -Depth 8|Set-Content -LiteralPath (Join-Path $run 'brush-cases.json')
 }
 if(!$PresetIds.Count){
 $seedPixels=Wait-StablePixels {Pixels}
 Select-Preset 'Liquify Push' 12
 $from=Point 110 110;$to=Point 215 140
 Stroke $from $to
 Wait-Until {(Pixels) -ne $seedPixels} 'Push did not deform the seeded artwork'
 $null=Wait-StablePixels {Pixels}
 foreach($mode in @(@('Edge',63),@('Reconstruct',64))){
  Select-Preset $mode[0] $mode[1]
  $settings=@((Model).state.tool_settings|ForEach-Object id)
  if('strength' -notin $settings -or 'size' -notin $settings){throw "$($mode[0]) lacks native Size or Strength controls"}
  if($mode[1] -eq 63 -and @($settings|Where-Object {$_ -in @('distortion','momentum')}).Count -ne 2){throw 'Edge lacks Distortion or Momentum'}
  if($mode[1] -eq 64 -and @($settings|Where-Object {$_ -in @('distortion','momentum')}).Count){throw 'Reconstruct exposes deformation-only controls'}
  $priorPixels=Wait-StablePixels {Pixels};$priorPaint=Revision
  Stroke $from $to
  Wait-Until {(Revision) -ne $priorPaint} "$($mode[0]) did not commit"
  Wait-Until {(Pixels) -ne $priorPixels} "$($mode[0]) did not change composed seeded artwork"
  $afterPixels=Wait-StablePixels {Pixels};$afterPaint=Revision
  Capture ("liquify-"+$mode[1]) -WithModel -Composed
  Assert-History 0x5a $priorPixels $priorPaint ($mode[0]+' Undo')
  Assert-History 0x59 $afterPixels $afterPaint ($mode[0]+' Redo')
  $checks["liquify_$($mode[1])"]='exact'
  if($mode[1] -eq 63){
   $adjust=Control 'tool-action-liquify_adjust'
   $adjust.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
   Wait-Until {@((Model).state.tool_settings|Where-Object id -eq 'liquify_amount').Count -eq 1} 'Adjust did not show Amount'
   $slider=Control 'tool-setting-liquify_amount-slider' -Type ([System.Windows.Automation.ControlType]::Slider)
   $scroll=$null
   if($slider.TryGetCurrentPattern([System.Windows.Automation.ScrollItemPattern]::Pattern,[ref]$scroll)){$scroll.ScrollIntoView()}
   $bounds=(Control 'tool-setting-liquify_amount-slider' -Arranged).Current.BoundingRectangle
   $startX=[int]($bounds.Left+$bounds.Width*.9);$endX=[int]($bounds.Left+$bounds.Width*.5);$sliderY=[int]($bounds.Top+$bounds.Height*.5)
   foreach($ending in @('cancel','up')){
    [CapyRowPointer]::Down('touch',$startX,$sliderY)
    try{
     for($step=1;$step -le 6;$step++){[CapyRowPointer]::Move([int]($startX+($endX-$startX)*$step/6),$sliderY);Start-Sleep -Milliseconds 30}
     Wait-Until {$setting=@((Model).state.tool_settings|Where-Object id -eq 'liquify_amount');$setting.Count -eq 1 -and $setting[0].value -gt .2 -and $setting[0].value -lt .8} 'Amount drag did not preview a middle value'
     Wait-Until {(Pixels) -ne $afterPixels} 'Amount drag did not preview composed pixels'
     if((Revision) -ne $afterPaint){throw 'Amount drag edited artwork before release'}
     if($ending -eq 'up'){$null=Wait-StablePixels {Pixels}}
    }finally{if($ending -eq 'cancel'){[CapyRowPointer]::Cancel()}else{[CapyRowPointer]::Up()}}
    if($ending -eq 'cancel'){
     Wait-Until {$setting=@((Model).state.tool_settings|Where-Object id -eq 'liquify_amount');$setting.Count -eq 1 -and [Math]::Abs($setting[0].value-1) -lt .001} 'Cancel did not restore Amount'
     Wait-Until {(Pixels) -eq $afterPixels} 'Cancel did not restore exact composed pixels'
     if((Wait-StablePixels {Pixels}) -ne $afterPixels -or (Revision) -ne $afterPaint){throw 'Cancel left a Liquify edit'}
    }
   }
   Wait-Until {(Revision) -ne $afterPaint} 'Amount release did not commit artwork'
   Wait-Until {(Pixels) -ne $afterPixels} 'Adjust did not change composed seeded artwork'
   $adjustedPixels=Wait-StablePixels {Pixels};$adjustedPaint=Revision
   Assert-History 0x5a $afterPixels $afterPaint 'Adjust Undo'
   Assert-History 0x59 $adjustedPixels $adjustedPaint 'Adjust Redo'
   $reset=Control 'tool-action-liquify_reset'
   $reset.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
   Wait-Until {(Pixels) -eq $seedPixels} 'Reset did not restore the seeded artwork'
   $resetPixels=Wait-StablePixels {Pixels}
   Assert-History-Pixels 0x5a $adjustedPixels 'Reset Undo'
   Assert-History-Pixels 0x59 $resetPixels 'Reset Redo'
   $checks.liquify_adjust_reset='exact'
   Select-Preset 'Liquify Push' 12
   Stroke $from $to
   Wait-Until {(Pixels) -ne $seedPixels} 'Push did not prepare artwork for Reconstruct'
  }
 }
 }
 $checks.scope=if($LiquifyOnly){'Liquify only'}elseif($PresetIds.Count){'selected presets'}else{'all presets'}
 $checks.brush_cases='passed';$checks.preset_ids=@($cases.preset);$checks.liquify_adjust_scope=if($PresetIds.Count){'outside selected preset run'}else{'passed'};Close-Review -Discard
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
