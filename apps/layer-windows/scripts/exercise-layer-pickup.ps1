param([Parameter(Mandatory)][string]$Executable,[ValidateSet('touch','pen','mouse')][string]$Device='touch',[ValidateSet('drawers')][string]$ColumnMode='drawers',[ValidateSet('dark','light')][string]$Theme='dark',[switch]$SwipeOnly,[switch]$PenSwipeOnly,[switch]$HoldOnly,[switch]$KeyboardContextOnly,[switch]$PresentationsOnly,[switch]$ClippedEdgesOnly,[switch]$PendingDropOnly,[string]$DropFixture,[string]$DropRun)
$ErrorActionPreference='Stop'
if($PenSwipeOnly){$SwipeOnly=$true;$Device='pen'}
if($SwipeOnly -and !$PenSwipeOnly -and !$PSBoundParameters.ContainsKey('Device')){
    foreach($device in @('touch','pen','mouse')){
        $options=@{Executable=$Executable;Device=$device;ColumnMode=$ColumnMode;SwipeOnly=$true;DropFixture=$DropFixture}
        if($PSBoundParameters.ContainsKey('Theme')){$options.Theme=$Theme}
        & $PSCommandPath @options
    }
    return
}
if(!$PSBoundParameters.ContainsKey('Theme')){
    foreach($theme in @('dark','light')){& $PSCommandPath -Executable $Executable -Device $Device -ColumnMode $ColumnMode -Theme $theme -SwipeOnly:$SwipeOnly -HoldOnly:$HoldOnly -KeyboardContextOnly:$KeyboardContextOnly -PresentationsOnly:$PresentationsOnly -ClippedEdgesOnly:$ClippedEdgesOnly -PendingDropOnly:$PendingDropOnly -DropFixture $DropFixture -DropRun $DropRun}
    return
}
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyFind='prefer-visible';$CapyPopups=$true
if(!('CapyRowPointer' -as [type])){Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')}
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=if($PendingDropOnly -and $DropRun){[IO.Path]::GetFullPath($DropRun)}else{Join-Path $repo ('artifacts/windows/layer-pickup/'+[Guid]::NewGuid().ToString('N'))}
[IO.Directory]::CreateDirectory($run)|Out-Null
$sourceReveals=[Collections.Generic.List[object]]::new()
$swipeStages=[Collections.Generic.List[object]]::new()
$pressContacts=[Collections.Generic.List[object]]::new()
$fixtureFailed=$false
function Gesture {try{(Find 'layer-list').Current.ItemStatus|ConvertFrom-Json}catch{}}
function Layer-Scrollbars {
    $viewport=Layer-Viewport
    $condition=[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::ScrollBar)
    @($root.FindAll([System.Windows.Automation.TreeScope]::Descendants,$condition)|Where-Object {
        $bounds=$_.Current.BoundingRectangle
        !$_.Current.IsOffscreen -and !$bounds.IsEmpty -and $bounds.Height -gt $bounds.Width -and $bounds.X -ge $viewport.Right-30 -and $bounds.X -le $viewport.Right+5 -and $bounds.Y -ge $viewport.Y-5 -and $bounds.Bottom -le $viewport.Bottom+5
    })
}
function Rows($Snapshot=(Model)) {
    @($Snapshot.state.layers|ForEach-Object {[ordered]@{id=$_.id;depth=$_.depth;visible=$_.visible;locked=$_.locked;mask_linked=$_.mask_linked;alpha_locked=$_.alpha_locked;pass_through=$_.pass_through;relationship=$_.relationship}})|ConvertTo-Json -Depth 8 -Compress
}
function Contact-State {
    $snapshot=Model;$raw=(Control 'layer-list').Current.ItemStatus
    $session=@($snapshot.windows_tabs.session_stamps|Where-Object id -eq $snapshot.windows_tabs.selected)[0]
    if(!$session){throw 'Contact state lacks its selected drawing checkpoint'}
    @{gesture_raw=$raw;gesture=($raw|ConvertFrom-Json);rows=(Rows $snapshot);selected=(@($snapshot.state.layers|Where-Object selected).id -join ',');editing_layer=$snapshot.state.layer_tools.editing_layer;revision=$snapshot.state.document_file.revision;checkpoint=$session.stamp.checkpoint;modified=$snapshot.state.document_file.modified}
}
function Record-Swipe([string]$Stage) {
    $swipeStages.Add(@{stage=$Stage;state=(Contact-State);delete_visible=($null -ne (Find "layer-$created-swipe-delete" -Visible))})
}
function Dismiss {
    [CapyRowPointer]::Key(0x1b)
    Wait-Until {!(Gesture).menu_open} 'Layer context menu did not close'
}
function Menu-State($Item) {
    try{$c=$Item.Current;@{id=$c.AutomationId;runtime_id=@($Item.GetRuntimeId());bounds=$c.BoundingRectangle.ToString();offscreen=$c.IsOffscreen;focused=$c.HasKeyboardFocus;enabled=$c.IsEnabled;process=$c.ProcessId}}catch{@{error=$_.Exception.Message}}
}
function Layer-History([string]$Name) {Invoke $Name -Name}
function Tap-RowBody([long]$Layer){
    $box=(Control "layer-row-$Layer" -Arranged).Current.BoundingRectangle
    $inset=8*[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
    [CapyRowPointer]::Down($Device,[int]($box.Left+$inset),[int]($box.Top+$box.Height/2));[CapyRowPointer]::Up()
}
function Layer-Viewport($Container=(Control 'layer-list' -Arranged)) {
    $list=$Container.Current.BoundingRectangle
    $header=Find 'layer-controls' -Visible;$footer=Find 'layer-footer' -Visible
    if(!$header -or !$footer){return $list}
    $header=$header.Current.BoundingRectangle;$footer=$footer.Current.BoundingRectangle
    $padding=[Math]::Max(0,$list.Top-$header.Bottom)
    if($footer.Top-$padding -le $list.Top -or $list.Width -le 0){throw 'Layers native viewport anchors are invalid'}
    [Windows.Rect]::new($list.X,$list.Top,$list.Width,($footer.Top-$padding-$list.Top))
}
function Undo-Redo([string]$Before,[string]$After) {
    Layer-History 'Undo';Wait-Until {(Rows) -eq $Before} 'One Undo did not restore the original layer order'
    Layer-History 'Redo';Wait-Until {(Rows) -eq $After} 'One Redo did not restore the completed layer move'
    Layer-History 'Undo';Wait-Until {(Rows) -eq $Before} 'Final Undo did not restore the fixture'
}
function Verify-PendingDropFixture {
    if(!$DropFixture){throw 'Build the LayerDrop control fixture and pass -DropFixture for deterministic final-drop coverage'}
    $fixture=(Resolve-Path -LiteralPath $DropFixture).Path
    if($fixture -eq $Executable){throw 'DropFixture must be the separate LayerDrop control executable'}
    $shell=[Diagnostics.Process]::GetCurrentProcess().MainModule.FileName
    $parentContext=if($env:CAPY_FIXTURE_CONTEXT -and (Test-Path -LiteralPath $env:CAPY_FIXTURE_CONTEXT)){Get-Content -LiteralPath $env:CAPY_FIXTURE_CONTEXT -Raw|ConvertFrom-Json}
    try {
        & $shell -NoProfile -ExecutionPolicy Bypass -File $PSCommandPath -Executable $fixture -Device $Device -Theme $Theme -PendingDropOnly -DropRun (Join-Path $run 'pending-drop')
        $childExit=$LASTEXITCODE
    }finally{
        if($parentContext){
            $childContext=Get-Content -LiteralPath $env:CAPY_FIXTURE_CONTEXT -Raw|ConvertFrom-Json
            Update-CapyFixtureContext ({
                param($context)
                $context.Clear()
                foreach($property in $parentContext.PSObject.Properties){$context[$property.Name]=$property.Value}
                $context.windows=@($parentContext.windows|Where-Object {$null -ne $_})+@($childContext.windows|Where-Object {$null -ne $_})
                $context.closes=@($parentContext.closes|Where-Object {$null -ne $_})+@($childContext.closes|Where-Object {$null -ne $_})
            }.GetNewClosure())
        }
    }
    if($childExit -ne 0){throw "Deterministic final-drop fixture exited $childExit"}
}
function Reveal([string]$Id) {
    if($Id -notmatch '^layer-(?:row-)?\d+(?:-|$)'){return}
    $attempts=[Collections.Generic.List[object]]::new();$revealed=$false
    try{
        $list=Control 'layer-list' -Arranged;$scroll=Scroll-Pattern $list
        $probe={
            $viewport=Layer-Viewport $list;$current=$scroll.Current;$item=Find $Id -Within $list
            $sample=@{percent=$current.VerticalScrollPercent;view_size=$current.VerticalViewSize;viewport=$viewport.ToString();source=$null;visible_hit=$false}
            if($item){
                $native=$item.Current;$bounds=$native.BoundingRectangle
                $sample.source=@{bounds=$bounds.ToString();offscreen=$native.IsOffscreen;point=$null}
                if(!$bounds.IsEmpty -and $bounds.Width -gt 0 -and $bounds.Height -gt 0 -and @(@($bounds.X,$bounds.Y,$bounds.Width,$bounds.Height)|Where-Object {[double]::IsNaN($_) -or [double]::IsInfinity($_)}).Count -eq 0){
                    $point=[Windows.Point]::new($(if($Id -like 'layer-row-*'){$bounds.X+2}else{$bounds.X+$bounds.Width*.5}),($bounds.Y+$bounds.Height*.5))
                    $sample.source.point=@($point.X,$point.Y)
                    $sample.visible_hit=!$native.IsOffscreen -and !$viewport.IsEmpty -and $viewport.Contains($point)
                }
            }
            $attempts.Add($sample);$sample.visible_hit
        }
        if(& $probe){$revealed=$true;return}
        if($scroll.Current.VerticallyScrollable){
            $scroll.SetScrollPercent(-1,0);$clock=[Diagnostics.Stopwatch]::StartNew()
            do{
                Start-Sleep -Milliseconds 150
                if(& $probe){$revealed=$true;return}
                if($scroll.Current.VerticalScrollPercent -ge 100){break}
                $scroll.Scroll([System.Windows.Automation.ScrollAmount]::NoAmount,[System.Windows.Automation.ScrollAmount]::LargeIncrement)
            }while($clock.Elapsed.TotalSeconds -lt $script:CapyWaitSeconds)
        }
        throw "Source $Id could not be revealed within its native layer viewport"
    }finally{
        $sourceReveals.Add(@{source=$Id;case=$script:case;revealed=$revealed;attempts=@($attempts.ToArray())})
    }
}
function Point([string]$Id,[switch]$Name) {
    if(!$Name){Reveal $Id}
    $bounds=(Control $Id -Name:$Name -Arranged).Current.BoundingRectangle
    $row=$Id -like 'layer-row-*'
    $x=if($row){$bounds.X+2}else{$bounds.X+$bounds.Width*.5}
    $y=$bounds.Y+$bounds.Height*.5
    @{x=[int]$x;y=[int]$y}
}
function Move-To($At) {[CapyRowPointer]::Move([int]$At.x,[int]$At.y)}
function Tap([string]$Id) {
    $at=Point $Id
    [CapyRowPointer]::Down($Device,$at.x,$at.y)
    Start-Sleep -Milliseconds 35
    [CapyRowPointer]::Up()
}

function Focus-Key([string]$Id,[uint16]$Key) {
    Wait-Until {
        $at=Point $Id
        $target=Control $Id
        $target.SetFocus()
        if(!$target.Current.HasKeyboardFocus){return $false}
        try{
            [CapyRowPointer]::KeyAt($Key,$at.x,$at.y)
            $true
        }catch{
            # Input-pane animations can move the HWND between UIA measurement
            # and its hit test. This rejection happens before any key is sent.
            if($_.Exception.InnerException.Message -eq 'Input point is outside the owned review.'){$false}else{throw}
        }
    } 'Native text-input layout did not settle on an owned control'
}
function Destination([double]$Layer,[double]$Fraction=.6) {
    $null=Point "layer-row-$Layer"
    $bounds=(Control "layer-row-$Layer").Current.BoundingRectangle
    @{x=[int]($bounds.X+$bounds.Width*.5);y=[int]($bounds.Y+$bounds.Height*$Fraction)}
}
function Press([string]$Id,[double]$Layer,[switch]$Grip) {
    $at=Point $Id
    $nativeInput=Input-Trace $at.x $at.y
    $script:press=@{case=$script:case;source=$Id;point=$at;intended_bounds=(Control $Id).Current.BoundingRectangle;foreground=$nativeInput.foreground;owned_window=$nativeInput.owned_window;hit=$nativeInput.hit}
    $pressContacts.Add($script:press)
    $admission=@{state=$null}
    [CapyRowPointer]::Down($Device,$at.x,$at.y)
    Wait-Until {$admission.state=Gesture;$g=$admission.state;$g.source -eq $Layer -and $g.phase -in @('pressed','held')} 'Layer row did not receive the contact'
    $g=$admission.state;$script:press.admitted_snapshot=$g
    $script:press.expected_classification=@{device=$Device;grip=[bool]$Grip;mask=($Id -eq "layer-$Layer-mask")}
    if($g.device -ne $Device -or $g.grip -ne [bool]$Grip -or $g.mask -ne ($Id -eq "layer-$Layer-mask")){throw 'Layer source or pointer device was misclassified'}
    if(!$Grip -and $g.phase -eq 'pressed' -and $g.scroll_claimed){throw 'Pending row hold stole scrolling'}
}
function Pick-Up([string]$Id,[double]$Layer,[switch]$Grip) {
    Press $Id $Layer -Grip:$Grip
    if($Device -ne 'mouse' -and !$Grip){
        $hold=@{state=$null}
        Wait-Until {$hold.state=Gesture;$g=$hold.state;$g.phase -eq 'held' -and $g.menu_open} 'Native pen/touch row hold did not open its menu' 4
        $script:press.held_snapshot=$hold.state
        if(!$hold.state.captured){throw 'Held row did not retain contact capture'}
    }
}
function Drag-PanelEdge([double]$Layer,[ValidateSet('before','after')][string]$Edge){
    $before=Rows;$at=Point "layer-$Layer-drag"
    $bounds=if($Edge -eq 'before'){(Control 'layer-controls' -Arranged).Current.BoundingRectangle}else{Layer-Viewport}
    $to=@{x=[int]($bounds.Left+$bounds.Width/2);y=[int]$(if($Edge -eq 'before'){$bounds.Top+$bounds.Height/2}else{$bounds.Bottom-2})}
    [CapyRowPointer]::Down($Device,$at.x,$at.y);Move-To @{x=$at.x-30;y=$at.y};Move-To $to
    Wait-Until {$g=Gesture;$g.phase -eq 'dragging' -and $g.can_drop} "Panel $Edge edge did not admit a drop"
    [CapyRowPointer]::Up();Wait-Until {(Gesture).phase -eq 'idle' -and (Rows) -ne $before} "Panel $Edge edge drop did not commit"
    $layers=@((Model).state.layers)
    $actual=if($Edge -eq 'before'){$layers[0].id}else{$layers[$layers.Count-2].id}
    if($actual -ne $Layer){throw "Panel $Edge edge placed layer $Layer at the wrong end"}
    Layer-History 'Undo';Wait-Until {(Rows) -eq $before} 'Panel edge drop was not one undo'
}
function Visible-LayerRows{
    $viewport=Layer-Viewport
    @((Model).state.layers|ForEach-Object {
        $row=Find ("layer-row-"+$_.id)
        if($row){$box=$row.Current.BoundingRectangle;if($box.Height -gt 0 -and $box.Bottom -gt $viewport.Top -and $box.Top -lt $viewport.Bottom){@{id=$_.id;bounds=$box}}}
    }|Sort-Object {$_.bounds.Top})
}
function Check-ClippedEdges{
    for($i=0;$i -lt 16;$i++){$count=(Model).state.layers.Count;Invoke 'layer-new';Wait-Until {(Model).state.layers.Count -eq $count+1} 'Clipped-edge layer did not appear'}
    $scroll=Scroll-Pattern (Control 'layer-list' -Arranged)
    if(!$scroll.Current.VerticallyScrollable){throw 'Clipped-edge fixture did not overflow Layers'}
    foreach($edge in 'before','after'){
        $script:case='clipped-'+$edge;$scroll.SetScrollPercent(-1,50)
        Wait-Until {[Math]::Abs($scroll.Current.VerticalScrollPercent-50) -lt .1} 'Clipped-edge list did not reach its middle'
        $viewport=Layer-Viewport;$visible=Visible-LayerRows;$row=if($edge -eq 'before'){$visible[0]}else{$visible[-1]}
        $height=($visible|ForEach-Object {$_.bounds.Height}|Measure-Object -Maximum).Maximum
        $overflow=if($edge -eq 'before'){$viewport.Top-$row.bounds.Top}else{$row.bounds.Bottom-$viewport.Bottom}
        $clipped=[Math]::Max($overflow,$height-$row.bounds.Height)
        $range=$viewport.Height*(100/$scroll.Current.VerticalViewSize-1)
        $direction=if($edge -eq 'before'){1}else{-1}
        $percent=50+$direction*($height*.8-$clipped)/$range*100
        $scroll.SetScrollPercent(-1,$percent)
        $admitted=@{row=$null;visible=$null;viewport=$null}
        Wait-Until {
            $admitted.viewport=Layer-Viewport;$admitted.visible=Visible-LayerRows
            $admitted.row=if($edge -eq 'before'){$admitted.visible[0]}else{$admitted.visible[-1]}
            $overflow=if($edge -eq 'before'){$admitted.viewport.Top-$admitted.row.bounds.Top}else{$admitted.row.bounds.Bottom-$admitted.viewport.Bottom}
            $clipped=[Math]::Max($overflow,$height-$admitted.row.bounds.Height)
            [Math]::Abs($scroll.Current.VerticalScrollPercent-$percent) -lt .1 -and $clipped -gt $height*.65 -and $clipped -lt $height*.95
        } 'Clipped-edge fixture did not expose a partially clipped boundary row'
        $viewport=$admitted.viewport;$target=$admitted.row;$next=$null
        if($edge -eq 'after'){
            $layers=@((Model).state.layers);$index=[Array]::IndexOf([int64[]]$layers.id,[int64]$target.id)
            if($index -ge 0 -and $index+1 -lt $layers.Count -and $layers[$index+1].depth -eq $layers[$index].depth){$next=$layers[$index+1].id}
        }
        $source=@($admitted.visible|Where-Object {$_.id -ne $target.id -and $_.bounds.Top -ge $viewport.Top -and $_.bounds.Bottom -le $viewport.Bottom -and [Math]::Abs($_.bounds.Height-$height) -lt .5}|Sort-Object {[Math]::Min($_.bounds.Top-$viewport.Top,$viewport.Bottom-$_.bounds.Bottom)} -Descending)[0]
        if(!$source){throw 'Clipped-edge fixture has no interior source row'}
        $before=Rows;$at=Point ("layer-"+$source.id+'-drag')
        [CapyRowPointer]::Down($Device,$at.x,$at.y);Move-To @{x=$at.x-30;y=$at.y}
        Wait-Until {(Gesture).phase -eq 'dragging'} 'Clipped-edge handle did not begin dragging'
        $outside=if($edge -eq 'before'){$viewport.Top-1}else{$viewport.Bottom+1}
        [CapyRowPointer]::Move([int]($viewport.Left+$viewport.Width*.5),[int]$outside);[CapyRowPointer]::Up()
        Wait-Until {$g=Gesture;$g.phase -eq 'idle' -and $g.last_release.commit} 'Clipped-edge drop did not commit'
        $released=(Gesture).last_release;$position=if($edge -eq 'before'){'above'}else{'below'}
        $boundary=($released.target -eq $target.id -and $released.position -eq $position) -or ($edge -eq 'after' -and $null -ne $next -and $released.target -eq $next -and $released.position -eq 'above')
        if(!$boundary -or $released.surface -ne 'row'){throw 'A clipped row intercepted the outside list-edge drop'}
        @{edge=$edge;source=$source.id;target=$target;viewport=$viewport;release=$released}|ConvertTo-Json -Depth 8|Set-Content (Join-Path $run ('clipped-'+$edge+'.json'))
        Capture ('clipped-'+$edge)
        Layer-History 'Undo';Wait-Until {(Rows) -eq $before} 'Clipped-edge drop was not one Undo'
    }
}
function Drag-Row([string]$Id,[double]$Layer,[double]$Target,[switch]$Grip,[switch]$Cancel,[double]$Fraction=.6,[switch]$PendingDropGate) {
    $before=Rows
    $to=Destination $Target $Fraction
    if($Device -ne 'mouse' -and !$Grip){
        Pick-Up $Id $Layer
        Move-To $to
        Start-Sleep -Milliseconds 150
        if((Gesture).phase -eq 'dragging' -or (Rows) -ne $before){throw 'Held pen/touch row movement reordered without its handle'}
        [CapyRowPointer]::Up()
        $finished=@{state=$null}
        Wait-Until {$finished.state=Gesture;$finished.state.phase -eq 'idle'} 'Held row movement retained a contact'
        if($finished.state.menu_open){Dismiss}
        $Id="layer-$Layer-drag";$Grip=[switch]::new($true)
        $to=Destination $Target $Fraction
    }
    $at=Point $Id
    $to=Destination $Target $Fraction
    $nextPress=if($PendingDropGate){Point "layer-$Target-drag"}
    $clientOrigin=[CapyRowPointer+Point]::new()
    [CapyRowPointer]::ClientToScreen($drawingWindow,[ref]$clientOrigin)|Out-Null
    $script:contact=@{source=$Id;target=$Target;from=$at;to=$to;source_bounds=(Control $Id).Current.BoundingRectangle;target_bounds=(Control "layer-row-$Target").Current.BoundingRectangle;viewport_bounds=(Layer-Viewport);window_bounds=$root.Current.BoundingRectangle;client_origin=@{x=$clientOrigin.x;y=$clientOrigin.y};dpi=[CapyRowPointer]::GetDpiForWindow($drawingWindow)}
    if($Grip){
        $at=$script:contact.from
        # No UIA wait may accidentally satisfy the hold timeout.
        [CapyRowPointer]::Down($Device,$at.x,$at.y);Start-Sleep -Milliseconds 35
        Move-To $to
    }else{
        Pick-Up $Id $Layer
        Move-To $to
    }
    $layers=@((Model).state.layers);$index=[Array]::IndexOf([int64[]]$layers.id,[int64]$Target)
    $next=if($index -ge 0 -and $index+1 -lt $layers.Count -and $layers[$index+1].depth -eq $layers[$index].depth){$layers[$index+1].id}
    $drop=@{state=$null}
    Wait-Until {
        $drop.state=Gesture;$g=$drop.state
        $g.phase -eq 'dragging' -and $g.can_drop -and ($g.target -eq $Target -or ($Fraction -gt .5 -and $null -ne $next -and $g.target -eq $next -and $g.position -eq 'above'))
    } 'Layer move did not reach a validated drop target'
    $admitted=$drop.state;$script:contact.admitted_snapshot=$admitted
    if($admitted.source -ne $Layer -or $admitted.device -ne $Device -or $admitted.grip -ne [bool]$Grip){throw 'The admitted drag used another source or device'}
    if($admitted.menu_open){throw 'Dragging retained the held context menu'}
    $editing=(Model).state.layer_tools.editing_layer
    $selected=@((Model).state.layers|Where-Object id -eq $Layer)[0].selected
    if($PendingDropGate){
        Invoke 'layer-drop-arm'
        Wait-Until {(Control 'layer-drop-arm').Current.ItemStatus -eq 'armed'} 'Final drop gate did not arm'
    }
    if($Cancel){
        if($Device -eq 'mouse'){[CapyRowPointer]::Key(0x1b);[CapyRowPointer]::Up()}else{[CapyRowPointer]::Cancel()}
    }else{
        if($PendingDropGate){
            [CapyRowPointer]::Up()
            Wait-Until {(Control 'layer-drop-arm').Current.ItemStatus -eq 'held' -and (Gesture).phase -eq 'finishing'} 'Final drop query was not held after release'
            [CapyRowPointer]::Down($Device,$nextPress.x,$nextPress.y)
            Wait-Until {(Gesture).last_ignored_down.generation -eq $admitted.generation} 'The next press did not reach the held final drop'
            [CapyRowPointer]::Up()
            Invoke 'layer-drop-release'
        }
        else{[CapyRowPointer]::Up()}
    }
    $finished=@{state=$null}
    Wait-Until {$finished.state=Gesture;$finished.state.phase -eq 'idle'} 'Layer contact did not finish'
    if($PendingDropGate){
        $script:contact.ignored_after_release=$finished.state.last_ignored_down
        if($finished.state.last_ignored_down.generation -ne $admitted.generation -or $finished.state.last_ignored_down.reason -ne 'drop_pending'){throw 'The next press did not overlap the pending final drop query'}
    }
    # Ordinary drawers retain their row surface during the closing animation.
    if($Cancel){
        if((Rows) -ne $before){throw 'Cancelled row move changed layers'}
        if($finished.state.menu_open){throw 'Cancelled row move retained its menu'}
    }else{
        Wait-Until {(Rows) -ne $before} 'Completed layer move did not change order'
        $after=Rows
        $current=(Model).state.layer_tools.editing_layer
        if($current.id -ne $editing.id -or $current.mask_selected -ne $editing.mask_selected -or
            @((Model).state.layers|Where-Object id -eq $Layer)[0].selected -ne $selected){throw 'Drop activated a child control or changed the drawing target'}
        if(!$finished.state.last_release.commit){throw 'Released layer move was not accepted'}
        Undo-Redo $before $after
    }
}
function Check-KeyboardContext([long]$Layer) {
    $script:case='keyboard-context'
    Focus-Key "layer-$Layer-name" 0x5d
    Wait-Until {(Gesture).menu_open} 'Keyboard context action did not open the layer menu'
    $beforeMenuRows=Rows;$beforeMenuRevision=(Model).state.document_file.revision
    $source=Control "layer-$Layer-name";$organize=Control 'menu-organize' -Arranged
    Tap 'menu-organize'
    $duplicate=Control 'layer-menu-duplicate' -Arranged
    $submenu=[System.Windows.Automation.TreeWalker]::ControlViewWalker.GetParent($duplicate)
    $menuItems=$submenu.FindAll([System.Windows.Automation.TreeScope]::Children,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::MenuItem))
    $nested=@{device=$Device;before=@{source=(Menu-State $source);parent=(Menu-State $organize);child=(Menu-State $duplicate);items=@($menuItems|ForEach-Object {Menu-State $_})};rows=$beforeMenuRows;revision=$beforeMenuRevision}
    try{
        Wait-Until {@($menuItems|Where-Object {$_.Current.HasKeyboardFocus -and !$_.Current.IsOffscreen}).Count -gt 0} 'Native Organize submenu did not acquire keyboard focus'
        [CapyRowPointer]::Key(0x1b)
        $escape=@{polls=0}
        Wait-Until {
            $matching=$false
            try{$child=Find 'layer-menu-duplicate';$matching=(!$child -or $child.Current.IsOffscreen) -and !$organize.Current.IsOffscreen -and $organize.Current.HasKeyboardFocus -and (Gesture).menu_open}catch [System.Windows.Automation.ElementNotAvailableException]{}
            if($matching){$escape.polls++}else{$escape.polls=0}
            $escape.polls -ge 2
        } 'First Escape did not close only the native Organize submenu'
        $nested.after_first=@{parent=(Menu-State $organize);child=(Menu-State $duplicate);source=(Menu-State $source);gesture=(Gesture)}
        Dismiss
        Wait-Until {$parent=Find 'menu-organize';!$parent -or $parent.Current.IsOffscreen} 'Second Escape did not dismiss the native parent menu'
        if((Rows) -ne $beforeMenuRows -or (Model).state.document_file.revision -ne $beforeMenuRevision){throw 'Nested menu cancellation changed layer state or history'}
    }finally{
        $nested.after_second=@{parent=(Menu-State $organize);child=(Menu-State $duplicate);source=(Menu-State $source);gesture=(Gesture);rows=(Rows);revision=(Model).state.document_file.revision}
        $nested|ConvertTo-Json -Depth 15|Set-Content (Join-Path $run 'nested-menu-escape.json')
    }
    if($Device -eq 'mouse'){
        $script:case='secondary-context'
        $at=Point "layer-$Layer-name"
        [CapyRowPointer]::RightClick($at.x,$at.y)
        Wait-Until {(Gesture).menu_open} 'Secondary click did not open the layer menu'
        Dismiss
    }
}
function Check-Presentations([long]$created,[long]$paint) {
    # Reuse the same row controller after native panel reparenting and drawer
    # creation. Workspace history is independent of each document reorder.
    # Verify collapse/expand before taking the history baseline. Both
    # presentations then exercise the same row controller and one-step Undo.
    Focus-Key 'panel-tab-layers' 0x5d;Invoke 'Collapse column' -Name
    Start-Sleep -Milliseconds 300
    $column=@((Model).layout.collapsed|Where-Object {($_.groups.icons.panel) -contains 'layers'})[0].id
    $at=Point 'column-icon-layers';[CapyRowPointer]::RightClick($at.x,$at.y)
    $individual=(Control 'Open individual panels' -Name).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern)
    if($individual.Current.ToggleState -ne [System.Windows.Automation.ToggleState]::On){
        $individual.Toggle()
        Wait-Until {@((Model).state.workspace.layout.column_stacks|Where-Object column -eq $column)[0].drawers} 'Individual panel drawers did not enable'
        $at=Point 'column-icon-layers';[CapyRowPointer]::RightClick($at.x,$at.y)
    }
    Invoke 'Expand column' -Name
    Wait-Until {$null -ne (Find 'panel-tab-layers')} 'Column mode setup did not restore Layers'
    $workspaceBefore=(Model).state.workspace|ConvertTo-Json -Depth 80 -Compress
    @{layout=(Model).layout;workspace=(Model).state.workspace}|ConvertTo-Json -Depth 90|Set-Content (Join-Path $run 'presentations-before.json')
    foreach($presentation in @('floating','drawer')){
        $script:case="$presentation-setup"
        if($presentation -eq 'floating'){
            $at=Point 'panel-tab-layers';$to=Point 'drawing-canvas'
            # Arrange the host with mouse input; each row below is tested with
            # the requested device. Tab tear-off has its own acceptance fixture.
            [CapyRowPointer]::Down('mouse',$at.x,$at.y);Start-Sleep -Milliseconds 35
            Move-To $to
            Wait-Until {
                try{$g=(Workspace-Root).Current.HelpText|ConvertFrom-Json;$g.phase -eq 'dragging'}catch{$false}
            } 'Layer tab did not begin its native drag'
            [CapyRowPointer]::Up()
            Wait-Until {@((Model).layout.groups|Where-Object {$_.panels -contains 'layers' -and $_.floating}).Count -eq 1} 'Layer tab did not tear off into a floating panel'
        }else{
            Focus-Key 'panel-tab-layers' 0x5d
            Invoke 'Collapse column' -Name
            Wait-Until {$tile=Find 'column-icon-layers';$tile -and !$tile.Current.IsOffscreen} 'Collapsed column did not expose Layers'
            Invoke 'column-icon-layers'
            Wait-Until {@((Model).state.customization.column_drawers|Where-Object {$_.anchor.origin -eq 'layers'}).Count -eq 1} 'Layer column drawer did not open'
        }
        $null=Point "layer-$created-name"
        $script:case="$presentation-row";Drag-Row "layer-$created-name" $created $paint
        $script:case="$presentation-whitespace";Drag-Row "layer-row-$created" $created $paint
        $script:case="$presentation-child";Drag-Row "layer-$created-visibility" $created $paint
        $script:case="$presentation-grip";Drag-Row "layer-$created-drag" $created $paint -Grip
        $script:case="$presentation-cancel";Drag-Row "layer-$created-name" $created $paint -Cancel
        if($presentation -eq 'drawer' -and @((Model).state.customization.column_drawers).Count -eq 0){
            # Row cancellation can consume Escape before workspace chrome.
            # Reopen only if the containing drawer was dismissed too.
            Invoke 'column-icon-layers'
            Wait-Until {@((Model).state.customization.column_drawers|Where-Object {$_.anchor.origin -eq 'layers'}).Count -eq 1} 'Layer drawer did not reopen after cancellation'
        }
        $script:case="$presentation-release"
        $before=Rows
        Pick-Up "layer-$created-visibility" $created
        if($Device -eq 'mouse'){
            Start-Sleep -Milliseconds 800
            if((Gesture).menu_open){throw 'Mouse row hold opened a presentation menu'}
            [CapyRowPointer]::Key(0x1b);[CapyRowPointer]::Up()
        }else{
            [CapyRowPointer]::Up()
            Wait-Until {$g=Gesture;$g.phase -eq 'idle' -and $g.menu_open} 'Held presentation release did not retain its menu'
            Dismiss
        }
        if((Rows) -ne $before){throw 'Held presentation release activated visibility'}
        if($presentation -eq 'drawer'){
            if(@((Model).state.customization.column_drawers).Count){Invoke 'column-icon-layers'}
            Wait-Until {@((Model).state.customization.column_drawers).Count -eq 0} 'Layer drawer did not close'
        }
        & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'window'
        Invoke 'undo_workspace'
        try{
            Wait-Until {
                $restored=Model
                (($restored.state.workspace|ConvertTo-Json -Depth 80 -Compress) -eq $workspaceBefore) -and
                    @($restored.layout.groups|Where-Object {$_.panels -contains 'layers' -and !$_.floating}).Count -eq 1
            } 'Workspace Undo did not restore docked Layers'
        }finally{
            @{layout=(Model).layout;workspace=(Model).state.workspace}|ConvertTo-Json -Depth 90|Set-Content (Join-Path $run ($presentation+'-undo.json'))
        }
        $null=Point "layer-$created-name"
    }
}
try {
    Enter-CapyEnvironment @('CAPY_LAYER_DROP_FIXTURE')
    $env:CAPY_STORAGE_DIR=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1'
    [IO.Directory]::CreateDirectory($env:CAPY_STORAGE_DIR)|Out-Null
    [IO.File]::WriteAllText((Settings-File),(@{language=@{Explicit='en'};theme=$Theme}|ConvertTo-Json -Depth 4))
    if($PendingDropOnly){
        $env:CAPY_LAYER_DROP_FIXTURE=$run
        $stderr=Join-Path $run 'stderr.log'
        $review=Start-Process -FilePath $Executable -WorkingDirectory $run -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
        $null=$review.Handle
        $script:CapyStateFile=Join-Path $run "ui-state-$($review.Id)-1.json"
        $script:CapyTraceDirectory=$run
        $native=@{window=$null}
        Wait-Until {
            $windows=@(Owned-ProcessWindows $review|Where-Object {$_.Root.Current.Name -eq 'Capy Canvas layer drop fixture'})
            if($windows.Count -eq 1){$native.window=$windows[0];$script:root=$native.window.Root;$script:drawingWindow=$native.window.Handle}
            $native.window -and (Model).state.layers.Count -ge 2 -and (Find 'layer-list' -Within $root)
        } 'Layer drop fixture did not present real shared layers' 75
        Register-CapyFixtureWindow $review $drawingWindow.ToInt64() $run
        [CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
        if(![CapyRowPointer]::SetForegroundWindow($drawingWindow)){throw 'Could not focus owned layer drop fixture'}
        [CapyRowPointer]::Initialize([uint32]$review.Id)
        $created=(Model).state.layer_tools.editing_layer.id
        $paint=@((Model).state.layers|Where-Object id -ne $created)[0].id
        if(!$created -or !$paint){throw 'Layer drop fixture requires two real movable layers'}
        $script:case='new-press-after-drop'
        Drag-Row "layer-$created-drag" $created $paint -Grip -PendingDropGate
        [CapyRowPointer]::Dispose()
        $null=[CapyWindowApi]::PostMessage($drawingWindow,0x10,[UIntPtr]::Zero,[IntPtr]::Zero)
        if(!$review.WaitForExit(15000)){throw 'Owned layer drop fixture did not close'}
        if($review.ExitCode -ne 0){throw "Owned layer drop fixture exited $($review.ExitCode)"}
        if((Get-Item -LiteralPath $stderr).Length){throw 'Layer drop fixture stderr requires review'}
        @{device=$Device;theme=$Theme;scope='deterministic real-layer final drop overlap';last_ignored_down='drop_pending';reorder_undo_redo='passed';zero_exit='passed'}|ConvertTo-Json
        return
    }
    $stderr=Join-Path $run 'stderr.log'
    $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $run 'stdout.log') -RedirectStandardError $stderr
    $null=$review.Handle
    @{process_id=$review.Id;run=$run;device=$Device;theme=$Theme}|ConvertTo-Json|Set-Content (Join-Path $repo 'artifacts/windows/layer-pickup-review.json')
    Write-Output "Owned layer pickup review $($review.Id) ($Device)"
    $native=@{window=$null}
    Wait-Until {$native.window=Owned-DrawingWindow $review;$null -ne $native.window -and (Model).brush_ready -and (Model).windows_workspace.ready -and !(Model).windows_workspace.busy} 'Layer review did not start' 45
    $root=$native.window.Root;$drawingWindow=$native.window.Handle
    $null=[CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))
    $null=[CapyRowPointer]::SetForegroundWindow($drawingWindow)
    Wait-Until {
        $foreground=[CapyRowPointer]::GetForegroundWindow();$owner=[uint32]0
        [CapyRowPointer]::GetWindowThreadProcessId($foreground,[ref]$owner)|Out-Null
        $script:foregroundState=@{window=$foreground.ToInt64();process=$owner;expected_process=$review.Id;expected_window=$drawingWindow.ToInt64()}
        if($owner -ne $review.Id){[CapyRowPointer]::SetForegroundWindow($drawingWindow)|Out-Null}
        $owner -eq $review.Id
    } 'Owned layer review did not receive foreground input'
    [CapyRowPointer]::Initialize([uint32]$review.Id)
    Use-PaintWorkspace
    # Current Paint defaults retain Layers in an open collapsed-column stack.
    # Begin with a fully expanded column before the fixture tests collapse/expand.
    if(Find 'column-icon-layers'){
        $at=Point 'column-icon-layers';[CapyRowPointer]::RightClick($at.x,$at.y)
        Invoke 'Expand column' -Name
        Wait-Until {$null -ne (Find 'panel-tab-layers') -and !(Find 'column-icon-layers')} 'Initial Layers column did not expand'
    }
    $paint=(Model).state.layer_tools.editing_layer.id
    $count=(Model).state.layers.Count;Invoke 'layer-new'
    Wait-Until {(Model).state.layers.Count -eq $count+1} 'New layer did not appear'
    $created=(Model).state.layer_tools.editing_layer.id
    if($ClippedEdgesOnly){
        Check-ClippedEdges
        [CapyRowPointer]::Dispose()
        & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Close -DiscardUnsaved
        if((Get-Item -LiteralPath $stderr).Length){throw 'Clipped-edge stderr requires review'}
        @{device=$Device;theme=$Theme;partially_clipped_header_footer_drops='passed';one_step_undo='passed'}|ConvertTo-Json
        return
    }
    if($PresentationsOnly){
        Check-Presentations $created $paint
        [CapyRowPointer]::Dispose()
        & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Close -DiscardUnsaved
        if((Get-Item -LiteralPath $stderr).Length){throw 'Native presentation stderr requires review'}
        @{device=$Device;theme=$Theme;floating_and_drawer_rows='passed';scope='Native layer floating and drawer presentations'}|ConvertTo-Json
        return
    }
    if($KeyboardContextOnly){
        Check-KeyboardContext $created
        [CapyRowPointer]::Dispose()
        & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Close -DiscardUnsaved
        if((Get-Item -LiteralPath $stderr).Length){throw 'Native keyboard context stderr requires review'}
        @{device=$Device;theme=$Theme;keyboard_context='passed';scope='Native layer keyboard and nested-menu context'}|ConvertTo-Json
        return
    }
    if($HoldOnly){
        $script:case='row-whitespace';Drag-Row "layer-row-$created" $created $paint
        [CapyRowPointer]::Dispose()
        & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Close -DiscardUnsaved
        if((Get-Item -LiteralPath $stderr).Length){throw 'Native row hold stderr requires review'}
        @{device=$Device;theme=$Theme;row_whitespace_hold_drag_undo_redo='passed';scope='Native row body hold and reorder'}|ConvertTo-Json
        return
    }
    $script:case='visibility-short-click'
    Tap "layer-$created-visibility"
    Wait-Until {!@((Model).state.layers|Where-Object id -eq $created)[0].visible} 'Short click did not toggle visibility'
    Tap "layer-$created-visibility"
    Wait-Until {@((Model).state.layers|Where-Object id -eq $created)[0].visible} 'Second short click did not restore visibility'
    $before=Rows
    $script:case='row-hold-release'
    Press "layer-$created-visibility" $created
    if($Device -eq 'mouse'){
        Start-Sleep -Milliseconds 800
        $g=Gesture
        if($g.menu_open -or $g.phase -eq 'held'){throw 'Stationary mouse row hold opened a menu'}
        [CapyRowPointer]::Key(0x1b);[CapyRowPointer]::Up()
    }else{
        Wait-Until {$g=Gesture;$g.phase -eq 'held' -and $g.menu_open} 'Row hold did not show its context menu'
        [CapyRowPointer]::Up()
        $finished=@{state=$null}
        Wait-Until {$finished.state=Gesture;$finished.state.phase -eq 'idle'} 'Held release retained a pointer'
        if(!$finished.state.menu_open){throw 'Held release dismissed its menu'}
        Dismiss
    }
    if((Rows) -ne $before){throw 'Held/cancelled release activated visibility'}
    if($Device -ne 'mouse'){
        $script:case='early-row-motion'
        $at=Point "layer-$created-name";$to=Destination $paint
        # Keep UIA calls out of the timing-critical sequence.
        [CapyRowPointer]::Down($Device,$at.x,$at.y);Start-Sleep -Milliseconds 35
        Move-To @{x=$at.x;y=$to.y};[CapyRowPointer]::Up()
        Wait-Until {(Gesture).phase -eq 'idle'} 'Early motion retained a contact'
        if((Rows) -ne $before -or (Gesture).menu_open -or (Find "layer-$created-swipe-delete" -Visible)){throw 'Early vertical row motion reordered layers or opened a menu or swipe action'}
    }
    if($Device -ne 'mouse'){
        $script:case='right-swipe-quick-reverse'
        $before=Rows
        $alphaBefore=@((Model).state.layers|Where-Object id -eq $created)[0].alpha_locked
        $at=Point "layer-$created-name"
        [CapyRowPointer]::Down($Device,$at.x,$at.y);Start-Sleep -Milliseconds 35
        Move-To @{x=$at.x+90;y=$at.y}
        Wait-Until {(Gesture).source_swipe -lt -29} 'Right swipe did not reach its action threshold'
        [CapyRowPointer]::Up()
        [CapyRowPointer]::Down($Device,$at.x,$at.y)
        Move-To @{x=$at.x-90;y=$at.y};[CapyRowPointer]::Up()
        Wait-Until {(Gesture).phase -eq 'idle' -and @((Model).state.layers|Where-Object id -eq $created)[0].alpha_locked -ne $alphaBefore} 'Immediate reverse swipe repeated the right swipe action'
        Layer-History 'Undo';Wait-Until {(Rows) -eq $before} 'Right swipe and immediate reverse changed more than one action'
        if(Find "layer-$created-swipe-delete" -Visible){Tap-RowBody $created;Wait-Until {!(Find "layer-$created-swipe-delete" -Visible)} 'Reverse swipe actions did not close on their row body'}
        $script:case='swipe-delete-close'
        $before=Rows
        Record-Swipe 'baseline'
        $at=Point "layer-$created-name"
        [CapyRowPointer]::Down($Device,$at.x,$at.y);Start-Sleep -Milliseconds 35
        Move-To @{x=$at.x-90;y=$at.y}
        $swipe=@{state=$null}
        Wait-Until {$swipe.state=Gesture;$swipe.state.source_swipe -gt 0} 'Partial swipe did not reach the native row'
        $partial=$swipe.state;[CapyRowPointer]::Up()
        $swipeStages.Add(@{stage='partial';gesture=$partial})
        if($partial.source_swipe -le 0 -or $partial.source_grip_opacity -le 0 -or $partial.source_grip_opacity -ge .6){throw 'Partial swipe did not fade the retained grip'}
        Wait-Until {$null -ne (Find "layer-$created-swipe-delete" -Visible)} 'Left swipe did not reveal Delete'
        Record-Swipe 'delete-opened'
        Tap-RowBody $created
        Wait-Until {!(Find "layer-$created-swipe-delete" -Visible)} 'A tap on an open row body did not close its actions'
        $at=Point "layer-$created-name"
        [CapyRowPointer]::Down($Device,$at.x,$at.y);Start-Sleep -Milliseconds 35
        Move-To @{x=$at.x-90;y=$at.y};[CapyRowPointer]::Up()
        Wait-Until {Find "layer-$created-swipe-delete" -Visible} 'Delete did not reopen after the row body tap'
        $action=(Control "layer-$created-swipe-delete" -Arranged).Current.BoundingRectangle
        $inset=2*[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
        $at=@{x=[int]($action.X+$action.Width*.5);y=[int][Math]::Ceiling($action.Top+$inset)}
        $finish=@{x=$at.x;y=[int][Math]::Floor($action.Bottom-$inset)}
        if(!$action.Contains([Windows.Point]::new($at.x,$at.y)) -or !$action.Contains([Windows.Point]::new($finish.x,$finish.y))){throw 'Displaced strip contact must start and finish inside Delete'}
        $generation=(Gesture).generation
        [CapyRowPointer]::Down($Device,$at.x,$at.y);Start-Sleep -Milliseconds 35
        Move-To $finish;[CapyRowPointer]::Up()
        $displaced=@{state=$null}
        Wait-Until {$displaced.state=Gesture;$displaced.state.generation -gt $generation -and $displaced.state.phase -eq 'idle'} 'Displaced row retained its contact'
        $swipeStages.Add(@{stage='displaced-terminal';gesture=$displaced.state;start=$at;finish=$finish;action_bounds=$action})
        if($displaced.state.last_cancel.generation -le $generation -or $displaced.state.last_cancel.reason -ne 'native_scroll'){throw 'Native movement did not admit the displaced strip scrolling prerequisite'}
        if($displaced.state.initial_swipe -le 0 -or $displaced.state.pickup -or $displaced.state.menu_open -or (Rows) -ne $before){throw 'Displaced strip contact activated an action, menu or reorder'}
        Record-Swipe 'after-vertical-motion'
        if(!(Find "layer-$created-swipe-delete" -Visible)){
            $at=Point "layer-$created-name"
            [CapyRowPointer]::Down($Device,$at.x,$at.y);Start-Sleep -Milliseconds 35
            Move-To @{x=$at.x-90;y=$at.y};[CapyRowPointer]::Up()
        }
        Wait-Until {$null -ne (Find "layer-$created-swipe-delete" -Visible)} 'Delete is not revealed before the reverse swipe'
        Record-Swipe 'open-before-reverse'
        $at=Point "layer-row-$created"
        [CapyRowPointer]::Down($Device,$at.x,$at.y);Start-Sleep -Milliseconds 35
        Move-To @{x=$at.x+110;y=$at.y};[CapyRowPointer]::Up()
        Wait-Until {(Gesture).phase -eq 'idle' -and $null -eq (Find "layer-$created-swipe-delete" -Visible)} 'Right swipe did not close Delete'
        Record-Swipe 'reverse-closed'
        if((Rows) -ne $before){throw 'Closing Delete applied the right swipe action'}
        $at=Point "layer-$created-name"
        [CapyRowPointer]::Down($Device,$at.x,$at.y);Start-Sleep -Milliseconds 35
        Move-To @{x=$at.x-90;y=$at.y};[CapyRowPointer]::Up()
        Wait-Until {$null -ne (Find "layer-$created-swipe-delete" -Visible)} 'Delete did not reveal again'
        Tap "layer-$created-swipe-delete"
        Wait-Until {!@((Model).state.layers|Where-Object id -eq $created).Count} 'Revealed Delete did not remove its row'
        Layer-History 'Undo';Wait-Until {(Rows) -eq $before} 'Revealed Delete requires more than one Undo'
        Record-Swipe 'delete-undone'
    }
    $script:case='settled-grip-before-pending-drop';Drag-Row "layer-$created-drag" $created $paint -Grip
    if($SwipeOnly){
        $script:case='settled-grip-reorder';Drag-Row "layer-$created-drag" $created $paint -Grip
        $script:case='settled-grip-cancel';Drag-Row "layer-$created-drag" $created $paint -Grip -Cancel
        [CapyRowPointer]::Dispose()
        & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Close -DiscardUnsaved
        if((Get-Item -LiteralPath $stderr).Length){throw 'Native swipe stderr requires review'}
        @{device=$Device;theme=$Theme;swipe_strip_grip_and_rest_gate=$(if($Device -eq 'mouse'){'not_applicable'}else{'passed'});settled_grip_reorder_undo_redo='passed';settled_grip_cancel='passed';scope='Functional native input; animation frame pacing and physical input unverified'}|ConvertTo-Json
        return
    }
    if($Device -eq 'mouse'){
        foreach($source in @("layer-$created-name","layer-$created-drag")){
            $script:case="middle-source-denied-$source"
            $at=Point $source;$to=Destination $paint;$before=Contact-State
            $tracePath=Join-Path $run ($source+'-middle-denied.json')
            $script:contact=$evidence=@{device=$Device;button='middle';source=$source;from=$at;to=$to;before=$before}
            $evidence|ConvertTo-Json -Depth 18|Set-Content $tracePath
            [CapyRowPointer]::MiddleDrag($at.x,$at.y,$to.x,$to.y)
            $evidence.contact_ms=[CapyRowPointer]::ContactDurationMilliseconds;$evidence.max_gap_ms=[CapyRowPointer]::MaxGapMilliseconds
            try{Wait-Until {$g=Gesture;$g.phase -eq 'idle' -and !$g.captured -and !$g.menu_open} 'Middle body or grip drag retained a custom contact, capture or menu'}
            finally{$evidence.after=Contact-State;$evidence.driver_active=[CapyRowPointer]::Active;$evidence|ConvertTo-Json -Depth 18|Set-Content $tracePath}
            $after=$evidence.after
            if($evidence.driver_active -or $after.rows -ne $before.rows -or $after.selected -ne $before.selected -or $after.revision -ne $before.revision -or ($after.editing_layer|ConvertTo-Json -Depth 12 -Compress) -ne ($before.editing_layer|ConvertTo-Json -Depth 12 -Compress)){throw 'Middle body or grip drag changed layer state or history'}
            if(($after.gesture.last_release|ConvertTo-Json -Depth 12 -Compress) -ne ($before.gesture.last_release|ConvertTo-Json -Depth 12 -Compress) -or ($after.gesture.last_cancel|ConvertTo-Json -Depth 12 -Compress) -ne ($before.gesture.last_cancel|ConvertTo-Json -Depth 12 -Compress)){throw 'Middle body or grip drag completed a custom gesture'}
        }
    }
    $script:case='locked-source-denied'
    Tap "layer-$created-name"
    $unlocked=Rows
    $lock=(Control 'layer-lock').GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern)
    if($lock.Current.ToggleState -ne [System.Windows.Automation.ToggleState]::Off){throw 'Locked-source fixture did not start unlocked'}
    $lock.Toggle()
    Wait-Until {$snapshot=Model;$row=@($snapshot.state.layers|Where-Object id -eq $created)[0];$row.locked -and @($row.drag_from_body).Count -eq 0 -and $snapshot.state.layer_tools.editing_layer.locked -and !(Control ("layer-$created-drag")).Current.IsEnabled} 'Lock did not publish an effectively locked source with no body pickup devices'
    $lockedOrder=Rows;$attempts=[Collections.Generic.List[object]]::new()
    foreach($source in @("layer-$created-name","layer-$created-drag")){
        if($source -like '*-drag' -and (Control $source).Current.IsEnabled){throw 'Locked source retained an enabled reorder handle'}
        $at=Point $source;$to=Destination $paint
        [CapyRowPointer]::Down($Device,$at.x,$at.y)
        try{
            Start-Sleep -Milliseconds 35;Move-To $to
            $held=Gesture;$attempts.Add(@{source=$source;from=$at;to=$to;held=$held;rows=(Rows)})
            $attempts|ConvertTo-Json -Depth 12|Set-Content (Join-Path $run 'locked-source-denied.json')
            if($held.phase -eq 'dragging' -or $held.can_drop -or (Rows) -ne $lockedOrder){throw 'Locked body or handle motion admitted a layer move'}
        }finally{if($Device -eq 'mouse'){[CapyRowPointer]::Key(0x1b);[CapyRowPointer]::Up()}else{[CapyRowPointer]::Cancel()}}
        Wait-Until {(Gesture).phase -eq 'idle'} 'Locked-source rejection retained a pointer contact'
        if((Gesture).menu_open){Dismiss}
        $snapshot=Model
        if((Rows $snapshot) -ne $lockedOrder -or !@($snapshot.state.layers|Where-Object id -eq $created)[0].locked){throw 'Rejected locked-source motion changed layers or cleared the lock'}
    }
    $attempts|ConvertTo-Json -Depth 12|Set-Content (Join-Path $run 'locked-source-denied.json')
    foreach($source in @("layer-$created-name","layer-$created-drag")){
        $script:case="locked-source-fast-exit-$source"
        $at=Point $source;$to=Point 'drawing-canvas'
        $target=Control $source;$canvas=Control 'drawing-canvas'
        $bounds=$target.Current.BoundingRectangle;$canvasBounds=$canvas.Current.BoundingRectangle;$viewport=Layer-Viewport
        $before=Contact-State
        if($paint -eq $created -or $before.rows -ne $lockedOrder -or $before.selected -ne [string]$created -or $before.editing_layer.id -ne $created -or !$before.editing_layer.locked -or ($source -like '*-drag' -and $target.Current.IsEnabled)){throw 'Fast locked-source exit lacks its selected locked baseline'}
        if(!$bounds.Contains($at.x,$at.y) -or !$canvasBounds.Contains($to.x,$to.y) -or $viewport.Contains($to.x,$to.y) -or !$root.Current.BoundingRectangle.Contains($to.x,$to.y)){throw 'Fast locked-source exit does not leave Layers for the drawing canvas'}
        $hits=@(foreach($point in @($at,$to)){
            $native=[CapyRowPointer+Point]::new();$native.x=$point.x;$native.y=$point.y
            $handle=[CapyRowPointer]::WindowFromPoint($native);$owner=[uint32]0
            [CapyRowPointer]::GetWindowThreadProcessId($handle,[ref]$owner)|Out-Null
            @{hwnd=$handle.ToInt64();process=$owner;input=(Input-Trace $point.x $point.y)}
        })
        if($hits[0].process -ne $review.Id -or $hits[1].process -ne $review.Id -or !$target.Current.BoundingRectangle.Equals($bounds) -or !$canvas.Current.BoundingRectangle.Equals($canvasBounds)){throw 'Fast locked-source exit lost its owned source or canvas geometry'}
        $tracePath=Join-Path $run ($source+'-fast-canvas-exit.json')
        $script:contact=$evidence=@{device=$Device;source=$source;from=$at;to=$to;source_bounds=$bounds;canvas_bounds=$canvasBounds;viewport_bounds=$viewport;hits=$hits;before=$before}
        $evidence|ConvertTo-Json -Depth 18|Set-Content $tracePath
        [CapyRowPointer]::Down($Device,$at.x,$at.y)
        [CapyRowPointer]::Move($to.x,$to.y)
        [CapyRowPointer]::Up()
        $evidence.contact_ms=[CapyRowPointer]::ContactDurationMilliseconds;$evidence.max_gap_ms=[CapyRowPointer]::MaxGapMilliseconds
        try{Wait-Until {$g=Gesture;$g.phase -eq 'idle' -and !$g.captured -and !$g.menu_open} 'Fast locked-source exit retained a contact, capture or menu'}
        finally{$evidence.after=Contact-State;$evidence.driver_active=[CapyRowPointer]::Active;$evidence|ConvertTo-Json -Depth 18|Set-Content $tracePath}
        $after=$evidence.after
        if($evidence.driver_active -or $after.rows -ne $before.rows -or $after.selected -ne $before.selected -or $after.revision -ne $before.revision -or ($after.editing_layer|ConvertTo-Json -Depth 12 -Compress) -ne ($before.editing_layer|ConvertTo-Json -Depth 12 -Compress)){throw 'Fast locked-source exit changed layer state or history'}
        Tap "layer-$paint-name"
        try{Wait-Until {$snapshot=Model;$g=Gesture;$snapshot.state.layer_tools.editing_layer.id -eq $paint -and @($snapshot.state.layers|Where-Object id -eq $paint)[0].selected -and $g.phase -eq 'idle' -and !$g.captured -and !$g.menu_open} 'Fresh native tap did not select the inactive layer after fast exit'}
        finally{$evidence.followup=Contact-State;$evidence|ConvertTo-Json -Depth 18|Set-Content $tracePath}
        if($evidence.followup.rows -ne $before.rows -or $evidence.followup.selected -ne [string]$paint -or $evidence.followup.checkpoint -ne $before.checkpoint -or $evidence.followup.modified -ne $before.modified -or $evidence.followup.gesture.phase -ne 'idle' -or $evidence.followup.gesture.captured -or $evidence.followup.gesture.menu_open){throw 'Fresh native selection after fast exit changed authored state or retained a contact'}
        Tap "layer-$created-name"
        try{Wait-Until {$snapshot=Model;$g=Gesture;$snapshot.state.layer_tools.editing_layer.id -eq $created -and @($snapshot.state.layers|Where-Object id -eq $created)[0].selected -and $g.phase -eq 'idle' -and !$g.captured -and !$g.menu_open} 'Native tap did not restore the locked fixture selection'}
        finally{$evidence.restored=Contact-State;$evidence|ConvertTo-Json -Depth 18|Set-Content $tracePath}
        $restored=$evidence.restored
        if($restored.rows -ne $before.rows -or $restored.selected -ne $before.selected -or $restored.checkpoint -ne $before.checkpoint -or $restored.modified -ne $before.modified -or ($restored.editing_layer|ConvertTo-Json -Depth 12 -Compress) -ne ($before.editing_layer|ConvertTo-Json -Depth 12 -Compress) -or $restored.gesture.phase -ne 'idle' -or $restored.gesture.captured -or $restored.gesture.menu_open){throw 'Native selection restore changed the locked fixture or retained a contact'}
    }
    Layer-History 'Undo'
    Wait-Until {$snapshot=Model;(Rows $snapshot) -eq $unlocked -and !@($snapshot.state.layers|Where-Object id -eq $created)[0].locked} 'Rejected locked-source motion added history after the Lock action'
    Layer-History 'Redo'
    Wait-Until {$snapshot=Model;(Rows $snapshot) -eq $lockedOrder -and @($snapshot.state.layers|Where-Object id -eq $created)[0].locked} 'One Redo did not restore the exact locked source'
    Layer-History 'Undo'
    Wait-Until {$snapshot=Model;$row=@($snapshot.state.layers|Where-Object id -eq $created)[0];(Rows $snapshot) -eq $unlocked -and !$row.locked -and (@($row.drag_from_body) -join ',') -eq 'mouse'} 'Final Undo did not restore unlocked mouse body pickup'
    $script:case='row-drag';Drag-Row "layer-$created-name" $created $paint
    $script:case='row-whitespace';Drag-Row "layer-row-$created" $created $paint
    $script:case='child-control-drag';Drag-Row "layer-$created-visibility" $created $paint
    $script:case='immediate-grip';Drag-Row "layer-$created-drag" $created $paint -Grip
    $script:case='header-edge-drop';Drag-PanelEdge $paint before
    $script:case='empty-list-edge-drop';Drag-PanelEdge $created after
    $script:case='row-cancel';Drag-Row "layer-$created-name" $created $paint -Cancel
    Check-KeyboardContext $created
    Check-Presentations $created $paint
    $before=Contact-State
    $space=@{before=(Layer-Viewport);artwork_before=@{rows=$before.rows;checkpoint=$before.checkpoint;modified=$before.modified}}
    try{
        $window=$root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern)
        $window.SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Maximized)
        Wait-Until {$window.Current.WindowVisualState -eq [System.Windows.Automation.WindowVisualState]::Maximized -and [Math]::Abs((Control 'drawing-canvas').Current.BoundingRectangle.Width-(Model).state.camera.viewport[0]) -lt .1} 'Maximized pickup geometry did not settle'
        $rowHeight=(Control "layer-row-$created").Current.BoundingRectangle.Height;$space.row_height=$rowHeight
        Wait-Until {(Layer-Viewport).Height -ge $rowHeight*5} 'Layer viewport did not expose the group drop targets away from scrolling edges'
        $after=Contact-State;$space.artwork_after=@{rows=$after.rows;checkpoint=$after.checkpoint;modified=$after.modified}
        if($after.rows -ne $before.rows -or $after.checkpoint -ne $before.checkpoint -or $after.modified -ne $before.modified){throw 'Layer viewport resize changed artwork or its checkpoint'}
    }finally{$space.after=Layer-Viewport;$space|ConvertTo-Json -Depth 8|Set-Content (Join-Path $run 'group-viewport.json')}
    $script:case='remaining-child-controls'
    Tap "layer-$created-name";Invoke 'layer-add-mask'
    Wait-Until {@((Model).state.layers|Where-Object id -eq $created)[0].has_mask} 'Mask did not appear for child pickup tests'
    Tap "layer-$created-content"
    foreach($child in @('selection','content','mask','link')){
        Drag-Row ("layer-$created-"+$child) $created $paint
    }
    Invoke 'layer-new-group'
    Wait-Until {
        $active=(Model).state.layer_tools.editing_layer
        $active.group -and $active.id -ne $created -and $null -ne (Find ("layer-"+$active.id+"-content"))
    } 'New group did not appear'
    $group=(Model).state.layer_tools.editing_layer.id
    if($Device -eq 'mouse'){
        $script:case='group-content-fast-exit'
        $before=Rows
        $at=Point "layer-$group-content";$to=Point 'drawing-canvas'
        [CapyRowPointer]::Down('mouse',$at.x,$at.y);Start-Sleep -Milliseconds 35
        Move-To $to
        Wait-Until {$g=Gesture;$g.phase -eq 'dragging' -and $g.source -eq $group -and $g.captured} 'Fast mouse exit from the group row lost its pickup'
        [CapyRowPointer]::Key(0x1b);[CapyRowPointer]::Up()
        Wait-Until {(Gesture).phase -eq 'idle'} 'Fast mouse exit cancellation retained its contact'
        if((Rows) -ne $before -or @((Model).state.layers|Where-Object id -eq $group)[0].collapsed){throw 'Fast mouse exit activated the group or changed layer order'}
    }
    $script:case='group-content'
    Drag-Row "layer-$group-content" $group $paint
    if(@((Model).state.layers|Where-Object id -eq $group)[0].collapsed){throw 'Group content drag activated the collapse button'}
    Tap "layer-$group-content"
    Wait-Until {@((Model).state.layers|Where-Object id -eq $group)[0].collapsed} 'Group did not collapse before an into-group drop'
    $script:case='drop-into-group'
    Drag-Row "layer-$created-name" $created $group -Fraction .5
    if(@((Model).state.layers|Where-Object id -eq $group)[0].collapsed){throw 'Dropping into a group did not expose its child'}
    $script:case='source-removal'
    Tap "layer-$created-name"
    $before=Rows
    Pick-Up "layer-$created-name" $created
    Invoke 'layer-delete'
    Wait-Until {!@((Model).state.layers|Where-Object id -eq $created).Count -and (Gesture).phase -eq 'idle'} 'Deleting the source did not cancel its contact'
    [CapyRowPointer]::Up()
    if((Gesture).menu_open){throw 'Source removal left a context menu'}
    Layer-History 'Undo';Wait-Until {(Rows) -eq $before} 'Undo did not restore the removed source'
    $script:case='native-scrolling'
    for($i=0;$i -lt 12;$i++){
        $count=(Model).state.layers.Count;Invoke 'layer-new'
        Wait-Until {(Model).state.layers.Count -eq $count+1} 'Overflow layer did not appear'
    }
    $scroll=Scroll-Pattern (Control 'layer-list')
    if(!$scroll.Current.VerticallyScrollable){throw 'Layer fixture did not overflow its native list'}
    $scroll.SetScrollPercent(-1,0)
    Wait-Until {$scroll.Current.VerticalScrollPercent -lt .1} 'Native list did not return to its top'
    if($Device -ne 'mouse'){
        $viewport=Layer-Viewport
        $visible=@((Model).state.layers|Where-Object {
            $row=Find ("layer-row-"+$_.id)
            $row -and !$row.Current.IsOffscreen -and $row.Current.BoundingRectangle.Y -gt $viewport.Y+25 -and $row.Current.BoundingRectangle.Bottom -lt $viewport.Bottom-10
        })
        if(!$visible.Count){throw 'No interior layer row is visible for the scrolling test'}
        $middle=$visible[[int][Math]::Floor($visible.Count/2)].id
        $at=Point "layer-$middle-name"
        $before=Rows
        [CapyRowPointer]::Down($Device,$at.x,$at.y)
        # Allow InteractionTracker to consume movement before release. A single
        # teleport followed immediately by Up need not produce a scroll frame.
        # All movement still precedes a stationary hold; no UIA calls intervene.
        foreach($dy in @(25,55,90)){
            Start-Sleep -Milliseconds 35
            Move-To @{x=$at.x;y=[int][Math]::Max($viewport.Y+4,$at.y-$dy)}
        }
        Start-Sleep -Milliseconds 35
        [CapyRowPointer]::Up()
        Wait-Until {$scroll.Current.VerticalScrollPercent -gt .5} 'Early pen/touch row movement did not scroll the native list'
        $finished=@{state=$null}
        Wait-Until {$finished.state=Gesture;$finished.state.phase -eq 'idle'} 'Native scrolling retained a reorder contact'
        if((Rows) -ne $before -or $finished.state.menu_open){throw 'Native scrolling changed the document or opened a menu'}
    }
    $script:case='mouse-scrollbar-transition'
    $viewport=Layer-Viewport
    [CapyRowPointer]::Hover([int]($viewport.Right-4),[int]($viewport.Y+$viewport.Height*.5))
    Wait-Until {@(Layer-Scrollbars).Count -gt 0} 'Mouse did not show the native layer scrollbar'
    $bar=@(Layer-Scrollbars)[0].Current.BoundingRectangle
    $percent=$scroll.Current.VerticalScrollPercent
    [CapyRowPointer]::Down('mouse',[int]($bar.X+$bar.Width*.5),[int]($bar.Y+$bar.Height*.8))
    Start-Sleep -Milliseconds 35
    [CapyRowPointer]::Up()
    Wait-Until {[Math]::Abs($scroll.Current.VerticalScrollPercent-$percent) -gt .5} 'Mouse scrollbar interaction did not scroll the native list'
    $script:case='edge-scroll-retention'
    $scroll.SetScrollPercent(-1,0)
    Wait-Until {$scroll.Current.VerticalScrollPercent -lt .1} 'Native list did not return to its top'
    $latest=(Model).state.layer_tools.editing_layer.id
    $at=Point "layer-$latest-drag"
    $viewport=Layer-Viewport
    $before=Rows
    [CapyRowPointer]::Down($Device,$at.x,$at.y);Start-Sleep -Milliseconds 35
    Move-To @{x=[int]($viewport.X+$viewport.Width*.5);y=[int]($viewport.Bottom-4)}
    if($Device -ne 'mouse'){Wait-Until {@(Layer-Scrollbars).Count -eq 0} 'First direct handle contact retained the mouse scrollbar'}
    $edge=@{state=$null}
    Wait-Until {$scrolled=$scroll.Current.VerticalScrollPercent -gt 60;$edge.state=Gesture;$edge.state.phase -eq 'dragging' -and $scrolled} 'Accepted drag did not keep scrolling after its source left the viewport'
    if($edge.state.source -ne $latest -or !$edge.state.captured){throw 'Auto-scroll lost the original source or capture'}
    if($Device -eq 'mouse'){[CapyRowPointer]::Key(0x1b);[CapyRowPointer]::Up()}else{[CapyRowPointer]::Cancel()}
    Wait-Until {(Gesture).phase -eq 'idle'} 'Edge-scroll cancellation retained a contact'
    if((Rows) -ne $before){throw 'Canceled edge-scroll changed layer order'}
    $script:case='minimize-cancellation'
    $scroll.SetScrollPercent(-1,0)
    Wait-Until {$scroll.Current.VerticalScrollPercent -lt .1} 'Native list did not return to its top after cancellation'
    Pick-Up "layer-$latest-name" $latest
    $held=Gesture
    $window=$root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern)
    $visualState=$window.Current.WindowVisualState
    $window.SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Minimized)
    try{
        Wait-Until {$g=Gesture;$g.phase -eq 'idle' -and !$g.menu_open -and $g.last_cancel.generation -ge $held.generation} 'Minimizing left the row contact or menu active'
        [CapyRowPointer]::Cancel()
    }finally{
        [CapyRowPointer]::Dispose()
        $window.SetWindowVisualState($visualState)
        $null=[CapyRowPointer]::SetForegroundWindow($drawingWindow)
        [CapyRowPointer]::Initialize([uint32]$review.Id)
    }
    if((Rows) -ne $before){throw 'Minimizing committed the row gesture'}
    # Finish pointer arbitration before text input: the system touch keyboard
    # legitimately changes the HWND and viewport while opening or closing.
    $null=Point 'layer-list'
    $scroll=Scroll-Pattern (Control 'layer-list')
    $scroll.SetScrollPercent(-1,100)
    Wait-Until {$scroll.Current.VerticalScrollPercent -gt 99.9} 'Native list did not reveal its original rows'
    $script:case='rename-ownership'
    Focus-Key "layer-$created-name" 0x71
    Wait-Until {$field=Find "layer-$created-rename";$field -and !$field.Current.IsOffscreen} 'F2 did not start native name editing'
    $field=Control "layer-$created-rename"
    $field.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('Renamed layer fixture')
    $at=Point "layer-$created-rename"
    [CapyRowPointer]::Down($Device,$at.x,$at.y);Start-Sleep -Milliseconds 35
    Move-To @{x=$at.x+12;y=$at.y};[CapyRowPointer]::Up()
    if((Gesture).phase -ne 'idle'){throw 'Layer pickup stole input from the native rename field'}
    Focus-Key "layer-$created-rename" 0x0d
    Wait-Until {(Control "layer-$created-name").Current.Name -eq 'Renamed layer fixture'} 'Enter did not commit the native rename'
    Focus-Key "layer-$created-name" 0x71
    Wait-Until {$field=Find "layer-$created-rename";$field -and !$field.Current.IsOffscreen} 'Rename did not reopen'
    (Control "layer-$created-rename").GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('Canceled name')
    Focus-Key "layer-$created-rename" 0x1b
    Wait-Until {(Control "layer-$created-name").Current.Name -eq 'Renamed layer fixture' -and !(Find "layer-$created-name").Current.IsOffscreen} 'Escape did not cancel only the rename'
    [CapyRowPointer]::Dispose()
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Close -DiscardUnsaved
    if((Get-Item -LiteralPath $stderr).Length){throw 'Native stderr requires review'}
    Verify-PendingDropFixture
    [pscustomobject]@{device=$Device;theme=$Theme;column_mode=$ColumnMode;short_click='passed';hold_release='passed';row_drag=$(if($Device -eq 'mouse'){'passed'}else{'handle_only_passed'});child_control_drag=$(if($Device -eq 'mouse'){'passed'}else{'denied_passed'});immediate_grip='passed';cancel='passed';one_step_undo_redo='passed';row_whitespace='passed';rename_ownership='passed';source_removal='passed';mask_and_selection_children='passed';group_content_and_into_drop='passed';keyboard_context='passed';edge_scroll_retention='passed';minimize_cancellation='passed';native_scroll=$(if($Device -eq 'mouse'){'not_applicable'}else{'passed'});floating_and_drawer_rows='passed';scope='OS-delivered synthetic input; physical-device and presentation-performance acceptance remain separate'}|ConvertTo-Json
}catch{
    $fixtureFailed=$true;$failure=$_
    try{
        @{case=$script:case;error=$failure.ToString();script_stack=$failure.ScriptStackTrace;contact=$script:contact;press=$script:press;foreground=$script:foregroundState;gesture=(Gesture);rows=(Rows);workspace_presentation=$(try{(Workspace-Root).Current.ItemStatus}catch{$null})}|ConvertTo-Json -Depth 10|Set-Content (Join-Path $run 'failure.json')
        if($review -and !$review.HasExited){
            & (Join-Path $PSScriptRoot 'inspect-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Output (Join-Path $run 'failure.png') -ClientOnly *> (Join-Path $run 'failure-window.json')
        }
    }catch{Write-Warning $_}
    throw $failure
}finally{
    $cleanup=@()
    foreach($finish in @(
        {if($Device -eq 'mouse' -and [CapyRowPointer]::Active){[CapyRowPointer]::Key(0x1b)}},
        {[CapyRowPointer]::Dispose()},
        {if($review){$review.Refresh();if(!$review.HasExited){$review.Kill();if(!$review.WaitForExit(8000)){throw 'Owned layer review did not stop during fixture cleanup'}}}},
        {Exit-CapyEnvironment},
        {if($pressContacts.Count){$pressContacts|ConvertTo-Json -Depth 12|Set-Content (Join-Path $run 'press-contacts.json')}},
        {if($swipeStages.Count){$swipeStages|ConvertTo-Json -Depth 18|Set-Content (Join-Path $run 'swipe-stages.json')}},
        {$sourceReveals|ForEach-Object {$_|ConvertTo-Json -Depth 8 -Compress}|Set-Content (Join-Path $run 'source-reveal.jsonl')}
    )){try{& $finish}catch{$cleanup+=($_|Out-String)}}
    if($cleanup.Count){
        try{$cleanup|Set-Content (Join-Path $run 'cleanup-failure.txt')}catch{Write-Warning $_}
        if(!$fixtureFailed){throw ($cleanup -join "`n")}
    }
}
