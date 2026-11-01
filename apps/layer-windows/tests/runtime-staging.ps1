param([Parameter(Mandatory)][string]$Output)
$ErrorActionPreference='Stop'
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
[xml]$packages=Get-Content (Join-Path $repo 'apps/layer-windows/packages.config')
$base=@($packages.packages.package|Where-Object id -eq 'Microsoft.WindowsAppSDK.Base')[0]
$vendor=Join-Path $repo ('artifacts/windows/packages/'+$base.id+'.'+$base.version+'/build/Microsoft.WindowsAppSDK.SelfContained.targets')
[xml]$vendorXml=Get-Content $vendor
[xml]$app=Get-Content (Join-Path $repo 'apps/layer-windows/CapyCanvas.vcxproj')
$namespace='http://schemas.microsoft.com/developer/msbuild/2003'
$vendorNs=[Xml.XmlNamespaceManager]::new($vendorXml.NameTable);$vendorNs.AddNamespace('m',$namespace)
$appNs=[Xml.XmlNamespaceManager]::new($app.NameTable);$appNs.AddNamespace('m',$namespace)
if(Test-Path -LiteralPath $Output){throw 'Runtime staging output already exists'}
[IO.Directory]::CreateDirectory($Output)|Out-Null
try{
 foreach($binary in @('main','LayerDrop')){
  $area=Join-Path $Output $binary;$out=Join-Path $area 'out'
  $componentA=Join-Path $area 'selected-a';$componentB=Join-Path $area 'selected-b'
  $files=@(@{component=$componentA;relative='Microsoft.ui.xaml.dll'},@{component=$componentB;relative='Microsoft.UI.Input.dll'},@{component=$componentA;relative='resources.pri'},@{component=$componentB;relative='fr/resources.dll.mui'},@{component=$componentB;relative='workloads.json'})
  foreach($file in $files){
   $file.source=Join-Path $file.component ('runtimes-framework/win-x64/native/'+$file.relative)
   $file.destination=Join-Path $out $file.relative
   [IO.Directory]::CreateDirectory((Split-Path $file.source))|Out-Null;[IO.Directory]::CreateDirectory((Split-Path $file.destination))|Out-Null
   [IO.File]::WriteAllText($file.source,('chosen|'+$file.relative));[IO.File]::WriteAllText($file.destination,('future|'+$file.relative))
   [IO.File]::SetLastWriteTimeUtc($file.source,[DateTime]::new(2026,9,1,0,0,0,[DateTimeKind]::Utc))
   [IO.File]::SetLastWriteTimeUtc($file.destination,[DateTime]::new(2026,10,22,0,0,0,[DateTimeKind]::Utc))
  }
  [IO.File]::WriteAllText((Join-Path $out 'owned-unrelated.txt'),'preserve')
  [xml]$project=@'
<Project xmlns="http://schemas.microsoft.com/developer/msbuild/2003">
 <PropertyGroup><WindowsAppSDKSelfContained>true</WindowsAppSDKSelfContained><NativePlatform>x64</NativePlatform><TargetPlatformVersion>10.0.26100.0</TargetPlatformVersion><CopyBuildOutputToOutputDirectory>false</CopyBuildOutputToOutputDirectory></PropertyGroup>
 <Import Project="$(MSBuildToolsPath)\Microsoft.Common.CurrentVersion.targets" />
 <Target Name="GetWindowsAppSDKNativePlatform" />
 <Target Name="Build" DependsOnTargets="AddMicrosoftWindowsAppSDKPayloadFilesFromComponents;CopyFilesToOutputDirectory" />
</Project>
'@
  $property=$project.CreateElement('PropertyGroup',$namespace)
  foreach($entry in @{OutDir=($out+'\');OutputPath=($out+'\');IntermediateOutputPath=((Join-Path $area 'obj')+'\')}.GetEnumerator()){$node=$project.CreateElement($entry.Key,$namespace);$node.InnerText=$entry.Value;$null=$property.AppendChild($node)}
  $null=$project.DocumentElement.PrependChild($property)
  $items=$project.CreateElement('ItemGroup',$namespace)
  foreach($component in @($componentA,$componentB)){$node=$project.CreateElement('WindowsAppSdkComponentPackages',$namespace);$node.SetAttribute('Include',$component+'\');$null=$items.AppendChild($node)}
  $null=$project.DocumentElement.AppendChild($items)
  $payload=$vendorXml.SelectSingleNode("m:Project/m:Target[@Name='AddMicrosoftWindowsAppSDKPayloadFilesFromComponents']",$vendorNs)
  if(!$payload){throw 'Selected SDK has no runtime payload discovery target'}
  $null=$project.DocumentElement.AppendChild($project.ImportNode($payload,$true))
  foreach($target in $app.SelectNodes("m:Project/m:Target[@Name='StageSelectedWindowsRuntime']",$appNs)){$null=$project.DocumentElement.AppendChild($project.ImportNode($target,$true))}
  $path=Join-Path $area 'runtime.proj';$project.Save($path)
  [IO.Directory]::CreateDirectory((Join-Path $area 'obj'))|Out-Null
  & msbuild $path /t:Build /nologo /v:minimal
  if($LASTEXITCODE){throw 'Runtime staging regression project failed'}
  foreach($file in $files){if((Get-FileHash $file.source).Hash -ne (Get-FileHash $file.destination).Hash){throw ('Selected older runtime bytes were not staged: '+$file.relative)}}
  if([IO.File]::ReadAllText((Join-Path $out 'owned-unrelated.txt')) -ne 'preserve'){throw 'Runtime staging changed an unrelated output'}
 }
}finally{Remove-Item -LiteralPath $Output -Recurse -Force}
'Selected older runtime payload replaces newer different output bytes in main and companion layouts'
