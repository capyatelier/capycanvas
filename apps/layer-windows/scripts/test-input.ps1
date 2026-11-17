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
$manifest = '/MANIFEST:EMBED',"/MANIFESTUAC:level='asInvoker' uiAccess='false'"
& cl /nologo /std:c++20 /EHsc /W4 /WX $optimization $source "/Fo$object" "/Fe$executable" windowsapp.lib /link @manifest
if($LASTEXITCODE -ne 0){throw 'Canvas queue test build failed. Run from a Visual Studio developer shell.'}
& $executable
if($LASTEXITCODE -ne 0){throw 'Canvas queue test failed.'}

$source=Join-Path $PSScriptRoot '../tests/panel-scroll.cpp'
$object=Join-Path $OutputDirectory 'panel-scroll.obj'
$executable=Join-Path $OutputDirectory 'panel-scroll-test.exe'
& cl /nologo /std:c++20 /EHsc /W4 /WX $optimization $source "/Fo$object" "/Fe$executable" /link @manifest
if($LASTEXITCODE -ne 0){throw 'Panel scroll receipt test build failed.'}
& $executable
if($LASTEXITCODE -ne 0){throw 'Panel scroll receipt test failed.'}

$source=Join-Path $PSScriptRoot '../tests/model-update.cpp'
$object=Join-Path $OutputDirectory 'model-update.obj'
$executable=Join-Path $OutputDirectory 'model-update-test.exe'
& cl /nologo /std:c++20 /EHsc /W4 /WX $optimization $source "/Fo$object" "/Fe$executable" windowsapp.lib /link @manifest
if($LASTEXITCODE -ne 0){throw 'Model update test build failed.'}
& $executable
if($LASTEXITCODE -ne 0){throw 'Model update replay failed.'}

$source=Join-Path $PSScriptRoot '../tests/file-drop.cpp'
$object=Join-Path $OutputDirectory 'file-drop.obj'
$executable=Join-Path $OutputDirectory 'file-drop-test.exe'
& cl /nologo /std:c++20 /EHsc /W4 /WX $optimization $source "/Fo$object" "/Fe$executable" windowsapp.lib /link @manifest
if($LASTEXITCODE -ne 0){throw 'DataPackage file drop test build failed.'}
& $executable $OutputDirectory
if($LASTEXITCODE -ne 0){throw 'DataPackage file drop test failed.'}

function Get-SettingsProducer([string]$Text,[string]$Pattern,[string]$Name){
 $found=[regex]::Matches($Text,$Pattern)
 if($found.Count -ne 1){throw "Expected exactly one SettingsView $Name producer"}
 $start=$found[0].Index;$depth=0
 $tokens=[regex]::Matches($Text.Substring($start),'(?s)"(?:\\.|[^"\\])*"|''(?:\\.|[^''\\])*''|//[^\r\n]*|/\*.*?\*/|[{}]')
 foreach($token in $tokens){
  if($token.Value -eq '{'){$depth++}
  elseif($token.Value -eq '}'){$depth--;if($depth -eq 0){return $Text.Substring($start,$token.Index+1)}}
 }
 throw "Unbalanced SettingsView $Name producer"
}
$native=[IO.File]::ReadAllText((Join-Path $PSScriptRoot '../SettingsView.cpp'))
$show=Get-SettingsProducer $native '(?m)^[ \t]*fire_and_forget\s+show\s*\(\s*\)\s*\{' 'show'
$hide=Get-SettingsProducer $native '(?m)^[ \t]*void\s+hide\s*\(\s*\)\s*\{' 'hide'
$apply=Get-SettingsProducer $native '(?m)^[ \t]*void\s+apply\s*\(\s*J\s+const\s*&\s*snapshot\s*\)\s*\{' 'apply'
$retire=Get-SettingsProducer $apply 'if\s*\(\s*!\s*model\s*\.\s*Size\s*\(\s*\)\s*\)\s*\{' 'shared retirement'
[IO.File]::WriteAllText((Join-Path $OutputDirectory 'settings-show.inc'),$show,[Text.UTF8Encoding]::new($false))
[IO.File]::WriteAllText((Join-Path $OutputDirectory 'settings-hide.inc'),$hide,[Text.UTF8Encoding]::new($false))
[IO.File]::WriteAllText((Join-Path $OutputDirectory 'settings-retire.inc'),$retire,[Text.UTF8Encoding]::new($false))
$source=Join-Path $PSScriptRoot '../tests/settings-show.cpp'
$object=Join-Path $OutputDirectory 'settings-show.obj'
$executable=Join-Path $OutputDirectory 'settings-show-test.exe'
& cl /nologo /std:c++20 /EHsc /W4 /WX $optimization ('/I'+$OutputDirectory) $source "/Fo$object" "/Fe$executable" /link @manifest
if($LASTEXITCODE -ne 0){throw 'Preferences show test build failed.'}
& $executable
if($LASTEXITCODE -ne 0){throw 'Preferences show test failed.'}

& {
 $tokens=$null;$errors=$null
 $ast=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot 'build.ps1'),[ref]$tokens,[ref]$errors)
 if($errors.Count){throw 'Build script parse failed'}
 $selection=$ast.FindAll({param($node) $node -is [Management.Automation.Language.AssignmentStatementAst] -and $node.Left.Extent.Text -in '$installation','$toolset'},$true)
 if($selection.Count -ne 2){throw 'Expected installation and toolset selection'}
 $expectedBuild=[pscustomobject]@{architecture='x64';hostArchitecture='x64';component='Microsoft.VisualStudio.Component.VC.Tools.x86.x64'}
 $build=$expectedBuild
 function Enter-WindowsDeveloperShell($Build) {
  if($Build -ne $expectedBuild){throw 'Toolset lookup used another architecture'}
  [pscustomobject]@{installationPath='C:\Visual Studio\C++';installationVersion=$case.version}
 }
 foreach($case in @(@{version='17.14.0';toolset='v143'},@{version='18.11.0';toolset='v145'})){
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
 $journey=@($ast.FindAll({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Run-Journey'},$true))
 if($journey.Count -ne 1){throw 'Localization journey must have one owner'}
 $journeyCalls=@($journey[0].FindAll({param($node) $node -is [Management.Automation.Language.CommandAst]},$true)|ForEach-Object {$_.GetCommandName()})
 foreach($phase in @('Preference-Numeric','Shortcut-Surfaces','Property-Surfaces','Panel-CopySurfaces','Notice-Surface')){if(@($journeyCalls|Where-Object {$_ -eq $phase}).Count -ne 1){throw "Localization journey lost or repeated $phase"}}
 foreach($phase in @('Header-MenuSurface','Profile-Surfaces','Tool-Surfaces','Filter-Surfaces')){if($phase -in $journeyCalls){throw "Localization journey repeated $phase"}}
 $partition=@($ast.FindAll({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Journey-Languages'},$true))
 if($partition.Count -ne 1){throw 'Localization Journey language partition must have one owner'}
 . ([scriptblock]::Create($partition[0].Extent.Text))
 foreach($count in @(15,16)){
  $choices=@(1..$count|ForEach-Object {[pscustomobject]@{index=$_;tag="tag-$_"}})
  $expected=@('1,2,3,4','5,6,7,8','9,10,11,12',((13..$count) -join ','))
  foreach($part in 1..4){if((@(Journey-Languages $choices $part).index -join ',') -cne $expected[$part-1]){throw 'Journey quarters changed language order or balance'}}
  $joined=@(foreach($part in 1..4){Journey-Languages $choices $part})
  if(($joined.tag -join ',') -cne ($choices.tag -join ',') -or @($joined.tag|Select-Object -Unique).Count -ne $count){throw 'Journey quarters repeated or lost a shipped tag'}
 }
 $allCalls=@($ast.FindAll({param($node) $node -is [Management.Automation.Language.CommandAst]},$true)|ForEach-Object {$_.GetCommandName()})
 foreach($phase in @('Tool-Surfaces','Filter-Surfaces','Preference-Numeric','Shortcut-Surfaces','Property-Surfaces','Panel-CopySurfaces','Run-Journey')){if(@($allCalls|Where-Object {$_ -eq $phase}).Count -ne 1){throw "Localization repeated or lost $phase"}}
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
 $profiles=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot 'LocalizationProfiles.ps1'),[ref]$tokens,[ref]$errors)
 if($errors.Count){throw 'Localization profile fixture did not parse'}
 foreach($name in @('Surface-Languages','Check-Copy')){
  $definition=$profiles.Find({param($node)$node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name},$true)
  if(!$definition){throw "Missing retained profile function: $name"}
  Invoke-Expression $definition.Extent.Text
 }
 $surface=@($profiles.FindAll({param($node)$node -is [Management.Automation.Language.CommandAst] -and $node.GetCommandName() -eq 'Surface-Languages' -and $node.CommandElements[1].Extent.Text -eq "'retained-source-profile'"},$true))
 if($surface.Count -ne 1){throw 'Expected one retained source profile language exercise'}
 $first=[pscustomobject]@{id=41};$second=[pscustomobject]@{id=42};$seen=@([pscustomobject]@{index=1;tag='en'})
 $choiceIdentity='4:1';$sourceIdentity='4:2'
 $script:profileModel=[pscustomobject]@{windows_active_tag='en';state=[pscustomobject]@{document_file=[pscustomobject]@{revision=7}};windows_document=[pscustomobject]@{details=[pscustomobject]@{source_profile='Embedded profile';feature_copy=[pscustomobject]@{color=[pscustomobject]@{current_source='Current source profile'}}}}}
 $sourceDocument=$script:profileModel.state.document_file|ConvertTo-Json -Depth 20 -Compress
 $script:profileChoice=[pscustomobject]@{Current=[pscustomobject]@{Name='Previous caption'}}
 $script:profileChoice|Add-Member ScriptMethod GetRuntimeId {if($script:profileCase -eq 'replaced'){4;9}else{4;1}}
 $script:profileSource=[pscustomobject]@{Current=[pscustomobject]@{Name='Embedded profile'}}
 $script:profileSource|Add-Member ScriptMethod GetRuntimeId {4;2}
 function Use-Window($Window){$script:profileWindow=$Window.id}
 function Language-Choice($Index){if($script:profileWindow -ne 42 -or $Index -ne 1){throw 'Retained profile language choice changed its publisher'}}
 function Model($Window){if($Window.id -ne 41){throw 'Retained profile read another window model'};$script:profileModel}
 function Fresh-Model($Window,$Tag){if($script:profileWindow -ne 41 -or $Window.id -ne 41 -or $Tag -ne 'en'){throw 'Retained profile assertions changed their target'};$script:profileAssertions++;$script:profileModel}
 function Find($Id){
  if($script:profileWindow -ne 41 -or $Id -ne 'document-profile-choice'){throw 'Retained profile acknowledgement inspected another control or window'}
  if($script:profileCase -ne 'missing'){$script:profileChoice}
 }
 function Control($Id){
  if($script:profileWindow -ne 41){throw 'Retained profile assertion inspected another window'}
  switch($Id){'document-profile-choice'{$script:profileChoice};'document-source-profile'{$script:profileSource};default{throw "Unexpected retained profile control: $Id"}}
 }
 function Catalog-Text($Tag,$Id){if($Tag -ne 'en' -or $Id -ne 'color-features-color-current-source'){throw 'Retained profile canonical check changed'};'Current source profile'}
 function Check-VisibleText($Id,$Expected){if($Id -ne 'document-source-current-label' -or $Expected -ne 'Current source profile'){throw 'Retained profile visible-copy assertion changed'}}
 function Capture-Window($Window,$Name){if($Window.id -ne 41 -or $Name -ne 'en-retained-source-profile'){throw 'Retained profile capture changed its target'};$script:profileCaptures++}
 function Wait-Until([scriptblock]$Condition,[string]$Message,[int]$Seconds){
  $script:profileWaits++
  if($Seconds -ne 30){throw 'Retained profile changed its existing acknowledgement bound'}
  foreach($publication in 1..2){
   if($publication -eq 2 -and $script:profileCase -eq 'caption'){$script:profileChoice.Current.Name='Current source profile'}
   $script:profilePolls++
   if(& $Condition){return}
   if($script:profileAssertions -or $script:profileCaptures){throw 'Retained profile asserted or captured before native acknowledgement'}
  }
  throw $Message
 }
 foreach($case in @('caption','stale','replaced','missing')){
  $script:profileCase=$case;$script:profileWaits=0;$script:profilePolls=0;$script:profileAssertions=0;$script:profileCaptures=0
  $script:profileChoice.Current.Name=if($case -eq 'replaced'){'Current source profile'}else{'Previous caption'}
  $failure=$null
  try{Invoke-Expression $surface[0].Extent.Text}catch{$failure=$_.Exception.Message}
  if($case -eq 'caption' -and $failure){throw $failure}
  if($script:profileWaits -ne 1 -or $script:profilePolls -ne 2){throw "Retained profile did not use one state acknowledgement budget: $case"}
  if($case -eq 'caption'){
   if($script:profileAssertions -ne 1 -or $script:profileCaptures -ne 1){throw 'Retained profile did not assert and capture exactly once after native publication'}
  }elseif($failure -ne 'Retained workflow did not adopt the new context' -or $script:profileAssertions -or $script:profileCaptures){throw "Retained profile accepted an unacknowledged native caption or identity: $case"}
 }
 foreach($case in @('stale','replaced')){
  $script:profileCase=$case;$script:profileChoice.Current.Name=if($case -eq 'replaced'){'Current source profile'}else{'Previous caption'}
  $failure=$null
  try{Check-Copy 'document-profile-choice' 'Current source profile' $choiceIdentity}catch{$failure=$_.Exception.Message}
  if($failure -ne 'Retained native control/caption changed incorrectly: document-profile-choice'){throw "Retained profile lost its original caption/identity assertion: $case"}
 }
}

& {
 $tokens=$null;$errors=$null
 $properties=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot 'LocalizationProperties.ps1'),[ref]$tokens,[ref]$errors)
 if($errors.Count){throw 'Localization property fixture did not parse'}
 $tool=$properties.Find({param($node)$node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Tool-Surfaces'},$false)
 $first=[pscustomobject]@{id=41};$script:transformPanelActive=$false;$script:missingAnchor=$false;$script:transformWaitObserved=$false
 function Toolbar-GroupedLanguageSurface {$script:transformPanelActive=$false}
 function Use-Window($Window){if($Window.id -ne 41){throw 'Transform setup changed its owned window'}}
 function Search-Command($Query,$Id){if($Id -cne 'scale_rotate'){throw 'Transform setup changed its command'}}
 function Copy-Panel($Id,$Probe){if($Id -cne 'tool_settings' -or $Probe -cne 'tool-setting-transform_x'){throw 'Transform setup selected the wrong panel'};$script:transformPanelActive=$true}
 function Model {[pscustomobject]@{state=[pscustomobject]@{layer_tools=[pscustomobject]@{tool='transform'}}}}
 function Find($Id){if($script:transformPanelActive -and $Id -in @('tool-setting-transform_x','tool-choice-transform-reference') -and !($script:missingAnchor -and $Id -eq 'tool-choice-transform-reference')){[pscustomobject]@{id=$Id}}}
 function Wait-Until([scriptblock]$Condition,[string]$Message,[int]$Seconds){$script:transformWaitObserved=$true;if(!(& $Condition)){throw $Message}}
 $prefix=@()
 foreach($statement in $tool.Body.EndBlock.Statements){
  $prefix+=$statement.Extent.Text
  if($statement -is [Management.Automation.Language.PipelineAst] -and $statement.PipelineElements[0] -is [Management.Automation.Language.CommandAst] -and $statement.PipelineElements[0].GetCommandName() -eq 'Wait-Until'){break}
 }
 $setup=[scriptblock]::Create(($prefix -join "`n"))
 & $setup
 if(!$script:transformPanelActive -or !$script:transformWaitObserved){throw 'Transform setup did not expose its actual native panel before checking controls'}
 $script:missingAnchor=$true;$rejected=$false
 try{& $setup}catch{if($_.Exception.Message -cne 'Transform did not expose retained native position and anchor controls'){throw};$rejected=$true}
 if(!$rejected){throw 'Transform setup accepted a missing native anchor'}
}

& {
 $tokens=$null;$errors=$null
 $properties=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot 'LocalizationProperties.ps1'),[ref]$tokens,[ref]$errors)
 if($errors.Count){throw 'Localization property fixture did not parse'}
 $tool=$properties.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Tool-Surfaces'},$false)
 if(!$tool){throw 'Tool surfaces fixture missing'}
 $statements=@($tool.Body.EndBlock.Statements)
 $check=-1;$start=-1
 for($i=0;$i -lt $statements.Count;$i++){
  $statement=$statements[$i]
  if($statement -is [Management.Automation.Language.AssignmentStatementAst] -and $statement.Left.Extent.Text -eq '$away'){$start=$i}
  if($statement -is [Management.Automation.Language.PipelineAst] -and $statement.PipelineElements[0] -is [Management.Automation.Language.CommandAst] -and $statement.PipelineElements[0].GetCommandName() -eq 'Wait-Until' -and $statement.Extent.Text.Contains('Previous tooltip did not retire before keyboard-only Tool Set copy acceptance')){$check=$i;break}
 }
 if($start -lt 0 -or $check -le $start){throw 'Tool Set tooltip park and acceptance check missing'}
 $prelude=(@($statements[$start..$check]|ForEach-Object {$_.Extent.Text}) -join "`n").Replace('[CapyRowPointer]','[CapyParkProbePointer]')
 Add-Type -TypeDefinition @'
public static class CapyParkProbePointer {
 public static int X;
 public static int Y;
 public static int HoverCount;
 public static bool FocusReady;
 public static bool FocusAtHover;
 public static void Hover(int x, int y) { X=x; Y=y; HoverCount++; FocusAtHover=FocusReady; }
}
'@
 function Control([string]$Id,[switch]$Arranged){if($Id -ne 'drawing-canvas' -or !$Arranged){throw 'Unexpected park control'};[pscustomobject]@{Current=[pscustomobject]@{BoundingRectangle=$script:parkCanvas}}}
 function Focus-Canvas {[CapyParkProbePointer]::FocusReady=$true}
 function Fresh-Model {[pscustomobject]@{state=[pscustomobject]@{camera=[pscustomobject]@{work_area=$script:parkArea}}}}
 function Capy-Tooltips([int]$ProcessId){$script:tooltipQueryCount++;if($ProcessId -ne 23){throw 'Tooltip check used another owner'}}
 function Wait-Until([scriptblock]$Condition,[string]$Message){
  if($Message -cne 'Previous tooltip did not retire before keyboard-only Tool Set copy acceptance'){throw 'Unexpected tooltip check'}
  $script:parkWaitCount++
  if(!(& $Condition)){throw 'Tool Set tooltip acceptance did not execute its zero-tooltip condition'}
 }
 foreach($case in @(
  @{rootX=100;rootY=50;canvasX=100;canvasY=98;area=@(72,135,1604.5,1384)},
  @{rootX=-800;rootY=200;canvasX=-760;canvasY=248;area=@(240,320,600,400)}
 )){
  $root=[pscustomobject]@{Current=[pscustomobject]@{BoundingRectangle=[pscustomobject]@{X=$case.rootX;Y=$case.rootY}}}
  $script:parkCanvas=[pscustomobject]@{Left=$case.canvasX;Top=$case.canvasY;Width=2560;Height=1528}
  $script:parkArea=$case.area;$script:parkWaitCount=0;$script:tooltipQueryCount=0;$review=[pscustomobject]@{Id=23}
  [CapyParkProbePointer]::X=0;[CapyParkProbePointer]::Y=0;[CapyParkProbePointer]::HoverCount=0;[CapyParkProbePointer]::FocusReady=$false;[CapyParkProbePointer]::FocusAtHover=$false
  & ([scriptblock]::Create($prelude))
  $x=[CapyParkProbePointer]::X;$y=[CapyParkProbePointer]::Y
  $areaLeft=$case.canvasX+$case.area[0];$areaTop=$case.canvasY+$case.area[1]
  if([CapyParkProbePointer]::HoverCount -ne 1 -or -not [CapyParkProbePointer]::FocusAtHover -or $script:parkWaitCount -ne 1 -or $script:tooltipQueryCount -ne 1){throw 'Tool Set park did not focus canvas before one hover and assert zero tooltips'}
  if($x -lt $areaLeft -or $x -ge $areaLeft+$case.area[2] -or $y -lt $areaTop -or $y -ge $areaTop+$case.area[3]){throw 'Tool Set park missed the active drawing work area'}
  if($x -ge $case.rootX+6 -and $x -lt $case.rootX+42 -and $y -ge $case.rootY+6 -and $y -lt $case.rootY+42){throw 'Tool Set park hovered the Zen tooltip owner'}
 }
}

& {
 $tokens=$null;$errors=$null
 $source=[IO.File]::ReadAllText((Join-Path $repo 'tools/windows-vm/run-fixtures.ps1'),[Text.Encoding]::UTF8)
 $ast=[Management.Automation.Language.Parser]::ParseInput($source,[ref]$tokens,[ref]$errors)
 if($errors.Count){throw 'VM runner parse failed'}
 $selection=$ast.FindAll({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Select-Fixtures'},$true)
 if($selection.Count -ne 1){throw 'Expected one fixture selection helper'}
 Invoke-Expression $selection[0].Extent.Text
 $available=@('shortcuts','localization','localization:light','localization:LargeText','localization:light-large','localization:RetainedToolsOnly','localization:retained-tools-light','localization:retained-tools-dark-large','localization:retained-tools-light-large','localization:JourneyOnly','localization:journey-light','localization:journey-dark-large','localization:journey-light-large','localization:journey-2-dark','localization:journey-2-light','localization:journey-2-dark-large','localization:journey-2-light-large','localization:journey-3-dark','localization:journey-3-light','localization:journey-3-dark-large','localization:journey-3-light-large','localization:journey-4-dark','localization:journey-4-light','localization:journey-4-dark-large','localization:journey-4-light-large')
 foreach($case in @(
  @{request=@();expected=$available},
  @{request=@('localization');expected=$available[1..($available.Count-1)]},
  @{request=@('localization:default');expected=@('localization')},
  @{request=@('localization:default','localization:LargeText');expected=@('localization','localization:LargeText')},
  @{request=@('localization:light','localization:light-large');expected=@('localization:light','localization:light-large')},
  @{request=@('localization:default','localization');expected=$available[1..($available.Count-1)]},
  @{request=@('localization:JourneyOnly','localization:retained-tools-light');expected=@('localization:retained-tools-light','localization:JourneyOnly')}
 )){
  if(((Select-Fixtures $available $case.request) -join ',') -ne ($case.expected -join ',')){throw 'Fixture selection changed the requested coverage'}
 }
 foreach($request in @('unknown','unknown:default','localization:default:light','localization:dark')){
  $rejected=$false;try{Select-Fixtures $available @($request)|Out-Null}catch{$rejected=$true}
  if(!$rejected){throw "Unknown fixture selector was accepted: $request"}
 }
 $variant=$ast.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-FixtureVariantArguments'},$true)
 Invoke-Expression $variant.Extent.Text
 $localizationTokens=$null;$localizationErrors=$null
 $localization=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot 'exercise-localization.ps1'),[ref]$localizationTokens,[ref]$localizationErrors)
 if($localizationErrors.Count){throw 'Localization selector source did not parse'}
 $routes=[ordered]@{localization=@()}
 foreach($parameter in $localization.ParamBlock.Parameters){
  $option=$parameter.Name.VariablePath.UserPath
  if($parameter.StaticType -eq [switch]){$routes["localization:$option"]=@(Get-FixtureVariantArguments 'localization' $option $null)}
  $choices=($parameter.Attributes|Where-Object {$_.TypeName.Name -eq 'ValidateSet'}).PositionalArguments.Value
  foreach($choice in $choices|Where-Object {$null -ne $_ -and $_ -ne $parameter.DefaultValue.Value}){$routes["localization:$choice"]=@(Get-FixtureVariantArguments 'localization' $option $choice)}
 }
 $expectedRoutes=@{
  'localization'=@();'localization:light'=@('-Theme','light');'localization:LargeText'=@('-LargeText');'localization:light-large'=@('-Theme','light-large');
  'localization:RetainedToolsOnly'=@('-RetainedToolsOnly');'localization:retained-tools-light'=@('-Theme','retained-tools-light');'localization:retained-tools-dark-large'=@('-Theme','retained-tools-dark-large');'localization:retained-tools-light-large'=@('-Theme','retained-tools-light-large')
  'localization:JourneyOnly'=@('-JourneyOnly');'localization:journey-light'=@('-Theme','journey-light');'localization:journey-dark-large'=@('-Theme','journey-dark-large');'localization:journey-light-large'=@('-Theme','journey-light-large')
  'localization:journey-2-dark'=@('-Theme','journey-2-dark');'localization:journey-2-light'=@('-Theme','journey-2-light');'localization:journey-2-dark-large'=@('-Theme','journey-2-dark-large');'localization:journey-2-light-large'=@('-Theme','journey-2-light-large')
  'localization:journey-3-dark'=@('-Theme','journey-3-dark');'localization:journey-3-light'=@('-Theme','journey-3-light');'localization:journey-3-dark-large'=@('-Theme','journey-3-dark-large');'localization:journey-3-light-large'=@('-Theme','journey-3-light-large')
  'localization:journey-4-dark'=@('-Theme','journey-4-dark');'localization:journey-4-light'=@('-Theme','journey-4-light');'localization:journey-4-dark-large'=@('-Theme','journey-4-dark-large');'localization:journey-4-light-large'=@('-Theme','journey-4-light-large')
 }
 foreach($name in $expectedRoutes.Keys){if(($routes[$name] -join ',') -cne ($expectedRoutes[$name] -join ',')){throw "Localization selector lost its exact arguments: $name"}}
 foreach($name in $expectedRoutes.Keys){if(!$routes.Contains($name)){throw "Localization lost an existing configuration: $name"}}
 $aliases=@($localization.EndBlock.Statements|Where-Object {$_ -is [Management.Automation.Language.IfStatementAst] -and $_.Extent.Text -match '^if\(\$Theme.StartsWith\('})
 foreach($case in @(
  @{name='retained-tools-light';mode='RetainedToolsOnly';theme='light';large=$false},@{name='retained-tools-dark-large';mode='RetainedToolsOnly';theme='dark';large=$true},@{name='retained-tools-light-large';mode='RetainedToolsOnly';theme='light';large=$true},
  @{name='journey-light';mode='JourneyOnly';theme='light';large=$false},@{name='journey-dark-large';mode='JourneyOnly';theme='dark';large=$true},@{name='journey-light-large';mode='JourneyOnly';theme='light';large=$true},
  @{name='journey-2-dark';mode='JourneyOnly';theme='dark';large=$false;part=2},@{name='journey-2-light';mode='JourneyOnly';theme='light';large=$false;part=2},@{name='journey-2-dark-large';mode='JourneyOnly';theme='dark';large=$true;part=2},@{name='journey-2-light-large';mode='JourneyOnly';theme='light';large=$true;part=2},
  @{name='journey-3-dark';mode='JourneyOnly';theme='dark';large=$false;part=3},@{name='journey-3-light';mode='JourneyOnly';theme='light';large=$false;part=3},@{name='journey-3-dark-large';mode='JourneyOnly';theme='dark';large=$true;part=3},@{name='journey-3-light-large';mode='JourneyOnly';theme='light';large=$true;part=3},
  @{name='journey-4-dark';mode='JourneyOnly';theme='dark';large=$false;part=4},@{name='journey-4-light';mode='JourneyOnly';theme='light';large=$false;part=4},@{name='journey-4-dark-large';mode='JourneyOnly';theme='dark';large=$true;part=4},@{name='journey-4-light-large';mode='JourneyOnly';theme='light';large=$true;part=4},
  @{name='retained-tools-light';mode='RetainedToolsOnly';theme='light';large=$true;input_large=$true},@{name='journey-light';mode='JourneyOnly';theme='light';large=$true;input_large=$true}
 )){
  $Theme=$case.name;$LargeText=[bool]$case.input_large;$RetainedToolsOnly=$false;$JourneyOnly=$false;$JourneyPart=1
  foreach($alias in $aliases){Invoke-Expression $alias.Extent.Text}
  $expectedPart=if($case.part){$case.part}else{1}
  if($Theme -cne $case.theme -or [bool]$LargeText -ne $case.large -or $JourneyPart -ne $expectedPart -or ![bool](Get-Variable -Name $case.mode -ValueOnly)){throw "Localization alias lost its theme, text scale or part: $($case.name)"}
 }
 . (Join-Path $PSScriptRoot 'CapyEnvironment.ps1')
 $start=$ast.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Start-Shortcuts'},$true)
 Invoke-Expression $start.Extent.Text
 $traces=Join-Path $OutputDirectory ('shortcut-traces-'+[Guid]::NewGuid().ToString('N'))
 New-Item -ItemType Directory $traces|Out-Null
 $priorContext=$env:CAPY_FIXTURE_CONTEXT
 try{
  $Executable=Join-Path $traces 'CapyCanvas.exe';$Output=$traces;$scripts=$traces
  Set-Content (Join-Path $traces 'CapyUia.ps1') 'function Owned-DrawingWindow {[pscustomobject]@{Handle=[IntPtr]2}}'
  $env:CAPY_FIXTURE_CONTEXT=Join-Path $traces 'context.json'
  $shortcutProcess=[pscustomobject]@{Id=4242;MainWindowHandle=[IntPtr]1;StartTime=(Get-Date).AddSeconds(-1)}
  $shortcutProcess|Add-Member -MemberType ScriptMethod -Name Refresh -Value {}
  function Start-Process {$shortcutProcess}
  $retired=Join-Path $traces 'ui-state-4242-1.json';$current=Join-Path $traces 'ui-state-4242-2.json'
  Set-Content $retired '{}';Set-Content $current '{}'
  function Get-ChildItem {Get-Item $retired;Get-Item $current}
  $arguments=@(Start-Shortcuts)
  if($arguments[-1] -ne $current){throw 'Shortcut fixture selected a retired window trace'}
 }finally{
  $env:CAPY_FIXTURE_CONTEXT=$priorContext
  Remove-Item -LiteralPath $traces -Recurse -Force
 }
}

& (Join-Path $PSScriptRoot '../tests/workspace-publisher-query.ps1')

& (Join-Path $PSScriptRoot '../tests/application-menu.ps1')

& (Join-Path $PSScriptRoot '../tests/application-menu-query.ps1')
& (Join-Path $PSScriptRoot '../tests/application-menu-invoke.ps1')

& (Join-Path $PSScriptRoot '../tests/paper-relief.ps1')
& (Join-Path $PSScriptRoot '../tests/stable-pixels.ps1')
& (Join-Path $PSScriptRoot '../tests/window-resize.ps1')

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
& (Join-Path $PSScriptRoot '../tests/document-invoke.ps1')
& (Join-Path $PSScriptRoot '../tests/recovery-fixture.ps1') -Output (Join-Path $OutputDirectory ('recovery-paths-'+[guid]::NewGuid().ToString('N')))



& {
    $native=[IO.File]::ReadAllText((Join-Path $PSScriptRoot '../LayerRowDrag.cpp'))
    $transition=[regex]::Match($native,'(?m)^        rejected\.reset\(\);ignoreClick=false;suppressContext=false;\+\+generation;[^\r\n]*')
    if(!$transition.Success){throw 'Layer contact generation transition is missing'}
    $source=Join-Path $OutputDirectory 'layer-generation-trace.cpp'
    $object=Join-Path $OutputDirectory 'layer-generation-trace.obj'
    $executable=Join-Path $OutputDirectory 'layer-generation-trace-test.exe'
    $harness=@'
#include <cassert>
#include <optional>
struct Contact {
    std::optional<int> rejected=7;
    bool ignoreClick=true,suppressContext=true,trace=true;
    unsigned generation=4,published=4;
    void publish(){published=generation;}
    void down(){
'@
    $harness+="`n"+$transition.Value+"`n    }`n};`n"+@'
int main(){
    Contact contact;
    contact.down();
    assert(contact.generation==5&&contact.published==5);
    assert(!contact.rejected&&!contact.ignoreClick&&!contact.suppressContext);
    contact.trace=false;contact.down();
    assert(contact.generation==6&&contact.published==5);
}
'@
    [IO.File]::WriteAllText($source,$harness)
    & cl /nologo /std:c++20 /EHsc /W4 /WX $optimization $source "/Fo$object" "/Fe$executable" /link @manifest
    if($LASTEXITCODE -ne 0){throw 'Layer contact generation test build failed'}
    & $executable
    if($LASTEXITCODE -ne 0){throw 'Layer contact generation trace stayed stale before contact validation'}
}

& {
    $native=[IO.File]::ReadAllText((Join-Path $PSScriptRoot '../WorkspaceView.cpp'))
    $gate=[regex]::Matches($native,'(?m)^ +[^\r\n]*data->apply\(configurationInputs,[^\r\n]*')
    if($gate.Count -ne 1){throw 'Workspace configuration gate is missing'}
    $source=Join-Path $OutputDirectory 'configuration-gate.cpp'
    $object=Join-Path $OutputDirectory 'configuration-gate.obj'
    $executable=Join-Path $OutputDirectory 'configuration-gate-test.exe'
    $harness=@'
#include <windows.h>
#include "NativeJson.h"
#include "CanvasModelUpdate.h"
#include <cassert>
#include <memory>
using namespace CapyUi;
struct Expansion {
    std::shared_ptr<CanvasModelSource> data;
    J geometry;
    int applied=0;
    void Apply(double){
        assert(!CanvasModelInputs::Capture::Active());++applied;
        geometry=str(object(data->state,L"customization"),L"expanded").empty()?J{}:J::Parse(LR"({"group":1})");
    }
};
struct View {
    std::shared_ptr<CanvasModelSource> data=std::make_shared<CanvasModelSource>();
    CanvasModelInputs configurationInputs;
    std::unique_ptr<Expansion> expansion=std::make_unique<Expansion>(Expansion{data});
    bool configured=false;
    int presented=0;
    double configurationHeight()const{return 0;}
    void updateConfiguration(){configured=!str(object(data->state,L"customization"),L"expanded").empty()||(configured&&expansion->geometry.Size());}
    void present(){
        assert(!CanvasModelInputs::Capture::Active());++presented;
        if(!expansion->geometry.Size()&&str(object(data->state,L"customization"),L"expanded").empty())configured=false;
    }
    void applyState(bool layoutChanged){
'@
    $harness+="`n"+$gate[0].Value+"`n    }`n};`n"+@'
int main(){
    winrt::init_apartment();
    View view;CanvasModelUpdate models;
    auto publish=[&](wchar_t const* packet,bool layoutChanged){
        models.Read(J::Parse(packet));CanvasModelSource::Adoption adoption(*view.data,models.Model(),models.changes);view.applyState(layoutChanged);
    };
    publish(LR"({"state":{"customization":{"expanded":"brush_size"},"brush":{"diameter":9}}})",true);
    assert(view.configured&&!view.expansion->applied&&!view.presented);
    view.expansion->Apply(0);view.present();
    publish(LR"({"model_update":[[["state","brush","diameter"],12]]})",false);
    assert(view.configured&&view.expansion->applied==1&&view.presented==1);
    publish(LR"({"model_update":[[["state","customization","expanded"],null]]})",false);
    assert(view.expansion->applied==2&&view.presented==2);
    assert(!view.configured&&!view.expansion->geometry.Size());
}
'@
    [IO.File]::WriteAllText($source,$harness)
    & cl /nologo /std:c++20 /EHsc /W4 /WX $optimization ('/I'+(Join-Path $PSScriptRoot '..')) $source "/Fo$object" "/Fe$executable" windowsapp.lib /link @manifest
    if($LASTEXITCODE -ne 0){throw 'Workspace configuration gate test build failed'}
    & $executable
    if($LASTEXITCODE -ne 0){throw 'A state-only configuration close kept the expanded panel configuration'}
}
