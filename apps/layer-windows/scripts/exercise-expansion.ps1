param([Parameter(Mandatory)][string]$Executable,[switch]$ResizeOnly,[switch]$ResizeTailOnly,[switch]$ResizeStackOnly,[ValidateSet('mouse','pen','touch')][string]$Device='mouse',[ValidateSet('dark','light')][string]$Theme='dark')
$ErrorActionPreference='Stop'
$ResizeOnly=$ResizeOnly -or $ResizeTailOnly -or $ResizeStackOnly
$script:ResizeVerticalScrollStatus='not-run'
$script:ResizeMixedInputStatus='not-run'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyCacheModel=$true
Add-Type -AssemblyName System.Drawing
if(!('CapyRowPointer' -as [type])){Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')}
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/expansion/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
function Resize-Layout {(Model).layout}
function Resize-Gesture {try{(Workspace-Root).Current.HelpText|ConvertFrom-Json}catch{}}
function Resize-State {(Model).state.workspace.layout|ConvertTo-Json -Depth 90 -Compress}
function Resize-Settled {
    $last=@{value='';count=0}
    Wait-Until {$next=Resize-State;if($next -eq $last.value){$last.count++}else{$last.value=$next;$last.count=0};$last.count -ge 3} 'Resize layout did not settle'
}
function Resize-Screen($Point){
    $origin=[CapyRowPointer+Point]::new()
    if(![CapyRowPointer]::ClientToScreen($drawingWindow,[ref]$origin)){throw 'Resize client origin is unavailable'}
    $scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
    @{x=[int]($origin.x+$Point.x*$scale);y=[int]($origin.y+$Point.y*$scale)}
}
function Resize-Point([double]$X,[double]$Y){@{x=$X;y=$Y}}
function Resize-Reach([object[]]$Candidates,[scriptblock]$Accept,[switch]$DoubleEscape){
    $probes=[Collections.Generic.List[object]]::new()
    foreach($point in $Candidates){
        $at=Resize-Screen $point
        $trace=Input-Trace $at.x $at.y
        if(@($trace.hit|Where-Object type -eq 'ControlType.ScrollBar').Count){$probes.Add(@{point=$point;input=$trace;excluded='native-scrollbar'});continue}
        if($Device -eq 'mouse'){[CapyRowPointer]::Hover($at.x,$at.y)}
        [CapyRowPointer]::Down($Device,$at.x,$at.y);Start-Sleep -Milliseconds 80
        $edge=(Resize-Gesture).edge
        if($Device -eq 'mouse'){
            [CapyRowPointer]::Key([uint32]$review.Id,0x1B)
            Wait-Until {!((Resize-Gesture).edge)} 'Mouse probe Escape did not clear its pending edge'
            if($DoubleEscape){[CapyRowPointer]::Key([uint32]$review.Id,0x1B)}
        }
        [CapyRowPointer]::Cancel()
        Wait-Until {$g=Resize-Gesture;$g.phase -eq 'idle' -and !$g.edge -and !$g.captured -and !$g.ignore_click} 'Resize probe left an active gesture or suppressed click'
        Resize-Settled
        $accepted=$edge -and (!$Accept -or (& $Accept $edge))
        $probes.Add(@{point=$point;input=$trace;edge=$edge;accepted=[bool]$accepted})
        $probes|ConvertTo-Json -Depth 12|Set-Content (Join-Path $run 'resize-reach-probes.json')
        if($accepted){return $point}
    }
    throw 'No native contact in the expected resize reach'
}
function Resize-Cursor([int]$Stock){
    if($Device -eq 'mouse'){Wait-Until {[CapyRowPointer]::Cursor().cursor -eq [CapyRowPointer]::StockCursor($Stock)} "Native resize cursor is not stock $Stock"}
}
function Resize-Drag($From,[object[]]$To,[scriptblock]$During,[int]$Cursor=0,[scriptblock]$Contact,[scriptblock]$Claim){
    $a=Resize-Screen $From
    if($Device -eq 'mouse'){[CapyRowPointer]::Hover($a.x,$a.y)}
    [CapyRowPointer]::Down($Device,$a.x,$a.y)
    try{
        if($Contact){& $Contact}
        for($n=0;$n -lt $To.Count;$n++){
            $b=Resize-Screen $To[$n]
            for($i=1;$i -le 8;$i++){[CapyRowPointer]::Move([int]($a.x+($b.x-$a.x)*$i/8),[int]($a.y+($b.y-$a.y)*$i/8));Start-Sleep -Milliseconds 16}
            [CapyRowPointer]::Verify()
            if($Claim){Wait-Until $Claim 'Shared canvas-side resize did not claim the gesture'}
            else{Wait-Until {(Resize-Gesture).edge.claimed} 'Native resize reach did not claim the gesture'}
            Start-Sleep -Milliseconds 200
            if($Cursor){Resize-Cursor $Cursor}
            if($During){& $During $n}
            $a=$b
        }
        [CapyRowPointer]::Up();Resize-Settled
    }finally{if([CapyRowPointer]::Active){[CapyRowPointer]::Cancel()}}
}
function Resize-Undo([string]$Before){WindowCommand 'undo_workspace';Wait-Until {(Resize-State) -ceq $Before} 'Resize was not one exact workspace Undo step';Resize-Settled}
function Resize-FloatingReady([int]$Id){
    Wait-Until {
        $g=(Resize-Layout).groups|Where-Object id -eq $Id|Select-Object -First 1
        if(!$g){return $false}
        foreach($edge in @('left','bottom')){
            $h=$g.resize_handles|Where-Object edge -eq $edge|Select-Object -First 1
            $at=Resize-Screen (Resize-Point $h.bounds.x $h.bounds.y)
            $native=(Control "floating-$Id-$edge" -Arranged).Current.BoundingRectangle
            $scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
            if([Math]::Abs($native.X-$at.x) -gt 2 -or [Math]::Abs($native.Y-$at.y) -gt 2 -or [Math]::Abs($native.Width-$h.bounds.width*$scale) -gt 2 -or [Math]::Abs($native.Height-$h.bounds.height*$scale) -gt 2){return $false}
        }
        (Control 'panel-tab-stats' -Arranged).Current.BoundingRectangle.Height -gt 0
    } 'Floating native frame/tab geometry did not reach its shared bounds'
    Wait-PresentedFrame
}
function Resize-VerticalScroll([int]$Id){
    if($Device -eq 'mouse'){$script:ResizeVerticalScrollStatus='not-applicable-mouse';return}
    $metrics={@(((Workspace-Root).Current.ItemStatus|ConvertFrom-Json).groups|Where-Object id -eq $Id)[0].body_scroll}
    $provider=@{element=$null;pattern=$null}
    $node=Control "workspace-group-$Id" -Arranged
    $scrollPredicate=[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::IsScrollPatternAvailableProperty,$true)
    Wait-Until {
        foreach($element in $node.FindAll([System.Windows.Automation.TreeScope]::Descendants,$scrollPredicate)){
            $pattern=$element.GetCurrentPattern([System.Windows.Automation.ScrollPattern]::Pattern)
            if($pattern.Current.VerticallyScrollable){$provider.element=$element;$provider.pattern=$pattern;return $true}
        }
        $false
    } 'Floating Stats exposes no native vertical scroll provider'
    $nativeScroll=$provider.pattern
    Wait-Until {$m=& $metrics;$nativeScroll.Current.VerticallyScrollable -and $m.extent_height -gt $m.viewport_height} 'Floating Stats has no real native vertical scroll range'
    $nativeScroll.SetScrollPercent([System.Windows.Automation.ScrollPattern]::NoScroll,0)
    Wait-Until {(& $metrics).vertical_offset -eq 0} 'Native Stats did not start at its top offset'
    $b=((Resize-Layout).groups|Where-Object id -eq $Id|Select-Object -First 1).bounds
    $from=Resize-Point ($b.x+$b.width-20) ($b.y+$b.height*.65)
    $to=Resize-Point $from.x ($from.y-80)
    $a=Resize-Screen $from;$z=Resize-Screen $to
    $receipt=@{device=$Device;from=$from;to=$to;before=(& $metrics);frame_before=$b;native_provider=@(Control-Trace $provider.element)}
    [CapyRowPointer]::Down($Device,$a.x,$a.y)
    try{
        for($i=1;$i -le 8;$i++){[CapyRowPointer]::Move($a.x,[int]($a.y+($z.y-$a.y)*$i/8));Start-Sleep -Milliseconds 16}
        Start-Sleep -Milliseconds 200
        [CapyRowPointer]::Up()
        Wait-Until {(& $metrics).vertical_offset -gt 1} 'A real vertical Stats swipe did not change native ScrollView.VerticalOffset'
        $after=((Resize-Layout).groups|Where-Object id -eq $Id|Select-Object -First 1).bounds
        if($after.width -ne $b.width -or $after.x -ne $b.x){throw 'The vertical Stats scroll resized or moved its floating frame'}
    }finally{
        if([CapyRowPointer]::Active){[CapyRowPointer]::Cancel()}
        $receipt.after=& $metrics;$receipt.frame_after=((Resize-Layout).groups|Where-Object id -eq $Id|Select-Object -First 1).bounds
        $receipt|ConvertTo-Json -Depth 12|Set-Content (Join-Path $run 'floating-vertical-scroll.json')
    }
    $nativeScroll.SetScrollPercent([System.Windows.Automation.ScrollPattern]::NoScroll,0)
    Wait-Until {(& $metrics).vertical_offset -eq 0} 'Native Stats scroll offset did not restore after the gesture'
    Resize-FloatingReady $Id
    $script:ResizeVerticalScrollStatus='passed'
    Write-Output "RESIZE_PASS actual-native-vertical-scroll frame-width-unchanged offset-restored $Device $Theme"
}
function Resize-Floating {
    Invoke-Id 'panel-tab-stats';Resize-Settled
    $beforeFloat=Resize-State
    $tab=(Control 'panel-tab-stats' -Arranged).Current.BoundingRectangle
    $work=(Resize-Layout).work_area
    $to=Resize-Screen (Resize-Point ($work.x+$work.width*.5) ($work.y+$work.height*.4))
    [CapyRowPointer]::Down('mouse',[int]($tab.X+$tab.Width*.5),[int]($tab.Y+$tab.Height*.5))
    [CapyRowPointer]::Move($to.x,$to.y)
    Wait-Until {(Resize-Gesture).phase -eq 'dragging'} 'Stats tab did not start a native tear-off'
    [CapyRowPointer]::Up();Resize-Settled
    $floating={@((Resize-Layout).groups|Where-Object {$_.floating -and $_.panels -contains 'stats'})[0]}
    Wait-Until {$null -ne (& $floating)} 'Stats did not float on the native canvas'
    Resize-FloatingReady (& $floating).id
    $beforeHeight=Resize-State
    $handle=(& $floating).resize_handles|Where-Object edge -eq 'bottom'|Select-Object -First 1
    $at=Resize-Screen (Resize-Point ($handle.bounds.x+$handle.bounds.width*.5) ($handle.bounds.y+$handle.bounds.height*.5))
    [CapyRowPointer]::Down('mouse',$at.x,$at.y)
    $to=Resize-Screen (Resize-Point ($handle.bounds.x+$handle.bounds.width*.5) ((& $floating).bounds.y+263))
    for($i=1;$i -le 8;$i++){[CapyRowPointer]::Move([int]($at.x+($to.x-$at.x)*$i/8),[int]($at.y+($to.y-$at.y)*$i/8));Start-Sleep -Milliseconds 16}
    [CapyRowPointer]::Up();Resize-Settled
    Wait-Until {(& $floating).bounds.height -ge 240} 'Native bottom handle did not prepare a tall floating panel'
    Resize-FloatingReady (& $floating).id
    Resize-VerticalScroll (& $floating).id
    $before=Resize-State
    $sides=@('left','right','bottom','bottom-left','bottom-right')
    if($Device -eq 'touch'){$sides+='right-canvas'}
    foreach($side in $sides){
        Resize-FloatingReady (& $floating).id
        $b=(& $floating).bounds
        $left=$side -match 'left';$right=$side -match 'right';$bottom=$side -match 'bottom'
        $y=if($bottom){$b.y+$b.height-12}else{$b.y+$b.height*.65}
        $candidates=@(foreach($distance in 12,20,28){
            $x=if($left){$b.x+$distance}elseif($right){$b.x+$b.width-$distance}else{$b.x+$b.width*.5}
            foreach($dy in 0,8,16){Resize-Point $x ($y-$dy)}
        })
        $canvasClaim=$null
        if($side -eq 'right-canvas'){
            $canvasOriginalRight=$b.x+$b.width
            $from=Resize-Point ($canvasOriginalRight+20) $y
            $canvasExpectedRight=$from.x+48
            $canvasAt=Resize-Screen $from
            Input-Trace $canvasAt.x $canvasAt.y|ConvertTo-Json -Depth 12|Set-Content (Join-Path $run 'floating-canvas-hit.json')
            $canvasClaim={$canvasCurrent=(& $floating).bounds;$canvasCurrent.x+$canvasCurrent.width -gt $canvasOriginalRight}
        }else{$from=Resize-Reach $candidates {param($edge)$edge.contact -eq 'free' -and [bool]$edge.vertical -eq $bottom}}
        $cursor=if($left -and $bottom){32643}elseif($right -and $bottom){32642}elseif($bottom){32645}else{32644}
        if($Device -eq 'mouse'){
            $handle=(& $floating).resize_handles|Where-Object edge -eq $side.Replace('-','_')|Select-Object -First 1
            $hover=Resize-Screen (Resize-Point ($handle.bounds.x+$handle.bounds.width*.5) ($handle.bounds.y+$handle.bounds.height*.5))
            [CapyRowPointer]::Hover($hover.x,$hover.y);Resize-Cursor $cursor
        }
        $dx=if($left){-48}elseif($right){48}else{0};$dy=if($bottom){48}else{0}
        Resize-Drag $from @((Resize-Point ($from.x+$dx) ($from.y+$dy))) -Cursor $cursor -Claim $canvasClaim
        $after=(& $floating).bounds
        if($side -eq 'right-canvas'){
            $canvasTolerance=((& $floating).resize_handles|Where-Object edge -eq 'right'|Select-Object -First 1).bounds.width
            if([Math]::Abs($after.x+$after.width-$canvasExpectedRight) -gt $canvasTolerance){throw 'The touch canvas-side floating edge did not catch its contact within its native handle'}
        }
        if((($left -or $right) -and $after.width -le $b.width+20) -or ($left -and $after.x -ge $b.x-20) -or ($bottom -and $after.height -le $b.height+20)){throw "Floating $side reach did not grow its owner: $($b|ConvertTo-Json -Compress) -> $($after|ConvertTo-Json -Compress)"}
        Resize-Undo $before
        Write-Output "RESIZE_PASS floating-$side $Device $Theme cursor=$cursor"
        if($Device -eq 'touch' -and $Theme -eq 'light' -and $side -eq 'right'){
            $mixedExpectedLayout=Resize-State
            Resize-Drag $from @((Resize-Point ($from.x+24) $from.y)) -During {[CapyRowPointer]::Cancel()}
            Wait-Until {(Resize-State) -ceq $mixedExpectedLayout} 'Canceled touch resize did not restore the mixed-input frame'
            Resize-FloatingReady (& $floating).id
            $mixedAt=Resize-Screen $from
            [CapyRowPointer]::Hover($mixedAt.x,$mixedAt.y);[CapyRowPointer]::Down('mouse',$mixedAt.x,$mixedAt.y)
            try{
                Wait-Until {$mixedEdge=(Resize-Gesture).edge;$mixedEdge.contact -eq 'free' -and !$mixedEdge.vertical} 'Mouse did not retain free horizontal reach after touch cancellation'
                $mixedPending=Resize-Gesture
                [CapyRowPointer]::Key([uint32]$review.Id,0x1B)
                Wait-Until {!((Resize-Gesture).edge)} 'Mixed-input mouse Escape did not clear its edge'
                [CapyRowPointer]::Cancel()
            }finally{if([CapyRowPointer]::Active){[CapyRowPointer]::Cancel()}}
            Wait-Until {$mixedState=Resize-Gesture;$mixedState.phase -eq 'idle' -and !$mixedState.ignore_click -and (Resize-State) -ceq $mixedExpectedLayout} 'Mixed-input mouse release changed its frame or left a contact'
            @{point=$from;mouse_pending=$mixedPending;after=(Resize-Gesture);layout=(Model).state.workspace.layout}|ConvertTo-Json -Depth 90|Set-Content (Join-Path $run 'floating-mixed-input.json')
            $script:ResizeMixedInputStatus='passed'
            Write-Output 'RESIZE_PASS touch-cancel-then-mouse-free-edge escape-release-exact-frame'
        }
    }
    Resize-Undo $beforeHeight
    Resize-Undo $beforeFloat
}
function Resize-TabStrip {
    $layout=Resize-Layout;$fitted=@((Model).state.workspace.layout.fit_height_groups)
    $pair=@{gap=$null;group=$null;panel=$null}
    foreach($gap in @($layout.dividers|Where-Object {$_.axis -eq 'vertical' -and !$_.band -and !$_.fixed})){
        $above=$layout.groups|Where-Object {!$_.floating -and [Math]::Abs($_.bounds.x-$gap.bounds.x) -lt 1 -and [Math]::Abs($_.bounds.y+$_.bounds.height-$gap.bounds.y) -lt 1}|Select-Object -First 1
        $below=$layout.groups|Where-Object {!$_.floating -and $_.tabs_visible -and $_.panels.Count -gt 1 -and [Math]::Abs($_.bounds.x-$gap.bounds.x) -lt 1 -and [Math]::Abs($_.bounds.y-$gap.bounds.y-$gap.bounds.height) -lt 1}|Select-Object -First 1
        if($above -and $below -and $fitted -notcontains $above.id -and $fitted -notcontains $below.id){$pair.gap=$gap;$pair.group=$below;$pair.panel=@($below.panels|Where-Object {$_ -ne $below.active})[0];break}
    }
    if(!$pair.gap){throw 'Paint has no inactive native tab immediately below a flexible group gap'}
    $tab=Control ('panel-tab-'+$pair.panel) -Arranged
    $native=$tab.Current.BoundingRectangle;$origin=Resize-Screen (Resize-Point 0 0)
    $scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
    $x=($native.X+$native.Width*.5-$origin.x)/$scale
    $from=Resize-Point $x ($pair.group.bounds.y+3);$at=Resize-Screen $from
    if(!$native.Contains([Windows.Point]::new($at.x,$at.y)) -or $x -ge $pair.group.bounds.x+$pair.group.bounds.width-36){throw 'The top-six-unit contact is not inside the inactive native tab away from its grip'}
    $before=Resize-State;$active=$pair.group.active
    Resize-Drag $from @((Resize-Point $from.x ($from.y-50))) -Cursor 32645 -Contact {
        Wait-Until {$e=(Resize-Gesture).edge;$e.contact -eq 'pickup' -and $e.vertical} 'Actual tab sliver did not expose vertical pickup edge reach'
        if(((Resize-Layout).groups|Where-Object id -eq $pair.group.id).active -ne $active){throw 'Tab activated before the pending vertical contact decided'}
    } -During {
        if(((Resize-Layout).groups|Where-Object id -eq $pair.group.id).active -ne $active){throw 'Tab-strip resize activated the inactive tab'}
    }
    $after=(Resize-Layout).dividers|Where-Object id -eq $pair.gap.id|Select-Object -First 1
    if($pair.gap.bounds.y-$after.bounds.y -lt 20){throw 'Actual tab-strip vertical reach did not move its group gap'}
    Resize-Undo $before
    Wait-Until {[Math]::Abs($tab.Current.BoundingRectangle.Y-$native.Y) -lt 2} 'Native tab did not return after gap Undo'
    [CapyRowPointer]::Down($Device,$at.x,$at.y)
    try{
        Wait-Until {(Resize-Gesture).edge.contact -eq 'pickup'} 'Fresh tab-sliver tap did not begin pending pickup'
        if(((Resize-Layout).groups|Where-Object id -eq $pair.group.id).active -ne $active){throw 'Tab-sliver tap selected its tab before release'}
        [CapyRowPointer]::Up()
    }finally{if([CapyRowPointer]::Active){[CapyRowPointer]::Cancel()}}
    Wait-Until {((Resize-Layout).groups|Where-Object id -eq $pair.group.id).active -eq $pair.panel} 'Tab-sliver tap did not select its inactive tab on release'
    Resize-Settled
    Write-Output "RESIZE_PASS actual-tab-top-six pickup vertical-gap unchanged-active exact-undo release-select $Device $Theme"
}
function Resize-Band {
    $layout=Resize-Layout;$band=$layout.dividers|Where-Object {$_.band -and $_.axis -eq 'vertical'}|Select-Object -First 1
    if(!$band){throw 'Paint has no toolbar-band resize edge'}
    $bar=$layout.groups|Where-Object {$_.panels -contains 'commands'}|Select-Object -First 1
    if(!$bar){throw 'Paint has no Commands toolbar band'}
    $open=$layout.collapsed|Where-Object open|Select-Object -First 1
    $openPanel=if($open){@($open.groups.icons.panel|Where-Object {(Control ('column-icon-'+$_)).Current.ItemStatus -eq 'Selected'})[0]}
    $before=Resize-State;$height=$bar.bounds.height
    $from=Resize-Reach @(foreach($x in 18,54,90,126,162,198){Resize-Point ($bar.bounds.x+$x) ($band.bounds.y-14)}) -DoubleEscape
    if(Find 'new-drawing-dialog'){throw 'Canceled native toolbar probe activated New drawing on release'}
    if($open -and !@((Resize-Layout).collapsed|Where-Object {$_.id -eq $open.id -and $_.open}).Count){
        if(!$openPanel){throw 'Band setup had no selected native stack icon'}
        Invoke-Id ('column-icon-'+$openPanel)
        Wait-Until {@((Resize-Layout).collapsed|Where-Object {$_.id -eq $open.id -and $_.open}).Count -eq 1} 'The native stack icon did not restore the band prerequisite'
        Resize-Settled
    }
    Resize-Drag $from @((Resize-Point $from.x ($from.y+72))) -Cursor 32645
    $after=(Resize-Layout).groups|Where-Object id -eq $bar.id|Select-Object -First 1
    if($after.bounds.height -lt $height+36){throw 'Toolbar-band reach did not add a tile lane'}
    Resize-Undo $before
    Write-Output "RESIZE_PASS commands-band-adds-lanes $Device $Theme"
}
function Resize-Columns {
    $layout=Resize-Layout;$toolbar=$layout.groups|Where-Object {$_.panels -contains 'toolbar'}|Select-Object -First 1
    $adjacent=@($toolbar.bounds.x,$toolbar.bounds.width)
    $edge=$layout.dividers|Where-Object {$_.axis -eq 'horizontal' -and $_.owner_side -eq 'left' -and !$_.fixed}|Sort-Object {$_.bounds.x}|Select-Object -Last 1
    $groups=@($layout.groups|Where-Object {!$_.floating -and [Math]::Abs($_.bounds.x+$_.bounds.width-$edge.bounds.x) -lt 1})
    $owner=$groups|Sort-Object {$_.bounds.height}|Select-Object -Last 1
    if(!$owner){throw 'Paint has no content column beside the toolbar'}
    $before=Resize-State
    $from=Resize-Reach @(foreach($g in $groups){foreach($dy in 24,48,72,96,120){if($dy+36 -lt $g.bounds.height){Resize-Point ($edge.bounds.x-20) ($g.bounds.y+$g.bounds.height-$dy)}}})
    $far=Resize-Point ($owner.bounds.x+12) $from.y
    $columnTargets=@($far,$from,$far)
    $columnReceipt=@{before=($before|ConvertFrom-Json);before_gesture=(Resize-Gesture);from=$from;far=$far;requested=@(foreach($target in $columnTargets){@{logical=$target;screen=(Resize-Screen $target)}});groups_before=@((Resize-Layout).groups);steps=[Collections.Generic.List[object]]::new()}
    $columnReceipt|ConvertTo-Json -Depth 90|Set-Content (Join-Path $run 'resize-column.json')
    Resize-Drag $from $columnTargets -Cursor 32644 -During {
        param($step)
        $columnReceipt.steps.Add(@{step=$step;requested=$columnReceipt.requested[$step];gesture=(Resize-Gesture);collapsed=@((Resize-Layout).collapsed);layout=(Model).state.workspace.layout})
        $columnReceipt|ConvertTo-Json -Depth 90|Set-Content (Join-Path $run 'resize-column.json')
        $collapsed=@((Resize-Layout).collapsed|Where-Object {$_.groups.icons.panel -contains $owner.active}).Count -gt 0
        if(($step -ne 1 -and !$collapsed) -or ($step -eq 1 -and $collapsed)){throw "Column collapse/reversal failed at step $step"}
        $now=(Resize-Layout).groups|Where-Object id -eq $toolbar.id|Select-Object -First 1
        if($now.bounds.x -ne $adjacent[0] -or $now.bounds.width -ne $adjacent[1]){throw 'Resizing the adjacent column changed the toolbar width or position'}
    }
    $closedState=Resize-State
    $column=(Resize-Layout).collapsed|Where-Object {$_.groups.icons.panel -contains $owner.active}|Select-Object -First 1
    $icon=$column.groups.icons|Where-Object panel -eq $owner.active|Select-Object -First 1
    $from=Resize-Point ($icon.bounds.x+$icon.bounds.width*.5) ($icon.bounds.y+$icon.bounds.height*.5)
    Resize-Drag $from @((Resize-Point ($from.x+90) $from.y)) -Cursor 32644
    Wait-Until {!@((Resize-Layout).collapsed|Where-Object {$_.id -eq $column.id -and !$_.open}).Count} 'A fresh outward swipe did not open the closed column'
    $now=(Resize-Layout).groups|Where-Object id -eq $toolbar.id|Select-Object -First 1
    if($now.bounds.x -ne $adjacent[0] -or $now.bounds.width -ne $adjacent[1]){throw 'Opening the closed column changed the adjacent toolbar'}
    Resize-Undo $closedState
    Resize-Undo $before
    Write-Output "RESIZE_PASS column-collapse-reversal fresh-closed-column-open adjacent-width exact-undo $Device $Theme"
}
function Resize-Stack {
    $destination=(Resize-Layout).collapsed|Sort-Object {$_.bounds.x}|Select-Object -Last 1
    if(!$destination){throw 'Paint has no collapsed column for the native stack setup'}
    $tab=(Control 'panel-tab-stats' -Arranged).Current.BoundingRectangle
    $to=Resize-Screen (Resize-Point ($destination.bounds.x+$destination.bounds.width*.5) ($destination.bounds.y*.5))
    [CapyRowPointer]::Down('mouse',[int]($tab.X+$tab.Width*.5),[int]($tab.Y+$tab.Height*.5))
    try{
        [CapyRowPointer]::Move($to.x,$to.y)
        Wait-Until {$hint=((Workspace-Root).Current.ItemStatus|ConvertFrom-Json).workspace_update.drag.drop_hint.target;(Resize-Gesture).phase -eq 'dragging' -and $hint.kind -eq 'stack_column' -and $hint.column -eq $destination.id} 'Stats tab did not preview a native stack-column drop'
        [CapyRowPointer]::Up();Resize-Settled
    }finally{if([CapyRowPointer]::Active){[CapyRowPointer]::Cancel()}}
    $member=@{column=$null}
    Wait-Until {$member.column=(Resize-Layout).collapsed|Where-Object {$_.groups.icons.panel -contains 'stats'}|Select-Object -First 1;$member.column -and @((Resize-Layout).collapsed|Where-Object stack -eq $member.column.stack).Count -eq 2} 'Native drop did not create an actual two-member column stack'
    $strip=$member.column;$panel='stats'
    foreach($opened in @((Resize-Layout).collapsed|Where-Object {$_.stack -eq $strip.stack -and $_.open})){
        $selected=@($opened.groups.icons.panel|Where-Object {(Control ('column-icon-'+$_)).Current.ItemStatus -eq 'Selected'})[0]
        if(!$selected){throw 'An open native stack member has no selected icon'}
        Invoke-Id ('column-icon-'+$selected);Resize-Settled
    }
    Wait-Until {!@((Resize-Layout).collapsed|Where-Object {$_.stack -eq $strip.stack -and $_.open}).Count} 'The native stack did not close every member before the edge test'
    $before=Resize-State
    $historyBefore=[bool]@((Model).state.commands|Where-Object {$_.id -eq 'undo_workspace' -and $_.enabled}).Count
    $receipt=@{before=($before|ConvertFrom-Json);undo_available_before=$historyBefore}
    $closed=(Resize-Layout).collapsed|Where-Object id -eq $strip.id|Select-Object -First 1
    $icon=$closed.groups.icons|Where-Object panel -eq $panel|Select-Object -First 1
    $from=Resize-Point ($icon.bounds.x+$icon.bounds.width*.5) ($icon.bounds.y+$icon.bounds.height*.5)
    $expected=Resize-Screen $from
    Wait-Until {$box=(Control ('column-icon-'+$panel) -Arranged).Current.BoundingRectangle;[Math]::Abs($box.X+$box.Width*.5-$expected.x) -le 2 -and [Math]::Abs($box.Y+$box.Height*.5-$expected.y) -le 2} 'The closed native stack icon did not reach its shared position'
    $out=if($closed.bounds.x -gt (Resize-Layout).viewport[0]*.5){-90}else{90}
    Resize-Drag $from @((Resize-Point ($from.x+$out) $from.y)) -Cursor 32644
    Wait-Until {@((Resize-Layout).collapsed|Where-Object {$_.id -eq $strip.id -and $_.open}).Count -eq 1} 'A closed stack member did not open from its native icon reach'
    $receipt.after_open=(Model).state.workspace.layout
    if((Resize-State) -cne $before){throw 'Opening a stack member changed durable layout'}
    $open=((Resize-Layout).collapsed|Where-Object id -eq $strip.id|Select-Object -First 1).open
    $inward=if($closed.bounds.x -gt (Resize-Layout).viewport[0]*.5){1}else{-1}
    $line=if($inward -eq 1){$open.bounds.x}else{$open.bounds.x+$open.bounds.width}
    $from=Resize-Reach @(foreach($dy in 60,92,124,156,188){Resize-Point ($line+$inward*20) ($open.bounds.y+$dy)})
    $receipt.after_probe=(Model).state.workspace.layout
    $receipt|ConvertTo-Json -Depth 90|Set-Content (Join-Path $run 'resize-stack.json')
    $far=Resize-Point ($line+$inward*$open.bounds.width) $from.y
    Resize-Drag $from @($far,$from,$far) -Cursor 32644 -During {
        param($step)
        $isOpen=@((Resize-Layout).collapsed|Where-Object {$_.id -eq $strip.id -and $_.open}).Count -eq 1
        $receipt.step=$step;$receipt.after_close=(Model).state.workspace.layout
        $receipt|ConvertTo-Json -Depth 90|Set-Content (Join-Path $run 'resize-stack.json')
        if(($step -ne 1 -and $isOpen) -or ($step -eq 1 -and !$isOpen)){throw "Stack close/reversal failed at step $step"}
    }
    $receipt.after_release=(Model).state.workspace.layout
    $receipt.undo_available_after=[bool]@((Model).state.commands|Where-Object {$_.id -eq 'undo_workspace' -and $_.enabled}).Count
    $receipt|ConvertTo-Json -Depth 90|Set-Content (Join-Path $run 'resize-stack.json')
    if((Resize-State) -cne $before){throw 'Stack close/reversal changed durable layout after release'}
    if($receipt.undo_available_after -ne $historyBefore){throw 'Transient stack close/reversal changed workspace Undo availability'}
    Write-Output "RESIZE_PASS actual-two-member-stack native-drop closed-open-close-reversal durable-unchanged $Device $Theme"
}
function WindowCommand([string]$Id){
    Wait-Until {$root.FindAll([System.Windows.Automation.TreeScope]::Descendants,
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,
        [System.Windows.Automation.ControlType]::MenuItem)).Count -eq 0} 'Previous native menu remained visible'
    Start-Sleep -Milliseconds 250
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'window';Invoke $Id
}
function Toolbar([string]$Id){(Model).panels|Where-Object id -eq $Id}
function ToolbarContext([string]$Id){
    Wait-Until {$root.FindAll([System.Windows.Automation.TreeScope]::Descendants,
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,
        [System.Windows.Automation.ControlType]::MenuItem)).Count -eq 0} 'Previous native menu remained visible'
    Start-Sleep -Milliseconds 250
    (Control $Id).SetFocus();[CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x10),0x79)
}
function Edit([string]$Id,[string]$Value){(Control $Id).GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($Value)}
function DialogButton([string]$Id,[string]$Name){
    $dialog=Control $Id;$found=@{item=$null}
    $buttonMatch=[System.Windows.Automation.AndCondition]::new(
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,$Name),
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::Button))
    Wait-Until {$found.item=$dialog.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$buttonMatch);$null -ne $found.item} "Missing dialog button $Name"
    $found.item
}
function InvokeDialog([string]$Id,[string]$Name){(DialogButton $Id $Name).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()}
function Configure([string]$Panel,[string]$Source="panel-tab-$Panel"){
    Wait-Until {$null -ne ((Model).panels|Where-Object id -eq $Panel)} "Missing shared panel $Panel"
    $model=(Model).panels|Where-Object id -eq $Panel
    ToolbarContext $Source
    Invoke ($model.configuration_title+'…') -Name
    Wait-Until {(Model).state.customization.expanded -eq $Panel} "Core did not expand $Panel"
    $configuration=Control "panel-configuration-$Panel"
    Wait-Until {$configuration.Current.BoundingRectangle.Width -gt 100} "Configuration $Panel has no native bounds"
    Start-Sleep -Milliseconds 400
    $configuration
}
function Dismiss([string]$Panel,[string]$Source="panel-tab-$Panel"){
    (Control $Source).SetFocus();[CapyRowPointer]::Key([uint32]$review.Id,0x1B)
    Wait-Until {$null -eq (Model).state.customization.expanded -and $null -eq (Find "panel-configuration-$Panel")} "Escape did not close $Panel configuration"
}
function Toggle([string]$Id){(Control $Id).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()}
function Wait-OverviewProjection{
    $drawingWindow=[IntPtr]$root.Current.NativeWindowHandle
    $origin=[CapyRowPointer+Point]::new()
    if(![CapyRowPointer]::ClientToScreen($drawingWindow,[ref]$origin)){throw 'Cannot locate overview projection'}
    $scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
    Wait-Until {
        $workspace=Workspace-Root
        if(!$workspace){return $false}
        $slots=@(($workspace.Current.ItemStatus|ConvertFrom-Json).overviews|Where-Object {$_.opacity -gt 0})
        $visible=@(foreach($slot in $slots){
            $b=$slot.bounds;$c=$slot.clip
            $x=[Math]::Max($b[0],$c[0]);$y=[Math]::Max($b[1],$c[1])
            $right=[Math]::Min($b[0]+$b[2],$c[0]+$c[2]);$bottom=[Math]::Min($b[1]+$b[3],$c[1]+$c[3])
            if($right -gt $x -and $bottom -gt $y){@{left=$origin.x+$x*$scale;top=$origin.y+$y*$scale;right=$origin.x+$right*$scale;bottom=$origin.y+$bottom*$scale}}
        })
        $remaining=[Collections.Generic.List[object]]::new()
        foreach($item in $root.FindAll([System.Windows.Automation.TreeScope]::Descendants,
            [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'navigator-overview'))){
            $current=$item.Current;$box=$current.BoundingRectangle
            if($current.ProcessId -eq $review.Id -and !$current.IsOffscreen -and $box.Width -gt 0 -and $box.Height -gt 0){$remaining.Add($box)}
        }
        if(!$visible.Count -or $visible.Count -ne $remaining.Count){return $false}
        foreach($box in $visible){
            $match=$remaining|Sort-Object {[Math]::Abs($_.Left-$box.left)+[Math]::Abs($_.Top-$box.top)}|Select-Object -First 1
            if([Math]::Abs($match.Left-$box.left) -gt 2 -or [Math]::Abs($match.Top-$box.top) -gt 2 -or
                [Math]::Abs($match.Right-$box.right) -gt 2 -or [Math]::Abs($match.Bottom-$box.bottom) -gt 2){return $false}
            $remaining.Remove($match)|Out-Null
        }
        $true
    } 'Published overview slots did not converge to live Navigator bounds'
}
function Check-OverviewOverlap($Configuration){
    $overview=(Control 'navigator-overview' -Within $Configuration).Current.BoundingRectangle
    $drawingWindow=[IntPtr]$root.Current.NativeWindowHandle
    $origin=[CapyRowPointer+Point]::new()
    if(![CapyRowPointer]::ClientToScreen($drawingWindow,[ref]$origin)){throw 'Cannot locate client capture'}
    $scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
    $document=(Model).state.tabs[0]
    $fit=[Math]::Min(($overview.Width-8*$scale)/$document.width,($overview.Height-8*$scale)/$document.height)
    $width=$document.width*$fit;$height=$document.height*$fit
    $groups=@((Model).layout.groups|Where-Object {$_.active -ne 'navigator'})
    $hit=$null
    foreach($fy in 0.1,0.3,0.5,0.7,0.9){foreach($fx in 0.04,0.2,0.5,0.8,0.96){
        if($hit){break}
        $px=[int]($overview.Left-$origin.x+($overview.Width-$width)/2+$width*$fx)
        $py=[int]($overview.Top-$origin.y+($overview.Height-$height)/2+$height*$fy)
        foreach($group in $groups){$b=$group.bounds
            if($px -gt $b.x*$scale -and $px -lt ($b.x+$b.width)*$scale -and $py -gt ($b.y+36)*$scale -and $py -lt ($b.y+$b.height)*$scale){$hit=@{x=$px;y=$py};break}}
    }}
    if(!$hit){throw 'Fixture did not overlap a lower native panel'}
    $x=$hit.x;$y=$hit.y
    $before=[Drawing.Bitmap]::new((Join-Path $run 'navigator-before.png'))
    $after=[Drawing.Bitmap]::new((Join-Path $run 'navigator.png'))
    try{
        $old=$before.GetPixel($x,$y);$pixel=$after.GetPixel($x,$y)
        if($old.R -gt 245 -and $old.G -gt 245 -and $old.B -gt 245){throw 'Overlap reference was already white'}
        if($pixel.R -lt 245 -or $pixel.G -lt 245 -or $pixel.B -lt 245){throw 'Lower native panel covers the GPU overview'}
        return @{x=$x;y=$y;before=$old.ToArgb()}
    }finally{$before.Dispose();$after.Dispose()}
}
function Shown([string]$Panel,[string]$Control){((Model).panels|Where-Object id -eq $Panel).controls|Where-Object control -eq $Control|Select-Object -ExpandProperty visible_in_panel}
function Check-SizeGrid($Within,[string]$Device,[string]$Label){
    $found=@{tiles=@()}
    Wait-Until {
        $buttons=$Within.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new(
            [System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::Button))
        $found.tiles=@($buttons|Where-Object {$_.Current.AutomationId -like 'size-preset-*'})
        $found.tiles.Count -eq 40
    } "Size grid shows $($found.tiles.Count) of 40 presets"
    $tiles=$found.tiles
    $scale=[CapyRowPointer]::GetDpiForWindow($review.MainWindowHandle)/96.;$tile=@(36,44)
    $first=$tiles[0].Current.BoundingRectangle
    $row=@($tiles|Where-Object {[Math]::Abs($_.Current.BoundingRectangle.Top-$first.Top) -lt 1})
    foreach($item in $row){$box=$item.Current.BoundingRectangle
        if([Math]::Abs($box.Width-$tile[0]*$scale) -gt 1.5 -or [Math]::Abs($box.Height-$tile[1]*$scale) -gt 1.5){throw "Size tile $($item.Current.AutomationId) is not $($tile[0]) x $($tile[1]) DIP"}}
    if($row.Count -lt 6){throw "Size grid fits only $($row.Count) presets in a row"}
    $pitch=($row[1].Current.BoundingRectangle.Left-$first.Left)/$scale
    if([Math]::Abs($pitch-$tile[0]-2) -gt 1){throw "Size tiles are $pitch DIP apart"}
    $rows=[Math]::Ceiling($tiles.Count/$row.Count)
    $whole=[Math]::Abs($tiles[-1].Current.BoundingRectangle.Height-$tile[1]*$scale) -le 1.5
    if($whole -and [Math]::Abs(($tiles[-1].Current.BoundingRectangle.Bottom-$first.Top)/$scale-($rows*$tile[1]+($rows-1)*2)) -gt 2){throw 'Size grid rows do not wrap at the shared spacing'}
    $target=$tiles|Where-Object {$_.Current.AutomationId -eq "size-preset-$Label"}
    $value=[double]::Parse($Label,[Globalization.CultureInfo]::InvariantCulture)
    $box=$target.Current.BoundingRectangle;$x=[int]($box.Left+$box.Width/2);$y=[int]($box.Top+$box.Height/2)
    [CapyRowPointer]::Down($Device,$x,$y);Start-Sleep -Milliseconds 40;[CapyRowPointer]::Up()
    Wait-Until {[Math]::Abs((Model).state.brush.diameter-$value) -lt .001} "$Device tap on size $Label did not set the brush size"
    Wait-Until {$target.Current.ItemStatus -ne ''} "Size $Label did not show as selected"
    @{presets=$tiles.Count;columns=$row.Count;rows=$rows;wrap_checked=$whole;device=$Device;size=$Label}
}
try{
    Enter-CapyEnvironment
    $env:CAPY_STORAGE_DIR=Join-Path $run 'profile'
    if($ResizeOnly){[IO.Directory]::CreateDirectory($env:CAPY_STORAGE_DIR)|Out-Null;[IO.File]::WriteAllText((Settings-File),(@{language=@{Explicit='en'};theme=$Theme}|ConvertTo-Json -Depth 4))}
    $env:CAPY_TRACE_UI='1';$env:CAPY_SMOKE_TEST='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
    $stderr=Join-Path $run 'stderr.log'
    $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
    $null=$review.Handle
    [IO.File]::WriteAllText((Join-Path $repo 'artifacts/windows/expansion-review.json'),(@{process_id=$review.Id;run=$run}|ConvertTo-Json))
    Write-Output "Owned expansion review $($review.Id)"
    Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero -and (Model).brush_ready} 'Review did not start' 45
    [CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
    $root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
    [CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)|Out-Null;[CapyRowPointer]::Initialize([uint32]$review.Id)
    if($ResizeOnly){
        $owned=@{drawing=$null}
        Wait-Until {$owned.drawing=Owned-DrawingWindow $review;$owned.drawing -and (Model).state.workspace_ready -and (Model).windows_workspace.ready -and !(Model).windows_workspace.busy -and (Model).state.theme -eq $Theme} 'Resize drawing did not become ready'
        $root=$owned.drawing.Root;$drawingWindow=$owned.drawing.Handle
        Maximize-DrawingWindow;Use-PaintWorkspace;Resize-Settled
        $resizeArtwork=@{before=(Model).state.document_file.revision}
        $resizeArtwork|ConvertTo-Json|Set-Content (Join-Path $run 'resize-artwork.json')
        if(!$ResizeStackOnly){
            if(!$ResizeTailOnly){Resize-Floating;Resize-TabStrip}
            Resize-Band;Resize-Columns
        }
        Resize-Stack
        $resizeArtwork.after=(Model).state.document_file.revision
        $resizeArtwork|ConvertTo-Json|Set-Content (Join-Path $run 'resize-artwork.json')
        if($resizeArtwork.after -cne $resizeArtwork.before){throw "Workspace resizing changed artwork revision from $($resizeArtwork.before) to $($resizeArtwork.after)"}
        Capture 'resize'
        & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close
        if((Get-Item -LiteralPath $stderr).Length){throw 'Native resize stderr requires inspection'}
        [pscustomobject]@{scope=$(if($ResizeStackOnly){'native two-member stack edge'}elseif($ResizeTailOnly){'native workspace resize tail'}else{'native workspace resize'});device=$Device;theme=$Theme;floating_edges=$(if($ResizeTailOnly -or $ResizeStackOnly){'not-run'}else{'passed'});actual_tab_top_six=$(if($ResizeTailOnly -or $ResizeStackOnly){'not-run'}else{'pickup-vertical-resize-and-release-selection-passed'});toolbar_lanes=$(if($ResizeStackOnly){'not-run'}else{'passed'});column_reversal=$(if($ResizeStackOnly){'not-run'}else{'passed'});actual_two_member_stack='closed-open-close-reversal-passed';adjacent_width=$(if($ResizeStackOnly){'not-run'}else{'unchanged'});workspace_undo=$(if($ResizeStackOnly){'not-run'}else{'exact'});cursor=$(if($Device -eq 'mouse'){if($ResizeTailOnly -or $ResizeStackOnly){'claimed-drag-passed'}else{'hover-and-drag-passed'}}else{'not-applicable'});artwork_revision_before=$resizeArtwork.before;artwork_revision_after=$resizeArtwork.after;native_vertical_scroll=$script:ResizeVerticalScrollStatus;mixed_input_after_cancel=$script:ResizeMixedInputStatus;evidence=$run}|ConvertTo-Json|Set-Content (Join-Path $run 'results.json')
        Get-Content (Join-Path $run 'results.json');return
    }
    if(Shown 'sizes' 'brush_size'){throw 'Sizes shows the Brush Size slider by default'}
    if((Model).state.customization.drawer -or !@((Model).layout.groups|Where-Object active -eq 'sizes').Count){Invoke-Id 'panel-tab-sizes'}
    $docked=Check-SizeGrid $root 'mouse' '1.5'
    $configuration=Configure 'sizes'
    $configured=@((Check-SizeGrid $configuration 'pen' '25'),(Check-SizeGrid $configuration 'touch' '0.7'))
    $identity=(Control 'configure-show-brush_size').GetRuntimeId() -join ':'
    $scale=(Control 'panel-tab-sizes').Current.BoundingRectangle.Height/36
    if([Math]::Abs($configuration.Current.BoundingRectangle.Width-380*$scale) -gt 2){throw 'Configuration does not use the shared 380 DIP width'}
    $before=(Model).state.brush.diameter
    (Control 'configure-brush_size-slider').GetCurrentPattern([System.Windows.Automation.RangeValuePattern]::Pattern).SetValue(0.35)
    Wait-Until {(Model).state.brush.diameter -ne $before} 'Configuration slider did not update shared brush size'
    Toggle 'configure-show-brush_size'
    Wait-Until {Shown 'sizes' 'brush_size'} 'Visibility checkbox did not show the main brush size control'
    if(((Control 'configure-show-brush_size').GetRuntimeId() -join ':') -ne $identity){throw 'Visibility change replaced configuration controls'}
    Toggle 'configure-show-brush_size'
    Wait-Until {!(Shown 'sizes' 'brush_size')} 'Visibility checkbox did not hide the main brush size control again'
    Capture 'sizes'
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Resize -Width 1500 -Height 1000
    Wait-Until {(Model).state.customization.expanded -eq 'sizes'} 'Resize dismissed configuration'
    Start-Sleep -Milliseconds 400
    if(((Control 'configure-show-brush_size').GetRuntimeId() -join ':') -ne $identity){throw 'Resize replaced configuration controls'}
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Resize -Width 900 -Height 720
    Start-Sleep -Milliseconds 400
    $placement=$configuration.Current.ItemStatus|ConvertFrom-Json
    if(!$placement -or $placement.configuration.width -gt 380){throw 'Narrow expansion lost shared geometry'}
    if(((Control 'configure-show-brush_size').GetRuntimeId() -join ':') -ne $identity){throw 'Narrow resize replaced configuration controls'}
    Capture 'sizes-narrow'
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Resize -Width 1500 -Height 1000
    Start-Sleep -Milliseconds 400
    Dismiss 'sizes'
    $configuration=Configure 'layers' $(if(Find 'panel-tab-layers'){'panel-tab-layers'}else{'column-icon-layers'})
    $original=(Model).state.layer_tools.editing_layer.id
    Invoke (((Model).state.commands|Where-Object id -eq 'add_layer').label) -Name -Within $configuration
    Wait-Until {(Model).state.layer_tools.editing_layer.id -ne $original} 'Configuration layer action did not create a layer'
    $created=(Model).state.layer_tools.editing_layer.id
    $selector=Control 'configure-control-layers'
    $selector.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    (Control 'Current ink' -Name -Within $selector).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
    $selector.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Collapse()
    Wait-Until {(Model).state.layer_tools.editing_layer.id -eq $original} 'Configuration selector did not change the shared editing layer'
    (Control 'configure-layer-opacity-slider').GetCurrentPattern([System.Windows.Automation.RangeValuePattern]::Pattern).SetValue(0.5)
    Wait-Until {(Model).state.layer_tools.editing_layer.opacity -eq 0.5} 'Configuration opacity did not affect the selected layer'
    if(((Model).state.layers|Where-Object id -eq $created).opacity -ne 1){throw 'Configuration opacity changed the previous target'}
    Toggle 'configure-show-layer_opacity'
    Wait-Until {!(Shown 'layers' 'layer_opacity')} 'Layer opacity visibility did not change'
    Toggle 'configure-show-layer_opacity'
    Wait-Until {Shown 'layers' 'layer_opacity'} 'Layer opacity visibility did not restore'
    Capture 'layers'
    Dismiss 'layers'
    # Navigator is already present in the full editor preset.
    $null=Control 'panel-tab-navigator'
    # Constrain the full editor so the configuration's GPU overview overlaps
    # the opaque Tool Set body; the wide preset leaves empty canvas below it.
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Resize -Width 1100 -Height 1000
    Start-Sleep -Milliseconds 400
    Capture 'navigator-before' -Composed
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Resize -Width 1500 -Height 1000
    $configuration=Configure 'navigator'
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Resize -Width 1800 -Height 1100
    Wait-OverviewProjection
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Resize -Width 1100 -Height 1000
    Wait-OverviewProjection
    $null=Control 'navigator-overview' -Within $configuration
    Invoke 'Test stroke' -Name
    Wait-Until {(Model).state.document_file.modified} 'Controlled stroke did not dirty the review'
    $zoom=(Control 'canvas-camera').Current.Name
    Invoke 'navigator-zoom_in' -Within $configuration
    Wait-Until {(Control 'canvas-camera').Current.Name -ne $zoom} 'Configuration Navigator did not update the shared camera'
    Start-Sleep -Milliseconds 250
    Capture 'navigator' -Composed
    $occlusion=Check-OverviewOverlap $configuration
    Dismiss 'navigator'
    Start-Sleep -Milliseconds 250
    Capture 'navigator-after' -Composed
    $restored=[Drawing.Bitmap]::new((Join-Path $run 'navigator-after.png'))
    try{$pixel=$restored.GetPixel($occlusion.x,$occlusion.y);if($pixel.R -gt 245 -and $pixel.G -gt 245 -and $pixel.B -gt 245){throw 'Closing configuration did not restore the lower native panel'}}
    finally{$restored.Dispose()}
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Resize -Width 1500 -Height 1000
    Start-Sleep -Milliseconds 400
    $configuration=Configure 'sizes'
    Invoke 'settings-button'
    $preferences=Control 'Preferences' -Name -Type ([System.Windows.Automation.ControlType]::Window)
    (Control 'Color theme' -Name -Type ([System.Windows.Automation.ControlType]::ComboBox)).GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    (Control 'Light' -Name -Type ([System.Windows.Automation.ControlType]::ListItem)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
    Wait-Until {(Model).state.theme -eq 'light'} 'Theme preference did not update'
    (Control 'CloseButton' -Within $preferences).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    Wait-Until {$null -eq (Find 'Preferences' -Name -Type ([System.Windows.Automation.ControlType]::Window)) -and (Control 'drawing-canvas').Current.IsEnabled} 'Preferences did not release the canvas'
    if(!(Model).state.customization.expanded){$configuration=Configure 'sizes'}
    $null=Control 'panel-configuration-sizes'
    Start-Sleep -Milliseconds 400
    Capture 'sizes-light'
    Dismiss 'sizes'
    $configuration=Configure 'toolbar' 'ribbon-grip-toolbar'
    Invoke 'Add Tools…' -Name -Within $configuration
    $null=Control 'tool-picker'
    InvokeDialog 'tool-picker' 'Cancel'
    Wait-Until {$null -eq (Find 'tool-picker')} 'Expansion Add Tools picker did not close'
    if((Model).state.customization.expanded){Dismiss 'toolbar' 'ribbon-grip-toolbar'}
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved
    if((Get-Item -LiteralPath $stderr).Length){throw 'Native stderr requires inspection'}
    [pscustomobject]@{size_grid=@($docked)+$configured;shared_expansion_width='passed';live_brush_size='passed';control_visibility='passed';retained_configuration='passed';resize='passed';escape_close='passed';layers='passed';editing_layer_selection='passed';targeted_opacity='passed';navigator_configuration='passed';gpu_preview_occlusion_and_restore='passed';theme='passed';toolbar_add_tools='passed';zero_exit='passed';scope='native configuration projection; full visual parity, physical input and presentation are separate'}|ConvertTo-Json
}catch{
    [IO.File]::WriteAllText((Join-Path $run 'failure-exception.txt'),$_.Exception.ToString())
    if($ResizeOnly -and $review -and !$review.HasExited){try{@{model=(Model);gesture=(Resize-Gesture)}|ConvertTo-Json -Depth 90|Set-Content (Join-Path $run 'resize-failure.json')}catch{}}
    if($review -and !$review.HasExited){try{Capture 'failure'}catch{}}
    [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{
    [CapyRowPointer]::Dispose()
    if($ResizeOnly -and $review -and !$review.HasExited){
        try{& (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved -StateDirectory $directory}catch{if(!$review.HasExited){Stop-Process -Id $review.Id -Force}}
    }
    Exit-CapyEnvironment
}
