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
& (Join-Path $PSScriptRoot 'fixture-type-references.ps1')
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

& {
    $photo=Fixture-Ast 'exercise-photo-memory.ps1'
    $definition=$photo.Find({param($n)$n -is [Management.Automation.Language.CommandAst] -and $n.GetCommandName() -eq 'Add-Type' -and $n.Extent.Text.Contains('public static class CapyPhotoMemoryPng')},$true)
    $acquire=$photo.Find({param($n)$n -is [Management.Automation.Language.TryStatementAst] -and $n.Body.Statements[0].Extent.Text -eq '$p=Get-Process -Id $child.ProcessId'},$true)
    $guard=$photo.Find({param($n)$n -is [Management.Automation.Language.IfStatementAst] -and $n.Extent.Text.StartsWith('if($child.path -ine $helper')},$true)
    if(!$definition -or !$acquire -or !$guard){throw 'Missing actual photo monitor acquisition or executable identity guard'}
    & ([scriptblock]::Create($definition.Extent.Text))
    $self=[Diagnostics.Process]::GetCurrentProcess();$probe=[powershell]::Create();$probe.Runspace=[runspacefactory]::CreateRunspace();$probe.Runspace.Open()
    try{
        $null=$probe.AddScript({param($Acquire,$Guard,$Owner,$Expected)
            $ErrorActionPreference='Stop';$children=@{}
            $child=[pscustomobject]@{ProcessId=$Owner;ExecutablePath=$null;CommandLine='owned process';ParentProcessId=$Owner}
            & ([scriptblock]::Create($Acquire))
            if(!$children.ContainsKey([int]$Owner)){throw 'Actual photo monitor did not retain the owned process'}
            $entry=$children[[int]$Owner]
            try{
                $child=@{path=$entry.path;exited=$true;exit_code=0};$helper=$Expected
                & ([scriptblock]::Create($Guard))
                $helper=$Expected+'.wrong';$rejected=$false
                try{& ([scriptblock]::Create($Guard))}catch{$rejected=$_.Exception.Message -eq 'Owned photo decoder path or exit status differs'}
                if(!$rejected){throw 'Actual photo monitor accepted the wrong executable identity'}
            }finally{$entry.process.Dispose()}
        }).AddArgument($acquire.Extent.Text).AddArgument($guard.Extent.Text).AddArgument($self.Id).AddArgument($self.MainModule.FileName)
        $null=$probe.Invoke();if($probe.Streams.Error.Count){throw ($probe.Streams.Error|Out-String)}
    }finally{$probe.Runspace.Dispose();$probe.Dispose();$self.Dispose()}
}
Write-Output 'Photo monitor resolves null CIM paths through the retained owned handle and rejects another executable identity.'

& {
    $color=Fixture-Ast 'exercise-compact-color.ps1'
    $capture=@($color.FindAll({param($n)$n -is [Management.Automation.Language.CommandAst] -and $n.GetCommandName() -eq 'Capture' -and $n.Extent.Text -eq "Capture 'retained-drawer'"},$true))
    $restore=@($color.FindAll({param($n)$n -is [Management.Automation.Language.AssignmentStatementAst] -and $n.Left.Extent.Text -eq '$root' -and $n.Right.Extent.Text -eq '$windowRoot'},$true))
    if($capture.Count -ne 1 -or $restore.Count -ne 1){throw 'Missing actual retained drawer capture and window restoration'}
    $start=[Math]::Min($capture[0].Extent.StartOffset,$restore[0].Extent.StartOffset)
    $end=[Math]::Max($capture[0].Extent.EndOffset,$restore[0].Extent.EndOffset)
    $windowRoot=[pscustomobject]@{Current=[pscustomobject]@{NativeWindowHandle=42}}
    $root=[pscustomobject]@{Current=[pscustomobject]@{NativeWindowHandle=0}};$observed=@{captures=0}
    function Capture([string]$Name){
        if($root.Current.NativeWindowHandle -ne 42){throw 'Retained drawer capture used a child instead of the owned window'}
        $observed.captures++
    }
    . ([scriptblock]::Create($color.Extent.Text.Substring($start,$end-$start)))
    if($observed.captures -ne 1 -or $root -ne $windowRoot){throw 'Retained drawer capture lost the owned window context'}
}
Write-Output 'Retained color drawer captures use the owned native window after scoped control checks.'

$noticeSource=Fixture-Ast 'LocalizationProperties.ps1'
$noticePreparation=$noticeSource.Find({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq 'Notice-Surface'},$true)
$noticeCommands=@($noticePreparation.Body.EndBlock.Statements|Where-Object {$_ -is [Management.Automation.Language.PipelineAst]}|ForEach-Object {$_.PipelineElements}|Where-Object {$_ -is [Management.Automation.Language.CommandAst]})
$prepare=@($noticeCommands|Where-Object {$_.GetCommandName() -eq 'Property-Panel' -and $_.CommandElements[1].Value -eq 'layers'})
$select=@($noticeCommands|Where-Object {$_.Extent.Text -ceq 'Invoke-Id "layer-$layer-name"'})
if($prepare.Count -ne 1 -or $select.Count -ne 1 -or $prepare[0].Extent.StartOffset -ge $select[0].Extent.StartOffset){throw 'Notice image selection must prepare Layers with Property-Panel before invoking its authored image'}
Write-Output 'Notice image selection uses the existing panel preparation before invoking the authored image.'

& {
    $tools=(Fixture-Ast 'LocalizationProperties.ps1').Find({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq 'Tool-Surfaces'},$true)
    $start=$tools.Extent.Text.LastIndexOf("`n",$tools.Extent.Text.IndexOf('$tooltipIndex=-1'))+1;$end=$tools.Extent.Text.IndexOf(' $away=',$start)
    if($end -lt $start){throw 'Missing actual Tool Set tooltip preparation'}
    $action=@{type='choose_tool_variant';anchor=@{kind='tile';panel='toolbar';tile=48};variant=@{type='command';command='move'}}
    $model=@{state=@{tool_set=@{groups=@();subtools=@(@{enabled=$false;label='Disabled'},@{enabled=$true;disabled_reason='Unavailable';label='Explained'},@{enabled=$true;label='English';action=$action})};document_file=@{revision=4}};windows_gpu_generation=3}
    $control=[pscustomobject]@{Current=[pscustomobject]@{HelpText=''}};$control|Add-Member ScriptMethod GetRuntimeId {[int[]]@(1,2)}
    function Fresh-Model{$model}
    function Copy-Panel([string]$Panel,[string]$Probe){if($Panel -ne 'brushes' -or $Probe -ne 'tool-subtool-2'){throw 'Tool Set tooltip selected another native projection'}}
    function Control([string]$Id){if($Id -ne 'tool-subtool-2'){throw 'Tool Set tooltip addressed a nonexistent group'};$control}
    . ([scriptblock]::Create($tools.Extent.Text.Substring($start,$end-$start)))
    if($tooltipId -ne 'tool-subtool-2' -or $tooltipIndex -ne 2){throw 'Tool Set tooltip accepted a disabled or explained choice'}
    $guard=$tools.Find({param($n)$n -is [Management.Automation.Language.IfStatementAst] -and $n.Extent.Text.Contains('Tool Set tooltip target/action/reason changed during copy publication')},$true)
    $check=[scriptblock]::Create($guard.Extent.Text);$item=$model.state.tool_set.subtools[2]
    & $check
    foreach($case in @('reason','help','action')){
        switch($case){'reason'{$item.disabled_reason='Unavailable'};'help'{$control.Current.HelpText='Unrelated'};'action'{$item.action=@{type='another_action'}}}
        $rejected=$false;try{& $check}catch{if($_.Exception.Message -ne 'Tool Set tooltip target/action/reason changed during copy publication'){throw};$rejected=$true}
        if(!$rejected){throw "Tool Set tooltip retained guard accepted changed $case"}
        $item.Remove('disabled_reason');$control.Current.HelpText='';$item.action=$action
    }
    $caption=$tools.Find({param($n)$n -is [Management.Automation.Language.CommandAst] -and $n.GetCommandName() -eq 'Wait-Until' -and $n.Extent.Text.Contains('Retained keyboard Tool Set tooltip kept an earlier exact caption')},$true)
    $captionCheck=[scriptblock]::Create($caption.CommandElements[1].ScriptBlock.Extent.Text.TrimStart('{').TrimEnd('}'))
    $tip=[pscustomobject]@{Current=[pscustomobject]@{Name='Japanese'}};$review=@{Id=33};$item.label='Japanese'
    function Capy-Tooltips{$tip}
    if(!(& $captionCheck)){throw 'Tool Set tooltip rejected its current localized leaf caption'}
    $tip.Current.Name='English';if(& $captionCheck){throw 'Tool Set tooltip accepted its previous leaf caption'}
}
Write-Output 'Tool Set localization uses enabled leaf rows and preserves current tooltip captions, help, reasons and actions.'
$noticeReady=@($noticeSource.FindAll({param($n)$n -is [Management.Automation.Language.CommandAst] -and $n.GetCommandName() -eq 'Wait-Until' -and $n.Extent.Text.EndsWith("'The retained visible native notice did not adopt its shared text and actions before expiry' 2")},$true))
if($noticeReady.Count -ne 1){throw 'Missing actual localized notice readiness condition'}
& {
    $condition=[scriptblock]::Create($noticeReady[0].CommandElements[1].ScriptBlock.Extent.Text.TrimStart('{').TrimEnd('}'))
    $snapshot=[pscustomobject]@{Name='Japanese';BoundingRectangle='same'}
    $noticeFrame=[pscustomobject]@{Snapshot=$snapshot};$noticeFrame|Add-Member ScriptMethod GetUpdatedCache {param($cache)[pscustomobject]@{Cached=$this.Snapshot}}
    $notice=@{owner=$null;bounds='same';stable=0};$before=@{name='English';actions=@()};$noticeCache=$null;$review=@{Id=33};$geometry=@{view=@{state=@{palette=@{}}}};$density=1.5;$paint=@{samples=0}
    function Test-CapySearchItem{$true}
    function Notice-Painted{$paint.samples++;$paint.samples -ge 2}
    if(& $condition){throw 'Localized notice accepted UIA before requested pixels were painted'}
    if(!(& $condition) -or $paint.samples -ne 2){throw 'Localized notice did not acknowledge its requested pixels in its existing readiness condition'}
}
if($env:OS -eq 'Windows_NT'){
    Add-Type -AssemblyName System.Drawing
    $colors=$noticeSource.Find({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq 'Notice-ActionColors'},$true)
    . ([scriptblock]::Create($colors.Extent.Text))
    $bitmap=[pscustomobject]@{Colors=@{'6,10'=[Drawing.Color]::FromArgb(255,63,63,63);'26,10'=[Drawing.Color]::FromArgb(255,63,63,63)}}
    $bitmap|Add-Member ScriptMethod GetPixel {param($x,$y)$this.Colors["$x,$y"]}
    $window=@{X=0;Y=0};$offers=@(@{id='accent';accent=$true},@{id='plain';accent=$false})
    $layout=@{actions=@(@{id='accent';bounds=@{Left=0;Y=0;Height=20}},@{id='plain';bounds=@{Left=20;Y=0;Height=20}})}
    $palette=@{accent='#0078D4';glass=@{panel=@((63/255.),(63/255.),(63/255.),1)}}
    if((Notice-ActionColors $bitmap $window $offers $layout $palette 1).valid){throw 'Notice colors accepted pixels from the previous layout'}
    $bitmap.Colors['6,10']=[Drawing.ColorTranslator]::FromHtml('#0078D4')
    if(!(Notice-ActionColors $bitmap $window $offers $layout $palette 1).valid){throw 'Notice colors rejected the requested action fills'}
    $bitmap.Colors['6,10']=[Drawing.ColorTranslator]::FromHtml('#0078D3')
    if((Notice-ActionColors $bitmap $window $offers $layout $palette 1).valid){throw 'Notice colors relaxed the exact accent fill'}
}
Write-Output 'Localized notice readiness waits for its exact action fills within its existing deadline.'

& {
    $documents=Fixture-Ast 'exercise-documents.ps1'
    $assert=$documents.Find({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq 'Assert-DocumentStderr'},$true)
    $close=@($documents.FindAll({param($n)$n -is [Management.Automation.Language.CommandAst] -and $n.GetCommandName() -eq 'Wait-Closed' -and $n.Extent.Text -eq "Wait-Closed 'Session close did not finish'"},$true))
    if(!$assert -or $close.Count -ne 1){throw 'Missing actual document close stderr assertion'}
    . ([scriptblock]::Create($assert.Extent.Text))
    $checks=@($close[0].Parent.Parent.Statements|Where-Object {$_.Find({param($n)$n -is [Management.Automation.Language.CommandAst] -and $n.GetCommandName() -eq 'Assert-DocumentStderr'},$true)})
    if($checks.Count -ne 1){throw 'Missing actual document close stderr reader'}
    $start=$close[0].Parent.Extent.EndOffset
    $read=[scriptblock]::Create($documents.Extent.Text.Substring($start,$checks[0].Extent.EndOffset-$start))
    $logDirectory=Join-Path $repo ('artifacts/windows/script-encoding/'+[Guid]::NewGuid().ToString('N'))
    [IO.Directory]::CreateDirectory($logDirectory)|Out-Null
    $stderr=Join-Path $logDirectory 'live-stderr.log'
    $writer=[IO.File]::Open($stderr,[IO.FileMode]::Create,[IO.FileAccess]::Write,([IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete))
    try{
        $bytes=[Text.Encoding]::UTF8.GetBytes("Drawing open: Unknown layer blend future-package-blend`n")
        $writer.Write($bytes,0,$bytes.Length);$writer.Flush()
        & $read
        $bytes=[Text.Encoding]::UTF8.GetBytes("unexpected diagnostic`n")
        $writer.Write($bytes,0,$bytes.Length);$writer.Flush();$rejected=$false
        try{& $read}catch{if($_.Exception.Message -ne 'Native review reported stderr'){throw};$rejected=$true}
        if(!$rejected){throw 'Actual document stderr assertion accepted an unexpected live diagnostic'}
    }finally{$writer.Dispose();[IO.File]::Delete($stderr);[IO.Directory]::Delete($logDirectory)}
}
Write-Output 'Document stderr reads share the retained live writer and preserve permitted and unexpected diagnostic assertions.'

& {
    $variations=Fixture-Ast 'exercise-tool-variations.ps1'
    $guard=$variations.Find({param($n)$n -is [Management.Automation.Language.IfStatementAst] -and $n.Extent.Text.Contains('Sketch did not retain every default header tool')},$true)
    if(!$guard){throw 'Missing actual Sketch header contract'}
    $keys=@($guard.Parent.Statements|Where-Object {$_ -is [Management.Automation.Language.AssignmentStatementAst] -and $_.Left.Extent.Text -eq '$keys'})
    if($keys.Count -ne 1){throw 'Missing actual Sketch header identities'}
    $check=[scriptblock]::Create("param(`$entries)`n"+$keys[0].Extent.Text+"`n"+$guard.Extent.Text)
    $entries=@(foreach($key in @('panel:adjustments','command:select','command:scale_rotate','command:drawing_brush','command:sculpt','command:eraser','tool_slot:fill','panel:layers','color:')){
        $parts=$key.Split(':');$control=@{kind=$parts[0]}
        switch($parts[0]){'panel'{$control.panel=$parts[1]};'command'{$control.command=$parts[1]};'tool_slot'{$control.slot=$parts[1]}}
        [pscustomobject]@{item=[pscustomobject]@{control=[pscustomobject]$control}}
    })
    & $check $entries
    $entries[6].item.control.slot='photo_fill';$rejected=$false
    try{& $check $entries}catch{if($_.Exception.Message -ne 'Sketch did not retain every default header tool'){throw};$rejected=$true}
    if(!$rejected){throw 'Sketch header contract accepted the wrong remembered Fill slot'}
    $entries[6].item.control.slot='fill';$rejected=$false
    try{& $check @($entries|Where-Object {$_.item.control.kind -ne 'tool_slot'})}catch{if($_.Exception.Message -ne 'Sketch did not retain every default header tool'){throw};$rejected=$true}
    if(!$rejected){throw 'Sketch header contract accepted a missing remembered Fill slot'}
}
Write-Output 'Actual Sketch header contract retains all authored tools and distinguishes the remembered Fill slot from another or missing slot.'
