param(
    [Parameter(Mandatory)][string]$Destination,
    [Parameter(Mandatory)][string]$PackagesDirectory,
    [Parameter(Mandatory)][string]$VisualStudio
)
$ErrorActionPreference='Stop'
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Destination=[IO.Path]::GetFullPath($Destination)
[IO.Directory]::CreateDirectory($Destination)|Out-Null
$utf8=[Text.UTF8Encoding]::new($false)
function Copy-Notice([string]$Source,[string]$Relative) {
    if(!(Test-Path -LiteralPath $Source -PathType Leaf)){throw "Missing notice: $Source"}
    $target=Join-Path $Destination $Relative
    [IO.Directory]::CreateDirectory((Split-Path -Parent $target))|Out-Null
    Copy-Item -LiteralPath $Source -Destination $target -Force
}
foreach($name in @('LICENSE','LICENSE-MIT','LICENSE-APACHE','BRANDING.md','THIRD_PARTY_NOTICES.md')){
    Copy-Notice (Join-Path $repo $name) "CapyCanvas/$name"
}
$cargoDirectory=if($env:CARGO_HOME){$env:CARGO_HOME}else{Join-Path $env:USERPROFILE '.cargo'}
$registries=@(Get-ChildItem -LiteralPath (Join-Path $cargoDirectory 'registry/src') -Directory)
Push-Location $repo
try {
    # metadata on the whole workspace also fetches unrelated GTK dependencies.
    # tree selects exactly this target, including build/proc-macro dependencies.
    $tree=& cargo tree --locked --offline -p layer-windows --target x86_64-pc-windows-msvc -e normal,build --prefix none --format '{p}|{l}'
    if($LASTEXITCODE -ne 0){throw 'Resolve/build the locked Windows dependencies before collecting notices.'}
    $local=& cargo metadata --locked --offline --no-deps --format-version 1|ConvertFrom-Json
    if($LASTEXITCODE -ne 0){throw 'Cannot read workspace package identities.'}
}finally{Pop-Location}
$workspaceNames=@($local.packages.name)
$dependencies=@{}
foreach($line in $tree){
    if($line -notmatch '^(?<name>[\w-]+) v(?<version>[^ ]+)(?: \((?<path>[^|]*)\))?\|(?<license>.+)$'){throw "Unrecognized Cargo dependency: $line"}
    $localSource=if($Matches.path -and [IO.Path]::IsPathRooted($Matches.path)){$Matches.path}else{$null}
    $name=$Matches.name;$version=$Matches.version;$license=$Matches.license -replace ' \(\*\)$',''
    if($name -in $workspaceNames){continue}
    $key="$name-$version"
    if($dependencies.ContainsKey($key)){continue}
    if($localSource){
        $source=(Resolve-Path -LiteralPath $localSource).Path
        $vendorRoot=(Resolve-Path -LiteralPath (Join-Path $repo 'vendor')).Path+[IO.Path]::DirectorySeparatorChar
        if(!$source.StartsWith($vendorRoot,[StringComparison]::OrdinalIgnoreCase)){throw "Unreviewed local dependency: $key"}
    }else{
        $sources=@($registries|ForEach-Object {Join-Path $_.FullName $key}|Where-Object {Test-Path -LiteralPath $_ -PathType Container})
        if($sources.Count -ne 1){throw "Expected one cached registry source for $key"}
        $source=$sources[0]
    }
    $notices=@(Get-ChildItem -LiteralPath $source -File|Where-Object Name -Match '^(LICENSE|LICENCE|COPYING|COPYRIGHT|NOTICE|UNLICENSE)')
    foreach($folder in @(Get-ChildItem -LiteralPath $source -Directory|Where-Object Name -Match '^(licenses|licences)$')){
        $notices+=@(Get-ChildItem -LiteralPath $folder.FullName -File -Recurse)
    }
    if(!$notices.Count){
        switch -Exact ($key){
            {$_ -in @('zune-core-0.4.12','zune-inflate-0.2.54','zune-jpeg-0.4.21')} {
                $notice=Join-Path $repo 'tools/build/licenses/zune-core-0.4.12-ZLIB.txt'
                $canonical=[Text.Encoding]::UTF8.GetBytes([IO.File]::ReadAllText($notice).Replace("`r`n","`n"))
                $hash=[Security.Cryptography.SHA256]::Create()
                try{$digest=[BitConverter]::ToString($hash.ComputeHash($canonical)).Replace('-','')}finally{$hash.Dispose()}
                if($digest -ne '7fa429541e55b1509909e058f2d21a37467e4958ec713b357f6e0cf9dc4ee352'){throw 'Original Zune license checksum differs'}
                Copy-Notice $notice "Cargo/$key/LICENSE-ZLIB"
            }
            {$_ -in @('atomig-macro-0.4.0','simd_helpers-0.1.0')} {
                Copy-Notice (Join-Path $repo "apps/layer-windows/packaging/notices/$key-MIT.txt") "Cargo/$key/LICENSE-MIT"
            }
            {$_ -in @('profiling-1.0.18','profiling-procmacros-1.0.18')} {Copy-Notice (Join-Path $repo 'apps/layer-windows/packaging/notices/profiling-1.0.18-MIT.txt') "Cargo/$key/LICENSE-MIT"}
            {$_ -in @('spirv-0.4.0+sdk-1.4.341.0','gl_generator-0.14.0','khronos_api-3.1.0')} {
                Copy-Notice (Join-Path $repo 'LICENSE-APACHE') "Cargo/$key/LICENSE-APACHE"
            }
            default {throw "No reviewed license text for $key ($license)"}
        }
    }
    foreach($notice in $notices){Copy-Notice $notice.FullName ("Cargo/$key/"+$notice.FullName.Substring($source.Length+1))}
    # Keep published attribution metadata, including authors and repositories.
    $manifest=if(Test-Path -LiteralPath (Join-Path $source 'Cargo.toml.orig')){'Cargo.toml.orig'}else{'Cargo.toml'}
    Copy-Notice (Join-Path $source $manifest) "Cargo/$key/$manifest"
    if($name -eq 'gl_generator'){
        $header=([IO.File]::ReadAllLines((Join-Path $source 'lib.rs'))|Select-Object -First 13) -join ([string][char]10)
        [IO.File]::WriteAllText((Join-Path $Destination "Cargo/$key/SOURCE-NOTICE.txt"),$header,$utf8)
    }
    if($name -eq 'khronos_api'){
        Copy-Notice (Join-Path $repo 'apps/layer-windows/packaging/notices/khronos_api-3.1.0-ANGLE.txt') "Cargo/$key/ANGLE-LICENSE"
        $headers=@(Get-ChildItem -LiteralPath $source -Filter '*.xml' -File -Recurse|Sort-Object FullName|ForEach-Object {
            $xml=[IO.File]::ReadAllText($_.FullName)
            $start=$xml.IndexOf('<comment>');$end=$xml.IndexOf('</comment>')
            if($start -ge 0 -and $end -gt $start){$xml.Substring($start+9,$end-$start-9)}
        })|Sort-Object -Unique
        [IO.File]::WriteAllText((Join-Path $Destination "Cargo/$key/REGISTRY-NOTICES.txt"),($headers -join ([string][char]10+[char]10)),$utf8)
    }
    if($name -eq 'libsqlite3-sys'){
        $sqlite=[IO.File]::ReadAllText((Join-Path $source 'sqlite3/sqlite3.h'))
        $end=$sqlite.IndexOf('*/')
        if($end -lt 0){throw 'SQLite source notice not found'}
        [IO.File]::WriteAllText((Join-Path $Destination "Cargo/$key/SQLITE-NOTICE.txt"),$sqlite.Substring(0,$end+2),$utf8)
    }
    $dependencies[$key]=[ordered]@{name=$name;version=$version;license=$license;source=$(if($localSource){"vendor/"+[IO.Path]::GetRelativePath((Join-Path $repo 'vendor'),$source).Replace('\','/')}else{"https://crates.io/crates/$name/$version"})}
}
[xml]$config=Get-Content (Join-Path $repo 'apps/layer-windows/packages.config') -Raw
foreach($package in $config.packages.package){
    $key="$($package.id).$($package.version)";$source=Join-Path $PackagesDirectory $key
    $notices=@(Get-ChildItem -LiteralPath $source -File|Where-Object Name -Match '(license|notice)')
    if(!$notices.Count -and $package.id -eq 'Microsoft.Windows.SDK.BuildTools'){
        [xml]$project=Get-Content (Join-Path $repo 'apps/layer-windows/CapyCanvas.vcxproj') -Raw
        $sdkVersion=@($project.Project.PropertyGroup.WindowsTargetPlatformVersion|Where-Object {$_})[0]
        $sdkRoot=if($env:WindowsSdkDir){$env:WindowsSdkDir}else{Join-Path ${env:ProgramFiles(x86)} 'Windows Kits/10'}
        Copy-Notice (Join-Path $sdkRoot "Licenses/$sdkVersion/sdk_license.rtf") "NuGet/$key/sdk_license.rtf"
        continue
    }
    if(!$notices.Count){throw "No NuGet notices for $key"}
    foreach($notice in $notices){Copy-Notice $notice.FullName "NuGet/$key/$($notice.Name)"}
}
$rustRoot=& rustc --print sysroot
if($LASTEXITCODE -ne 0){throw 'Cannot locate the Rust toolchain notices'}
Copy-Notice (Join-Path $rustRoot 'share/doc/rust/COPYRIGHT-library.html') 'Rust/COPYRIGHT-library.html'
Copy-Notice (Join-Path $VisualStudio 'Licenses/1033/Redist.txt') 'VisualCpp/Redist.txt'
Copy-Notice (Join-Path $VisualStudio 'Licenses/1033/ThirdPartyNotices.txt') 'VisualCpp/ThirdPartyNotices.txt'
Copy-Notice (Join-Path $repo 'apps/layer-windows/packaging/notices/README.md') 'SUPPLEMENTAL-NOTICES.md'
$ordered=@($dependencies.Keys|Sort-Object|ForEach-Object {$dependencies[$_]})
[IO.File]::WriteAllText((Join-Path $Destination 'Cargo-dependencies.json'),($ordered|ConvertTo-Json -Depth 5),$utf8)
Write-Output "Collected notices for $($dependencies.Count) Cargo packages, the pinned NuGet packages and native runtimes."
