param([Parameter(Mandatory)][string]$Executable,[ValidateSet('mouse','pen','touch')][string]$Device='mouse')
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/edge-reach/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
function Gesture {try{(Workspace-Root).Current.HelpText|ConvertFrom-Json}catch{}}
function Layout {(Model).layout}
function Column([int]$Id){(Layout).collapsed|Where-Object id -eq $Id|Select-Object -First 1}
function Divider([int]$Id){(Layout).dividers|Where-Object id -eq $Id|Select-Object -First 1}
function Settled {
    $settled=@{value='';count=0}
    Wait-Until {
        $next=(Model).state.workspace.layout|ConvertTo-Json -Depth 90 -Compress
        if($next -eq $settled.value){$settled.count++}else{$settled.value=$next;$settled.count=0}
        $settled.count -ge 3
    } 'Workspace publication did not settle'
}
function Screen([double]$X,[double]$Y){
    $point=[CapyRowPointer+Point]::new()
    if(![CapyRowPointer]::ClientToScreen($window,[ref]$point)){throw 'Native client origin is unavailable'}
    $scale=[CapyRowPointer]::GetDpiForWindow($window)/96.
    @{x=[int]($point.x+$X*$scale);y=[int]($point.y+$Y*$scale)}
}
function Swipe($From,$To,[int]$Steps=10,[scriptblock]$During){
    $a=Screen $From.x $From.y;$b=Screen $To.x $To.y
    if($Device -eq 'mouse'){[CapyRowPointer]::Hover($a.x,$a.y)}
    [CapyRowPointer]::Down($Device,$a.x,$a.y)
    for($i=1;$i -le $Steps;$i++){[CapyRowPointer]::Move([int]($a.x+($b.x-$a.x)*$i/$Steps),[int]($a.y+($b.y-$a.y)*$i/$Steps));Start-Sleep -Milliseconds 16}
    Start-Sleep -Milliseconds 250
    if($During){& $During}
    [CapyRowPointer]::Up();Settled
}
function Contact($Point){
    $at=Screen $Point.x $Point.y
    if($Device -eq 'mouse'){[CapyRowPointer]::Hover($at.x,$at.y)}
    [CapyRowPointer]::Down($Device,$at.x,$at.y)
    Start-Sleep -Milliseconds 120
    $edge=(Gesture).edge
    [CapyRowPointer]::Cancel();Start-Sleep -Milliseconds 120
    $edge
}
function Reach-Point([object[]]$Candidates,[scriptblock]$Accept){
    foreach($point in $Candidates){$edge=Contact $point;Settled;if($edge -and (& $Accept $edge)){return $point}}
    throw 'No candidate contact lies within the edge reach'
}
function Width([int]$Group){((Layout).groups|Where-Object id -eq $Group|Select-Object -First 1).bounds.width}
function WindowCommand([string]$Id){& (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'Window';Invoke $Id;Wait-Until {$null -eq (Find $Id)} 'Workspace menu did not close'}
function Start-Review([string]$Theme){
    $script:stderr=Join-Path $run ($Theme+'-stderr.log')
    $env:CAPY_STORAGE_DIR=Join-Path $run ($Theme+'-profile')
    [IO.Directory]::CreateDirectory($env:CAPY_STORAGE_DIR)|Out-Null
    [IO.File]::WriteAllText((Settings-File),(@{language=@{Explicit='en'};theme=$Theme}|ConvertTo-Json -Depth 4))
    $script:review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
    $null=$review.Handle
    @{process_id=$review.Id;run=$run;device=$Device}|ConvertTo-Json|Set-Content (Join-Path $repo 'artifacts/windows/edge-reach-review.json')
    Write-Output "Owned edge reach review $($review.Id) ($Device, $Theme)"
    $native=@{window=$null}
    Wait-Until {$native.window=Owned-DrawingWindow $review;$null -ne $native.window -and (Model).brush_ready -and (Model).windows_workspace.ready -and !(Model).windows_workspace.busy -and (Model).state.theme -eq $Theme} 'Edge reach review did not start' 45
    $script:root=$native.window.Root;$script:window=$native.window.Handle;$script:drawingWindow=$window
    $null=[CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4));$null=[CapyRowPointer]::SetForegroundWindow($window)
    (Control 'drawing-canvas').SetFocus()
    [CapyRowPointer]::Initialize([uint32]$review.Id)
    Maximize-DrawingWindow
}
function Check-ColumnEdge([string]$Theme){
    $layout=Layout
    $edge=$layout.dividers|Where-Object {$_.axis -eq 'horizontal' -and $_.owner_side -eq 'left' -and !$_.fixed}|Sort-Object {$_.bounds.x}|Select-Object -Last 1
    if(!$edge){throw 'The left column has no resize edge'}
    $line=$edge.bounds.x+$edge.bounds.width*.5
    $group=$layout.groups|Where-Object {!$_.floating -and [Math]::Abs($_.bounds.x+$_.bounds.width-$edge.bounds.x) -lt 1}|Sort-Object {$_.bounds.height}|Select-Object -Last 1
    if(!$group){throw 'No docked group meets the left column edge'}
    $column=@($layout.groups|Where-Object {!$_.floating -and [Math]::Abs($_.bounds.x+$_.bounds.width-$edge.bounds.x) -lt 1})
    $inside={param($id)$x=(Divider $id).bounds.x-20;Reach-Point @(foreach($g in $column){for($y=$g.bounds.y+$g.bounds.height-24;$y -gt $g.bounds.y+48;$y-=24){@{x=$x;y=$y}}}) {param($edge)$edge.contact -eq 'free'}}
    $from=& $inside $edge.id
    $before=Width $group.id
    $swipe={
        if($Device -eq 'mouse'){
            try{Wait-Until {(Gesture).edge.claimed} "claim"}catch{throw "${Theme}: the mouse swipe did not claim the column edge from $($from|ConvertTo-Json -Compress): $((Gesture)|ConvertTo-Json -Compress -Depth 5)"}
            if([CapyRowPointer]::Cursor().cursor -ne [CapyRowPointer]::StockCursor(32644)){throw "${Theme}: a claimed column edge did not show the resize pointer"}
        }
    }
    Swipe $from @{x=$line+60;y=$from.y} -During $swipe
    $grown=(Width $group.id)-$before
    if([Math]::Abs($grown-60) -gt 3){throw "${Theme}: an outward swipe from inside the column did not grow it to the contact ($grown)"}
    if((Gesture).last_edge.claimed -ne $true){throw "${Theme}: the column swipe ended without an edge claim"}
    $grownWidth=Width $group.id
    $reset=& $inside $edge.id;$at=Screen $reset.x $reset.y
    [void][CapyRowPointer]::DoubleClick($Device,$at.x,$at.y)
    Wait-Until {[Math]::Abs((Width $group.id)-$grownWidth) -gt 30} "${Theme}: a double press in the column edge reach did not reset its width"
    Settled;$before=Width $group.id;$edge=Divider $edge.id;$from=& $inside $edge.id
    $canvas=@{x=$edge.bounds.x+$edge.bounds.width+20;y=$from.y}
    Swipe $canvas @{x=$canvas.x+50;y=$canvas.y}
    if($Device -eq 'touch'){
        if((Width $group.id) -lt $before+40){throw "${Theme}: touch did not grab the edge from the canvas"}
        WindowCommand 'undo_workspace';Wait-Until {[Math]::Abs((Width $group.id)-$before) -lt 1} "${Theme}: the canvas edge grab was not one Undo step"
    }elseif([Math]::Abs((Width $group.id)-$before) -gt .5){throw "${Theme}: the canvas did not keep the $Device contact"}
    Swipe $from @{x=$from.x;y=$from.y-60}
    if([Math]::Abs((Width $group.id)-$before) -gt .5){throw "${Theme}: a vertical swipe grabbed a column edge"}
}
function Check-GroupGap([string]$Theme){
    $layout=Layout
    $beside={param($d,$above)$layout.groups|Where-Object {!$_.floating -and [Math]::Abs($_.bounds.x-$d.bounds.x) -lt 1 -and [Math]::Abs($(if($above){$_.bounds.y+$_.bounds.height-$d.bounds.y}else{$_.bounds.y-$d.bounds.y-$d.bounds.height})) -lt 1}|Select-Object -First 1}
    $fitted=@((Model).state.workspace.layout.fit_height_groups)
    $flexible={param($g)$g -and $fitted -notcontains $g.id}
    $gap=$layout.dividers|Where-Object {$_.axis -eq 'vertical' -and !$_.band -and !$_.fixed -and (& $flexible (& $beside $_ $true)) -and (& $flexible (& $beside $_ $false))}|Sort-Object {$_.bounds.x}|Select-Object -First 1
    if(!$gap){throw 'Paint has no resizable gap between groups'}
    $candidates=foreach($dx in 30,60,90){@{x=$gap.bounds.x+$dx;y=$gap.bounds.y+$gap.bounds.height+3};@{x=$gap.bounds.x+$dx;y=$gap.bounds.y-10}}
    $from=Reach-Point $candidates {param($edge)$edge.vertical -and $edge.contact -ne 'input'}
    $tab=(Model).layout.groups|Where-Object {$_.bounds.y -gt $gap.bounds.y}|Sort-Object {$_.bounds.y}|Select-Object -First 1
    Swipe $from @{x=$from.x;y=$from.y-50}
    $moved=$gap.bounds.y-(Divider $gap.id).bounds.y
    if($moved -lt 20){throw "${Theme}: a vertical swipe beside a gap did not move it ($moved from $($from|ConvertTo-Json -Compress), $((Gesture)|ConvertTo-Json -Compress -Depth 5))"}
    if($tab -and ((Layout).groups|Where-Object id -eq $tab.id).active -ne $tab.active){throw "${Theme}: a gap swipe from the tab strip selected a tab"}
    if((Gesture).last_edge.claimed -ne $true -or !(Gesture).last_edge.vertical){throw "${Theme}: the gap moved without a vertical edge claim"}
    WindowCommand 'undo_workspace';Wait-Until {[Math]::Abs((Divider $gap.id).bounds.y-$gap.bounds.y) -lt 1} "${Theme}: the gap resize was not one Undo step"
}
function Check-ClosedStrip([string]$Theme){
    $strip=(Layout).collapsed|Where-Object open|Select-Object -First 1
    if(!$strip){throw 'Paint has no open stack member'}
    $panel=@($strip.groups.icons.panel|Where-Object {(Control ('column-icon-'+$_)).Current.ItemStatus -eq 'Selected'})[0]
    if(!$panel){throw 'The open stack member has no selected icon'}
    Invoke-Id ('column-icon-'+$panel);Wait-Until {!(Column $strip.id).open} "${Theme}: the open member did not close"
    $icon=((Column $strip.id).groups.icons|Where-Object panel -eq $panel|Select-Object -First 1).bounds
    $from=@{x=$icon.x+$icon.width*.5;y=$icon.y+$icon.height*.5}
    $outward=if($strip.bounds.x -gt (Layout).viewport[0]*.5){-90}else{90}
    Swipe $from @{x=$from.x+$outward;y=$from.y}
    try{Wait-Until {$column=Column $strip.id;!$column -or $column.open} "open"}catch{throw "${Theme}: an outward swipe from a closed strip did not open it: $((Gesture)|ConvertTo-Json -Compress -Depth 5)"}
    if((Gesture).last_edge.claimed -ne $true){throw "${Theme}: the closed strip opened without an edge claim"}
}
try {
    Enter-CapyEnvironment
    $env:CAPY_TRACE_UI='1'
    foreach($theme in 'dark','light'){
        Start-Review $theme
        Settled
        Check-ColumnEdge $theme
        Check-GroupGap $theme
        Check-ClosedStrip $theme
        Capture ("edge-reach-"+$theme)
        & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close
        if((Get-Item -LiteralPath $stderr).Length){throw "Native stderr requires inspection ($theme)"}
        [CapyRowPointer]::Dispose()
    }
    [pscustomobject]@{device=$Device;column_edge_from_inside='passed';resize_pointer=($Device -eq 'mouse');double_press_reset='passed';canvas_contact=$(if($Device -eq 'touch'){'grabs'}else{'kept'});vertical_swipe_kept='passed';group_gap='passed';closed_strip='passed';themes='dark,light';scope='OS-delivered synthetic input; physical devices and timing remain separate'}|ConvertTo-Json
}catch{
    if($review -and !$review.HasExited){try{Capture 'failure'}catch{}}
    [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{
    [CapyRowPointer]::Dispose()
    Exit-CapyEnvironment
}
