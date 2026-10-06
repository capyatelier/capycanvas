param([Parameter(Mandatory)][string]$Executable,[ValidateSet('mouse','touch')][string]$Device='mouse',[ValidateSet('dark','light')][string]$Theme='dark')
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyFind='prefer-visible';$CapyPopups=$true;$CapyWaitSeconds=30
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
Add-Type -AssemblyName System.Drawing
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/image-rows/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$checks=[ordered]@{device=$Device;theme=$Theme}
function Sta([scriptblock]$Script,[object[]]$Arguments=@()){
 $shell=[powershell]::Create();$shell.Runspace=[runspacefactory]::CreateRunspace();$shell.Runspace.ApartmentState='STA';$shell.Runspace.Open()
 try{$null=$shell.AddScript('Add-Type -AssemblyName System.Windows.Forms,System.Drawing').AddStatement().AddScript($Script);foreach($a in $Arguments){$null=$shell.AddArgument($a)};$result=$shell.Invoke();if($shell.HadErrors){throw ($shell.Streams.Error|Out-String)};$result}
 finally{$shell.Runspace.Dispose();$shell.Dispose()}
}
function Gesture {try{(Find 'layer-list').Current.ItemStatus|ConvertFrom-Json}catch{}}
function Image-Layer{@((Model).state.layers|Where-Object {$_.object_count -gt 0})|Select-Object -First 1}
function Images{@((Image-Layer).objects)}
function Image([double]$Id){@(Images|Where-Object id -eq $Id)|Select-Object -First 1}
function Order{(@(Images|ForEach-Object id)) -join ','}
function Requests{@((Model).state.requests).Count}
function Settled{Wait-Until {(Requests) -eq 0 -and !(Model).state.document_file.busy} 'The document request did not finish'}
function Chord([uint16[]]$Modifiers,[uint16]$Key){
 (Control 'drawing-canvas').SetFocus()
 [CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)|Out-Null
 [CapyRowPointer]::Chord([uint32]$review.Id,$Modifiers,$Key)
}
function Paste-Image([string]$Name,[int[]]$Rgb,[int]$Width,[int]$Height){
 $path=Join-Path $run "$Name.png"
 $bitmap=[Drawing.Bitmap]::new($Width,$Height);try{$g=[Drawing.Graphics]::FromImage($bitmap);$g.Clear([Drawing.Color]::FromArgb(255,$Rgb[0],$Rgb[1],$Rgb[2]));$g.Dispose();$bitmap.Save($path,[Drawing.Imaging.ImageFormat]::Png)}finally{$bitmap.Dispose()}
 Sta {param($path)$image=[Drawing.Image]::FromFile($path);try{[Windows.Forms.Clipboard]::SetImage($image)}finally{$image.Dispose()}} @($path)|Out-Null
 Chord @(0x11,0x10) 0x56;Settled
}
function Point([string]$Id){
 $stable=@{bounds=$null;count=0}
 Wait-Until {
  $item=Find $Id
  if(!$item -or $item.Current.IsOffscreen){return $false}
  $bounds=$item.Current.BoundingRectangle
  if($bounds.IsEmpty -or $bounds.Width -le 0 -or $bounds.Height -le 0){return $false}
  if($bounds -eq $stable.bounds){$stable.count++}else{$stable.bounds=$bounds;$stable.count=0}
  $stable.count -ge 2
 } "$Id did not arrange visibly"
 @{x=[int]($stable.bounds.X+$stable.bounds.Width*.5);y=[int]($stable.bounds.Y+$stable.bounds.Height*.5)}
}
function Tap([string]$Id){$at=Point $Id;[CapyRowPointer]::Down($Device,$at.x,$at.y);Start-Sleep -Milliseconds 35;[CapyRowPointer]::Up()}
function Menu-Item([string]$Name){
 $hit=@{item=$null};Wait-Until {$hit.item=Find $Name -Name -Type ([System.Windows.Automation.ControlType]::MenuItem);$hit.item -and !$hit.item.Current.IsOffscreen} "Missing menu item: $Name"
 $hit.item
}
function Choose([string]$Name){(Menu-Item $Name).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()}
function Context([string]$Id){
 $at=Point $Id
 if($Device -eq 'mouse'){[CapyRowPointer]::RightClick($at.x,$at.y)}
 else{
  [CapyRowPointer]::Down($Device,$at.x,$at.y)
  Wait-Until {(Gesture).menu_open} 'Holding the row did not open its menu' 4
  [CapyRowPointer]::Up()
 }
}
function Dismiss{[CapyRowPointer]::Key(0x1b);Wait-Until {!(Gesture).menu_open} 'The row menu did not close'}
function Canvas-Point{
 $area=(Model).state.camera.work_area;$bounds=(Control 'drawing-canvas').Current.BoundingRectangle
 @{x=[int]($bounds.X+$area[0]+$area[2]/2);y=[int]($bounds.Y+$area[1]+$area[3]/2)}
}
try {
 Enter-CapyEnvironment
 $env:CAPY_STORAGE_DIR=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1'
 [IO.File]::WriteAllText((Settings-File),(@{language=@{Explicit='en'};theme=$Theme}|ConvertTo-Json -Depth 4))
 $stderr=Join-Path $run 'stderr.log'
 $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
 $null=$review.Handle
 Write-Output "Owned image row review $($review.Id): $run"
 Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero -and (Model).brush_ready -and (Model).windows_workspace.ready -and !(Model).windows_workspace.busy} 'Image row review did not start' 90
 $root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
 $root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Maximized)
 Start-Sleep -Milliseconds 600
 [CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
 $null=[CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)
 [CapyRowPointer]::Initialize([uint32]$review.Id)
 if(!@((Model).layout.groups|Where-Object {$_.active -eq 'layers'}).Count){Invoke 'Layers' -Name}

 $count=@((Model).state.layers).Count
 Paste-Image 'red' @(220,40,30) 640 480
 Wait-Until {@((Model).state.layers).Count -eq $count+1 -and (Image-Layer).object_count -eq 1} 'Paste in Place did not create an image layer'
 Paste-Image 'blue' @(30,60,220) 480 360
 Wait-Until {@((Model).state.layers).Count -eq $count+1 -and (Image-Layer).object_count -eq 2} 'The second image did not enter the active image layer'
 $layer=(Image-Layer).id
 if(!(Image-Layer).expanded){Tap "layer-$layer-expand"}
 Wait-Until {(Image-Layer).expanded -and @(Images).Count -eq 2} 'The image list did not expand'
 $expand=Control "layer-$layer-expand"
 if($expand.Current.Name -ne 'Collapse image list'){throw "Unexpected expand caption: $($expand.Current.Name)"}
 $ids=@(Images|ForEach-Object id);$front=$ids[0];$back=$ids[1]
 foreach($id in $ids){
  $null=Point "image-row-$id"
  Wait-Until {(Control "image-$id-thumbnail").Current.ItemStatus -eq 'Ready'} "Image $id preview did not arrive" 45
 }
 if((Control "image-row-$front").Current.Name -ne (Image $front).label){throw 'The image row is not named after its image'}
 Capture "image-rows-$Device-$Theme" -WithModel
 $checks.rows_and_previews='passed'

 Tap "image-$front-name"
 Wait-Until {(Image $front).selected -and !(Image $back).selected} 'Selecting an image row did not select only that image'
 if($Device -eq 'mouse'){
  $at=Point "image-$back-name"
  [CapyRowPointer]::Hold(0x10,$true)
  try{Start-Sleep -Milliseconds 100;[CapyRowPointer]::Down('mouse',$at.x,$at.y);Start-Sleep -Milliseconds 35;[CapyRowPointer]::Up();Start-Sleep -Milliseconds 100}
  finally{[CapyRowPointer]::Hold(0x10,$false)}
 }else{
  Context "image-row-$back";Choose 'Add to Selection'
 }
 Wait-Until {(Image $front).selected -and (Image $back).selected} 'Additive selection did not add the second image'
 Context "image-row-$front";$null=Menu-Item 'Remove from Selection';$null=Menu-Item 'Bring to Front'
 Capture "image-row-menu-$Device-$Theme" -Composed
 Dismiss
 $checks.selection_and_menu='passed'

 Tap "image-$back-visibility"
 Wait-Until {!(Image $back).visible} 'The eye did not hide the image'
 if((Control "image-$back-visibility").Current.Name -ne 'Show Image'){throw 'The hidden image eye is not named Show Image'}
 Invoke 'Undo' -Name;Wait-Until {(Image $back).visible} 'Undo did not show the image again'
 $checks.visibility='passed'

 $before=Order
 $from=Point "image-$front-drag";$bounds=(Control "image-row-$back").Current.BoundingRectangle
 [CapyRowPointer]::Down($Device,$from.x,$from.y);Start-Sleep -Milliseconds 35
 [CapyRowPointer]::Move([int]($bounds.X+$bounds.Width*.5),[int]($bounds.Y+$bounds.Height*.8))
 Wait-Until {$g=Gesture;$g.phase -eq 'dragging' -and $g.can_drop -and $g.target -eq $back -and $g.position -eq 'below'} 'Dragging the image grip did not reach the other image'
 [CapyRowPointer]::Up()
 Wait-Until {(Gesture).phase -eq 'idle' -and (Order) -ne $before} 'Dropping the image did not reorder the images'
 if((Order) -ne "$back,$front"){throw "Unexpected image order: $(Order)"}
 Invoke 'Undo' -Name;Wait-Until {(Order) -eq $before} 'Undo did not restore the image order'
 $checks.reorder='passed'

 Tap "layer-$layer-expand"
 Wait-Until {!(Image-Layer).expanded -and !(Find "image-row-$front")} 'Collapsing did not hide the image rows'
 Tap "layer-$layer-expand"
 Wait-Until {(Image-Layer).expanded -and @(Images).Count -eq 2} 'Expanding again did not restore the image rows'
 $checks.collapse='passed'

 (Find 'drawing-canvas').SetFocus();[CapyRowPointer]::Key([uint32]$review.Id,0x4F)
 Wait-Until {((Model).state.commands|Where-Object id -eq 'move').selected} 'O did not choose Move'
 $center=Canvas-Point
 if($Device -eq 'mouse'){
  $area=(Model).state.camera.work_area;$bounds=(Control 'drawing-canvas').Current.BoundingRectangle
  [CapyRowPointer]::Down('mouse',[int]($bounds.X+$area[0]+12),[int]($bounds.Y+$area[1]+12));Start-Sleep -Milliseconds 35;[CapyRowPointer]::Up()
  Wait-Until {!@(Images|Where-Object selected).Count} 'Clicking empty canvas did not clear the image selection'
 }
 Tap "image-$back-name";Wait-Until {(Image $back).selected -and !(Image $front).selected} 'The back image row did not select it'
 [CapyRowPointer]::Down($Device,$center.x,$center.y);Start-Sleep -Milliseconds 35;[CapyRowPointer]::Up()
 Start-Sleep -Milliseconds 300
 if(!(Image $back).selected -or (Image $front).selected){throw 'A contact inside the selected image did not keep its selection'}
 Invoke 'Deselect Images' -Name;Wait-Until {!@(Images|Where-Object selected).Count} 'Deselect Images did not clear the image selection'
 [CapyRowPointer]::Down($Device,$center.x,$center.y);Start-Sleep -Milliseconds 35;[CapyRowPointer]::Up()
 Wait-Until {(Image $front).selected -and !(Image $back).selected} 'The canvas did not pick the front image'
 if(@((Model).state.layers).Count -ne $count+1){throw 'Picking an image changed the layers'}
 Capture "image-handles-$Device-$Theme" -WithModel
 $checks.canvas_picking='passed'

 Invoke-Id (Tool-Tile 'brush')
 Wait-Until {(Model).state.layer_tools.editing_layer.id -eq $layer} 'The image layer is not the active layer'
 $center=Canvas-Point
 [CapyRowPointer]::Down('pen',$center.x,$center.y);for($i=1;$i -le 10;$i++){[CapyRowPointer]::Move($center.x+$i*4,$center.y+$i*2);Start-Sleep -Milliseconds 12};[CapyRowPointer]::Up()
 Wait-Until {(Model).state.notice -and @((Model).state.notice.actions).Count -eq 3} 'Painting on images did not offer the image actions'
 $actions=@((Model).state.notice.actions|ForEach-Object id) -join ','
 if($actions -ne 'add_mask,new_paint_layer,rasterize_layer'){throw "Unexpected image refusal actions: $actions"}
 foreach($token in @('add_mask','new_paint_layer','rasterize_layer')){$null=Control "canvas-notice-action-$token"}
 Capture "image-paint-refusal-$Device-$Theme"
 $layers=@((Model).state.layers).Count
 Invoke 'canvas-notice-action-new_paint_layer'
 Wait-Until {@((Model).state.layers).Count -eq $layers+1 -and (Model).state.layer_tools.editing_layer.id -ne $layer} 'New Paint Layer did not add a paint layer'
 Invoke 'Undo' -Name;Wait-Until {@((Model).state.layers).Count -eq $layers} 'Undo did not remove the new paint layer'
 $checks.paint_refusal_actions=$actions

 Tap "layer-$layer-name";Wait-Until {(Model).state.layer_tools.editing_layer.id -eq $layer} 'Selecting the image layer row did not activate it'
 Context "layer-row-$layer";Choose 'Rasterize Layer'
 Wait-Until {!(Image-Layer) -and @((Model).state.layers).Count -eq $layers} 'Rasterize Layer did not turn the images into paint'
 Invoke 'Undo' -Name;Wait-Until {(Image-Layer).object_count -eq 2} 'Undo did not restore the image layer'
 $checks.rasterize_layer='passed'

 [CapyRowPointer]::Dispose()
 & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved
 if((Get-Item -LiteralPath $stderr).Length){throw 'Native image row stderr needs inspection'}
 $checks.evidence=$run
 [PSCustomObject]$checks|ConvertTo-Json
}catch{
 if($review -and !$review.HasExited){try{Capture 'failure' -WithModel}catch{}}
 [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{[CapyRowPointer]::Dispose();Exit-CapyEnvironment}
