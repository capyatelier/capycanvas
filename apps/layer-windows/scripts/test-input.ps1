param([string]$OutputDirectory, [ValidateSet('Debug','Release')][string]$Configuration = 'Release')
$ErrorActionPreference='Stop'
$repo=Split-Path -Parent (Split-Path -Parent (Split-Path -Parent $PSScriptRoot))
if(!$OutputDirectory){$OutputDirectory=Join-Path $repo 'artifacts/windows/tests'}
$OutputDirectory=[IO.Path]::GetFullPath($OutputDirectory)
New-Item -ItemType Directory -Force $OutputDirectory | Out-Null
$source=Join-Path $PSScriptRoot '../tests/work-buffer.cpp'
$object=Join-Path $OutputDirectory 'work-buffer.obj'
$executable=Join-Path $OutputDirectory 'work-buffer-test.exe'
$optimization = if ($Configuration -eq 'Debug') { '/Od' } else { '/O2' }
& cl /nologo /std:c++20 /EHsc /W4 /WX $optimization $source "/Fo$object" "/Fe$executable"
if($LASTEXITCODE -ne 0){throw 'Canvas queue test build failed. Run from a Visual Studio developer shell.'}
& $executable
if($LASTEXITCODE -ne 0){throw 'Canvas queue test failed.'}

& {
 $tokens=$null;$errors=$null
 $ast=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot 'build.ps1'),[ref]$tokens,[ref]$errors)
 if($errors.Count){throw 'Build script parse failed'}
 $selection=$ast.FindAll({param($node) $node -is [Management.Automation.Language.AssignmentStatementAst] -and $node.Left.Extent.Text -in '$installation','$toolset'},$true)
 if($selection.Count -ne 2){throw 'Expected installation and toolset selection'}
 foreach($case in @(@{version='17.14.0';toolset='v143'},@{version='18.11.0';toolset='v145'})){
  $vswhere={
   if($args -contains '-format'){
    ConvertTo-Json -InputObject @(@{installationPath='C:\Visual Studio\C++';installationVersion=$case.version}) -Compress
   }elseif($args[-1] -eq 'installationPath'){'C:\Visual Studio\C++'}
   else{'19.0.0'}
  }
  foreach($statement in $selection){Invoke-Expression $statement.Extent.Text}
  if($toolset -ne $case.toolset){throw 'Toolset came from another Visual Studio installation'}
 }
}

& {
 $tokens=$null;$errors=$null
 $ast=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot 'build.ps1'),[ref]$tokens,[ref]$errors)
 if($errors.Count){throw 'Build script parse failed'}
 $directory=@($ast.EndBlock.Statements|Where-Object {
  ($_ -is [Management.Automation.Language.IfStatementAst] -and $_.Clauses[0].Item1.Extent.Text -eq '!$IntermediateDirectory') -or
  ($_ -is [Management.Automation.Language.AssignmentStatementAst] -and $_.Left.Extent.Text -eq '$IntermediateDirectory')
 })
 $nativeBuild=$ast.FindAll({param($node) $node -is [Management.Automation.Language.CommandAst] -and $node.GetCommandName() -eq 'msbuild'},$true)
 if($directory.Count -ne 2 -or $nativeBuild.Count -ne 1){throw 'Expected default and normalized intermediate directory and one native build'}
 . (Join-Path $PSScriptRoot 'WindowsArchitecture.ps1')
 function msbuild {$script:nativeBuildArguments=@($args)}
 $Configuration='Release';$fixture='false';$toolset='v145';$PackagesDirectory='C:\fixture\packages'
 $seen=@()
 foreach($case in @(@{architecture='x64';output='C:\fixture\first';expected='C:\fixture\first.obj'},@{architecture='arm64';output='C:\fixture\second\';expected='C:\fixture\second.obj'},@{architecture='arm64';output='C:\fixture\third';intermediate='C:\fixture\shared.obj';expected='C:\fixture\shared.obj'})){
  $build=Resolve-WindowsArchitecture $case.architecture
  $rustDirectory='C:\fixture\cargo-target\'+$build.target+'\release'
  $OutputDirectory=$case.output
  $IntermediateDirectory=$case.intermediate
  foreach($statement in $directory){Invoke-Expression $statement.Extent.Text}
  Invoke-Expression $nativeBuild[0].Extent.Text
  if($script:nativeBuildArguments -notcontains "/p:Platform=$($build.platform)" -or $script:nativeBuildArguments -notcontains "/p:CapyRustDirectory=$rustDirectory"){throw 'The native build did not link the selected Rust target'}
  if($script:nativeBuildArguments -notcontains "/p:IntDir=$($case.expected)/" -or $script:nativeBuildArguments -notcontains "/p:OutDir=$OutputDirectory/"){throw 'The native build did not isolate intermediates beside its output'}
  $seen+=$intermediateDirectory
 }
 if(@($seen|Select-Object -Unique).Count -ne 3){throw 'Distinct native outputs reused intermediate objects'}
}

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

& (Join-Path $PSScriptRoot '../tests/workspace-publisher-query.ps1')

& (Join-Path $PSScriptRoot '../tests/application-menu.ps1')

& (Join-Path $PSScriptRoot '../tests/application-menu-query.ps1')

& (Join-Path $PSScriptRoot '../tests/paper-relief.ps1')

& {
 $tokens=$null;$errors=$null
 $source=[IO.File]::ReadAllText((Join-Path $PSScriptRoot 'exercise-workspace-pickup.ps1'),[Text.Encoding]::UTF8)
 $ast=[Management.Automation.Language.Parser]::ParseInput($source,[ref]$tokens,[ref]$errors)
 if($errors.Count){throw 'Workspace pickup fixture parse failed'}
 foreach($name in @('Floating-Point','Floating-Toolbar')){
  $helper=$ast.FindAll({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name},$true)
  if($helper.Count -ne 1){throw "Expected one $name helper"}
  Invoke-Expression $helper[0].Extent.Text
 }
 $preview=@{x=546;y=48;width=108;height=280};$area=@{x=256;y=48;width=688;height=846}
 foreach($case in @(
  @{at=@{x=600;y=318};density=1;expected=@{x=600;y=601}},
  @{at=@{x=1139;y=706};density=1.5;expected=@{x=1139;y=1130}},
  @{at=@{x=1439;y=865};density=2;expected=@{x=1439;y=1431}},
  @{at=@{x=1091;y=128};preview=@{x=982;y=-302;width=218;height=440};density=1;expected=@{x=600;y=681}}
 )){
  $bounds=if($case.preview){$case.preview}else{$preview}
  $point=Floating-Point $case.at $bounds $area $case.density
  if($point.x -ne $case.expected.x -or $point.y -ne $case.expected.y){throw 'Floating destination did not preserve the native grip offset at the current density'}
 }
 $model='{"workspace_update":{"revision":12,"drag":null},"state":{"workspace":{"layout":{"floating":[{"root":{"kind":"tabs","id":2,"panels":["toolbar"]}}]}}},"layout":{"groups":[{"id":2,"panels":["toolbar"],"floating":true}]}}'|ConvertFrom-Json
 if(!(Floating-Toolbar $model 11)){throw 'Committed floating toolbar was rejected'}
 if(Floating-Toolbar $model 12){throw 'Stale floating layout passed release'}
 $model.workspace_update.drag=[pscustomobject]@{group=@{id=2}}
 if(Floating-Toolbar $model 11){throw 'Held floating preview passed release'}
 $model.workspace_update.drag=$null;$model.state.workspace.layout.floating=@()
 if(Floating-Toolbar $model 11){throw 'Resolved-only floating preview passed release'}
 $model.state.workspace.layout.floating=@([pscustomobject]@{root=@{kind='tabs';id=2;panels=@('toolbar')}})
 $model.layout.groups[0].floating=$false
 if(Floating-Toolbar $model 11){throw 'Docked toolbar passed floating release'}
}

& (Join-Path $PSScriptRoot '../tests/numeric-readout.ps1')
& (Join-Path $PSScriptRoot '../tests/ink-color.ps1')
& (Join-Path $PSScriptRoot '../tests/recovery-fixture.ps1') -Output (Join-Path $OutputDirectory ('recovery-paths-'+[guid]::NewGuid().ToString('N')))
