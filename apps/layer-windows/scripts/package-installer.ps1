param(
    [Parameter(Mandatory)][string]$PortableResultFile,
    [switch]$TestIdentity,
    [switch]$AllowDirty,
    [string[]]$SignArguments
)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName System.IO.Compression.FileSystem
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
. (Join-Path $PSScriptRoot 'PortablePackage.ps1')
$packager=Get-PackagingSource $repo @('apps/layer-windows/scripts/package-installer.ps1','apps/layer-windows/scripts/installer.nsi','apps/layer-windows/scripts/PortablePackage.ps1','apps/layer-windows/scripts/package-logos.ps1','apps/layer-web/icons/layer-zen-looking-up-symbolic.svg') -AllowDirty:$AllowDirty
$identity=if($TestIdentity){@{name='Capy Canvas Installer Test';key='CapyCanvasInstallerTest';progid='CapyAtelier.CapyCanvas.InstallerTest.capy'}}else{@{name='Capy Canvas';key='CapyCanvas';progid='CapyAtelier.CapyCanvas.capy'}}
$portable=Read-PortablePackage $PortableResultFile
$manifest=$portable.manifest
$Version=$manifest.version+'.0'

$nsisVersion='3.11'
$nsisArchive=Join-Path $repo "artifacts/windows/tools/nsis-$nsisVersion.zip"
$nsisSha256='c7d27f780ddb6cffb4730138cd1591e841f4b7edb155856901cdf5f214394fa1'
if(!(Test-Path -LiteralPath $nsisArchive)){
    [IO.Directory]::CreateDirectory((Split-Path -Parent $nsisArchive))|Out-Null
    Invoke-WebRequest "https://downloads.sourceforge.net/project/nsis/NSIS%203/$nsisVersion/nsis-$nsisVersion.zip" -OutFile ($nsisArchive+'.download') -UseBasicParsing -UserAgent 'Wget'
    Move-Item -LiteralPath ($nsisArchive+'.download') -Destination $nsisArchive
}
if((Get-FileHash -LiteralPath $nsisArchive -Algorithm SHA256).Hash -ne $nsisSha256){throw "Unexpected NSIS archive: $nsisArchive"}
$run=Join-Path $repo ('artifacts/windows/installer/'+[Guid]::NewGuid().ToString('N'))
[IO.Compression.ZipFile]::ExtractToDirectory($nsisArchive,(Join-Path $run 'tools'))
$makensis=Join-Path $run "tools/nsis-$nsisVersion/makensis.exe"

$payload=Join-Path $run 'CapyCanvas'
$utf8=[Text.UTF8Encoding]::new($false);$lf=[string][char]10
$readme=@'
Capy Canvas for Windows

The installer puts the app in AppData\Local\Programs\Capy Canvas for the current
user, adds it to the Start menu and opens .capy drawings with it. Preferences
and app data are kept in AppData\Roaming\CapyAtelier\CapyCanvas and
AppData\Local\CapyAtelier\CapyCanvas; uninstalling the app keeps them.

See Notices for project, branding, dependency and runtime terms. See
package-manifest.json for the source commit and payload hashes.
'@
$packaging=[ordered]@{format='installer';source_commit=$packager.commit;development=$packager.dirty;payload_development=$manifest.development;installer='nsis-'+$nsisVersion;identity=$identity.key;version=$Version;test_identity=[bool]$TestIdentity;generators=$packager.generators}
Write-PackagedPayload $portable.source $payload $manifest $packaging {
    param($payload)
    & (Join-Path $PSScriptRoot 'package-logos.ps1') -Destination (Join-Path $run 'logos') -Icon (Join-Path $payload 'CapyCanvas.ico')
    [IO.File]::WriteAllText((Join-Path $payload 'README.txt'),$readme.Replace(([string][char]13+[char]10),$lf)+$lf,$utf8)
}
$label='capycanvas-'+$manifest.version+'-windows-x64'+$(if($TestIdentity){'-installer-test'}else{'-setup'})
if($manifest.development){$label+='-development'}
$installer=Join-Path $run ($label+'.exe');$repeat=Join-Path $run 'repeat.exe'
function Build-Installer([string]$Path,[string[]]$Defines){
    & $makensis -V2 -NOCD "-DPAYLOAD=$payload" "-DOUTFILE=$Path" "-DVERSION=$Version" "-DNAME=$($identity.name)" "-DKEY=$($identity.key)" "-DPROGID=$($identity.progid)" @Defines (Join-Path $PSScriptRoot 'installer.nsi') *> (Join-Path $run ([IO.Path]::GetFileName($Path)+'.log'))
    if($LASTEXITCODE -ne 0){throw "NSIS failed; inspect $run"}
}
foreach($path in @($installer,$repeat)){Build-Installer $path @()}
Assert-PackagingSourceUnchanged $repo $packager
$hash=(Get-FileHash -LiteralPath $installer -Algorithm SHA256).Hash.ToLowerInvariant()
if((Get-FileHash -LiteralPath $repeat -Algorithm SHA256).Hash.ToLowerInvariant() -ne $hash){throw 'Repeated installer assembly produced different bytes.'}
Remove-Item -LiteralPath $repeat
if($SignArguments){
    $signer=Join-Path $run 'sign-uninstaller.cmd'
    [IO.File]::WriteAllText($signer,'@signtool sign '+(($SignArguments|ForEach-Object {'"'+$_+'"'}) -join ' ')+' %1'+[char]13+[char]10,$utf8)
    Build-Installer $installer @("-DSIGNER=$signer")
    & signtool sign @SignArguments $installer
    if($LASTEXITCODE -ne 0){throw 'Signing the setup program failed.'}
    $hash=(Get-FileHash -LiteralPath $installer -Algorithm SHA256).Hash.ToLowerInvariant()
}
$report=[ordered]@{installer=$installer;sha256=$hash;payload=$payload;source_commit=$manifest.source_commit;development=$manifest.development;packaging_source_commit=$packager.commit;name=$identity.name;key=$identity.key;progid=$identity.progid;version=$Version;test_identity=[bool]$TestIdentity;signed=[bool]$SignArguments;repeat_installer='passed'}
[IO.File]::WriteAllText(($installer+'.sha256'),$hash+'  '+[IO.Path]::GetFileName($installer)+$lf,$utf8)
[IO.File]::WriteAllText((Join-Path $run 'result.json'),($report|ConvertTo-Json)+$lf,$utf8)
$report|ConvertTo-Json
