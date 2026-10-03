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

& {
 $tokens=$null;$errors=$null
 $source=[IO.File]::ReadAllText((Join-Path $PSScriptRoot 'exercise-localization.ps1'),[Text.Encoding]::UTF8)
 $ast=[Management.Automation.Language.Parser]::ParseInput($source,[ref]$tokens,[ref]$errors)
 if($errors.Count){throw 'Localization fixture parse failed'}
 $trace=$ast.FindAll({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Trace-Part'},$true)
 if($trace.Count -ne 1){throw 'Expected one trace helper'}
 Invoke-Expression $trace[0].Extent.Text
 $script:current=[pscustomobject]@{id=41;hwnd=41};$review=[pscustomobject]@{Id=10};$run=$OutputDirectory
 function Read-Snapshot([string]$Path){
  if($Path -ne (Join-Path $run 'search-state-10-41.json')){throw ('Wrong owner path: '+$Path)}
  [pscustomobject]@{process_id=10;window_id=41;search='owned search'}
 }
 function Caller-LocalContext{
  $current=[pscustomobject]@{layer=$false;sample_width=1}
  Trace-Part 'search-state' 'search'
 }
 if((Caller-LocalContext) -ne 'owned search'){throw 'Caller-local state shadowed the owned window'}
}

& {
 $tokens=$null;$errors=$null
 $source=[IO.File]::ReadAllText((Join-Path $repo 'tools/windows-vm/run-fixtures.ps1'),[Text.Encoding]::UTF8)
 $ast=[Management.Automation.Language.Parser]::ParseInput($source,[ref]$tokens,[ref]$errors)
 if($errors.Count){throw 'VM runner parse failed'}
 $selection=$ast.FindAll({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Select-Fixtures'},$true)
 if($selection.Count -ne 1){throw 'Expected one fixture selection helper'}
 Invoke-Expression $selection[0].Extent.Text
 $available=@('shortcuts','localization','localization:light','localization:LargeText','localization:light-large')
 foreach($case in @(
  @{request=@();expected=$available},
  @{request=@('localization');expected=$available[1..4]},
  @{request=@('localization:default');expected=@('localization')},
  @{request=@('localization:default','localization:LargeText');expected=@('localization','localization:LargeText')},
  @{request=@('localization:light','localization:light-large');expected=@('localization:light','localization:light-large')},
  @{request=@('localization:default','localization');expected=$available[1..4]}
 )){
  if(((Select-Fixtures $available $case.request) -join ',') -ne ($case.expected -join ',')){throw 'Fixture selection changed the requested coverage'}
 }
 foreach($request in @('unknown','unknown:default','localization:default:light','localization:dark')){
  $rejected=$false;try{Select-Fixtures $available @($request)|Out-Null}catch{$rejected=$true}
  if(!$rejected){throw "Unknown fixture selector was accepted: $request"}
 }
}
