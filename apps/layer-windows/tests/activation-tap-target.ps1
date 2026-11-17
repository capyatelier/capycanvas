param([string]$FixturePath=(Join-Path $PSScriptRoot '../scripts/exercise-activation.ps1'))
$ErrorActionPreference='Stop'
Add-Type -AssemblyName WindowsBase
$tokens=$null;$errors=$null
$tree=[Management.Automation.Language.Parser]::ParseFile($FixturePath,[ref]$tokens,[ref]$errors)
if($errors.Count){throw ($errors|Out-String)}
$tap=$tree.Find({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -ceq 'Native-Tap'},$true)
if(!$tap){throw 'Actual activation contact helper is missing'}
$guards=@($tap.Body.EndBlock.Statements|Where-Object {($_ -is [Management.Automation.Language.IfStatementAst] -or $_ -is [Management.Automation.Language.ForEachStatementAst]) -and $_.Extent.Text.Contains('moved before native input')})
if($guards.Count -ne 1){throw 'Actual activation contact has no unique target admission guard'}
$down=$tap.Body.Find({param($n)$n -is [Management.Automation.Language.InvokeMemberExpressionAst] -and $n.Member.Value -ceq 'Down'},$true)
if(!$down -or $guards[0].Extent.EndOffset -ge $down.Extent.StartOffset){throw 'Actual target admission no longer precedes Down'}
$guard=[scriptblock]::Create($guards[0].Extent.Text)
$Id='zoom-200';$box=[Windows.Rect]::new(1493,980,534,51);$x=1760;$y=1006;$dx=0;$dy=0
$cases=@(
 @{name='retained clipped height';after='1493,980,534,50';accept=$true},
 @{name='unchanged';after='1493,980,534,51';accept=$true},
 @{name='extent expands';after='1493,980,534,55';accept=$true},
 @{name='origin moves horizontally';after='1494,980,534,51';accept=$false},
 @{name='origin moves vertically';after='1493,981,534,51';accept=$false},
 @{name='frozen point clipped';after='1493,980,534,26';accept=$false},
 @{name='drift endpoint clipped';after='1493,980,534,50';accept=$false;dy=24},
 @{name='native target replaced';after='1493,980,534,51';accept=$false;replaced=$true}
)
foreach($case in $cases){
 $dy=if($case.ContainsKey('dy')){$case.dy}else{0}
 $afterRuntime=if($case.replaced){@(42,917702,4,992)}else{@(42,917702,4,991)}
 $script:lastInput=@{before_hover=@{bounds=$box.ToString();runtime_id=@(42,917702,4,991)};before_down=@{bounds=$case.after;runtime_id=$afterRuntime}}
 $failure=$null;try{& $guard}catch{$failure=$_}
 if($case.accept -and $failure){throw $failure}
 if(!$case.accept -and (!$failure -or $failure.Exception.Message -cne 'Invalid run: target zoom-200 moved before native input')){throw "Actual activation target guard accepted an invalid path or failed elsewhere: $($case.name); $failure"}
}
'Activation target admission: 8 actual-guard schedules passed; retained extent-only change keeps the original frozen point and no tolerance.'
