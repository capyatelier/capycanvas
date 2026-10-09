param([Parameter(Mandatory)][string]$Executable,[ValidateSet('mouse','pen','touch')][string]$Device='mouse',[ValidateSet('dark','light')][string]$Theme='dark',[switch]$MotionPreflight)
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
. (Join-Path $PSScriptRoot 'ObjectFixture.ps1')
function Artwork-Pixels{
 $foreground=[CapyRowPointer]::GetForegroundWindow()
 if($foreground -ne $drawingWindow){
  $foregroundOwner=[uint32]0
  [CapyRowPointer]::GetWindowThreadProcessId($foreground,[ref]$foregroundOwner)|Out-Null
  $foregroundClass=try{[System.Windows.Automation.AutomationElement]::FromHandle($foreground).Current.ClassName}catch{'unavailable'}
  throw "The owned drawing must be foreground for composed artwork samples (actual HWND=$($foreground.ToInt64()), PID=$foregroundOwner, class='$foregroundClass'; expected HWND=$($drawingWindow.ToInt64()), PID=$($review.Id))"
 }
 $bounds=(Control 'drawing-canvas').Current.BoundingRectangle;$camera=(Model).state.camera
 if($bounds -ne $sampleProjection.bounds -or (Json @($camera.viewport,$camera.translation,$camera.zoom,$camera.rotation,$camera.flipped)) -ne $sampleCamera){throw "The camera or arranged canvas moved during the image artwork comparison (bounds '$($sampleProjection.bounds)' -> '$bounds'; camera '$sampleCamera' -> '$(Json @($camera.viewport,$camera.translation,$camera.zoom,$camera.rotation,$camera.flipped))')"}
 Sample-ObjectArtwork $bounds $samplePoints
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
 Assert-ObjectMotion $Before $After $Name
 $a=Effective-Affine $Before (Front-Layer $Before);$b=Effective-Affine $After (Front-Layer $After)
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

function Rows{@((Model).state.layers|Where-Object object)}
function Row([double]$Id){@((Model).state.layers|Where-Object id -eq $Id)[0]}
function Order{(@(Rows|ForEach-Object id)) -join ','}
function Gesture {try{(Find 'layer-list').Current.ItemStatus|ConvertFrom-Json}catch{}}
function Chord([uint16[]]$Modifiers,[uint16]$Key){
 (Control 'drawing-canvas').SetFocus();[CapyRowPointer]::SetForegroundWindow($drawingWindow)|Out-Null
 [CapyRowPointer]::Chord([uint32]$review.Id,$Modifiers,$Key)
}
function Paste-In-Place{
 & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'edit'
 Invoke-Id 'paste_in_place'
}
function Requests{@((Model).state.requests).Count}
function Paste-Image([string]$Name,[int[]]$Rgb,[int]$Width,[int]$Height){
 $path=Join-Path $run "$Name.png"
 $bitmap=[Drawing.Bitmap]::new($Width,$Height);try{$g=[Drawing.Graphics]::FromImage($bitmap);$g.Clear([Drawing.Color]::FromArgb(255,$Rgb[0],$Rgb[1],$Rgb[2]));$g.Dispose();$bitmap.Save($path,[Drawing.Imaging.ImageFormat]::Png)}finally{$bitmap.Dispose()}
 $count=@((Model).state.layers).Count
 Sta {param($path)$image=[Drawing.Image]::FromFile($path);try{[Windows.Forms.Clipboard]::SetImage($image)}finally{$image.Dispose()}} @($path)|Out-Null
 Chord @(0x11,0x10) 0x56
 Wait-Until {@((Model).state.layers).Count -eq $count+1 -and (Model).state.layer_tools.editing_layer.object -and @((Model).state.requests).Count -eq 0} 'Paste did not create a separate Object layer'
}
try{
 Enter-CapyEnvironment
 $env:CAPY_STORAGE_DIR=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1'
 [IO.File]::WriteAllText((Settings-File),(@{language=@{Explicit='en'};theme=$Theme}|ConvertTo-Json -Depth 4))
 $stderr=Join-Path $run 'stderr.log';Start-Review $stderr
 if($MotionPreflight){
  & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'Edit'
  (Control 'menu-image').GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
  Invoke-Id 'canvas_size';Wait-Until {Find 'canvas-size-width'} 'Object diagnostic Canvas Size did not open'
  (Control 'canvas-size-width' -Type ([System.Windows.Automation.ControlType]::Edit)).GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('9504')
  (Control 'canvas-size-height' -Type ([System.Windows.Automation.ControlType]::Edit)).GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('6336')
  Invoke-Id 'canvas-size-anchor-top_left'
  Wait-Until {$draft=(Model).state.layer_tools.canvas_size;$draft.values[0] -eq 9504 -and $draft.values[1] -eq 6336 -and $draft.anchor -eq 'top_left' -and $draft.can_apply} 'Object diagnostic dimensions did not reach Canvas Size'
  Invoke 'Apply' -Name
  Wait-Until {!(Model).state.layer_tools.canvas_size -and (Model).state.tabs[0].width -eq 9504 -and (Model).state.tabs[0].height -eq 6336 -and !(Model).state.document_file.busy} 'Object diagnostic extent did not apply'
  Fit-Canvas
 }
 Save-ProjectAs (Join-Path $run 'Object source.capy')
 Paste-Image 'red' @(220,40,30) 640 480
 $back=(Model).state.layer_tools.editing_layer.id
 if($MotionPreflight){Save-ProjectAs (Join-Path $run 'placement-base.capy')}
 Paste-Image 'blue' @(30,60,220) $(if($MotionPreflight){4096}else{480}) $(if($MotionPreflight){3072}else{360})
 $front=(Model).state.layer_tools.editing_layer.id
 if(@(Rows).Count -ne 2 -or $front -eq $back){throw 'Two source files did not become two ordinary Object layers'}
 foreach($id in @($front,$back)){
  Wait-Until {(Control "layer-$id-thumbnail").Current.ItemStatus -eq 'Ready'} 'Object layer thumbnail did not arrive' 45
  if(Find "layer-$id-expand"){throw 'An Object layer still exposes a child image list'}
 }
 Capture "object-layers-$Device-$Theme" -WithModel
 $checks.rows_and_previews='passed'
 Tap "layer-$front-name"
 Invoke "layer-$back-selection"
 Wait-Until {(Row $front).selected -and (Row $back).selected} 'Ordinary layer selection did not include both Object layers'
 Invoke "layer-$back-selection"
 Wait-Until {(Row $front).selected -and !(Row $back).selected} 'Ordinary layer selection did not remove the second Object layer'
 Tap "layer-$back-visibility";Wait-Until {!(Row $back).visible} 'Layer visibility did not hide its object'
 Assert-ImageHistory 'Undo' {(Row $back).visible}
 $checks.selection_and_visibility='passed'
 $before=Order
 $from=Point "layer-$front-drag";$bounds=(Control "layer-row-$back" -Arranged).Current.BoundingRectangle
 [CapyRowPointer]::Down($Device,$from.x,$from.y);[CapyRowPointer]::Move([int]($bounds.X+$bounds.Width*.5),[int]($bounds.Y+$bounds.Height*.8))
 Wait-Until {(Gesture).phase -eq 'dragging' -and (Gesture).can_drop} 'Ordinary layer grip did not start the object-layer drag'
 [CapyRowPointer]::Key(0x1b);[CapyRowPointer]::Up()
 Wait-Until {(Gesture).phase -eq 'idle'} 'Escape did not cancel the object-layer drag'
 if((Order) -ne $before){throw 'Canceled layer drag changed order'}
 $from=Point "layer-$front-drag";$bounds=(Control "layer-row-$back").Current.BoundingRectangle
 [CapyRowPointer]::Down($Device,$from.x,$from.y);[CapyRowPointer]::Move([int]($bounds.X+$bounds.Width*.5),[int]($bounds.Y+$bounds.Height*.8))
 Wait-Until {(Gesture).phase -eq 'dragging' -and (Gesture).can_drop} 'Layer drag did not reach the second object'
 [CapyRowPointer]::Up();Wait-Until {(Order) -eq "$back,$front"} 'Layer drag did not reorder Object layers'
 Assert-ImageHistory 'Undo' {(Order) -eq $before};Assert-ImageHistory 'Redo' {(Order) -eq "$back,$front"};Assert-ImageHistory 'Undo' {(Order) -eq $before}
 $checks.layer_reorder='passed'
 Tap "layer-$front-name";Wait-Until {(Row $front).selected -and !(Row $back).selected} 'The front Object layer did not select'
 Chord @() 0x4f;Wait-Until {((Model).state.commands|Where-Object id -eq 'move').selected} 'O did not choose Move'
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
 if($MotionPreflight){
  $initialPixels=Artwork-Pixels
  $bounds=$sampleProjection.bounds
  $metadata=[ordered]@{schema=1;scope='Local generated red/blue Object diagnostic; not the reference photograph or tier hardware';project=$authoredSource;project_sha256=(Get-FileHash $authoredSource).Hash;placement_base=(Join-Path $run 'placement-base.capy');placement_base_sha256=(Get-FileHash (Join-Path $run 'placement-base.capy')).Hash;image=(Join-Path $run 'blue.png');image_sha256=(Get-FileHash (Join-Path $run 'blue.png')).Hash;exe_sha256=(Get-FileHash $Executable).Hash;dll_sha256=(Get-FileHash (Join-Path $directory 'layer_windows.dll')).Hash;theme=$Theme;camera=$sampleProjection.camera;camera_readout=(Control 'canvas-camera').Current.Name;object_source_sha256=(Get-FileHash (Join-Path $repo 'crates/layer-ui/src/object_editing.rs')).Hash;ruler_source_sha256=(Get-FileHash (Join-Path $repo 'crates/layer-ui/src/rulers.rs')).Hash;canvas_bounds=@($bounds.X,$bounds.Y,$bounds.Width,$bounds.Height);dpi=[CapyRowPointer]::GetDpiForWindow($drawingWindow);layer_label=(Front-Layer $authored).data.name;source_extent=@(4096,3072);canvas_extent=@(9504,6336);sample_points=$samplePoints;initial_pixels=$initialPixels;raw_visual_review='required';measured=$false}
  Capture 'object-motion-preflight' -WithModel -Composed
  $metadata|ConvertTo-Json -Depth 20|Set-Content (Join-Path $run 'object-motion-preflight.json')
  & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Close|Out-Null
  if(!$review.WaitForExit(15000) -or $review.ExitCode -ne 0){throw 'Object preflight did not close cleanly'}
  return
 }
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
 Context "layer-row-$front";(Menu-Item 'Organize').GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand();Choose 'Duplicate'
 Wait-Until {@(Rows).Count -eq 3 -and @(Rows|Where-Object selected).Count -eq 1 -and !(Row $front).selected} 'Duplicate Layer did not select one new Object layer'
 Park;Wait-Until {(Artwork-Pixels) -eq $originalPixels} 'Duplicating an opaque layer in place changed its composed artwork'
 $duplicatePixels=Wait-StablePixels {Artwork-Pixels}
 $duplicated=Join-Path $run 'image-duplicated.capy';Save-ProjectAs $duplicated
 Assert-ImageCopy $copyManifest (Package $duplicated) 'Duplicate Layer'
 Capture "image-duplicated-$Device-$Theme" -WithModel -Composed
 Assert-ImageHistory 'Undo' {@(Rows).Count -eq 2 -and (Order) -eq $before -and (Artwork-Pixels) -eq $originalPixels}
 Assert-ImageHistory 'Redo' {@(Rows).Count -eq 3 -and (Artwork-Pixels) -eq $duplicatePixels}
 Assert-ImageHistory 'Undo' {@(Rows).Count -eq 2 -and (Order) -eq $before -and (Artwork-Pixels) -eq $originalPixels}
 Tap "layer-$front-name"
 $checks.duplicate_layer='passed'
 Wait-Until {(Row $front).selected -and !(Row $back).selected -and (Order) -eq $before} 'Copy source selection or order changed'
 Sta {[Windows.Forms.Clipboard]::SetText('awaiting image copy')}|Out-Null
 Chord @(0x11) 0x43
 Wait-Until {(Requests) -eq 0 -and (Sta {[Windows.Forms.Clipboard]::ContainsData('art.capycanvas.clip.nonce')})} 'Copy Layer did not publish its private clipboard identity'
 Paste-In-Place
 Wait-Until {(Requests) -eq 0 -and @(Rows).Count -eq 3} 'Pasting the copied Object layer did not add one sibling'
 Park;Wait-Until {(Artwork-Pixels) -eq $originalPixels} 'Paste in Place changed opaque composed artwork'
 $copyPasted=Join-Path $run 'clipboard-pasted.capy';Save-ProjectAs $copyPasted
 Assert-ImageCopy $copyManifest (Package $copyPasted) 'Paste in Place'
 Assert-ImageHistory 'Undo' {@(Rows).Count -eq 2 -and (Order) -eq $before -and (Artwork-Pixels) -eq $originalPixels}
 Assert-ImageHistory 'Redo' {@(Rows).Count -eq 3 -and (Artwork-Pixels) -eq $originalPixels}
 Assert-ImageHistory 'Undo' {@(Rows).Count -eq 2 -and (Order) -eq $before -and (Artwork-Pixels) -eq $originalPixels}
 Tap "layer-$front-name"
 Wait-Until {(Row $front).selected -and !(Row $back).selected} 'The front Object layer did not select before Cut'
 Sta {[Windows.Forms.Clipboard]::SetText('awaiting image cut')}|Out-Null
 Chord @(0x11) 0x58
 Wait-Until {(Requests) -eq 0 -and @(Rows).Count -eq 1 -and (Sta {[Windows.Forms.Clipboard]::ContainsData('art.capycanvas.clip.nonce')})} 'Cut did not copy and remove the selected layer'
 Park;Wait-Until {Opaque-ImageChanged $originalPixels (Artwork-Pixels)} 'Cut did not change the composed image pixels'
 Assert-ImageHistory 'Undo' {@(Rows).Count -eq 2 -and (Order) -eq $before -and (Artwork-Pixels) -eq $originalPixels}
 $checks.structured_layer_clipboard_history='passed'

 Chord @(0x11) 0x41;Wait-Until {(Model).state.layer_tools.has_selection} 'Select All did not create a pixel selection'
 Chord @(0x11) 0x44;Wait-Until {!((Model).state.layer_tools.has_selection)} 'Deselect did not clear the pixel selection'
 Chord @() 0x42;Wait-Until {((Model).state.commands|Where-Object id -eq 'brush').selected} 'B did not remove transform handles before conversion samples'
 Tap "layer-$front-name";Wait-Until {(Model).state.layer_tools.editing_layer.id -eq $front} 'The front Object layer did not activate'
 Park;$imagePixels=Wait-StablePixels {Artwork-Pixels}
 Context "layer-row-$front";Choose 'Rasterize Layer'
 Wait-Until {!(Row $front).object -and @(Rows).Count -eq 1} 'Rasterize Layer did not turn only the selected Object layer into paint'
 Park;$rasterPixels=Wait-StablePixels {Artwork-Pixels};$rasterDifference=Pixel-Difference $imagePixels $rasterPixels
 @{image=$imagePixels;raster=$rasterPixels;difference=$rasterDifference}|ConvertTo-Json|Set-Content (Join-Path $run 'rasterize-pixels.json')
 if($rasterDifference.max_channel_delta -gt 1){throw 'Rasterize Layer changed the composed artwork beyond one display level'}
 Assert-ImageHistory 'Undo' {(Row $front).object -and @(Rows).Count -eq 2 -and (Artwork-Pixels) -eq $imagePixels}
 Assert-ImageHistory 'Redo' {!(Row $front).object -and (Artwork-Pixels) -eq $rasterPixels}
 Context "layer-row-$front";Choose 'Convert to Object Layer'
 Wait-Until {(Row $front).object -and @(Rows).Count -eq 2} 'Convert to Object Layer did not replace paint on the same layer'
 Park;$convertedPixels=Wait-StablePixels {Artwork-Pixels};$conversionDifference=Pixel-Difference $rasterPixels $convertedPixels
 @{raster=$rasterPixels;converted=$convertedPixels;difference=$conversionDifference}|ConvertTo-Json|Set-Content (Join-Path $run 'convert-to-object-pixels.json')
 if($conversionDifference.max_channel_delta -gt 1){throw 'Convert to Object Layer changed the composed artwork beyond one display level'}
 $converted=Join-Path $run 'image-converted.capy';Save-ProjectAs $converted;$conversion=Package $converted
 if(@(Object-Layers $conversion).Count -ne 2 -or @($conversion.objects|Where-Object type -eq 'capy.image-object/1').Count -ne 2 -or @($conversion.objects|Where-Object type -eq 'capy.image/1').Count -ne 2 -or @($conversion.objects|Where-Object type -eq 'capy.object-layer/1').Count){throw 'Conversion did not save one image per ordinary Object layer'}
 $backLayer=@(Object-Layers $authored|Where-Object id -ne (Front-Layer $authored).id)[0]
 $backObject=@($authored.objects|Where-Object id -eq $backLayer.data.content.objects.ref)[0]
 if((Json @($conversion.objects|Where-Object id -in @($backLayer.id,$backObject.id,$backObject.data.image.ref)|Sort-Object id)) -ne (Json @($authored.objects|Where-Object id -in @($backLayer.id,$backObject.id,$backObject.data.image.ref)|Sort-Object id))){throw 'Conversion changed the other Object layer or its immutable source'}
 Capture "image-converted-$Device-$Theme" -WithModel -Composed
 Assert-ImageHistory 'Undo' {!(Row $front).object -and (Artwork-Pixels) -eq $rasterPixels}
 Assert-ImageHistory 'Redo' {(Row $front).object -and (Artwork-Pixels) -eq $convertedPixels}
 Assert-ImageHistory 'Undo' {!(Row $front).object -and (Artwork-Pixels) -eq $rasterPixels}
 Assert-ImageHistory 'Undo' {(Row $front).object -and @(Rows).Count -eq 2 -and (Artwork-Pixels) -eq $imagePixels}
 $checks.layer_conversions_and_pixel_selection='passed'

 $saved=Join-Path $run 'Object layers 日本語.capy';Save-ProjectAs $saved
 $savedIdentity=Identity (Package $saved)
 if($savedIdentity -ne (Identity $authored)){throw 'Clipboard and conversion history changed authored image identities, effective affines or stack order'}
 & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'File';Invoke-Id 'close_document'
 Wait-Until {$review.HasExited} 'Closing the sole edited drawing did not exit' 30 -Closing
 if($review.ExitCode -ne 0){throw 'The edited drawing did not close cleanly'}
 [CapyRowPointer]::Dispose()
 $reopenStderr=Join-Path $run 'reopen-stderr.log';Start-Review $reopenStderr
 Open-Project $saved
 Wait-Until {(Model).state.document_file.location.uri -eq $saved -and !(Model).state.document_file.busy -and @(Rows).Count -eq 2 -and (Model).brush_ready} 'The edited Object layer drawing did not reopen'
 $reopened=Join-Path $run 'Object layers reopened.capy';Save-ProjectAs $reopened
 if((Identity (Package $reopened)) -ne $savedIdentity){throw 'Save/reopen changed image identities, shared sources, effective affines or sibling order'}
 foreach($id in @(Rows|ForEach-Object id)){Wait-Until {(Control "layer-$id-thumbnail").Current.ItemStatus -eq 'Ready'} 'The reopened Object layer thumbnail did not arrive' 45}
 Capture "object-layers-reopened-$Device-$Theme" -WithModel -Composed
 $checks.edited_object_layer_save_reopen='passed'
 [CapyRowPointer]::Dispose()
 & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Close -DiscardUnsaved
 foreach($log in @($stderr,$reopenStderr)){if((Get-Item -LiteralPath $log).Length){throw 'Native image row stderr needs inspection'}}
 $checks.evidence=$run;[PSCustomObject]$checks|ConvertTo-Json
}catch{
 if($imagePixels -and $review -and !$review.HasExited){try{@{before=$imagePixels;current=(Artwork-Pixels)}|ConvertTo-Json|Set-Content (Join-Path $run 'conversion-failure-pixels.json')}catch{}}
 if($review -and !$review.HasExited){try{Capture 'failure' -WithModel}catch{}}
 [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{[CapyRowPointer]::Dispose();Exit-CapyEnvironment}
