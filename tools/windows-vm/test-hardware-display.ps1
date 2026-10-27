$ErrorActionPreference='Stop'
$tokens=$null;$errors=$null
$ast=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot 'hardware-display.ps1'),[ref]$tokens,[ref]$errors)
if($errors.Count){throw ($errors|Out-String)}
$function=$ast.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'New-HardwareEdid'},$true)
. ([ScriptBlock]::Create($function.Extent.Text))
Add-Type -Path (Join-Path $PSScriptRoot 'hardware-display.cs')
$attached=[CapyHardwareDisplay+Device]::new();$attached.name='attached';$attached.description='NVIDIA assigned';$attached.flags=1
$inactive=[CapyHardwareDisplay+Device]::new();$inactive.name='inactive';$inactive.description='NVIDIA assigned';$inactive.flags=0
$selected=[CapyHardwareDisplay]::SelectSource(@($attached,$inactive))
if($selected.name -ne 'attached'){throw 'Selected an inactive display connector'}
foreach($size in @(@(1920,1080),@(2560,1600),@(1920,1200))){
 $edid=New-HardwareEdid $size[0] $size[1]
 if($edid.Length -ne 128 -or (($edid|Measure-Object -Sum).Sum%256) -ne 0){throw 'Invalid EDID block/checksum'}
 foreach($item in @(@(54,120),@(72,60))){
  $o=$item[0];$width=$edid[$o+2]+(($edid[$o+4] -shr 4)*256);$height=$edid[$o+5]+(($edid[$o+7] -shr 4)*256)
  $clock=($edid[$o]+256*$edid[$o+1])*10000
  $refresh=$clock/(($width+160)*($height+45))
  if($width -ne $size[0] -or $height -ne $size[1] -or [Math]::Abs($refresh-$item[1]) -gt .01){throw 'Wrong requested EDID timing'}
 }
 if($edid[111] -ne 253 -or $edid[114] -ne 120){throw 'Monitor range descriptor was corrupted'}
}
foreach($size in @(@(1280,720),@(4096,2160),@(4095,4095))){
 $failed=$false;try{$null=New-HardwareEdid $size[0] $size[1]}catch{$failed=$true}
 if(!$failed){throw 'Invalid EDID dimensions/clock accepted'}
}
'Hardware desktop parser, native declarations and EDID timing tests passed'
