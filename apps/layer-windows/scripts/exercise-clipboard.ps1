param([Parameter(Mandatory)][string]$Executable,[ValidateSet('dark','light')][string]$Theme='dark')
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyWaitSeconds=30;$CapyFind='prefer-visible';$CapyPopups=$true
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
Add-Type -AssemblyName System.Drawing
Add-Type -Path (Join-Path $PSScriptRoot 'PackageFixture.cs')
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$run=Join-Path $repo ('artifacts/windows/clipboard/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$CapyTraceDirectory=$run
$checks=[ordered]@{theme=$Theme}
function Sta([scriptblock]$Script,[object[]]$Arguments=@()){
 $shell=[powershell]::Create();$shell.Runspace=[runspacefactory]::CreateRunspace();$shell.Runspace.ApartmentState='STA';$shell.Runspace.Open()
 try{$null=$shell.AddScript('Add-Type -AssemblyName System.Windows.Forms,System.Drawing').AddStatement().AddScript($Script);foreach($a in $Arguments){$null=$shell.AddArgument($a)};$result=$shell.Invoke();if($shell.HadErrors){throw ($shell.Streams.Error|Out-String)};$result}
 finally{$shell.Runspace.Dispose();$shell.Dispose()}
}
function Formats{@(Sta {$data=[Windows.Forms.Clipboard]::GetDataObject();if($data){$data.GetFormats()}})}
function Clip-Size{
 $size=Sta {$data=[Windows.Forms.Clipboard]::GetDataObject();$stream=$data.GetData('PNG');if($stream){$image=[Drawing.Image]::FromStream($stream);try{"$($image.Width)x$($image.Height)"}finally{$image.Dispose()}}}
 [string]$size
}
function Start-Review([string]$Name){
 $script:CapyStateFile=$null
 $script:stderr=Join-Path $run "$Name-stderr.log"
 $script:review=Start-Process -FilePath $Executable -WorkingDirectory $run -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
 $null=$review.Handle
 Write-Output "Owned clipboard review $($review.Id): $run"
 $native=@{window=$null}
 Wait-Until {$native.window=Owned-DrawingWindow $review;$native.window -and (Model).brush_ready -and (Model).windows_workspace.ready -and !(Model).windows_workspace.busy} 'Review did not start' 90
 if((Model).state.theme -ne $Theme){throw 'The clipboard review did not apply the requested theme'}
 $script:drawingWindow=$native.window.Handle;$script:root=$native.window.Root
 [CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
 [CapyRowPointer]::SetForegroundWindow($drawingWindow)|Out-Null;[CapyRowPointer]::Initialize([uint32]$review.Id)
}
function Close-Review{
 [CapyRowPointer]::Dispose()
 & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Close -DiscardUnsaved -StateDirectory $run
 if((Get-Item -LiteralPath $stderr).Length){throw "Native clipboard stderr needs inspection: $stderr"}
}
function Active-Tab{@((Model).state.tabs|Where-Object active)[0]}
function Document-Identity{
 $view=Model;$m=$view.state;$stamp=@($view.windows_tabs.session_stamps|Where-Object id -eq $view.windows_tabs.selected)[0].stamp
 [ordered]@{drawing=$stamp|Select-Object artwork,checkpoint,revision,working_generation;file=$m.document_file|Select-Object revision,modified,location;layers=$m.layers|Select-Object id,label,group,adjustment_effect,visible,locked,paint_revision,mask_revision,object;depth=$view.color_panel.document_depth}|ConvertTo-Json -Depth 20 -Compress
}
function Cross-Window-Clip{
 $identity=Document-Identity;$sourceRoot=$root;$sourceHandle=$drawingWindow;$sourceState=$script:CapyStateFile
 $label=(Model).state.layer_tools.editing_layer.label;$windowsFile=Join-Path $run ("windows-"+$review.Id+".json")
 $before=@((Read-Snapshot $windowsFile).windows).Count
 & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'File'
 Invoke 'New Window' -Name
 Wait-Until {@((Read-Snapshot $windowsFile).windows).Count -eq $before+1} 'New Window did not create an owned clipboard destination' 90
 $peer=@((Read-Snapshot $windowsFile).windows|Where-Object {[long]$_.hwnd -ne $sourceHandle.ToInt64()})[-1]
 try{
  $script:CapyStateFile=Join-Path $run ("ui-state-"+$review.Id+"-"+$peer.id+".json")
  Wait-Until {(Model).brush_ready -and (Model).windows_workspace.ready -and !(Model).windows_workspace.busy} 'The clipboard destination window did not become ready' 90
  $native=Owned-DrawingWindow $review ([long]$peer.hwnd)
  $script:root=$native.Root;$script:drawingWindow=$native.Handle
  Chord @(0x11) 0x56
  Wait-Until {(Requests) -eq 0 -and (Model).state.document_file.modified -and (Model).brush_ready} 'The copied layer did not paste into the second window' 90
  Capture "cross-window-layer-$Theme" -WithModel
  if((Model).state.layer_tools.editing_layer.label -ne $label -or (Model).state.layer_tools.editing_layer.object){throw 'The second window imported the public PNG instead of retaining the copied layer'}
  Copied {Chord @(0x11) 0x43} 'The second window did not publish its copied layer before closing'
  & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Close -DiscardUnsaved -StateDirectory $run
 }finally{$script:root=$sourceRoot;$script:drawingWindow=$sourceHandle;$script:CapyStateFile=$sourceState;[CapyRowPointer]::SetForegroundWindow($drawingWindow)|Out-Null}
 Wait-Until {@((Read-Snapshot $windowsFile).windows).Count -eq $before -and (Model).brush_ready} 'Closing the clipboard destination did not return to the source window'
 if((Document-Identity) -ne $identity){throw 'Pasting in another window changed the original artwork'}
 $checks.same_process_cross_window_layer_copy='passed'
}
function Package([string]$Path){
 $members=[CapyPackageFixture]::Read($Path)
 @{members=$members;manifest=([Text.Encoding]::UTF8.GetString(@($members|Where-Object Key -eq 'manifest.json')[0].Value)|ConvertFrom-Json -Depth 100)}
}
function Json($Value){ConvertTo-Json -InputObject $Value -Depth 100 -Compress}
function Resource-Value($Value,$Package){
 if($Value -is [array]){return ,@($Value|ForEach-Object {Resource-Value $_ $Package})}
 if($null -eq $Value -or $Value.GetType() -ne [System.Management.Automation.PSCustomObject]){return $Value}
 if($Value.PSObject.Properties['ref']){
  $resource=@($Package.manifest.resources|Where-Object id -eq $Value.ref)
  if($resource.Count -ne 1){throw 'Paint data must reference exactly one package resource'}
  $resource=$resource[0];$location=$resource.location
  $member=@($Package.members|Where-Object Key -eq $(if($location.pack){$location.pack}else{$location.member}))
  if($member.Count -ne 1){throw 'Paint resource member is missing'}
  $bytes=[byte[]]::new([int]$resource.bytes);[Array]::Copy($member[0].Value,[int64]$location.offset,$bytes,0,$bytes.Length)
  return [ordered]@{type=$resource.type;encoding=$resource.encoding;data=(Resource-Value $resource.data $Package);sha256=[Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($bytes))}
 }
 $record=[ordered]@{};foreach($property in $Value.PSObject.Properties|Sort-Object Name){$record[$property.Name]=Resource-Value $property.Value $Package};$record
}
function Authored-Paint($Package){
 $layers=@($Package.manifest.objects|Where-Object {$_.type -eq 'capy.occurrence/3' -and $_.data.content.paint})
 if($layers.Count -ne 1){throw 'The authored clipboard fixture must contain one paint layer'}
 $layer=$layers[0];$paint=@($Package.manifest.objects|Where-Object id -eq $layer.data.content.paint.ref)
 if($paint.Count -ne 1 -or $paint[0].type -ne 'capy.paint-source/2' -or $paint[0].data.base -or !$paint[0].data.tiles){throw 'The copied layer lost its authored raster tiles or became a flattened image'}
 $properties=[ordered]@{};foreach($property in $layer.data.PSObject.Properties|Where-Object Name -NotIn @('content','offset')|Sort-Object Name){$properties[$property.Name]=$property.Value}
 Json ([ordered]@{properties=$properties;paint=(Resource-Value $paint[0].data $Package)})
}
function Image-Identity($Package){Json @($Package.manifest.objects|Where-Object type -in @('capy.image/1','capy.image-object/1','capy.occurrence/3','capy.stack/1')|Sort-Object id)}
function Affine($Object){if($null -eq $Object.data.affine){@(1,0,0,1,0,0)}else{$Object.data.affine}}
function Root-Stack($Package){
 $composition=@($Package.manifest.objects|Where-Object id -eq $Package.manifest.root.ref)
 $stack=@($Package.manifest.objects|Where-Object id -eq $composition[0].data.result.object.ref)
 if($composition.Count -ne 1 -or $stack.Count -ne 1 -or $stack[0].type -ne 'capy.stack/1'){throw 'Position checks require an ordinary root layer stack'}
 $stack[0]
}
function Object-Layers($Package){@($Package.manifest.objects|Where-Object {$_.type -eq 'capy.occurrence/3' -and $_.data.content.objects})}
function Effective-Affine($Package,$Layer){
 if($Layer.id -notin (Root-Stack $Package).data.entries.ref){throw 'Position checks require a root Object layer'}
 $object=@($Package.manifest.objects|Where-Object id -eq $Layer.data.content.objects.ref)
 if($object.Count -ne 1 -or $object[0].type -ne 'capy.image-object/1'){throw 'The Object layer must directly own one image object'}
 $affine=@(Affine $object[0]);$offset=if($Layer.data.offset){$Layer.data.offset}else{@(0,0)}
 foreach($axis in 0..1){$affine[4+$axis]+=[double]$offset[$axis]};$affine
}
function Document-Point($Camera,[double[]]$Surface){
 $x=$Surface[0]-$Camera.translation[0];$y=$Surface[1]-$Camera.translation[1];$cos=[Math]::Cos($Camera.rotation);$sin=[Math]::Sin($Camera.rotation)
 @((($cos*$x+$sin*$y)/$Camera.zoom*$(if($Camera.flipped[0]){-1}else{1})),((-$sin*$x+$cos*$y)/$Camera.zoom*$(if($Camera.flipped[1]){-1}else{1})))
}
function Paste-Position([string]$Command,[string]$Shortcut,[uint16[]]$Modifiers,$Before,[switch]$Cursor){
 [CapyRowPointer]::SetForegroundWindow($drawingWindow)|Out-Null;(Control 'drawing-canvas').SetFocus()
 Wait-Until {(Control 'drawing-canvas').Current.HasKeyboardFocus -and [CapyRowPointer]::GetForegroundWindow() -eq $drawingWindow} 'Paste position needs the owned drawing canvas focused'
 $binding=@((Model).state.commands|Where-Object id -eq $Command)
 if($binding.Count -ne 1 -or !$binding[0].enabled -or $binding[0].shortcut -ne $Shortcut){throw "$Command does not expose the default $Shortcut binding"}
 $original=@($Before.manifest.objects|Where-Object type -eq 'capy.image-object/1')
 $owners=@(Object-Layers $Before)
 if($original.Count -ne 1 -or $owners.Count -ne 1 -or $owners[0].data.content.objects.ref -ne $original[0].id){throw 'Position checks require one copied image in one ordinary Object layer'}
 $layer=$owners[0];$a=Effective-Affine $Before $layer;$order=@((Root-Stack $Before).data.entries.ref)
 $image=@($Before.manifest.objects|Where-Object id -eq $original[0].data.image.ref)[0]
 if(($a[0..3] -join ',') -ne '1,0,0,1' -or ($image.data.extent -join 'x') -ne '40x30'){throw 'Position checks require the original unscaled 40x30 image'}
 $clip=@((Clip-Size) -split 'x'|ForEach-Object {[double]$_})
 $bounds=(Control 'drawing-canvas' -Arranged).Current.BoundingRectangle;$camera=(Model).state.camera;$area=$camera.work_area
 $surface=@(($area[0]+$area[2]/2),($area[1]+$area[3]/2));$point=$null;$requested=$null;$hover=$null
 if($Cursor){
  $point=@([int]($bounds.X+($area[0]+$area[2]*.25)*$bounds.Width/$camera.viewport[0]),[int]($bounds.Y+($area[1]+$area[3]*.65)*$bounds.Height/$camera.viewport[1]))
  if(!$bounds.Contains([double]$point[0],[double]$point[1])){throw 'The clipboard cursor point is outside the canvas'}
  $requested=@($point);$document=Document-Identity
  $park=@([int]($bounds.X+($area[0]+$area[2]*.75)*$bounds.Width/$camera.viewport[0]),[int]($bounds.Y+($area[1]+$area[3]*.35)*$bounds.Height/$camera.viewport[1]))
  $work=[Windows.Rect]::new($bounds.X+$area[0]*$bounds.Width/$camera.viewport[0],$bounds.Y+$area[1]*$bounds.Height/$camera.viewport[1],$area[2]*$bounds.Width/$camera.viewport[0],$area[3]*$bounds.Height/$camera.viewport[1])
  foreach($at in @($park,$requested)){if(!$work.Contains([Windows.Rect]::new($at[0]-48,$at[1]-48,96,96))){throw 'Cursor acknowledgment patches must fit inside the owned work area'}}
  if([Math]::Pow($park[0]-$requested[0],2)+[Math]::Pow($park[1]-$requested[1],2) -lt 96*96){throw 'Cursor acknowledgment needs distinct park and target points'}
  [CapyRowPointer]::Hover($park[0],$park[1])
  $empty=Wait-StablePixels {Screen-Pixels $requested[0] $requested[1]}
  if((Screen-Pixels $requested[0] $requested[1] (Join-Path $run 'cursor-before.png')) -ne $empty){throw 'Cursor baseline changed before its capture'}
  [CapyRowPointer]::Hover($requested[0],$requested[1])
  $observed=[CapyWindowExercise+Point]::new()
  if(![CapyWindowExercise]::GetCursorPos([ref]$observed)){throw 'The clipboard cursor position is unavailable'}
  $point=@($observed.x,$observed.y)
  if(!$bounds.Contains([double]$point[0],[double]$point[1])){throw 'The observed clipboard cursor is outside the canvas'}
  $surface=@((($point[0]-$bounds.X)*$camera.viewport[0]/$bounds.Width),(($point[1]-$bounds.Y)*$camera.viewport[1]/$bounds.Height))
  Wait-Until {(Screen-Pixels $requested[0] $requested[1]) -ne $empty} 'Mouse hover did not present the shared cursor at the paste target'
  $shown=Wait-StablePixels {Screen-Pixels $requested[0] $requested[1]}
  if($shown -eq $empty -or (Screen-Pixels $requested[0] $requested[1] (Join-Path $run 'cursor-after.png')) -ne $shown){throw 'Mouse hover did not retain distinct stable cursor pixels'}
  Capture "paste-cursor-acknowledged-$Theme" -Composed -WithModel
  $hover=@{park=$park;requested=$requested;observed=$point;sample_center=$requested;before_sha256=$empty;after_sha256=$shown;camera_before=$camera;camera_after=(Model).state.camera;document_before=$document;document_after=(Document-Identity)}
  $hover|ConvertTo-Json -Depth 12|Set-Content (Join-Path $run 'cursor-acknowledgment.json')
  if((Json $hover.camera_after) -ne (Json $camera) -or $hover.document_after -ne $document){throw 'Acknowledging cursor hover changed the camera or drawing'}
 }
 $center=Document-Point $camera $surface
 $expected=@($a);foreach($axis in 0..1){$expected[4+$axis]+=[Math]::Round($center[$axis]-$clip[$axis]/2,[MidpointRounding]::AwayFromZero)-[Math]::Floor($a[4+$axis])}
 $revision=(Model).state.document_file.revision
 if(!(Control 'drawing-canvas').Current.HasKeyboardFocus){throw 'The canvas lost keyboard focus after cursor acknowledgment'}
 [CapyRowPointer]::Chord([uint32]$review.Id,$Modifiers,0x56)
 Wait-Until {(Model).state.document_file.revision -gt $revision -and @((Model).state.layers|Where-Object object).Count -eq 2 -and (Model).state.layer_tools.editing_layer.object -and (Requests) -eq 0} "$Shortcut did not paste a second image"
 if((Model).state.layer_tools.tool -eq 'transform'){throw "$Shortcut opened placement handles"}
 $path=Join-Path $run ($Command+'.capy');Save-ProjectAs $path;$after=Package $path
 $added=@($after.manifest.objects|Where-Object {$_.type -eq 'capy.image-object/1' -and $_.id -notin $original.id})
 $destinations=@(Object-Layers $after);$addedLayers=@($destinations|Where-Object id -NotIn $owners.id)
 $actual=if($addedLayers.Count -eq 1){Effective-Affine $after $addedLayers[0]}else{@()}
 @{command=$Command;shortcut=$binding[0].shortcut;camera=$camera;requested_cursor=$requested;cursor=$point;surface=$surface;document_center=$center;clipboard_extent=$clip;expected_affine=$expected;actual_affine=$actual}|ConvertTo-Json -Depth 10|Set-Content (Join-Path $run ($Command+'-position.json'))
 if($added.Count -ne 1 -or $destinations.Count -ne 2 -or $addedLayers.Count -ne 1 -or $addedLayers[0].data.content.objects.ref -ne $added[0].id){throw "$Shortcut did not add one independent Object layer"}
 $expectedOrder=@(foreach($id in $order){if($id -eq $layer.id){$addedLayers[0].id};$id})
 if(((Root-Stack $after).data.entries.ref -join ',') -ne ($expectedOrder -join ',')){throw "$Shortcut did not preserve the sibling layer order"}
 if((Json @($after.manifest.objects|Where-Object id -eq $layer.id)) -ne (Json @($layer))){throw "$Shortcut changed the original Object layer"}
 if($added[0].data.image.ref -ne $original[0].data.image.ref -or (Json @($after.manifest.objects|Where-Object type -eq 'capy.image/1'|Sort-Object id)) -ne (Json @($Before.manifest.objects|Where-Object type -eq 'capy.image/1'|Sort-Object id))){throw "$Shortcut changed the immutable source"}
 if((Json @($after.manifest.objects|Where-Object id -eq $original[0].id)) -ne (Json $original)){throw "$Shortcut changed the original image object"}
 foreach($i in 0..5){if([Math]::Abs($actual[$i]-$expected[$i]) -gt .000001){throw "$Shortcut changed image placement on affine component $i"}}
 Capture "$Command-$Theme" -WithModel
 Invoke 'Undo' -Name;Wait-Until {@((Model).state.layers|Where-Object object).Count -eq 1} "One Undo did not remove $Command"
 $restored=Join-Path $run ($Command+'-undo.capy');Save-ProjectAs $restored
 if((Image-Identity (Package $restored)) -ne (Image-Identity $Before)){throw "$Shortcut Undo changed the original image graph"}
 $checks[$Command]='default shortcut; immutable source/effective affine/sibling order; one-step Undo'
}
function New-Image([scriptblock]$Action,[string]$Extent,[string]$Name,[int]$Objects=-1,[string[]]$Sources=@(),$AuthoredSource=$null){
 $source=(Model).windows_tabs.selected;$count=@((Model).windows_tabs.tabs).Count;$identity=Document-Identity
 $identity|Set-Content (Join-Path $run ($Name+'-source-before.json'))
 & $Action
 Wait-Until {@((Model).windows_tabs.tabs).Count -eq $count+1 -and (Model).windows_tabs.selected -ne $source -and (Model).brush_ready -and (Requests) -eq 0 -and !(Model).state.document_file.busy} "$Name did not open a new drawing" 90
 $tab=Active-Tab
 if("$($tab.width)x$($tab.height)" -ne $Extent -or !(Model).state.document_file.modified -or (Model).state.document_file.location){throw "$Name did not open an unsaved drawing at the clipboard bounds"}
 if((Model).state.layer_tools.tool -eq 'transform'){throw "$Name opened placement handles"}
 if($Objects -ge 0 -and @((Model).state.layers|Where-Object object).Count -ne $Objects){throw "$Name did not preserve the clipboard image objects"}
 if($AuthoredSource){
  if(@((Model).state.commands|Where-Object {$_.id -eq 'undo' -and $_.enabled}).Count){throw 'Authored New Image unexpectedly has Undo history'}
  $created=(Model).windows_tabs.selected;$createdIdentity=Document-Identity
  & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'File';Invoke-Id 'close_document'
  Wait-Until {@((Model).state.requests|Where-Object {$_.kind.request.type -eq 'confirm_close'}).Count -eq 1} 'Authored New Image did not request unsaved-close confirmation'
  $dialog=Control 'document-dialog' -Arranged
  Capture "$Name-unsaved-close-$Theme" -WithModel
  Invoke (Model).document_options.cancel_label -Name -Within $dialog
  Wait-Until {!(Find 'document-dialog' -Visible) -and (Requests) -eq 0 -and !(Model).state.document_file.busy} 'Cancel did not dismiss the authored drawing close'
  if((Model).windows_tabs.selected -ne $created -or @((Model).windows_tabs.tabs).Count -ne $count+1 -or (Document-Identity) -ne $createdIdentity){throw 'Cancel changed the authored unsaved drawing'}
  $checks.authored_new_image_unsaved_close='shared confirm_close; native dialog; Cancel preserves the modified drawing'
 }
 Capture "$Name-$Theme" -WithModel
 $saved=Join-Path $run "$Name 日本語.capy";Save-ProjectAs $saved
 if($AuthoredSource){
  $copied=Package $saved;$expected=Authored-Paint $AuthoredSource;$actual=Authored-Paint $copied
  $expected|Set-Content (Join-Path $run 'authored-source-identity.json');$actual|Set-Content (Join-Path $run 'authored-copy-identity.json')
  if($actual -ne $expected){throw 'New Image changed authored layer properties, raster tiles or material'}
  $layers=@($copied.manifest.objects|Where-Object type -eq 'capy.occurrence/3')
  if($layers.Count -ne 2 -or @($layers|Where-Object {$_.data.content.effect -and $_.data.visible -eq $false}).Count -ne 1){throw 'Authored New Image did not retain one paint layer and transparent paper'}
  if((Model).state.document_file.modified -or !(Model).state.document_file.location){throw 'Saving the authored New Image did not establish its own clean file'}
  $checks.authored_layer_retained='paint source and exact raster payloads; opacity and other authored properties; transparent paper'
 }
 if($Sources.Count){
  $manifest=(Package $saved).manifest
  $extents=@($manifest.objects|Where-Object type -eq 'capy.image/1'|ForEach-Object {$_.data.extent -join 'x'}|Sort-Object)
  if(($extents -join ',') -ne (($Sources|Sort-Object) -join ',')){throw "$Name changed the source image extents"}
  foreach($object in $manifest.objects|Where-Object type -eq 'capy.image-object/1'){
   $affine=if($null -eq $object.data.affine){@(1,0,0,1,0,0)}else{$object.data.affine}
   if(($affine[0..3] -join ',') -ne '1,0,0,1'){throw "$Name scaled or rotated a clipboard image"}
  }
 }
 & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'File';Invoke-Id 'close_document'
 Wait-Until {@((Model).windows_tabs.tabs).Count -eq $count -and (Model).windows_tabs.selected -eq $source -and (Model).brush_ready -and !(Model).state.document_file.busy} "$Name did not return to its source drawing" 90
 $returned=Document-Identity;$returned|Set-Content (Join-Path $run ($Name+'-source-after.json'))
 if($returned -ne $identity){throw "$Name changed the source drawing"}
 $checks[$Name]=$Extent
}
function Layers{@((Model).state.layers).Count}
function Requests{@((Model).state.requests).Count}
function Chord([uint16[]]$Modifiers,[uint16]$Key){
 (Control 'drawing-canvas').SetFocus()
 [CapyRowPointer]::SetForegroundWindow($drawingWindow)|Out-Null
 Wait-Until {(Control 'drawing-canvas').Current.HasKeyboardFocus -and [CapyRowPointer]::GetForegroundWindow() -eq $drawingWindow} 'Clipboard input needs the owned drawing canvas focused'
 [CapyRowPointer]::Chord([uint32]$review.Id,$Modifiers,$Key)
}
function Copied([scriptblock]$Action,[string]$Message){
 Sta {[Windows.Forms.Clipboard]::SetText('awaiting copy')}|Out-Null;& $Action
 Wait-Until {(Formats) -contains 'art.capycanvas.clip.nonce' -and (Formats) -contains 'PNG' -and (Requests) -eq 0} $Message
}
function Settled{Wait-Until {(Requests) -eq 0 -and !(Model).state.document_file.busy} 'The clipboard request did not finish'}
function Changed([scriptblock]$Action){
 $revision=(Model).state.document_file.revision
 & $Action
 Wait-Until {(Model).state.document_file.revision -gt $revision} 'The clipboard edit was not acknowledged'
 Settled
}
function Mask-Changed([scriptblock]$Action){
 $id=(Model).state.layer_tools.editing_layer.id
 $revision=@((Model).state.layers|Where-Object id -eq $id)[0].mask_revision
 Changed $Action
 Wait-Until {@((Model).state.layers|Where-Object id -eq $id)[0].mask_revision -ne $revision} 'The pasted mask pixels did not change'
}
function Tool([string]$Command){
 Invoke-Id (Tool-Tile $Command)
}
function Native-Clipboard-NoEffect($Target,[string]$Name){
 $identity=Document-Identity;$source=(Model).windows_tabs.selected;$tabs=@((Model).windows_tabs.tabs).Count;$requests=Requests;$notice=(Model).state.notice|ConvertTo-Json -Depth 10 -Compress
 foreach($key in @(0x43,0x58,0x56)){
  if($key -eq 0x56){Sta {param($path)$image=[Drawing.Image]::FromFile($path);try{[Windows.Forms.Clipboard]::SetImage($image)}finally{$image.Dispose()}} @($foreign)|Out-Null}
  else{Sta {[Windows.Forms.Clipboard]::SetText('native clipboard sentinel')}|Out-Null}
  $Target.SetFocus();Wait-Until {$Target.Current.HasKeyboardFocus} "$Name did not take keyboard focus"
  [CapyRowPointer]::Chord([uint32]$review.Id,@(0x11),$key)
  $watch=[Diagnostics.Stopwatch]::StartNew()
  do{
   if((Requests) -ne $requests -or (Document-Identity) -ne $identity -or (Model).windows_tabs.selected -ne $source -or @((Model).windows_tabs.tabs).Count -ne $tabs){throw "$Name sent a native clipboard key to the artwork"}
   if(((Model).state.notice|ConvertTo-Json -Depth 10 -Compress) -ne $notice){throw "$Name ran an artwork clipboard action that produced a notice"}
   if($key -ne 0x56 -and [string](Sta {[Windows.Forms.Clipboard]::GetText()}) -ne 'native clipboard sentinel'){throw "$Name replaced the clipboard without a native selection"}
   Start-Sleep -Milliseconds 50
  }while($watch.Elapsed.TotalSeconds -lt 1)
 }
 $checks[$Name]='passed'
}
function Paste-In-Place{
 & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'edit'
 (Control 'Paste in Place' -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
}
function Menu-Item([string]$Name){
 $condition=[System.Windows.Automation.AndCondition]::new(
  [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty,$review.Id),
  [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,$Name),
  [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::MenuItem))
 [System.Windows.Automation.AutomationElement]::RootElement.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$condition)
}
try {
 Enter-CapyEnvironment
 $env:CAPY_STORAGE_DIR=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1';$env:CAPY_SMOKE_TEST='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
 [IO.File]::WriteAllText((Settings-File),(@{language=@{Explicit='en'};theme=$Theme}|ConvertTo-Json -Depth 4))
 Start-Review 'clipboard'
 $document=@((Model).state.tabs|Where-Object active)[0];$extent="$($document.width)x$($document.height)"
 Invoke 'Test stroke' -Name
 Wait-Until {(Model).state.document_file.modified} 'Stroke not acknowledged'
 $opacity=Control 'layer-opacity';$opacity.SetFocus();$opacity.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('55')
 (Control 'drawing-canvas').SetFocus()
 Wait-Until {[Math]::Abs((Model).state.layer_tools.editing_layer.opacity-.55) -lt .000001 -and !(Model).state.layer_tools.has_selection} 'The authored clipboard layer opacity did not settle'
 $authoredPath=Join-Path $run 'Authored source.capy';Save-ProjectAs $authoredPath;$authoredSource=Package $authoredPath
 $null=Authored-Paint $authoredSource
 $count=Layers
 Copied {Chord @(0x11) 0x43} 'Ctrl+C did not write the PNG and its nonce'
 if(!(Clip-Size)){throw 'The copied PNG does not decode'}
 if((Layers) -ne $count){throw 'Copy changed the layers'}
 $checks.keyboard_copy_writes_png_and_nonce=Clip-Size
 $bitmapSize=[string](Sta {$image=[Windows.Forms.Clipboard]::GetImage();if($image){try{"$($image.Width)x$($image.Height)"}finally{$image.Dispose()}}})
 if($bitmapSize -ne (Clip-Size)){throw 'The standard Bitmap consumer did not receive the copied image'}
 $checks.standard_bitmap_consumer=$bitmapSize
 $paletteIdentity=Document-Identity;$paletteExtent=Clip-Size
 Invoke 'panel-tab-palettes'
 Wait-Until {@((Model).palette_panel.swatches).Count -gt 0} 'The palette clipboard review did not expose saved swatches'
 $swatch=Control ('palette-swatch-'+@((Model).palette_panel.swatches)[0].id) -Arranged
 Copied {$swatch.SetFocus();Wait-Until {$swatch.Current.HasKeyboardFocus} 'The palette swatch did not take focus';[CapyRowPointer]::Chord([uint32]$review.Id,@(0x11),0x43)} 'A focused palette swatch blocked artwork Copy'
 if((Clip-Size) -ne $paletteExtent -or (Document-Identity) -ne $paletteIdentity){throw 'Copy from a focused palette swatch changed the original artwork or clipboard pixels'}
 $checks.palette_swatch_keeps_artwork_copy='passed'
 Invoke 'panel-tab-color'
 Cross-Window-Clip
 New-Image {Chord @(0x11,0x12) 0x4e} (Clip-Size) 'internal-paste-as-new-image' -AuthoredSource $authoredSource
 $checks.copy_survives_source_window_close='passed'
 Chord @(0x11) 0x56;Settled
 Wait-Until {(Layers) -eq $count+1} 'Ctrl+V did not paste the window copy'
 if((Model).state.layer_tools.tool -eq 'transform'){throw 'Pasting the window copy opened placement handles'}
 Invoke 'Undo' -Name;Wait-Until {(Layers) -eq $count} 'One Undo did not remove the pasted copy'
 Paste-In-Place;Settled
 Wait-Until {(Layers) -eq $count+1} 'Paste in Place did not paste the window copy'
 Invoke 'Undo' -Name;Wait-Until {(Layers) -eq $count} 'One Undo did not remove Paste in Place'
 $checks.own_paste_and_paste_in_place='passed'
 $insertIdentity=Document-Identity;$insertExtent=Clip-Size
 foreach($binding in @(@('copy','Ctrl+C / Ctrl+Insert'),@('cut','Ctrl+X / Shift+Delete'),@('paste_image','Ctrl+V / Shift+Insert'))){
  $command=@((Model).state.commands|Where-Object id -eq $binding[0])
  if($command.Count -ne 1 -or $command[0].shortcut -ne $binding[1]){throw ('Clipboard alternatives are missing from '+$binding[0])}
 }
 $insertEntry=& (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'Edit' -PassThru
 $insertMenu=@{copy=$null}
 Wait-Until {$insertMenu.copy=Menu-Item 'Copy';$insertMenu.copy -and !$insertMenu.copy.Current.IsOffscreen} 'Edit did not expose the native clipboard shortcut hints'
 Capture "insert-shortcuts-menu-$Theme" -WithModel -Composed
 if($insertMenu.copy.Current.IsOffscreen){throw 'The Edit menu closed before its shortcut capture completed'}
 [CapyRowPointer]::Key([uint32]$review.Id,0x1b)
 Wait-Until {$item=Menu-Item 'Copy';!$item -or $item.Current.IsOffscreen} 'Edit did not dismiss before Insert shortcut input'
 $insertMenuOpen=try{$insertEntry.Current.ControlType -eq [System.Windows.Automation.ControlType]::MenuItem -and !$insertEntry.Current.IsOffscreen}catch [System.Windows.Automation.ElementNotAvailableException]{$false}
 if($insertMenuOpen){
  [CapyRowPointer]::Key([uint32]$review.Id,0x1b)
  Wait-Until {try{$insertEntry.Current.IsOffscreen}catch [System.Windows.Automation.ElementNotAvailableException]{$true}} 'The compact application menu did not dismiss before Insert shortcut input'
 }
 $checks.insert_shortcut_hints_visual_review='required: inspect native Edit menu captions and longer shortcut hints'
 Copied {Chord @(0x11) 0x2d} 'Ctrl+Insert did not publish the copied artwork'
 if((Document-Identity) -ne $insertIdentity -or (Clip-Size) -ne $insertExtent){throw 'Ctrl+Insert changed the artwork or copied different bounds'}
 Changed {Chord @(0x10) 0x2d}
 Wait-Until {(Layers) -eq $count+1} 'Shift+Insert did not paste the copied layer'
 if((Model).state.layer_tools.tool -eq 'transform'){throw 'Shift+Insert changed the internal layer paste destination'}
 Changed {Invoke 'Undo' -Name};Wait-Until {(Layers) -eq $count} 'One Undo did not remove the Shift+Insert layer'
 Chord @(0x11) 0x41;Wait-Until {(Model).state.layer_tools.has_selection} 'Insert-key Cut selection did not become ready'
 $insertRevision=(Model).state.document_file.revision
 Copied {Chord @(0x10) 0x2e} 'Shift+Delete did not publish the selected pixels'
 Wait-Until {(Model).state.document_file.revision -gt $insertRevision -and (Clip-Size) -eq $extent} 'Shift+Delete did not copy and erase the selected canvas'
 if((Layers) -ne $count){throw 'Shift+Delete changed the layer count'}
 Changed {Invoke 'Undo' -Name}
 Chord @(0x11) 0x44;Wait-Until {!((Model).state.layer_tools.has_selection)} 'Insert-key selection did not clear after Cut Undo'
 $insertRestored=Join-Path $run 'Insert shortcuts restored.capy';Save-ProjectAs $insertRestored
 if((Authored-Paint (Package $insertRestored)) -ne (Authored-Paint $authoredSource)){throw 'One Shift+Delete Undo did not restore the authored paint source exactly'}
 $checks.insert_clipboard_keys='Ctrl+Insert preserves artwork; Shift+Insert pastes one layer; Shift+Delete copies and erases; one Undo restores exact authored payload'
 $first=(Model).state.layer_tools.editing_layer.id
 Invoke 'layer-new';Wait-Until {(Layers) -eq $count+1} 'The multi-layer clipboard source was not created'
 Invoke 'Test stroke' -Name
 Invoke "layer-$first-selection";Wait-Until {@((Model).state.layers|Where-Object selected).Count -eq 2} 'The clipboard sources were not both selected'
 Chord @(0x11) 0x41;Wait-Until {(Model).state.layer_tools.has_selection} 'The regional clipboard selection was not acknowledged'
 Copied {Chord @(0x11) 0x43} 'Selected-region Copy did not publish both layers'
 Chord @(0x11) 0x56;Settled
 Wait-Until {(Layers) -eq $count+3 -and @((Model).state.layers|Where-Object selected).Count -eq 2} 'Selected-region Paste did not preserve separate layers'
 Invoke 'Undo' -Name;Wait-Until {(Layers) -eq $count+1} 'One Undo did not remove both pasted layers'
 Copied {Chord @(0x11) 0x58} 'Selected-region Cut did not publish both layers'
 Chord @(0x11) 0x56;Settled
 Wait-Until {(Layers) -eq $count+3} 'Selected-region Cut flattened its layers'
 Invoke 'Undo' -Name;Wait-Until {(Layers) -eq $count+1} 'Undo did not remove the pasted multi-layer cut'
 Changed {Invoke 'Undo' -Name}
 Invoke "layer-$first-selection"
 Chord @(0x11) 0x44
 Invoke 'layer-delete';Wait-Until {(Layers) -eq $count} 'Delete did not remove the temporary clipboard source'
 $checks.selected_region_preserves_multiple_layers='passed'
 Chord @(0x11) 0x41;Wait-Until {(Model).state.layer_tools.has_selection} 'Ctrl+A did not select the canvas'
 $revision=(Model).state.document_file.revision
 Copied {Chord @(0x11) 0x58} 'Ctrl+X did not write the clipboard'
 Wait-Until {(Model).state.document_file.revision -gt $revision -and (Clip-Size) -eq $extent} 'Ctrl+X did not copy the selected canvas and erase it'
 Chord @(0x11) 0x56;Settled
 Wait-Until {(Layers) -eq $count+1} 'Ctrl+V did not paste the cut pixels'
 Invoke 'Undo' -Name;Wait-Until {(Layers) -eq $count} 'Undo did not remove the pasted cut'
 $checks.cut_selected_canvas=Clip-Size
 Tool 'lasso';Wait-Until {(Model).state.canvas_bar.context.kind -eq 'selection' -and ((Find 'canvas-bar-menu-copy') -or (Find 'canvas-bar-more'))} 'The selection bar did not appear'
 if(Find 'canvas-bar-menu-copy'){Invoke 'canvas-bar-menu-copy'}else{
  Invoke 'canvas-bar-more';$copy=@{item=$null};Wait-Until {$copy.item=Menu-Item 'Copy';$copy.item} 'The selection bar overflow did not offer Copy'
  $copy.item.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
 }
 $merged=@{item=$null};Wait-Until {$merged.item=Menu-Item 'Copy Merged';$merged.item} 'Copy did not list Copy Merged'
 Copied {$pattern=$null;if($merged.item.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern,[ref]$pattern)){$pattern.Invoke()}else{$merged.item.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()}} 'Copy Merged did not write the clipboard'
 if((Clip-Size) -ne $extent){throw 'Copy Merged did not copy the visible canvas'}
 Capture 'selection-copy'
 $checks.selection_bar_copy_merged=Clip-Size
 Chord @(0x11) 0x44;Wait-Until {!((Model).state.layer_tools.has_selection)} 'Deselect did not clear the pixel selection before image-object checks'
 $foreign=Join-Path $run 'foreign.png'
 $bitmap=[Drawing.Bitmap]::new(40,30);try{$g=[Drawing.Graphics]::FromImage($bitmap);$g.Clear([Drawing.Color]::FromArgb(255,30,160,90));$g.Dispose();$bitmap.Save($foreign,[Drawing.Imaging.ImageFormat]::Png)}finally{$bitmap.Dispose()}
 $maskLayer=(Model).state.layer_tools.editing_layer.id
 Invoke 'layer-add-mask';Wait-Until {(Model).state.layer_tools.editing_layer.has_mask} 'The clipboard mask was not added'
 Invoke "layer-$maskLayer-mask";Wait-Until {(Model).state.layer_tools.editing_layer.mask_selected} 'The clipboard mask did not receive focus'
 Chord @(0x11) 0x41;Wait-Until {(Model).state.layer_tools.has_selection} 'The mask clipboard selection was not acknowledged'
 Copied {Chord @(0x11) 0x43} 'Focused-mask Copy did not publish pixels'
 $paintRevision=@((Model).state.layers|Where-Object id -eq $maskLayer)[0].paint_revision
 Copied {Chord @(0x11) 0x58} 'Focused-mask Cut did not publish pixels'
 Mask-Changed {Chord @(0x11) 0x56}
 if((Layers) -ne $count -or !(Model).state.layer_tools.editing_layer.mask_selected -or @((Model).state.layers|Where-Object id -eq $maskLayer)[0].paint_revision -ne $paintRevision){throw 'Mask Cut/Paste changed layer content or created a layer'}
 Changed {Chord @(0x11) 0x5a}
 Changed {Chord @(0x11) 0x5a}
 Sta {param($path)$image=[Drawing.Image]::FromFile($path);try{[Windows.Forms.Clipboard]::SetImage($image)}finally{$image.Dispose()}} @($foreign)|Out-Null
 Mask-Changed {Chord @(0x11) 0x56}
 if((Layers) -ne $count -or !(Model).state.layer_tools.editing_layer.mask_selected -or @((Model).state.layers|Where-Object id -eq $maskLayer)[0].paint_revision -ne $paintRevision){throw 'External image Paste escaped the focused mask'}
 Changed {Chord @(0x11) 0x5a}
 Changed {Chord @(0x11) 0x5a};Wait-Until {!((Model).state.layer_tools.has_selection)} 'Undo did not clear the temporary mask selection'
 Changed {Chord @(0x11) 0x5a};Wait-Until {!(Model).state.layer_tools.editing_layer.has_mask} 'Undo did not remove the temporary clipboard mask'
 $checks.focused_mask_copy_cut_and_external_paste='passed'
 Sta {param($path)$image=[Drawing.Image]::FromFile($path);try{[Windows.Forms.Clipboard]::SetImage($image)}finally{$image.Dispose()}} @($foreign)|Out-Null
 New-Image {& (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'Edit';Invoke-Id 'paste_as_new_image'} '40x30' 'external-paste-as-new-image'
 Sta {param($path)$image=[Drawing.Image]::FromFile($path);$png=[IO.MemoryStream]::new([byte[]](137,80,78,71,13,10,26,10,0,0,0,0));try{$data=[Windows.Forms.DataObject]::new();$data.SetImage($image);$data.SetData('PNG',$false,$png);[Windows.Forms.Clipboard]::SetDataObject($data,$true)}finally{$png.Dispose();$image.Dispose()}} @($foreign)|Out-Null
 if((Formats) -notcontains 'PNG' -or (Formats) -notcontains 'Bitmap'){throw 'The fallback producer did not advertise both the corrupt PNG and valid Bitmap'}
 New-Image {Chord @(0x11,0x12) 0x4e} '40x30' 'corrupt-png-falls-back-to-bitmap' 0 @('40x30')
 Paste-In-Place;Settled
 try{Wait-Until {(Layers) -eq $count+1} 'Paste in Place did not paste the image from another application'}catch{throw "$_ formats=$((Formats) -join ',') notice=$((Model).state.notice|ConvertTo-Json -Compress -Depth 4) requests=$(Requests)"}
 if(!((Model).state.layer_tools.editing_layer.object)){throw 'The image from another application did not become an Object layer'}
 Chord @() 0x4f
 Wait-Until {((Model).state.commands|Where-Object id -eq 'move').selected -and ((Model).state.commands|Where-Object id -eq 'copy').enabled} 'Move did not target the pasted image'
 Copied {Chord @(0x11) 0x43} 'Copy did not deliver the selected image'
 New-Image {Chord @(0x11,0x12) 0x4e} (Clip-Size) 'structured-paste-as-new-image' 1 @('40x30')
 $positionSource=Join-Path $run 'Position source.capy';Save-ProjectAs $positionSource;$positionReference=Package $positionSource
 Paste-Position 'paste_at_view' 'Ctrl+Shift+V' @(0x11,0x10) $positionReference
 Paste-Position 'paste_at_cursor' 'Ctrl+Alt+V' @(0x11,0x12) $positionReference -Cursor
 Invoke 'Undo' -Name;Wait-Until {(Layers) -eq $count} 'One Undo did not remove the pasted image'
 $checks.foreign_image_paste_in_place='passed'
 $wide=Join-Path $run 'wide.png'
 $bitmap=[Drawing.Bitmap]::new(80,20);try{$g=[Drawing.Graphics]::FromImage($bitmap);$g.Clear([Drawing.Color]::FromArgb(255,210,50,30));$g.Dispose();$bitmap.Save($wide,[Drawing.Imaging.ImageFormat]::Png)}finally{$bitmap.Dispose()}
 Sta {param($first,$second)$files=[Collections.Specialized.StringCollection]::new();$null=$files.Add($first);$null=$files.Add($second);[Windows.Forms.Clipboard]::SetFileDropList($files)} @($foreign,$wide)|Out-Null
 New-Image {Chord @(0x11,0x12) 0x4e} '80x30' 'external-batch-as-new-image' 2 @('40x30','80x20')
 (Control 'drawing-canvas').SetFocus();[CapyRowPointer]::Key([uint32]$review.Id,0x42)
 Wait-Until {((Model).state.commands|Where-Object id -eq 'brush').selected -and @((Model).state.tool_settings).Count} 'B did not return to the brush after the image paste'
 $tabSource=(Model).windows_tabs.selected;$tabIdentity=Document-Identity;$tabCount=@((Model).windows_tabs.tabs).Count
 & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'File';Invoke-Id 'new_document'
 Invoke 'Create' -Name -Within (Control 'document-dialog')
 Wait-Until {@((Model).windows_tabs.tabs).Count -eq $tabCount+1 -and (Model).windows_tabs.selected -ne $tabSource -and (Requests) -eq 0 -and (Model).brush_ready} 'The tab clipboard review did not create a second drawing' 90
 Invoke 'Test stroke' -Name
 Wait-Until {(Model).state.document_file.modified} 'The tab clipboard review stroke did not reach the drawing'
 Save-ProjectAs (Join-Path $run 'Tab clipboard.capy')
 $tabCopyIdentity=Document-Identity
 Copied {Chord @(0x11) 0x43} 'The tab clipboard review could not copy its drawing'
 $tabExtent=Clip-Size
 $nativeTab=$null
 foreach($id in @(('drawing-tab-'+(Model).windows_tabs.selected),'drawing-selector')){$candidate=Find $id;if($candidate -and !$candidate.Current.IsOffscreen -and $candidate.Current.IsEnabled){$nativeTab=$candidate;break}}
 if(!$nativeTab){throw 'The drawing tabs did not expose a visible native keyboard target'}
 Copied {$nativeTab.SetFocus();Wait-Until {$nativeTab.Current.HasKeyboardFocus} 'The drawing tab did not take focus';[CapyRowPointer]::Chord([uint32]$review.Id,@(0x11),0x43)} 'A focused drawing tab blocked artwork Copy'
 if((Clip-Size) -ne $tabExtent -or (Document-Identity) -ne $tabCopyIdentity){throw 'Copy from a focused drawing tab changed the artwork or clipboard pixels'}
 $checks.drawing_tabs_keep_artwork_copy='passed'
 & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'File';Invoke-Id 'close_document'
 Wait-Until {@((Model).windows_tabs.tabs).Count -eq $tabCount -and (Model).windows_tabs.selected -eq $tabSource -and (Model).brush_ready} 'The tab clipboard review did not return to the original drawing' 90
 if((Document-Identity) -ne $tabIdentity){throw 'Clipboard keys on a drawing tab changed the original artwork'}
 & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'File';Invoke-Id 'new_document'
 $newDialog=Control 'document-dialog'
 Native-Clipboard-NoEffect (Control 'Cancel' -Name -Within $newDialog) 'modal_button_keeps_native_clipboard_keys'
 Invoke 'Cancel' -Name -Within $newDialog
 Wait-Until {!(Find 'document-dialog') -and (Requests) -eq 0} 'Cancel did not close the new drawing dialog'
 Sta {[Windows.Forms.Clipboard]::SetText('clipboard sentinel')}|Out-Null
 $entry=Control ('tool-setting-'+@((Model).state.tool_settings)[0].id)
 $entry.SetFocus();Wait-Until {$entry.Current.HasKeyboardFocus} 'The brush size field did not take focus'
 [CapyRowPointer]::Chord([uint32]$review.Id,@(0x11),0x41);[CapyRowPointer]::Chord([uint32]$review.Id,@(0x11),0x43);Start-Sleep -Milliseconds 400
 $text=[string](Sta {[Windows.Forms.Clipboard]::GetText()})
 if((Requests) -ne 0 -or $text -eq 'clipboard sentinel' -or (Formats) -contains 'art.capycanvas.clip.nonce'){throw "A focused text field lost Ctrl+C to the canvas: $text"}
 $checks.text_field_keeps_ctrl_c=$text
 $textIdentity=Document-Identity;$textTab=(Model).windows_tabs.selected;$textTabs=@((Model).windows_tabs.tabs).Count
 $textValue=$entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern);$originalText=$textValue.Current.Value
 if([string]::IsNullOrEmpty($originalText)){throw 'Native text shortcut proof requires selected field text'}
 Sta {[Windows.Forms.Clipboard]::SetText('awaiting Insert copy')}|Out-Null
 $entry.SetFocus();Wait-Until {$entry.Current.HasKeyboardFocus} 'The native field did not retain text focus'
 [CapyRowPointer]::Chord([uint32]$review.Id,@(0x11),0x41)
 [CapyRowPointer]::Chord([uint32]$review.Id,@(0x11),0x2d)
 Wait-Until {[string](Sta {[Windows.Forms.Clipboard]::GetText()}) -ceq $originalText} 'Ctrl+Insert did not copy the focused field text'
 if((Formats) -contains 'art.capycanvas.clip.nonce' -or (Requests) -ne 0 -or (Document-Identity) -ne $textIdentity){throw 'Ctrl+Insert escaped native text editing'}
 [CapyRowPointer]::Chord([uint32]$review.Id,@(0x10),0x2e)
 Wait-Until {$entry.Current.HasKeyboardFocus -and $textValue.Current.Value -eq '' -and [string](Sta {[Windows.Forms.Clipboard]::GetText()}) -ceq $originalText} 'Shift+Delete did not cut only the focused field text'
 if((Formats) -contains 'art.capycanvas.clip.nonce' -or (Requests) -ne 0 -or (Document-Identity) -ne $textIdentity){throw 'Shift+Delete escaped native text editing'}
 [CapyRowPointer]::Chord([uint32]$review.Id,@(0x10),0x2d)
 Wait-Until {$entry.Current.HasKeyboardFocus -and $textValue.Current.Value -ceq $originalText} 'Shift+Insert did not restore the cut native text'
 if((Requests) -ne 0 -or (Document-Identity) -ne $textIdentity -or (Model).windows_tabs.selected -ne $textTab -or @((Model).windows_tabs.tabs).Count -ne $textTabs){throw 'Insert clipboard keys in native text changed the artwork'}
 $checks.native_text_insert_keys='Ctrl+Insert copies selected text; Shift+Delete cuts it; Shift+Insert restores it with no artwork request or change'
 Close-Review
 $env:CAPY_STORAGE_DIR=Join-Path $run 'startup-profile'
 [IO.File]::WriteAllText((Settings-File),(@{language=@{Explicit='en'};theme=$Theme}|ConvertTo-Json -Depth 4))
 Start-Review 'startup'
 Sta {param($path)$image=[Drawing.Image]::FromFile($path);try{[Windows.Forms.Clipboard]::SetImage($image)}finally{$image.Dispose()}} @($foreign)|Out-Null
 Chord @(0x11) 0x56
 Wait-Until {$tab=Active-Tab;$tab.width -eq 40 -and $tab.height -eq 30 -and (Requests) -eq 0 -and !(Model).state.document_file.busy -and (Model).brush_ready} 'Startup Paste did not use the clipboard dimensions' 90
 if(!(Model).state.document_file.modified -or (Model).state.document_file.location -or (Model).state.layer_tools.tool -eq 'transform'){throw 'Startup Paste did not open an unsaved drawing without placement handles'}
 Capture "startup-paste-$Theme" -WithModel
 Save-ProjectAs (Join-Path $run 'Startup paste.capy')
 $source=(Model).windows_tabs.selected;$tabCount=@((Model).windows_tabs.tabs).Count;$layerCount=Layers
 Copied {Chord @(0x11) 0x43} 'The newly pasted drawing could not be copied'
 Chord @(0x11) 0x56
 Wait-Until {(Layers) -eq $layerCount+1 -and (Requests) -eq 0} 'Ordinary Paste did not add to the saved drawing'
 if((Model).windows_tabs.selected -ne $source -or @((Model).windows_tabs.tabs).Count -ne $tabCount){throw 'Paste into the saved drawing opened another tab'}
 Invoke 'Undo' -Name;Wait-Until {(Layers) -eq $layerCount} 'Undo did not remove the pasted layer'
 $checks.startup_paste_and_saved_drawing='passed'
 Close-Review
 [PSCustomObject]$checks|ConvertTo-Json|Tee-Object -FilePath (Join-Path $run 'result.json')
}catch{
 if($review -and !$review.HasExited){try{Capture 'failure'}catch{}}
 [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{[CapyRowPointer]::Dispose();Exit-CapyEnvironment}
