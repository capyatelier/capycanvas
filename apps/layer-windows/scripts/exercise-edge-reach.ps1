param([Parameter(Mandatory)][string]$Executable,[ValidateSet('mouse','pen','touch')][string]$Device='mouse',[switch]$ColumnResetOnly,[switch]$CanvasResetOnly,[switch]$ClosedStripOnly,[switch]$LayerRowOnly,[switch]$CancelOnly,[switch]$EdgeEscapeOnly,[switch]$NumericOnly,[switch]$SweepOnly,[ValidateSet('photo','photo-open','photo-collapsed','paint')][string[]]$SweepScenarios=@('photo','photo-open','photo-collapsed','paint'),[ValidateSet('dark','light')][string]$Theme)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyPopups=$true
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/edge-reach/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
function Gesture {
    if(!$script:gestureRoot){$script:gestureRoot=Workspace-Root}
    $trace=$script:gestureRoot.Current.HelpText
    $gesture=try{$trace|ConvertFrom-Json}catch{}
    if($gesture -and $gesture.PSObject.Properties.Name -contains 'native_scrollers' -and @($gesture.native_scrollers).Count){throw "Workspace scrollers accept native touch or pen: $($gesture.native_scrollers|ConvertTo-Json -Compress)"}
    $gesture
}
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
function Check-EdgeEscape($From){
    $state={ $m=Model;@{brush=$m.state.brush;tool_set=$m.state.tool_set;workspace=$m.state.workspace.layout}|ConvertTo-Json -Depth 95 -Compress }
    $beforeState=& $state
    $point=Screen $From.x $From.y
    $records=[Collections.Generic.List[object]]::new()
    $observe={param($stage)$m=Model;$input=Input-Trace $point.x $point.y;$records.Add(@{stage=$stage;gesture=Gesture;input=@{point=$input.point;foreground=$input.foreground;owned_window=$input.owned_window;hit=@($input.hit|Select-Object id,name,type,process);focus=@($input.focus|Select-Object id,name,type,process)};brush=$m.state.brush;tool_set=$m.state.tool_set;workspace=$m.state.workspace.layout})}
    try{
        & $observe 'before'
        [CapyRowPointer]::Down($Device,$point.x,$point.y)
        Wait-Until {$null -ne (Gesture).edge -and !(Gesture).edge.claimed} 'The pending edge contact was not published'
        & $observe 'down'
        [CapyRowPointer]::Key(0x1b);& $observe 'after-escape'
        [CapyRowPointer]::Up();Settled;& $observe 'after-up'
        if((& $state) -cne $beforeState){throw 'Escape changed the pending edge brush, tool or layout after release'}
        & $observe 'pre-swipe'
        Swipe $From @{x=$From.x+64;y=$From.y} -During {
            Wait-Until {(Gesture).edge.claimed} 'The edge did not claim before Escape'
            & $observe 'claimed-swipe'
            [CapyRowPointer]::Key(0x1b)
        }
        & $observe 'after-claimed-escape'
        if((& $state) -cne $beforeState -or (Gesture).edge){throw 'Escape did not restore the claimed edge brush, tool and layout after release'}
    }catch{& $observe 'failed-swipe';throw}finally{
        $records|ConvertTo-Json -Depth 95|Set-Content (Join-Path $run ('edge-escape-'+$Theme+'.json'))
        if([CapyRowPointer]::Active){[CapyRowPointer]::Cancel()}
    }
}
function Start-Review([string]$Theme,[string]$Name=$Theme){
    $script:gestureRoot=$null
    $script:stderr=Join-Path $run ($Name+'-stderr.log')
    $env:CAPY_STORAGE_DIR=Join-Path $run ($Name+'-profile')
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
function Reach-Samples {
    $layout=Layout
    foreach($divider in @($layout.dividers|Where-Object {!$_.fixed -and $_.owner_side})){
        $bounds=$divider.bounds
        if($divider.axis -eq 'horizontal'){
            $sign=if($divider.owner_side -eq 'right'){1}else{-1}
            $edge=if($sign -gt 0){$bounds.x+$bounds.width}else{$bounds.x}
            for($y=$bounds.y+12;$y -le $bounds.y+$bounds.height-12;$y+=48){
                foreach($depth in 4,18,32){
                    $x=$edge+$sign*$depth
                    if($x -gt 1 -and $x -lt $layout.viewport[0]-1){[pscustomobject]@{divider=$divider.id;axis='horizontal';depth=$depth;x=$x;y=$y;dx=-$sign*6;dy=0}}
                }
            }
        }else{
            for($x=$bounds.x+12;$x -le $bounds.x+$bounds.width-12;$x+=48){
                foreach($depth in 4,16){foreach($side in -1,1){
                    $y=if($side -lt 0){$bounds.y-$depth}else{$bounds.y+$bounds.height+$depth}
                    [pscustomobject]@{divider=$divider.id;axis='vertical';depth=$depth;x=$x;y=$y;dx=0;dy=6}
                }}
            }
        }
    }
}
function Reach-Geometry {(Layout).dividers|ForEach-Object {"{0}:{1:0.0},{2:0.0},{3:0.0},{4:0.0}" -f $_.id,$_.bounds.x,$_.bounds.y,$_.bounds.width,$_.bounds.height}|Out-String}
function Append-SweepRow($Row){
    $stream=[IO.FileStream]::new((Join-Path $run 'sweep.jsonl'),[IO.FileMode]::Append,[IO.FileAccess]::Write,[IO.FileShare]::ReadWrite)
    $writer=[IO.StreamWriter]::new($stream,[Text.UTF8Encoding]::new($false))
    try{$writer.WriteLine(($Row|ConvertTo-Json -Depth 8 -Compress))}finally{$writer.Dispose()}
}
function Check-ReachSweep([string]$Theme){
    $rows=[Collections.Generic.List[object]]::new()
    foreach($scenario in $SweepScenarios){
        Start-Review $Theme ($Theme+'-'+$scenario)
        if($scenario -like 'photo*'){Use-Workspace 'builtin:workspace:photographer' 'Photo'}else{Use-PaintWorkspace}
        Settled
        if($scenario -eq 'photo-open'){
            $panel=((Layout).collapsed|Select-Object -First 1).groups.icons.panel|Select-Object -First 1
            Invoke-Id ('column-icon-'+$panel);Settled
        }
        if($scenario -eq 'photo-collapsed'){
            $layout=Layout;$strip=$layout.collapsed|Where-Object {!$_.open}|Select-Object -First 1
            $interior=$layout.dividers|Where-Object {$_.axis -eq 'horizontal' -and [Math]::Abs($_.bounds.x-$strip.bounds.x-$strip.bounds.width) -lt .5}|Select-Object -First 1
            if(!$interior){throw 'No interior divider beside the Photo strip'}
            $y=$strip.bounds.y+$strip.bounds.height*.5
            Swipe @{x=$interior.bounds.x+$interior.bounds.width*.5;y=$y} @{x=$layout.viewport[0]-2;y=$y} 20
            if(@((Layout).collapsed).Count -lt 2){throw 'The outer Photo column did not collapse'}
        }
        $baseline=(Model).state.workspace.layout|ConvertTo-Json -Depth 90 -Compress
        $geometry=Reach-Geometry
        $active=@{};foreach($group in (Layout).groups){$active[$group.id]=$group.active}
        $null=Gesture
        (Layout)|ConvertTo-Json -Depth 40|Set-Content (Join-Path $run ("sweep-$Theme-$scenario-layout.json"))
        $strips=@((Layout).collapsed.bounds)
        $samples=@(Reach-Samples|Sort-Object { $sample=$_;$inside=@($strips|Where-Object {$sample.x -ge $_.x -and $sample.x -le $_.x+$_.width -and $sample.y -ge $_.y -and $sample.y -le $_.y+$_.height});if($inside.Count){0}else{1} })
        foreach($sample in $samples){
            $script:sweepProbe=@{theme=$Theme;device=$Device;scenario=$scenario;sample=$sample;baseline=($baseline|ConvertFrom-Json);geometry=$geometry}
            @{theme=$Theme;device=$Device;scenario=$scenario;sample=$sample}|ConvertTo-Json -Depth 8|Set-Content (Join-Path $run 'sweep-current.json')
            $at=Screen $sample.x $sample.y
            if($Device -eq 'mouse'){[CapyRowPointer]::Hover($at.x,$at.y)}
            try{
                [CapyRowPointer]::Down($Device,$at.x,$at.y);Start-Sleep -Milliseconds 60
                $down=Gesture
                $eligible=$null -ne $down.edge -and $(if($sample.axis -eq 'horizontal'){$down.edge.contact -eq 'free'}else{[bool]$down.edge.vertical})
                $claimed=$false
                if($eligible){
                    $scale=[CapyRowPointer]::GetDpiForWindow($window)/96.
                    for($i=1;$i -le 10;$i++){[CapyRowPointer]::Move([int]($at.x+$sample.dx*$i*$scale),[int]($at.y+$sample.dy*$i*$scale));Start-Sleep -Milliseconds 16}
                    Start-Sleep -Milliseconds 180
                    $mid=Gesture;$claimed=[bool]$mid.edge.claimed
                }
                if($claimed){
                    [CapyRowPointer]::Key(0x1b)
                    Wait-Until {$ended=Gesture;!$ended.edge -and $ended.last_edge.phase -eq 'cancel' -and $ended.last_edge.claimed} 'Escape did not finish the claimed edge sweep contact'
                }
                if([CapyRowPointer]::Active){[CapyRowPointer]::Cancel()}
                Settled
                $after=Gesture
                if($after.edge){throw 'Canceled edge sweep retained a contact'}
                if((Reach-Geometry) -cne $geometry){throw 'Canceled edge sweep changed the arranged divider geometry'}
                if(!$eligible){
                    foreach($group in (Layout).groups){
                        if($active.ContainsKey($group.id) -and $group.active -ne $active[$group.id]){
                            $tab=Control ('panel-tab-'+$active[$group.id]) -Arranged
                            $tab.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
                        }
                    }
                    Settled
                }
                $restored=((Model).state.workspace.layout|ConvertTo-Json -Depth 90 -Compress) -ceq $baseline
                $row=[pscustomobject]@{theme=$Theme;scenario=$scenario;device=$Device;divider=$sample.divider;axis=$sample.axis;depth=$sample.depth;x=$sample.x;y=$sample.y;admitted=($null -ne $down.edge);contact=$down.edge.contact;eligible=$eligible;claimed=$claimed;lost=($eligible -and !$claimed);terminal=$after.last_edge;restored=$restored}
                $rows.Add($row);Append-SweepRow $row
                if($row.lost -or !$restored -or $after.edge){throw "Edge sweep lost or retained a contact: $($row|ConvertTo-Json -Depth 8 -Compress)"}
                if((Reach-Geometry) -cne $geometry){throw 'Canceled edge sweep changed the arranged divider geometry'}
            }finally{if([CapyRowPointer]::Active){[CapyRowPointer]::Cancel()}}
        }
        & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close
        [CapyRowPointer]::Dispose()
        if((Get-Item -LiteralPath $stderr).Length){throw "Native stderr requires inspection ($Theme, $scenario)"}
        $completed=@($rows|Where-Object scenario -eq $scenario)
        [pscustomobject]@{theme=$Theme;device=$Device;scenario=$scenario;samples=$completed.Count;admitted=@($completed|Where-Object admitted).Count;claimed=@($completed|Where-Object claimed).Count;lost=@($completed|Where-Object lost).Count;evidence=$run}|ConvertTo-Json
    }
    $rows|Export-Csv -NoTypeInformation -Path (Join-Path $run ("sweep-$Theme.csv"))
    [pscustomobject]@{theme=$Theme;device=$Device;scenarios=$SweepScenarios;samples=$rows.Count;admitted=@($rows|Where-Object admitted).Count;claimed=@($rows|Where-Object claimed).Count;lost=@($rows|Where-Object lost).Count;evidence=$run}|ConvertTo-Json
}
function Check-ColumnEdge([string]$Theme){
    $layout=Layout
    $edge=$layout.dividers|Where-Object {$_.axis -eq 'horizontal' -and $_.owner_side -eq 'left' -and !$_.fixed}|Sort-Object {$_.bounds.x}|Select-Object -Last 1
    if(!$edge){throw 'The left column has no resize edge'}
    $line=$edge.bounds.x+$edge.bounds.width*.5
    $group=$layout.groups|Where-Object {!$_.floating -and [Math]::Abs($_.bounds.x+$_.bounds.width-$edge.bounds.x) -lt 1}|Sort-Object {$_.bounds.height}|Select-Object -Last 1
    if(!$group){throw 'No docked group meets the left column edge'}
    $column=@($layout.groups|Where-Object {!$_.floating -and [Math]::Abs($_.bounds.x+$_.bounds.width-$edge.bounds.x) -lt 1})
    $subtools=@((Model).state.tool_set.subtools);$selected=$null
    for($i=0;$i -lt $subtools.Count;$i++){if($subtools[$i].selected){$selected=Find ('tool-subtool-'+$i);break}}
    $origin=Screen 0 0;$density=[CapyRowPointer]::GetDpiForWindow($window)/96.
    if(!$selected -or $selected.Current.IsOffscreen){throw 'SETUP: no arranged selected ToolSet subtool is available for the column-reach probe'}
    $bounds=$selected.Current.BoundingRectangle
    $selectedPoint=@{x=$edge.bounds.x-20;y=($bounds.Y+$bounds.Height*.5-$origin.y)/$density};$selectedScreen=Screen $selectedPoint.x $selectedPoint.y
    if(!$bounds.Contains([double]$selectedScreen.x,[double]$selectedScreen.y)){throw 'SETUP: the selected ToolSet subtool has no native point within the column reach'}
    $contact=Contact $selectedPoint
    @{id=$selected.Current.AutomationId;name=$selected.Current.Name;bounds=@($bounds.X,$bounds.Y,$bounds.Width,$bounds.Height);point=$selectedPoint;screen=$selectedScreen;edge=$contact}|ConvertTo-Json -Depth 8|Set-Content (Join-Path $run ('selected-subtool-edge-'+$Theme+'.json'))
    if($contact){throw 'The selected immediate-bubble ToolSet subtool yielded its contact to column resizing'}
    $inside={param($id)$x=(Divider $id).bounds.x-20;Reach-Point @(foreach($g in $column){for($y=$g.bounds.y+$g.bounds.height-24;$y -gt $g.bounds.y+48;$y-=24){@{x=$x;y=$y}}}) {param($edge)$edge.contact -eq 'free'}}
    $from=& $inside $edge.id
    Check-EdgeEscape $from
    if($EdgeEscapeOnly){return}
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
function Check-NumericEdge([string]$Theme){
    Use-PaintWorkspace;Settled
    $before=(Model).state.workspace.layout|ConvertTo-Json -Depth 90 -Compress
    $records=[Collections.Generic.List[object]]::new()
    foreach($id in @('number-caption-tool-setting-angle','tool-setting-angle','tool-setting-angle-slider','number-root-tool-setting-angle')){
        $bounds=(Control $id -Arranged).Current.BoundingRectangle
        $x=if($id -eq 'number-root-tool-setting-angle'){$bounds.Left+10}else{$bounds.Right-3}
        $y=if($id -eq 'number-root-tool-setting-angle'){$bounds.Bottom-8}else{$bounds.Bottom-3}
        try{
            [CapyRowPointer]::Down($Device,[int]$x,[int]$y);Start-Sleep -Milliseconds 160
            $down=Gesture
            for($i=1;$i -le 6;$i++){[CapyRowPointer]::Move([int]$x,[int]($y+90*$i/6));Start-Sleep -Milliseconds 20}
            Start-Sleep -Milliseconds 120;$moved=Gesture
            $changed=((Model).state.workspace.layout|ConvertTo-Json -Depth 90 -Compress) -cne $before
            if($Device -eq 'mouse'){[CapyRowPointer]::Key(0x1b);[CapyRowPointer]::Up()}else{[CapyRowPointer]::Cancel()}
            Settled
            $records.Add(@{id=$id;bounds=$bounds;point=@($x,$y);down=$down;moved=$moved;layout_changed=$changed;after=Gesture})
            $records|ConvertTo-Json -Depth 95|Set-Content (Join-Path $run ('numeric-edge-'+$Theme+'.json'))
            if($down.edge -or $moved.edge -or $changed){throw "$Theme $Device numeric component $id yielded to workspace resizing"}
            if(((Model).state.workspace.layout|ConvertTo-Json -Depth 90 -Compress) -cne $before){throw 'Numeric contact cancellation changed the workspace'}
        }finally{if([CapyRowPointer]::Active){[CapyRowPointer]::Cancel()}}
    }
}
function Check-ColumnHandleReset([string]$Theme){
    Use-PaintWorkspace;Settled
    $target=@((Layout).groups|Where-Object {$_.panels -contains 'toolbar'})[0]
    $sourceBand=@((Model).state.workspace.layout.bands|Where-Object {$_.edge -eq 'left' -and $_.root.id -ne $target.id})[0]
    $tab=(Control 'panel-tab-brushes' -Arranged).Current.BoundingRectangle
    [CapyRowPointer]::RightClick([int]($tab.X+$tab.Width*.5),[int]($tab.Y+$tab.Height*.5));Invoke 'Collapse column' -Name
    Wait-Until {$null -ne (Column $sourceBand.root.id)} 'The setup column did not collapse'
    $grip=(Control ('column-grip-'+$sourceBand.root.id) -Arranged).Current.BoundingRectangle
    [CapyRowPointer]::Down('mouse',[int]($grip.X+$grip.Width*.5),[int]($grip.Y+$grip.Height*.5))
    try{
        $to=Screen ($target.bounds.x+4) ($target.bounds.y+$target.bounds.height*.5)
        [CapyRowPointer]::Move($to.x,$to.y)
        Wait-Until {$drop=((Workspace-Root).Current.ItemStatus|ConvertFrom-Json).workspace_update.drag.drop_hint;(Gesture).phase -eq 'dragging' -and $drop.target.kind -eq 'split' -and $drop.target.group -eq $target.id -and $drop.target.edge -eq 'left'} 'The column grip did not preview a split beside the complete toolbar column'
        [CapyRowPointer]::Up()
    }finally{if([CapyRowPointer]::Active){[CapyRowPointer]::Cancel()}}
    Settled
    if(Column $sourceBand.root.id){
        $icon=(Control 'column-icon-brushes' -Arranged).Current.BoundingRectangle
        [CapyRowPointer]::RightClick([int]($icon.X+$icon.Width*.5),[int]($icon.Y+$icon.Height*.5));Invoke 'Expand column' -Name
        Settled
    }
    Wait-Until {@((Layout).dividers|Where-Object {$_.axis -eq 'horizontal' -and !$_.band -and $_.owner_side -eq 'left' -and !$_.fixed}).Count -gt 0} 'The native column move did not create a nested owned divider'
    $edge=@((Layout).dividers|Where-Object {$_.axis -eq 'horizontal' -and !$_.band -and $_.owner_side -eq 'left' -and !$_.fixed}|Sort-Object {$_.bounds.x})|Select-Object -First 1
    $groups=@((Layout).groups|Where-Object {!$_.floating -and [Math]::Abs($_.bounds.x+$_.bounds.width-$edge.bounds.x) -lt 1})
    $adjacent=@((Layout).groups|Where-Object {!$_.floating -and [Math]::Abs($_.bounds.x-$edge.bounds.x-$edge.bounds.width) -lt 1})
    if(!$groups.Count -or !$adjacent.Count){throw 'The nested divider has no adjacent column and toolbar groups'}
    $owner=@{divider=$edge.id;groups=@($groups.id)}
    $other=@{widths=@($adjacent|ForEach-Object {@{id=$_.id;width=$_.bounds.width}})}
    $canonical=Reach-Point @(foreach($id in $owner.groups){$g=@((Layout).groups|Where-Object id -eq $id)[0];foreach($fraction in .9,.6,.3){@{x=$edge.bounds.x-20;y=$g.bounds.y+$g.bounds.height*$fraction}}}) {param($edge)$edge.contact -eq 'free' -and !$edge.vertical}
    $point=Screen $canonical.x $canonical.y;[void][CapyRowPointer]::DoubleClick($Device,$point.x,$point.y);Settled
    $original=Width $owner.groups[0];$edge=Divider $owner.divider
    $handle=Control ('divider-'+$edge.id) -Arranged;$bounds=$handle.Current.BoundingRectangle
    $from=@{x=$edge.bounds.x+$edge.bounds.width*.5;y=$edge.bounds.y+$edge.bounds.height*.5}
    $at=Screen $from.x $from.y
    if($handle.Current.ProcessId -ne $review.Id -or !$bounds.Contains([double]$at.x,[double]$at.y) -or $edge.bounds.width -ne 6){throw 'The column reset contact is not on the actual six-unit owned divider'}
    $singleBefore=(Model).state.workspace.layout|ConvertTo-Json -Depth 90 -Compress
    $historyBefore=@((Model).state.commands|Where-Object {$_.id -match 'undo_workspace|redo_workspace'}|Select-Object id,enabled)|ConvertTo-Json -Compress
    [CapyRowPointer]::Down($Device,[int]($bounds.X+$bounds.Width*.5),[int]($bounds.Y+$bounds.Height*.25))
    [CapyRowPointer]::Up();Settled
    $historyAfter=@((Model).state.commands|Where-Object {$_.id -match 'undo_workspace|redo_workspace'}|Select-Object id,enabled)|ConvertTo-Json -Compress
    if((Width $owner.groups[0]) -ne $original -or ((Model).state.workspace.layout|ConvertTo-Json -Depth 90 -Compress) -cne $singleBefore -or $historyAfter -cne $historyBefore){throw 'One divider tap changed column width, layout or workspace history availability'}
    Swipe $from @{x=$from.x+64;y=$from.y}
    Wait-Until {[Math]::Abs((Width $owner.groups[0])-$original-64) -lt 1} "The left actual divider did not resize its owner by 64 units"
    $grown=Width $owner.groups[0];$grownLayout=(Model).state.workspace.layout|ConvertTo-Json -Depth 90 -Compress
    $handle=Control ('divider-'+$owner.divider) -Arranged;$bounds=$handle.Current.BoundingRectangle
    $point=@([int]($bounds.X+$bounds.Width*.5),[int]($bounds.Y+$bounds.Height*.5))
    $record=@{theme=$Theme;device=$Device;side='left';divider=$owner.divider;group_ids=$owner.groups;original_width=$original;grown_width=$grown;native_handle_bounds=@($bounds.X,$bounds.Y,$bounds.Width,$bounds.Height);double_press_point=$point;before_reset=(Model).state.workspace.layout;other_owner=$other;after_reset=$null;reset_width=$null}
    try{
        [void][CapyRowPointer]::DoubleClick($Device,$point[0],$point[1])
        Wait-Until {(Width $owner.groups[0]) -eq $original} "The left actual divider double press did not restore its exact preferred width ($original)"
        Settled
        foreach($width in $other.widths){if((Width $width.id) -ne $width.width){throw 'Resetting a divider changed the other column owner'}}
        $resetLayout=(Model).state.workspace.layout|ConvertTo-Json -Depth 90 -Compress
        WindowCommand 'undo_workspace';Settled
        if((Width $owner.groups[0]) -ne $grown -or ((Model).state.workspace.layout|ConvertTo-Json -Depth 90 -Compress) -cne $grownLayout){throw 'One workspace Undo did not restore the exact grown column'}
        WindowCommand 'redo_workspace';Settled
        if((Width $owner.groups[0]) -ne $original -or ((Model).state.workspace.layout|ConvertTo-Json -Depth 90 -Compress) -cne $resetLayout){throw 'One workspace Redo did not restore the exact preferred column width'}
    }finally{
        $record.after_reset=(Model).state.workspace.layout;$record.reset_width=Width $owner.groups[0]
        $record|ConvertTo-Json -Depth 95|Set-Content (Join-Path $run ('column-reset-'+$Theme+'.json'))
    }
}
function Check-CanvasReset([string]$Theme){
    if($Device -ne 'touch'){throw 'CanvasResetOnly requires touch input'}
    $drawingHistory={@{layers=(Model).state.layers.Count;commands=@((Model).state.commands|Where-Object {$_.id -in @('undo','redo')}|Select-Object id,enabled)}|ConvertTo-Json -Compress}
    $drawingBefore=& $drawingHistory
    $edge=(Layout).dividers|Where-Object {$_.axis -eq 'horizontal' -and $_.owner_side -eq 'left' -and !$_.fixed}|Sort-Object {$_.bounds.x}|Select-Object -Last 1
    if(!$edge){throw 'The left column has no canvas-side edge'}
    $groups=@((Layout).groups|Where-Object {!$_.floating -and [Math]::Abs($_.bounds.x+$_.bounds.width-$edge.bounds.x) -lt 1})
    $free=Reach-Point @(foreach($g in $groups){for($y=$g.bounds.y+$g.bounds.height-24;$y -gt $g.bounds.y+48;$y-=24){@{x=$edge.bounds.x-20;y=$y}}}) {param($contact)$contact.contact -eq 'free'}
    $at=Screen $free.x $free.y;[void][CapyRowPointer]::DoubleClick('touch',$at.x,$at.y);Settled
    $preferred=Width $groups[0].id
    $receipts=[Collections.Generic.List[object]]::new()
    foreach($distance in 20,35,40){
        $edge=Divider $edge.id
        $from=@{x=$edge.bounds.x+$edge.bounds.width*.5;y=$free.y}
        Swipe $from @{x=$from.x+64;y=$from.y}
        $grown=Width $groups[0].id
        if([Math]::Abs($grown-$preferred-64) -ge 1){throw 'Canvas reset setup did not grow the column by 64 units'}
        $grownLayout=(Model).state.workspace.layout|ConvertTo-Json -Depth 90 -Compress
        $edge=Divider $edge.id;$point=@{x=$edge.bounds.x+$edge.bounds.width+$distance;y=$free.y}
        $camera=(Model).state.camera;$scale=$camera.viewport[0]/(Layout).viewport[0]
        $area=@($camera.work_area|ForEach-Object {$_/$scale})
        if(!$area -or $point.x -lt $area[0] -or $point.x -ge $area[0]+$area[2] -or $point.y -lt $area[1] -or $point.y -ge $area[1]+$area[3]){throw 'The reset point is outside the published canvas work area'}
        $at=Screen $point.x $point.y
        $record=@{theme=$Theme;distance=$distance;preferred=$preferred;grown=$grown;point=$point;work_area=$area;before=(Model).state.workspace.layout;after=$null}
        try{
            if($distance -eq 40){
                [void][CapyRowPointer]::DoubleClick('touch',$at.x,$at.y);Settled
                if((Width $groups[0].id) -ne $grown){throw 'A touch double tap beyond 36 units reset the column'}
                WindowCommand 'undo_workspace';Settled
            }else{
                if($distance -eq 20){
                    try{
                        [CapyRowPointer]::Down('touch',$at.x,$at.y);[CapyRowPointer]::Up()
                        [CapyRowPointer]::Down('touch',$at.x,$at.y)
                        Wait-Until {[CapyRowPointer]::Active -and (Width $groups[0].id) -eq $preferred} 'Canvas-side double tap did not reset on the second press' 2
                        $record.second_down_width=Width $groups[0].id;$record.second_contact_held=[CapyRowPointer]::Active
                    }finally{[CapyRowPointer]::Up()}
                }else{[void][CapyRowPointer]::DoubleClick('touch',$at.x,$at.y)}
                Wait-Until {(Width $groups[0].id) -eq $preferred} "Canvas-side touch double tap at $distance units did not restore exact preferred width ($preferred)"
                Settled;$resetLayout=(Model).state.workspace.layout|ConvertTo-Json -Depth 90 -Compress
                WindowCommand 'undo_workspace';Settled
                if((Width $groups[0].id) -ne $grown -or ((Model).state.workspace.layout|ConvertTo-Json -Depth 90 -Compress) -cne $grownLayout){throw 'Canvas reset Undo did not restore the exact grown layout'}
                WindowCommand 'redo_workspace';Settled
                if((Width $groups[0].id) -ne $preferred -or ((Model).state.workspace.layout|ConvertTo-Json -Depth 90 -Compress) -cne $resetLayout){throw 'Canvas reset Redo did not restore the exact preferred layout'}
            }
        }finally{
            $record.after=(Model).state.workspace.layout;$record.width=Width $groups[0].id;$record.drawing_history_before=$drawingBefore;$record.drawing_history_after=& $drawingHistory;$receipts.Add($record)
            $receipts|ConvertTo-Json -Depth 95|Set-Content (Join-Path $run ('canvas-reset-'+$Theme+'.json'))
            if($record.drawing_history_after -cne $drawingBefore){throw 'Canvas reset changed drawing history or layer count'}
        }
    }
    $edge=Divider $edge.id;$from=@{x=$edge.bounds.x+$edge.bounds.width*.5;y=$free.y}
    Swipe $from @{x=$from.x+64;y=$from.y}
    $grownLayout=(Model).state.workspace.layout|ConvertTo-Json -Depth 90 -Compress
    $edge=Divider $edge.id;$at=Screen ($edge.bounds.x+$edge.bounds.width+20) $free.y
    try{
        [CapyRowPointer]::Down('touch',$at.x,$at.y);[CapyRowPointer]::Cancel()
        [CapyRowPointer]::Down('touch',$at.x,$at.y);[CapyRowPointer]::Up();Settled
        if(((Model).state.workspace.layout|ConvertTo-Json -Depth 90 -Compress) -cne $grownLayout){throw 'A canceled first canvas contact left a stale double-tap candidate'}
        $receipts.Add(@{theme=$Theme;cancel_first_then_single_tap='unchanged';width=Width $groups[0].id;layout=(Model).state.workspace.layout})
        $receipts|ConvertTo-Json -Depth 95|Set-Content (Join-Path $run ('canvas-reset-'+$Theme+'.json'))
    }finally{if([CapyRowPointer]::Active){[CapyRowPointer]::Cancel()}}
    WindowCommand 'undo_workspace';Settled
    if((Width $groups[0].id) -ne $preferred){throw 'Canceled-tap setup did not Undo to exact preferred width'}
    if((& $drawingHistory) -cne $drawingBefore){throw 'Canceled canvas contact changed drawing history or layer count'}
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
function Check-LayerRowEdge([string]$Theme){
    Use-PaintWorkspace;Settled
    $target=(Layout).groups|Where-Object {$_.panels -contains 'tool_settings'}|Select-Object -First 1
    $tab=(Control 'panel-tab-layers' -Arranged).Current.BoundingRectangle
    $to=Screen ($target.bounds.x+$target.bounds.width*.5) ($target.bounds.y+$target.bounds.height*.5)
    try{
        [CapyRowPointer]::Down('mouse',[int]($tab.X+$tab.Width*.5),[int]($tab.Y+$tab.Height*.5))
        [CapyRowPointer]::Move($to.x,$to.y)
        Wait-Until {$drop=((Workspace-Root).Current.ItemStatus|ConvertFrom-Json).workspace_update.drag.drop_hint;$drop.target.kind -eq 'tab' -and $drop.target.group -eq $target.id} 'Layers did not preview docking into the middle group'
        [CapyRowPointer]::Up();Settled
    }finally{if([CapyRowPointer]::Active){[CapyRowPointer]::Cancel()}}
    for($i=0;$i -lt 6;$i++){$count=(Model).state.layers.Count;Invoke 'layer-new';Wait-Until {(Model).state.layers.Count -eq $count+1} 'Layer row setup did not add a layer'}
    $tab=(Control 'panel-tab-layers' -Arranged).Current.BoundingRectangle
    [CapyRowPointer]::RightClick([int]($tab.X+$tab.Width*.5),[int]($tab.Y+$tab.Height*.5))
    Invoke (((Model).panels|Where-Object id -eq 'layers').configuration_title+'…') -Name
    Wait-Until {(Model).state.customization.expanded -eq 'layers'} 'Layer configuration did not open'
    (Control 'configure-show-layer_actions').GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
    Wait-Until {$control=@(((Model).panels|Where-Object id -eq 'layers').controls|Where-Object control -eq 'layer_actions');$control.Count -eq 1 -and !$control[0].visible_in_panel} 'Layer action footer did not become hidden'
    $dismiss=Control 'panel-tab-layers';$dismiss.SetFocus()
    Wait-Until {$dismiss.Current.HasKeyboardFocus} 'Layer tab did not receive keyboard focus before Escape'
    [CapyRowPointer]::Key([uint32]$review.Id,0x1b)
    Wait-Until {$null -eq (Model).state.customization.expanded} 'Layer configuration did not close'
    Settled
    $group=(Layout).groups|Where-Object {$_.panels -contains 'layers'}|Select-Object -First 1
    $gap=(Layout).dividers|Where-Object {$_.axis -eq 'vertical' -and !$_.fixed -and [Math]::Abs($_.bounds.y-$group.bounds.y-$group.bounds.height) -lt 1}|Select-Object -First 1
    if(!$gap){throw 'The docked Layers group has no lower row-resize gap'}
    $point=@{x=$group.bounds.x+$group.bounds.width*.65;y=$gap.bounds.y-12};$at=Screen $point.x $point.y
    $rows=$root.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition)
    $row=@($rows|Where-Object {$_.Current.AutomationId -match '^layer-row-[0-9]+$' -and $_.Current.BoundingRectangle.Contains([double]$at.x,[double]$at.y)})|Select-Object -First 1
    if(!$row){throw 'The near-gap contact does not lie inside an actual native LayerRow'}
    $contact=Contact $point
    if($contact.contact -ne 'pickup' -or !$contact.vertical){throw 'The actual LayerRow did not publish a vertical Pickup edge candidate'}
    $before=(Model).state.workspace.layout|ConvertTo-Json -Depth 90 -Compress
    Swipe $point @{x=$point.x-50;y=$point.y}
    if(((Model).state.workspace.layout|ConvertTo-Json -Depth 90 -Compress) -cne $before -or (Gesture).last_edge.claimed){throw 'A sideways LayerRow swipe resized the workspace'}
    Swipe $point @{x=$point.x;y=$point.y-50}
    if(!(Gesture).last_edge.claimed -or !(Gesture).last_edge.vertical -or [Math]::Abs((Divider $gap.id).bounds.y-$gap.bounds.y) -lt 20){throw 'A vertical LayerRow contact did not hand off to row resizing'}
    @{row=$row.Current.AutomationId;point=$point;contact=$contact;terminal=(Gesture).last_edge;before=$before;after=(Model).state.workspace.layout}|ConvertTo-Json -Depth 95|Set-Content (Join-Path $run ('layer-row-edge-'+$Theme+'.json'))
    WindowCommand 'undo_workspace';Settled
    if(((Model).state.workspace.layout|ConvertTo-Json -Depth 90 -Compress) -cne $before){throw 'LayerRow resize Undo did not restore the exact workspace'}
}
function Check-NativeEdgeCancel([string]$Theme){
    if($Device -eq 'mouse'){throw 'CancelOnly requires pen or touch native cancellation'}
    $edge=(Layout).dividers|Where-Object {$_.axis -eq 'horizontal' -and $_.owner_side -eq 'left' -and !$_.fixed}|Sort-Object {$_.bounds.x}|Select-Object -Last 1
    $groups=@((Layout).groups|Where-Object {!$_.floating -and [Math]::Abs($_.bounds.x+$_.bounds.width-$edge.bounds.x) -lt 1})
    $from=Reach-Point @(foreach($g in $groups){for($y=$g.bounds.y+$g.bounds.height-24;$y -gt $g.bounds.y+48;$y-=24){@{x=$edge.bounds.x-20;y=$y}}}) {param($contact)$contact.contact -eq 'free'}
    $before=(Model).state.workspace.layout|ConvertTo-Json -Depth 90 -Compress
    $records=[Collections.Generic.List[object]]::new()
    foreach($claimed in $false,$true){
        try{
            if($claimed){Swipe $from @{x=$from.x+64;y=$from.y} -During {Wait-Until {(Gesture).edge.claimed} 'Native edge did not claim before cancellation';[CapyRowPointer]::Cancel()}}
            else{$at=Screen $from.x $from.y;[CapyRowPointer]::Down($Device,$at.x,$at.y);Wait-Until {$null -ne (Gesture).edge -and !(Gesture).edge.claimed} 'Pending native edge was not published';[CapyRowPointer]::Cancel();Settled}
            $terminal=Gesture
            $records.Add(@{claimed=$claimed;gesture=$terminal;layout=(Model).state.workspace.layout})
            $records|ConvertTo-Json -Depth 95|Set-Content (Join-Path $run ('native-edge-cancel-'+$Theme+'.json'))
            if(((Model).state.workspace.layout|ConvertTo-Json -Depth 90 -Compress) -cne $before -or $terminal.edge -or $terminal.captured){throw 'Native pointer cancellation did not restore layout and release edge capture'}
        }finally{if([CapyRowPointer]::Active){[CapyRowPointer]::Cancel()}}
    }
}
function Check-ClosedStrip([string]$Theme){
    $strip=(Layout).collapsed|Where-Object open|Select-Object -First 1
    if(!$strip){throw 'Paint has no open stack member'}
    $panel=@($strip.groups.icons.panel|Where-Object {(Control ('column-icon-'+$_)).Current.ItemStatus -eq 'Selected'})[0]
    if(!$panel){throw 'The open stack member has no selected icon'}
    Invoke-Id ('column-icon-'+$panel);Wait-Until {!(Column $strip.id).open} "${Theme}: the open member did not close"
    $closed=Column $strip.id
    $icon=($closed.groups.icons|Where-Object panel -eq $panel|Select-Object -First 1).bounds
    $points=@(@{kind='tile';x=$icon.x+$icon.width*.5;y=$icon.y+$icon.height*.5})
    foreach($gap in @($closed.groups.divider|Where-Object {$_.height -gt 0})){$points+=@{kind='gap';x=$icon.x+$icon.width*.5;y=$gap.y+$gap.height*.5}}
    $bottom=($closed.groups.icons.bounds|Sort-Object {$_.y+$_.height}|Select-Object -Last 1)
    if($closed.content.y+$closed.content.height -gt $bottom.y+$bottom.height+12){$points+=@{kind='empty';x=$icon.x+$icon.width*.5;y=$bottom.y+$bottom.height+12}}
    if(!($points|Where-Object kind -eq 'gap') -or !($points|Where-Object kind -eq 'empty')){throw 'The closed strip fixture needs both a group gap and empty content space'}
    $outward=if($strip.bounds.x -gt (Layout).viewport[0]*.5){-90}else{90}
    $before=(Model).state.workspace.layout|ConvertTo-Json -Depth 90 -Compress
    foreach($from in $points){
        Swipe $from @{x=$from.x+$outward;y=$from.y} -During {
            try{Wait-Until {$column=Column $strip.id;!$column -or $column.open} "open"}catch{throw "${Theme}: an outward swipe from closed-strip $($from.kind) did not open it: $((Gesture)|ConvertTo-Json -Compress -Depth 5)"}
            if(!(Gesture).edge.claimed){throw "${Theme}: the closed strip opened without an edge claim"}
            [CapyRowPointer]::Key(0x1b)
        }
        if((Gesture).last_edge.claimed -ne $true){throw "${Theme}: the closed strip opened without an edge claim"}
        if(((Model).state.workspace.layout|ConvertTo-Json -Depth 90 -Compress) -cne $before){throw "${Theme}: canceling the $($from.kind) swipe did not restore the closed strip"}
    }
}
try {
    Enter-CapyEnvironment
    $env:CAPY_TRACE_UI='1'
    $themes=if($Theme){@($Theme)}else{@('dark','light')}
    foreach($theme in $themes){
        if($SweepOnly){Check-ReachSweep $theme;continue}
        Start-Review $theme
        Settled
        if($NumericOnly){Check-NumericEdge $theme}elseif($EdgeEscapeOnly){Check-ColumnEdge $theme}elseif($CancelOnly){Check-NativeEdgeCancel $theme}elseif($LayerRowOnly){Check-LayerRowEdge $theme}elseif($ClosedStripOnly){Check-ClosedStrip $theme}elseif($CanvasResetOnly){Check-CanvasReset $theme}elseif($ColumnResetOnly){Check-ColumnHandleReset $theme}else{
            Check-ColumnEdge $theme
            Check-GroupGap $theme
            Check-ClosedStrip $theme
            if($Device -eq 'touch'){Check-LayerRowEdge $theme}
        }
        Capture ("edge-reach-"+$theme)
        & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close
        if((Get-Item -LiteralPath $stderr).Length){throw "Native stderr requires inspection ($theme)"}
        [CapyRowPointer]::Dispose()
    }
    if($SweepOnly){[pscustomobject]@{device=$Device;themes=($themes -join ',');reach_sweep='passed';evidence=$run}|ConvertTo-Json}
    elseif($NumericOnly){[pscustomobject]@{device=$Device;themes=($themes -join ',');numeric_edge_reservation='passed';evidence=$run;scope='Native NumberControl caption, readout, slider and component gap only'}|ConvertTo-Json}
    elseif($EdgeEscapeOnly){[pscustomobject]@{device=$Device;themes=($themes -join ',');edge_escape='passed';evidence=$run;scope='Pending and claimed edge Escape only'}|ConvertTo-Json}
    elseif($CancelOnly){[pscustomobject]@{device=$Device;themes=($themes -join ',');native_edge_cancel='passed';evidence=$run;scope='Pending and claimed native edge cancellation only'}|ConvertTo-Json}
    elseif($LayerRowOnly){[pscustomobject]@{device=$Device;themes=($themes -join ',');layer_row_edge_handoff='passed';evidence=$run;scope='Actual LayerRow Pickup edge arbitration only'}|ConvertTo-Json}
    elseif($ClosedStripOnly){[pscustomobject]@{device=$Device;themes=($themes -join ',');closed_strip='passed';evidence=$run;scope='Native closed-column outward swipe only'}|ConvertTo-Json}
    elseif($CanvasResetOnly){[pscustomobject]@{device=$Device;themes=($themes -join ',');canvas_side_reset='passed';preferred_width='exact';workspace_undo_redo='exact';evidence=$run;scope='Native touch canvas-side double taps only; physical shell gestures remain separate'}|ConvertTo-Json}
    elseif($ColumnResetOnly){[pscustomobject]@{device=$Device;themes=($themes -join ',');actual_nested_divider_reset='passed';owner_side='left';preferred_width='exact';other_owner='unchanged';workspace_undo_redo='exact';evidence=$run;scope='Actual native nested divider contacts only; physical devices and timing remain separate'}|ConvertTo-Json}
    else{[pscustomobject]@{device=$Device;column_edge_from_inside='passed';resize_pointer=($Device -eq 'mouse');double_press_reset='passed';canvas_contact=$(if($Device -eq 'touch'){'grabs'}else{'kept'});vertical_swipe_kept='passed';group_gap='passed';closed_strip='passed';themes=($themes -join ',');scope='OS-delivered synthetic input; physical devices and timing remain separate'}|ConvertTo-Json}
}catch{
    try{@{gesture=Gesture;brush=(Model).state.brush;tool_set=(Model).state.tool_set;workspace=(Model).state.workspace.layout;sweep=$script:sweepProbe;geometry=$(if($SweepOnly){Reach-Geometry})}|ConvertTo-Json -Depth 95|Set-Content (Join-Path $run 'failure-native.json')}catch{}
    if($review -and !$review.HasExited){try{Capture 'failure'}catch{}}
    [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{
    [CapyRowPointer]::Dispose()
    if($review -and !$review.HasExited){& (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close}
    Exit-CapyEnvironment
}
