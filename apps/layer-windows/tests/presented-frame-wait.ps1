$ErrorActionPreference='Stop'
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$tokens=$null;$errors=$null
$ast=[Management.Automation.Language.Parser]::ParseFile((Join-Path $repo 'apps/layer-windows/scripts/CapyUia.ps1'),[ref]$tokens,[ref]$errors)
if($errors.Count){throw 'CapyUia does not parse'}
foreach($name in 'Read-Snapshot','Wait-PresentedFrame','Wait-StablePixels'){
    $helper=@($ast.FindAll({param($node)$node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name},$true))
    if($helper.Count -ne 1){throw "Missing $name"}
    . ([scriptblock]::Create($helper[0].Extent.Text))
}
$out=Join-Path $repo ('artifacts/windows/presented-frame-'+[Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory $out|Out-Null
try{
    $review=[pscustomobject]@{Id=$PID}
    $state=Join-Path $out "ui-state-$PID-7.json"
    function State-File{$state}
    function Write-Trace([string]$Kind,$Value){$Value|ConvertTo-Json -Compress|Set-Content -LiteralPath (Join-Path $out "$Kind-$PID-7.json")}
    function Wait-Until([scriptblock]$Condition,[string]$Message){
        for($poll=0;$poll -lt 4;$poll++){if(& $Condition){return};$script:polls++;if($script:advance){& $script:advance}}
        throw $Message
    }
    function Expect([string]$Name,[bool]$Refused){
        $script:polls=0;$failed=$false
        try{Wait-PresentedFrame}catch{if($_.Exception.Message -ne 'The display did not show the latest canvas frame'){throw};$failed=$true}
        if($failed -ne $Refused){throw "$Name was $(if($failed){'refused'}else{'accepted'})"}
        $script:checks++
    }
    $checks=0
    Write-Trace 'ui-state' @{process_id=$PID;window_id=7;presented=12;model=@{}}
    Write-Trace 'camera-state' @{process_id=$PID;window_id=7;presented=14;camera=@{}}
    $script:advance=$null
    Write-Trace 'presentation-state' @{process_id=$PID;window_id=7;displayed=13}
    Expect 'a display behind the newer camera frame' $true
    Write-Trace 'presentation-state' @{process_id=$PID;window_id=7;displayed=14}
    Expect 'the displayed camera frame' $false
    Write-Trace 'presentation-state' @{process_id=$PID;window_id=7;displayed=11}
    $script:shown=11;$script:advance={$script:shown++;Write-Trace 'presentation-state' @{process_id=$PID;window_id=7;displayed=$script:shown}}
    Expect 'a display that catches up' $false
    if($script:polls -ne 3){throw 'The wait accepted a frame before it was displayed'}
    $script:advance=$null
    Write-Trace 'presentation-state' @{process_id=($PID+1);window_id=7;displayed=99}
    Expect 'another process display' $true
    Remove-Item -LiteralPath (Join-Path $out "presentation-state-$PID-7.json")
    Expect 'a missing display acknowledgement' $true
    Write-Trace 'presentation-state' @{process_id=$PID;window_id=7;displayed=$null}
    Expect 'an adapter without frame statistics' $false
    Remove-Item -LiteralPath (Join-Path $out "ui-state-$PID-7.json"),(Join-Path $out "camera-state-$PID-7.json")
    Expect 'an untraced review' $false
    Write-Trace 'ui-state' @{process_id=$PID;window_id=7;presented=20;model=@{}}
    Write-Trace 'presentation-state' @{process_id=$PID;window_id=7;displayed=19}
    $sampled=$false
    try{Wait-StablePixels {$script:sampled=$true;'pixels'}|Out-Null}catch{if($_.Exception.Message -ne 'The display did not show the latest canvas frame'){throw}}
    if($sampled){throw 'Pixels were sampled before the latest frame was displayed'}
    $checks++
    "Presented frame wait: $checks passed"
}finally{Remove-Item -LiteralPath $out -Recurse -Force}
