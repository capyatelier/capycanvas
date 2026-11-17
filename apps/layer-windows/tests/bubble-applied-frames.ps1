$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot '../scripts/CanonicalJson.ps1')
$source=Join-Path $PSScriptRoot '../scripts/BubbleVisualChecks.ps1'
$tokens=$null;$errors=$null
$ast=[Management.Automation.Language.Parser]::ParseFile($source,[ref]$tokens,[ref]$errors)
if($errors.Count){throw 'Bubble visual fixture parse failed'}
& {
    foreach($name in @('Bubble-AppliedFrames','Wait-BubbleNative','Wait-BubbleOverlayPhase','Wait-BubbleVisualLayout','Assert-BubbleInspectorFrames')){
        $definition=@($ast.FindAll({param($node)$node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name}.GetNewClosure(),$true))
        if($definition.Count -ne 1){throw "Missing actual $name fixture function"}
        . ([scriptblock]::Create($definition[0].Extent.Text))
    }
    $sourceIdentity=@{kind='command';command='drawing_brush'}
    $placement=@{id=23;corner='top-left';offset=@(4,5)}
    function Packets{@{view=@{id=23;source=$sourceIdentity}}}
    function Center([long]$Id,$Packet){@{x=10;y=20}}
    function Model{@{state=@{document_file=@{epoch=7};workspace=@{layout=@{bubbles=@{bubbles=@($placement)}}}}}}
    function Workspace-Root{
        $ready=$script:step -ge 8;$frame=$script:step
        if($script:case -eq 'same-frame' -and !$ready){$frame=0}
        $row=@{bubble=23;part='face';epoch=7;owner=101;source=($sourceIdentity|ConvertTo-Json -Compress);placement=$placement
            connected=$true;visible=$true;terminal=$true;last_frame=$frame;bounds_frame=$frame;overlay_phase='pressed';focus_ring=$true}
        $native=@{frame=$frame;qpc=[Diagnostics.Stopwatch]::GetTimestamp();serial=1;overflow=0;settled=$true;query_wake=3;reply_wake=3;motion_moving=$false;current=@($row)}
        switch($script:case){
            'unsettled'{$native.settled=$ready;$row.terminal=$ready}
            'held-stationary'{$native.settled=$ready;$native.contact_active=$true;$native.polling=$true}
            'stale-reply'{if(!$ready){$native.reply_wake=2}}
            'motion-pending'{$native.motion_moving=!$ready}
            'geometry-progress'{$native.settled=$ready;$row.bounds=@{x=[Math]::Min($script:step,8);y=20;width=40;height=40}}
            'stale-epoch'{if(!$ready){$row.epoch=6}}
            'stale-source'{if(!$ready){$row.source='{"kind":"command","command":"drawing_eraser"}'}}
            'nested-source'{$row.source='{"kind":"control","control":{"kind":"command","command":"drawing_brush"}}'}
            'nested-command'{$row.source='{"kind":"control","control":{"kind":"command","command":"drawing_eraser"}}'}
            'missing-owner'{if(!$ready){$row.owner=0}}
            'stale-target'{if(!$ready){$row.placement=@{id=23;corner='top-left';offset=@(0,0)}}}
            'stale-frame'{if(!$ready){$row.last_frame=$frame-1}}
            'stale-qpc'{if(!$ready){$native.qpc=1}}
            'overlay'{$native.settled=$false;if(!$ready){$row.overlay_phase='hover'}}
            'focus'{$native.settled=$false;$row.focus_ring=$ready}
            'overflow'{$native.overflow=1}
            'missing'{$native=@{}}
        }
        [pscustomobject]@{Current=@{ItemStatus=(@{bubble_frames=@{applied=$native}}|ConvertTo-Json -Depth 20 -Compress)}}
    }
    function Wait-Until([scriptblock]$Condition,[string]$Message,[scriptblock]$ResetBound){
        for($script:step=1;$script:step -le 12;$script:step++){if(&$Condition){return};if($ResetBound -and (&$ResetBound)){$script:resets++}}
        throw $Message
    }
    $cases=@('unsettled','held-stationary','stale-reply','motion-pending','geometry-progress','stale-epoch','stale-source','missing-owner','stale-target','stale-frame','stale-qpc','same-frame')
    if($env:CAPY_OLD_BUBBLE_HELPER){
        $oldTokens=$null;$oldErrors=$null
        $oldAst=[Management.Automation.Language.Parser]::ParseFile($env:CAPY_OLD_BUBBLE_HELPER,[ref]$oldTokens,[ref]$oldErrors)
        if($oldErrors.Count){throw 'OLD bubble helper parse failed'}
        $oldDefinition=@($oldAst.FindAll({param($node)$node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Wait-BubbleVisualLayout'},$true))
        if($oldDefinition.Count -ne 1){throw 'OLD bubble helper lacks Wait-BubbleVisualLayout'}
        . ([scriptblock]::Create($oldDefinition[0].Extent.Text))
        $cases=@('unsettled')
    }
    foreach($script:case in $cases){
        $script:step=0;$script:resets=0;Wait-BubbleVisualLayout
        if($script:step -lt 8){throw "Native bubble readiness admitted $script:case before its applied terminal receipt"}
        if($script:case -eq 'geometry-progress' -and $script:resets -lt 1){throw 'Native geometry advancement did not extend its inactivity bound'}
        if($script:case -eq 'stale-frame' -and $script:resets -ne 0){throw 'Native frame heartbeat extended its inactivity bound'}
    }
    if($env:CAPY_OLD_BUBBLE_HELPER){return}
    foreach($script:case in @('overlay','focus')){
        $script:step=0
        if($script:case -eq 'overlay'){Wait-BubbleOverlayPhase 23 'pressed'}else{$null=Wait-BubbleNative -Id 23 -Part 'face' -Focus $true}
        if($script:step -lt 8){throw "Native bubble readiness admitted stale $script:case"}
    }
    foreach($script:case in @('overflow','missing')){
        $script:step=0;$failure=$null
        try{Wait-BubbleVisualLayout}catch{$failure=$_}
        if(!$failure -or $failure.Exception.Message -cne 'Native bubble applied-frame evidence is missing or overflowed'){throw "Native bubble readiness accepted $script:case evidence"}
    }
    & {
        if($env:CAPY_OLD_BUBBLE_SOURCE){
            $oldTokens=$null;$oldErrors=$null
            $oldAst=[Management.Automation.Language.Parser]::ParseFile($env:CAPY_OLD_BUBBLE_SOURCE,[ref]$oldTokens,[ref]$oldErrors)
            if($oldErrors.Count){throw 'OLD bubble source helper parse failed'}
            $oldDefinition=@($oldAst.FindAll({param($node)$node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -ceq 'Wait-BubbleNative'},$true))
            if($oldDefinition.Count -ne 1){throw 'OLD bubble source helper lacks Wait-BubbleNative'}
            . ([scriptblock]::Create($oldDefinition[0].Extent.Text))
        }
        $sourceIdentity='{"control":{"command":"drawing_brush","kind":"command"},"kind":"control"}'|ConvertFrom-Json
        $script:case='nested-source';$script:step=0
        $null=Wait-BubbleNative -Id 23 -Part 'face' -Message 'Released bubble did not settle'
        $script:case='nested-command';$script:step=0;$failure=$null
        try{$null=Wait-BubbleNative -Id 23 -Part 'face' -Message 'Released bubble did not settle'}catch{$failure=$_}
        if(!$failure -or $failure.Exception.Message -cne 'Released bubble did not settle'){throw 'Native bubble source accepted a different nested command'}
    }
    & {
        $fixture=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/exercise-tool-bubbles.ps1'),[ref]$tokens,[ref]$errors)
        if($errors.Count){throw 'Tool bubble put-back fixture parse failed'}
        $capture=$fixture.Find({param($node)$node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -ceq 'Settled-Capture'},$true)
        if(!$capture){throw 'Missing actual settled bubble capture'}
        . ([scriptblock]::Create($capture.Extent.Text))
        $emptyCheck=$fixture.Find({param($node)$node -is [Management.Automation.Language.IfStatementAst] -and $node.Extent.Text -ceq "if(@(Packets).Count){throw 'Put back left bubbles behind'}"},$true)
        $putBack=$fixture.Find({param($node)$node -is [Management.Automation.Language.CommandAst] -and $node.GetCommandName() -ceq 'Settled-Capture' -and $node.CommandElements[1].Value -ceq 'put-back'},$true)
        if(!$emptyCheck -or !$putBack){throw 'Missing actual empty put-back capture boundary'}
        $sequence=[scriptblock]::Create($emptyCheck.Extent.Text+"`n"+$putBack.Extent.Text)
        function Packets{if($script:case -eq 'model-only'){@{view=@{id=23;source=$sourceIdentity}}}}
        function Model{@{state=@{document_file=@{epoch=7};workspace=@{layout=@{bubbles=@{bubbles=@()}}}}}}
        function Capture([string]$Name,[switch]$Composed,[switch]$WithModel){
            if($Name -cne 'put-back' -or !$Composed -or !$WithModel){throw 'Empty scene changed its actual capture'}
            $seen.captures++
        }
        function Workspace-Root{
            $native=@{frame=$script:step;qpc=[Diagnostics.Stopwatch]::GetTimestamp();serial=1;overflow=0;settled=$true;query_wake=3;reply_wake=3;motion_moving=$false;current=@()}
            $retired=@{bubble=23;part='card';epoch=6;owner=101;connected=$false;visible=$false;backing_visible=$false;inspector_visible=$false;inspector=$true;terminal=$false;last_frame=0;bounds_frame=0}
            switch($script:case){
                'history'{$native.current=@($retired)}
                'retired-stale-motion'{$native.query_wake=632;$native.reply_wake=628;$native.motion_moving=$true;$native.current=@(foreach($id in 1..58){@{bubble=$id;part='card';epoch=6;owner=100+$id;connected=$false;visible=$false;backing_visible=$false;inspector_visible=$false;inspector=$true;terminal=$false;last_frame=0;bounds_frame=0}})}
                'active'{$retired.epoch=7;$retired.connected=$true;$retired.visible=$true;$retired.terminal=$true;$native.current=@($retired)}
                'retiring'{$retired.epoch=7;$retired.connected=$true;$native.current=@($retired)}
                'foreign-epoch-connected'{$retired.connected=$true;$native.current=@($retired)}
                'visible'{$retired.visible=$true;$native.current=@($retired)}
                'backing'{$retired.backing_visible=$true;$native.current=@($retired)}
                'inspector'{$retired.inspector_visible=$true;$native.current=@($retired)}
                'missing-flag'{$retired.Remove('connected');$native.current=@($retired)}
                'malformed-flag'{$retired.connected='false';$native.current=@($retired)}
                'stale-frame'{$native.frame=0}
                'stale-qpc'{$native.qpc=1}
                'missing-current'{$native.Remove('current')}
                'null-current'{$native.current=$null}
                'object-current'{$native.current=$retired}
                'null-part'{$native.current=@($null)}
                'unsettled'{$native.settled=$false}
                'missing-settled'{$native.Remove('settled')}
                'malformed-settled'{$native.settled='true'}
                'wake-mismatch'{$native.reply_wake=2}
                'missing-query'{$native.Remove('query_wake')}
                'missing-reply'{$native.Remove('reply_wake')}
                'missing-motion'{$native.Remove('motion_moving')}
                'motion'{$native.motion_moving=$true}
            }
            [pscustomobject]@{Current=@{ItemStatus=(@{bubble_frames=@{applied=$native}}|ConvertTo-Json -Depth 20 -Compress)}}
        }
        if($env:CAPY_OLD_BUBBLE_EMPTY){
            $oldTokens=$null;$oldErrors=$null
            $oldAst=[Management.Automation.Language.Parser]::ParseFile($env:CAPY_OLD_BUBBLE_EMPTY,[ref]$oldTokens,[ref]$oldErrors)
            if($oldErrors.Count){throw 'OLD empty bubble helper parse failed'}
            $oldDefinition=@($oldAst.FindAll({param($node)$node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -ceq 'Wait-BubbleNative'},$true))
            if($oldDefinition.Count -ne 1){throw 'OLD empty bubble helper lacks Wait-BubbleNative'}
            . ([scriptblock]::Create($oldDefinition[0].Extent.Text))
            $script:case='retired-stale-motion';$script:step=0;$seen=@{captures=0}
            . $sequence
            throw 'OLD bubble consumer unexpectedly accepted the empty put-back scene'
        }
        foreach($script:case in @('empty','history','retired-stale-motion','active','retiring','foreign-epoch-connected','visible','backing','inspector','missing-flag','malformed-flag','stale-frame','stale-qpc','missing-current','null-current','object-current','null-part','unsettled','missing-settled','malformed-settled','wake-mismatch','missing-query','missing-reply','missing-motion','motion')){
            $script:step=0;$script:resets=0;$seen=@{captures=0};$failure=$null
            $expectedFailure=if($script:case -eq 'missing-settled'){'Native bubble applied-frame evidence is missing or overflowed'}else{'Bubble edge scene did not settle'}
            try{. $sequence}catch{$failure=$_}
            if($script:case -in @('empty','history','wake-mismatch','motion','retired-stale-motion')){
                if($failure){throw $failure}
                if($seen.captures -ne 1){throw 'Acknowledged empty bubble scene skipped its one original capture'}
            }elseif(!$failure -or $failure.Exception.Message -cne $expectedFailure -or $seen.captures){throw "Empty scene admitted $script:case before native retirement acknowledgement"}
            if($script:resets){throw 'Empty scene extended its inactivity bound without positive native progress'}
        }
        $script:case='model-only';$script:step=0;$failure=$null
        try{$null=Wait-BubbleNative -Message 'Model bubble did not settle'}catch{$failure=$_}
        if(!$failure -or $failure.Exception.Message -cne 'Model bubble did not settle'){throw 'Empty native scene acknowledged a model bubble without its native parts'}
        foreach($qualified in @(@{Id=23},@{Part='face'},@{Overlay='pressed'},@{Focus=$true},@{Focus=$false})){
            $script:case='empty';$script:step=0;$failure=$null
            try{$null=Wait-BubbleNative @qualified -Message 'Qualified bubble did not settle'}catch{$failure=$_}
            if(!$failure -or $failure.Exception.Message -cne 'Qualified bubble did not settle'){throw 'Empty scene acknowledged a missing qualified bubble target'}
        }
    }
    $before=@{frame=10;inspector_frames=3;inspector_invalid=0}
    Assert-BubbleInspectorFrames $before @{frame=11;inspector_frames=4;inspector_invalid=0}
    foreach($after in @(@{frame=11;inspector_frames=4;inspector_invalid=1},@{frame=11;inspector_frames=3;inspector_invalid=0},@{frame=10;inspector_frames=4;inspector_invalid=0},@{frame=11})){
        $failure=$null;try{Assert-BubbleInspectorFrames $before $after}catch{$failure=$_}
        if(!$failure){throw 'Inspector retirement accepted missing coverage or a detached peer with native backing'}
    }
}
if($env:CAPY_OLD_BUBBLE_HELPER){Write-Output 'OLD bubble helper semantic unsettled schedule passed.'}
else{Write-Output 'Bubble applied-frame readiness: terminal, epoch, source, owner, target, frame, overlay, focus and inspector lifetime cases passed.'}

& {
    foreach($pair in @(
        @('{"z":{"b":2,"a":1},"a":[null,[null,[],[1,null]],null]}','{"a":[null,[null,[],[1,null]],null],"z":{"a":1,"b":2}}'),
        @('{}','{}'),@('[]','[]')
    )){
        $first=ConvertTo-Json -InputObject (Canonical ($pair[0]|ConvertFrom-Json)) -Depth 100 -Compress
        $second=ConvertTo-Json -InputObject (Canonical ($pair[1]|ConvertFrom-Json)) -Depth 100 -Compress
        if($first -cne $second){throw 'Canonical JSON changed equivalent nested object keys'}
    }
    foreach($empty in @([pscustomobject]@{},@{})){
        $actual=ConvertTo-Json -InputObject (Canonical $empty) -Depth 100 -Compress
        if($actual -cne '{}'){throw 'Canonical JSON changed an empty object or dictionary'}
        $roundtrip=$actual|ConvertFrom-Json
        if(@($roundtrip.PSObject.Properties).Count){throw 'Canonical empty object gained a key on roundtrip'}
    }
    $actual=ConvertTo-Json -InputObject (Canonical ([object[]]@())) -Depth 100 -Compress
    if($actual -cne '[]'){throw 'Canonical JSON changed an empty array'}
    $values=[object[]]@($null,[object[]]@($null,[object[]]@(),[object[]]@(1,$null)),$null)
    $actual=ConvertTo-Json -InputObject (Canonical $values) -Depth 100 -Compress
    if($actual -cne '[null,[null,[],[1,null]],null]'){throw 'Canonical JSON dropped null array items, nesting or empty arrays'}
    foreach($pair in @(
        @('[1,2]','[2,1]'),@('{"v":[null,1]}','{"v":[1,null]}'),@('{"a":1}','{"A":1}'),
        @('{"a":1}','{"a":1.000001}'),@('{"a":"x"}','{"a":"X"}'),
        @('{"a":null}','{}'),@('{"a":false}','{"a":null}'),@('{"a":1}','{"b":1}')
    )){
        $first=ConvertTo-Json -InputObject (Canonical ($pair[0]|ConvertFrom-Json)) -Depth 100 -Compress
        $second=ConvertTo-Json -InputObject (Canonical ($pair[1]|ConvertFrom-Json)) -Depth 100 -Compress
        if($first -ceq $second){throw 'Canonical JSON accepted changed array order, key, value, case or number'}
    }
    $record=[Collections.Specialized.OrderedDictionary]::new([StringComparer]::Ordinal)
    $record.Add('a',2);$record.Add('A',1)
    $actual=ConvertTo-Json -InputObject (Canonical $record) -Depth 100 -Compress
    if($actual -cne '{"A":1,"a":2}'){throw 'Canonical JSON folded case-distinct keys or used nonordinal key order'}
}
Write-Output 'Canonical bubble source: real nested native/model key order accepted; changed command, fields, case, numbers and array order rejected; null array items and nesting retained.'

Write-Output 'Empty put-back scene: authoritative settled acknowledgement accepts fully detached history despite stale motion replies; live parts, backing, stale/missing receipts and qualified targets reject before capture.'
