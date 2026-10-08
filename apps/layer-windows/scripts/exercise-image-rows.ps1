param([Parameter(Mandatory)][string]$Executable,[ValidateSet('mouse','pen','touch')][string]$Device='mouse',[ValidateSet('dark','light')][string]$Theme='dark')
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyFind='prefer-visible';$CapyPopups=$true;$CapyWaitSeconds=30
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
Add-Type -Path (Join-Path $PSScriptRoot 'PackageFixture.cs')
Add-Type -AssemblyName System.Drawing
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/image-rows/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$checks=[ordered]@{device=$Device;theme=$Theme}
function Start-Review([string]$Log){
 $script:review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $Log
 $null=$review.Handle
 Write-Output "Owned image row review $($review.Id): $run"
 $native=@{window=$null}
 Wait-Until {$native.window=Owned-DrawingWindow $review;$native.window -and (Model).brush_ready -and (Model).windows_workspace.ready -and !(Model).windows_workspace.busy} 'Image row review did not start' 90
 $script:drawingWindow=$native.window.Handle;$script:root=$native.window.Root
 $root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Maximized)
 Start-Sleep -Milliseconds 600
 [CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
 $null=[CapyRowPointer]::SetForegroundWindow($drawingWindow)
 [CapyRowPointer]::Initialize([uint32]$review.Id)
}
function Sta([scriptblock]$Script,[object[]]$Arguments=@()){
 $shell=[powershell]::Create();$shell.Runspace=[runspacefactory]::CreateRunspace();$shell.Runspace.ApartmentState='STA';$shell.Runspace.Open()
 try{$null=$shell.AddScript('Add-Type -AssemblyName System.Windows.Forms,System.Drawing').AddStatement().AddScript($Script);foreach($a in $Arguments){$null=$shell.AddArgument($a)};$result=$shell.Invoke();if($shell.HadErrors){throw ($shell.Streams.Error|Out-String)};$result}
 finally{$shell.Runspace.Dispose();$shell.Dispose()}
}
function Package([string]$Path){
 $members=[CapyPackageFixture]::Read($Path)
 [Text.Encoding]::UTF8.GetString(@($members|Where-Object Key -eq 'manifest.json')[0].Value)|ConvertFrom-Json -Depth 100
}
function Canonical($Value){
 if($null -ne $Value -and $Value.GetType() -eq [System.Management.Automation.PSCustomObject]){
  $record=[ordered]@{}
  foreach($property in $Value.PSObject.Properties|Sort-Object Name){$record[$property.Name]=Canonical $property.Value}
  return $record
 }
 if($Value -is [array]){return ,@($Value|ForEach-Object {Canonical $_})}
 $Value
}
function Json($Value){ConvertTo-Json -InputObject (Canonical $Value) -Depth 100 -Compress}
function Identity($Manifest){Json @($Manifest.objects|Where-Object type -in @('capy.image-object/1','capy.object-layer/1','capy.image/1','capy.occurrence/3')|Sort-Object id)}
function Assert-ImageCopy($Before,$After,[string]$Operation){
 $original=@($Before.objects|Where-Object type -eq 'capy.image-object/1');$pasted=@($After.objects|Where-Object type -eq 'capy.image-object/1')
 $owners=@($Before.objects|Where-Object type -eq 'capy.object-layer/1');$destinations=@($After.objects|Where-Object type -eq 'capy.object-layer/1')
 if($original.Count -ne 2 -or $owners.Count -ne 1 -or @($owners[0].data.children).Count -ne 2){throw 'The saved copy source must contain two images in one object layer'}
 $added=@($pasted|Where-Object id -NotIn $original.id)
 if($pasted.Count -ne 3 -or $added.Count -ne 1 -or $destinations.Count -ne 1 -or $destinations[0].id -ne $owners[0].id){throw "$Operation did not add one new image object to the original layer"}
 if((Json @($After.objects|Where-Object type -eq 'capy.image/1'|Sort-Object id)) -ne (Json @($Before.objects|Where-Object type -eq 'capy.image/1'|Sort-Object id))){throw "$Operation changed source images or added a flattened capy.image record"}
 if((Json @($pasted|Where-Object id -In $original.id|Sort-Object id)) -ne (Json @($original|Sort-Object id))){throw "$Operation changed an original image object"}
 $source=@($original|Where-Object id -eq $owners[0].data.children[0].ref)[0]
 if(!$source -or $added[0].data.image.ref -ne $source.data.image.ref -or (Json $added[0].data.affine) -ne (Json $source.data.affine)){throw "$Operation did not preserve the copied source image and affine"}
 if(($destinations[0].data.children.ref -join ',') -ne ((@($added[0].id)+@($owners[0].data.children.ref)) -join ',')){throw "$Operation did not preserve the ordered image-layer members"}
}
function Front-Object($Manifest){
 $owner=@($Manifest.objects|Where-Object type -eq 'capy.object-layer/1')[0]
 @($Manifest.objects|Where-Object id -eq $owner.data.children[0].ref)[0]
}
function Image-Point($Manifest,[double]$X,[double]$Y){
 $object=Front-Object $Manifest;$a=$object.data.affine
 $source=@($Manifest.objects|Where-Object id -eq $object.data.image.ref)[0]
 $owner=@($Manifest.objects|Where-Object type -eq 'capy.object-layer/1')[0]
 $occurrence=@($Manifest.objects|Where-Object {$_.type -eq 'capy.occurrence/3' -and $_.data.content.objects.ref -eq $owner.id})[0]
 $offset=if($occurrence.data.offset){$occurrence.data.offset}else{@(0,0)}
 @(($a[0]*$source.data.extent[0]*$X+$a[2]*$source.data.extent[1]*$Y+$a[4]+[double]$offset[0]),($a[1]*$source.data.extent[0]*$X+$a[3]*$source.data.extent[1]*$Y+$a[5]+[double]$offset[1]))
}
function Screen-Point([double[]]$Point,$Projection){
 if(!$Projection){$Projection=@{camera=(Model).state.camera;bounds=(Control 'drawing-canvas' -Arranged).Current.BoundingRectangle}}
 $c=$Projection.camera;$b=$Projection.bounds
 $x=$Point[0]*$c.zoom*$(if($c.flipped[0]){-1}else{1});$y=$Point[1]*$c.zoom*$(if($c.flipped[1]){-1}else{1})
 @([int]($b.X+([Math]::Cos($c.rotation)*$x-[Math]::Sin($c.rotation)*$y+$c.translation[0])*$b.Width/$c.viewport[0]),[int]($b.Y+([Math]::Sin($c.rotation)*$x+[Math]::Cos($c.rotation)*$y+$c.translation[1])*$b.Height/$c.viewport[1]))
}
function Artwork-Pixels{
 $foreground=[CapyRowPointer]::GetForegroundWindow()
 if($foreground -ne $drawingWindow){
  $foregroundOwner=[uint32]0
  [CapyRowPointer]::GetWindowThreadProcessId($foreground,[ref]$foregroundOwner)|Out-Null
  $foregroundClass=try{[System.Windows.Automation.AutomationElement]::FromHandle($foreground).Current.ClassName}catch{'unavailable'}
  throw "The owned drawing must be foreground for composed artwork samples (actual HWND=$($foreground.ToInt64()), PID=$foregroundOwner, class='$foregroundClass'; expected HWND=$($drawingWindow.ToInt64()), PID=$($review.Id))"
 }
 $bounds=(Control 'drawing-canvas').Current.BoundingRectangle;$camera=(Model).state.camera
 if($bounds -ne $sampleProjection.bounds -or (Json @($camera.viewport,$camera.translation,$camera.zoom,$camera.rotation,$camera.flipped)) -ne $sampleCamera){throw 'The camera or arranged canvas moved during the image artwork comparison'}
 $bitmap=[Drawing.Bitmap]::new([int]$bounds.Width,[int]$bounds.Height);$g=[Drawing.Graphics]::FromImage($bitmap)
 try{
  $g.CopyFromScreen([int]$bounds.X,[int]$bounds.Y,0,0,$bitmap.Size)
  (@(foreach($point in $samplePoints){$c=$bitmap.GetPixel([int]($point[0]-$bounds.X),[int]($point[1]-$bounds.Y));$c.ToArgb()}) -join ',')
 }finally{$g.Dispose();$bitmap.Dispose()}
}
function Pixel-Difference([string]$Before,[string]$After){
 $old=$Before.Split(',');$current=$After.Split(',');$maximum=0;$changed=0
 if($old.Count -ne $current.Count){throw 'Composed sample counts changed'}
 for($i=0;$i -lt $old.Count;$i++){
  if($old[$i] -eq $current[$i]){continue};$changed++
  $a=[Drawing.Color]::FromArgb([int]$old[$i]);$b=[Drawing.Color]::FromArgb([int]$current[$i])
  foreach($channel in 'R','G','B','A'){$maximum=[Math]::Max($maximum,[Math]::Abs($a.$channel-$b.$channel))}
 }
 @{max_channel_delta=$maximum;changed_samples=$changed;sample_count=$old.Count}
}
function Opaque-ImageChanged([string]$Before,[string]$After){
 $old=$Before.Split(',');$current=$After.Split(',');$changed=[bool[]]::new($old.Length)
 for($i=0;$i -lt $old.Length;$i++){
  if($old[$i] -eq $current[$i]){continue}
  foreach($value in @($old[$i],$current[$i])){
   $color=[Drawing.Color]::FromArgb([int]$value)
   if(($color.R -gt $color.G+80 -and $color.R -gt $color.B+80) -or ($color.B -gt $color.R+80 -and $color.B -gt $color.G+80)){$changed[$i]=$true}
  }
 }
 foreach($y in 0..14){foreach($x in 0..14){$i=$y*16+$x;if($changed[$i] -and $changed[$i+1] -and $changed[$i+16] -and $changed[$i+17]){return $true}}}
 $false
}
function Park{
 $at=Point 'settings-button';[CapyRowPointer]::Hover($at.x,$at.y)
}
function Assert-ImageHistory([string]$Command,[scriptblock]$Ready){
 Wait-Until {((Model).state.commands|Where-Object id -eq $Command.ToLowerInvariant()).enabled} "$Command did not become available"
 $revision=(Model).state.document_file.revision
 Invoke $Command -Name
 Wait-Until {(Model).state.document_file.revision -ne $revision -and (& $Ready)} "$Command did not restore the expected image edit in one step"
}
function Assert-ImageTransform([string]$Name,$Before,$After){
 $old=Front-Object $Before;$edited=@($after.objects|Where-Object id -eq $old.id)[0]
 if(!$edited -or (Json $old.data.affine) -eq (Json $edited.data.affine)){throw "$Name did not save a changed affine on the selected image"}
 $normalized=(Json $after)|ConvertFrom-Json -Depth 100
 @($normalized.objects|Where-Object id -eq $old.id)[0].data.affine=$old.data.affine
 if((Identity $normalized) -ne (Identity $Before)){throw "$Name changed an image source, an unselected image, the image order or layer presentation"}
 $a=$old.data.affine;$b=$edited.data.affine
 if($Name -eq 'move' -and (Json $a[0..3]) -ne (Json $b[0..3])){throw 'Move changed the image scale or rotation'}
 if($Name -eq 'scale' -and @(0..3|Where-Object {[Math]::Abs($b[$_]-$a[$_]*1.25) -gt .02}).Count){throw 'The corner drag did not scale the image to its requested size'}
 if($Name -eq 'rotate' -and (@(0..3|Where-Object {[Math]::Abs($b[$_]-@(-$a[1],$a[0],-$a[3],$a[2])[$_]) -gt 0.00001}).Count)){throw 'Rotate Right did not preserve the image scale in its quarter turn'}
}
function Author-Image([string]$Name,$Before,[scriptblock]$Edit){
 Park;$pixels=Wait-StablePixels {Artwork-Pixels};$revision=(Model).state.document_file.revision
 & $Edit
 Wait-Until {(Model).state.document_file.revision -ne $revision -and ((Model).state.commands|Where-Object id -eq 'undo').enabled} "$Name did not commit an image edit"
 Park;Wait-Until {Opaque-ImageChanged $pixels (Artwork-Pixels)} "$Name did not change the composed opaque artwork"
 $changed=Wait-StablePixels {Artwork-Pixels};Capture "image-$Name-$Device-$Theme" -WithModel -Composed
 Assert-ImageHistory 'Undo' {(Artwork-Pixels) -eq $pixels};Assert-ImageHistory 'Redo' {(Artwork-Pixels) -eq $changed}
 $path=Join-Path $run ("image-$Name.capy");Save-ProjectAs $path;$after=Package $path
 Assert-ImageTransform $Name $Before $after
 $after
}
function Drag-Image([int[]]$From,[int[]]$To){
 [CapyRowPointer]::Down($Device,$From[0],$From[1])
 try{for($step=1;$step -le 8;$step++){[CapyRowPointer]::Move([int]($From[0]+($To[0]-$From[0])*$step/8),[int]($From[1]+($To[1]-$From[1])*$step/8))};[CapyRowPointer]::Up()}
 finally{[CapyRowPointer]::Cancel()}
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
 [CapyRowPointer]::SetForegroundWindow($drawingWindow)|Out-Null
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
function Choose([string]$Name){
 $item=Menu-Item $Name;$item.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
 Wait-Until {
  $closed=try{$item.Current.IsOffscreen}catch [System.Windows.Automation.ElementNotAvailableException]{$true}
  $closed -and [CapyRowPointer]::GetForegroundWindow() -eq $drawingWindow
 } 'The invoked row menu did not dismiss and return foreground to its owned drawing'
}
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
 Start-Review $stderr
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
 $from=Point "image-$front-drag";$bounds=(Control "image-row-$back" -Arranged).Current.BoundingRectangle
 [CapyRowPointer]::Down($Device,$from.x,$from.y)
 [CapyRowPointer]::Move([int]($bounds.X+$bounds.Width*.5),[int]($bounds.Y+$bounds.Height*.8))
 Wait-Until {(Gesture).phase -eq 'dragging' -and (Gesture).can_drop} 'The image grip did not begin a cancellable drag'
 [CapyRowPointer]::Key(0x1b);[CapyRowPointer]::Up()
 Wait-Until {(Gesture).phase -eq 'idle'} 'Escape did not cancel the image-row drag'
 if((Order) -ne $before){throw 'Canceling the image-row drag changed its order'}
 $checks.reorder_cancel='passed'
 $from=Point "image-$front-drag";$bounds=(Control "image-row-$back").Current.BoundingRectangle
 [CapyRowPointer]::Down($Device,$from.x,$from.y);Start-Sleep -Milliseconds 35
 [CapyRowPointer]::Move([int]($bounds.X+$bounds.Width*.5),[int]($bounds.Y+$bounds.Height*.8))
 Wait-Until {$g=Gesture;$g.phase -eq 'dragging' -and $g.can_drop -and $g.target -eq $back -and $g.position -eq 'below'} 'Dragging the image grip did not reach the other image'
 [CapyRowPointer]::Up()
 Wait-Until {(Gesture).phase -eq 'idle' -and (Order) -ne $before} 'Dropping the image did not reorder the images'
 if((Order) -ne "$back,$front"){throw "Unexpected image order: $(Order)"}
 Invoke 'Undo' -Name;Wait-Until {(Order) -eq $before} 'Undo did not restore the image order'
 Invoke 'Redo' -Name;Wait-Until {(Order) -eq "$back,$front"} 'Redo did not restore the image order in one step'
 Invoke 'Undo' -Name;Wait-Until {(Order) -eq $before} 'Undo after Redo did not restore the original image order'
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
 Chord @(0x11) 0x44;Wait-Until {!@(Images|Where-Object selected).Count} 'Deselect Images did not clear the image selection'
 [CapyRowPointer]::Down($Device,$center.x,$center.y);Start-Sleep -Milliseconds 35;[CapyRowPointer]::Up()
 Wait-Until {(Image $front).selected -and !(Image $back).selected} 'The canvas did not pick the front image'
 if(@((Model).state.layers).Count -ne $count+1){throw 'Picking an image changed the layers'}
 Capture "image-handles-$Device-$Theme" -WithModel
 $checks.canvas_picking='passed'

 $authoredSource=Join-Path $run 'image-before-authoring.capy';Save-ProjectAs $authoredSource
 $authored=Package $authoredSource
 $sampleProjection=@{camera=(Model).state.camera;bounds=(Control 'drawing-canvas' -Arranged).Current.BoundingRectangle}
 $camera=$sampleProjection.camera;$sampleCamera=Json @($camera.viewport,$camera.translation,$camera.zoom,$camera.rotation,$camera.flipped)
 $corners=@(foreach($point in @(@(0,0),@(1,0),@(0,1),@(1,1))){,(Screen-Point (Image-Point $authored $point[0] $point[1]) $sampleProjection)})
 $xs=@($corners|ForEach-Object {$_[0]});$ys=@($corners|ForEach-Object {$_[1]})
 $left=($xs|Measure-Object -Minimum).Minimum;$top=($ys|Measure-Object -Minimum).Minimum
 $initial=[Windows.Rect]::new($left,$top,($xs|Measure-Object -Maximum).Maximum-$left,($ys|Measure-Object -Maximum).Maximum-$top)
 $moved=[Windows.Rect]::new($initial.X+60,$initial.Y+35,$initial.Width,$initial.Height)
 $scaled=[Windows.Rect]::new($moved.X,$moved.Y,$moved.Width*1.25,$moved.Height*1.25)
 $rotated=[Windows.Rect]::new($scaled.X+($scaled.Width-$scaled.Height)/2,$scaled.Y+($scaled.Height-$scaled.Width)/2,$scaled.Height,$scaled.Width)
 $envelope=$initial;foreach($bounds in @($moved,$scaled,$rotated)){$envelope=[Windows.Rect]::Union($envelope,$bounds)};$envelope.Inflate(5,5)
 $samplePoints=@(foreach($y in 0..15){foreach($x in 0..15){,@([int]($envelope.X+$envelope.Width*($x+.5)/16),[int]($envelope.Y+$envelope.Height*($y+.5)/16))}})
 @{initial=$initial.ToString();moved=$moved.ToString();scaled=$scaled.ToString();rotated=$rotated.ToString();envelope=$envelope.ToString();pitch=@(($envelope.Width/16),($envelope.Height/16));points=$samplePoints}|ConvertTo-Json -Depth 4|Set-Content (Join-Path $run 'image-sample-grid.json')
 foreach($point in $samplePoints){if(!$sampleProjection.bounds.Contains([double]$point[0],[double]$point[1])){throw 'The authored-image sample grid does not fit the arranged canvas'}}
 $authored=Author-Image 'move' $authored {
  $from=Screen-Point (Image-Point $authored .7 .65);Drag-Image $from @(($from[0]+60),($from[1]+35))
 }
 Chord @(0x11) 0x54
 Wait-Until {(Model).state.layer_tools.tool -eq 'transform'} 'Scale-Rotate did not target the selected image'
 $authored=Author-Image 'scale' $authored {
  Drag-Image (Screen-Point (Image-Point $authored 1 1)) (Screen-Point (Image-Point $authored 1.25 1.25))
 }
 $authored=Author-Image 'rotate' $authored {Invoke-Id 'canvas-bar-transform_rotate_right'}
 $checks.image_affine_authoring='passed'
 $copyManifest=$authored
 Park;$originalPixels=Wait-StablePixels {Artwork-Pixels}
 Context "image-row-$front";Choose 'Duplicate Images'
 Wait-Until {(Image-Layer).object_count -eq 3 -and @(Images|Where-Object selected).Count -eq 1 -and !(Image $front).selected} 'Duplicate Images did not select one new image'
 Park;Wait-Until {(Artwork-Pixels) -eq $originalPixels} 'Duplicating an opaque image in place changed its composed artwork'
 $duplicatePixels=Wait-StablePixels {Artwork-Pixels}
 $duplicated=Join-Path $run 'image-duplicated.capy';Save-ProjectAs $duplicated
 Assert-ImageCopy $copyManifest (Package $duplicated) 'Duplicate Images'
 Capture "image-duplicated-$Device-$Theme" -WithModel -Composed
 Assert-ImageHistory 'Undo' {(Image-Layer).object_count -eq 2 -and (Order) -eq $before}
 Assert-ImageHistory 'Redo' {(Image-Layer).object_count -eq 3 -and (Artwork-Pixels) -eq $duplicatePixels}
 Assert-ImageHistory 'Undo' {(Image-Layer).object_count -eq 2 -and (Order) -eq $before}
 Tap "image-$front-name"
 $checks.duplicate_images='passed'
 Wait-Until {(Image $front).selected -and !(Image $back).selected -and (Order) -eq $before} 'Saving before Copy changed the selected image or its order'
 Sta {[Windows.Forms.Clipboard]::SetText('awaiting image copy')}|Out-Null
 Chord @(0x11) 0x43
 Wait-Until {(Requests) -eq 0 -and (Sta {[Windows.Forms.Clipboard]::ContainsData('art.capycanvas.clip.nonce')})} 'Copy Image did not publish its private clipboard identity'
 Chord @(0x11,0x10) 0x56
 Wait-Until {(Requests) -eq 0 -and (Image-Layer).object_count -eq 3} 'Pasting the copied image did not add an image to its layer'
 if(@((Model).state.layers).Count -ne $count+1){throw 'Pasting an image copy created another layer'}
 $copyPasted=Join-Path $run 'clipboard-pasted.capy';Save-ProjectAs $copyPasted
 Assert-ImageCopy $copyManifest (Package $copyPasted) 'Paste in Place'
 $checks.structured_image_clipboard='passed'
 Invoke 'Undo' -Name;Wait-Until {(Image-Layer).object_count -eq 2} 'One Undo did not remove the pasted image'
 Invoke 'Redo' -Name;Wait-Until {(Image-Layer).object_count -eq 3} 'One Redo did not restore the pasted image'
 Invoke 'Undo' -Name;Wait-Until {(Image-Layer).object_count -eq 2} 'Undo after Redo did not remove the pasted image'
 Tap "image-$front-name"
 Wait-Until {(Image $front).selected -and !(Image $back).selected} 'The front image did not select before Cut'
 Sta {[Windows.Forms.Clipboard]::SetText('awaiting image cut')}|Out-Null
 Chord @(0x11) 0x58
 Wait-Until {(Requests) -eq 0 -and (Image-Layer).object_count -eq 1 -and (Sta {[Windows.Forms.Clipboard]::ContainsData('art.capycanvas.clip.nonce')})} 'Cut did not copy and remove the selected image'
 Invoke 'Undo' -Name;Wait-Until {(Image-Layer).object_count -eq 2 -and (Order) -eq $before} 'One Undo did not restore the cut image and its order'
 $checks.image_clipboard_history='passed'

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
 Park;$imagePixels=Wait-StablePixels {Artwork-Pixels}
 Context "layer-row-$layer";Choose 'Rasterize Layer'
 Wait-Until {!(Image-Layer) -and @((Model).state.layers).Count -eq $layers} 'Rasterize Layer did not turn the images into paint'
 Park;$rasterPixels=Wait-StablePixels {Artwork-Pixels};$rasterDifference=Pixel-Difference $imagePixels $rasterPixels
 @{image=$imagePixels;raster=$rasterPixels;difference=$rasterDifference}|ConvertTo-Json|Set-Content (Join-Path $run 'rasterize-pixels.json')
 if($rasterDifference.max_channel_delta -gt 1){throw 'Rasterize Layer changed the composed artwork beyond one display level'}
 Assert-ImageHistory 'Undo' {(Image-Layer).object_count -eq 2 -and (Artwork-Pixels) -eq $imagePixels}
 Assert-ImageHistory 'Redo' {!(Image-Layer) -and (Artwork-Pixels) -eq $rasterPixels}
 Context "layer-row-$layer";Choose 'Convert to Image Layer'
 Wait-Until {(Image-Layer).id -eq $layer -and (Image-Layer).object_count -eq 1 -and @((Model).state.layers).Count -eq $layers} 'Convert to Image Layer did not replace paint on the same layer'
 Park;$convertedPixels=Wait-StablePixels {Artwork-Pixels};$conversionDifference=Pixel-Difference $rasterPixels $convertedPixels
 @{raster=$rasterPixels;converted=$convertedPixels;difference=$conversionDifference}|ConvertTo-Json|Set-Content (Join-Path $run 'convert-to-image-pixels.json')
 if($conversionDifference.max_channel_delta -gt 1){throw 'Convert to Image Layer changed the composed artwork beyond one display level'}
 $converted=Join-Path $run 'image-converted.capy';Save-ProjectAs $converted;$conversion=Package $converted
 if(@($conversion.objects|Where-Object type -eq 'capy.image-object/1').Count -ne 1 -or @($conversion.objects|Where-Object type -eq 'capy.image/1').Count -ne 1 -or @(@($conversion.objects|Where-Object type -eq 'capy.object-layer/1')[0].data.children).Count -ne 1){throw 'Convert to Image Layer did not save one image and one ordered child'}
 Capture "image-converted-$Device-$Theme" -WithModel -Composed
 Assert-ImageHistory 'Undo' {!(Image-Layer) -and (Artwork-Pixels) -eq $rasterPixels}
 Assert-ImageHistory 'Redo' {(Image-Layer).object_count -eq 1 -and (Artwork-Pixels) -eq $convertedPixels}
 Assert-ImageHistory 'Undo' {!(Image-Layer) -and (Artwork-Pixels) -eq $rasterPixels}
 Assert-ImageHistory 'Undo' {(Image-Layer).object_count -eq 2 -and (Artwork-Pixels) -eq $imagePixels}
 $checks.layer_conversions='passed'

 $saved=Join-Path $run 'Images 日本語.capy'
 Save-ProjectAs $saved
 $savedIdentity=Identity (Package $saved)
 if($savedIdentity -ne (Identity $authored)){throw 'Clipboard and conversion history did not restore the authored image identities and affines'}
 & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'File';Invoke-Id 'close_document'
 Wait-Until {$review.HasExited} 'Closing the sole edited drawing did not exit' 30 -Closing
 if($review.ExitCode -ne 0){throw 'The edited drawing did not close cleanly'}
 [CapyRowPointer]::Dispose()
 $reopenStderr=Join-Path $run 'reopen-stderr.log';Start-Review $reopenStderr
 Open-Project $saved
 Wait-Until {(Model).state.document_file.location.uri -eq $saved -and !(Model).state.document_file.busy -and (Image-Layer).object_count -eq 2} 'The edited image drawing did not reopen'
 if(!(Image-Layer).expanded){Tap ('layer-'+(Image-Layer).id+'-expand')}
 Wait-Until {@(Images).Count -eq 2} 'The reopened image rows did not expand'
 $reopened=Join-Path $run 'Images reopened.capy';Save-ProjectAs $reopened
 if((Identity (Package $reopened)) -ne $savedIdentity){throw 'Save/reopen changed image identities, shared sources, affines or ordered layer members'}
 foreach($id in @(Images|ForEach-Object id)){
  Wait-Until {(Control "image-$id-thumbnail").Current.ItemStatus -eq 'Ready'} 'The reopened image preview did not arrive' 45
 }
 Capture "image-reopened-$Device-$Theme" -WithModel -Composed
 $checks.edited_image_save_reopen='passed'

 [CapyRowPointer]::Dispose()
 & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow -Action Close -DiscardUnsaved
 foreach($log in @($stderr,$reopenStderr)){if((Get-Item -LiteralPath $log).Length){throw 'Native image row stderr needs inspection'}}
 $checks.evidence=$run
 [PSCustomObject]$checks|ConvertTo-Json
}catch{
 if($imagePixels -and $review -and !$review.HasExited){try{@{before=$imagePixels;current=(Artwork-Pixels)}|ConvertTo-Json|Set-Content (Join-Path $run 'conversion-failure-pixels.json')}catch{}}
 if($review -and !$review.HasExited){try{Capture 'failure' -WithModel}catch{}}
 [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{[CapyRowPointer]::Dispose();Exit-CapyEnvironment}
