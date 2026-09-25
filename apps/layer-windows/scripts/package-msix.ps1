param(
    [Parameter(Mandatory)][string]$PortableResultFile,
    [ValidatePattern('^[1-9][0-9]*\.[0-9]+\.[0-9]+\.[0-9]+$')][string]$Version='1.0.0.0',
    [ValidateNotNullOrEmpty()][string]$Publisher='CN=Capy Atelier',
    [ValidatePattern('^[A-Za-z0-9][A-Za-z0-9.-]{2,49}$')][string]$IdentityName='CapyAtelier.CapyCanvas',
    [switch]$UnsignedTestIdentity,
    [switch]$AllowDirty
)
$ErrorActionPreference='Stop'
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
function Source-IsDirty {
    & git -C $repo diff --quiet HEAD --
    if($LASTEXITCODE -gt 1){throw 'Cannot inspect tracked packaging sources.'}
    $tracked=$LASTEXITCODE -eq 1
    $untracked=@(& git -C $repo ls-files --others --exclude-standard)
    if($LASTEXITCODE -ne 0){throw 'Cannot inspect untracked packaging sources.'}
    return $tracked -or $untracked.Count -gt 0
}
$packagingCommit=(& git -C $repo rev-parse HEAD).Trim()
if($LASTEXITCODE -ne 0){throw 'A Git checkout is required to identify the packager source.'}
$packagingDirty=Source-IsDirty
if($packagingDirty -and !$AllowDirty){throw 'Commit packaging sources first, or use -AllowDirty for a development package.'}
$generatorPaths=@('apps/layer-windows/scripts/package-msix.ps1','apps/layer-windows/scripts/normalize-msix.ps1','apps/layer-windows/scripts/package-logos.ps1','apps/layer-web/icons/layer-zen-looking-up-symbolic.svg')
$generators=@(foreach($path in $generatorPaths){[ordered]@{path=$path;sha256=(Get-FileHash -LiteralPath (Join-Path $repo $path)).Hash.ToLowerInvariant()}})
$displayName='Capy Canvas'
if($Publisher.Contains('OID.2.25.311729368913984317654407730594956997722')){throw 'Use -UnsignedTestIdentity to request the isolated test publisher.'}
if($UnsignedTestIdentity){
    if($IdentityName.Length+5 -gt 50){throw 'The final MSIX identity name cannot exceed 50 characters.'}
    $IdentityName+='.Test'
    $Publisher+=', OID.2.25.311729368913984317654407730594956997722=1'
    $displayName+=' (MSIX Test)'
}
$result=Get-Content -LiteralPath (Resolve-Path -LiteralPath $PortableResultFile).Path -Raw|ConvertFrom-Json
$source=(Resolve-Path -LiteralPath $result.payload).Path
$manifest=Get-Content -LiteralPath (Join-Path $source 'package-manifest.json') -Raw|ConvertFrom-Json
if($manifest.schema -ne 1 -or $manifest.architecture -ne 'x64' -or $manifest.source_commit -ne $result.source_commit -or $manifest.packaging){throw 'Expected a portable x64 package result and matching manifest.'}
$parsedVersion=[Version]::Parse($Version)
foreach($part in @($parsedVersion.Major,$parsedVersion.Minor,$parsedVersion.Build,$parsedVersion.Revision)){if($part -gt 65535){throw 'MSIX version components cannot exceed 65535.'}}
$declared=[Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
$prefix=$source+[IO.Path]::DirectorySeparatorChar
$entries=@(Get-ChildItem -LiteralPath $source -Recurse -Force)
if($entries|Where-Object {$_.Attributes -band [IO.FileAttributes]::ReparsePoint}){throw 'Portable payload must not contain reparse points.'}
foreach($file in $manifest.files){
    if(!$file.path -or $file.path -match '(^|/)\.\.?(/|$)|[:\\]|^/|/$' -or $file.path -ieq 'package-manifest.json'){throw 'Invalid portable manifest path.'}
    $path=[IO.Path]::GetFullPath((Join-Path $source $file.path))
    if(!$path.StartsWith($prefix,[StringComparison]::OrdinalIgnoreCase) -or !$declared.Add($file.path)){throw 'Invalid or duplicate portable manifest path.'}
    if((Get-Item -LiteralPath $path).Length -ne $file.bytes -or (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash -ne $file.sha256){throw "Portable payload hash mismatch: $($file.path)"}
}
if(@($entries|Where-Object {!$_.PSIsContainer}).Count -ne $declared.Count+1){throw 'Portable payload contains files absent from its manifest.'}
$sdk=Join-Path ([Environment]::GetFolderPath('ProgramFilesX86')) ('Windows Kits/10/bin/'+$manifest.windows_sdk+'/x64')
$makeappx=Join-Path $sdk 'makeappx.exe'
if(!(Test-Path -LiteralPath $makeappx)){throw 'Install the Windows SDK version recorded by the portable build.'}
$run=Join-Path $repo ('artifacts/windows/msix/'+[Guid]::NewGuid().ToString('N'))
$payload=Join-Path $run 'CapyCanvas'
[IO.Directory]::CreateDirectory($payload)|Out-Null
foreach($item in Get-ChildItem -LiteralPath $source -Force){Copy-Item -LiteralPath $item.FullName -Destination $payload -Recurse}
& (Join-Path $PSScriptRoot 'package-logos.ps1') -Destination (Join-Path $payload 'PackageAssets')
$escape={param([string]$value)[Security.SecurityElement]::Escape($value)}
$identity=& $escape $IdentityName;$publisherXml=& $escape $Publisher
$displayXml=& $escape $displayName
$xml=@"
<?xml version="1.0" encoding="utf-8"?>
<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"
 xmlns:uap="http://schemas.microsoft.com/appx/manifest/uap/windows10"
 xmlns:uap10="http://schemas.microsoft.com/appx/manifest/uap/windows10/10"
 xmlns:rescap="http://schemas.microsoft.com/appx/manifest/foundation/windows10/restrictedcapabilities"
 IgnorableNamespaces="uap uap10 rescap">
 <Identity Name="$identity" Publisher="$publisherXml" Version="$Version" ProcessorArchitecture="x64" />
 <Properties><DisplayName>$displayXml</DisplayName><PublisherDisplayName>Capy Atelier</PublisherDisplayName><Logo>PackageAssets\Logo50.png</Logo></Properties>
 <Resources><Resource Language="en-US" /></Resources>
 <Dependencies><TargetDeviceFamily Name="Windows.Desktop" MinVersion="10.0.22000.0" MaxVersionTested="10.0.26100.0" /></Dependencies>
 <Applications><Application Id="App" Executable="CapyCanvas.exe" uap10:RuntimeBehavior="packagedClassicApp" uap10:TrustLevel="mediumIL">
  <uap:VisualElements DisplayName="$displayXml" Description="Native drawing and painting" Square150x150Logo="PackageAssets\Logo150.png" Square44x44Logo="PackageAssets\Logo44.png" BackgroundColor="transparent" />
  <Extensions><uap:Extension Category="windows.fileTypeAssociation"><uap:FileTypeAssociation Name="capycanvas">
   <uap:DisplayName>Capy Canvas drawing</uap:DisplayName><uap:SupportedFileTypes><uap:FileType>.capy</uap:FileType></uap:SupportedFileTypes>
  </uap:FileTypeAssociation></uap:Extension></Extensions>
 </Application></Applications>
 <Capabilities><rescap:Capability Name="runFullTrust" /></Capabilities>
</Package>
"@
$utf8=[Text.UTF8Encoding]::new($false);$lf=[string][char]10
[IO.File]::WriteAllText((Join-Path $payload 'AppxManifest.xml'),$xml.Replace("`r`n",$lf)+$lf,$utf8)
# Refresh inventory because the installed package has its own manifest and logos.
$readme="Capy Canvas for Windows`n`nThis MSIX contains the native app and its app-local runtimes.`nSee Notices for project, branding and dependency terms.`nSee package-manifest.json for the source commit and payload hashes.`n"
if($UnsignedTestIdentity){$readme+="This package uses a separate unsigned identity for local Windows 11 testing.`n"}else{$readme+="The MSIX must be signed by its publisher before distribution.`n"}
[IO.File]::WriteAllText((Join-Path $payload 'README.txt'),$readme,$utf8)
[string[]]$paths=@(Get-ChildItem -LiteralPath $payload -File -Recurse -Force|Where-Object {$_.FullName -ne (Join-Path $payload 'package-manifest.json')}|ForEach-Object {$_.FullName.Substring($payload.Length+1).Replace('\','/')})
[Array]::Sort($paths,[StringComparer]::Ordinal)
$manifest.files=@(foreach($path in $paths){$file=Get-Item -LiteralPath (Join-Path $payload $path);[ordered]@{path=$path;bytes=$file.Length;sha256=(Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant()}})
$manifest|Add-Member -NotePropertyName packaging -NotePropertyValue ([ordered]@{format='msix';source_commit=$packagingCommit;development=$packagingDirty;payload_development=$manifest.development;identity=$IdentityName;publisher=$Publisher;version=$Version;unsigned_test_identity=[bool]$UnsignedTestIdentity;generators=$generators})
$manifest.development=[bool]($manifest.development -or $packagingDirty)
[IO.File]::WriteAllText((Join-Path $payload 'package-manifest.json'),($manifest|ConvertTo-Json -Depth 10)+$lf,$utf8)
foreach($file in Get-ChildItem -LiteralPath $payload -File -Recurse){$file.LastWriteTimeUtc=[DateTime]::new(2000,1,1,0,0,0,[DateTimeKind]::Utc)}
$label='CapyCanvas-windows-x64-'+$manifest.source_commit.Substring(0,12)+'-'+$Version+$(if($UnsignedTestIdentity){'-test'}else{'-unsigned'})
if($manifest.development){$label+='-development'}
$archive=Join-Path $run ($label+'.msix');$repeat=Join-Path $run 'repeat.msix'
foreach($path in @($archive,$repeat)){
    & $makeappx pack /d $payload /p $path /h SHA256 *> (Join-Path $run ([IO.Path]::GetFileName($path)+'.log'))
    if($LASTEXITCODE -ne 0){throw "MakeAppx validation failed; inspect $run"}
    & (Join-Path $PSScriptRoot 'normalize-msix.ps1') -Path $path
}
if((& git -C $repo rev-parse HEAD).Trim() -ne $packagingCommit -or (!$packagingDirty -and (Source-IsDirty))){throw 'Packaging sources changed during assembly.'}
foreach($generator in $generators){if((Get-FileHash -LiteralPath (Join-Path $repo $generator.path)).Hash -ne $generator.sha256){throw 'An MSIX generator changed during assembly.'}}
$hash=(Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant()
if((Get-FileHash -LiteralPath $repeat -Algorithm SHA256).Hash.ToLowerInvariant() -ne $hash){throw 'Repeated MSIX assembly produced different bytes.'}
$report=[ordered]@{archive=$archive;sha256=$hash;payload=$payload;source_commit=$manifest.source_commit;development=$manifest.development;packaging_source_commit=$packagingCommit;identity=$IdentityName;publisher=$Publisher;version=$Version;unsigned_test_identity=[bool]$UnsignedTestIdentity;signed=$false;repeat_archive='passed'}
[IO.File]::WriteAllText(($archive+'.sha256'),$hash+'  '+[IO.Path]::GetFileName($archive)+$lf,$utf8)
[IO.File]::WriteAllText((Join-Path $run 'result.json'),($report|ConvertTo-Json)+$lf,$utf8)
$report|ConvertTo-Json
