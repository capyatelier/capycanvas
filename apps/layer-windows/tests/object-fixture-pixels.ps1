$ErrorActionPreference='Stop'
Add-Type -AssemblyName System.Drawing
. (Join-Path $PSScriptRoot '../scripts/ObjectFixture.ps1')
$manifest=[pscustomobject]@{root=@{ref='composition'};objects=@(
 @{id='composition';data=@{result=@{object=@{ref='stack'}}}},
 @{id='stack';type='capy.stack/1';data=@{entries=@(@{ref='layer'})}},
 @{id='layer';type='capy.occurrence/3';data=@{content=@{object=@{ref='object'}}}},
 @{id='object';type='capy.image-object/1';data=@{image=@{ref='image'};affine=@(1,1,-1,1,10,0)}},
 @{id='image';type='capy.image/1';data=@{extent=@(10,10)}}
)}
$projection=@{camera=@{zoom=1;rotation=0;flipped=@($false,$false);translation=@(0,0);viewport=@(100,100)};bounds=@{X=0;Y=0;Width=100;Height=100}}
$points=@(@(9,9),@(14,4),@(19,0),@(10,0),@(10,2),@(10,3))
if((@(Outline-Samples $manifest $projection $points) -join ',') -cne '1,3,4'){throw 'Outline samples lost segment distance, pixel centers or the 1.5 px band'}
$projection.bounds.X=12.25;$projection.bounds.Y=-4.5
$position=Screen-Position @(5.25,-2.1) $projection
if($position[0] -isnot [double] -or $position[1] -isnot [double] -or $position[0] -ne 17.5 -or [Math]::Abs($position[1]+6.6) -gt 1e-12){throw 'Screen position lost its unrounded doubles'}
if(((Screen-Point @(5.25,-2.1) $projection) -join ',') -cne '18,-7'){throw 'Screen point changed its integer rounding'}
$all=Pixel-Difference '0,0,0,0' '2,1,0,0'
$skipped=Pixel-Difference '0,0,0,0' '2,1,0,0' -Skip @(0)
if($all.max_channel_delta -ne 2 -or $all.changed_samples -ne 2 -or $all.sample_count -ne 4 -or $all.skipped_samples -ne 0){throw 'Unfiltered pixel comparison changed'}
if($skipped.max_channel_delta -ne 1 -or $skipped.changed_samples -ne 1 -or $skipped.sample_count -ne 4 -or $skipped.skipped_samples -ne 1){throw 'Skipped samples still affect the pixel comparison'}
'Object fixture outline and pixel comparisons passed'
