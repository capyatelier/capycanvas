function Navigator-LoadTypes {
 Add-Type -AssemblyName System.Drawing,UIAutomationClient,UIAutomationTypes,WindowsBase,System.Windows.Forms
 $references=@();$referenceDirectory=Join-Path $PSHOME 'ref'
 if(Test-Path $referenceDirectory){$references=@(Get-ChildItem (Join-Path $referenceDirectory '*.dll')|ForEach-Object FullName)}
 $windowsBase=[System.Windows.Automation.AutomationElement].GetProperty('Current').PropertyType.GetProperty('BoundingRectangle').PropertyType.Assembly
 $references=@($references|Where-Object {[IO.Path]::GetFileName($_) -ne 'WindowsBase.dll' -or [Reflection.AssemblyName]::GetAssemblyName($_).Version -ge $windowsBase.GetName().Version})
 if(!@($references|Where-Object {[IO.Path]::GetFileName($_) -eq 'WindowsBase.dll'}).Count){$references+=$windowsBase.Location}
 $names=@($references|ForEach-Object {[IO.Path]::GetFileName($_)})
 foreach($type in @([Drawing.Bitmap],[Drawing.Color],[System.Windows.Automation.AutomationElement],[System.Windows.Automation.AutomationProperty],[System.Windows.Rect],[Diagnostics.Stopwatch],[Collections.Generic.HashSet[int]],[Threading.Thread],[Text.Encoding])){
  $assemblies=@($type.Assembly,$type.BaseType.Assembly)+@($type.GetInterfaces()|ForEach-Object Assembly)
  foreach($assembly in @($assemblies|Where-Object {$_ -and $_ -ne [object].Assembly})){
   $name=[IO.Path]::GetFileName($assembly.Location)
   if($name -notin $names){$references+=$assembly.Location;$names+=$name}
  }
 }
 Add-Type -Path (Join-Path $PSScriptRoot 'NavigatorObserver.cs') -ReferencedAssemblies $references
 Add-Type -Path (Join-Path $PSScriptRoot 'NavigatorFrames.cs') -ReferencedAssemblies $references
}
function Navigator-ValidateResults([string]$Name,$Results){
 $seenImage=$false;$bad=[Collections.Generic.List[object]]::new()
 foreach($result in $results){
  if($result.classification -eq 'U'){continue}
  if($result.classification -eq 'I'){$seenImage=$true}
  elseif($result.classification -ne 'A' -or $seenImage -or $Name -in @('retained-column-drag','retained-panel-divider')){$bad.Add($result)}
 }
 if($bad.Count){throw "$Name exposed invalid Navigator samples: $($bad.classification -join ',')"}
 if(!@($results|Where-Object classification -ne 'U').Count){throw "$Name has no classified Navigator samples"}
 if(!@($results|Where-Object classification -eq 'I').Count){throw "$Name never displayed a filled Navigator image"}

}
function Navigator-Reference {
 Wait-Until {
  $ready=Capture 'retained-warm-ready'
  try{
   if(!(Surround-Matches $ready)){return $false}
   $image=Image-Rect
   foreach($fraction in @(.2,.5,.8)){
    $pixel=$ready.GetPixel([int]($image.Left+$image.Width*$fraction),[int]($image.Top+$image.Height*.5))
    if([Math]::Abs($pixel.R-$paperRgb[0]) -gt 3 -or [Math]::Abs($pixel.G-$paperRgb[1]) -gt 3 -or [Math]::Abs($pixel.B-$paperRgb[2]) -gt 3){return $false}
   }
   $true
  }finally{$ready.Dispose()}
 } 'Warm Navigator setup did not display the known paper image'
 $script:navShapeSource=$null
 $workspace=Workspace-Root;$contentBounds=[CapyWindowApi]::ContentRect($drawingWindow)
 if($workspace.Current.ProcessId -ne $review.Id -or $contentBounds.right -le $contentBounds.left -or $contentBounds.bottom -le $contentBounds.top){throw 'Navigator geometry root has no owned native content bounds'}
 $script:navGeometryOrigin=[pscustomobject]@{x=$contentBounds.left;y=$contentBounds.top}
 $overviewElement=Control 'navigator-overview' -Arranged;$overview=$overviewElement.Current.BoundingRectangle;$tabElement=Find 'panel-tab-navigator' -Visible;if(!$tabElement){$tabElement=Control 'drawer-tab-navigator' -Arranged};$tab=$tabElement.Current.BoundingRectangle
 $script:navHeaderOffset=Navigator-HeaderOffset $overview $tab
 $window=[CapyNavigatorCapture+Rect]::new();[CapyNavigatorCapture]::GetWindowRect($drawingWindow,[ref]$window)|Out-Null
 $screen=[System.Windows.Forms.Screen]::FromHandle($drawingWindow).Bounds;$script:navScreen=$screen
 $walker=[System.Windows.Automation.TreeWalker]::RawViewWalker;$frameElement=$walker.GetParent($overviewElement)
 while($frameElement -and $frameElement.Current.AutomationId -notlike 'column-drawer-*' -and $frameElement.Current.AutomationId -notlike 'workspace-group-*'){$frameElement=$walker.GetParent($frameElement)}
 if(!$frameElement){throw 'Navigator native frame ancestry is missing'}
 $footer=(Control 'navigator-rotate_left' -Arranged).Current.BoundingRectangle
 if($frameElement.Current.AutomationId -like 'column-drawer-*'){
  $column=@((Model).layout.collapsed|Where-Object {$_.groups.icons.panel -contains 'navigator'})
  $radii=@($column.corner_radii)
  if($column.Count -ne 1 -or $radii.Count -ne 4 -or @($radii|Where-Object {$_ -le 0 -or $_ -ne $radii[0]}).Count){throw 'Navigator shared surface radius is missing or inconsistent'}
  $script:navSurfaceRadius=[double]$radii[0]
  $headerPath=Join-Path $PSScriptRoot '../UiControls.h';$headerText=[IO.File]::ReadAllText($headerPath)
  $constants=[regex]::Matches($headerText,'constexpr float CornerFit=([0-9.]+)f;')
  if($constants.Count -ne 1){throw 'Navigator native Border corner fit source is missing'}
  $script:navCornerFit=[double]::Parse($constants[0].Groups[1].Value,[Globalization.CultureInfo]::InvariantCulture)
  if($script:navCornerFit -le 0 -or $script:navCornerFit -gt 1){throw 'Navigator native Border corner fit is invalid'}
  $headerHash=(Get-FileHash $headerPath -Algorithm SHA256).Hash
  $buildPath=Join-Path (Split-Path -Parent $Executable) 'fixture-build.json';$build=[IO.File]::ReadAllText($buildPath)|ConvertFrom-Json
  $binaryHeader=$build.inputs.'apps/layer-windows/UiControls.h'
  if(!$binaryHeader -or $binaryHeader -ne $headerHash){throw 'Navigator corner fit source does not match the executable build input'}
  $script:navShapeSource=@{surface_radius=$script:navSurfaceRadius;corner_fit=$script:navCornerFit;header_sha256=$headerHash}
 }
 $script:navDrawerId=if($frameElement.Current.AutomationId -like 'column-drawer-*'){$frameElement.Current.AutomationId}else{$null}
 $frame=$frameElement.Current.BoundingRectangle
 $native=@($overview,$tab,$frame)+@(foreach($id in @('zoom_out','zoom_in','rotate_left','rotate_right','flip_horizontal','flip_vertical')){(Control ('navigator-'+$id) -Arranged).Current.BoundingRectangle})
 $owned=[Drawing.Rectangle]::FromLTRB([Math]::Max($screen.Left,$window.left),[Math]::Max($screen.Top,$window.top),[Math]::Min($screen.Right,$window.right),[Math]::Min($screen.Bottom,$window.bottom))
 $script:navArea=Navigator-CaptureArea $native $owned
 if($script:navArea.Width -le 0 -or $script:navArea.Height -le 0){throw 'Navigator capture is outside the owned display'}
 $full=Capture 'retained-reference';$crop=$full.Clone([Drawing.Rectangle]::new($script:navArea.Left-$window.left,$script:navArea.Top-$window.top,$script:navArea.Width,$script:navArea.Height),$full.PixelFormat)
 $reference=Join-Path $run 'retained-reference-roi.png';$crop.Save($reference);$full.Dispose()
 $script:navReferencePath=$reference
 $script:navReference=@{area=@($script:navArea.Left,$script:navArea.Top,$script:navArea.Width,$script:navArea.Height);origin=@($script:navOrigin.x,$script:navOrigin.y);geometry_origin=@($script:navGeometryOrigin.x,$script:navGeometryOrigin.y);content_bounds=@($contentBounds.left,$contentBounds.top,($contentBounds.right-$contentBounds.left),($contentBounds.bottom-$contentBounds.top));screen=@($script:navScreen.Left,$script:navScreen.Top,$script:navScreen.Width,$script:navScreen.Height);scale=$scale;header_offset=$script:navHeaderOffset;overview=(Navigator-FrameRect $overview);header=(Navigator-FrameRect $tab);frame=(Navigator-FrameRect $frame);footer=(Navigator-FrameRect $footer);shape_source=$script:navShapeSource;drawer_id=$script:navDrawerId;aspect=$script:navAspect;paper_rgb=$paperRgb;background_rgb=(Navigator-Rgb (Model).state.palette.bg);panel_rgb=(Navigator-Rgb (Model).state.palette.panel)}
 if($script:navClassifier){$script:navClassifier.Dispose()}
 $script:navClassifier=[NavigatorFrameClassifier]::new($reference,(Navigator-FrameRect $overview),(Navigator-FrameRect $tab),$scale,$paperRgb,(Navigator-Rgb (Model).state.palette.bg),(Navigator-Rgb (Model).state.palette.panel),(Navigator-FrameRect $footer))
 $geometry=[NavigatorGeometry]::new();$geometry.overview=Navigator-FrameRect $overview;$geometry.header=Navigator-FrameRect $tab;$geometry.footer=Navigator-FrameRect $footer
 try{$settled=$script:navClassifier.ClassifyBitmap($crop,0,@($geometry),$script:navAspect);if($settled.classification -ne 'I'){throw "Navigator settled reference is $($settled.classification), expected I"}}finally{$crop.Dispose()}
}
function Navigator-CaptureArea($Bounds,[Drawing.Rectangle]$Owned){
 $left=($Bounds|Measure-Object Left -Minimum).Minimum;$top=($Bounds|Measure-Object Top -Minimum).Minimum
 $right=($Bounds|Measure-Object Right -Maximum).Maximum;$bottom=($Bounds|Measure-Object Bottom -Maximum).Maximum
 $area=[Drawing.Rectangle]::FromLTRB([int][Math]::Floor($left-$script:navColumnTravel-4*$scale),[int][Math]::Floor($top-$script:navPanelTravel-4*$scale),[int][Math]::Ceiling($right+4*$scale),[int][Math]::Ceiling($bottom+$script:navPanelTravel+4*$scale))
 [Drawing.Rectangle]::Intersect($area,$Owned)
}
function Navigator-HeaderOffset($Overview,$Tab){@(($Overview.Left-$Tab.Left),($Overview.Top-$Tab.Top),$Tab.Width,$Tab.Height)}
function Navigator-FrameRect($Rect){@(([double]$Rect.Left-$script:navArea.Left),([double]$Rect.Top-$script:navArea.Top),[double]$Rect.Width,[double]$Rect.Height)}
function Navigator-Rgb([string]$Color){$c=[Drawing.ColorTranslator]::FromHtml($Color);@([int]$c.R,[int]$c.G,[int]$c.B)}
function Navigator-Collapse {
 $b=(Control 'panel-tab-navigator' -Arranged).Current.BoundingRectangle
 [CapyRowPointer]::RightClick([int]($b.Left+$b.Width/2),[int]($b.Top+$b.Height/2));Invoke 'Collapse column' -Name
 Wait-Until {!(Find 'navigator-overview' -Visible)} 'Navigator did not collapse'
}
function Navigator-Expand {
 $b=(Control 'column-icon-navigator' -Arranged).Current.BoundingRectangle
 [CapyRowPointer]::RightClick([int]($b.Left+$b.Width/2),[int]($b.Top+$b.Height/2));Invoke 'Expand column' -Name
}
function Navigator-DrawerShape($Evidence,$Drawer){
 if(!$Evidence -or $Evidence.id -ne $Drawer.id -or !$Evidence.status -or $Evidence.error -or $Evidence.bounds.Count -ne 4){throw 'Navigator drawer shape metadata is missing'}
 $shape=$Evidence.status|ConvertFrom-Json;$bounds=$shape.placement.bounds;$native=if($Drawer.actual_bounds){$Drawer.actual_bounds}else{$Drawer.bounds}
 foreach($key in @('x','y','width','height')){if([Math]::Abs([double]$bounds.$key-[double]$native.$key) -gt .001){throw 'Navigator drawer shape does not match the native snapshot'}}
 $physical=@(($script:navGeometryOrigin.x+$native.x*$scale),($script:navGeometryOrigin.y+$native.y*$scale),($native.width*$scale),($native.height*$scale))
 for($i=0;$i -lt 4;$i++){if([Math]::Abs($physical[$i]-$Evidence.bounds[$i]) -gt .51){throw 'Navigator drawer UIA bounds do not match the native snapshot'}}
 $flags=@($shape.connection.square_corners)
 if($flags.Count -ne 4 -or @($flags|Where-Object {$_ -isnot [bool]}).Count){throw 'Navigator drawer corner flags are missing'}
 [double[]]@(foreach($flag in $flags){if($flag){0.}else{$script:navSurfaceRadius*$script:navCornerFit*$scale}})
}
function Navigator-Geometry($Snapshot,[bool]$InDrawer,$DrawerEvidence=$null){
 $overview=@($snapshot.elements|Where-Object id -eq 'navigator-overview')[0]
  if(!$overview){return}
  $geometry=[NavigatorGeometry]::new();$geometry.qpc=$snapshot.snapshot_qpc
  $bounds=if($overview.actual_bounds){$overview.actual_bounds}else{$overview.bounds}
  $geometry.overview=@(($script:navGeometryOrigin.x+$bounds.x*$scale-$script:navArea.Left),($script:navGeometryOrigin.y+$bounds.y*$scale-$script:navArea.Top),($bounds.width*$scale),($bounds.height*$scale))
  $action=@($snapshot.elements|Where-Object id -eq 'navigator-rotate_left')[0]
  if($action){$f=if($action.actual_bounds){$action.actual_bounds}else{$action.bounds};$geometry.footer=@(($script:navGeometryOrigin.x+$f.x*$scale-$script:navArea.Left),($script:navGeometryOrigin.y+$f.y*$scale-$script:navArea.Top),($f.width*$scale),($f.height*$scale))}
  $geometry.header=@(($geometry.overview[0]-$script:navHeaderOffset[0]),($geometry.overview[1]-$script:navHeaderOffset[1]),$script:navHeaderOffset[2],$script:navHeaderOffset[3])
  $geometry.clip=@(($script:navScreen.Left-$script:navArea.Left),($script:navScreen.Top-$script:navArea.Top),$script:navScreen.Width,$script:navScreen.Height)
  $geometry.screen=[double[]]$geometry.clip.Clone()
  if($InDrawer){
   $drawerElement=@($snapshot.elements|Where-Object {$_.id -like 'column-drawer-*'})[0]
   if($drawerElement){$geometry.corners=Navigator-DrawerShape $DrawerEvidence $drawerElement;$clip=if($drawerElement.actual_bounds){$drawerElement.actual_bounds}else{$drawerElement.bounds};$geometry.frame=@(($script:navGeometryOrigin.x+$clip.x*$scale-$script:navArea.Left),($script:navGeometryOrigin.y+$clip.y*$scale-$script:navArea.Top),($clip.width*$scale),($clip.height*$scale));$clipLeft=[Math]::Max($script:navScreen.Left,$script:navGeometryOrigin.x+$clip.x*$scale);$clipTop=[Math]::Max($script:navScreen.Top,$script:navGeometryOrigin.y+$clip.y*$scale);$clipRight=[Math]::Min($script:navScreen.Right,$script:navGeometryOrigin.x+($clip.x+$clip.width)*$scale);$clipBottom=[Math]::Min($script:navScreen.Bottom,$script:navGeometryOrigin.y+($clip.y+$clip.height)*$scale);$geometry.clip=@(($clipLeft-$script:navArea.Left),($clipTop-$script:navArea.Top),([Math]::Max(0,$clipRight-$clipLeft)),([Math]::Max(0,$clipBottom-$clipTop)))}
   if(!$drawerElement){throw 'Arranged Navigator has no native drawer frame'}
  }
  $geometry
}
function Navigator-States($Raw,$DrawerRaw,[bool]$InDrawer){
 $states=[Collections.Generic.List[NavigatorGeometry]]::new();$rejected=[Collections.Generic.List[object]]::new()
 for($index=0;$index -lt $Raw.Count;$index++){
  $snapshot=$null
  try{
   $snapshot=$Raw[$index]|ConvertFrom-Json;$geometry=Navigator-Geometry $snapshot $InDrawer $DrawerRaw[$index]
   if(!$geometry){throw 'No arranged Navigator overview'}
   $geometry.source_index=$index;$states.Add($geometry)
  }catch{$rejected.Add(@{source_index=$index;qpc=$snapshot.snapshot_qpc;reason=$_.Exception.Message})}
 }
 @{states=$states.ToArray();rejected=$rejected.ToArray()}
}
function Navigator-Foreground([string]$Hex){
 Invoke 'color-edit';Wait-Until {Find 'edit-color-apply'} 'Witness colour editor did not open'
 Invoke 'edit-color-hex';$entry=Control 'edit-color-hex-entry';$entry.SetFocus();$value=$entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern);$previous=$value.Current.Value;$value.SetValue($Hex)
 [CapyRowPointer]::Key([uint32]$review.Id,0x0D);Invoke 'edit-color-apply';Wait-Until {!(Find 'edit-color-apply')} 'Witness colour editor did not close'
 $expected=[Drawing.ColorTranslator]::FromHtml($Hex)
 Wait-Canvas {$model=Model;$actual=$model.state.colors.foreground.rgba;$expectedRgba=@(($expected.R/255.),($expected.G/255.),($expected.B/255.),($expected.A/255.));$model.brush_ready -and @(0..3|Where-Object {[Math]::Abs($actual[$_]-$expectedRgba[$_]) -gt .002}).Count -eq 0} 'Witness foreground did not commit'
 $previous
}
function Navigator-WitnessSize([double]$Size){
 (Control 'tool-setting-size' -Arranged).GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($Size.ToString('R',[Globalization.CultureInfo]::InvariantCulture))
 (Control 'drawing-canvas' -Arranged).SetFocus()
 Wait-Canvas {$model=Model;[Math]::Abs($model.state.brush.diameter-$Size) -lt .001 -and $model.brush_ready} 'Witness brush size did not commit'
}
function Navigator-MainAuthored($Pose,$Document,[bool]$Expected,[string]$Name){
 $window=[CapyNavigatorCapture+Rect]::new();[CapyNavigatorCapture]::GetWindowRect($drawingWindow,[ref]$window)|Out-Null
 $projection=[pscustomobject]@{classification='I';image=@(($script:navOrigin.x-$window.left+$Pose.translation[0]),($script:navOrigin.y-$window.top+$Pose.translation[1]),($Document.width*$Pose.zoom),($Document.height*$Pose.zoom));visible=$null}
 Wait-Until {
  if(Find 'navigator-overview' -Visible){throw 'Main authored witness requires a detached Navigator'}
  $bitmap=Capture $Name
  try{
   $first=Join-Path $run ($Name+'-first.png');if(!(Test-Path $first)){$bitmap.Save($first)}
   $projection.visible=@(0.,0.,[double]$bitmap.Width,[double]$bitmap.Height)
   $result=Navigator-AuthoredFrame $null $bitmap $projection $Expected 0
   $result -and $result.valid
  }finally{$bitmap.Dispose()}
 } 'Detached authored colour did not reach the registered main canvas region'
}
function Navigator-DetachedStroke {
 if(Find 'navigator-overview' -Visible){throw 'Authored update requires a detached Navigator'}
 $original=(Model).state.brush;$originalBrush=$original|ConvertTo-Json -Depth 5 -Compress
 Invoke (Tool-Tile 'pen')
 Wait-Canvas {$model=Model;@($model.state.commands|Where-Object {$_.id -eq 'pen' -and $_.selected}).Count -eq 1 -and $model.brush_ready} 'Opaque Pen did not become ready'
 $penSize=(Model).state.brush.diameter;Navigator-WitnessSize 4
 Fit-Canvas;$pose=(Model).state.camera;$document=(Model).state.tabs[0];$revision=(Model).state.document_file.revision
 Navigator-MainAuthored $pose $document $false ("retained-detached-before-$revision")
 $x=[int]($script:navOrigin.x+$pose.translation[0]+$document.width*$pose.zoom*.05);$y=[int]($script:navOrigin.y+$pose.translation[1]+$document.height*$pose.zoom*.10)
 [CapyRowPointer]::Down('mouse',$x,$y)
 try{for($i=1;$i -le 12;$i++){[CapyRowPointer]::Move([int]($x+$document.width*$pose.zoom*.12*$i/12),$y)}}finally{[CapyRowPointer]::Up()}
 Wait-Canvas {$view=Model;$view.state.document_file.revision -ne $revision -and $view.brush_ready} 'Detached authored stroke did not commit'
 Navigator-MainAuthored $pose $document $true ("retained-detached-ready-$revision")
 Navigator-WitnessSize $penSize
 if($original.tool -ne (Model).state.brush.tool){Invoke (Tool-Tile $original.tool)}
 Wait-Canvas {$model=Model;($model.state.brush|ConvertTo-Json -Depth 5 -Compress) -ceq $originalBrush -and $model.brush_ready} 'Authored witness did not restore the original brush'
 for($i=0;$i -lt 14;$i++){& (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'View';Invoke 'zoom_out'}
}
function Navigator-DetachedUndo {
 if(Find 'navigator-overview' -Visible){throw 'Authored Undo requires a detached Navigator'}
 $revision=(Model).state.document_file.revision;Invoke 'Undo' -Name
 Wait-Canvas {(Model).state.document_file.revision -ne $revision} 'Detached authored Undo did not commit'
}
function Navigator-MarkRegion($Image){@(($Image[0]+$Image[2]*.025),($Image[1]+$Image[3]*.04),($Image[0]+$Image[2]*.20),($Image[1]+$Image[3]*.17))}
function Navigator-AuthoredFrame($Previous,$Bitmap,$Result,[bool]$Expected,[int]$Index){
 if($Previous){return $Previous}
 if($Result.classification -ne 'I'){return}
 $region=Navigator-MarkRegion $Result.image;$clip=$Result.visible
 if($region[0] -lt $clip[0] -or $region[1] -lt $clip[1] -or $region[2] -gt ($clip[0]+$clip[2]) -or $region[3] -gt ($clip[1]+$clip[3])){return}
 $shape=[NavigatorGeometry]::new();$shape.frame=$Result.frame;$shape.corners=$Result.corners
 foreach($x in @($region[0],$region[2])){foreach($y in @($region[1],$region[3])){if(![NavigatorFrameClassifier]::FrameInterior($shape,$x,$y,3.)){return}}}
 $count=0
 for($y=[int][Math]::Ceiling($region[1]);$y -lt [Math]::Floor($region[3]);$y++){for($x=[int][Math]::Ceiling($region[0]);$x -lt [Math]::Floor($region[2]);$x++){
  $pixel=$Bitmap.GetPixel($x,$y)
  if(($pixel.R-$pixel.G)-($paperRgb[0]-$paperRgb[1]) -ge 8 -and ($pixel.B-$pixel.G)-($paperRgb[2]-$paperRgb[1]) -ge 8){$count++}
 }}
 @{frame=$Index;expected_mark=$Expected;magenta_pixels=$count;valid=if($Expected){$count -ge 4}else{$count -eq 0};image=$Result.image;visible=$clip;visibility='Selected native image and actual frame clip geometry only'}
}
function Navigator-TestAuthoredContent {
 $paperRgb=@(244,241,232);$bitmap=[Drawing.Bitmap]::new(100,60);$graphics=[Drawing.Graphics]::FromImage($bitmap)
 $visible=[pscustomobject]@{classification='I';image=@(10.,10.,80.,40.);visible=@(10.,10.,80.,40.)}
 $partial=[pscustomobject]@{classification='I';image=$visible.image;visible=@(20.,10.,70.,40.)}
 try{
  foreach($color in @([Drawing.Color]::FromArgb(244,241,232),[Drawing.Color]::FromArgb(247,238,235),[Drawing.Color]::FromArgb(160,160,160),[Drawing.Color]::FromArgb(20,20,20))){
   $graphics.Clear($color);$absent=Navigator-AuthoredFrame $null $bitmap $visible $false 3
   if(!$absent.valid){throw 'Paper tolerance or neutral ink became an authored colour witness'}
  }
  $graphics.Clear([Drawing.Color]::FromArgb(244,241,232));$stale=Navigator-AuthoredFrame $null $bitmap $visible $true 3
  $brush=[Drawing.SolidBrush]::new([Drawing.Color]::FromArgb(246,205,235))
  try{$graphics.FillRectangle($brush,14,13,8,3)}finally{$brush.Dispose()}
  $present=Navigator-AuthoredFrame $null $bitmap $visible $true 4
  $removed=Navigator-AuthoredFrame $null $bitmap $visible $false 4
  if(!$present.valid -or $removed.valid){throw 'Faint authored pigment was not distinguished from its removal'}
  if($null -ne (Navigator-AuthoredFrame $null $bitmap $partial $true 3)){throw 'Partially clipped authored region became eligible'}
  $rounded=[pscustomobject]@{classification='I';image=$visible.image;visible=$visible.visible;frame=@(7.,7.,83.,43.);corners=@(0.,0.,0.,0.)}
  if(!(Navigator-AuthoredFrame $null $bitmap $rounded $true 3)){throw 'Square-corner authored region was incorrectly clipped'}
  $rounded.corners=@(14.58,0.,0.,0.)
  if($null -ne (Navigator-AuthoredFrame $null $bitmap $rounded $true 3)){throw 'Rounded-corner clipped authored region became eligible'}
  $first=Navigator-AuthoredFrame $stale $bitmap $visible $true 4
  if($first.valid -or $first.frame -ne 3){throw 'A later authored image replaced the stale first eligible frame'}
 }finally{$graphics.Dispose();$bitmap.Dispose()}
}
function Navigator-QualifyArtwork($Classifier,$Bitmap,[long]$Qpc,$States,[double]$Aspect,[Nullable[bool]]$AuthoredMark){
 $Classifier.ClearArtwork()
 $endpoint=$Classifier.ClassifyBitmap($Bitmap,$Qpc,$States,$Aspect)
 if($endpoint.classification -ne 'I'){throw 'Final captured artwork did not pass the original filled-image gate'}
 if($null -ne $AuthoredMark){
  $content=Navigator-AuthoredFrame $null $Bitmap $endpoint ([bool]$AuthoredMark) 0
  if(!$content -or !$content.valid){throw 'Final captured artwork did not contain the expected authored state'}
 }
 $Classifier.SetArtwork($Bitmap,$endpoint)
 $endpoint
}
function Navigator-Observe([string]$Name,[scriptblock]$Command,[Nullable[bool]]$AuthoredMark=$null){
 $folder=Join-Path $run $Name;[IO.Directory]::CreateDirectory($folder)|Out-Null
 [IO.File]::Copy($script:navReferencePath,(Join-Path $folder 'reference.png'))
 $observer=[NavigatorObserver]::new((Workspace-Root),$(if($Name -like 'retained-drawer*'){$script:navDrawerId}else{$null}));$observer.Start();$sample=$null
 $observerError=$null
 try{$sample=Sample-Frames $script:navArea $Command}finally{try{$observer.Dispose()}catch{$observerError=($_|Out-String)}}
 $metadata=@{reference=$script:navReference;command_frame=3;command_qpc=$sample.times[3].action;frequency=[Diagnostics.Stopwatch]::Frequency;times=$sample.times;reads=$observer.Reads();raw=$observer.Raw();drawer_raw=$observer.DrawerRaw();observer_overflow=$observer.Overflow;observer_error=$observerError;capture='GDI samples after DwmFlush; does not establish complete 120 Hz presentation coverage'}
 if($Name -like 'retained-drawer*'){$metadata.closed_context=$script:navClosedContext}
 $metadata|ConvertTo-Json -Depth 12|Set-Content (Join-Path $folder 'metadata.json')
 if($sample){
  for($index=0;$index -lt $sample.frames.Count;$index++){$sample.frames[$index].Save((Join-Path $folder ('frame-{0:D3}.png' -f $index)))}
 }
 if($observerError){foreach($image in $sample.frames){$image.Dispose()};throw $observerError}
 if($sample.times[0].begin -ge $sample.times[3].action){foreach($image in $sample.frames){$image.Dispose()};throw 'Navigator capture did not start before input'}
 $qualified=Navigator-States $observer.Raw() $observer.DrawerRaw() ($Name -like 'retained-drawer*');$states=$qualified.states
 $metadata.rejected_states=$qualified.rejected
 $metadata|ConvertTo-Json -Depth 12|Set-Content (Join-Path $folder 'metadata.json')
 $results=[Collections.Generic.List[object]]::new();$contentCheck=$null
 try{
  $endpoint=Navigator-QualifyArtwork $script:navClassifier $sample.frames[-1] ([long]$sample.times[-1].begin) $states $script:navAspect $AuthoredMark
  @{frame=($sample.frames.Count-1);result=$endpoint;authored_expected=$AuthoredMark}|ConvertTo-Json -Depth 8|Set-Content (Join-Path $folder 'artwork-endpoint.json')
  for($index=0;$index -lt $sample.frames.Count;$index++){
   if($index -ge 3){
    $result=$script:navClassifier.ClassifyBitmap($sample.frames[$index],[long]$sample.times[$index].begin,$states,$script:navAspect);$results.Add($result)
    if($null -ne $AuthoredMark){$contentCheck=Navigator-AuthoredFrame $contentCheck $sample.frames[$index] $result ([bool]$AuthoredMark) $index}
   }
  }
 }finally{$script:navClassifier.ClearArtwork();foreach($image in $sample.frames){$image.Dispose()}}
 $results.ToArray()|ConvertTo-Json -Depth 8|Set-Content (Join-Path $folder 'classes.json')
 $metadata.classified_count=@($results|Where-Object classification -ne 'U').Count;$metadata.unclassified=@($results|Where-Object classification -eq 'U'|ForEach-Object qpc);$metadata.unclassified_count=$results.Count-$metadata.classified_count
 $metadata|ConvertTo-Json -Depth 12|Set-Content (Join-Path $folder 'metadata.json')
 if(($sample.times[-1].end-$sample.times[3].action)*1000/[Diagnostics.Stopwatch]::Frequency -lt 500){throw 'Navigator observation did not cover 500 ms after input'}
 Navigator-ValidateResults $Name $results
 if($null -ne $AuthoredMark){
  if(!$contentCheck){throw 'Authored mark region was never fully displayed'}
  $contentCheck|ConvertTo-Json -Depth 5|Set-Content (Join-Path $folder 'authored-content.json')
  if(!$contentCheck.valid){throw 'First displayed authored region contains stale Navigator content'}
 }

}
function Navigator-TestGeometry {
 $script:navArea=[Drawing.Rectangle]::new(100,200,400,300);$script:navOrigin=[pscustomobject]@{x=-500;y=-600};$script:navGeometryOrigin=[pscustomobject]@{x=100;y=200};$script:navScreen=[Drawing.Rectangle]::new(0,0,1920,1080)
 $script:navHeaderOffset=Navigator-HeaderOffset ([Drawing.RectangleF]::new(42.5,64.25,100.5,80.75)) ([Drawing.RectangleF]::new(30,40,32,16))
 $relative=@(Navigator-FrameRect ([Drawing.RectangleF]::new(112.5,225.25,100.5,80.75)))
 if(($script:navHeaderOffset -join ',') -ne '12.5,24.25,32,16' -or ($relative -join ',') -ne '12.5,25.25,100.5,80.75'){throw 'Navigator relative bounds did not produce four scalar coordinates'}
 $scale=1.5;$script:navColumnTravel=150;$script:navPanelTravel=60
 $frame=[Drawing.RectangleF]::new(1104,73,363,293);$overview=[Drawing.RectangleF]::new(1116,139,339,170);$header=[Drawing.RectangleF]::new(1104,73,162,54);$footer=[Drawing.RectangleF]::new(1116,313,339,42)
 $envelope=Navigator-CaptureArea @($overview,$header,$footer,$frame) ([Drawing.Rectangle]::new(0,0,1920,1080))
 if($envelope.Left -ne 948 -or $envelope.Top -ne 7 -or $envelope.Right -ne 1473 -or $envelope.Bottom -ne 432){throw 'Navigator capture omits the outer drawer frame beyond the settled overview'}
 $snapshot=[pscustomobject]@{snapshot_qpc=123;elements=@([pscustomobject]@{id='navigator-overview';actual_bounds=[pscustomobject]@{x=40;y=60;width=100;height=80}},[pscustomobject]@{id='column-drawer-1';actual_bounds=[pscustomobject]@{x=-100;y=-150;width=300;height=300}})}
 foreach($case in @(@{scale=1.;overview='40,60,100,80';header='27.5,35.75,32,16';clip='-100,-150,300,300'},@{scale=1.5;overview='60,90,150,120';header='47.5,65.75,32,16';clip='-100,-200,400,425'},@{scale=2.;overview='80,120,200,160';header='67.5,95.75,32,16';clip='-100,-200,500,500'})){
  $scale=$case.scale;$normal=Navigator-Geometry $snapshot $false;$script:navSurfaceRadius=18.;$script:navCornerFit=.54
  $shape=[pscustomobject]@{id='column-drawer-1';status='{"placement":{"bounds":{"x":-100,"y":-150,"width":300,"height":300}},"connection":{"square_corners":[false,true,false,true]}}';bounds=@((100-100*$scale),(200-150*$scale),(300*$scale),(300*$scale))}
  $drawer=Navigator-Geometry $snapshot $true $shape
  if(($normal.overview -join ',') -ne $case.overview -or ($normal.header -join ',') -ne $case.header -or ($normal.clip -join ',') -ne '-100,-200,1920,1080' -or ($drawer.clip -join ',') -ne $case.clip){throw "Navigator native geometry differs at scale $scale"}
  if($script:navOrigin.x -ne -500 -or $script:navOrigin.y -ne -600){throw 'Native geometry changed the camera input origin'}
  foreach($vector in @($normal.overview,$normal.header,$normal.clip,$drawer.clip)){
   if($vector.Count -ne 4 -or @($vector|Where-Object {$_ -isnot [double]}).Count){throw 'Navigator native geometry is not four scalar doubles'}
  }
 }
 foreach($invalid in @($null,[pscustomobject]@{id=$shape.id;bounds=$shape.bounds},[pscustomobject]@{id=$shape.id;status=$shape.status;bounds=@(0.,0.,600.,600.)},[pscustomobject]@{id=$shape.id;status=$shape.status.Replace('false,true,false,true','false,true,false');bounds=$shape.bounds})){
  $rejected=$false;try{$null=Navigator-Geometry $snapshot $true $invalid}catch{$rejected=$true}
  if(!$rejected){throw 'Missing or mismatched actual drawer shape evidence was accepted'}
 }
}
function Navigator-TestFooter($Graphics,[int]$Top){
 $Graphics.FillRectangle([Drawing.Brushes]::Gold,94,($Top+2),11,3)
 $Graphics.FillRectangle([Drawing.Brushes]::Gold,94,($Top+2),3,12)
 $Graphics.FillRectangle([Drawing.Brushes]::Gold,94,($Top+11),10,3)
}
function Navigator-TestHorizontal($Classifier,$Reference){
 $left=[NavigatorGeometry]::new();$left.qpc=1;$left.overview=@(60,34,100,80);$left.header=@(60,10,100,24);$left.footer=@(60,100,100,20);$left.screen=@(0,0,150,180);$left.frame=@(60,10,130,87);$left.clip=@(60,10,90,87)
 $right=[NavigatorGeometry]::new();$right.qpc=999;$right.overview=@(80,34,100,80);$right.header=@(80,10,100,24);$right.footer=@(80,100,100,20);$right.screen=@(0,0,150,180);$right.frame=@(80,10,110,67);$right.clip=@(80,10,70,67)
 $moving=[Drawing.Bitmap]::new(200,180);$graphics=[Drawing.Graphics]::FromImage($moving)
 try{$graphics.Clear([Drawing.Color]::FromArgb(20,20,20));$graphics.SetClip([Drawing.Rectangle]::new(70,10,80,77));$graphics.DrawImageUnscaled($Reference,20,0)}finally{$graphics.Dispose()}
 $hole=$moving.Clone();$graphics=[Drawing.Graphics]::FromImage($hole);$brush=[Drawing.SolidBrush]::new([Drawing.Color]::FromArgb(20,20,20))
 try{$graphics.FillRectangle($brush,100,71,10,5)}finally{$graphics.Dispose();$brush.Dispose()}
 $mixed=$moving.Clone();$graphics=[Drawing.Graphics]::FromImage($mixed)
 try{$graphics.FillRectangle([Drawing.Brushes]::DimGray,80,60,10,8)}finally{$graphics.Dispose()}
 try{
  $filled=$Classifier.ClassifyBitmap($moving,1000,@($left,$right),2.);$canvas=$Classifier.ClassifyBitmap($hole,1000,@($left,$right),2.);$partial=$Classifier.ClassifyBitmap($mixed,1000,@($left,$right),2.)
  if($filled.classification -ne 'I' -or $canvas.classification -ne 'C' -or $partial.classification -ne 'M' -or [Math]::Abs($filled.visible[2]-76) -gt .01 -or [Math]::Abs($filled.visible[3]-36) -gt .01){throw 'Native horizontal bracket did not preserve displayed clip, canvas holes and mixed images'}
  $rejected=$Classifier.ClassifyBitmap($moving,1000,@($left),2.).classification -eq 'U'
  if(!$rejected){throw 'An unbracketed horizontal position was accepted'}
  $left.source_index=3;$right.source_index=5
  $rejected=$Classifier.ClassifyBitmap($moving,1000,@($left,$right),2.).classification -eq 'U'
  if(!$rejected){throw 'Horizontal interpolation crossed a rejected native state'}
  $right.source_index=4
  if($Classifier.ClassifyBitmap($moving,1000,@($left,$right),2.).classification -ne 'I'){throw 'Adjacent qualified native states did not bracket displayed geometry'}
  $left.source_index=-1;$right.source_index=-1
  $changed=[NavigatorGeometry]::new();$changed.qpc=1001;$changed.overview=@(60,34,100,80);$changed.header=@(60,10,100,24);$changed.footer=@(60,100,100,20);$changed.screen=@(0,0,150,180);$changed.frame=@(60,10,130,107);$changed.clip=@(60,10,90,107)
  $rejected=$Classifier.ClassifyBitmap($moving,1000,@($left,$right,$changed),2.).classification -eq 'U'
  if(!$rejected){throw 'Ambiguous horizontal phase brackets were accepted'}
 }finally{$moving.Dispose();$hole.Dispose();$mixed.Dispose()}
}
function Navigator-TestStates($Geometry){
 foreach($stamp in @(0,2)){
  $copy=[NavigatorGeometry]::new();foreach($key in @('overview','header','footer','clip','frame','screen','corners')){$copy.$key=$Geometry.$key}
  $copy.qpc=$stamp;$copy.source_index=$stamp/2;$copy
 }
}
function Navigator-TestRounded($Classifier,$Ink,$Partial){
 $shape=[NavigatorGeometry]::new();$shape.overview=$Partial.overview;$shape.header=$Partial.header;$shape.footer=$Partial.footer;$shape.clip=$Partial.clip;$shape.frame=$Partial.clip;$shape.corners=@(0.,0.,14.58,0.)
 $curved=$Ink.Clone();$unknown=$null
 try{
  $excluded=0
  for($y=54;$y -lt 60;$y++){for($x=57;$x -lt 97;$x++){
   if(![NavigatorFrameClassifier]::FrameInterior($shape,($x+.5),($y+.5),3.)){$curved.SetPixel($x,$y,[Drawing.Color]::FromArgb(169,166,161));$excluded++}
  }}
  if($excluded -eq 0 -or $Classifier.ClassifyBitmap($curved,1,(Navigator-TestStates $shape),2.).classification -ne 'I'){throw 'Actual rounded-frame boundary was classified as missing artwork'}
  $shape.corners=@(0.,0.,0.,0.)
  if($Classifier.ClassifyBitmap($curved,1,(Navigator-TestStates $shape),2.).classification -ne 'P'){throw 'Square frame incorrectly excluded the rounded boundary defect'}
  $shape.corners=@(0.,0.,14.58,0.);$unknown=$curved.Clone();$unknown.SetPixel(85,55,[Drawing.Color]::Blue)
  $bad=$Classifier.ClassifyBitmap($unknown,1,(Navigator-TestStates $shape),2.)
  if($bad.classification -ne 'P'){throw 'Rounded frame accepted an unmatched interior artwork pixel'}
  $rejected=$false;try{Navigator-ValidateResults 'synthetic-open' @($bad,$Classifier.ClassifyBitmap($curved,1,(Navigator-TestStates $shape),2.))}catch{$rejected=$true}
  if(!$rejected){throw 'Rounded frame accepted a stale first image followed by a correct image'}
  $small=[NavigatorGeometry]::new();$small.frame=@(0.,0.,10.,8.);$small.corners=@(999.,999.,999.,999.)
  $clamped=[NavigatorGeometry]::new();$clamped.frame=$small.frame;$clamped.corners=@(4.,4.,4.,4.)
  for($y=0;$y -lt 8;$y++){for($x=0;$x -lt 10;$x++){if([NavigatorFrameClassifier]::FrameInterior($small,($x+.5),($y+.5),0.) -ne [NavigatorFrameClassifier]::FrameInterior($clamped,($x+.5),($y+.5),0.)){throw 'Rounded frame radius did not clamp to half its dimensions'}}}
 }finally{$curved.Dispose();if($unknown){$unknown.Dispose()}}
}
function Navigator-TestArtwork($Classifier,$Reference,$Geometry,$Placeholder,$Exposure){
 $paperRgb=@(255,255,255);$marked=$Reference.Clone();$graphics=[Drawing.Graphics]::FromImage($marked)
 try{$graphics.FillRectangle([Drawing.Brushes]::Magenta,54,51,50,28)}finally{$graphics.Dispose()}
 $partial=[NavigatorGeometry]::new();$partial.qpc=1;$partial.overview=$Geometry.overview;$partial.header=$Geometry.header;$partial.footer=$Geometry.footer;$partial.clip=@(50,10,50,53)
 $ink=[Drawing.Bitmap]::new(200,180);$floating=[Drawing.Bitmap]::new(200,180);$unknown=$Reference.Clone();$canvas=$Reference.Clone()
 foreach($image in @($ink,$floating)){
  $graphics=[Drawing.Graphics]::FromImage($image)
  try{$graphics.Clear([Drawing.Color]::FromArgb(20,20,20));$graphics.SetClip([Drawing.Rectangle]::new(50,10,50,53));if($image -eq $ink){$graphics.DrawImageUnscaled($marked,0,0)}else{$graphics.DrawImage($marked,[Drawing.Rectangle]::new(54,51,46,12),[Drawing.Rectangle]::new(54,51,46,12),[Drawing.GraphicsUnit]::Pixel)}}finally{$graphics.Dispose()}
 }
 $unmatchedInk=$ink.Clone();$unmatchedInk.SetPixel(90,55,[Drawing.Color]::Blue)
 $mixed=$marked.Clone();$graphics=[Drawing.Graphics]::FromImage($mixed);try{$graphics.FillRectangle([Drawing.Brushes]::DimGray,120,60,10,8)}finally{$graphics.Dispose()}
 $graphics=[Drawing.Graphics]::FromImage($unknown);try{$graphics.FillRectangle([Drawing.Brushes]::Blue,54,51,92,46)}finally{$graphics.Dispose()}
 $nearEndpoint=$Reference.Clone();$nearObserved=$Reference.Clone()
 foreach($case in @(@{image=$nearEndpoint;channel=253},@{image=$nearObserved;channel=250})){$graphics=[Drawing.Graphics]::FromImage($case.image);$brush=[Drawing.SolidBrush]::new([Drawing.Color]::FromArgb($case.channel,$case.channel,$case.channel));try{$graphics.FillRectangle($brush,54,51,92,46)}finally{$graphics.Dispose();$brush.Dispose()}}
 $graphics=[Drawing.Graphics]::FromImage($canvas);$brush=[Drawing.SolidBrush]::new([Drawing.Color]::FromArgb(20,20,20));try{$graphics.FillRectangle($brush,54,51,92,46)}finally{$graphics.Dispose();$brush.Dispose()}
 try{
  foreach($case in @(@{image=$Placeholder;states=@($Geometry);expected=$null},@{image=$Reference;states=@($partial);expected=$null},@{image=$Reference;states=@($Geometry);expected=$true},@{image=$marked;states=@($Geometry);expected=$false})){
   $rejected=$false;try{$null=Navigator-QualifyArtwork $Classifier $case.image 1 $case.states 2. $case.expected}catch{$rejected=$true}
   if(!$rejected){throw 'An unfilled, clipped or authored-stale endpoint became an artwork oracle'}
  }
  $null=Navigator-QualifyArtwork $Classifier $marked 1 @($Geometry) 2. $true
  $valid=$Classifier.ClassifyBitmap($ink,1,(Navigator-TestStates $partial),2.)
  Navigator-TestRounded $Classifier $ink $partial
  $rounded=[NavigatorGeometry]::new();$rounded.overview=$Geometry.overview;$rounded.header=$Geometry.header;$rounded.footer=$Geometry.footer;$rounded.frame=@(50.,10.,100.,130.);$rounded.corners=@(14.58,14.58,14.58,14.58)
  foreach($case in @(@{image=$Placeholder;kind='P'},@{image=$Exposure;kind='C'},@{image=$mixed;kind='M'})){
   if($Classifier.ClassifyBitmap($case.image,1,@($rounded),2.).classification -ne $case.kind){throw "Rounded frame changed the interior $($case.kind) negative"}
  }
  if($valid.classification -ne 'I' -or $valid.paper -ne 0){throw 'A fully registered ink-only clip did not display as a filled image'}
  foreach($case in @(@{image=$Placeholder;kind='P'},@{image=$Exposure;kind='C'},@{image=$mixed;kind='M'},@{image=$unknown;kind='P'},@{image=$canvas;kind='C'},@{image=$floating;kind='image-before-panel'},@{image=$unmatchedInk;kind='P'})){
   $states=if($case.image -eq $floating -or $case.image -eq $unmatchedInk){Navigator-TestStates $partial}else{@($Geometry)};$result=$Classifier.ClassifyBitmap($case.image,1,$states,2.)
   if($result.classification -ne $case.kind){throw "Artwork evidence changed the $($case.kind) negative"}
   $rejected=$false;try{Navigator-ValidateResults 'synthetic-open' @($result,$valid)}catch{$rejected=$true};if(!$rejected){throw 'Artwork evidence accepted an invalid first frame followed by a correct image'}
  }
  $old=$Classifier.ClassifyBitmap($Reference,1,@($Geometry),2.);$first=Navigator-AuthoredFrame $null $Reference $old $true 3
  $first=Navigator-AuthoredFrame $first $ink $valid $true 4
  if(!$first -or $first.valid -or $first.frame -ne 3){throw 'An authored-correct endpoint replaced the stale first eligible image'}
  $null=Navigator-QualifyArtwork $Classifier $nearEndpoint 1 @($Geometry) 2. $false
  if($Classifier.ClassifyBitmap($nearObserved,1,@($Geometry),2.).classification -ne 'P'){throw 'Near-paper endpoint colours expanded the original paper tolerance'}
 }finally{$Classifier.ClearArtwork();foreach($image in @($marked,$ink,$floating,$unknown,$canvas,$mixed,$unmatchedInk,$nearEndpoint,$nearObserved)){$image.Dispose()}}
}
function Navigator-TestClassifier {
 Navigator-TestAuthoredContent
 Navigator-TestGeometry
 $path=Join-Path $run 'classifier-reference.png';$reference=[Drawing.Bitmap]::new(200,180);$graphics=[Drawing.Graphics]::FromImage($reference)
 try{
  $graphics.Clear([Drawing.Color]::FromArgb(20,20,20));$graphics.FillRectangle([Drawing.Brushes]::DimGray,50,10,100,110)
  Navigator-TestFooter $graphics 102
  $graphics.DrawString('Navigator',[Drawing.SystemFonts]::DefaultFont,[Drawing.Brushes]::Gold,58,16)
  $graphics.FillRectangle([Drawing.Brushes]::White,54,51,92,46);$graphics.FillRectangle([Drawing.Brushes]::Red,64,56,10,5)
 }finally{$graphics.Dispose()}
 $reference.Save($path);$classifier=[NavigatorFrameClassifier]::new($path,@(50,34,100,80),@(50,10,100,24),1.,@(255,255,255),@(20,20,20),@(105,105,105),@(50,100,100,20))
 $geometry=[NavigatorGeometry]::new();$geometry.qpc=1;$geometry.overview=@(50,34,100,80);$geometry.header=@(50,10,100,24);$geometry.footer=@(50,100,100,20)
 $placeholder=$reference.Clone();$graphics=[Drawing.Graphics]::FromImage($placeholder)
 try{$graphics.FillRectangle([Drawing.Brushes]::DimGray,54,51,92,46)}finally{$graphics.Dispose()}
 $floating=[Drawing.Bitmap]::new(200,180);$graphics=[Drawing.Graphics]::FromImage($floating)
 try{$graphics.Clear([Drawing.Color]::FromArgb(20,20,20));$graphics.DrawImage($reference,[Drawing.Rectangle]::new(54,51,92,46),[Drawing.Rectangle]::new(54,51,92,46),[Drawing.GraphicsUnit]::Pixel)}finally{$graphics.Dispose()}
 $exposure=$reference.Clone();$graphics=[Drawing.Graphics]::FromImage($exposure)
 $exposureBrush=[Drawing.SolidBrush]::new([Drawing.Color]::FromArgb(20,20,20))
 try{$graphics.FillRectangle($exposureBrush,90,71,20,5)}finally{$graphics.Dispose();$exposureBrush.Dispose()}
 $future=[NavigatorGeometry]::new();$future.qpc=999;$future.overview=@(50,34,100,100);$future.header=@(50,10,100,24);$future.footer=@(50,120,100,20)
 $mismatch=$reference.Clone();$graphics=[Drawing.Graphics]::FromImage($mismatch)
 try{$graphics.FillRectangle([Drawing.Brushes]::DimGray,50,100,100,40);Navigator-TestFooter $graphics 122}finally{$graphics.Dispose()}
 $missing=[NavigatorGeometry]::new();$missing.qpc=1;$missing.overview=@(50,34,100,80);$missing.header=@(50,10,100,24)
 $unmatched=$reference.Clone();$graphics=[Drawing.Graphics]::FromImage($unmatched)
 try{$graphics.FillRectangle([Drawing.Brushes]::DimGray,50,100,100,40)}finally{$graphics.Dispose()}
 try{
  if($classifier.ClassifyBitmap($mismatch,1000,@($geometry),2.).classification -ne 'U'){throw 'A single stale native size was judged without its matching action-row geometry'}
  Navigator-TestArtwork $classifier $reference $geometry $placeholder $exposure
  Navigator-TestHorizontal $classifier $reference
  $incomplete=[pscustomobject]@{snapshot_qpc=1;elements=@([pscustomobject]@{id='navigator-overview';bounds=@{x=50;y=34;width=100;height=80}})}|ConvertTo-Json -Depth 8 -Compress
  $unqualified=Navigator-States @($incomplete) @($null) $true
  if($unqualified.states.Count -ne 0 -or $unqualified.rejected.Count -ne 1 -or $unqualified.rejected[0].source_index -ne 0){throw 'Incomplete native state did not preserve its rejection evidence'}
  $rejected=$classifier.ClassifyBitmap($reference,1,[NavigatorGeometry[]]$unqualified.states,2.).classification -eq 'U'
  if(!$rejected){throw 'A displayed Navigator without qualified geometry was accepted'}
  foreach($case in @(@{image=$reference;states=@($missing,$future)},@{image=$unmatched;states=@($geometry,$future)})){
   $rejected=$false
   $rejected=$classifier.ClassifyBitmap($case.image,1000,$case.states,2.).classification -eq 'U'
   if(!$rejected){throw 'An ambiguous native size accepted a missing or unmatched action-row anchor'}
  }
  $u=[pscustomobject]@{classification='U'};$i=[pscustomobject]@{classification='I'}
  Navigator-ValidateResults 'synthetic-open' @($u,$i)
  foreach($sequence in @(@($u),@([pscustomobject]@{classification='C'},$u,$i),@($i,$u,[pscustomobject]@{classification='A'}))){
   $rejected=$false;try{Navigator-ValidateResults 'synthetic-open' $sequence}catch{$rejected=$true};if(!$rejected){throw 'Unclassified frames waived pixel order or coverage'}
  }
  $oldDisplayed=$classifier.ClassifyBitmap($reference,1000,@($geometry,$future),2.)
  $wrongImage=$classifier.ClassifyBitmap($mismatch,1000,@($geometry,$future),2.)
  $canvasHole=$classifier.ClassifyBitmap($exposure,1,@($geometry),2.)
  if($oldDisplayed.classification -ne 'I' -or $wrongImage.classification -ne 'M' -or $canvasHole.classification -ne 'C'){throw 'Native action-row geometry did not distinguish an old displayed panel from a true image mismatch or canvas hole'}
  $filled=$classifier.ClassifyBitmap($reference,1,@($geometry),2.);$empty=$classifier.ClassifyBitmap($placeholder,1,@($geometry),2.);$early=$classifier.ClassifyBitmap($floating,1,@($geometry),2.)
  if($filled.classification -ne 'I' -or $empty.classification -ne 'P' -or $early.classification -ne 'image-before-panel'){throw 'Navigator classifier did not distinguish filled, placeholder, and floating images'}
  foreach($invalid in @($empty,$early,$wrongImage,$canvasHole)){
   $rejected=$false;try{Navigator-ValidateResults 'synthetic-open' @($invalid,$filled)}catch{$rejected=$true}
   if(!$rejected){throw 'Navigator gate accepted an invalid first panel frame followed by I'}
  }
 }finally{$classifier.Dispose();$reference.Dispose();$placeholder.Dispose();$floating.Dispose();$exposure.Dispose();$mismatch.Dispose();$unmatched.Dispose()}
}
function Navigator-RetainedFrames {
 . (Join-Path $PSScriptRoot 'ComposedFrames.ps1')
 Navigator-LoadTypes
 Navigator-TestClassifier
 $script:navOrigin=[CapyRowPointer+Point]::new();[CapyRowPointer]::ClientToScreen($drawingWindow,[ref]$script:navOrigin)|Out-Null
 $script:navColumnTravel=100*$scale;$script:navPanelTravel=40*$scale
 $camera=(Model).state.camera;$document=(Model).state.tabs[0];$script:navAspect=[double]$document.width/$document.height
 if($camera.rotation -ne 0 -or $camera.flipped[0] -or $camera.flipped[1]){throw 'Navigator witness requires the restored unrotated and unflipped camera'}
 $canvas=Control 'drawing-canvas' -Arranged;$canvas.SetFocus()
 Navigator-WitnessSize 4
 $witnessBefore=Capture 'retained-before-marks';$witnessArea=Image-Rect
 foreach($mark in @(@(.22,.22,.36,.25),@(.63,.71,.75,.64),@(.25,.27,.25,.38))){
  $x=[int]($script:navOrigin.x+$camera.translation[0]+$document.width*$camera.zoom*$mark[0]);$y=[int]($script:navOrigin.y+$camera.translation[1]+$document.height*$camera.zoom*$mark[1])
  [CapyRowPointer]::Down('mouse',$x,$y)
  for($step=1;$step -le 12;$step++){[CapyRowPointer]::Move([int]($x+$document.width*$camera.zoom*($mark[2]-$mark[0])*$step/12),[int]($y+$document.height*$camera.zoom*($mark[3]-$mark[1])*$step/12))}
  [CapyRowPointer]::Up()
 }
 Wait-Canvas {(Model).state.document_file.modified} 'Asymmetric Navigator witness was not drawn'
 try{Wait-Until {$after=Capture 'retained-after-marks';try{(Different $witnessBefore $after $witnessArea) -ge 20}finally{$after.Dispose()}} 'Asymmetric witness did not reach the Navigator image'}finally{$witnessBefore.Dispose()}
 for($step=0;$step -lt 14;$step++){Invoke 'navigator-zoom_out'}
 Navigator-Reference
 $originalForeground=Navigator-Foreground '#ff00ff'
 try{
  Navigator-Collapse
  Navigator-DetachedStroke
  $b=(Control 'column-icon-navigator' -Arranged).Current.BoundingRectangle
  [CapyRowPointer]::RightClick([int]($b.Left+$b.Width/2),[int]($b.Top+$b.Height/2))
  $expand=(Control 'Expand column' -Name -Arranged).Current.BoundingRectangle
  Navigator-Observe 'retained-expand' {param($i)if($i -eq 3){[CapyRowPointer]::Down('mouse',[int]($expand.Left+$expand.Width/2),[int]($expand.Top+$expand.Height/2));[CapyRowPointer]::Up()}} $true
  $null=Control 'navigator-overview' -Arranged
  Navigator-Collapse;Navigator-DetachedUndo
  $icon=(Control 'column-icon-navigator' -Arranged).Current.BoundingRectangle
  [CapyRowPointer]::RightClick([int]($icon.Left+$icon.Width/2),[int]($icon.Top+$icon.Height/2));$expand=(Control 'Expand column' -Name -Arranged).Current.BoundingRectangle
  Navigator-Observe 'retained-expand-undo' {param($i)if($i -eq 3){[CapyRowPointer]::Down('mouse',[int]($expand.Left+$expand.Width/2),[int]($expand.Top+$expand.Height/2));[CapyRowPointer]::Up()}} $false
  Navigator-Collapse
  $b=(Control 'column-icon-navigator' -Arranged).Current.BoundingRectangle
  [CapyRowPointer]::RightClick([int]($b.Left+$b.Width/2),[int]($b.Top+$b.Height/2))
  $toggle=(Control 'Open individual panels' -Name).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern)
  if($toggle.Current.ToggleState -eq [System.Windows.Automation.ToggleState]::Off){$toggle.Toggle()}
  [CapyRowPointer]::Key(0x1b)
  Invoke 'column-icon-navigator';$null=Control 'navigator-overview' -Arranged
  Navigator-Reference
  $drawerIdentity=(Control 'navigator-overview' -Arranged).GetRuntimeId() -join ':'
  Invoke 'column-icon-navigator';Wait-Until {!(Find 'navigator-overview' -Visible)} 'Warmed Navigator drawer did not close'
  Navigator-DetachedStroke
  $contextStart=Model;$returnTab=$contextStart.windows_tabs.selected;$otherTab=@($contextStart.windows_tabs.tabs|Where-Object {$_.id -ne $returnTab})[0]
  if(!$otherTab){throw 'Closed Navigator context check requires another drawing'}
  Invoke ('drawing-tab-'+$otherTab.id)
  Wait-Canvas {$current=Model;$current.windows_tabs.selected -eq $otherTab.id -and $current.brush_ready -and $current.canvas_ready} 'Closed Navigator context destination did not settle' 60
  $contextOther=Model
  Invoke ('drawing-tab-'+$returnTab)
  Wait-Canvas {$current=Model;$current.windows_tabs.selected -eq $returnTab -and $current.brush_ready -and $current.canvas_ready} 'Closed Navigator context return did not settle' 60
  $contextEnd=Model
  $script:navClosedContext=@{drawing=$returnTab;other_drawing=$otherTab.id;generation_before=$contextStart.state.toolbar_context_generation;generation_other=$contextOther.state.toolbar_context_generation;generation_after=$contextEnd.state.toolbar_context_generation;runtime_id_before=$drawerIdentity}
  $script:navClosedContext|ConvertTo-Json|Set-Content (Join-Path $run 'closed-context.json')
  if($script:navClosedContext.generation_other -eq $script:navClosedContext.generation_before -and $script:navClosedContext.generation_after -eq $script:navClosedContext.generation_other){throw 'Drawing switches did not change the closed Navigator toolbar context'}
  $drawerIcon=(Control 'column-icon-navigator' -Arranged).Current.BoundingRectangle
  Navigator-Observe 'retained-drawer' {param($i)if($i -eq 3){[CapyRowPointer]::Down('mouse',[int]($drawerIcon.Left+$drawerIcon.Width/2),[int]($drawerIcon.Top+$drawerIcon.Height/2));[CapyRowPointer]::Up()}} $true
  if(((Find 'navigator-overview' -Visible).GetRuntimeId() -join ':') -ne $drawerIdentity){throw 'Warmed drawer reopen replaced the native Navigator'}
  Invoke 'column-icon-navigator';Wait-Until {!(Find 'navigator-overview' -Visible)} 'Authored drawer did not close';Navigator-DetachedUndo
  Navigator-Observe 'retained-drawer-undo' {param($i)if($i -eq 3){[CapyRowPointer]::Down('mouse',[int]($drawerIcon.Left+$drawerIcon.Width/2),[int]($drawerIcon.Top+$drawerIcon.Height/2));[CapyRowPointer]::Up()}} $false
  Invoke 'column-icon-navigator';Navigator-Expand;$null=Control 'navigator-overview' -Arranged
  Navigator-Reference
  $divider=@((Model).layout.dividers|Where-Object {$_.axis -eq 'horizontal' -and !$_.fixed}|Sort-Object {$_.bounds.x} -Descending)[0]
  if(!$divider){throw 'Navigator column divider is missing'}
  $x=[int]($script:navOrigin.x+($divider.bounds.x+$divider.bounds.width/2)*$scale);$y=[int]($script:navOrigin.y+120*$scale)
  Navigator-Observe 'retained-column-drag' {
   param($i)
   if($i -eq 3){[CapyRowPointer]::Down('mouse',$x,$y)}
   if($i -ge 4 -and $i -le 75){[CapyRowPointer]::Move([int]($x-$script:navColumnTravel*($i-3)/72),$y)}
   if($i -eq 76){[CapyRowPointer]::Up()}
  }
  $overview=(Control 'navigator-overview' -Arranged).Current.BoundingRectangle
  $vertical=@((Model).layout.dividers|Where-Object {$_.axis -eq 'vertical' -and !$_.fixed -and $_.bounds.x -lt (($overview.Right-$script:navOrigin.x)/$scale) -and ($_.bounds.x+$_.bounds.width) -gt (($overview.Left-$script:navOrigin.x)/$scale)})[0]
  if(!$vertical){throw 'Navigator panel divider is missing'}
  $x=[int]($script:navOrigin.x+($vertical.bounds.x+$vertical.bounds.width/2)*$scale);$y=[int]($script:navOrigin.y+($vertical.bounds.y+$vertical.bounds.height/2)*$scale)
  Navigator-Observe 'retained-panel-divider' {
   param($i)
   if($i -eq 3){[CapyRowPointer]::Down('mouse',$x,$y)}
   if($i -ge 4 -and $i -le 75){[CapyRowPointer]::Move($x,[int]($y+$script:navPanelTravel*($i-3)/72))}
   if($i -eq 76){[CapyRowPointer]::Up()}
  }
  $null=Navigator-Foreground $originalForeground
 }finally{$script:navClassifier.Dispose()}
}
