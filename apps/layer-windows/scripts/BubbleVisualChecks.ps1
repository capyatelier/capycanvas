Add-Type -AssemblyName System.Drawing

function New-BubbleHdrDrawing([string]$Depth){
    $script:HdrSdrId=(Model).windows_tabs.selected
    & (Join-Path $CapyScripts 'open-application-menu.ps1') -Root $root -Name 'File'
    Invoke 'new_document';New-DrawingPixels 256 192
    $choice=Control 'document-depth'
    $choice.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    $label=if($Depth -eq 'F32'){'32-bit float HDR'}else{'16-bit float HDR'}
    (Control $label -Name -Type ([System.Windows.Automation.ControlType]::ListItem)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
    Invoke 'Create' -Name
    Wait-Until {$m=Model;$m.brush_ready -and $m.canvas_ready -and !$m.windows_document -and $m.color_panel.hdr -and $m.color_panel.document_depth -eq $Depth} 'Native float drawing did not become ready' 60
    $script:HdrDrawingId=(Model).windows_tabs.selected
}
function Bubble-HdrState {
    $m=Model;$session=@($m.windows_tabs.session_stamps|Where-Object id -eq $m.windows_tabs.selected)[0]
    if(!$session){throw 'HDR fixture lacks its document checkpoint'}
    @{document=$m.windows_tabs.selected;revision=$m.state.document_file.revision;checkpoint=$session.stamp.checkpoint;modified=$m.state.document_file.modified
      paint=([ordered]@{slot=$m.state.colors.slot;paint_slot=$m.state.colors.paint_slot;definition=$m.color_panel.definition;base=$m.color_panel.base;intensity=$m.color_panel.intensity}|ConvertTo-Json -Depth 20 -Compress)
      base=($m.color_panel.base|ConvertTo-Json -Depth 10 -Compress);intensity=$m.color_panel.intensity;alpha=$m.color_panel.definition.rgba[3]}
}
function Assert-BubbleHdrState([string]$Stage,$Before,[switch]$Paint,[switch]$Alpha){
    $after=Bubble-HdrState;$hdrEvidence.Add(@{stage=$Stage;before=$Before;after=$after})
    foreach($field in @('document','revision','checkpoint','modified')){if($after.$field -ne $Before.$field){throw "$Stage changed authored $field"}}
    if($Alpha -and $after.alpha -ne $Before.alpha){throw "$Stage changed authored paint alpha"}
    if($Paint -and $after.paint -ne $Before.paint){throw "$Stage did not restore the exact paint color and intensity"}
}
function Show-BubbleHdrColor{
    $before=Bubble-HdrState
    & (Join-Path $CapyScripts 'open-application-menu.ps1') -Root $root -Name 'Window'
    $item=Control 'Color' -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)
    $toggle=$item.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern)
    if($toggle.Current.ToggleState -eq [System.Windows.Automation.ToggleState]::Off){$toggle.Toggle()}else{Escape}
    $controls=@()
    foreach($id in @('color-readout','color-edit','color-shape-0','color-shape-1','color-foreground','color-background')){
        $element=Control $id -Arranged;$bounds=$element.Current.BoundingRectangle
        $controls+=@{id=$id;bounds=@($bounds.X,$bounds.Y,$bounds.Width,$bounds.Height)}
    }
    $hdrEvidence.Add(@{stage='native-color-panel';controls=$controls;workspace=(Model).windows_workspace.id})
    Assert-BubbleHdrState 'show-native-color-panel' $before -Paint
}
function Bubble-HdrRange([long]$Id){
    (Control "bubble-intensity-$Id" -Arranged).GetCurrentPattern([System.Windows.Automation.RangeValuePattern]::Pattern)
}
function Set-BubbleHdrValue([long]$Id,[double]$Stops){
    $alpha=(Model).color_panel.definition.rgba[3]
    (Bubble-HdrRange $Id).SetValue($Stops)
    Wait-Until {[Math]::Abs((Packet $Id).color.intensity.stops-$Stops) -lt .005 -and [Math]::Abs((Model).color_panel.intensity-$Stops) -lt .005} 'Accessible intensity did not converge with the shared color'
    if((Model).color_panel.definition.rgba[3] -ne $alpha){throw 'Accessible intensity changed authored paint alpha'}
}
function Bubble-HdrPoint([long]$Id,[double]$Fraction){
    $spec=(Packet $Id).color.intensity;$path=$spec.path
    if($path.Count -ne 65){throw 'HDR arc must publish its complete sampled path'}
    $index=[int][Math]::Round([Math]::Max(0.,[Math]::Min(1.,$Fraction))*64);$point=Relative $Id $path[$index]
    $point.stops=[Math]::Round($spec.range[0]+($spec.range[1]-$spec.range[0])*$index/64.,2);$point
}
function Set-BubbleHdrPaint([double]$Stops,[switch]$Base){
    (Control 'color-readout').SetFocus();[CapyRowPointer]::Key([uint32]$review.Id,0x5D)
    Invoke 'edit-color';$null=Control 'edit-color-apply'
    if($Base){
        Invoke 'edit-color-form-0';Invoke 'edit-color-form-0-linear_rgb'
        foreach($entry in @(@('edit-color-0-0','1'),@('edit-color-0-1','0.5'),@('edit-color-0-2','0.25'))){
            Invoke $entry[0];$field=Control ($entry[0]+'-entry');$field.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($entry[1]);$field.SetFocus();[CapyRowPointer]::Key([uint32]$review.Id,0x0D)
        }
    }
    Invoke 'edit-color-intensity';$field=Control 'edit-color-intensity-entry'
    $field.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($Stops.ToString([Globalization.CultureInfo]::InvariantCulture));$field.SetFocus();[CapyRowPointer]::Key([uint32]$review.Id,0x0D)
    Wait-Until {!(Find 'edit-color-intensity-entry') -and (Control 'edit-color-apply').Current.IsEnabled} 'Native HDR intensity draft did not validate'
    Invoke 'edit-color-apply'
    Wait-Until {!(Find 'edit-color-apply') -and [Math]::Abs((Model).color_panel.intensity-$Stops) -lt .005} 'Native HDR paint draft did not commit'
}
function Invoke-BubbleHdrChecks {
    $initial=Bubble-HdrState;Set-BubbleHdrPaint -1 -Base;Assert-BubbleHdrState 'seed-linear-hdr-color' $initial
    $area=(Model).layout.work_area
    $script:color=Take 'color' '' ($area.x+$area.width*.6) ($area.y+$area.height*.5)
    $id=$color
    Wait-Until {(Packet $id).color -and (View $id).color.intensity -and (Find "bubble-intensity-$id" -Visible)} 'HDR bubble did not expose its native intensity control'
    $element=Control "bubble-intensity-$id" -Arranged;$range=Bubble-HdrRange $id;$shared=(Packet $id).color.intensity
    $hdrEvidence.Add(@{stage='accessible-range';name=$element.Current.Name;minimum=$range.Current.Minimum;maximum=$range.Current.Maximum;value=$range.Current.Value;small_change=$range.Current.SmallChange;shared=$shared;depth=$HdrDepth;display=(Model).windows_display})
    if($element.Current.Name -ne $shared.label -or !$element.Current.IsEnabled -or $range.Current.IsReadOnly -or $range.Current.Minimum -ne $shared.range[0] -or $range.Current.Maximum -ne $shared.range[1] -or [Math]::Abs($range.Current.SmallChange-.1) -gt .0001){throw 'Native HDR accessibility differs from the shared intensity contract'}
    if($HdrRetirementOnly){Invoke-BubbleHdrRetirement $id;return}
    if($HdrPreviewOnly){Set-BubbleHdrPaint 12 -Base;Assert-BubbleHdrPickerPreview $id;return}
    foreach($pointer in @('mouse','pen','touch')){
        Set-BubbleHdrPaint -1 -Base
        foreach($ending in @('commit','cancel','hold')){
            Set-BubbleHdrValue $id -1;$before=Bubble-HdrState;$center=Center $id
            $from=Bubble-HdrPoint $id .375;$to=Bubble-HdrPoint $id .625
            [CapyRowPointer]::Down($pointer,$from.x,$from.y)
            try{
                if($ending -eq 'hold'){
                    Wait-Until {$null -ne (Find 'Put back' -Name -Type ([System.Windows.Automation.ControlType]::MenuItem))} 'Holding HDR intensity did not open bubble context after rollback'
                    Assert-BubbleHdrState "$pointer-held-intensity" $before -Paint
                    [CapyRowPointer]::Up();Escape
                }else{
                    [CapyRowPointer]::Move($to.x,$to.y)
                    Wait-Until {[Math]::Abs((Packet $id).color.intensity.stops-3) -lt .08} 'HDR intensity motion did not reach its shared value'
                    if((Bubble-HdrState).base -ne $before.base){throw 'Intensity changed the normalized picker base'}
                    if($ending -eq 'cancel'){if($pointer -eq 'mouse'){Escape};[CapyRowPointer]::Cancel()}else{[CapyRowPointer]::Up()}
                }
            }finally{if([CapyRowPointer]::Active){[CapyRowPointer]::Cancel()}}
            if($ending -eq 'commit'){Wait-Until {[Math]::Abs((Model).color_panel.intensity-3) -lt .08} 'Released HDR intensity did not commit'}else{Wait-Until {(Bubble-HdrState).paint -eq $before.paint} 'Canceled HDR intensity did not restore its paint'}
            Assert-BubbleHdrState "$pointer-intensity-$ending" $before -Paint:($ending -ne 'commit') -Alpha
            $after=Center $id;if([Math]::Abs($after.x-$center.x) -gt 2 -or [Math]::Abs($after.y-$center.y) -gt 2){throw 'Intensity input moved the bubble face'}
        }
        Set-BubbleHdrValue $id 3;$reset=Bubble-HdrPoint $id .5
        $null=[CapyRowPointer]::DoubleClick($pointer,$reset.x,$reset.y)
        Wait-Until {[Math]::Abs((Packet $id).color.intensity.stops) -lt .005} 'Double tapping HDR intensity did not reset to zero'
        Set-BubbleHdrValue $id 3
        foreach($ending in @('commit','cancel')){
            $before=Bubble-HdrState;$local=Wheel-FieldPoint $id;$from=Relative $id $local;$to=Relative $id @((-$local[0]),$local[1])
            if($ending -eq 'cancel'){$swap=$from;$from=$to;$to=$swap}
            [CapyRowPointer]::Down($pointer,$from.x,$from.y)
            try{
                [CapyRowPointer]::Move($to.x,$to.y)
                Wait-Until {(Bubble-HdrState).base -ne $before.base} 'HDR wheel did not edit its normalized color'
                if([Math]::Abs((Packet $id).color.intensity.stops-3) -gt .005){throw 'Editing the wheel changed HDR intensity'}
                if($ending -eq 'cancel'){Escape;[CapyRowPointer]::Cancel()}else{[CapyRowPointer]::Up()}
            }finally{if([CapyRowPointer]::Active){[CapyRowPointer]::Cancel()}}
            if($ending -eq 'cancel'){Wait-Until {(Bubble-HdrState).paint -eq $before.paint} 'Cancelled HDR wheel did not restore exact paint'}
            Assert-BubbleHdrState "$pointer-hdr-wheel-$ending" $before -Paint:($ending -eq 'cancel') -Alpha
        }
        Set-BubbleHdrValue $id 0
        $before=Bubble-HdrState;$center=Center $id;$from=Bubble-HdrPoint $id .5;$outside=Relative $id @(([double](View $id).diameter*1.1),([double](View $id).diameter*.15))
        [CapyRowPointer]::Down($pointer,$from.x,$from.y)
        try{
            [CapyRowPointer]::Move($outside.x,$outside.y)
            Wait-Until {[Math]::Abs((Packet $id).color.intensity.stops-6) -lt .005} 'HDR intensity did not clamp outside its endpoint'
            $after=Center $id;if([Math]::Abs($after.x-$center.x) -gt 2 -or [Math]::Abs($after.y-$center.y) -gt 2){throw 'Leaving the intensity arc escaped into a bubble drag'}
            Escape;[CapyRowPointer]::Up()
        }finally{if([CapyRowPointer]::Active){[CapyRowPointer]::Cancel()}}
        Wait-Until {(Bubble-HdrState).paint -eq $before.paint} 'Outside intensity cancellation did not restore paint'
        Assert-BubbleHdrState "$pointer-intensity-outside-clamp" $before -Paint
    }
    $checks.Add('hdr-mouse-pen-touch-commit-cancel-hold-double-tap-reset')
    Set-BubbleHdrValue $id 0;$before=Bubble-HdrState
    $element=Control "bubble-intensity-$id";Focus-Keyboard $element
    [CapyRowPointer]::Key([uint32]$review.Id,0x27);Wait-Until {[Math]::Abs((Packet $id).color.intensity.stops-.1) -lt .005} 'HDR keyboard increment was not0.1EV'
    Settled-Capture 'hdr-keyboard-focus';Assert-BubbleHdrNoTooltip;[CapyRowPointer]::Key([uint32]$review.Id,0x24)
    Wait-Until {$s=(Packet $id).color.intensity;[Math]::Abs($s.stops-$s.range[0]) -lt .005} 'HDR Home did not reach the shared minimum'
    [CapyRowPointer]::Key([uint32]$review.Id,0x23)
    Wait-Until {$s=(Packet $id).color.intensity;[Math]::Abs($s.stops-$s.range[1]) -lt .005} 'HDR End did not reach the shared maximum'
    Assert-BubbleHdrState 'hdr-keyboard-endpoints' $before -Alpha;Set-BubbleHdrValue $id 0
    Set-BubbleHdrValue $id -1;$before=Bubble-HdrState;$first=Bubble-HdrPoint $id .75;$second=Relative $id @(0,(-[double](View $id).diameter*.35))
    [CapyCanvasTouch]::Initialize([uint32]$review.Id)
    try{
        [CapyCanvasTouch]::Down(1,$first.x,$first.y)
        Wait-Until {[Math]::Abs((Packet $id).color.intensity.stops-4) -lt .08} 'First touch did not own HDR intensity'
        [CapyCanvasTouch]::Down(2,$second.x,$second.y)
        [CapyCanvasTouch]::Pair($first.x+20,$first.y,$second.x+20,$second.y)
        [CapyCanvasTouch]::Up(2);[CapyCanvasTouch]::Up(1)
    }finally{[CapyCanvasTouch]::Dispose()}
    Wait-Until {(Bubble-HdrState).paint -eq $before.paint} 'Second touch did not roll back the intensity transaction'
    Assert-BubbleHdrState 'hdr-second-touch' $before -Paint
    $checks.Add('hdr-native-accessibility-keyboard-and-touch-takeover')
    Set-BubbleHdrPaint 12;$before=Bubble-HdrState;$range=Bubble-HdrRange $id;$shared=(Packet $id).color.intensity
    if($shared.range[1] -lt 12 -or $range.Current.Maximum -lt 12 -or [Math]::Abs($range.Current.Value-12) -gt .005){throw 'HDR intensity range clipped a12EV paint color'}
    $fraction=(10-$shared.range[0])/($shared.range[1]-$shared.range[0]);$target=Bubble-HdrPoint $id $fraction
    $nativeFace=Control "tool-bubble-$id" -Arranged;$bounds=$nativeFace.Current.BoundingRectangle;$colorBounds=(Packet $id).color.bounds
    $index=[int][Math]::Round($fraction*64);$observations=[Collections.Generic.List[object]]::new();$held=@{initial=$null;final=$null}
    $attempt=@{stage='hdr-expanded-contact';target=$target;local=$shared.path[$index];path_index=$index;shared=$shared;scale=$scale;center=(Center $id)
        unrounded_origin=@(($bounds.X-$colorBounds.x*$scale),($bounds.Y-$colorBounds.y*$scale));native_before=@($bounds.X,$bounds.Y,$bounds.Width,$bounds.Height);started_utc=[DateTime]::UtcNow.ToString('o')}
    $expandedWatch=[Diagnostics.Stopwatch]::StartNew()
    [CapyRowPointer]::Down('mouse',$target.x,$target.y)
    $attempt.down_completed_ms=$expandedWatch.Elapsed.TotalMilliseconds
    try{
        Wait-Until {
            $readStarted=$expandedWatch.Elapsed.TotalMilliseconds;$live=(Packet $id).color.intensity
            $observations.Add(@{phase='down';started_ms=$readStarted;finished_ms=$expandedWatch.Elapsed.TotalMilliseconds;utc=[DateTime]::UtcNow.ToString('o');stops=$live.stops;range=$live.range;point=$live.point})
            if([Math]::Abs($live.stops-$target.stops) -ge .08){return $false}
            $held.initial=$live;return $true
        } 'Expanded HDR arc did not retain its range'
        $attempt.repeated_begin_ms=$expandedWatch.Elapsed.TotalMilliseconds
        for($step=0;$step -lt 4;$step++){[CapyRowPointer]::Move($target.x,$target.y);Start-Sleep -Milliseconds 25}
        $attempt.repeated_end_ms=$expandedWatch.Elapsed.TotalMilliseconds
        Wait-Until {
            $readStarted=$expandedWatch.Elapsed.TotalMilliseconds;$live=(Packet $id).color.intensity;$native=$range.Current
            $markerError=[Math]::Sqrt([Math]::Pow(($live.point[0]-$shared.path[$index][0])*$scale,2)+[Math]::Pow(($live.point[1]-$shared.path[$index][1])*$scale,2))
            $stableMarker=[Math]::Sqrt([Math]::Pow(($live.point[0]-$held.initial.point[0])*$scale,2)+[Math]::Pow(($live.point[1]-$held.initial.point[1])*$scale,2))
            $sample=@{phase='same-position';started_ms=$readStarted;finished_ms=$expandedWatch.Elapsed.TotalMilliseconds;utc=[DateTime]::UtcNow.ToString('o');stops=$live.stops;range=$live.range;point=$live.point;native=@($native.Minimum,$native.Maximum,$native.Value);marker_error=$markerError;marker_change=$stableMarker}
            $observations.Add($sample);$held.final=$sample
            [Math]::Abs($live.stops-$target.stops) -lt .08 -and [Math]::Abs($native.Value-$target.stops) -lt .08 -and
                $live.range[0] -eq $shared.range[0] -and $live.range[1] -eq $shared.range[1] -and
                $native.Minimum -eq $shared.range[0] -and $native.Maximum -eq $shared.range[1] -and $markerError -le 2 -and $stableMarker -le 2
        } 'Repeated HDR positions changed the held intensity range or marker'
        Escape;[CapyRowPointer]::Up()
    }catch{$attempt.error=$_.Exception.Message;throw}finally{
        try{$bounds=$nativeFace.Current.BoundingRectangle;$attempt.native_after=@($bounds.X,$bounds.Y,$bounds.Width,$bounds.Height);$attempt.native_value=$range.Current.Value}catch{$attempt.native_error=$_.Exception.Message}
        $attempt.observations=@($observations.ToArray());$attempt.ended_ms=$expandedWatch.Elapsed.TotalMilliseconds;$hdrEvidence.Add($attempt)
        if([CapyRowPointer]::Active){[CapyRowPointer]::Cancel()}
    }
    Wait-Until {(Bubble-HdrState).paint -eq $before.paint} 'Expanded HDR cancellation did not restore12EV';Assert-BubbleHdrState 'hdr-expanded-range' $before -Paint
    Settled-Capture 'hdr-expanded-12ev'
    Assert-BubbleHdrPickerPreview $id
    Invoke 'color-transparent';Wait-Until {(Model).state.colors.slot -eq 'transparent'} 'Transparent paint did not become selected'
    $before=Bubble-HdrState;$element=Control "bubble-intensity-$id";$shared=(Packet $id).color.intensity
    if($element.Current.IsEnabled -or $shared.enabled){throw 'Transparent HDR intensity remains enabled'}
    $rejected=$null;try{(Bubble-HdrRange $id).SetValue(1)}catch{$rejected=$_.Exception.Message}
    $hdrEvidence.Add(@{stage='hdr-transparent-accessibility';error=$rejected});Assert-BubbleHdrState 'hdr-transparent-range-disabled' $before -Paint
    $at=Bubble-HdrPoint $id .5;[CapyRowPointer]::Down('mouse',$at.x,$at.y);[CapyRowPointer]::Up();Assert-BubbleHdrState 'hdr-transparent-disabled' $before -Paint
    Assert-BubbleHdrRamp $id 'hdr-transparent-disabled'
    Settled-Capture 'hdr-transparent-disabled';Invoke 'color-foreground';Wait-Until {(Model).state.colors.slot -eq 'foreground'} 'Foreground paint did not return'
    Set-BubbleHdrValue $id 0
    Assert-BubbleHdrWheelShapes $id
    Assert-BubbleHdrVisuals $id
    Invoke-BubbleHdrRetirement $id
    $checks.Add('hdr-float-depth-expanded-range-transparent-and-context-retirement')
}
function Bubble-HdrTrianglePixels([long]$Id,[string]$Stage,$Baseline=$null){
    (Control 'drawing-canvas').SetFocus();[CapyRowPointer]::Hover($w.X+20,$w.Y+20);Wait-BubbleVisualLayout
    $geometry=(Model).color_panel.geometry;$radius=($geometry.disc_radius+$geometry.inner)*.5
    $locals=@(@((.5-$radius),.5),@((.5+$radius*[Math]::Cos(2.2)),(.5+$radius*[Math]::Sin(2.2))),@((.5+$radius*[Math]::Cos(-2.2)),(.5+$radius*[Math]::Sin(-2.2))))
    $centroid=@((($geometry.triangle[0][0]+$geometry.triangle[1][0]+$geometry.triangle[2][0])/3),(($geometry.triangle[0][1]+$geometry.triangle[1][1]+$geometry.triangle[2][1])/3))
    foreach($local in $locals){foreach($projection in @(1,2)){if([CapyBubbleFixtureNative]::ColorHit((Join-Path $directory 'layer_windows.dll'),($local[0]*1000),($local[1]*1000),1000,$projection) -ne 0){throw 'Triangle containment sample overlaps the shared wheel'}}}
    if([CapyBubbleFixtureNative]::ColorHit((Join-Path $directory 'layer_windows.dll'),($centroid[0]*1000),($centroid[1]*1000),1000,1) -ne 2){throw 'Triangle centroid is outside its shared field'}
    $snapshots=@{}
    foreach($surface in @("bubble-wheel-$Id",'color-wheel')){
        $bounds=(Control $surface -Arranged).Current.BoundingRectangle
        $middle=@{x=[int][Math]::Round($bounds.X+$centroid[0]*$bounds.Width);y=[int][Math]::Round($bounds.Y+$centroid[1]*$bounds.Height)}
        $null=Wait-StablePixels {Screen-Pixels $middle.x $middle.y}
        $record=@{surface=$surface;bounds=@($bounds.X,$bounds.Y,$bounds.Width,$bounds.Height);samples=@();maximum_error=0.}
        for($index=0;$index -lt $locals.Count;$index++){
            $local=$locals[$index];$at=@{x=[int][Math]::Round($bounds.X+$local[0]*$bounds.Width);y=[int][Math]::Round($bounds.Y+$local[1]*$bounds.Height)}
            $pixels=Read-BubbleVisualPatch $at (Join-Path $run "$Stage-$surface-$index.png")
            $record.samples+=@{local=$local;point=$at;pixels=$pixels}
            if($Baseline){
                $expected=$Baseline[$surface].samples[$index]
                if($at.x -ne $expected.point.x -or $at.y -ne $expected.point.y){throw 'Shape switching moved the native wheel containment samples'}
                for($pixel=0;$pixel -lt $pixels.Count;$pixel++){foreach($channel in @('r','g','b')){$record.maximum_error=[Math]::Max($record.maximum_error,[Math]::Abs($pixels[$pixel][$channel]-$expected.pixels[$pixel][$channel]))}}
            }
        }
        $record.interior=Bubble-HdrPixel $middle "$Stage-$surface-interior";$record.interior_point=$middle
        $snapshots[$surface]=$record;$hdrEvidence.Add(@{stage=$Stage;containment=$record})
        if($Baseline){
            if($record.maximum_error -gt 4){throw 'Native HDR triangle field escaped its shared clip'}
            $difference=0.;foreach($channel in @('r','g','b')){$difference=[Math]::Max($difference,[Math]::Abs($record.interior[$channel]-$Baseline[$surface].samples[0].pixels[40][$channel]))}
            if($difference -le 12){throw 'Native HDR triangle field is blank at its shared centroid'}
        }
    }
    return $snapshots
}
function Assert-BubbleHdrWheelShapes([long]$Id){
    $original=(Model).color_panel.shape;Set-BubbleVisualDiameter $Id 216;Set-BubbleHdrValue $Id 3
    $shapeControl={param($Shape)
        $model=(Model).color_panel
        if($model.shape -ne $Shape){$index=[Array]::IndexOf(@($model.other_shapes),$Shape);if($index -lt 0){throw 'Native color panel does not publish the requested alternate shape'};Invoke "color-shape-$index";Wait-Until {(Model).color_panel.shape -eq $Shape} 'Native wheel shape did not update'}
    }
    try{
        $containment=$null
        foreach($shape in @('circle','triangle','square')){
            & $shapeControl $shape;Wait-BubbleVisualLayout;$null=Assert-BubbleVisualGeometry $Id "hdr-shape-$shape"
            if($shape -eq 'circle'){$containment=Bubble-HdrTrianglePixels $Id 'hdr-triangle-circle-baseline'}
            elseif($shape -eq 'triangle'){$null=Bubble-HdrTrianglePixels $Id 'hdr-triangle-containment' $containment}
            $side=(View $Id).diameter-14;$geometry=(Model).color_panel.geometry;$projection=@{circle=2;triangle=1;square=0}[$shape]
            foreach($part in @('hue','field')){
                $locals=if($part -eq 'hue'){$radius=($geometry.inner+$geometry.outer)*.5*$side;@(@(($radius*[Math]::Cos(.6)),($radius*[Math]::Sin(.6))),@(($radius*[Math]::Cos(1.4)),($radius*[Math]::Sin(1.4))))}else{@(@(((.56-.5)*$side),((.46-.5)*$side)),@(((.62-.5)*$side),((.55-.5)*$side)))}
                foreach($local in $locals){$hit=[CapyBubbleFixtureNative]::ColorHit((Join-Path $directory 'layer_windows.dll'),($local[0]+$side*.5),($local[1]+$side*.5),$side,$projection);if($hit -ne $(if($part -eq 'hue'){1}else{2})){throw 'HDR shape fixture point misses its shared wheel part'}}
                $before=Bubble-HdrState;$from=Relative $Id $locals[0];$to=Relative $Id $locals[1]
                [CapyRowPointer]::Down('mouse',$from.x,$from.y)
                try{
                    [CapyRowPointer]::Move($to.x,$to.y)
                    Wait-Until {(Bubble-HdrState).base -ne $before.base} 'Native HDR shape contact did not update its color'
                    if([Math]::Abs((Packet $Id).color.intensity.stops-3) -gt .005){throw 'Native HDR hue or field pick changed intensity'}
                    Settled-Capture "hdr-shape-$shape-$part";Escape;[CapyRowPointer]::Up()
                }finally{if([CapyRowPointer]::Active){[CapyRowPointer]::Cancel()}}
                Wait-Until {(Bubble-HdrState).paint -eq $before.paint} 'Native HDR shape cancellation did not restore paint'
                Assert-BubbleHdrState "hdr-shape-$shape-$part" $before -Paint
                if($shape -eq 'triangle'){$null=Bubble-HdrTrianglePixels $Id "hdr-triangle-$part-restored" $containment}
            }
        }
    }finally{& $shapeControl $original;Set-BubbleHdrValue $Id 0}
    $checks.Add('hdr-native-circle-triangle-square-hue-and-field-preserve-intensity')
}
function Write-BubbleHdrPickerReceipt([long]$Id,[string]$Stage,$Paper){
    $receipt=@{stage=$Stage;requested_screen=$Paper;utc=[DateTime]::UtcNow.ToString('o')}
    try{
        $m=Model;$native=(Workspace-Root).Current.ItemStatus|ConvertFrom-Json
        $receipt.document=$m.windows_tabs.selected;$receipt.depth=$m.color_panel.document_depth;$receipt.tool=$m.state.layer_tools.tool
        $receipt.camera=$m.state.camera;$receipt.shared_picker=$m.color_preview.picker
        $receipt.native_preview=$native.color_preview;$receipt.native_revision=$native.revision;$receipt.native_model_revision=$native.model_revision
        $cursor=[CapyRowPointer]::Cursor();$receipt.actual_screen=@{x=$cursor.position.x;y=$cursor.position.y};$receipt.foreground=[CapyRowPointer]::GetForegroundWindow().ToInt64()
        $bounds=(Find 'drawing-canvas').Current.BoundingRectangle
        $receipt.canvas_bounds=@($bounds.X,$bounds.Y,$bounds.Width,$bounds.Height)
        $receipt.requested_surface=@((($Paper.x-$bounds.X)*$m.state.camera.viewport[0]/$bounds.Width),(($Paper.y-$bounds.Y)*$m.state.camera.viewport[1]/$bounds.Height))
        $range=(Find "bubble-intensity-$Id").GetCurrentPattern([System.Windows.Automation.RangeValuePattern]::Pattern).Current
        $receipt.intensity=@{value=$range.Value;range=@($range.Minimum,$range.Maximum)}
    }catch{$receipt.diagnostic_error=$_.Exception.Message}
    $hdrEvidence.Add($receipt);$receipt|ConvertTo-Json -Depth 12|Set-Content (Join-Path $run ($Stage+'.json'))
}
function Assert-BubbleHdrPickerPreview([long]$Id){
    $original=Bubble-HdrState;$paper=Document-Point .18 .18;$source=Header-Source 'color_picker' ''
    if(!$source){throw 'HDR preview fixture lacks its native picker source'}
    $start={Tap (At ('header-item-'+$source.id));Wait-Until {(Model).state.layer_tools.tool -in @('pick_visible','pick_layer')} 'Native color picker did not activate';[CapyRowPointer]::Hover($paper.x,$paper.y)}
    $preview=@{value=$null}
    & $start
    try{
        Wait-Until {$preview.value=(Workspace-Root).Current.ItemStatus|ConvertFrom-Json;$null -ne $preview.value.color_preview} 'Native HDR picker did not publish its paper preview'
        Write-BubbleHdrPickerReceipt $Id 'hdr-picker-reference-receipt' $paper
        [CapyRowPointer]::Down('mouse',$paper.x,$paper.y);[CapyRowPointer]::Up()
        Wait-Until {(Model).state.layer_tools.tool -eq 'paint'} 'Native picker did not commit its paper sample'
    }catch{Write-BubbleHdrPickerReceipt $Id 'hdr-picker-reference-failure' $paper;throw
    }finally{if([CapyRowPointer]::Active){[CapyRowPointer]::Cancel()};if((Model).state.layer_tools.tool -in @('pick_visible','pick_layer')){Escape}}
    Assert-BubbleHdrState 'hdr-picker-reference-sample' $original
    $expected=(Packet $Id).color.intensity;Assert-BubbleHdrRamp $Id 'hdr-picker-reference'
    Set-BubbleHdrPaint 12 -Base;$before=Bubble-HdrState;$range=Bubble-HdrRange $Id
    $points=@((Relative $Id $expected.path[8]),(Relative $Id $expected.path[48]),(Relative $Id $expected.point));$baseline=@()
    foreach($point in $points){$baseline+=Wait-StablePixels {Patch-Pixels $point}}
    & $start
    try{
        Wait-Until {$preview.value=(Workspace-Root).Current.ItemStatus|ConvertFrom-Json;$preview.value.color_preview -and [Math]::Abs($range.Current.Value-$expected.stops) -lt .005} 'HDR preview did not update the live native intensity value'
        Write-BubbleHdrPickerReceipt $Id 'hdr-picker-preview-receipt' $paper
        if([Math]::Abs($range.Current.Minimum-$expected.range[0]) -gt .005 -or [Math]::Abs($range.Current.Maximum-$expected.range[1]) -gt .005){throw 'HDR preview retained the committed intensity range'}
        Assert-BubbleHdrRamp $Id 'hdr-picker-preview' -Spec $expected -KeepPointer
        $current=Patch-Pixels $points[0]
        if($current -eq $baseline[0]){throw 'HDR picker preview changed accessibility without updating actual ramp pixels'}
        $hdrEvidence.Add(@{stage='hdr-picker-preview';sample=$preview.value.color_preview;expected=$expected;range=@($range.Current.Minimum,$range.Current.Maximum);value=$range.Current.Value;ramp_changed=$true})
        Settled-Capture 'hdr-picker-preview';Escape
    }catch{Write-BubbleHdrPickerReceipt $Id 'hdr-picker-preview-failure' $paper;throw
    }finally{if((Model).state.layer_tools.tool -in @('pick_visible','pick_layer')){Escape}}
    [CapyRowPointer]::Hover($w.X+20,$w.Y+20)
    Wait-Until {(Model).state.layer_tools.tool -eq 'paint' -and (Bubble-HdrState).paint -eq $before.paint -and [Math]::Abs((Bubble-HdrRange $Id).Current.Value-12) -lt .005} 'Cancelled HDR preview did not restore committed paint and intensity'
    for($i=0;$i -lt $points.Count;$i++){Wait-Until {(Patch-Pixels $points[$i]) -eq $baseline[$i]} 'Cancelled HDR preview did not restore exact ramp and marker pixels'}
    Assert-BubbleHdrState 'hdr-picker-preview-cancel' $before -Paint;Settled-Capture 'hdr-picker-preview-cancelled'
    $checks.Add('hdr-picker-preview-updates-native-ramp-marker-range-and-cancels-exactly')
}
function Assert-BubbleHdrPainting([long]$Id,[string]$Stage){
    Wait-Until {$m=Model;$m.brush_ready -and $m.canvas_ready -and !$m.windows_rendering_suspended -and !(Find 'canvas-status' -Visible)} 'Drawing transition left an unavailable canvas or a stale failure message'
    & (Join-Path $CapyScripts 'open-application-menu.ps1') -Root $root -Name 'View';Invoke 'fit_canvas'
    Wait-BubbleVisualLayout
    $before=Bubble-HdrState;$at=Document-Point .2 .2;$center=Center $Id
    Paint-Gap $Id @((($at.x-$center.x)/$scale),(($at.y-$center.y)/$scale)) $Stage -NoHalo
    $first=Bubble-HdrState
    $evidence=@{stage=$Stage;before=$before;first_after=$first;after=$first;canvas_status_visible=$null;painting_and_exact_pixel_undo=$true}
    $hdrEvidence.Add($evidence)
    Wait-Until {
        $evidence.after=Bubble-HdrState
        foreach($field in @('document','checkpoint','modified','paint')){if($evidence.after.$field -ne $before.$field){return $false}}
        $true
    } "$Stage Undo did not publish its restored document, checkpoint, dirty state and paint"
    $evidence.canvas_status_visible=[bool](Find 'canvas-status' -Visible)
    if($evidence.canvas_status_visible){throw 'Painting after drawing activation left a native failure message'}
}
function Invoke-BubbleHdrRetirement([long]$Id){
    foreach($context in @(@{name='slot';device='mouse'},@{name='mask';device='pen'},@{name='document';device='touch'})){
        Set-BubbleHdrValue $Id 0;$before=Bubble-HdrState;$at=Bubble-HdrPoint $Id .75;$stale=Bubble-HdrRange $Id
        [CapyRowPointer]::Down($context.device,$at.x,$at.y)
        try{
            Wait-Until {[Math]::Abs((Packet $Id).color.intensity.stops-4) -lt .08} 'Context retirement did not begin with an admitted intensity contact'
            switch($context.name){
                'slot' {Invoke 'color-background';Wait-Until {(Model).state.colors.slot -eq 'background'} 'HDR slot did not switch while held'}
                'mask' {(Control 'drawing-canvas').SetFocus();[CapyRowPointer]::Key([uint32]$review.Id,0x51);Wait-Until {(Model).state.layer_tools.quick_mask -and !(Packet $Id).color -and !(Find "bubble-intensity-$Id" -Visible)} 'Mask context did not retire HDR intensity'}
                'document' {Next-Drawing;Wait-Until {(Model).windows_tabs.selected -eq $script:HdrSdrId -and (Model).brush_ready -and !(Packet $Id).color -and !(Find "bubble-intensity-$Id" -Visible)} 'SDR document did not retire HDR intensity'}
            }
            $target=Bubble-HdrState
            [CapyRowPointer]::Move($at.x+24,$at.y);[CapyRowPointer]::Cancel()
            Assert-BubbleHdrState ('hdr-'+$context.name+'-retired-contact') $target -Paint
            if($context.name -ne 'slot'){
                $rejected=$null;try{$stale.SetValue(2)}catch{$rejected=$_.Exception.Message}
                $hdrEvidence.Add(@{stage=('hdr-'+$context.name+'-retired-range');error=$rejected})
                Start-Sleep -Milliseconds 100;Assert-BubbleHdrState ('hdr-'+$context.name+'-retired-range') $target -Paint
                Wait-Until {!(Find 'canvas-status' -Visible)} 'Retired HDR contact left a stale native failure message'
                $null=Assert-BubbleVisualGeometry $Id ('hdr-'+$context.name+'-sdr-contour');Settled-Capture ('hdr-'+$context.name+'-sdr-contour')
            }
        }finally{if([CapyRowPointer]::Active){[CapyRowPointer]::Cancel()}}
        if($context.name -eq 'document'){Assert-BubbleHdrPainting $Id 'hdr-retired-sdr'}
        switch($context.name){
            'slot' {Invoke 'color-foreground';Wait-Until {(Model).state.colors.slot -eq 'foreground'} 'Foreground did not restore after retirement'}
            'mask' {(Control 'drawing-canvas').SetFocus();Escape;Wait-Until {!(Model).state.layer_tools.quick_mask} 'Quick Mask did not close'}
            'document' {Next-Drawing;Wait-Until {(Model).windows_tabs.selected -eq $script:HdrDrawingId -and (Model).brush_ready} 'HDR drawing did not reactivate'}
        }
        Wait-Until {(Packet $Id).color -and (Find "bubble-intensity-$Id" -Visible)} 'Returning to HDR did not restore a live native intensity control'
        $restored=Bubble-HdrState
        if($restored.document -ne $before.document -or $restored.checkpoint -ne $before.checkpoint -or $restored.modified -ne $before.modified){throw 'HDR context replacement changed authored history'}
        Set-BubbleHdrValue $Id 0
        if($context.name -eq 'document'){Assert-BubbleHdrPainting $Id 'hdr-reactivated'}
    }
}
function Set-BubbleHdrOptions([long]$Id,[bool]$Pair,[bool]$Palette,$Recent=$null){
    $wanted=@{pair=$Pair;palette=$Palette;recent=$(if($null -eq $Recent){$Palette}else{[bool]$Recent})};$view=View $Id
    if(!@($wanted.Keys|Where-Object {$view.options.$_ -ne $wanted[$_]}).Count){return}
    $at=Center $Id;[CapyRowPointer]::RightClick($at.x,$at.y);$null=Menu-Item 'Put back'
    foreach($option in @(@{key='pair';label='Primary and secondary'},@{key='palette';label='Palette'},@{key='recent';label='Recent colors'})){
        if((View $Id).options.($option.key) -ne $wanted[$option.key]){Invoke $option.label -Name;Wait-Until {(View $Id).options.($option.key) -eq $wanted[$option.key]} 'HDR bubble option did not update'}
    }
    Escape;Wait-BubbleVisualLayout
}
function Bubble-HdrNativeButtons {
    $cache=[System.Windows.Automation.CacheRequest]::new()
    foreach($property in @([System.Windows.Automation.AutomationElement]::NameProperty,[System.Windows.Automation.AutomationElement]::BoundingRectangleProperty,[System.Windows.Automation.AutomationElement]::IsOffscreenProperty)){$cache.Add($property)}
    $scope=$cache.Activate()
    try{
        foreach($element in $root.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::Button))){
            $c=$element.Cached;$b=$c.BoundingRectangle
            if(!$c.IsOffscreen -and !$b.IsEmpty){@{name=$c.Name;bounds=@($b.X,$b.Y,$b.Width,$b.Height)}}
        }
    }finally{$scope.Dispose()}
}
function Assert-BubbleHdrGeometry([long]$Id,[string]$Stage,[switch]$RequirePill){
    $geometry=Assert-BubbleVisualGeometry $Id $Stage;$packet=Packet $Id;$view=$packet.view;$center=Center $Id
    if($RequirePill -and (!$view.pill -or !$packet.pill)){throw 'HDR corner capture requires its open palette pill'}
    $native=@(Bubble-HdrNativeButtons);$matches=[Collections.Generic.List[object]]::new()
    $caption=[System.Windows.Automation.AndCondition]::new([System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,$packet.color.intensity.text),[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::Text))
    $face=(Control "tool-bubble-$Id" -Arranged).Current.BoundingRectangle
    foreach($text in $root.FindAll([System.Windows.Automation.TreeScope]::Descendants,$caption)){if(!$text.Current.IsOffscreen -and $face.IntersectsWith($text.Current.BoundingRectangle)){throw 'HDR bubble incorrectly includes the docked-panel intensity caption'}}
    foreach($bead in @($view.beads)){
        $p=$bead.placement;if($p.alpha -le 0){continue};$point=Relative $Id $p.center;$size=$p.size*$scale
        $found=@($native|Where-Object {$b=$_.bounds;$_.name -eq $bead.label -and [Math]::Abs($b[2]-$size) -le 2 -and [Math]::Abs($b[3]-$size) -le 2 -and [Math]::Abs($b[0]+$b[2]*.5-$point.x) -le 2 -and [Math]::Abs($b[1]+$b[3]*.5-$point.y) -le 2})
        if($found.Count -ne 1){throw "HDR native bead does not match its shared placement: $($bead.label)"}
        $matches.Add(@{kind=$p.bead.kind;native=$found[0];placement=$p})
    }
    if($view.pill){
        $pill=$packet.pill
        foreach($button in @(@{x=0;width=32},@{x=34;width=116},@{x=152;width=32})){
            $x=$center.x+($pill.x+$button.x)*$scale;$y=$center.y+$pill.y*$scale
            $found=@($native|Where-Object {$b=$_.bounds;[Math]::Abs($b[0]-$x) -le 1.5 -and [Math]::Abs($b[1]-$y) -le 1.5 -and [Math]::Abs($b[2]-$button.width*$scale) -le 1.5 -and [Math]::Abs($b[3]-30*$scale) -le 2})
            if($found.Count -ne 1){throw 'HDR palette pill does not retain GTK button allocation and spacing'}
        }
    }
    $hdrEvidence.Add(@{stage=$Stage;geometry=$geometry;shared_color=$packet.color;beads=@($matches.ToArray());pill=$packet.pill})
}
function Bubble-HdrPixel($Point,[string]$Name){
    $pixels=Read-BubbleVisualPatch $Point (Join-Path $run ($Name+'.png'))
    $pixels[40]
}
function Assert-BubbleHdrRamp([long]$Id,[string]$Stage,$Spec=$null,[switch]$KeepPointer){
    if(!$KeepPointer){(Control 'drawing-canvas').SetFocus();[CapyRowPointer]::Hover($w.X+20,$w.Y+20)}
    Wait-BubbleVisualLayout
    if(!$Spec){$Spec=(Packet $Id).color.intensity}
    if($spec.path.Count -ne $spec.ramp.Count -or $spec.path.Count -ne 65){throw 'HDR ramp and arc path do not share their sample count'}
    if($spec.marker[3] -ne 1){throw 'HDR intensity marker must remain opaque independently of authored paint alpha'}
    $samples=[Collections.Generic.List[object]]::new()
    foreach($index in @(0,8,32,48,64)){
        $local=$spec.path[$index]
        if([Math]::Sqrt([Math]::Pow($local[0]-$spec.point[0],2)+[Math]::Pow($local[1]-$spec.point[1],2)) -le $spec.geometry.marker_radius+7){continue}
        $at=Relative $Id $local;$actual=Bubble-HdrPixel $at "$Stage-ramp-$index";$expected=$spec.ramp[$index];$error=0.
        for($channel=0;$channel -lt 3;$channel++){$key=@('r','g','b')[$channel];$error=[Math]::Max($error,[Math]::Abs($actual[$key]-255*$expected[$channel]))}
        $samples.Add(@{index=$index;point=$at;expected=$expected;actual=$actual;maximum_error=$error})
    }
    $at=Relative $Id $spec.point;$actual=Bubble-HdrPixel $at "$Stage-marker";$markerError=0.
    for($channel=0;$channel -lt 3;$channel++){$key=@('r','g','b')[$channel];$markerError=[Math]::Max($markerError,[Math]::Abs($actual[$key]-255*$spec.marker[$channel]))}
    $hdrEvidence.Add(@{stage=$Stage;ramp_pixels=@($samples.ToArray());marker=@{point=$at;actual=$actual;expected=$spec.marker;maximum_error=$markerError}})
    if(@($samples|Where-Object maximum_error -gt 4).Count -or $markerError -gt 2){throw 'Native HDR ramp, round caps or marker differs from shared display colors'}
}
function Assert-BubbleHdrNoTooltip([string]$Stage='hdr-keyboard-no-tooltip'){
    $condition=[System.Windows.Automation.AndCondition]::new(
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::ToolTip),
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty,$review.Id))
    $found=@([System.Windows.Automation.AutomationElement]::RootElement.FindAll([System.Windows.Automation.TreeScope]::Descendants,$condition))
    if(!$found.Count){$found=@($root.FindAll([System.Windows.Automation.TreeScope]::Descendants,$condition))}
    $visible=@(foreach($item in $found){$current=$item.Current;$bounds=$current.BoundingRectangle;if(!$current.IsOffscreen -and $bounds.Width -gt 0 -and $bounds.Height -gt 0){@{name=$current.Name;class=$current.ClassName;runtime_id=($item.GetRuntimeId() -join ':');bounds=@($bounds.X,$bounds.Y,$bounds.Width,$bounds.Height)}}})
    $hdrEvidence.Add(@{stage=$Stage;visible=$visible})
    if($visible.Count){throw 'Focused HDR intensity displayed a native tooltip over its presentation'}
}
function Assert-BubbleHdrFocus([long]$Id){
    (Control 'drawing-canvas').SetFocus();$spec=(Packet $Id).color.intensity
    $at=Relative $Id @(($spec.point[0]+$spec.geometry.marker_radius+4),$spec.point[1]);$before=Bubble-HdrPixel $at 'hdr-focus-before'
    $element=Control "bubble-intensity-$Id";Focus-Keyboard $element
    Assert-BubbleHdrNoTooltip 'hdr-focus-ring-no-tooltip'
    Start-Sleep -Milliseconds 150;$after=Bubble-HdrPixel $at 'hdr-focus-after';$ink=[Drawing.ColorTranslator]::FromHtml((Model).state.palette.text);$error=0.
    foreach($channel in @('r','g','b')){$expected=.1*$before[$channel]+.9*$ink.($channel.ToUpperInvariant());$error=[Math]::Max($error,[Math]::Abs($after[$channel]-$expected))}
    $hdrEvidence.Add(@{stage='hdr-focus-ring';point=$at;before=$before;after=$after;maximum_error=$error})
    if($error -gt 4){throw 'HDR focus ring does not use its shared marker position and GTK text overlay'}
    Settled-Capture 'hdr-focus-ring';(Control 'drawing-canvas').SetFocus()
}
function Assert-BubbleHdrZeroTick([long]$Id){
    $original=(Packet $Id).color.intensity.stops;Set-BubbleHdrValue $Id 3
    try{
        (Control 'drawing-canvas').SetFocus();[CapyRowPointer]::Hover($w.X+20,$w.Y+20);Wait-BubbleVisualLayout
        $spec=(Packet $Id).color.intensity;$g=$spec.geometry;$fraction=(0-$spec.range[0])/($spec.range[1]-$spec.range[0])
        $angle=[Math]::PI-$g.end_angle-$fraction*([Math]::PI-2*$g.end_angle);$x=[Math]::Cos($angle);$y=[Math]::Sin($angle)
        $radius=$g.radius+$g.width*.5+3.5;$local=@(($g.center[0]+$radius*$x),($g.center[1]+$radius*$y));$at=Relative $Id $local
        $pixels=Read-BubbleVisualPatch $at (Join-Path $run 'hdr-zero-tick.png')
        $left=Bubble-HdrPixel (Relative $Id @(($local[0]-4*$y),($local[1]+4*$x))) 'hdr-zero-left'
        $right=Bubble-HdrPixel (Relative $Id @(($local[0]+4*$y),($local[1]-4*$x))) 'hdr-zero-right'
        $ink=[Drawing.ColorTranslator]::FromHtml((Model).state.palette.text);$samples=@()
        foreach($i in @(30,31,32,39,40,41,48,49,50)){
            $dot=0.;$norm=0.;$background=@{}
            foreach($c in @('r','g','b')){$background[$c]=($left[$c]+$right[$c])*.5;$delta=$ink.($c.ToUpperInvariant())-$background[$c];$dot+=($pixels[$i][$c]-$background[$c])*$delta;$norm+=$delta*$delta}
            $alpha=if($norm){$dot/$norm}else{0};$error=0.
            foreach($c in @('r','g','b')){$expected=$background[$c]*(1-$alpha)+$ink.($c.ToUpperInvariant())*$alpha;$error=[Math]::Max($error,[Math]::Abs($pixels[$i][$c]-$expected))}
            $samples+=@{index=$i;alpha=$alpha;maximum_error=$error}
        }
        $hdrEvidence.Add(@{stage='hdr-zero-ev-tick';point=$at;geometry=$g;range=$spec.range;neighbor_pixels=@($left,$right);samples=$samples})
        if(!@($samples|Where-Object {$_.alpha -ge .65 -and $_.alpha -le .94 -and $_.maximum_error -le 6}).Count){throw 'HDR zero-EV tick is absent from its shared radial position or differs from the text overlay'}
    }finally{Set-BubbleHdrValue $Id $original}
}
function Assert-BubbleHdrVisuals([long]$Id){
    $before=Bubble-HdrState;$options=(View $Id).options;$modes=@('off','low','medium','high');$original=(Model).state.palette.glass.transparency
    try{
        Set-BubbleVisualTransparency 0
        foreach($diameter in @(128,216,352)){
            Set-BubbleVisualDiameter $Id $diameter
            foreach($pair in @($false,$true)){
                Set-BubbleHdrOptions $Id $pair $false;Wait-BubbleVisualLayout
                $stage="hdr-$diameter-pair-$pair";Assert-BubbleHdrGeometry $Id $stage;Assert-BubbleHdrRamp $Id $stage;Settled-Capture $stage
            }
            Set-BubbleHdrOptions $Id $true $true
            foreach($corner in @(@{name='top-left';x=0;y=0},@{name='top-right';x=1;y=0},@{name='bottom-right';x=1;y=1},@{name='bottom-left';x=0;y=1})){
                $area=(Model).layout.work_area;$bounds=(Packet $Id).color.bounds
                $x=$area.x+$(if($corner.x){$area.width-$bounds.x-$bounds.width-12}else{12-$bounds.x})
                $y=$area.y+$(if($corner.y){$area.height-$bounds.y-$bounds.height-12}else{12-$bounds.y})
                Move-BubbleVisual $Id $x $y;$stage="hdr-$diameter-$($corner.name)"
                $palette=@((View $Id).beads|Where-Object {$_.placement.bead.kind -eq 'palettes'})[0]
                if(!$palette){throw 'HDR palette option did not expose its bead at the work-area corner'}
                if(!(View $Id).pill){Tap (Relative $Id $palette.placement.center);Wait-Until {(View $Id).pill} 'HDR palette pill did not reopen at the work-area corner'}
                Wait-BubbleVisualLayout
                Assert-BubbleHdrGeometry $Id $stage -RequirePill;Assert-BubbleHdrRamp $Id $stage;Settled-Capture $stage
            }
        }
        Set-BubbleVisualDiameter $Id 216
        foreach($index in 0..3){Set-BubbleVisualTransparency $index;$stage="hdr-transparency-$($modes[$index])";Assert-BubbleHdrGeometry $Id $stage;Assert-BubbleHdrRamp $Id $stage;Settled-Capture $stage}
        Assert-BubbleHdrFocus $Id;Assert-BubbleHdrZeroTick $Id
        Set-BubbleHdrOptions $Id $false $false;Wait-BubbleVisualLayout
        $snapshot=Bubble-HdrState;$bounds=(Packet $Id).color.bounds;$diameter=(View $Id).diameter
        foreach($local in @(@(0,(-$diameter*.5+2)),@(0,($bounds.y+$bounds.height-2)))){
            $at=Relative $Id $local;[CapyRowPointer]::Down('mouse',$at.x,$at.y);Start-Sleep -Milliseconds 35;[CapyRowPointer]::Up()
        }
        Assert-BubbleHdrState 'hdr-top-and-expanded-bottom-rim-own-input' $snapshot -Paint
        $checkpoint=$snapshot.checkpoint;Paint-Gap $Id @(($diameter*.5),($diameter*.5)) 'hdr-outside-shoulder' -NoHalo
        Wait-Until {(Bubble-HdrState).checkpoint -eq $checkpoint -and (Bubble-HdrState).modified -eq $snapshot.modified} 'Shoulder canvas Undo did not restore authored history'
        $checks.Add('hdr-shared-contour-wheel-ramp-caps-marker-palette-beads-sizes-corners-transparency-focus')
    }finally{
        if((Model).state.palette.glass.transparency -ne $original){Set-BubbleVisualTransparency ([Array]::IndexOf($modes,$original))}
        Set-BubbleHdrOptions $Id ([bool]$options.pair) ([bool]$options.palette) ([bool]$options.recent)
    }
    $after=Bubble-HdrState
    if($after.checkpoint -ne $before.checkpoint -or $after.modified -ne $before.modified -or $after.paint -ne $before.paint){throw 'HDR visual checks changed authored history or selected paint'}
}

function Read-BubbleVisualPatch($Point,[string]$Path){
    $bitmap=[Drawing.Bitmap]::new(9,9);$graphics=[Drawing.Graphics]::FromImage($bitmap)
    try{
        $graphics.CopyFromScreen($Point.x-4,$Point.y-4,0,0,$bitmap.Size)
        $pixels=@(foreach($y in 0..8){foreach($x in 0..8){$color=$bitmap.GetPixel($x,$y);@{r=[int]$color.R;g=[int]$color.G;b=[int]$color.B}}})
        $bitmap.Save($Path)
        return ,$pixels
    }finally{$graphics.Dispose();$bitmap.Dispose()}
}
function Measure-BubbleVisualOverlay($Baseline,$Actual,$Ink,[double]$Alpha){
    $errors=@();$changes=@()
    for($i=0;$i -lt $Baseline.Count;$i++){
        foreach($channel in @('r','g','b')){
            $expected=(1-$Alpha)*$Baseline[$i][$channel]+$Alpha*$Ink[$channel]
            $errors+=[Math]::Abs($Actual[$i][$channel]-$expected)
            $changes+=[Math]::Abs($Actual[$i][$channel]-$Baseline[$i][$channel])
        }
    }
    @{maximum_error=($errors|Measure-Object -Maximum).Maximum;mean_change=($changes|Measure-Object -Average).Average;alpha=$Alpha}
}
function Bubble-VisualBounds($Element){
    $bounds=$Element.Current.BoundingRectangle
    return ,@([double]$bounds.X,[double]$bounds.Y,[double]$bounds.Width,[double]$bounds.Height)
}
function Bubble-VisualState([long]$Id){
    $model=Model
    [ordered]@{
        view=(View $Id);layout=$model.state.workspace.layout;revision=$model.state.document_file.revision
        brush=$model.state.brush.preset;colors=$model.state.colors
        checkpoint=@($model.windows_tabs.session_stamps|Where-Object id -eq $model.windows_tabs.selected)[0].stamp.checkpoint
    }|ConvertTo-Json -Depth 90 -Compress
}
function Bubble-VisualTags([long]$Id,[string]$Label){
    $center=Center $Id;$reach=((Packet $Id).reach+16)*$scale
    foreach($element in $root.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition)){
        $current=$element.Current;$bounds=$current.BoundingRectangle
        if($current.ControlType -ne [System.Windows.Automation.ControlType]::Text -or $current.Name -ne $Label -or $current.IsOffscreen -or $bounds.IsEmpty -or $bounds.Width -le 0 -or $bounds.Height -le 0 -or $bounds.Width -gt 78*$scale+2){continue}
        if([Math]::Abs($bounds.X+$bounds.Width*.5-$center.x) -le $reach -and [Math]::Abs($bounds.Y+$bounds.Height*.5-$center.y) -le $reach){
            @{name=$current.Name;bounds=@($bounds.X,$bounds.Y,$bounds.Width,$bounds.Height)}
        }
    }
}
function Assert-BubbleVisualChooserTags([long]$Id){
    $view=View $Id;$items=@($view.chooser)
    $hidden=@($items|Where-Object {$_.kind -ne 'brush' -and $_.enabled -and !$_.shown})[0]
    $original=@($items|Where-Object {$_.kind -eq 'medium' -and $_.shown})[0]
    if(!$hidden -or ($hidden.kind -eq 'medium' -and !$original)){throw 'Chooser tag check needs a hidden enabled choice and its original shown medium'}
    $before=Bubble-VisualState $Id;$away=@{x=[int]($w.X+20);y=[int]($w.Y+20)};$point=Relative $Id $hidden.center
    $evidence=[ordered]@{id=$Id;label=$hidden.label;original_medium=$original.label;point=$point;baseline=@();hover=@();pressed=@();restored=@()};$down=$false
    try{
        [CapyRowPointer]::Hover($away.x,$away.y);Start-Sleep -Milliseconds 250
        $evidence.baseline=@(Bubble-VisualTags $Id $hidden.label)
        if($evidence.baseline.Count){throw 'Unshown chooser tag is visible before hover'}
        [CapyRowPointer]::Hover($point.x,$point.y);Start-Sleep -Milliseconds 250
        $evidence.hover=@(Bubble-VisualTags $Id $hidden.label)
        if($evidence.hover.Count){throw 'Passive mouse hover revealed an unshown chooser tag'}
        [CapyRowPointer]::Down('mouse',$point.x,$point.y);$down=$true
        Wait-Until {$evidence.pressed=@(Bubble-VisualTags $Id $hidden.label);$evidence.pressed.Count -gt 0} 'Held chooser choice did not expose its native text tag'
    }finally{
        try{
            if($down){
                try{
                    if($hidden.kind -eq 'medium'){
                        $restore=@((View $Id).chooser|Where-Object {$_.kind -eq $original.kind -and $_.label -eq $original.label})[0]
                        if(!$restore){throw 'Original chooser medium disappeared during tag check'}
                        $at=Relative $Id $restore.center;[CapyRowPointer]::Move($at.x,$at.y)
                        Wait-Until {@((View $Id).chooser|Where-Object {$_.kind -eq $original.kind -and $_.label -eq $original.label -and $_.shown}).Count -eq 1} 'Held chooser contact did not restore its original medium'
                    }
                }finally{
                    try{Escape}finally{[CapyRowPointer]::Cancel()}
                }
            }
        }finally{
            [CapyRowPointer]::Hover($away.x,$away.y)
            $evidence|ConvertTo-Json -Depth 10|Set-Content (Join-Path $run "bubble-$Id-chooser-tags.json")
        }
    }
    Wait-BubbleVisualLayout
    Wait-Until {!@(Bubble-VisualTags $Id $hidden.label).Count} 'Cancelled chooser retained its unshown tag'
    $evidence.restored=@(Bubble-VisualTags $Id $hidden.label)
    $after=Bubble-VisualState $Id
    $evidence|ConvertTo-Json -Depth 10|Set-Content (Join-Path $run "bubble-$Id-chooser-tags.json")
    if($after -ne $before){@{before=($before|ConvertFrom-Json);after=($after|ConvertFrom-Json)}|ConvertTo-Json -Depth 90|Set-Content (Join-Path $run "bubble-$Id-chooser-tag-state-change.json");throw 'Cancelled chooser tag check changed the bubble, workspace or artwork'}
    $checks.Add('chooser-tags-hidden-on-hover-visible-during-choice-and-restored')
}
function Assert-BubbleVisualStates([long]$Id){
    $view=View $Id
    if(!$view.selected -or !$view.enabled -or $view.kind -notin @('dial','brush','medium','controls')){throw 'Visual state check requires an enabled selected tool face'}
    $point=Relative $Id @(-18,-7);$away=@{x=[int]($w.X+20);y=[int]($w.Y+20)}
    $face=Control "tool-bubble-$Id" -Arranged;$prefix=Join-Path $run "bubble-$Id-visual"
    $color=[Drawing.ColorTranslator]::FromHtml((Model).state.palette.text)
    $ink=@{r=[int]$color.R;g=[int]$color.G;b=[int]$color.B}
    $before=Bubble-VisualState $Id;$evidence=[ordered]@{id=$Id;point=$point;ink=$ink;scale=$scale}
    $down=$false;$visualFailure=$null
    try{
        [CapyRowPointer]::Hover($away.x,$away.y)
        $null=Wait-StablePixels {Patch-Pixels $point}
        $baseline=Read-BubbleVisualPatch $point ($prefix+'-idle-patch.png')
        $idleBounds=Bubble-VisualBounds $face
        $null=Screen-Pixels $point.x $point.y ($prefix+'-idle.png')
        [CapyRowPointer]::Hover($point.x,$point.y)
        $null=Wait-StablePixels {Patch-Pixels $point}
        $hover=Read-BubbleVisualPatch $point ($prefix+'-hover-patch.png')
        $evidence.hover=Measure-BubbleVisualOverlay $baseline $hover $ink .07
        $null=Screen-Pixels $point.x $point.y ($prefix+'-hover.png')
        [CapyRowPointer]::Down('mouse',$point.x,$point.y);$down=$true
        Start-Sleep -Milliseconds 100
        $pressed=Read-BubbleVisualPatch $point ($prefix+'-pressed-patch.png')
        $pressedBounds=Bubble-VisualBounds $face
        $evidence.pressed=Measure-BubbleVisualOverlay $baseline $pressed $ink .14
        $evidence.idle_bounds=$idleBounds;$evidence.pressed_bounds=$pressedBounds
        $null=Screen-Pixels $point.x $point.y ($prefix+'-pressed.png')
        $origin=Center $Id
        foreach($cycle in 1..3){
            [CapyRowPointer]::Move(($point.x+[int](20*$scale)),$point.y)
            Wait-Until {[Math]::Abs((Center $Id).x-$origin.x) -gt 8*$scale} 'Held bubble did not move beyond its press slop'
            [CapyRowPointer]::Move($point.x,$point.y)
            Wait-Until {$at=Center $Id;[Math]::Abs($at.x-$origin.x) -lt 2 -and [Math]::Abs($at.y-$origin.y) -lt 2} 'Held bubble did not return to its starting position'
            $null=Wait-StablePixels {Patch-Pixels $point}
            $returned=Read-BubbleVisualPatch $point ($prefix+"-returned-$cycle-patch.png")
            $evidence["returned-$cycle"]=Measure-BubbleVisualOverlay $baseline $returned $ink .07
        }
        $null=Screen-Pixels $point.x $point.y ($prefix+'-returned.png')
        Capture 'face-drag-return-held' -Composed -WithModel
        $heldBounds=Bubble-VisualBounds $face;$evidence.held_bounds=$heldBounds
        for($i=2;$i -lt 4;$i++){if([Math]::Abs($heldBounds[$i]-$idleBounds[$i]*1.04) -gt 1){throw 'Held bubble did not retain its native 104 percent drag size'}}
    }catch{
        $visualFailure=$_;$evidence.failure=$_.ToString();throw
    }finally{
        try{
            if($down){
                try{
                    [CapyRowPointer]::Key([uint32]$review.Id,0x1b)
                    Wait-Until {
                        $bounds=Bubble-VisualBounds $face;$evidence.cancelled_bounds=$bounds
                        foreach($i in 0..3){if([Math]::Abs($bounds[$i]-$idleBounds[$i]) -gt .75){return $false}}
                        $true
                    } 'Escape did not restore idle native bounds before mouse release'
                    $cancelled=Read-BubbleVisualPatch $point ($prefix+'-cancelled-patch.png')
                    $evidence.after_escape_idle=Measure-BubbleVisualOverlay $baseline $cancelled $ink 0
                    $evidence.after_escape_hover=Measure-BubbleVisualOverlay $baseline $cancelled $ink .07
                }finally{[CapyRowPointer]::Cancel()}
            }
        }catch{
            $evidence.cleanup_failure=$_.ToString();if(!$visualFailure){$visualFailure=$_;throw}
        }finally{
            try{[CapyRowPointer]::Hover($away.x,$away.y)}catch{$evidence.hover_cleanup_failure=$_.ToString();if(!$visualFailure){throw}}finally{$evidence|ConvertTo-Json -Depth 10|Set-Content ($prefix+'.json')}
        }
    }
    $null=Wait-StablePixels {Patch-Pixels $point}
    $after=Bubble-VisualState $Id
    if($after -ne $before){@{before=($before|ConvertFrom-Json);after=($after|ConvertFrom-Json)}|ConvertTo-Json -Depth 90|Set-Content ($prefix+'-state-change.json');throw 'Cancelled visual state checks changed the bubble, workspace or artwork'}
    foreach($phase in @('hover','pressed','returned-1','returned-2','returned-3')){
        $measurement=$evidence[$phase]
        if($measurement.maximum_error -gt 4 -or $measurement.mean_change -lt 2){throw "Selected bubble $phase pixels do not match its text-color overlay: $($measurement|ConvertTo-Json -Compress)"}
    }
    for($i=0;$i -lt 4;$i++){if([Math]::Abs($idleBounds[$i]-$pressedBounds[$i]) -gt .75){throw 'Pressing the ordinary bubble face changed its native bounds'}}
    $checks.Add('selected-face-hover-pressed-pixels-and-fixed-size')
    $checks.Add('three-held-drag-returns-retain-hover-fill')
}
function Wait-BubbleVisualLayout{
    $settled=@{last='';same=0}
    Wait-Until {
        $bounds=@(foreach($packet in Packets){$center=Center $packet.view.id $packet;@($packet.view.id,$center.x,$center.y)})|ConvertTo-Json -Compress
        if($bounds -eq $settled.last){$settled.same++}else{$settled.last=$bounds;$settled.same=0}
        $settled.same -ge 5
    } 'Bubble edge scene did not settle'
}
function Bubble-VisualBead([long]$Id,$Bead){
    $point=Relative $Id $Bead.placement.center;$size=$Bead.placement.size*$scale
    $matches=@(foreach($element in $root.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition)){
        $current=$element.Current;$bounds=$current.BoundingRectangle
        if($current.ControlType -ne [System.Windows.Automation.ControlType]::Button -or $current.Name -ne $Bead.label -or $current.IsOffscreen -or $bounds.IsEmpty){continue}
        if([Math]::Abs($bounds.Width-$size) -le 2 -and [Math]::Abs($bounds.Height-$size) -le 2 -and [Math]::Abs($bounds.X+$bounds.Width*.5-$point.x) -le 2 -and [Math]::Abs($bounds.Y+$bounds.Height*.5-$point.y) -le 2){$element}
    })
    if($matches.Count -ne 1){throw "Expected one native bead matching its shared bounds: $($Bead.label)"}
    $matches[0]
}
function Assert-BubbleVisualOrbitReturn($Samples,$Turned){
    $difference={param($a,$b) $maximum=0.;foreach($i in 0..3){$maximum=[Math]::Max($maximum,[Math]::Abs($a[$i]-$b[$i]))};$maximum}
    $changed=-1;$gap=0.;$previous=0.
    for($i=0;$i -lt $Samples.Count;$i++){
        $gap=[Math]::Max($gap,$Samples[$i].ms-$previous);$previous=$Samples[$i].ms
        if((& $difference $Samples[$i].bounds $Turned) -gt 2){$changed=$i;break}
    }
    if($changed -lt 0){throw 'Native orbit return did not acknowledge movement'}
    if($Samples[$changed].ms -ge 220 -or $gap -gt 75){throw 'Native orbit samples did not resolve movement before a 220 ms tween could complete'}
    if($Samples.Count-$changed -lt 3){throw 'Native orbit return needs at least two samples after its first movement'}
    $final=$Samples[$Samples.Count-1].bounds;$firstError=& $difference $Samples[$changed].bounds $final;$maximum=$firstError
    for($i=$changed+1;$i -lt $Samples.Count;$i++){$maximum=[Math]::Max($maximum,(& $difference $Samples[$i].bounds $final))}
    if($maximum -gt 2){throw "Returning inside press slop animated native bead bounds instead of snapping: $maximum physical pixels"}
    @{first_change_ms=$Samples[$changed].ms;maximum_ack_sample_gap_ms=$gap;first_change_final_error_px=$firstError;maximum_post_change_error_px=$maximum;post_change_samples=($Samples.Count-$changed-1);geometry_claim='first observed movement already at final bounds'}
}
function Assert-BubbleVisualBeadTransitions([long]$Id){
    $original=View $Id
    if($original.kind -ne 'color' -or !$original.pill -or !$original.options.palette){throw 'Bead transition check requires the open palette pill'}
    $before=Bubble-VisualState $Id;$paletteName=$original.pill;$layout=Workspace-Key;$resized=$false;$down=$false
    $evidence=[ordered]@{id=$Id;palette=$paletteName;alpha_curve='not compared';orbit=@{};revival=@{}}
    $prefix=Join-Path $run "bubble-$Id-bead-transitions"
    try{
        if($original.options.size -ne 136){
            $at=Center $Id;[CapyRowPointer]::RightClick($at.x,$at.y);Invoke 'Small' -Name;$resized=$true
            Wait-Until {(View $Id).options.size -eq 136} 'Native Small option did not resize the wheel';Escape
        }
        Wait-BubbleVisualLayout;Start-Sleep -Milliseconds 300
        $small=View $Id
        $bead=@($small.beads|Where-Object {$_.placement.bead.kind -eq 'swatch' -and $_.placement.bead.index -eq 6})[0]
        $palette=@((Model).state.color_library.palettes|Where-Object name -eq $paletteName)[0]
        if(!$bead -or $palette.swatches.Count -le @($small.beads|Where-Object {$_.placement.bead.kind -eq 'swatch'}).Count){throw 'Small wheel fixture does not overflow its palette orbit'}
        $peer=Bubble-VisualBead $Id $bead;$from=Relative $Id $bead.placement.center
        $angle=[Math]::Atan2($bead.placement.center[1],$bead.placement.center[0]);$radius=[Math]::Sqrt([Math]::Pow($bead.placement.center[0],2)+[Math]::Pow($bead.placement.center[1],2))
        $turn=Relative $Id @(($radius*[Math]::Cos($angle+.32)),($radius*[Math]::Sin($angle+.32)))
        $back=Relative $Id @(($radius*[Math]::Cos($angle+.04)),($radius*[Math]::Sin($angle+.04)))
        $distance={param($a,$b) [Math]::Sqrt([Math]::Pow($a.x-$b.x,2)+[Math]::Pow($a.y-$b.y,2))/$scale}
        $evidence.orbit=@{from=$from;turned=$turn;returned=$back;outward_dip=(& $distance $from $turn);returned_dip=(& $distance $from $back);initial_bounds=(Bubble-VisualBounds $peer);samples=@()}
        if($evidence.orbit.outward_dip -le 8 -or $evidence.orbit.returned_dip -ge 8){throw 'Orbit fixture does not cross and return inside press slop'}
        $null=Screen-Pixels $from.x $from.y ($prefix+'-orbit-before.png')
        [CapyRowPointer]::Down('mouse',$from.x,$from.y);$down=$true;[CapyRowPointer]::Move($turn.x,$turn.y)
        Wait-Until {
            $moved=@((View $Id).beads|Where-Object {$_.placement.bead.kind -eq 'swatch' -and $_.placement.bead.index -eq 6})[0]
            $moved -and [Math]::Abs($moved.placement.center[0]-$bead.placement.center[0]) -gt 8
        } 'Held palette orbit did not move beyond its original position'
        Start-Sleep -Milliseconds 300
        $evidence.orbit.turned_bounds=Bubble-VisualBounds $peer
        $returnClock=[Diagnostics.Stopwatch]::StartNew();[CapyRowPointer]::Move($back.x,$back.y)
        $samples=[Collections.Generic.List[object]]::new()
        do{$bounds=Bubble-VisualBounds $peer;$samples.Add(@{ms=$returnClock.Elapsed.TotalMilliseconds;bounds=$bounds});Start-Sleep -Milliseconds 10}while($returnClock.ElapsedMilliseconds -lt 300)
        $evidence.orbit.samples=@($samples.ToArray())
        $returned=@((View $Id).beads|Where-Object {$_.placement.bead.kind -eq 'swatch' -and $_.placement.bead.index -eq 6})[0]
        $null=Bubble-VisualBead $Id $returned
        $last=$samples[$samples.Count-1].bounds;$turned=$evidence.orbit.turned_bounds
        if([Math]::Sqrt([Math]::Pow($last[0]-$turned[0],2)+[Math]::Pow($last[1]-$turned[1],2)) -le 8*$scale){throw 'Native bead did not follow the held orbit back inside the starting slop'}
        $evidence.orbit.return_motion=Assert-BubbleVisualOrbitReturn $samples $turned
        $evidence.orbit.returned_shared=$returned
        Capture 'orbit-return-held' -Composed -WithModel
    }finally{
        try{if($down){try{Escape}finally{[CapyRowPointer]::Cancel()};$down=$false}}finally{
            try{if($resized){Restore-Bubble $Id;Wait-Until {(Workspace-Key) -eq $layout} 'Workspace Undo did not restore the original wheel size and layout'}}finally{
                $evidence|ConvertTo-Json -Depth 20|Set-Content ($prefix+'.json')
            }
        }
    }
    Wait-BubbleVisualLayout;Start-Sleep -Milliseconds 300
    if((Bubble-VisualState $Id) -ne $before){throw 'Cancelled orbit and size restoration changed the bubble, colors, artwork or workspace'}
    $model=Model;$palettes=@($model.state.color_library.palettes);$index=[Array]::IndexOf(@($palettes.name),$paletteName)
    $choices=@(foreach($direction in @(-1,1)){
        $neighbor=$palettes[($index+$direction+$palettes.Count)%$palettes.Count]
        $retiring=@((View $Id).beads|Where-Object {$_.placement.bead.kind -eq 'swatch' -and $_.placement.bead.index -ge $neighbor.swatches.Count}|Sort-Object {$_.placement.bead.index} -Descending)[0]
        if($retiring){@{direction=$direction;neighbor=$neighbor;bead=$retiring}}
    })
    if(!$choices.Count){throw 'Palette fixture has no neighboring palette that retires a visible bead'}
    $choice=$choices[0];$bead=$choice.bead;$peer=Bubble-VisualBead $Id $bead;$face=Control "tool-bubble-$Id" -Arranged;$pill=(Packet $Id).pill
    $evidence.revival=@{label=$bead.label;index=$bead.placement.bead.index;neighbor=$choice.neighbor.name;direction=$choice.direction;initial_bounds=(Bubble-VisualBounds $peer);native_retirement=$false;native_revival=$false}
    $point=Relative $Id $bead.placement.center;$null=Screen-Pixels $point.x $point.y ($prefix+'-revival-before.png')
    $stepPoint={param($direction) $bounds=$face.Current.BoundingRectangle;@{x=[int]($bounds.X+$bounds.Width*.5+($pill.x+$(if($direction -lt 0){16}else{$pill.width-16}))*$scale);y=[int]($bounds.Y+$bounds.Height*.5+($pill.y+15)*$scale)}}
    try{
        $next=& $stepPoint $choice.direction;$previous=& $stepPoint (-$choice.direction)
        $observer=[CapyBubbleFoldObserver]::new((State-File),$Id,$peer)
        try{
            $evidence.revival.injected_pair_ms=[CapyRowPointer]::RapidReopen('mouse',$next.x,$next.y,$previous.x,$previous.y,$observer.NativeFoldSignal)
            $evidence.revival.native_retirement=$observer.NativeFoldDetectedTicks -gt 0
            $evidence.revival.native_last_visible_ticks=$observer.NativeLastOpenTicks;$evidence.revival.native_retired_ticks=$observer.NativeFoldDetectedTicks
            $evidence.revival.reverse_down_ticks=[CapyRowPointer]::RapidReopenDownTicks;$evidence.revival.reverse_up_ticks=[CapyRowPointer]::RapidReopenUpTicks
            $evidence.revival.native_to_reverse_up_upper_ms=([CapyRowPointer]::RapidReopenUpTicks-$observer.NativeLastOpenTicks)*1000./[Diagnostics.Stopwatch]::Frequency
            $evidence.revival.retirement_to_reverse_up_ms=([CapyRowPointer]::RapidReopenUpTicks-$observer.NativeFoldDetectedTicks)*1000./[Diagnostics.Stopwatch]::Frequency
            $evidence.revival.observer_samples=$observer.Samples
        }finally{$observer.Dispose()}
        $null=Screen-Pixels $point.x $point.y ($prefix+'-reviving.png')
        if(!$evidence.revival.native_retirement -or $evidence.revival.retirement_to_reverse_up_ms -lt 0 -or $evidence.revival.native_to_reverse_up_upper_ms -ge 220){throw "Palette revival was not exercised within 220 ms of native retirement: $($evidence.revival.native_to_reverse_up_upper_ms) ms upper bound"}
        Wait-Until {(View $Id).pill -eq $paletteName} 'Rapid reverse step did not restore the original palette'
        Wait-Until {try{!$peer.Current.IsOffscreen -and $null -ne [System.Windows.Automation.TreeWalker]::RawViewWalker.GetParent($peer)}catch [System.Windows.Automation.ElementNotAvailableException]{$false}} 'Retiring native bead was replaced instead of revived within its transition'
        $evidence.revival.native_revival=$true
        Settled-Capture 'palette-rapid-revival'
        $restored=@((View $Id).beads|Where-Object {$_.placement.bead.kind -eq 'swatch' -and $_.placement.bead.index -eq $bead.placement.bead.index})[0]
        $null=Bubble-VisualBead $Id $restored;$evidence.revival.final_bounds=Bubble-VisualBounds $peer
    }finally{
        try{if($down){[CapyRowPointer]::Cancel();$down=$false}
            if((View $Id).pill -eq $choice.neighbor.name){$restore=& $stepPoint (-$choice.direction);[CapyRowPointer]::Down('mouse',$restore.x,$restore.y);Start-Sleep -Milliseconds 10;[CapyRowPointer]::Up();Wait-Until {(View $Id).pill -eq $paletteName} 'Palette cleanup did not restore its original selection'}
        }finally{$evidence|ConvertTo-Json -Depth 20|Set-Content ($prefix+'.json')}
    }
    Wait-BubbleVisualLayout
    if((Bubble-VisualState $Id) -ne $before){throw 'Bead transitions changed the bubble, colors, artwork or workspace'}
    $checks.Add('orbit-return-inside-slop-snaps-native-geometry')
    $checks.Add('native-palette-bead-revival-within-220ms-and-exact-state-restoration')
}
function Move-BubbleVisual([long]$Id,[double]$X,[double]$Y){
    $view=View $Id;$center=Center $Id;$target=Screen $X $Y
    $from=if($view.kind -eq 'color'){Relative $Id @((-$view.diameter*.5+3),0)}else{$center}
    Drag $from @{x=[int]($from.x+$target.x-$center.x);y=[int]($from.y+$target.y-$center.y)}
    Wait-BubbleVisualLayout
}
function Set-BubbleVisualDiameter([long]$Id,[double]$Diameter){
    $area=(Model).layout.work_area
    Move-BubbleVisual $Id ($area.x+$area.width*.5) ($area.y+$area.height*.5)
    $size=(View $Id).diameter;$center=Center $Id;$spread=[int][Math]::Round(($size*.5-4)*$scale)
    $factor=if($Diameter -eq 128){($Diameter-8)/$size}elseif($Diameter -eq 352){($Diameter+8)/$size}else{$Diameter/$size}
    Touch-Pair ($center.x-$spread) $center.y ($center.x+$spread) $center.y 0 0 $factor 16 "visual-size-$Diameter"
    Wait-Until {[Math]::Abs((View $Id).diameter-$Diameter) -le $(if($Diameter -in @(128,352)){.01}else{2})} "Wheel did not reach its requested $Diameter DIP size"
    Wait-BubbleVisualLayout
}
function Assert-BubbleVisualGeometry([long]$Id,[string]$Stage){
    $view=View $Id;$packet=Packet $Id;$center=Center $Id;$area=(Model).layout.work_area
    $left=$w.X+$area.x*$scale;$top=$w.Y+$area.y*$scale;$right=$left+$area.width*$scale;$bottom=$top+$area.height*$scale
    $regions=[Collections.Generic.List[object]]::new()
    $faceSize=$view.face
    if($packet.color){$b=$packet.color.bounds;$faceSize=@($b.width,$b.height);$regions.Add(@{kind='face';x=$center.x+$b.x*$scale;y=$center.y+$b.y*$scale;width=$b.width*$scale;height=$b.height*$scale})}
    else{$regions.Add(@{kind='face';x=$center.x-$view.face[0]*$scale*.5;y=$center.y-$view.face[1]*$scale*.5;width=$view.face[0]*$scale;height=$view.face[1]*$scale})}
    foreach($bead in @($view.beads)){
        $p=$bead.placement;if($p.alpha -le 0){continue}
        $regions.Add(@{kind=('bead-'+$p.bead.kind);x=$center.x+($p.center[0]-$p.size*.5)*$scale;y=$center.y+($p.center[1]-$p.size*.5)*$scale;width=$p.size*$scale;height=$p.size*$scale})
    }
    foreach($item in @($view.chooser)){
        $size=if($item.kind -eq 'brush'){@(152,48)}else{@(44,44)}
        $regions.Add(@{kind=('chooser-'+$item.label);x=$center.x+($item.center[0]-$size[0]*.5)*$scale;y=$center.y+($item.center[1]-$size[1]*.5)*$scale;width=$size[0]*$scale;height=$size[1]*$scale})
    }
    if($view.pill){$pill=$packet.pill;$regions.Add(@{kind='palette-pill';x=$center.x+$pill.x*$scale;y=$center.y+$pill.y*$scale;width=$pill.width*$scale;height=$pill.height*$scale})}
    $face=Bubble-VisualBounds (Control "tool-bubble-$Id" -Arranged)
    $evidence=@{stage=$Stage;id=$Id;diameter=$view.diameter;work_area=@($left,$top,$right,$bottom);shared_regions=@($regions.ToArray());native_face=$face}
    if($view.kind -eq 'color'){
        $image=Bubble-VisualBounds (Control "bubble-wheel-$Id" -Arranged)
        $evidence.native_wheel=$image;$expected=($view.diameter-14)*$scale
        for($i=2;$i -lt 4;$i++){if([Math]::Abs($image[$i]-$expected) -gt 2){throw 'Native wheel image does not preserve its shared 7 DIP rim'}}
        if([Math]::Abs($image[0]+$image[2]*.5-$center.x) -gt 2 -or [Math]::Abs($image[1]+$image[3]*.5-$center.y) -gt 2){throw 'Native wheel image shifted away from its face center'}
    }
    if($HdrOnly){$hdrEvidence.Add(@{stage=$Stage;geometry=$evidence})}else{$evidence|ConvertTo-Json -Depth 12 -Compress|Add-Content (Join-Path $run 'bubble-edge-geometry.jsonl')}
    for($i=2;$i -lt 4;$i++){if([Math]::Abs($face[$i]-$faceSize[$i-2]*$scale) -gt 2){throw 'Native bubble face size differs from its shared presentation'}}
    foreach($region in $regions){
        if($region.x -lt $left-2 -or $region.y -lt $top-2 -or $region.x+$region.width -gt $right+2 -or $region.y+$region.height -gt $bottom+2){throw "Bubble $Id $($region.kind) exceeds its work area during $Stage"}
    }
    return $evidence
}
function Set-BubbleVisualTransparency([ValidateRange(0,3)][int]$Index){
    $types=[System.Windows.Automation.ControlType];$modes=@('off','low','medium','high')
    (Control 'drawing-canvas').SetFocus()
    [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),[uint16]0xBC)
    $dialog=Control 'Preferences' -Name -Type $types::Window
    $circle=Control "preference-transparency-$Index" -Within $dialog
    if($circle.Current.Name -ne @('Off','Low','Medium','High')[$Index]){throw 'Transparency preference has the wrong accessible name'}
    Invoke "preference-transparency-$Index" -Within $dialog
    Wait-Until {$state=(Model).state;$state.settings.transparency -eq $modes[$Index] -and $state.palette.glass.transparency -eq $modes[$Index] -and $circle.Current.ItemStatus -eq 'Selected'} 'Native transparency preference did not update its shared setting'
    $close=$dialog.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.AndCondition]::new(
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'CloseButton'),
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,$types::Button)))
    $close.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    Wait-Until {!(Find 'Preferences' -Name -Type $types::Window)} 'Native transparency Preferences did not close'
    Wait-BubbleVisualLayout
}
function Open-BubbleVisualChooser([long]$Id){
    for($i=0;$i -lt 4 -and !@((View $Id).chooser).Count;$i++){Tap (Center $Id);Start-Sleep -Milliseconds 250}
    if(!@((View $Id).chooser).Count){throw 'Visual scene dial did not open its chooser'}
    Wait-BubbleVisualLayout
}
function Assert-BubbleVisualEdges([long]$Dial,[long]$Color){
    if((View $Color).kind -ne 'color'){throw 'Visual edge matrix requires a color wheel'}
    $model=Model;$revision=$model.state.document_file.revision;$colors=$model.state.colors|ConvertTo-Json -Depth 30 -Compress
    foreach($diameter in @(128,216,352)){
        Set-BubbleVisualDiameter $Color $diameter
        foreach($corner in @(@{name='top-left';x=0;y=0},@{name='top-right';x=1;y=0},@{name='bottom-right';x=1;y=1},@{name='bottom-left';x=0;y=1})){
            $area=(Model).layout.work_area;$radius=(View $Color).diameter*.5
            $x=$area.x+$(if($corner.x){$area.width-$radius-12}else{$radius+12})
            $y=$area.y+$(if($corner.y){$area.height-$radius-12}else{$radius+12})
            Move-BubbleVisual $Color $x $y
            $palette=@((View $Color).beads|Where-Object {$_.placement.bead.kind -eq 'palettes'})[0]
            if($palette -and !(View $Color).pill){Tap (Relative $Color $palette.placement.center);Wait-Until {(View $Color).pill} 'Palette bead did not reopen at the work-area corner';Wait-BubbleVisualLayout}
            $stage="wheel-$diameter-$($corner.name)"
            $null=Assert-BubbleVisualGeometry $Color $stage
            Settled-Capture $stage
        }
    }
    $area=(Model).layout.work_area
    Move-BubbleVisual $Color ($area.x+$area.width*.5) ($area.y+$area.height*.55)
    $center=Center $Color
    Move-BubbleVisual $Dial (($center.x-$w.X)/$scale+120) (($center.y-$w.Y)/$scale)
    Open-BubbleVisualChooser $Dial
    $null=Assert-BubbleVisualGeometry $Dial 'crowded-chooser';$null=Assert-BubbleVisualGeometry $Color 'crowded-wheel'
    $dialCenter=Center $Dial;$colorCenter=Center $Color
    $distance=[Math]::Sqrt([Math]::Pow($dialCenter.x-$colorCenter.x,2)+[Math]::Pow($dialCenter.y-$colorCenter.y,2))
    if($distance -lt ((View $Dial).face[0]+(View $Color).face[0])*$scale*.5-2){throw 'Crowded native dial and wheel faces overlap after settling'}
    Settled-Capture 'crowded-chooser-and-wheel'
    $modes=@('off','low','medium','high');$original=(Model).state.palette.glass.transparency
    try{
        foreach($index in 0..3){
            if(@((View $Dial).chooser).Count){Tap (Center $Dial);Wait-Until {!@((View $Dial).chooser).Count} 'Chooser did not close before changing transparency'}
            Set-BubbleVisualTransparency $index
            Settled-Capture "transparency-$($modes[$index])-all"
            Open-BubbleVisualChooser $Dial
            $null=Assert-BubbleVisualGeometry $Dial "transparency-$($modes[$index])-chooser"
            Settled-Capture "transparency-$($modes[$index])-chooser"
            Escape
        }
    }finally{
        if((Model).state.palette.glass.transparency -ne $original){Set-BubbleVisualTransparency ([Array]::IndexOf($modes,$original))}
    }
    $after=Model
    if($after.state.document_file.revision -ne $revision -or ($after.state.colors|ConvertTo-Json -Depth 30 -Compress) -ne $colors){throw 'Native bubble edge matrix changed artwork or selected colors'}
    $checks.Add('wheel-minimum-medium-maximum-four-corners-and-crowded-chooser')
    $checks.Add('all-transparency-levels-with-live-chooser')
}
