param([string]$ExpansionFixture=(Join-Path $PSScriptRoot '../scripts/exercise-expansion.ps1'))
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot '../scripts/ComposedFrames.ps1')
$baseline=[Drawing.Bitmap]::new(4,4);$changed=$null
try{
    $changed=[Drawing.Bitmap]::new(4,4)
    for($y=0;$y -lt 4;$y++){for($x=0;$x -lt 4;$x++){
        $baseline.SetPixel($x,$y,[Drawing.Color]::Black)
        $changed.SetPixel($x,$y,[Drawing.Color]::Black)
    }}
    $envelope=[TabFrames+Envelope]::new($baseline,$baseline)
    if($envelope.Difference($changed) -ne 0){throw 'Composed frames changed an identical black image'}
    $changed.SetPixel(1,1,[Drawing.Color]::White)
    if($envelope.Difference($changed) -ne 1){throw 'Composed frames did not identify exactly one changed pixel'}
}finally{
    if($changed){$changed.Dispose()}
    $baseline.Dispose()
}
$images=@{};$tint=[Drawing.Color]::FromArgb(143,255,255,255)
try{
    foreach($name in 'held','final','double','gap','darker','stale','hole'){$images[$name]=[Drawing.Bitmap]::new(60,24)}
    for($y=0;$y -lt 24;$y++){for($x=0;$x -lt 60;$x++){
        $strip=$x -ge 40 -and $x -lt 52;$icon=$strip -and $x -ge 44 -and $x -lt 47 -and $y -ge 10 -and $y -lt 13
        $held=if($x -ge 20 -and $x -lt 23 -and $y -ge 5 -and $y -lt 8){46}elseif($x -ge 10 -and $x -lt 56){224}else{184}
        $final=if($icon){46}elseif($strip){224}elseif($x -ge 37 -and $x -lt 40){178}else{184}
        $values=@{held=$held;final=$final;double=$(if($strip -and !$icon){241}else{$final});gap=$(if($strip -and !$icon){184}else{$final});darker=$(if($strip -and !$icon){200}else{$final})}
        $values.stale=if($x -ge 20 -and $x -lt 30 -and $y -ge 15 -and $y -lt 17){100}else{$values.double}
        $values.hole=if($x -ge 48 -and $x -lt 51 -and $y -ge 16 -and $y -lt 20){184}else{$values.double}
        foreach($name in $values.Keys){$images[$name].SetPixel($x,$y,[Drawing.Color]::FromArgb($values[$name],$values[$name],$values[$name]))}
    }}
    $envelope=[TabFrames+Envelope]::new($images.held,$images.final);$column=[Drawing.Rectangle]::new(40,0,12,24)
    foreach($name in 'double','gap','darker','stale','hole'){if($envelope.Difference($images[$name]) -le 8){throw "Composed frames did not flag the $name column state"}}
    if(!$envelope.ExtraTint($images.double,$images.final,$column,[Drawing.Rectangle]::Empty,$tint)){throw 'Composed frames did not recognize one extra column tint'}
    foreach($name in 'gap','darker','stale','hole'){if($envelope.ExtraTint($images[$name],$images.final,$column,[Drawing.Rectangle]::Empty,$tint)){throw "Composed frames accepted the $name column state as one extra tint"}}
}finally{foreach($image in $images.Values){$image.Dispose()}}
Write-Output 'Composed frames actual helper compiled and compared memory pixels'

& {
    $tokens=$null;$errors=$null
    $ast=[Management.Automation.Language.Parser]::ParseFile($ExpansionFixture,[ref]$tokens,[ref]$errors)
    if($errors.Count){throw ($errors|Out-String)}
    $check=$ast.Find({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq 'Check-NarrowExpansion'},$true)
    $assignment=$check.Find({param($n)$n -is [Management.Automation.Language.AssignmentStatementAst] -and $n.Left.Extent.Text -ceq '$expanded'},$true)
    if(!$assignment){throw 'Missing actual narrow-expansion trajectory oracle'}
    $statements=$assignment.Parent.Statements;$start=-1;$end=-1
    for($i=0;$i -lt $statements.Count;$i++){
        if($statements[$i].Extent.StartOffset -eq $assignment.Extent.StartOffset){$start=$i}
        if($statements[$i] -is [Management.Automation.Language.AssignmentStatementAst] -and $statements[$i].Left.Extent.Text -ceq '$configuration'){$end=$i;break}
    }
    if($start -lt 0 -or $end -le $start){throw 'Missing actual narrow-expansion geometry assertions'}
    if($start -gt 0 -and $statements[$start-1].Extent.Text.Contains('Narrow expansion native geometry history overflowed')){$start--}
    $oracle=[scriptblock]::Create(($statements[$start..($end-1)]|ForEach-Object {$_.Extent.Text}) -join "`n")
    function Set-Content{param($Path,[Parameter(ValueFromPipeline)]$Value)process{}}
    function Geometry([double]$Width,[double]$Height,[double]$ConfigurationWidth=60){
        [pscustomobject]@{group=4;bounds=[pscustomobject]@{width=$Width;height=$Height};configuration=[pscustomobject]@{width=$ConfigurationWidth}}
    }
    function Frame([int]$Serial,[double]$Width,[double]$Height){[pscustomobject]@{frame=$Serial;panel='sizes';geometry=(Geometry $Width $Height)}}
    $checks=0
    foreach($case in 'coalesced-uia','all-events','only-final','one-intermediate','repeated-geometry','wrong-height','wrong-configuration','stale','foreign-group','foreign-panel','unfinished','overflow','previous-overflow'){
        $Panel='sizes';$placement=@([pscustomobject]@{id=4});$scale=1.;$folder=$PSScriptRoot
        $docked=[pscustomobject]@{Width=100.;Height=200.};$final=Geometry 200 300
        $receipts=@(1..3|ForEach-Object {[pscustomobject]@{expansion=$final}})
        $beforeFrames=[pscustomobject]@{frame=10;dropped=0}
        $expansionFrames=[pscustomobject]@{dropped=0;records=@((Frame 11 125 225),(Frame 12 150 250),(Frame 13 200 300))}
        $expectedFailure=$null
        switch($case){
            'only-final' {$expansionFrames.records=@($expansionFrames.records[2]);$expectedFailure='Narrow expansion did not capture two distinct intermediate geometry samples'}
            'one-intermediate' {$expansionFrames.records=@($expansionFrames.records[0],$expansionFrames.records[2]);$expectedFailure='Narrow expansion did not capture two distinct intermediate geometry samples'}
            'repeated-geometry' {$expansionFrames.records[1].geometry=Geometry 125 225;$expectedFailure='Narrow expansion did not capture two distinct intermediate geometry samples'}
            'wrong-height' {$expansionFrames.records[0].geometry=Geometry 125 270;$expectedFailure='Narrow expansion height departed from its width-derived shared trajectory'}
            'wrong-configuration' {$expansionFrames.records[0].geometry=Geometry 125 225 80;$expectedFailure='Narrow expansion changed its resolved configuration width while opening'}
            'stale' {$expansionFrames.records[0].frame=8;$expansionFrames.records[1].frame=9;$expectedFailure='Narrow expansion did not capture two distinct intermediate geometry samples'}
            'foreign-group' {$expansionFrames.records[0].geometry.group=9;$expansionFrames.records[1].geometry.group=9;$expectedFailure='Narrow expansion did not capture two distinct intermediate geometry samples'}
            'foreign-panel' {$expansionFrames.records[0].panel='navigator';$expansionFrames.records[1].panel='navigator';$expectedFailure='Narrow expansion did not capture two distinct intermediate geometry samples'}
            'unfinished' {$receipts[0].expansion=Geometry 190 290;$expectedFailure='Narrow expansion did not finish within the composed sample window'}
            'overflow' {$expansionFrames.dropped=1;$expectedFailure='Narrow expansion native geometry history overflowed'}
            'previous-overflow' {$beforeFrames.dropped=99}
        }
        $changes=@([pscustomobject]@{Timestamp=13;Status=($final|ConvertTo-Json -Depth 8 -Compress)})
        if($case -eq 'all-events'){$changes=@($expansionFrames.records|ForEach-Object {[pscustomobject]@{Timestamp=$_.frame;Status=($_.geometry|ConvertTo-Json -Depth 8 -Compress)}})}
        $failure=$null
        try{. $oracle}catch{$failure=$_.Exception.Message}
        if($failure -cne $expectedFailure){throw "Actual narrow-expansion oracle $case failed: $failure (expected $expectedFailure)"}
        if(!$expectedFailure -and $intermediate.Count -ne 2){throw 'Actual native trajectory omitted one distinct intermediate geometry'}
        $checks++
    }
    Write-Output "Expansion native-frame trajectory: $checks actual-AST cases passed"
}
