param([Parameter(Mandatory)][string]$ResultFile)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName System.IO.Compression.FileSystem
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$result=Get-Content -LiteralPath (Resolve-Path -LiteralPath $ResultFile).Path -Raw|ConvertFrom-Json
$archive=(Resolve-Path -LiteralPath $result.archive).Path
if($result.signed -or (Get-FileHash -LiteralPath $archive).Hash -ne $result.sha256){throw 'Expected the unchanged unsigned MSIX result.'}
$run=Join-Path $repo ('artifacts/windows/msix-review/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$zip=[IO.Compression.ZipFile]::OpenRead($archive)
try {
    $names=[Collections.Generic.Dictionary[string,IO.Compression.ZipArchiveEntry]]::new([StringComparer]::OrdinalIgnoreCase)
    foreach($entry in $zip.Entries){
        $name=[Uri]::UnescapeDataString($entry.FullName)
        if(!$name -or $name -match '(^|/)\.\.?(/|$)|[:\\]|^/|/$' -or $names.ContainsKey($name)){throw 'Invalid or duplicate MSIX archive entry.'}
        $names.Add($name,$entry)
        if($entry.LastWriteTime.DateTime -ne [DateTime]::new(2000,1,1)){throw 'MSIX timestamp was not normalized.'}
    }
    if($names.ContainsKey('AppxSignature.p7x')){throw 'Do not run unsigned normalization tests on a signed package.'}
    $stream=$zip.GetEntry('package-manifest.json').Open();$reader=[IO.StreamReader]::new($stream)
    try{$manifest=$reader.ReadToEnd()|ConvertFrom-Json}finally{$reader.Dispose()}
    if($manifest.schema -ne 1 -or $manifest.packaging.format -ne 'msix' -or $manifest.source_commit -ne $result.source_commit -or $manifest.development -ne $result.development){throw 'MSIX result and inventory do not agree.'}
    $declared=[Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    foreach($file in $manifest.files){
        if(!$declared.Add($file.path) -or !$names.ContainsKey($file.path)){throw 'MSIX inventory entry is missing or duplicated.'}
        $entry=$names[$file.path];$stream=$entry.Open();$sha=[Security.Cryptography.SHA256]::Create()
        try{$hash=[BitConverter]::ToString($sha.ComputeHash($stream)).Replace('-','').ToLowerInvariant()}finally{$stream.Dispose();$sha.Dispose()}
        if($entry.Length -ne $file.bytes -or $hash -ne $file.sha256){throw "MSIX payload hash mismatch: $($file.path)"}
    }
    foreach($name in @('package-manifest.json','AppxBlockMap.xml','[Content_Types].xml')){
        if(!$names.ContainsKey($name) -or $declared.Contains($name)){throw 'Missing or conflicting MSIX footprint.'}
    }
    if($names.Count -ne $declared.Count+3){throw 'MSIX contains files outside its inventory and required footprints.'}
}finally{$zip.Dispose()}
$sdk=Join-Path ([Environment]::GetFolderPath('ProgramFilesX86')) ('Windows Kits/10/bin/'+$manifest.windows_sdk+'/x64')
$unpacked=Join-Path $run 'unpacked app with spaces'
& (Join-Path $sdk 'makeappx.exe') unpack /p $archive /d $unpacked *> (Join-Path $run 'unpack.log')
if($LASTEXITCODE -ne 0){throw "MakeAppx could not unpack the normalized archive: $run"}
foreach($file in $manifest.files){
    if((Get-FileHash -LiteralPath (Join-Path $unpacked $file.path)).Hash -ne $file.sha256){throw "MakeAppx extraction changed $($file.path)"}
}
[xml]$appx=Get-Content -LiteralPath (Join-Path $unpacked 'AppxManifest.xml') -Raw
$identity=$appx.Package.Identity
if($identity.Name -cne $result.identity -or $identity.Publisher -cne $result.publisher -or $identity.Version -ne $result.version -or $identity.ProcessorArchitecture -ne 'x64'){throw 'Unexpected MSIX identity.'}
$application=$appx.Package.Applications.Application
$uap10='http://schemas.microsoft.com/appx/manifest/uap/windows10/10'
if($application.Executable -ne 'CapyCanvas.exe' -or $application.GetAttribute('RuntimeBehavior',$uap10) -ne 'packagedClassicApp' -or $application.GetAttribute('TrustLevel',$uap10) -ne 'mediumIL'){throw 'Unexpected MSIX activation contract.'}
if(@($application.Extensions.Extension.FileTypeAssociation.SupportedFileTypes.FileType) -notcontains '.capy'){throw 'MSIX does not associate .capy drawings.'}
$oid='OID.2.25.311729368913984317654407730594956997722=1'
if($identity.Publisher.Contains($oid) -ne [bool]$result.unsigned_test_identity){throw 'Unsigned test publisher isolation is incorrect.'}
$logos=Join-Path $run 'repeat logos'
& (Join-Path $PSScriptRoot 'package-logos.ps1') -Destination $logos
foreach($size in @(44,50,150)){
    $name="Logo$size.png"
    if((Get-FileHash -LiteralPath (Join-Path $logos $name)).Hash -ne (Get-FileHash -LiteralPath (Join-Path $unpacked ('PackageAssets/'+$name))).Hash){throw 'Repeated logo generation changed bytes.'}
}
$copy=Join-Path $run 'idempotence.msix'
Copy-Item -LiteralPath $archive -Destination $copy
& (Join-Path $PSScriptRoot 'normalize-msix.ps1') -Path $copy
if((Get-FileHash -LiteralPath $copy).Hash -ne $result.sha256){throw 'MSIX normalization is not idempotent.'}
# Independent .NET ZIP32 fixtures also check ordinary headers and that a late
# signature refusal leaves the entire input unchanged.
function New-Fixture([string]$Name,[bool]$Signed){
    $path=Join-Path $run $Name
    $fixture=[IO.Compression.ZipFile]::Open($path,[IO.Compression.ZipArchiveMode]::Create)
    try{
        foreach($entryName in @('payload.txt')+$(if($Signed){@('AppxSignature.p7x')}else{@()})){
            $entry=$fixture.CreateEntry($entryName);$stream=$entry.Open();$writer=[IO.StreamWriter]::new($stream)
            try{$writer.Write('normalization fixture')}finally{$writer.Dispose()}
        }
    }finally{$fixture.Dispose()}
    return $path
}
$plain=New-Fixture 'zip32.zip' $false
& (Join-Path $PSScriptRoot 'normalize-msix.ps1') -Path $plain
$fixture=[IO.Compression.ZipFile]::OpenRead($plain)
try{
    $entry=$fixture.GetEntry('payload.txt')
    if($entry.LastWriteTime.DateTime -ne [DateTime]::new(2000,1,1)){throw 'ZIP32 timestamp was not normalized.'}
    $reader=[IO.StreamReader]::new($entry.Open())
    try{if($reader.ReadToEnd() -cne 'normalization fixture'){throw 'ZIP32 payload was changed.'}}finally{$reader.Dispose()}
}finally{$fixture.Dispose()}
# Exercise a real Windows sharing violation, including a lock that never clears.
Add-Type -TypeDefinition '
using System;
using System.Threading;
using System.Threading.Tasks;
public static class CapyMsixLockFixture {
    public static Task ReleaseAfter(IDisposable stream, int milliseconds) {
        return Task.Run(() => { Thread.Sleep(milliseconds); stream.Dispose(); });
    }
}'
$transient=New-Fixture 'transient-lock.zip' $false
$held=[IO.File]::Open($transient,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::Read)
$release=[CapyMsixLockFixture]::ReleaseAfter($held,400)
$lockTimer=[Diagnostics.Stopwatch]::StartNew()
try{& (Join-Path $PSScriptRoot 'normalize-msix.ps1') -Path $transient}finally{$release.GetAwaiter().GetResult()}
if($lockTimer.Elapsed.TotalMilliseconds -lt 300 -or (Get-FileHash -LiteralPath $transient).Hash -ne (Get-FileHash -LiteralPath $plain).Hash){
    throw 'Transient sharing conflict did not wait and normalize to the same bytes.'
}
$locked=New-Fixture 'persistent-lock.zip' $false
$lockedHash=(Get-FileHash -LiteralPath $locked).Hash
$held=[IO.File]::Open($locked,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::Read)
$lockTimer.Restart();$rejected=$false
try{
    try{& (Join-Path $PSScriptRoot 'normalize-msix.ps1') -Path $locked}catch [IO.IOException]{
        if(($_.Exception.GetBaseException().HResult -band 0xffff) -notin @(32,33)){throw}
        $rejected=$true
    }
}finally{$held.Dispose()}
if(!$rejected -or $lockTimer.Elapsed.TotalSeconds -lt 5 -or $lockTimer.Elapsed.TotalSeconds -gt 8 -or
    (Get-FileHash -LiteralPath $locked).Hash -ne $lockedHash){throw 'Persistent sharing conflict did not fail within its bound without mutation.'}
$signed=New-Fixture 'signed-sentinel.zip' $true
$before=(Get-FileHash -LiteralPath $signed).Hash
$rejected=$false
try{& (Join-Path $PSScriptRoot 'normalize-msix.ps1') -Path $signed}catch{
    if($_.ToString() -notlike '*Refusing to alter a signed MSIX*'){throw}
    $rejected=$true
}
if(!$rejected -or (Get-FileHash -LiteralPath $signed).Hash -ne $before){throw 'Signature guard failed or modified the input.'}
# Deliberately invalid portable inventories must fail before any assembly.
$inputRoot=Join-Path $run 'invalid inputs'
$inputPayload=Join-Path $inputRoot 'payload'
[IO.Directory]::CreateDirectory($inputPayload)|Out-Null
[IO.File]::WriteAllText((Join-Path $inputPayload 'file.txt'),'package input fixture')
$inputFile=[ordered]@{path='file.txt';bytes=(Get-Item -LiteralPath (Join-Path $inputPayload 'file.txt')).Length;sha256=(Get-FileHash -LiteralPath (Join-Path $inputPayload 'file.txt')).Hash}
$inputManifest=[ordered]@{schema=1;architecture='x64';source_commit=$result.source_commit;files=@($inputFile)}
$inputResult=Join-Path $inputRoot 'result.json'
[ordered]@{payload=$inputPayload;source_commit=$result.source_commit}|ConvertTo-Json|Set-Content -LiteralPath $inputResult
function Reject-Input([string]$Expected,[hashtable]$Arguments=@{}){
    $inputManifest|ConvertTo-Json -Depth 5|Set-Content -LiteralPath (Join-Path $inputPayload 'package-manifest.json')
    $rejected=$false
    try{& (Join-Path $PSScriptRoot 'package-msix.ps1') -PortableResultFile $inputResult -AllowDirty @Arguments|Out-Null}catch{
        if($_.ToString() -notlike "*$Expected*"){throw}
        $rejected=$true
    }
    if(!$rejected){throw "Invalid MSIX input was accepted: $Expected"}
}
$validHash=$inputFile.sha256
$inputFile.sha256='0'*64
Reject-Input 'Portable payload hash mismatch'
$inputFile.sha256=$validHash
$inputManifest.files=@($inputFile,$inputFile)
Reject-Input 'Invalid or duplicate portable manifest path'
$inputManifest.files=@($inputFile)
$inputFile.path='../file.txt'
Reject-Input 'Invalid portable manifest path'
$inputFile.path='file.txt'
Reject-Input 'version components cannot exceed 65535' @{Version='1.0.65536.0'}
Reject-Input 'final MSIX identity name cannot exceed 50' @{IdentityName=('A'*50);UnsignedTestIdentity=$true}
[IO.File]::WriteAllText((Join-Path $inputPayload 'extra.txt'),'not in inventory')
Reject-Input 'Portable payload contains files absent from its manifest'
[ordered]@{
    archive=$archive;sha256=$result.sha256;source_commit=$result.source_commit;development=$result.development;
    archive_files=$names.Count;inventory='passed';makeappx_unpack='passed';activation_manifest='passed';
    repeat_logos='passed';normalization_idempotence='passed';zip32='passed';signed_refusal_without_mutation='passed';invalid_inputs='passed';
    transient_lock_retry='passed';persistent_lock_timeout_without_mutation='passed';
    scope='Archive validation only. Installed identity, launch, update, uninstall and distribution signing remain separate checks.'
}|ConvertTo-Json|Tee-Object -FilePath (Join-Path $run 'result.json')
