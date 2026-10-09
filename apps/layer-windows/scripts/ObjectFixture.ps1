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
function Identity($Manifest){Json @($Manifest.objects|Where-Object type -in @('capy.image-object/1','capy.image/1','capy.occurrence/3','capy.stack/1')|Sort-Object id)}
function Root-Stack($Manifest){
 $composition=@($Manifest.objects|Where-Object id -eq $Manifest.root.ref)
 $stack=@($Manifest.objects|Where-Object id -eq $composition[0].data.result.object.ref)
 if($composition.Count -ne 1 -or $stack.Count -ne 1 -or $stack[0].type -ne 'capy.stack/1'){throw 'Image authoring requires an ordinary root layer stack'}
 $stack[0]
}
function Object-Layers($Manifest){@($Manifest.objects|Where-Object {$_.type -eq 'capy.occurrence/3' -and $_.data.content.objects})}
function Front-Layer($Manifest){
 $layers=@(Object-Layers $Manifest);$id=@((Root-Stack $Manifest).data.entries.ref|Where-Object {$_ -in $layers.id})[0]
 @($layers|Where-Object id -eq $id)[0]
}
function Front-Object($Manifest){
 $layer=Front-Layer $Manifest
 @($Manifest.objects|Where-Object id -eq $layer.data.content.objects.ref)[0]
}
function Effective-Affine($Manifest,$Layer){
 if($Layer.id -notin (Root-Stack $Manifest).data.entries.ref){throw 'Image authoring requires a root Object layer'}
 $object=@($Manifest.objects|Where-Object id -eq $Layer.data.content.objects.ref)
 if($object.Count -ne 1 -or $object[0].type -ne 'capy.image-object/1'){throw 'The Object layer must directly own one image object'}
 $affine=if($object[0].data.affine){@($object[0].data.affine)}else{@(1,0,0,1,0,0)}
 $offset=if($Layer.data.offset){$Layer.data.offset}else{@(0,0)}
 foreach($axis in 0..1){$affine[4+$axis]+=[double]$offset[$axis]};$affine
}
function Assert-ImageCopy($Before,$After,[string]$Operation){
 $original=@($Before.objects|Where-Object type -eq 'capy.image-object/1');$pasted=@($After.objects|Where-Object type -eq 'capy.image-object/1')
 $owners=@(Object-Layers $Before);$destinations=@(Object-Layers $After);$added=@($pasted|Where-Object id -NotIn $original.id);$addedLayers=@($destinations|Where-Object id -NotIn $owners.id)
 if($original.Count -ne 2 -or $owners.Count -ne 2 -or $pasted.Count -ne 3 -or $added.Count -ne 1 -or $destinations.Count -ne 3 -or $addedLayers.Count -ne 1 -or $addedLayers[0].data.content.objects.ref -ne $added[0].id){throw "$Operation did not add one independent Object layer"}
 if((Json @($After.objects|Where-Object type -eq 'capy.image/1'|Sort-Object id)) -ne (Json @($Before.objects|Where-Object type -eq 'capy.image/1'|Sort-Object id))){throw "$Operation changed source images or added a flattened capy.image record"}
 if((Json @($pasted|Where-Object id -In $original.id|Sort-Object id)) -ne (Json @($original|Sort-Object id)) -or (Json @($destinations|Where-Object id -In $owners.id|Sort-Object id)) -ne (Json @($owners|Sort-Object id))){throw "$Operation changed an original object or its layer"}
 $sourceLayer=Front-Layer $Before;$source=Front-Object $Before
 if($added[0].data.image.ref -ne $source.data.image.ref -or (Json (Effective-Affine $After $addedLayers[0])) -ne (Json (Effective-Affine $Before $sourceLayer))){throw "$Operation did not preserve the copied source and effective affine"}
 $expected=@(foreach($id in (Root-Stack $Before).data.entries.ref){if($id -eq $sourceLayer.id){$addedLayers[0].id};$id})
 if(((Root-Stack $After).data.entries.ref -join ',') -ne ($expected -join ',')){throw "$Operation did not preserve the sibling layer order"}
}
function Image-Point($Manifest,[double]$X,[double]$Y){
 $object=Front-Object $Manifest;$a=Effective-Affine $Manifest (Front-Layer $Manifest)
 $source=@($Manifest.objects|Where-Object id -eq $object.data.image.ref)[0]
 @(($a[0]*$source.data.extent[0]*$X+$a[2]*$source.data.extent[1]*$Y+$a[4]),($a[1]*$source.data.extent[0]*$X+$a[3]*$source.data.extent[1]*$Y+$a[5]))
}
function Screen-Point([double[]]$Point,$Projection){
 if(!$Projection){$Projection=@{camera=(Model).state.camera;bounds=(Control 'drawing-canvas' -Arranged).Current.BoundingRectangle}}
 $c=$Projection.camera;$b=$Projection.bounds
 $x=$Point[0]*$c.zoom*$(if($c.flipped[0]){-1}else{1});$y=$Point[1]*$c.zoom*$(if($c.flipped[1]){-1}else{1})
 @([int]($b.X+([Math]::Cos($c.rotation)*$x-[Math]::Sin($c.rotation)*$y+$c.translation[0])*$b.Width/$c.viewport[0]),[int]($b.Y+([Math]::Sin($c.rotation)*$x+[Math]::Cos($c.rotation)*$y+$c.translation[1])*$b.Height/$c.viewport[1]))
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
function Sample-ObjectArtwork($bounds,$samplePoints){
 $bitmap=[Drawing.Bitmap]::new([int]$bounds.Width,[int]$bounds.Height);$g=[Drawing.Graphics]::FromImage($bitmap)
 try{
  $g.CopyFromScreen([int]$bounds.X,[int]$bounds.Y,0,0,$bitmap.Size)
  (@(foreach($point in $samplePoints){$c=$bitmap.GetPixel([int]($point[0]-$bounds.X),[int]($point[1]-$bounds.Y));$c.ToArgb()}) -join ',')
 }finally{$g.Dispose();$bitmap.Dispose()}
}
function Assert-ObjectMotion($Before,$After,[string]$Mode){
 $old=Front-Object $Before;$edited=@($After.objects|Where-Object id -eq $old.id)
 if($edited.Count -ne 1){throw 'Object motion lost its saved image object'}
 $a=Effective-Affine $Before (Front-Layer $Before);$b=Effective-Affine $After (Front-Layer $After)
 if(!@(0..5|Where-Object {$a[$_] -ne $b[$_]}).Count){throw 'Object motion did not change its saved affine'}
 $normalized=(Json $After)|ConvertFrom-Json -Depth 100
 $object=@($normalized.objects|Where-Object id -eq $old.id)[0]
 if($old.data.PSObject.Properties['affine']){$object.data.affine=$old.data.affine}else{$object.data.PSObject.Properties.Remove('affine')}
 if((Identity $normalized) -ne (Identity $Before)){throw 'Object motion changed a source, other object, layer or order'}
 $linearChanged=@(0..3|Where-Object {$a[$_] -ne $b[$_]}).Count -gt 0
 if($Mode -eq 'move' -and $linearChanged){throw 'Object Move changed scale or rotation'}
 if($Mode -in @('scale','rotate') -and !$linearChanged){throw 'Object transform changed only its translation'}
 if($Mode -eq 'scale' -and ($b[0] -le 0 -or $b[3] -le 0 -or [Math]::Abs($b[1]) -gt .000001 -or [Math]::Abs($b[2]) -gt .000001)){throw 'Corner scale flipped or rotated the Object'}
 if($Mode -eq 'rotate'){
  foreach($i in @(0,2)){if([Math]::Abs(($a[$i]*$a[$i]+$a[$i+1]*$a[$i+1])-($b[$i]*$b[$i]+$b[$i+1]*$b[$i+1])) -gt .000001){throw 'Object Rotate changed its scale'}}
 }
}
