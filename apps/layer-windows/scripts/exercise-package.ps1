param([Parameter(Mandatory)][string]$Archive)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
Add-Type -AssemblyName System.IO.Compression.FileSystem
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Archive=(Resolve-Path -LiteralPath $Archive).Path
$run=Join-Path $repo ('artifacts/windows/package-review/'+[Guid]::NewGuid().ToString('N'))
$unpacked=Join-Path $run 'extracted app with spaces'
[IO.Directory]::CreateDirectory($unpacked)|Out-Null
$prefix=[IO.Path]::GetFullPath($unpacked)+[IO.Path]::DirectorySeparatorChar
$zip=[IO.Compression.ZipFile]::OpenRead($Archive)
$names=[Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
try {
    foreach($entry in $zip.Entries){
        $name=$entry.FullName
        if(!$name.StartsWith('CapyCanvas/') -or $name -match '(^|/)\.\.?(/|$)|[:\\]' -or !$names.Add($name)){throw "Invalid archive entry: $name"}
        $path=[IO.Path]::GetFullPath((Join-Path $unpacked $name))
        if(!$path.StartsWith($prefix,[StringComparison]::OrdinalIgnoreCase)){throw 'Archive entry escaped extraction root'}
        if($name.EndsWith('/')){throw 'Expected file entries only'}
        [IO.Directory]::CreateDirectory((Split-Path -Parent $path))|Out-Null
        $source=$entry.Open();$target=[IO.File]::Open($path,[IO.FileMode]::CreateNew)
        try{$source.CopyTo($target)}finally{$target.Dispose();$source.Dispose()}
    }
}finally{$zip.Dispose()}
$payload=Join-Path $unpacked 'CapyCanvas'
$manifest=Get-Content (Join-Path $payload 'package-manifest.json') -Raw|ConvertFrom-Json
if($manifest.schema -ne 1 -or $manifest.architecture -ne 'x64'){throw 'Unsupported package manifest'}
$declared=[Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
foreach($file in $manifest.files){
    if(!$declared.Add($file.path) -or !$names.Contains('CapyCanvas/'+$file.path)){throw 'Manifest contains duplicate or missing files'}
    $path=Join-Path $payload $file.path
    if((Get-Item -LiteralPath $path).Length -ne $file.bytes -or (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash -ne $file.sha256){throw "Package file does not match its hash: $($file.path)"}
}
if($names.Count -ne $declared.Count+1){throw 'Archive has files absent from its manifest'}
$CapyTraceDirectory=$run
try {
    Enter-CapyEnvironment @('CAPY_FILTERS_DIR','CAPY_FILTERS_MODE','PATH')
    # A build shell adds SDK shader compilers to PATH; exercise the bundled runtime.
    $env:PATH=(Join-Path $env:WINDIR 'System32')+[IO.Path]::PathSeparator+$env:WINDIR
    $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'profile'
    $env:CAPY_TRACE_UI='1';$env:CAPY_SMOKE_TEST='1'
    $stderr=Join-Path $run 'stderr.log'
    # Launch with a separate working directory to detect resource lookup that
    # accidentally depends on the development checkout or current directory.
    $review=Start-Process -FilePath (Join-Path $payload 'CapyCanvas.exe') -WorkingDirectory $run -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
    $null=$review.Handle
    @{process_id=$review.Id;run=$run;archive=$Archive;payload=$payload}|ConvertTo-Json|Set-Content (Join-Path $repo 'artifacts/windows/package-review.json')
    Write-Output "Owned package review $($review.Id)"
    Wait-Until {(Model).brush_ready -and (Model).windows_workspace.ready -and !(Model).windows_workspace.busy} 'Extracted package did not initialize' 60
    (Get-Process -Id $review.Id).Modules|Select-Object ModuleName,FileName|ConvertTo-Json|Set-Content (Join-Path $run 'loaded-modules.json')
    foreach($module in @((Get-Process -Id $review.Id).Modules|Where-Object ModuleName -Match '^(dxcompiler|dxil|D3DCOMPILER_47)\.dll$')){
        $path=[IO.Path]::GetFullPath($module.FileName)
        if(!$path.StartsWith($payload+[IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase) -and !$path.StartsWith([IO.Path]::GetFullPath((Join-Path $env:WINDIR 'System32'))+[IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase)){throw 'Shader compiler loaded from outside the package or Windows system directory'}
    }
    $modulePaths=@{}
    foreach($name in @('layer_windows.dll','Microsoft.UI.Xaml.dll','Microsoft.WindowsAppRuntime.dll','vcruntime140.dll','msvcp140.dll')){
        $module=@((Get-Process -Id $review.Id).Modules|Where-Object ModuleName -EQ $name)
        if($module.Count -ne 1 -or ![IO.Path]::GetFullPath($module[0].FileName).StartsWith($payload+[IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase)){throw "Runtime was not loaded from the extracted package: $name"}
        $modulePaths[$name]='app-local'
    }
    foreach($action in @('Test stroke','Undo','Redo','Test pan','Resize')){
        & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action $action
        if($action -in @('Test stroke','Redo')){Wait-Until {(Model).state.document_file.modified} 'Package stroke did not change the document'}
        if($action -eq 'Undo'){Wait-Until {!(Model).state.document_file.modified} 'Package Undo did not restore the clean document'}
    }
    & (Join-Path $PSScriptRoot 'inspect-window.ps1') -ProcessId $review.Id -Output (Join-Path $run 'package.png') -ClientOnly *> (Join-Path $run 'capture.json')
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved -StateDirectory $run
    if((Get-Item -LiteralPath $stderr).Length){throw 'Extracted package stderr requires inspection'}
    [ordered]@{archive_sha256=(Get-FileHash -LiteralPath $Archive -Algorithm SHA256).Hash.ToLowerInvariant();source_commit=$manifest.source_commit;development=$manifest.development;inventory='passed';app_local_runtimes=$modulePaths;extracted_launch='passed';drawing_undo_redo='passed';pan_resize='passed';zero_exit='passed';scope='extraction and runtime-origin checks on this host; clean-machine installation and physical input/performance remain separate'}|ConvertTo-Json -Depth 5|Tee-Object -FilePath (Join-Path $run 'result.json')
}catch{
    $failure=$_
    $failure.ToString()|Set-Content (Join-Path $run 'failure.txt')
    if($review -and !$review.HasExited){
        & (Join-Path $PSScriptRoot 'inspect-window.ps1') -ProcessId $review.Id -Output (Join-Path $run 'failure.png') -ClientOnly *> (Join-Path $run 'failure-capture.json')
    }
    throw $failure
}finally{
    Exit-CapyEnvironment
}
