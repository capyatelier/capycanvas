param([Parameter(Mandatory)][string]$Executable,[ValidateSet('dark','light')][string]$Theme='dark',[ValidateSet('all','baseline','cold-controls','first-curve','deferred','deferred-prior-edit','response','overlays','baseline-light','cold-controls-light','first-curve-light','deferred-light','deferred-prior-edit-light','response-light','overlays-light')][string]$Journey='all')
if($Journey.EndsWith('-light')){$Journey=$Journey.Substring(0,$Journey.Length-6);$Theme='light'}
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
Add-Type -Path (Join-Path $PSScriptRoot 'CanvasTouchDriver.cs')
Add-Type -AssemblyName System.Drawing
$CapyCaptureDelay=250
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/tilt/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
function Json($Value){ConvertTo-Json -InputObject $Value -Compress -Depth 40}
function Tilt{(Model).state.tilt_calibration}
function Points{(Tilt).editor.points}
function Curve{Json (Points)}
function Saved{Json (Model).state.settings.tilt_calibrations}
function Same-TiltProfiles($Left,$Right){
    $keys=@($Left.PSObject.Properties.Name|Sort-Object)
    if((Json $keys) -ne (Json @($Right.PSObject.Properties.Name|Sort-Object))){return $false}
    foreach($key in $keys){
        if((Json $Left.($key).axes) -ne (Json $Right.($key).axes) -or !(Same-Curve $Left.($key).response $Right.($key).response)){return $false}
    }
    $true
}
function History{
    $state=(Model).state
    Json @($state.document_file.revision,$state.document_file.modified,(@($state.commands|Where-Object id -eq 'undo')[0].enabled),(@($state.commands|Where-Object id -eq 'redo')[0].enabled))
}
function Artwork{
    $state=(Model).state
    Json @($state.camera,$state.layers,$state.layer_tools,$state.workspace,$state.document_file,
        @($state.commands|Where-Object {$_.id -in @('undo','redo','deselect')}|Select-Object id,label,enabled))
}
function Evidence([string]$Name,$Value){[IO.File]::WriteAllText((Join-Path $run ($Name+'.json')),($Value|ConvertTo-Json -Depth 50))}
function Same-Bounds($Left,$Right){
    foreach($field in @('x','y','width','height')){if([Math]::Abs($Left.$field-$Right.$field) -gt .5){return $false}}
    $true
}
function Untouched([string]$Stage,[string]$Before,[int[]]$Paper,[string]$Pixels){
    if((Artwork) -ne $Before){throw "$Stage changed artwork, selection commands, camera or workspace"}
    if((Wait-StablePixels {Screen-Pixels $Paper[0] $Paper[1]}) -ne $Pixels){throw "$Stage changed the exact exposed-paper sample"}
}
function Tilt-Parent($Peer){[System.Windows.Automation.TreeWalker]::ControlViewWalker.GetParent($Peer)}
function Tilt-ContactDown{[CapyRowPointer]::Active}
function Tilt-Scroll($Peer){
    $node=Tilt-Parent $Peer
    while($node){
        if($node.Current.ProcessId-ne$review.Id){throw 'Calibration scroll ancestor is foreign'}
        $pattern=$null
        if($node.TryGetCurrentPattern([System.Windows.Automation.ScrollPattern]::Pattern,[ref]$pattern)){
            return [pscustomobject]@{node=$node;pattern=$pattern}
        }
        if(($node.GetRuntimeId()-join',')-eq($root.GetRuntimeId()-join',')){break}
        $node=Tilt-Parent $node
    }
    throw 'Calibration body exposes no native ScrollPattern'
}
function Tilt-Contained($Peer,$Body){
    $bounds=$Peer.Current.BoundingRectangle;$viewport=$Body.Current.BoundingRectangle
    foreach($value in @($bounds.X,$bounds.Y,$bounds.Width,$bounds.Height,$viewport.X,$viewport.Y,$viewport.Width,$viewport.Height)){
        if([double]::IsNaN($value)-or[double]::IsInfinity($value)){throw 'Calibration has non-finite native bounds'}
    }
    !$Peer.Current.IsOffscreen -and !$Body.Current.IsOffscreen -and !$bounds.IsEmpty -and $bounds.Width-gt0 -and $bounds.Height-gt0 -and !$viewport.IsEmpty -and $viewport.Width-gt0 -and $viewport.Height-gt0 -and $viewport.Contains($bounds)
}
function Reveal-Tilt([string]$Id,[switch]$NoScroll){
    $revealClock=[Diagnostics.Stopwatch]::StartNew()
    $peer=Control $Id -NoPopups
    if($root.Current.ProcessId-ne$review.Id -or $peer.Current.ProcessId-ne$review.Id){throw 'Calibration target does not belong to the owned root'}
    if($peer.Current.AutomationId-cne$Id){throw 'Calibration peer has the wrong AutomationId'}
    if($Id-notmatch'^pen-(tilt|pressure)-(curve|firmer|lighter|direction|sensitivity-title|direction-title|reverse-horizontal|reverse-vertical|swap-axes)$'){
        return (Control $Id -Arranged -NoPopups)
    }
    $runtime=$peer.GetRuntimeId();$scroll=Tilt-Scroll $peer
    $group=if($Id-match'-curve$'){'top'}else{'bottom'}
    $percent=if($group-eq'top'){0}else{100}
    $bounds=$peer.Current.BoundingRectangle;$viewport=$scroll.node.Current.BoundingRectangle
    $trace=[ordered]@{id=$Id;runtime_id=$runtime;group=$group;no_scroll=[bool]$NoScroll;before_offscreen=$peer.Current.IsOffscreen;before_bounds=@($bounds.X,$bounds.Y,$bounds.Width,$bounds.Height);viewport=@($viewport.X,$viewport.Y,$viewport.Width,$viewport.Height);before_percent=$scroll.pattern.Current.VerticalScrollPercent;passed=$false}
    try{
        if($NoScroll -or (Tilt-ContactDown)){
            if(!(Tilt-Contained $peer $scroll.node)){throw 'Calibration target must be revealed before native contact or wheel validation'}
        }elseif($scroll.pattern.Current.VerticallyScrollable){
            if($peer.Current.IsOffscreen){
                $item=$null
                if($peer.TryGetCurrentPattern([System.Windows.Automation.ScrollItemPattern]::Pattern,[ref]$item)){$item.ScrollIntoView()}
            }
            if($scroll.pattern.Current.VerticalScrollPercent-ne$percent){$scroll.pattern.SetScrollPercent([System.Windows.Automation.ScrollPattern]::NoScroll,$percent)}
            Wait-Until {
                if($peer.Current.ProcessId-ne$review.Id -or $scroll.node.Current.ProcessId-ne$review.Id){throw 'Calibration owner changed while scrolling'}
                $scroll.pattern.Current.VerticalScrollPercent-eq$percent -and (Tilt-Contained $peer $scroll.node)
            } 'Native calibration scroll did not reveal its finite body target'
        }elseif(!(Tilt-Contained $peer $scroll.node)){throw 'Calibration target is clipped and native scrolling is unavailable'}
        $arranged=Control $Id -Arranged -NoPopups -Within $scroll.node
        if(($arranged.GetRuntimeId()-join',')-ne($runtime-join',')){throw 'Scrolling replaced the retained calibration peer'}
        if($arranged.Current.ProcessId-ne$review.Id -or $scroll.node.Current.ProcessId-ne$review.Id -or !(Tilt-Contained $arranged $scroll.node)){throw 'Arranged calibration target escaped its owned finite viewport'}
        $after=$arranged.Current.BoundingRectangle
        $trace.after_bounds=@($after.X,$after.Y,$after.Width,$after.Height)
        $trace.after_percent=$scroll.pattern.Current.VerticalScrollPercent;$trace.after_offscreen=$arranged.Current.IsOffscreen;$trace.passed=$true
        $arranged
    }finally{
        if(!$script:tiltReveals){$script:tiltReveals=[Collections.Generic.List[object]]::new()}
        $trace.elapsedReveal_ms=$revealClock.Elapsed.TotalMilliseconds
        $script:tiltReveals.Add($trace);$evidenceClock=[Diagnostics.Stopwatch]::StartNew()
        Evidence 'native-body-reveals' @($script:tiltReveals)
        $evidenceClock.Stop();[IO.File]::AppendAllText((Join-Path $run 'native-body-reveal-costs.jsonl'),((@{utc=[DateTime]::UtcNow.ToString('o');id=$Id;passed=$trace.passed;elapsedReveal_ms=$trace.elapsedReveal_ms;evidence_serialization_write_ms=$evidenceClock.Elapsed.TotalMilliseconds}|ConvertTo-Json -Compress)+[Environment]::NewLine),[Text.UTF8Encoding]::new($false))
    }
}

function Center([string]$Id,[switch]$NoReveal){$r=(Reveal-Tilt $Id -NoScroll:$NoReveal).Current.BoundingRectangle;@([int]($r.Left+$r.Width/2),[int]($r.Top+$r.Height/2))}
function Native-Click([string]$Id,[string]$Device='mouse',[switch]$NoReveal){
    $at=Center $Id -NoReveal:$NoReveal
    [CapyRowPointer]::Down($Device,$at[0],$at[1]);[CapyRowPointer]::Up()
}
function Start-Review([switch]$Maximized){
    $script:launch++
    $script:stderr=Join-Path $run "stderr-$launch.log"
    $script:review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
    $null=$review.Handle;$owned=@{window=$null}
    Wait-Until {$owned.window=Owned-DrawingWindow $review;$null -ne $owned.window -and (Model).brush_ready} 'Tilt review did not start' 45
    $script:root=$owned.window.Root;$script:drawingWindow=$owned.window.Handle
    if($Maximized){Maximize-DrawingWindow}
    Use-PaintWorkspace
    if(!(Model).windows_isolated_settings){throw 'Tilt review requires isolated preferences'}
    [CapyRowPointer]::SetForegroundWindow($drawingWindow)|Out-Null
    [CapyRowPointer]::Initialize([uint32]$review.Id)
    Control 'canvas-view-info' -Arranged|Out-Null;Fit-Canvas
}
function Close-Review{
    [CapyCanvasTouch]::Dispose()
    [CapyRowPointer]::Dispose()
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Close -DiscardUnsaved
    if((Get-Item -LiteralPath $stderr).Length){throw "Tilt native stderr requires inspection: $stderr"}
}
function Search-Command([string]$Id){
    $command=@((Model).state.commands|Where-Object id -eq $Id)[0]
    (Control 'drawing-canvas' -Arranged).SetFocus()
    [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),0x4B)
    Wait-Until {Find 'command-search' -Visible} 'Command search did not open'
    (Control 'command-search').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($command.label)
    Wait-Until {$row=Find 'command-result-0' -Visible;$row -and $row.Current.Name.StartsWith($command.label)} "Tool search did not find $Id"
    [CapyRowPointer]::Key([uint32]$review.Id,0x0D)
    Wait-Until {!(Find 'command-search' -Visible)} "Command search did not close after $Id"
}
function Tool([string]$Id){Search-Command $Id;Wait-Until {@((Model).state.commands|Where-Object {$_.id -eq $Id -and $_.selected}).Count -eq 1} "Tool did not activate: $Id"}
function Open-Tilt{
    if(Find 'settings-button' -Visible){Invoke-Id 'settings-button'}else{
        $caption=@((Model).application_menus|Where-Object id -eq 'edit')[0].label
        if(!$caption){throw 'Current shared application menu is missing: edit'}
        & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'edit' -Caption $caption
        Invoke-Id 'settings'
    }
    Wait-Until {(Model).state.settings_open} 'Preferences action was not acknowledged'
    Control 'Preferences' -Name -Type ([System.Windows.Automation.ControlType]::Window)|Out-Null
    Invoke 'Pen & Input' -Name
    $adjust=Control 'setting-action-pen_tilt';$scroll=$null
    if($adjust.TryGetCurrentPattern([System.Windows.Automation.ScrollItemPattern]::Pattern,[ref]$scroll)){$scroll.ScrollIntoView()}
    Invoke 'setting-action-pen_tilt'
    Wait-Until {(Tilt) -and !(Find 'Preferences' -Name -Type ([System.Windows.Automation.ControlType]::Window))} 'Adjust did not replace Preferences with Pen tilt'
    Control 'pen-tilt-dialog' -Arranged|Out-Null
    Reveal-Tilt 'pen-tilt-curve'|Out-Null
}
function At([double]$X,[double]$Y){
    $r=(Reveal-Tilt 'pen-tilt-curve').Current.BoundingRectangle
    $inset=(Tilt).editor.controls.inset*$script:scale
    @([int]($r.Left+$inset+$X*($r.Width-2*$inset)),[int]($r.Top+$inset+(1-$Y)*($r.Height-2*$inset)))
}
function Drag([string]$Device,[int[]]$From,[int[]]$To,[switch]$Cancel){
    [CapyRowPointer]::Down($Device,$From[0],$From[1])
    try{
        for($i=1;$i -le 8;$i++){[CapyRowPointer]::Move([int]($From[0]+($To[0]-$From[0])*$i/8),[int]($From[1]+($To[1]-$From[1])*$i/8));Start-Sleep -Milliseconds 30}
    }finally{if($Cancel){[CapyRowPointer]::Cancel()}else{[CapyRowPointer]::Up()}}
    $settled=@{curve='';count=0}
    Wait-Until {$value=Curve;if($value -eq $settled.curve){$settled.count++}else{$settled.curve=$value;$settled.count=0};$settled.count -ge 3} 'The released native curve did not settle'
}
function Canvas-Point{Clear-CanvasPoint 'pen-tilt-dialog'}
function Expected-TiltDrag($Before,$Native,[double[]]$Viewport,[int[]]$From,[int[]]$To,[double]$Scale){
    $values=@($Before.x,$Before.y,$Before.width,$Before.height,$Native.Left,$Native.Top,$Native.Width,$Native.Height,$Scale)+@($Viewport)+@($From)+@($To)
    foreach($value in $values){if([double]::IsNaN($value)-or[double]::IsInfinity($value)){throw 'Tilt drag expectation requires finite geometry'}}
    if($Viewport.Count-ne2-or$From.Count-ne2-or$To.Count-ne2-or$Scale-le0-or$Before.width-le0-or$Before.height-le0-or$Native.Width-le0-or$Native.Height-le0-or$Viewport[0]-le0-or$Viewport[1]-le0){throw 'Tilt drag expectation requires an arranged extent and viewport'}
    $dx=($To[0]-$From[0])/$Scale;$dy=($To[1]-$From[1])/$Scale
    $width=[Math]::Min($Before.width,$Viewport[0]);$height=[Math]::Min($Before.height,$Viewport[1])
    $x=[Math]::Max(0.,[Math]::Min($Before.x+$dx,$Viewport[0]-$width))
    $y=[Math]::Max(0.,[Math]::Min($Before.y+$dy,$Viewport[1]-$height))
    $origin=@(($Native.Left-$Before.x*$Scale),($Native.Top-$Before.y*$Scale))
    [pscustomobject]@{requested_delta=@($dx,$dy);shared=[pscustomobject]@{x=$x;y=$y;width=$width;height=$height};native=[Windows.Rect]::new($origin[0]+$x*$Scale,$origin[1]+$y*$Scale,$width*$Scale,$height*$Scale);native_origin=$origin;scale=$Scale}
}
function Tilt-DragMatches($Expected,$Actual,$Native){
    foreach($field in @('x','y','width','height')){if($null-eq$Actual.$field){return $false};$delta=[Math]::Abs($Actual.$field-$Expected.shared.$field);if([double]::IsNaN($delta)-or[double]::IsInfinity($delta)-or$delta-ge1){return $false}}
    foreach($field in @('Left','Top','Width','Height')){if($null-eq$Native.$field){return $false};$delta=[Math]::Abs($Native.$field-$Expected.native.$field);if([double]::IsNaN($delta)-or[double]::IsInfinity($delta)-or$delta-ge2){return $false}}
    $true
}

function Assert-Geometry{
    $script:scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
    $frame=(Control 'pen-tilt-dialog' -Arranged).Current.BoundingRectangle
    $plot=(Reveal-Tilt 'pen-tilt-curve').Current.BoundingRectangle
    $preview=(Reveal-Tilt 'pen-tilt-direction').Current.BoundingRectangle
    if($frame.Width/$scale -lt 383 -or [Math]::Abs($plot.Width-$plot.Height) -gt $scale -or [Math]::Abs(($plot.Left+$plot.Right-$frame.Left-$frame.Right)/2) -gt $scale){throw 'Tilt plot is not square and centered in its minimum-width utility'}
    if([Math]::Abs($preview.Width/$scale-72) -gt 1 -or [Math]::Abs($preview.Height/$scale-72) -gt 1 -or [Math]::Abs(($plot.Right-$preview.Right)/$scale-8) -gt 1 -or [Math]::Abs(($plot.Bottom-$preview.Bottom)/$scale-8) -gt 1){throw 'Tilt preview is not 72px with an 8px inset inside the lower-right plot corner'}
    $controls=@{}
    foreach($id in @('firmer','lighter','reset','cancel','apply','reverse-horizontal','reverse-vertical','swap-axes')){
        $r=(Reveal-Tilt "pen-tilt-$id").Current.BoundingRectangle
        if([Math]::Abs($r.Height/$scale-36) -gt 1){throw "Tilt $id is not tile-height: $r at scale $scale"}
        if($id -in @('firmer','lighter','reverse-horizontal','reverse-vertical','swap-axes') -and [Math]::Abs($r.Width/$scale-36) -gt 1){throw "Tilt $id is not a square tile segment"}
        $controls[$id]=@($r.X,$r.Y,$r.Width,$r.Height)
    }
    $reset=(Control 'pen-tilt-reset' -Arranged).Current.BoundingRectangle;$cancel=(Control 'pen-tilt-cancel' -Arranged).Current.BoundingRectangle;$apply=(Control 'pen-tilt-apply' -Arranged).Current.BoundingRectangle
    if([Math]::Abs(($reset.Left-$frame.Left)/$scale-16) -gt 1 -or [Math]::Abs(($frame.Right-$apply.Right)/$scale-16) -gt 1 -or [Math]::Abs(($apply.Left-$cancel.Right)/$scale-8) -gt 1){throw 'Tilt footer does not preserve 16px gutters and 8px action spacing'}
    $first=(Reveal-Tilt 'pen-tilt-reverse-horizontal').Current.BoundingRectangle;$last=(Reveal-Tilt 'pen-tilt-swap-axes').Current.BoundingRectangle
    $firmer=(Reveal-Tilt 'pen-tilt-firmer').Current.BoundingRectangle;$lighter=(Reveal-Tilt 'pen-tilt-lighter').Current.BoundingRectangle
    if([Math]::Abs($first.Top-$firmer.Top) -gt $scale -or [Math]::Abs($last.Top-$lighter.Top) -gt $scale -or $first.Left -le $lighter.Right -or $firmer.Top -lt $plot.Bottom){throw 'Tilt sensitivity and direction controls do not share one row below the graph'}
    $titles=@{}
    foreach($id in @('sensitivity-title','direction-title')){
        $title=Reveal-Tilt "pen-tilt-$id";$r=$title.Current.BoundingRectangle
        $titles[$id]=@($r.X,$r.Y,$r.Width,$r.Height)
        $pattern=$null
        if($title.TryGetCurrentPattern([System.Windows.Automation.TextPattern]::Pattern,[ref]$pattern)){
            $text=$pattern.DocumentRange.GetBoundingRectangles()
            if($text.Count -eq 4 -and [Math]::Abs($r.Width-$text[2]) -gt 2*$scale){throw "Tilt $id reserves space beyond its text"}
        }
    }
    $sensitivity=(Reveal-Tilt 'pen-tilt-sensitivity-title').Current.BoundingRectangle;$direction=(Reveal-Tilt 'pen-tilt-direction-title').Current.BoundingRectangle
    if([Math]::Abs(($firmer.Left-$sensitivity.Right)/$scale-4) -gt 1 -or [Math]::Abs(($first.Left-$direction.Right)/$scale-4) -gt 1){throw 'Tilt labels do not sit 4px before their button bars'}
    if([Math]::Abs(($direction.Left-$lighter.Right)/$scale-8) -gt 1 -or [Math]::Abs(($sensitivity.Left+$last.Right-$frame.Left-$frame.Right)/2) -gt $scale){throw 'Tilt control groups do not share an 8px gap in a centered natural-width row'}
    @{theme=$Theme;scale=$scale;controls=$controls;titles=$titles;frame=@($frame.X,$frame.Y,$frame.Width,$frame.Height);plot=@($plot.X,$plot.Y,$plot.Width,$plot.Height);preview=@($preview.X,$preview.Y,$preview.Width,$preview.Height)}|ConvertTo-Json -Depth 8|Set-Content (Join-Path $run 'geometry.json')
}
function Preview([string]$Stage,[string]$ExpectedCurve=''){
    $history=History;$artwork=Artwork;$draft=if($ExpectedCurve){$ExpectedCurve}else{Curve};$paperPoint=Canvas-Point
    $pixels=Wait-StablePixels {Screen-Pixels $paperPoint[0] $paperPoint[1]}
    $circle=Center 'pen-tilt-direction'
    [CapyRowPointer]::PenTilt(40,0);[CapyRowPointer]::PenHover($circle[0],$circle[1])
    Wait-Until {$tilt=Tilt;$tilt.ready -and $tilt.input_direction -and [Math]::Abs($tilt.input_direction[0]-40./90.) -lt .015} "$Stage circle hover did not sense native tilt"
    if((Curve) -ne $draft){throw "$Stage first hover did not preserve or load the expected device response"}
    [CapyRowPointer]::PenTilt(0,60);[CapyRowPointer]::Down('pen',$circle[0],$circle[1])
    try{
        [CapyRowPointer]::Move($circle[0]+4,$circle[1]+4)
        Wait-Until {$tilt=Tilt;$tilt.input_direction -and [Math]::Abs($tilt.input_direction[1]-2./3.) -lt .015 -and $tilt.editor.marker} "$Stage circle contact did not update the live tilt marker"
        Capture "$Stage-circle-contact" -WithModel
    }finally{[CapyRowPointer]::Up();[CapyRowPointer]::PenLeave()}
    $frame=(Control 'pen-tilt-dialog' -Arranged).Current.BoundingRectangle
    [CapyRowPointer]::PenTilt(-30,0);[CapyRowPointer]::PenHover([int]($frame.Left+90*$scale),[int]($frame.Top+24*$scale))
    Wait-Until {$tilt=Tilt;$tilt.input_direction -and [Math]::Abs($tilt.input_direction[0]+1./3.) -lt .015} "$Stage dialog header hover did not sense tilt"
    [CapyRowPointer]::PenLeave()
    $preview=(Reveal-Tilt 'pen-tilt-direction').Current.BoundingRectangle
    $gap=@([int]($frame.Left+8*$scale),[int]($preview.Top+$preview.Height/2))
    [CapyRowPointer]::PenTilt(0,-40);[CapyRowPointer]::PenHover($gap[0],$gap[1]);[CapyRowPointer]::Down('pen',$gap[0],$gap[1])
    try{
        Wait-Until {$tilt=Tilt;$tilt.input_direction -and [Math]::Abs($tilt.input_direction[1]+40./90.) -lt .015} "$Stage opaque body gutter did not sense native contact"
        Capture "$Stage-body-contact" -WithModel
    }finally{[CapyRowPointer]::Up();[CapyRowPointer]::PenLeave()}
    if((Wait-StablePixels {Screen-Pixels $paperPoint[0] $paperPoint[1]}) -ne $pixels){throw "$Stage sensor-only input changed the exact canvas sample"}
    if((History) -ne $history -or (Curve) -ne $draft){throw "$Stage sensor-only input changed artwork history or the curve draft"}
    if((Artwork) -ne $artwork){throw "$Stage sensor-only input changed camera, selection commands, layers or workspace"}
}
function Set-Axes([int]$Mask,[string]$Device='mouse'){
    $names=@('reverse_horizontal','reverse_vertical','swap');$ids=@('reverse-horizontal','reverse-vertical','swap-axes')
    for($axis=0;$axis -lt 3;$axis++){
        $expected=($Mask -band (1 -shl $axis)) -ne 0
        if([bool]((Tilt).axes.($names[$axis])) -ne $expected){
            Native-Click "pen-tilt-$($ids[$axis])" $Device
            Wait-Until {[bool]((Tilt).axes.($names[$axis])) -eq $expected} 'Independent tilt segment did not publish its toggle'
        }
    }
}
function Cold-Controls{
    Start-Review -Maximized;Tool 'lasso';Open-Tilt;Assert-Geometry
    $saved=Saved;$neutral=Curve;$before=Artwork;$paper=Canvas-Point;$pixels=Wait-StablePixels {Screen-Pixels $paper[0] $paper[1]}
    foreach($id in @('curve','firmer','lighter','reset','reverse-horizontal','reverse-vertical','swap-axes')){
        if(!(Reveal-Tilt "pen-tilt-$id").Current.IsEnabled){throw "Cold $id is disabled before pen identification"}
    }
    Drag 'mouse' (At .4 .4) (At .55 .65);Set-Axes 7
    if((Tilt).ready -or (Control 'pen-tilt-apply').Current.IsEnabled -or @(Points).Count -ne 3){throw 'Cold curve/axes editing incorrectly requires or identifies a pen'}
    Native-Click 'pen-tilt-reset'
    Wait-Until {(Curve) -eq $neutral -and !(Tilt).axes.reverse_horizontal -and !(Tilt).axes.reverse_vertical -and !(Tilt).axes.swap} 'Cold Reset did not restore neutral response and directions'
    Drag 'touch' (At .4 .4) (At .55 .65);Set-Axes 3
    $draft=Curve;$axes=Json (Tilt).axes;$bounds=(Tilt).bounds
    [CapyRowPointer]::PenTilt(30,20);$at=Center 'pen-tilt-direction';[CapyRowPointer]::PenHover($at[0],$at[1])
    Wait-Until {(Tilt).ready -and (Tilt).input_direction} 'Cold draft did not bind on native utility hover'
    [CapyRowPointer]::PenLeave()
    if((Curve) -ne $draft -or (Json (Tilt).axes) -ne $axes -or !(Same-Bounds (Tilt).bounds $bounds)){throw 'First identity replaced cold response, directions or placement'}
    Untouched 'Cold controls and utility binding' $before $paper $pixels
    Evidence 'cold-controls' @{saved=$saved;draft=(Points);axes=(Tilt).axes;bounds=(Tilt).bounds;artwork=$before;pixels=$pixels}
    Native-Click 'pen-tilt-cancel';Wait-Until {$null -eq (Tilt)} 'Cold draft Cancel did not close'
    if((Saved) -ne $saved){throw 'Cold draft Cancel changed persisted profiles'}
    Close-Review
}
function First-CurveContact{
    Start-Review -Maximized;Tool 'lasso';Open-Tilt;Assert-Geometry
    if((Tilt).ready){throw 'Direct first-contact case already knows a pen'}
    $saved=Saved;$neutral=Curve;$before=Artwork;$paper=Canvas-Point;$pixels=Wait-StablePixels {Screen-Pixels $paper[0] $paper[1]}
    $bounds=(Tilt).bounds;$epoch=(Tilt).editor.controls.epoch;$owner=Json (Reveal-Tilt 'pen-tilt-curve').GetRuntimeId()
    $from=At .4 .4;$to=At .58 .68
    [CapyRowPointer]::PenTilt(40,20);[CapyRowPointer]::Down('pen',$from[0],$from[1])
    try{
        Wait-Until {@(Points).Count -eq 3} 'The first penDown did not add a native curve control'
        if((Tilt).editor.controls.epoch -ne $epoch -or (Json (Control 'pen-tilt-curve').GetRuntimeId()) -ne $owner -or !(Same-Bounds (Tilt).bounds $bounds)){throw 'First graph contact replaced its native curve owner, epoch or placement'}
        [CapyRowPointer]::Move($to[0],$to[1])
        Wait-Until {[Math]::Abs((Points)[1][0]-.58) -lt .02 -and [Math]::Abs((Points)[1][1]-.68) -lt .02} 'The identifying pen contact lost its captured curve before Move'
        Evidence 'first-curve-down' @{epoch=$epoch;owner=$owner;bounds=(Tilt).bounds;points=(Points);ready=(Tilt).ready}
        Capture 'first-curve-down' -WithModel -Composed
    }finally{[CapyRowPointer]::Cancel()}
    Wait-Until {(Curve) -eq $neutral -and (Tilt).ready} 'Cancel did not restore the first identifying curve gesture'
    Untouched 'First graph pen contact' $before $paper $pixels
    Native-Click 'pen-tilt-cancel';Wait-Until {$null -eq (Tilt)} 'First-contact draft Cancel did not close'
    if((Saved) -ne $saved){throw 'First-contact cancellation changed persisted profiles'}
    Close-Review
}
function Deferred-CurveContact([switch]$PriorEdit){
    Start-Review -Maximized;Tool 'lasso';Open-Tilt;Assert-Geometry
    [CapyCanvasTouch]::Initialize([uint32]$review.Id)
    if((Tilt).ready){throw 'Deferred first-contact case already knows a pen'}
    if($PriorEdit){Native-Click 'pen-tilt-lighter';Set-Axes 1}
    $stage=if($PriorEdit){'deferred-prior-edit'}else{'deferred-neutral'}
    $saved=Saved;$draft=Curve;$axes=Json (Tilt).axes;$before=Artwork;$paper=Canvas-Point;$pixels=Wait-StablePixels {Screen-Pixels $paper[0] $paper[1]}
    $bounds=(Tilt).bounds;$epoch=(Tilt).editor.controls.epoch;$owner=Json (Control 'pen-tilt-curve').GetRuntimeId()
    $from=At .4 .4;$to=At .5 .6;$next=At .6 .7
    $at=Center 'pen-tilt-direction' -NoReveal
    [CapyCanvasTouch]::Down(1,$from[0],$from[1])
    try{
        [CapyCanvasTouch]::Move(1,$to[0],$to[1]);Wait-Until {@(Points).Count -eq 3 -and [Math]::Abs((Points)[1][0]-.5) -lt .02} 'Unbound touch did not capture and edit its curve'
        $heldDirection=Center 'pen-tilt-direction' -NoReveal
        if((Json $heldDirection)-cne(Json $at)){throw 'Held touch changed the arranged direction target'}
        [CapyRowPointer]::PenTilt(40,20);[CapyRowPointer]::PenHover($at[0],$at[1]);[CapyRowPointer]::PenLeave()
        [CapyCanvasTouch]::Move(1,$next[0],$next[1])
        Wait-Until {[Math]::Abs((Points)[1][0]-.6) -lt .02 -and [Math]::Abs((Points)[1][1]-.7) -lt .02} 'First pen sensing interrupted the existing touch curve capture'
        if((Tilt).ready -or (Tilt).editor.controls.epoch -ne $epoch -or (Json (Control 'pen-tilt-curve').GetRuntimeId()) -ne $owner -or !(Same-Bounds (Tilt).bounds $bounds)){throw 'First binding did not defer while the existing touch curve gesture remained active'}
        Evidence ($stage+'-held') @{epoch=$epoch;owner=$owner;points=(Points);bounds=(Tilt).bounds;ready=(Tilt).ready;prior_edit=[bool]$PriorEdit}
        Capture ($stage+'-held') -WithModel -Composed
    }finally{[CapyCanvasTouch]::CancelAll();[CapyRowPointer]::Cancel()}
    Wait-Until {(Tilt).ready -and (Curve) -eq $draft -and (Json (Tilt).axes) -eq $axes} 'Cancel did not bind the sensed pen and restore prior unbound edit intent'
    if((Tilt).editor.controls.epoch -ne $epoch -or !(Same-Bounds (Tilt).bounds $bounds)){throw 'Deferred binding changed the editor epoch or manual placement'}
    Untouched $stage $before $paper $pixels
    Evidence ($stage+'-cancelled') @{epoch=(Tilt).editor.controls.epoch;points=(Points);axes=(Tilt).axes;bounds=(Tilt).bounds;ready=(Tilt).ready;artwork=$before;pixels=$pixels}
    Native-Click 'pen-tilt-cancel';Wait-Until {$null -eq (Tilt)} 'Deferred draft Cancel did not close'
    if((Saved) -ne $saved){throw 'Deferred curve cancellation changed persisted profiles'}
    Close-Review
}
function Response-Switching{
    Start-Review -Maximized;Tool 'lasso';Open-Tilt;Assert-Geometry
    $saved=Saved;$neutral=Curve;$pressureSaved=Json (Model).state.settings.pressure_curve
    $tiltPlot=(Reveal-Tilt 'pen-tilt-curve').Current.BoundingRectangle;$oldOwner=Json (Control 'pen-tilt-curve').GetRuntimeId()
    Drag 'mouse' (At .4 .4) (At .55 .65);Set-Axes 7
    Search-Command 'pen_pressure'
    Wait-Until {$null -eq (Tilt) -and (Model).state.pressure_calibration -and (Find 'pen-pressure-dialog' -Visible)} 'Pressure did not replace the live Tilt utility'
    $pressurePlot=(Reveal-Tilt 'pen-pressure-curve').Current.BoundingRectangle
    if([Math]::Abs($pressurePlot.Width-$tiltPlot.Width) -gt $scale -or [Math]::Abs($pressurePlot.Height-$tiltPlot.Height) -gt $scale){throw 'Pressure and Tilt do not retain matching square plots'}
    foreach($kind in @('pressure','tilt')){
        if($kind -eq 'tilt'){
            Native-Click 'pen-pressure-lighter';Search-Command 'pen_tilt'
            Wait-Until {$null -eq (Model).state.pressure_calibration -and (Tilt) -and (Find 'pen-tilt-dialog' -Visible)} 'Tilt did not replace the live Pressure utility'
            if((Curve) -ne $neutral -or (Tilt).axes.reverse_horizontal -or (Tilt).axes.reverse_vertical -or (Tilt).axes.swap -or (Json (Control 'pen-tilt-curve').GetRuntimeId()) -eq $oldOwner){throw 'Switching response kinds retained a retired curve owner or unapplied draft'}
        }
        $title=(Control "pen-$kind-title" -Arranged).Current.BoundingRectangle;$close=(Control "pen-$kind-close" -Arranged).Current.BoundingRectangle
        if([Math]::Abs(($title.Top+$title.Height/2)-($close.Top+$close.Height/2)) -gt $scale){throw "$kind title and Close do not share a centerline"}
    }
    if((Saved) -ne $saved -or (Json (Model).state.settings.pressure_curve) -ne $pressureSaved){throw 'Switching response kinds committed a retired draft'}
    Evidence 'response-switching' @{tilt_plot=$tiltPlot.ToString();pressure_plot=$pressurePlot.ToString();retired_tilt_owner=$oldOwner;new_tilt_owner=(Control 'pen-tilt-curve').GetRuntimeId();saved=$saved}
    Capture 'response-switching' -WithModel -Composed
    Native-Click 'pen-tilt-close';Close-Review
}
function Column{ @((Model).layout.collapsed|Where-Object {$_.groups.icons.panel -contains 'layers'})[0] }
function Context([string]$Id){
    (Control $Id -Arranged).SetFocus();[CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x10),0x79)
    Control 'Collapse column' -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)|Out-Null
}
function Screen-Bounds($Bounds){
    $origin=[CapyRowPointer+Point]::new()
    if(![CapyRowPointer]::ClientToScreen($drawingWindow,[ref]$origin)){throw 'Owned native client origin is unavailable'}
    [Windows.Rect]::new($origin.x+$Bounds.x*$scale,$origin.y+$Bounds.y*$scale,$Bounds.width*$scale,$Bounds.height*$scale)
}
function Move-Utility([double]$X,[double]$Y){
    $frame=(Control 'pen-tilt-dialog' -Arranged).Current.BoundingRectangle
    $from=@([int]($frame.Left+90*$scale),[int]($frame.Top+24*$scale));$to=@([int]($from[0]+$X-$frame.Left),[int]($from[1]+$Y-$frame.Top))
    Drag 'mouse' $from $to
    Wait-Until {$actual=(Control 'pen-tilt-dialog' -Arranged).Current.BoundingRectangle;[Math]::Abs($actual.Left-$X) -lt 2 -and [Math]::Abs($actual.Top-$Y) -lt 2} 'Utility did not reach its native overlap target'
}
function Park-UtilityCapture{
    $window=$root.Current.BoundingRectangle
    [CapyRowPointer]::Hover([int]($window.Left+$window.Width/2),[int]($window.Top+42*$scale))
    $tooltip=[System.Windows.Automation.AndCondition]::new(
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::ToolTip),
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty,$review.Id))
    Wait-Until {!@([System.Windows.Automation.AutomationElement]::RootElement.FindAll([System.Windows.Automation.TreeScope]::Descendants,$tooltip)|Where-Object {!$_.Current.IsOffscreen}).Count} 'Utility tooltip did not leave the capture area'
}
function Utility-Overlays{
    Start-Review -Maximized;Tool 'lasso'
    if(!(Column)){Context 'panel-tab-layers';Invoke 'Collapse column' -Name;Wait-Until {Column} 'Layers column did not collapse'}
    if(!(Column).open){Invoke 'column-icon-layers';Wait-Until {(Column).open} 'Collapsed Layers column did not expand'}
    Open-Tilt;Assert-Geometry
    if(!(Column).open){throw 'Opening Pen tilt closed the expanded reference column'}
    $initial=(Tilt).bounds;$work=(Model).layout.work_area;$drawer=(Column).open.bounds
    if($work.width -ge $initial.width+32 -and $work.height -ge $initial.height+32){
        if($initial.x -lt $work.x+15 -or $initial.y -lt $work.y+15 -or $initial.x+$initial.width -gt $work.x+$work.width-15 -or $initial.y+$initial.height -gt $work.y+$work.height-15){throw 'Default utility is not inset within the clear canvas'}
    }elseif($initial.x -lt 0 -or $initial.y -lt 0 -or $initial.x+$initial.width -gt (Model).layout.viewport[0]+.5 -or $initial.y+$initial.height -gt (Model).layout.viewport[1]+.5){throw 'Utility larger than clear canvas escaped the window fallback bounds'}
    if($initial.x -lt $drawer.x+$drawer.width -and $initial.x+$initial.width -gt $drawer.x -and $initial.y -lt $drawer.y+$drawer.height -and $initial.y+$initial.height -gt $drawer.y){throw 'Default utility overlaps the expanded column'}
    $frame=(Control 'pen-tilt-dialog' -Arranged).Current.BoundingRectangle;$view=Screen-Bounds @{x=0;y=0;width=(Model).layout.viewport[0];height=(Model).layout.viewport[1]};$column=Screen-Bounds $drawer
    Move-Utility ([Math]::Min($column.Left-80*$scale,$view.Right-$frame.Width-4*$scale)) ([Math]::Max($view.Top,[Math]::Min($column.Top+8*$scale,$view.Bottom-$frame.Height-4*$scale)))
    $frame=(Control 'pen-tilt-dialog' -Arranged).Current.BoundingRectangle;$at=Center 'pen-tilt-lighter'
    if(!$column.Contains([double]$at[0],[double]$at[1])){throw 'The native sensitivity target does not overlap the expanded column'}
    $curve=Curve;Native-Click 'pen-tilt-lighter';Wait-Until {(Curve) -ne $curve} 'The utility did not receive the native click above the expanded column'
    if(!(Column).open){throw 'The utility click fell through and closed the expanded column'}
    Native-Click 'pen-tilt-reset';Wait-Until {(Curve) -eq $curve} 'Reset did not settle before the opaque-pixel checkpoint'
    Park-UtilityCapture
    Capture 'utility-over-column' -WithModel -Composed
    $x=[int]([Math]::Max($frame.Left+60,$column.Left+48));$y=[int]([Math]::Max($frame.Top+60,$column.Top+48))
    $sample=[Windows.Rect]::new($x-48,$y-48,96,96)
    if(!$frame.Contains($sample) -or !$column.Contains($sample)){throw 'The opaque pixel sample does not fit inside both utility and expanded column'}
    $samples=@([pscustomobject]@{kind='column';x=$x;y=$y;pixels=(Wait-StablePixels {Screen-Pixels $x $y (Join-Path $run 'utility-column-before.png')});after_pixels=$null});$bounds=(Tilt).bounds;$connections=(Column).open.connections
    foreach($connection in $connections){
        $link=Screen-Bounds $connection[1].bounds
        if(!$frame.IntersectsWith($link)){continue}
        $cx=[int][Math]::Max($frame.Left+60,[Math]::Min($frame.Right-60,$link.Left+$link.Width/2));$cy=[int][Math]::Max($frame.Top+60,[Math]::Min($frame.Bottom-60,$link.Top+$link.Height/2))
        $crop=[Windows.Rect]::new($cx-48,$cy-48,96,96)
        if(!$frame.Contains($crop) -or !$crop.IntersectsWith($link)){continue}
        $kind='connector-'+$connection[0]
        $samples+=[pscustomobject]@{kind=$kind;x=$cx;y=$cy;bounds=$link.ToString();pixels=(Wait-StablePixels {Screen-Pixels $cx $cy (Join-Path $run ("utility-$kind-before.png"))});after_pixels=$null}
    }
    if($samples.Count -lt 2){throw 'The utility interior does not overlap an expanded column connector sample'}
    Invoke-Id 'column-icon-layers';Wait-Until {!(Column).open} 'The reference column did not close'
    Park-UtilityCapture
    if(!(Same-Bounds (Tilt).bounds $bounds)){throw 'Closing the column moved the manually placed utility'}
    foreach($sample in $samples){$sample.after_pixels=Wait-StablePixels {Screen-Pixels $sample.x $sample.y (Join-Path $run ("utility-$($sample.kind)-after.png"))}}
    Evidence 'utility-over-column' @{default_bounds=$initial;work_area=$work;drawer=$drawer;overlap=$bounds;connections=$connections;samples=$samples}
    foreach($sample in $samples){if($sample.after_pixels -ne $sample.pixels){throw "$($sample.kind) paints through the opaque utility interior"}}
    $window=$root.Current.BoundingRectangle;$frame=(Control 'pen-tilt-dialog' -Arranged).Current.BoundingRectangle
    Move-Utility $frame.Left ($view.Top+4*$scale)
    $at=Center 'pen-tilt-title';$header=(Screen-Bounds @{x=0;y=0;width=(Model).layout.viewport[0];height=$work.y})
    if(!$header.Contains([double]$at[0],[double]$at[1])){throw 'The native utility title does not overlap the header'}
    $frame=(Control 'pen-tilt-dialog' -Arranged).Current.BoundingRectangle;Move-Utility ($frame.Left-20*$scale) ($frame.Top+4*$scale)
    if($root.Current.BoundingRectangle -ne $window){throw 'Dragging the utility over the header moved the OS window'}
    Capture 'utility-over-header' -WithModel -Composed
    Native-Click 'pen-tilt-close';Wait-Until {$null -eq (Tilt)} 'Overlap utility did not close'
    (Control 'column-icon-layers' -Arranged).SetFocus();[CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x10),0x79);Invoke 'Expand column' -Name
    Wait-Until {!(Column) -and (Find 'panel-tab-layers' -Visible)} 'Layers column did not restore'
    Short-Viewport
    Close-Review
}
function Short-Viewport{
    [CapyWindowApi]::ShowWindow($drawingWindow,9)|Out-Null
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Resize -Width ([int](700*$scale)) -Height ([int](480*$scale))
    Wait-Until {$bounds=(Control 'drawing-canvas' -Arranged).Current.BoundingRectangle;$camera=(Model).state.camera;[Math]::Abs($bounds.Width-$camera.viewport[0]) -lt 2 -and [Math]::Abs($bounds.Height-$camera.viewport[1]) -lt 2} 'Short viewport did not acknowledge native bounds'
    Open-Tilt
    $frame=(Control 'pen-tilt-dialog' -Arranged).Current.BoundingRectangle;$footer=(Control 'pen-tilt-apply' -Arranged).Current.BoundingRectangle
    if(!$root.Current.BoundingRectangle.Contains($frame) -or !$frame.Contains($footer)){throw 'Short viewport clipped the utility or its Apply footer'}
    $axis=Control 'pen-tilt-swap-axes';$walker=[System.Windows.Automation.TreeWalker]::ControlViewWalker;$node=$walker.GetParent($axis);$scroll=$null
    while($node){if($node.TryGetCurrentPattern([System.Windows.Automation.ScrollPattern]::Pattern,[ref]$scroll) -and $scroll.Current.VerticallyScrollable){break};$node=$walker.GetParent($node)}
    if(!$node){throw 'Short utility body exposes no native scrolling'}
    $body=$node.Current.BoundingRectangle;$at=@([int]($body.Left+$body.Width/2),[int]($body.Bottom-16*$scale))
    [CapyRowPointer]::Wheel($at[0],$at[1],-1200)
    Wait-Until {$scroll.Current.VerticalScrollPercent -gt 0 -and !$axis.Current.IsOffscreen -and $frame.Contains($axis.Current.BoundingRectangle)} 'Native wheel did not reveal short-viewport direction controls'
    $before=(Tilt).axes.swap;Native-Click 'pen-tilt-swap-axes' -NoReveal;Wait-Until {(Tilt).axes.swap -ne $before} 'The scrolled native direction segment did not toggle'
    Evidence 'short-viewport' @{frame=$frame.ToString();footer=$footer.ToString();body=$body.ToString();scroll=$scroll.Current.VerticalScrollPercent;axis=$axis.Current.BoundingRectangle.ToString()}
    Capture 'short-viewport' -WithModel -Composed
    Native-Click 'pen-tilt-close';Maximize-DrawingWindow
}
$completed=[Collections.Generic.List[string]]::new()
try{
    Enter-CapyEnvironment
    $env:CAPY_STORAGE_DIR=Join-Path $run 'profile'
    [IO.File]::WriteAllText((Settings-File),(@{theme=$Theme;cursor='none';language=@{Explicit='en'}}|ConvertTo-Json -Depth 4))
    $env:CAPY_TRACE_UI='1';$env:CAPY_SMOKE_TEST='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
    [CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
    if($Journey -in @('all','baseline')){
    Start-Review -Maximized;Tool 'lasso';Open-Tilt;Assert-Geometry
    if((Tilt).ready -or (Control 'pen-tilt-apply').Current.IsEnabled){throw 'A cold editor already claims a pen device'}
    $saved=Saved;$curve=Curve;$frame=(Control 'pen-tilt-dialog' -Arranged).Current.BoundingRectangle;$beforeDrag=Model
    $from=@([int]($frame.Left+90*$scale),[int]($frame.Top+24*$scale));$to=@([int]($frame.Left-110*$scale),[int]($frame.Top+44*$scale))
    $expectedDrag=Expected-TiltDrag $beforeDrag.state.tilt_calibration.bounds $frame $beforeDrag.layout.viewport $from $to $scale
    Evidence 'cold-drag-request' @{before=$beforeDrag.state.tilt_calibration.bounds;viewport=$beforeDrag.layout.viewport;from=$from;to=$to;expected=$expectedDrag}
    Drag 'mouse' $from $to
    Wait-Until {Tilt-DragMatches $expectedDrag (Tilt).bounds (Control 'pen-tilt-dialog' -Arranged).Current.BoundingRectangle} 'Cold utility drag did not publish its final native placement'
    Evidence 'cold-drag-acknowledged' @{shared=(Tilt).bounds;native=(Control 'pen-tilt-dialog' -Arranged).Current.BoundingRectangle;expected=$expectedDrag}
    $bounds=(Tilt).bounds;$canvas=Canvas-Point
    $beforePenModel=Model;$canvasNative=(Control 'drawing-canvas' -Arranged).Current.BoundingRectangle;$utilityNative=(Control 'pen-tilt-dialog' -Arranged).Current.BoundingRectangle
    $sampleNative=[Windows.Rect]::new($canvas[0]-48,$canvas[1]-48,96,96)
    @{bounds=$bounds;curve=(Points);camera=$beforePenModel.state.camera;readout=(Control 'canvas-view-info').Current.Name;active_tab=@($beforePenModel.state.tabs|Where-Object active)[0];canvas_native=$canvasNative;utility_native=$utilityNative;point=$canvas;sample_native=$sampleNative;canvas_contains=$canvasNative.Contains($sampleNative);utility_nonintersection=(!$utilityNative.IntersectsWith($sampleNative))}|ConvertTo-Json -Depth 8|Set-Content (Join-Path $run 'first-pen-before.json')
    [CapyRowPointer]::PenTilt(40,0);[CapyRowPointer]::Down('pen',$canvas[0],$canvas[1])
    try{
        Wait-Until {(Tilt).ready -and (Find 'pen-tilt-dialog' -Visible)} 'First Lasso penDown hid the Tilt utility'
        $after=(Tilt).bounds
        @{bounds=$after;curve=(Points)}|ConvertTo-Json -Depth 8|Set-Content (Join-Path $run 'first-pen-after.json')
        if([Math]::Abs($after.x-$bounds.x) -gt .5 -or [Math]::Abs($after.y-$bounds.y) -gt .5 -or [Math]::Abs($after.width-$bounds.width) -gt .5 -or [Math]::Abs($after.height-$bounds.height) -gt .5 -or (Curve) -ne $curve){throw 'First pen identification reset untouched placement or response'}
        Capture 'first-lasso-down' -WithModel
    }finally{[CapyRowPointer]::Up();[CapyRowPointer]::PenLeave()}
    Native-Click 'pen-tilt-lighter'
    Wait-Until {(Curve) -ne $curve} 'Controls stopped responding after first canvas contact'
    Native-Click 'pen-tilt-cancel';Wait-Until {$null -eq (Tilt)} 'Cancel did not close the first utility'
    if((Saved) -ne $saved){throw 'Cancel saved the first unbound response'}
    Close-Review
    Start-Review -Maximized;Tool 'pen'
    Wait-Until {(Model).brush_ready} 'The painting preset was not ready for sensor isolation checks' 45
    Open-Tilt;Assert-Geometry
    if((Tilt).ready){throw 'The restarted cold session identified a pen before input'}
    Preview 'cold'
    $neutral=Curve;$saved=Saved
    for($mask=0;$mask -lt 8;$mask++){
        Set-Axes $mask (@('mouse','touch','pen')[$mask%3])
        $at=Center 'pen-tilt-direction';[CapyRowPointer]::PenTilt(30,20);[CapyRowPointer]::PenHover($at[0],$at[1])
        $pose=@{view=$null}
        Wait-Until {$pose.view=Tilt;$pose.view.input_direction -and $pose.view.adjusted_direction -and $pose.view.input_direction[0] -gt 0 -and $pose.view.input_direction[1] -gt 0} 'The new diagonal native tilt pose was not acknowledged'
        $input=@($pose.view.input_direction);$expected=@($input[0],$input[1])
        if($mask -band 4){$expected=@($input[1],$input[0])}
        if($mask -band 1){$expected[0]=-$expected[0]};if($mask -band 2){$expected[1]=-$expected[1]}
        Wait-Until {$out=(Tilt).adjusted_direction;[Math]::Abs($out[0]-$expected[0]) -lt .002 -and [Math]::Abs($out[1]-$expected[1]) -lt .002} "Axes combination $mask did not transform the live direction"
        [CapyRowPointer]::PenLeave()
    }
    Capture 'axes-combined' -WithModel
    Drag 'mouse' (At .45 .55) (At .55 .65)
    Wait-Until {@(Points).Count -eq 3 -and (Curve) -ne $neutral} 'Tilt graph did not add and edit its shared control'
    $draft=Curve
    @{curve=(Points)}|ConvertTo-Json -Depth 8|Set-Content (Join-Path $run 'curve-before-pen-cancel.json')
    $point=(Points)[1]
    Drag 'pen' (At $point[0] $point[1]) (At .65 .7) -Cancel
    @{curve=(Points)}|ConvertTo-Json -Depth 8|Set-Content (Join-Path $run 'curve-after-pen-cancel.json')
    Wait-Until {(Curve) -eq $draft} 'Native pen cancellation did not restore the Tilt curve'
    Native-Click 'pen-tilt-cancel';Wait-Until {$null -eq (Tilt)} 'Cancel did not close Tilt'
    if((Saved) -ne $saved){throw 'Cancel committed draft curve or axes'}
    Open-Tilt;Drag 'touch' (At .45 .55) (At .55 .65);Set-Axes 7
    Wait-Until {@(Points).Count -eq 3} 'Touch did not edit the Tilt response'
    $applied=Curve;$axes=Json (Tilt).axes
    Native-Click 'pen-tilt-apply';Wait-Until {$null -eq (Tilt) -and (Saved) -ne $saved} 'Apply did not save Tilt calibration'
    $persisted=Saved
    Wait-Until {Same-TiltProfiles (Read-Snapshot (Settings-File)).tilt_calibrations (Model).state.settings.tilt_calibrations} 'Apply did not persist the per-device profile'
    Open-Tilt;Native-Click 'pen-tilt-reset'
    Wait-Until {(Curve) -eq $neutral -and !(Tilt).axes.reverse_horizontal -and !(Tilt).axes.reverse_vertical -and !(Tilt).axes.swap} 'Reset did not restore neutral curve and all three independent axes'
    Native-Click 'pen-tilt-close';Wait-Until {$null -eq (Tilt)} 'Close did not dismiss Tilt'
    Open-Tilt
    if((Curve) -ne $applied -or (Json (Tilt).axes) -ne $axes){throw 'Close committed an unapplied Reset'}
    Native-Click 'pen-tilt-close';Close-Review
    Start-Review -Maximized;Tool 'pen'
    Wait-Until {(Model).brush_ready} 'The painting preset was not ready for sensor isolation checks' 45
    Open-Tilt
    Preview 'persisted' $applied
    if((Saved) -ne $persisted -or (Curve) -ne $applied -or (Json (Tilt).axes) -ne $axes){throw 'Restart did not rebind the same persisted physical-source calibration'}
    Capture 'persisted-profile' -WithModel
    Native-Click 'pen-tilt-close';Close-Review
    $completed.Add('baseline')
    Evidence 'checkpoint-baseline' @{journey='baseline';status='passed'}
    }
    $env:CAPY_STORAGE_DIR=Join-Path $run 'first-contact-profile'
    [IO.File]::WriteAllText((Settings-File),(@{theme=$Theme;cursor='none';language=@{Explicit='en'}}|ConvertTo-Json -Depth 4))
    $journeys=[ordered]@{
        'cold-controls'={Cold-Controls}
        'first-curve'={First-CurveContact}
        'deferred'={Deferred-CurveContact}
        'deferred-prior-edit'={Deferred-CurveContact -PriorEdit}
        'response'={Response-Switching}
        'overlays'={Utility-Overlays}
    }
    foreach($name in $journeys.Keys){
        if($Journey -ne 'all' -and $Journey -ne $name){continue}
        Write-Output "Tilt journey: $name"
        & $journeys[$name]
        $completed.Add($name)
        Evidence ('checkpoint-'+$name) @{journey=$name;status='passed'}
    }
    @{theme=$Theme;journey=$Journey;passed_journeys=@($completed.ToArray());scope='isolated OS-injected input; concurrent mouse-plus-pen, physical pen, rotated hardware and frame performance unverified';run=$run}|ConvertTo-Json
}catch{
    try{Capture 'failure' -WithModel}catch{}
    [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{
    [CapyCanvasTouch]::Dispose()
    [CapyRowPointer]::Dispose()
    Exit-CapyEnvironment
}
