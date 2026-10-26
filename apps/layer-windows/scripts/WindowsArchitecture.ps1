function Resolve-WindowsArchitecture([ValidateSet('native','x64','arm64')][string]$Architecture='native') {
    $Architecture = $Architecture.ToLowerInvariant()
    $native = if ($env:PROCESSOR_ARCHITEW6432) { $env:PROCESSOR_ARCHITEW6432 } else { $env:PROCESSOR_ARCHITECTURE }
    $hostArchitecture = if ($native -eq 'ARM64') { 'arm64' } else { 'x64' }
    if ($Architecture -eq 'native') { $Architecture = $hostArchitecture }
    $target = if ($Architecture -eq 'arm64') { 'aarch64-pc-windows-msvc' } else { 'x86_64-pc-windows-msvc' }
    $component = if ($Architecture -eq 'arm64') { 'Microsoft.VisualStudio.Component.VC.Tools.ARM64' } else { 'Microsoft.VisualStudio.Component.VC.Tools.x86.x64' }
    $platform = if ($Architecture -eq 'arm64') { 'ARM64' } else { 'x64' }
    [pscustomobject]@{architecture=$Architecture;platform=$platform;target=$target;hostArchitecture=$hostArchitecture;component=$component}
}

function Enter-WindowsDeveloperShell($Build) {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
    if (!(Test-Path -LiteralPath $vswhere)) { throw 'Install Visual Studio Build Tools with Desktop development with C++ and Windows SDK 10.0.26100.0.' }
    $installation = & $vswhere -latest -products '*' -requires $Build.component -format json | ConvertFrom-Json
    if (!$installation) { throw "Install Visual Studio C++ tools for $($Build.architecture) ($($Build.component))." }
    Import-Module (Join-Path $installation.installationPath 'Common7/Tools/Microsoft.VisualStudio.DevShell.dll') | Out-Null
    Enter-VsDevShell -VsInstallPath $installation.installationPath -SkipAutomaticLocation -DevCmdArguments "-arch=$($Build.architecture) -host_arch=$($Build.hostArchitecture)" | Out-Null
    $installation
}

function Get-WindowsBuildDirectory([string]$Repo,$Build,[string]$Configuration) {
    if ($Build.architecture -eq 'arm64') { Join-Path $Repo "artifacts/windows/arm64/$Configuration" }
    else { Join-Path $Repo "artifacts/windows/$Configuration" }
}

function Test-WindowsArm64XBinary([IO.BinaryReader]$Reader,[long]$PeOffset) {
    $stream = $Reader.BaseStream
    if ($stream.Length -lt $PeOffset+24) { return $false }
    $stream.Position = $PeOffset+6
    $sectionCount = $Reader.ReadUInt16()
    $stream.Position = $PeOffset+20
    $optionalSize = $Reader.ReadUInt16()
    $optional = $PeOffset+24
    $sectionTable = $optional+$optionalSize
    if ($optionalSize -lt 200 -or !$sectionCount -or $sectionTable+40*$sectionCount -gt $stream.Length) { return $false }
    $stream.Position = $optional
    if ($Reader.ReadUInt16() -ne 0x20B) { return $false }
    $stream.Position = $optional+192
    $loadRva = $Reader.ReadUInt32()
    $loadSize = $Reader.ReadUInt32()
    if (!$loadRva -or $loadSize -lt 232) { return $false }
    $sections = @(for ($i=0; $i -lt $sectionCount; $i++) {
        $stream.Position = $sectionTable+40*$i+12
        $rva = $Reader.ReadUInt32()
        $rawSize = $Reader.ReadUInt32()
        $raw = $Reader.ReadUInt32()
        @{rva=[long]$rva;size=[long]$rawSize;raw=[long]$raw}
    })
    $loadSection = $sections | Where-Object { $loadRva -ge $_.rva -and $loadRva+232 -le $_.rva+$_.size } | Select-Object -First 1
    if (!$loadSection) { return $false }
    $loadOffset = $loadSection.raw+$loadRva-$loadSection.rva
    if ($loadOffset+232 -gt $stream.Length) { return $false }
    $stream.Position = $loadOffset
    if ($Reader.ReadUInt32() -lt 232) { return $false }
    $stream.Position = $loadOffset+200
    if (!$Reader.ReadUInt64()) { return $false }
    $stream.Position = $loadOffset+224
    $relocationOffset = $Reader.ReadUInt32()
    $relocationSection = $Reader.ReadUInt16()
    if (!$relocationSection -or $relocationSection -gt $sectionCount) { return $false }
    $section = $sections[$relocationSection-1]
    if ($relocationOffset+8 -gt $section.size) { return $false }
    $table = $section.raw+$relocationOffset
    if ($table+8 -gt $stream.Length) { return $false }
    $stream.Position = $table
    if ($Reader.ReadUInt32() -ne 1) { return $false }
    $size = $Reader.ReadUInt32()
    $end = $table+8+$size
    if ($end -gt $stream.Length -or $relocationOffset+8+$size -gt $section.size) { return $false }
    while ($stream.Position+12 -le $end) {
        $symbol = $Reader.ReadUInt64()
        $relocationSize = $Reader.ReadUInt32()
        if (!$relocationSize -or $stream.Position+$relocationSize -gt $end) { return $false }
        if ($symbol -eq 6) { return $true }
        $stream.Position += $relocationSize
    }
    return $false
}

function Test-WindowsBinaryArchitecture([string]$Path,[ValidateSet('x64','arm64')][string]$Architecture,[switch]$AllowCompanion) {
    $machines = if ($Architecture -eq 'arm64') { @(0xAA64,0xA64E) } else { @(0x8664) }
    $reader = [IO.BinaryReader]::new([IO.File]::OpenRead($Path))
    try {
        if ($reader.ReadUInt16() -ne 0x5A4D) { throw "Invalid Windows binary: $Path" }
        $reader.BaseStream.Position = 0x3C
        $peOffset = $reader.ReadUInt32()
        $reader.BaseStream.Position = $peOffset
        if ($reader.ReadUInt32() -ne 0x4550) { throw "Invalid Windows binary: $Path" }
        $machine = $reader.ReadUInt16()
        if ($machine -in $machines) { return $true }
        if (!$AllowCompanion -or $Architecture -ne 'x64' -or $machine -ne 0xAA64 -or $Path -notmatch '_ec\.dll$') { return $false }
        $paired = $Path -replace '_ec\.dll$', '.dll'
        if (!(Test-Path -LiteralPath $paired -PathType Leaf) -or !(Test-WindowsBinaryArchitecture $paired x64)) { return $false }
        Test-WindowsArm64XBinary $reader $peOffset
    } finally { $reader.Dispose() }
}

function Assert-WindowsBinaryArchitecture([string]$Directory,[ValidateSet('x64','arm64')][string]$Architecture) {
    foreach ($file in Get-ChildItem -LiteralPath $Directory -File -Recurse | Where-Object Extension -In @('.exe','.dll')) {
        if (!(Test-WindowsBinaryArchitecture $file.FullName $Architecture -AllowCompanion)) { throw "Expected $Architecture binary: $($file.FullName)" }
    }
}
