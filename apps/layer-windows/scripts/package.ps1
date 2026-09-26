param([string]$OutputRoot,[switch]$SkipRestore,[switch]$AllowDirty)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName System.IO.Compression.FileSystem
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
if(!$OutputRoot){$OutputRoot=Join-Path $repo 'artifacts/windows/distribution'}
$OutputRoot=[IO.Path]::GetFullPath($OutputRoot)
$artifactRoot=[IO.Path]::GetFullPath((Join-Path $repo 'artifacts/windows'))+[IO.Path]::DirectorySeparatorChar
if(!$OutputRoot.StartsWith($artifactRoot,[StringComparison]::OrdinalIgnoreCase)){throw 'Package output must stay under this checkout artifacts/windows directory.'}
Push-Location $repo
try {
    $commit=(& git rev-parse HEAD).Trim()
    if($LASTEXITCODE -ne 0){throw 'A Git checkout is required to identify the package source.'}
    function Source-IsDirty {
        & git diff --quiet HEAD --
        if($LASTEXITCODE -gt 1){throw 'Cannot inspect tracked package sources.'}
        $tracked=$LASTEXITCODE -eq 1
        $untracked=@(& git ls-files --others --exclude-standard)
        if($LASTEXITCODE -ne 0){throw 'Cannot inspect untracked package sources.'}
        return $tracked -or $untracked.Count -gt 0
    }
    $dirty=Source-IsDirty
    if($dirty -and !$AllowDirty){throw 'Commit or preserve source changes first. -AllowDirty creates an explicitly marked development package.'}
    $run=Join-Path $OutputRoot ([Guid]::NewGuid().ToString('N'))
    $build=Join-Path $run 'build'
    $payload=Join-Path $run 'CapyCanvas'
    [IO.Directory]::CreateDirectory($build)|Out-Null
    [IO.Directory]::CreateDirectory($payload)|Out-Null
    # Fresh directories cannot contain old diagnostics, profiles or staged assets.
    & (Join-Path $PSScriptRoot 'build.ps1') -Configuration Release -OutputDirectory $build -SkipRestore:$SkipRestore *> (Join-Path $run 'build.log')
    if($LASTEXITCODE -ne 0){throw "Package build failed; inspect $run/build.log"}
    $vswhere=Join-Path ([Environment]::GetFolderPath('ProgramFilesX86')) 'Microsoft Visual Studio/Installer/vswhere.exe'
    $installation=(& $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath).Trim()
    $redistRoot=$env:VCToolsRedistDir
    if(!$redistRoot){throw 'The selected build toolchain did not identify its redistributable runtime.'}
    $crt=@(Get-ChildItem -LiteralPath (Join-Path $redistRoot 'x64') -Directory|Where-Object Name -Match '^Microsoft\.VC\d+\.CRT$')
    if($crt.Count -ne 1){throw 'Cannot identify one x64 Visual C++ CRT directory.'}
    $crtFiles=@(Get-ChildItem -LiteralPath $crt[0].FullName -Filter '*.dll' -File)
    if(!($crtFiles.Name -contains 'vcruntime140.dll') -or !($crtFiles.Name -contains 'msvcp140.dll')){throw 'Required redistributable CRT binaries are missing.'}
    foreach($file in $crtFiles){
        if($file.Length -lt 1024){throw "Invalid redistributable binary: $($file.Name)"}
        Copy-Item -LiteralPath $file.FullName -Destination (Join-Path $payload $file.Name)
    }
    # Include localized resources and workload manifests, excluding compiler products.
    foreach($file in Get-ChildItem -LiteralPath $build -File -Recurse){
        if($file.Extension -in @('.pdb','.ilk','.lib','.exp','.obj','.pch')){continue}
        $relative=$file.FullName.Substring($build.Length+1)
        if($relative -match '(^|[\\/])(ui-state|camera-state|canvas-state|windows-)|\.(log|sqlite3|dmp)$'){throw "Private diagnostic in clean build output: $relative"}
        $target=Join-Path $payload $relative
        [IO.Directory]::CreateDirectory((Split-Path -Parent $target))|Out-Null
        Copy-Item -LiteralPath $file.FullName -Destination $target
    }
    foreach($name in @('CapyCanvas.exe','layer_windows.dll','CapyCanvas.pri','Microsoft.UI.Xaml.dll','Microsoft.WindowsAppRuntime.dll')){
        if(!(Test-Path -LiteralPath (Join-Path $payload $name) -PathType Leaf)){throw "Incomplete runtime payload: $name"}
    }
    & (Join-Path $PSScriptRoot 'collect-package-notices.ps1') -Destination (Join-Path $payload 'Notices') -PackagesDirectory (Join-Path $repo 'artifacts/windows/packages') -VisualStudio $installation
    $utf8=[Text.UTF8Encoding]::new($false);$lf=[string][char]10
    $readme=@'
Capy Canvas for Windows

Extract the entire ZIP to a folder, then run CapyCanvas.exe.
Keep the files and subfolders together. Windows 11 x64 is required.
The Windows App SDK and Visual C++ runtimes are included beside the app.
Preferences and workspace data use the normal per-user application directory.

This package is unsigned. Its manifest identifies the source commit and whether
uncommitted development changes were included. See Notices for project,
branding, dependency and runtime terms. See package-manifest.json for all file
sizes and SHA-256 hashes.
'@
    [IO.File]::WriteAllText((Join-Path $payload 'README.txt'),$readme.Replace(([string][char]13+[char]10),$lf)+$lf,$utf8)
    [string[]]$paths=@(Get-ChildItem -LiteralPath $payload -File -Recurse|ForEach-Object {$_.FullName.Substring($payload.Length+1).Replace('\','/')})
    [Array]::Sort($paths,[StringComparer]::Ordinal)
    $files=@(foreach($path in $paths){
        $file=Get-Item -LiteralPath (Join-Path $payload $path)
        [ordered]@{path=$path;bytes=$file.Length;sha256=(Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant()}
    })
    [xml]$config=Get-Content (Join-Path $repo 'apps/layer-windows/packages.config') -Raw
    $nuget=@($config.packages.package|ForEach-Object {[ordered]@{name=$_.id;version=$_.version}})
    if((& git rev-parse HEAD).Trim() -ne $commit -or (!$dirty -and (Source-IsDirty))){throw 'Package sources changed during the build; retry from a stable checkout.'}
    $manifest=[ordered]@{
        schema=1;application='Capy Canvas';source_commit=$commit;development=$dirty;architecture='x64';minimum_windows='11';
        rust=(& rustc --version).Trim();visual_cpp_tools=$env:VCToolsVersion;windows_sdk=$env:WindowsSDKVersion.TrimEnd('\');
        cargo_lock_sha256=(Get-FileHash -LiteralPath (Join-Path $repo 'Cargo.lock') -Algorithm SHA256).Hash.ToLowerInvariant();
        nuget=$nuget;files=$files
    }
    [IO.File]::WriteAllText((Join-Path $payload 'package-manifest.json'),($manifest|ConvertTo-Json -Depth 8)+$lf,$utf8)
    # Reproducible archive assembly for identical payload bytes, independent of
    # directory enumeration order and source file modification timestamps.
    # This does not claim bit-identical rebuilds across toolchain installations.
    function Write-Archive([string]$Path) {
        $stream=[IO.File]::Open($Path,[IO.FileMode]::CreateNew)
        $zip=[IO.Compression.ZipArchive]::new($stream,[IO.Compression.ZipArchiveMode]::Create,$false)
        try{
            [string[]]$entries=@(Get-ChildItem -LiteralPath $payload -File -Recurse|ForEach-Object {$_.FullName.Substring($payload.Length+1).Replace('\','/')})
            [Array]::Sort($entries,[StringComparer]::Ordinal)
            foreach($relative in $entries){
                $entry=$zip.CreateEntry('CapyCanvas/'+$relative,[IO.Compression.CompressionLevel]::Optimal)
                $entry.LastWriteTime=[DateTimeOffset]::new(2000,1,1,0,0,0,[TimeSpan]::Zero)
                $entry.ExternalAttributes=0
                $source=[IO.File]::OpenRead((Join-Path $payload $relative));$target=$entry.Open()
                try{$source.CopyTo($target)}finally{$target.Dispose();$source.Dispose()}
            }
        }finally{$zip.Dispose();$stream.Dispose()}
    }
    $label='CapyCanvas-windows-x64-'+$commit.Substring(0,12)+$(if($dirty){'-development'}else{''})
    $archive=Join-Path $run ($label+'.zip')
    Write-Archive $archive
    $repeat=Join-Path $run 'reproducibility-check.zip';Write-Archive $repeat
    $hash=(Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant()
    if((Get-FileHash -LiteralPath $repeat -Algorithm SHA256).Hash.ToLowerInvariant() -ne $hash){throw 'Repeated archive assembly produced different bytes.'}
    [IO.File]::WriteAllText(($archive+'.sha256'),$hash+'  '+[IO.Path]::GetFileName($archive)+$lf,$utf8)
    $result=[ordered]@{archive=$archive;sha256=$hash;payload=$payload;development=$dirty;source_commit=$commit;file_count=$files.Count+1;repeat_archive='passed'}
    [IO.File]::WriteAllText((Join-Path $run 'result.json'),($result|ConvertTo-Json)+$lf,$utf8)
    $result|ConvertTo-Json
}finally{Pop-Location}
