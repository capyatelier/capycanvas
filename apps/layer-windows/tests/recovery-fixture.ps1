param([Parameter(Mandatory)][string]$Output)
$ErrorActionPreference='Stop'
if(Test-Path -LiteralPath $Output){throw 'Fresh fixture output required'}
$Output=[IO.Directory]::CreateDirectory($Output).FullName
$scripts=Join-Path $PSScriptRoot '../scripts'
$tokens=$null;$errors=$null
$ast=[Management.Automation.Language.Parser]::ParseFile((Join-Path $scripts 'exercise-artwork-recovery.ps1'),[ref]$tokens,[ref]$errors)
if($errors.Count){throw 'Recovery fixture parse failed'}
$function=$ast.Find({param($node)$node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Failed-Source'},$true)
. ([scriptblock]::Create($function.Extent.Text))
$drawingSource=$Output
$heads=@('head.0','head.1','head.json')
$folder=[IO.Directory]::CreateDirectory((Join-Path $Output 'nested folder#')).FullName
$resource=Join-Path $folder ('paint '+[char]0x03b1+'.bin')
foreach($head in $heads){[IO.File]::WriteAllText((Join-Path $Output $head),'{"generation":7}')}
[IO.File]::WriteAllBytes($resource,[byte[]]@(1,2,3))
[IO.File]::WriteAllText((Join-Path $Output '.lock'),'lock')
$diagnostic=Join-Path $Output 'restore-error.txt'
[IO.File]::WriteAllText($diagnostic,'unreadable session')
$before=Failed-Source
$records=ConvertFrom-Json -InputObject $before
$relative=Join-Path 'nested folder#' (Split-Path -Leaf $resource)
if($records.Count -ne 4 -or @($records.path|Where-Object {$_ -in ($heads+@($relative))}).Count -ne 4){throw 'Recovery source paths or diagnostic exclusion changed'}
[IO.File]::WriteAllText($diagnostic,'retry failed')
if((Failed-Source) -cne $before){throw 'Recovery diagnostics changed the source signature'}
[IO.File]::WriteAllText((Join-Path $Output 'head.1'),'{"generation":8}')
if((Failed-Source) -ceq $before){throw 'Recovery signature missed changed head slot bytes'}
[IO.File]::WriteAllText((Join-Path $Output 'head.1'),'{"generation":7}')
if((Failed-Source) -cne $before){throw 'Restored head slot changed the source signature'}
[IO.File]::WriteAllBytes($resource,[byte[]]@(1,2,4))
if((Failed-Source) -ceq $before){throw 'Recovery signature missed changed artwork bytes'}
$ast=[Management.Automation.Language.Parser]::ParseFile((Join-Path $scripts 'collect-package-notices.ps1'),[ref]$tokens,[ref]$errors)
if($errors.Count){throw 'Notice collector parse failed'}
$assignment=$ast.Find({param($node)$node -is [Management.Automation.Language.AssignmentStatementAst] -and $node.Left.Extent.Text -eq '$dependencies[$key]'},$true)
$vendorRoot=[IO.Directory]::CreateDirectory((Join-Path $Output 'vendor')).FullName+[IO.Path]::DirectorySeparatorChar
$source=[IO.Directory]::CreateDirectory((Join-Path $vendorRoot ('crate #'+[char]0x03b1))).FullName
$localSource=$source;$name='fixture';$version='1.0.0';$license='MIT';$key='fixture-1.0.0';$dependencies=@{}
Invoke-Expression $assignment.Extent.Text
if($dependencies[$key].source -cne ('vendor/crate #'+[char]0x03b1)){throw 'Vendored notice source lost its relative path'}
Write-Output 'Recovery source preservation and notice paths passed'
