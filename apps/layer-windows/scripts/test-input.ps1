param([string]$OutputDirectory)
$ErrorActionPreference='Stop'
$repo=Split-Path -Parent (Split-Path -Parent (Split-Path -Parent $PSScriptRoot))
if(!$OutputDirectory){$OutputDirectory=Join-Path $repo 'artifacts/windows/tests'}
$OutputDirectory=[IO.Path]::GetFullPath($OutputDirectory)
New-Item -ItemType Directory -Force $OutputDirectory | Out-Null
$source=Join-Path $PSScriptRoot '../tests/work-buffer.cpp'
$object=Join-Path $OutputDirectory 'work-buffer.obj'
$executable=Join-Path $OutputDirectory 'work-buffer-test.exe'
& cl /nologo /std:c++20 /EHsc /W4 /WX $source "/Fo$object" "/Fe$executable"
if($LASTEXITCODE -ne 0){throw 'Canvas queue test build failed. Run from a Visual Studio developer shell.'}
& $executable
if($LASTEXITCODE -ne 0){throw 'Canvas queue test failed.'}
