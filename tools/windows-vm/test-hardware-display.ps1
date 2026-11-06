$ErrorActionPreference='Stop'
$tokens=$null;$errors=$null
$ast=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot 'hardware-display.ps1'),[ref]$tokens,[ref]$errors)
if($errors.Count){throw ($errors|Out-String)}
$function=$ast.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'New-HardwareEdid'},$true)
. ([ScriptBlock]::Create($function.Extent.Text))
& {
 Add-Type -TypeDefinition @'
namespace CapyFixtureScaleTest {
 public static class Native {
  public static int Calls;public static string Source;public static int Current,Desired;
  public static object SetFixtureScale(string source,int current,int desired){
   Calls++;Source=source;Current=current;Desired=desired;
   return new{source=source,current_scale=current,target_scale=desired,restart_required=current!=desired};
  }
 }
}
'@
 function Get-ChildItem {throw 'Fixture DPI read registry topology'}
 function Get-ItemProperty {throw 'Fixture DPI read the registry'}
 function New-Item {throw 'Fixture DPI created a registry key'}
 function Set-ItemProperty {throw 'Fixture DPI wrote the registry'}
 $definition=$ast.Find({param($node)$node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Set-FixtureMonitorScale'},$true)
 if(!$definition){throw 'Missing actual fixture scale function'}
 . ([scriptblock]::Create($definition.Extent.Text.Replace('[CapyHardwareDisplay]','[CapyFixtureScaleTest.Native]')))
 $display=[pscustomobject]@{device='\\.\DISPLAY2';adapter='NVIDIA assigned';primary=$true;scalePercent=125}
 $result=Set-FixtureMonitorScale $display 150
 if([CapyFixtureScaleTest.Native]::Calls -ne 1 -or [CapyFixtureScaleTest.Native]::Source -cne $display.device -or [CapyFixtureScaleTest.Native]::Current -ne 125 -or [CapyFixtureScaleTest.Native]::Desired -ne 150){throw 'Fixture DPI did not delegate once to the exact NVIDIA source'}
 if($result.source -cne $display.device -or $result.target_scale -ne 150 -or !$result.restart_required){throw 'Fixture DPI touched registry topology or lost the native receipt'}
}
Add-Type -Path (Join-Path $PSScriptRoot 'hardware-display.cs')
foreach($case in @(@(125,150,0,-1,3,1),@(100,150,0,0,3,2),@(175,150,2,-3,4,1),@(100,150,-1,-1,3,1),@(150,150,-1,-2,3,-1))){
 if([CapyHardwareDisplay]::RelativeScale($case[0],$case[1],$case[2],$case[3],$case[4]) -ne $case[5]){throw 'Native source DPI did not preserve its relative scale index'}
}
foreach($case in @(@(125,150,3,-1,3),@(110,150,0,-1,3),@(125,110,0,-1,3),@(125,150,4,-1,3),@(125,150,[int]::MaxValue,0,[int]::MaxValue))){
 $failed=$false;try{$null=[CapyHardwareDisplay]::RelativeScale($case[0],$case[1],$case[2],$case[3],$case[4])}catch{$failed=$true}
 if(!$failed){throw 'Invalid native DPI request was accepted'}
}
foreach($case in @(@('OpenAdapter',80),@('CloseAdapter',4),@('DpiHeader',20),@('DpiGet',32),@('DpiSet',24))){
 $type=[CapyHardwareDisplay].GetNestedType($case[0])
 if([Runtime.InteropServices.Marshal]::SizeOf([Activator]::CreateInstance($type)) -ne $case[1]){throw "Wrong native DPI packet size: $($case[0])"}
}
if([Runtime.InteropServices.Marshal]::OffsetOf([CapyHardwareDisplay+DpiHeader],'source').ToInt32() -ne 16 -or [Runtime.InteropServices.Marshal]::OffsetOf([CapyHardwareDisplay+DpiGet],'current').ToInt32() -ne 24){throw 'Wrong native DPI packet offsets'}
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
