function Test-SourceDirty([string]$Repo) {
    & git -C $Repo diff --quiet HEAD --
    if($LASTEXITCODE -gt 1){throw 'Cannot inspect tracked packaging sources.'}
    $tracked=$LASTEXITCODE -eq 1
    $untracked=@(& git -C $Repo ls-files --others --exclude-standard)
    if($LASTEXITCODE -ne 0){throw 'Cannot inspect untracked packaging sources.'}
    return $tracked -or $untracked.Count -gt 0
}

function Get-PackagingSource([string]$Repo,[string[]]$Generators,[switch]$AllowDirty) {
    $commit=(& git -C $Repo rev-parse HEAD).Trim()
    if($LASTEXITCODE -ne 0){throw 'A Git checkout is required to identify the packager source.'}
    $dirty=Test-SourceDirty $Repo
    if($dirty -and !$AllowDirty){throw 'Commit packaging sources first, or use -AllowDirty for a development package.'}
    [pscustomobject]@{
        commit=$commit;dirty=$dirty
        generators=@(foreach($path in $Generators){[ordered]@{path=$path;sha256=(Get-FileHash -LiteralPath (Join-Path $Repo $path)).Hash.ToLowerInvariant()}})
    }
}

function Assert-PackagingSourceUnchanged([string]$Repo,$Source) {
    if((& git -C $Repo rev-parse HEAD).Trim() -ne $Source.commit -or (!$Source.dirty -and (Test-SourceDirty $Repo))){throw 'Packaging sources changed during assembly.'}
    foreach($generator in $Source.generators){if((Get-FileHash -LiteralPath (Join-Path $Repo $generator.path)).Hash -ne $generator.sha256){throw 'A packaging generator changed during assembly.'}}
}

function Get-ShippedLanguages([string]$Repo) {
    $inventory=[regex]::Match([IO.File]::ReadAllText((Join-Path $Repo 'crates/layer-ui/src/localization.rs')),'pub const SHIPPED_LANGUAGES[^;]+;').Value
    $tags=[ordered]@{}
    foreach($match in [regex]::Matches([IO.File]::ReadAllText((Join-Path $Repo 'crates/layer-ui/src/localization_languages.rs')),'\("(\w+)", "([^"\n]+)",')){$tags[$match.Groups[1].Value]=$match.Groups[2].Value}
    $names=@([regex]::Matches($inventory,'UiLanguage::(\w+)')|ForEach-Object {$_.Groups[1].Value})
    if(($names -join ',') -eq 'ALL'){$names=@($tags.Keys)}
    $languages=@($names|ForEach-Object {$tags[$_]})
    if(!$languages.Count -or $languages -contains $null){throw 'The shared shipping language inventory could not be read.'}
    $languages
}
# The payload of a portable release build, after checking every file against its manifest.
function Read-PortablePackage([string]$ResultFile) {
    $result=Get-Content -LiteralPath (Resolve-Path -LiteralPath $ResultFile).Path -Raw|ConvertFrom-Json
    $source=(Resolve-Path -LiteralPath $result.payload).Path
    $manifest=Get-Content -LiteralPath (Join-Path $source 'package-manifest.json') -Raw|ConvertFrom-Json
    if($manifest.schema -ne 1 -or $manifest.architecture -ne 'x64' -or !$manifest.release_identity -or $manifest.source_commit -ne $result.source_commit -or $manifest.packaging){throw 'Expected a portable x64 release package result and matching manifest.'}
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
    [pscustomobject]@{source=$source;manifest=$manifest}
}

# Copies the payload and records its new contents and the packaging step in its manifest.
function Write-PackagedPayload([string]$Source,[string]$Payload,$Manifest,$Packaging,[scriptblock]$Extend) {
    [IO.Directory]::CreateDirectory($Payload)|Out-Null
    foreach($item in Get-ChildItem -LiteralPath $Source -Force){Copy-Item -LiteralPath $item.FullName -Destination $Payload -Recurse}
    if($Extend){& $Extend $Payload}
    [string[]]$paths=@(Get-ChildItem -LiteralPath $Payload -File -Recurse -Force|Where-Object {$_.FullName -ne (Join-Path $Payload 'package-manifest.json')}|ForEach-Object {$_.FullName.Substring($Payload.Length+1).Replace('\','/')})
    [Array]::Sort($paths,[StringComparer]::Ordinal)
    $Manifest.files=@(foreach($path in $paths){$file=Get-Item -LiteralPath (Join-Path $Payload $path);[ordered]@{path=$path;bytes=$file.Length;sha256=(Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant()}})
    $Manifest|Add-Member -NotePropertyName packaging -NotePropertyValue $Packaging
    $Manifest.development=[bool]($Manifest.development -or $Packaging.development)
    $utf8=[Text.UTF8Encoding]::new($false)
    [IO.File]::WriteAllText((Join-Path $Payload 'package-manifest.json'),($Manifest|ConvertTo-Json -Depth 10)+[char]10,$utf8)
    foreach($file in Get-ChildItem -LiteralPath $Payload -File -Recurse){$file.LastWriteTimeUtc=[DateTime]::new(2000,1,1,0,0,0,[DateTimeKind]::Utc)}
}
