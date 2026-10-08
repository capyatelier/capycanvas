param(
    [Parameter(Mandatory)][string]$PortableResultFile,
    [ValidateNotNullOrEmpty()][string]$Publisher='CN=25C3FE75-9C78-42B1-91A5-2D7CD498E9E7',
    [ValidatePattern('^[A-Za-z0-9][A-Za-z0-9.-]{2,49}$')][string]$IdentityName='CapyAtelier.CapyCanvas',
    [switch]$UnsignedTestIdentity,
    [switch]$AllowDirty
)
$ErrorActionPreference='Stop'
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
. (Join-Path $PSScriptRoot 'PortablePackage.ps1')
$languages=@(Get-ShippedLanguages $repo)
$generators=@('apps/layer-windows/scripts/package-msix.ps1','apps/layer-windows/scripts/PortablePackage.ps1','apps/layer-windows/scripts/normalize-msix.ps1','apps/layer-windows/scripts/package-logos.ps1','apps/layer-web/icons/layer-zen-looking-up-symbolic.svg','crates/layer-ui/src/localization.rs','crates/layer-ui/src/localization_languages.rs')+@($languages|ForEach-Object {"assets/locales/$_/common.ftl"})
$packager=Get-PackagingSource $repo $generators -AllowDirty:$AllowDirty
$resources=($languages|ForEach-Object {'<Resource Language="'+[Security.SecurityElement]::Escape($_)+'" />'}) -join ''
$displaySuffix=''
if($Publisher.Contains('OID.2.25.311729368913984317654407730594956997722')){throw 'Use -UnsignedTestIdentity to request the isolated test publisher.'}
if($UnsignedTestIdentity){
    if($IdentityName.Length+5 -gt 50){throw 'The final MSIX identity name cannot exceed 50 characters.'}
    $IdentityName+='.Test'
    $Publisher+=', OID.2.25.311729368913984317654407730594956997722=1'
    $displaySuffix=' (MSIX Test)'
}
$portable=Read-PortablePackage $PortableResultFile
$manifest=$portable.manifest
if($manifest.version -notmatch '^[1-9][0-9]*\.[0-9]{1,3}\.[0-9]{1,3}$'){throw 'MSIX packages need a nonzero major version.'}
$Version=$manifest.version+'.0'
$sdk=Join-Path ([Environment]::GetFolderPath('ProgramFilesX86')) ('Windows Kits/10/bin/'+$manifest.windows_sdk+'/x64')
$makeappx=Join-Path $sdk 'makeappx.exe'
$makepri=Join-Path $sdk 'makepri.exe'
if(!(Test-Path -LiteralPath $makeappx) -or !(Test-Path -LiteralPath $makepri)){throw 'Install the Windows SDK version recorded by the portable build.'}
$run=Join-Path $repo ('artifacts/windows/msix/'+[Guid]::NewGuid().ToString('N'))
$payload=Join-Path $run 'CapyCanvas'
$escape={param([string]$value)[Security.SecurityElement]::Escape($value)}
$identity=& $escape $IdentityName;$publisherXml=& $escape $Publisher
$displayXml='ms-resource:Resources/AppName'
$xml=@"
<?xml version="1.0" encoding="utf-8"?>
<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"
 xmlns:uap="http://schemas.microsoft.com/appx/manifest/uap/windows10"
 xmlns:uap10="http://schemas.microsoft.com/appx/manifest/uap/windows10/10"
 xmlns:rescap="http://schemas.microsoft.com/appx/manifest/foundation/windows10/restrictedcapabilities"
 IgnorableNamespaces="uap uap10 rescap">
 <Identity Name="$identity" Publisher="$publisherXml" Version="$Version" ProcessorArchitecture="x64" />
 <Properties><DisplayName>$displayXml</DisplayName><PublisherDisplayName>Capy Atelier</PublisherDisplayName><Logo>PackageAssets\Logo50.png</Logo></Properties>
 <Resources>$resources</Resources>
 <Dependencies><TargetDeviceFamily Name="Windows.Desktop" MinVersion="10.0.22000.0" MaxVersionTested="10.0.26100.0" /></Dependencies>
 <Applications><Application Id="App" Executable="CapyCanvas.exe" uap10:RuntimeBehavior="packagedClassicApp" uap10:TrustLevel="mediumIL">
  <uap:VisualElements DisplayName="$displayXml" Description="Native drawing and painting" Square150x150Logo="PackageAssets\Logo150.png" Square44x44Logo="PackageAssets\Logo44.png" BackgroundColor="transparent" />
  <Extensions><uap:Extension Category="windows.fileTypeAssociation"><uap:FileTypeAssociation Name="capycanvas">
   <uap:DisplayName>Capy Canvas drawing</uap:DisplayName><uap:SupportedFileTypes><uap:FileType ContentType="application/vnd.capycanvas">.capy</uap:FileType></uap:SupportedFileTypes>
  </uap:FileTypeAssociation></uap:Extension></Extensions>
 </Application></Applications>
 <Capabilities><rescap:Capability Name="runFullTrust" /></Capabilities>
</Package>
"@
$utf8=[Text.UTF8Encoding]::new($false);$lf=[string][char]10
$readme="Capy Canvas for Windows`n`nThis MSIX contains the native app and its app-local runtimes.`nSee Notices for project, branding and dependency terms.`nSee package-manifest.json for the source commit and payload hashes.`n"
if($UnsignedTestIdentity){$readme+="This package uses a separate unsigned identity for local Windows 11 testing.`n"}else{$readme+="The MSIX must be signed by its publisher before distribution.`n"}
$packaging=[ordered]@{format='msix';source_commit=$packager.commit;development=$packager.dirty;payload_development=$manifest.development;identity=$IdentityName;publisher=$Publisher;version=$Version;unsigned_test_identity=[bool]$UnsignedTestIdentity;generators=$packager.generators}
Write-PackagedPayload $portable.source $payload $manifest $packaging {
    param($payload)
    & (Join-Path $PSScriptRoot 'package-logos.ps1') -Destination (Join-Path $payload 'PackageAssets')
    [IO.File]::WriteAllText((Join-Path $payload 'AppxManifest.xml'),$xml.Replace("`r`n",$lf)+$lf,$utf8)
    [IO.File]::WriteAllText((Join-Path $payload 'README.txt'),$readme,$utf8)
    $namesRoot=Join-Path $run 'AppNames'
    foreach($tag in $languages){
        $source=[IO.File]::ReadAllText((Join-Path $repo "assets/locales/$tag/common.ftl"))
        $name=[regex]::Match($source,'(?m)^common-app-name = ([^{}\r\n]+)$')
        if(!$name.Success){throw "$($tag): application name must be literal text."}
        $directory=Join-Path $namesRoot "Strings/$tag"
        [IO.Directory]::CreateDirectory($directory)|Out-Null
        $value=& $escape ($name.Groups[1].Value+$displaySuffix)
        [IO.File]::WriteAllText((Join-Path $directory 'Resources.resw'),"<?xml version=`"1.0`" encoding=`"utf-8`"?><root><data name=`"AppName`"><value>$value</value></data></root>"+$lf,$utf8)
    }
    $config=Join-Path $run 'pri-config.xml'
    & $makepri createconfig /cf $config /dq en /pv 10.0 /o *> (Join-Path $run 'pri-config.log')
    if($LASTEXITCODE -ne 0){throw "MakePRI configuration failed; inspect $run"}
    [xml]$priConfig=[IO.File]::ReadAllText($config)
    $packagingNode=$priConfig.SelectSingleNode('/resources/packaging')
    if($packagingNode){$packagingNode.ParentNode.RemoveChild($packagingNode)|Out-Null}
    $priConfig.Save($config)
    & $makepri new /pr $namesRoot /cf $config /mn (Join-Path $payload 'AppxManifest.xml') /of (Join-Path $payload 'resources.pri') /o *> (Join-Path $run 'pri-build.log')
    if($LASTEXITCODE -ne 0){throw "MakePRI indexing failed; inspect $run"}
}
$label='capycanvas-'+$manifest.version+'-windows-x64'+$(if($UnsignedTestIdentity){'-test'}else{''})
if($manifest.development){$label+='-development'}
$archive=Join-Path $run ($label+'.msix');$repeat=Join-Path $run 'repeat.msix'
foreach($path in @($archive,$repeat)){
    & $makeappx pack /d $payload /p $path /h SHA256 *> (Join-Path $run ([IO.Path]::GetFileName($path)+'.log'))
    if($LASTEXITCODE -ne 0){throw "MakeAppx validation failed; inspect $run"}
    & (Join-Path $PSScriptRoot 'normalize-msix.ps1') -Path $path
}
Assert-PackagingSourceUnchanged $repo $packager
$hash=(Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant()
if((Get-FileHash -LiteralPath $repeat -Algorithm SHA256).Hash.ToLowerInvariant() -ne $hash){throw 'Repeated MSIX assembly produced different bytes.'}
$report=[ordered]@{archive=$archive;sha256=$hash;payload=$payload;source_commit=$manifest.source_commit;development=$manifest.development;packaging_source_commit=$packager.commit;identity=$IdentityName;publisher=$Publisher;version=$Version;unsigned_test_identity=[bool]$UnsignedTestIdentity;signed=$false;repeat_archive='passed'}
[IO.File]::WriteAllText(($archive+'.sha256'),$hash+'  '+[IO.Path]::GetFileName($archive)+$lf,$utf8)
[IO.File]::WriteAllText((Join-Path $run 'result.json'),($report|ConvertTo-Json)+$lf,$utf8)
$report|ConvertTo-Json
