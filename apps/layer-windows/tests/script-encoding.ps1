$ErrorActionPreference='Stop'
$files=Get-ChildItem (Join-Path $PSScriptRoot '../scripts'),$PSScriptRoot -Filter '*.ps1' -File
foreach($file in $files){
    $bytes=[IO.File]::ReadAllBytes($file.FullName)
    $source=[Text.UTF8Encoding]::new($false,$true).GetString($bytes)
    if($source -match '[^\x00-\x7F]' -and !($bytes.Length -ge 3 -and $bytes[0] -eq 239 -and $bytes[1] -eq 187 -and $bytes[2] -eq 191)){
        throw "Unicode PowerShell source needs a UTF-8 BOM for Windows PowerShell: $($file.Name)"
    }
    $tokens=$null;$errors=$null
    [Management.Automation.Language.Parser]::ParseFile($file.FullName,[ref]$tokens,[ref]$errors)|Out-Null
    if($errors.Count){throw "$($file.Name): $($errors.Message -join '; ')"}
}
Write-Output "PowerShell source encoding and parsing passed for $($files.Count) scripts."
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$run=Join-Path $repo ('artifacts/windows/script-encoding/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$records=@([pscustomobject]@{tag='ja';title=([string][char]0x65e5+[char]0x672c+[char]0x8a9e);hint=([char]::ConvertFromUtf32(0x1f3a8)+[char]0x2026)})
$Name='captions';$layout=$records[0];$model=$records[0];$result=$records[0]|ConvertTo-Json -Depth 8
$OutputDirectory=$run;$layoutEvidence=$records[0];$elements=$records[0];$manifest=$records[0]
$sources=@(
    @{file='exercise-localization.ps1';needles=@('configuration-captions.json',"'-layout.json'","'-model.json'","Join-Path `$run 'results.json'");outputs=@('configuration-captions.json','captions-layout.json','captions-model.json','results.json')},
    @{file='CapyUia.ps1';needles=@("'-layout.json'","'-model.json'");outputs=@('captions-layout.json','captions-model.json')},
    @{file='capture-editor.ps1';needles=@('layout-$name.json','model-$name.json','elements-$name.json','fixtures.json');outputs=@('layout-captions.json','model-captions.json','elements-captions.json','fixtures.json')}
)
$writers=@(foreach($source in $sources){
    $ast=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot ('../scripts/'+$source.file)),[ref]$tokens,[ref]$errors)
    for($i=0;$i -lt $source.needles.Count;$i++){
        $needle=$source.needles[$i]
        $writer=$ast.Find({param($node)$node -is [Management.Automation.Language.PipelineAst] -and $node.Extent.Text.Contains($needle)}.GetNewClosure(),$true)
        if(!$writer){throw "Missing evidence writer in $($source.file): $needle"}
        @{node=$writer;file=$source.outputs[$i]}
    }
})
try{
    foreach($case in $writers){
        & ([scriptblock]::Create($case.node.Extent.Text))
        $bytes=[IO.File]::ReadAllBytes((Join-Path $run $case.file))
        if($bytes.Length -ge 3 -and $bytes[0] -eq 239 -and $bytes[1] -eq 187 -and $bytes[2] -eq 191){throw "Evidence JSON must omit the UTF-8 BOM: $($case.file)"}
        $actual=[Text.UTF8Encoding]::new($false,$true).GetString($bytes)|ConvertFrom-Json
        if($actual.title -cne $records[0].title -or $actual.hint -cne $records[0].hint){throw "Localization evidence lost Unicode text: $($case.file)"}
    }
}finally{foreach($case in $writers){[IO.File]::Delete((Join-Path $run $case.file))};[IO.Directory]::Delete($run)}
Write-Output "$($writers.Count) capture and localization evidence writers preserve Japanese, emoji and ellipsis as UTF-8 without BOM."
function Fixture-Ast([string]$Name){
    $tokens=$null;$errors=$null
    $ast=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot ('../scripts/'+$Name)),[ref]$tokens,[ref]$errors)
    if(@($errors).Count){throw "$Name failed parsing"};$ast
}
Add-Type -AssemblyName System.Drawing
foreach($fixture in @(@{name='exercise-localization.ps1';width=64;height=64},@{name='exercise-documents.ps1';width=128;height=96})){
    $ast=Fixture-Ast $fixture.name
    $guard=$ast.Find({param($n)$n -is [Management.Automation.Language.IfStatementAst] -and $n.Extent.Text.Contains('89504E470D0A1A0A')},$true)
    if(!$guard){throw 'Missing actual PNG header guard'}
    $bitmap=[Drawing.Bitmap]::new($fixture.width,$fixture.height);$stream=[IO.MemoryStream]::new()
    try{$bitmap.Save($stream,[Drawing.Imaging.ImageFormat]::Png);$png=$stream.ToArray()}finally{$bitmap.Dispose();$stream.Dispose()}
    $bytes=$png;$check=[scriptblock]::Create($guard.Clauses[0].Item1.Extent.Text)
    if(& $check){throw 'Actual fixture rejected known-size PNG bytes'}
    $png[0]=0;$bytes=$png
    if(!(& $check)){throw 'Actual fixture accepted corrupt PNG signature'}
}
$ast=Fixture-Ast 'exercise-snap.ps1'
$hashExpression=$ast.Find({param($n)$n -is [Management.Automation.Language.PipelineAst] -and $n.Extent.Text.Contains('ComputeHash($stream.ToArray())')},$true)
if(!$hashExpression){throw 'Missing actual snap hash expression'}
$expected='BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD'
$stream=[IO.MemoryStream]::new([byte[]]@(97,98,99));$hash=[Security.Cryptography.SHA256]::Create()
try{if((& ([scriptblock]::Create($hashExpression.Extent.Text))) -cne $expected){throw 'Actual snap expression changed SHA256 of abc'}}finally{$hash.Dispose();$stream.Dispose()}
$ast=Fixture-Ast 'exercise-localization.ps1'
$hashExpression=$ast.Find({param($n)$n -is [Management.Automation.Language.PipelineAst] -and $n.Extent.Text.Contains('ComputeHash($encoded.ToArray())')},$true)
if(!$hashExpression){throw 'Missing actual canvas hash expression'}
$encoded=[IO.MemoryStream]::new([byte[]]@(97,98,99));$sha=[Security.Cryptography.SHA256]::Create();$hash=$sha
try{if((& ([scriptblock]::Create($hashExpression.Extent.Text))) -cne $expected){throw 'Actual canvas expression changed SHA256 of abc'}}finally{$sha.Dispose();$encoded.Dispose()}
$clipboard=Fixture-Ast 'exercise-clipboard.ps1'
foreach($name in @('Json','Resource-Value')){
    $fn=$clipboard.Find({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq $name}.GetNewClosure(),$true)
    if(!$fn){throw 'Missing actual clipboard helper'};. ([scriptblock]::Create($fn.Extent.Text))
}
$leaf='leaf-'+$records[0].title+$records[0].hint;$nested='"'+$leaf+'"'
foreach($i in 1..80){$nested='{"branch":'+$nested+'}'}
$members=@([pscustomobject]@{Key='manifest.json';Value=[Text.Encoding]::UTF8.GetBytes($nested)});$After=$nested|ConvertFrom-Json;$decoded=0
foreach($name in @('exercise-clipboard.ps1','exercise-image-objects.ps1','ObjectFixture.ps1')){
    $ast=Fixture-Ast $name
    foreach($command in $ast.FindAll({param($n)$n -is [Management.Automation.Language.CommandAst] -and $n.GetCommandName() -eq 'ConvertFrom-Json'},$true)){
        $value=& ([scriptblock]::Create($command.Parent.Extent.Text))
        foreach($i in 1..80){if($value.GetType() -ne [Management.Automation.PSCustomObject]){throw 'Fixture JSON lost structured nested objects'};$value=$value.branch}
        if($value -cne $leaf){throw 'Fixture JSON truncated nested Unicode data'};$decoded++
    }
}
if($decoded -ne 4){throw 'Expected all four actual fixture JSON decode pipelines'}
$package=@{members=@([pscustomobject]@{Key='pack';Value=[byte[]]@(0,97,98,99,255)});manifest=@{resources=@(
    [pscustomobject]@{id='r';type='sample';encoding='raw';bytes='3';location=[pscustomobject]@{pack='pack';offset='1'};data=(' {"values":[true,null,2.5]} '|ConvertFrom-Json)},
    [pscustomobject]@{id='s';type='nested';encoding='raw';bytes='3';location=[pscustomobject]@{member='pack';offset='1'};data=('{"child":{"ref":"r"}}'|ConvertFrom-Json)}
)}}
$resolved=Resource-Value ('{"outer":[null,{"ref":"s"},false]}'|ConvertFrom-Json) $package
$resource=$resolved['outer'][1];$child=$resource['data']['child']
if($resolved['outer'][0] -ne $null -or $resolved['outer'][2] -ne $false -or $resource['type'] -cne 'nested' -or $resource['sha256'] -cne $expected -or $child['sha256'] -cne $expected -or $child['data']['values'][0] -ne $true -or $child['data']['values'][1] -ne $null -or $child['data']['values'][2] -ne 2.5){throw 'Actual recursive clipboard helper changed resource bytes or structured data'}
Write-Output 'PS5-compatible fixture APIs passed: known PNG guards, SHA256 bytes, four nested JSON decoders and recursive clipboard resources.'

Add-Type -AssemblyName System.IO.Compression.FileSystem
$archive=[IO.Compression.ZipFile]::OpenRead((Join-Path $repo 'crates/layer-core/src/package/codec/fixtures/shared-image-objects.capy'))
try{
    $reader=[IO.StreamReader]::new($archive.GetEntry('manifest.json').Open(),[Text.Encoding]::UTF8)
    try{$fixedManifest=$reader.ReadToEnd()|ConvertFrom-Json}finally{$reader.Dispose()}
}finally{$archive.Dispose()}
foreach($fixtureName in @('exercise-clipboard.ps1','ObjectFixture.ps1')){& {
    param($FixtureName,$Manifest)
    $ast=Fixture-Ast $FixtureName
    foreach($name in @('Json','Canonical','Root-Stack','Object-Layers','Affine','Effective-Affine','Front-Layer','Front-Object','Assert-ImageCopy')){
        $fn=$ast.Find({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq $name}.GetNewClosure(),$true)
        if($fn){. ([scriptblock]::Create($fn.Extent.Text))}
    }
    $packageInput=if($FixtureName -eq 'exercise-clipboard.ps1'){@{manifest=$Manifest}}else{$Manifest}
    $layers=@(Object-Layers $packageInput)
    if($layers.Count -ne 2 -or ($layers.data.name -join ',') -ne 'First,Second'){throw "$FixtureName did not find both fixed Object layers"}
    $first=@($layers|Where-Object {$_.data.name -eq 'First'})[0]
    $affine=@(Effective-Affine $packageInput $first);$expected=@(1,.125,.25,.875,16777230.125,-8388618.25)
    foreach($i in 0..5){if($affine[$i] -ne $expected[$i]){throw "$FixtureName changed the fixed effective affine at $i"}}
    if($FixtureName -ne 'ObjectFixture.ps1'){return}
    $front=Front-Object $Manifest
    if($front.id -ne '00000000000000000000000000000009'){throw 'Object fixture did not resolve the fixed front image'}
    $authored=$Manifest;$backLayer=@($layers|Where-Object id -ne $first.id)[0]
    $back=(Fixture-Ast 'exercise-image-rows.ps1').Find({param($n)$n -is [Management.Automation.Language.AssignmentStatementAst] -and $n.Left.Extent.Text -eq '$backObject'},$true)
    if(!$back){throw 'Missing actual back image lookup'}
    $backObject=& ([scriptblock]::Create($back.Right.Extent.Text))
    if($backObject.id -ne '0000000000000000000000000000000a'){throw 'Image rows did not resolve the fixed back image'}
    $after=(Json $Manifest)|ConvertFrom-Json;$copiedImage=(Json $front)|ConvertFrom-Json;$copiedLayer=(Json $first)|ConvertFrom-Json
    $copiedImage.id='fffffffffffffffffffffffffffffff1';$copiedLayer.id='fffffffffffffffffffffffffffffff2';$copiedLayer.data.content.object.ref=$copiedImage.id
    $after.objects+=@($copiedImage,$copiedLayer);$stack=Root-Stack $after;$stack.data.entries=@([pscustomobject]@{ref=$copiedLayer.id})+@($stack.data.entries)
    Assert-ImageCopy $Manifest $after 'Fixed image copy'
    $copiedLayer.data.content.object.ref=$front.id;$failure=$null
    try{Assert-ImageCopy $Manifest $after 'Wrong owner'}catch{$failure=$_.Exception.Message}
    if($failure -ne 'Wrong owner did not add one independent Object layer'){throw 'Image copy accepted the wrong image owner'}
} $fixtureName $fixedManifest}
Write-Output 'Fixed image fixture passed: ordinary Object owners, front/back image lookup, effective affine, independent copy and wrong-owner rejection.'

$newImage=(Fixture-Ast 'exercise-clipboard.ps1').Find({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq 'New-Image'},$true)
$affineLoop=$newImage.Find({param($n)$n -is [Management.Automation.Language.ForEachStatementAst] -and $n.Variable.Extent.Text -eq '$object' -and $n.Extent.Text.Contains('scaled or rotated a clipboard image')},$true)
if(!$affineLoop){throw 'Missing actual clipboard image affine loop'}
foreach($case in @(
    @{data='{}';scaled=$false},
    @{data='{"affine":[1.0,0.0,0.0,1.0,20.0,0.0]}';scaled=$false},
    @{data='{"affine":[1.0,0.0,0.0,1.0,0.0,5.0]}';scaled=$false},
    @{data='{"affine":[0.999999999999999,0.0,0.0,1.0,20.0,0.0]}';scaled=$true},
    @{data='{"affine":[1.0,0.25,0.0,1.0,20.0,0.0]}';scaled=$true},
    @{data='{"affine":[0.0,1.0,-1.0,0.0,20.0,0.0]}';scaled=$true}
)){
    $manifest=('{"objects":[{"type":"capy.image-object/1","data":'+$case.data+'}]}')|ConvertFrom-Json;$Name='Numeric affine fixture';$failure=$null
    try{. ([scriptblock]::Create($affineLoop.Extent.Text))}catch{$failure=$_.Exception.Message}
    if($case.scaled){if($failure -ne "$Name scaled or rotated a clipboard image"){throw 'Actual clipboard check accepted a changed linear transform'}}
    elseif($failure){throw "Actual clipboard check rejected numeric identity: linear=$($affine[0..3] -join ','); types=$(@($affine[0..3]|ForEach-Object {$_.GetType().FullName}) -join ','); $failure"}
}
Write-Output 'Clipboard affine checks compare exact numbers: default and translated identity accepted; scale, shear and rotation rejected.'

& {
    $idle=Fixture-Ast 'exercise-idle-brush.ps1'
    $idlePolicy=@($idle.EndBlock.Statements|Where-Object {$_ -is [Management.Automation.Language.IfStatementAst] -and $_.Extent.Text.Contains('$env:CAPY_WAIT_SCALE')})
    if($idlePolicy.Count -ne 1){throw 'Missing actual idle readiness deadline policy'}
    $previous=$env:CAPY_WAIT_SCALE
    try{
        foreach($case in @(@{wet=$false;scale='3';expected=$null},@{wet=$true;scale='3';expected='3'},@{wet=$false;scale=$null;expected=$null})){
            $WetRoundOnly=$case.wet;$env:CAPY_WAIT_SCALE=$case.scale
            . ([scriptblock]::Create($idlePolicy[0].Extent.Text))
            if($env:CAPY_WAIT_SCALE -ne $case.expected){throw 'Idle readiness did not select its functional deadlines'}
        }
    }finally{$env:CAPY_WAIT_SCALE=$previous}
}
Write-Output 'Idle readiness owns its functional deadlines without changing Wet Round scope.'
